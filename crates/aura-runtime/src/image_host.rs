//! The host part of the `image` module.
//!
//! `std/image.au` holds an image as a width, a height, a channel count and
//! a `list[uint8]` of samples. Every function here takes one large value,
//! the bytes of a file, the samples of an image, or an `Array`, and then
//! plain integers, so the native entry reads the large value in place and
//! copies nothing on the way in.

use super::*;
use crate::image_codec::{self, Filter, Format, ImageError};

fn image_error(name: &str, message: impl AsRef<str>) -> Diagnostic {
    Diagnostic::coded(
        "AU4001",
        format!("`{}` {}", name.replace("::", "."), message.as_ref()),
    )
}

fn int(value: i64) -> Value {
    Value::Int(IntegerValue::from_i64(value))
}

fn int64_type() -> Type {
    Type::named("int64")
}

fn bytes_type() -> Type {
    Type::Named("list".to_string(), vec![Type::named("uint8")])
}

fn bytes_value(bytes: Vec<u8>) -> Value {
    Value::Vec(VecValue {
        element_type: SharedType::named("uint8"),
        elements: ListElements::from_uint8(bytes),
        str_view: Default::default(),
    })
}

/// An integer argument of an `image` host call.
pub fn image_number(value: &Value, name: &str, index: usize) -> Result<i64> {
    match value {
        Value::Int(number) => number
            .as_i128()
            .and_then(|number| i64::try_from(number).ok()),
        _ => None,
    }
    .ok_or_else(|| {
        image_error(
            name,
            format!("expects argument {} to be `int64`", index + 1),
        )
    })
}

/// The bytes of a `list[uint8]` argument, borrowed when the list keeps
/// them unboxed.
fn bytes_of<'a>(value: &'a Value, name: &str) -> Result<Cow<'a, [u8]>> {
    let elements = runtime_uint8_elements(value, &name.replace("::", "."))?;
    match elements.as_uint8() {
        Some(bytes) => Ok(Cow::Borrowed(bytes)),
        None => host_bytes_from_runtime_elements(elements).map(Cow::Owned),
    }
}

/// A side or a count the Aura source passed, which must be at least
/// `least`.
fn size(numbers: &[i64], index: usize, least: i64, name: &str, what: &str) -> Result<usize> {
    let value = numbers[index];
    if value < least {
        return Err(image_error(
            name,
            format!("requires {what} of at least {least}, found `{value}`"),
        ));
    }
    Ok(value as usize)
}

/// The width, height and channel count at `numbers[at..at + 3]`, checked
/// against the number of samples.
fn layout(numbers: &[i64], at: usize, samples: usize, name: &str) -> Result<(usize, usize, usize)> {
    let width = size(numbers, at, 1, name, "a width")?;
    let height = size(numbers, at + 1, 1, name, "a height")?;
    let channels = numbers[at + 2];
    if !(1..=4).contains(&channels) {
        return Err(image_error(
            name,
            format!("requires 1 to 4 channels, found `{channels}`"),
        ));
    }
    let channels = channels as usize;
    let expected = width
        .checked_mul(height)
        .and_then(|pixels| pixels.checked_mul(channels));
    if expected != Some(samples) {
        return Err(image_error(
            name,
            format!(
                "requires `pixels` to hold width * height * channels = {width} * {height} * {channels} bytes, found {samples}"
            ),
        ));
    }
    Ok((width, height, channels))
}

/// What a failed decode or encode hands the Aura source: the error's
/// number, its limit when it has one, and its text.
fn failure(error: &ImageError) -> (i64, i64, String) {
    let limit = match error {
        ImageError::TooLarge { limit_pixels } => *limit_pixels as i64,
        _ => 0,
    };
    (error.code(), limit, error.to_string())
}

fn tuple(element_types: Vec<Type>, elements: Vec<Value>) -> Value {
    Value::Tuple(TupleValue {
        element_types,
        elements,
    })
}

/// `(status, width, height, channels, pixels, message)`: status 0 and the
/// image, or an error number, with the pixel limit in `width` when the
/// image was over it.
fn decoded(result: std::result::Result<image_codec::Pixels, ImageError>) -> Value {
    let (status, width, height, channels, data, message) = match result {
        Ok(pixels) => (
            0,
            pixels.width as i64,
            pixels.height as i64,
            pixels.channels as i64,
            pixels.data,
            String::new(),
        ),
        Err(error) => {
            let (code, limit, message) = failure(&error);
            (code, limit, 0, 0, Vec::new(), message)
        }
    };
    tuple(
        vec![
            int64_type(),
            int64_type(),
            int64_type(),
            int64_type(),
            bytes_type(),
            Type::named("str"),
        ],
        vec![
            int(status),
            int(width),
            int(height),
            int(channels),
            bytes_value(data),
            Value::string(message),
        ],
    )
}

fn expect_numbers(name: &str, numbers: &[i64], expected: usize) -> Result<()> {
    if numbers.len() != expected {
        return Err(image_error(
            name,
            format!(
                "expects {} arguments, found {}",
                expected + 1,
                numbers.len() + 1
            ),
        ));
    }
    Ok(())
}

/// Runs the `image` host function `name` on its large argument and its
/// integers.
pub fn evaluate_image_builtin(name: &str, data: &Value, numbers: &[i64]) -> Result<Value> {
    match name {
        // (format, width, height, channels, message), or minus the error
        // number as the format.
        "image::__aura_info" => {
            expect_numbers(name, numbers, 0)?;
            let (format, width, height, channels, message) =
                match image_codec::header(&bytes_of(data, name)?) {
                    Ok(header) => (
                        header.format.code(),
                        header.width as i64,
                        header.height as i64,
                        header.channels as i64,
                        String::new(),
                    ),
                    Err(error) => (-error.code(), 0, 0, 0, error.to_string()),
                };
            Ok(tuple(
                vec![
                    int64_type(),
                    int64_type(),
                    int64_type(),
                    int64_type(),
                    Type::named("str"),
                ],
                vec![
                    int(format),
                    int(width),
                    int(height),
                    int(channels),
                    Value::string(message),
                ],
            ))
        }
        "image::__aura_decode" => {
            expect_numbers(name, numbers, 1)?;
            let limit = numbers[0];
            if !(1..=image_codec::MAX_PIXEL_LIMIT).contains(&limit) {
                return Err(image_error(
                    name,
                    format!(
                        "requires a pixel limit from 1 to {}, found `{limit}`",
                        image_codec::MAX_PIXEL_LIMIT
                    ),
                ));
            }
            Ok(decoded(image_codec::decode(
                &bytes_of(data, name)?,
                limit as usize,
            )))
        }
        // (status, bytes, message)
        "image::__aura_encode" => {
            expect_numbers(name, numbers, 5)?;
            let pixels = bytes_of(data, name)?;
            let (width, height, channels) = layout(numbers, 1, pixels.len(), name)?;
            let option = numbers[4];
            let result = match Format::from_code(numbers[0]) {
                Some(Format::Png) => {
                    if !(0..=9).contains(&option) {
                        return Err(image_error(
                            name,
                            format!("requires a PNG level from 0 to 9, found `{option}`"),
                        ));
                    }
                    image_codec::encode_png(width, height, channels, &pixels, option as u32)
                }
                Some(Format::Jpeg) => {
                    if !(1..=100).contains(&option) {
                        return Err(image_error(
                            name,
                            format!("requires a JPEG quality from 1 to 100, found `{option}`"),
                        ));
                    }
                    image_codec::encode_jpeg(width, height, channels, &pixels, option as u8)
                }
                Some(Format::Pnm) => image_codec::encode_pnm(width, height, channels, &pixels),
                None => {
                    return Err(image_error(
                        name,
                        format!("knows no format numbered `{}`", numbers[0]),
                    ))
                }
            };
            let (status, bytes, message) = match result {
                Ok(bytes) => (0, bytes, String::new()),
                Err(error) => (error.code(), Vec::new(), error.to_string()),
            };
            Ok(tuple(
                vec![int64_type(), bytes_type(), Type::named("str")],
                vec![int(status), bytes_value(bytes), Value::string(message)],
            ))
        }
        "image::__aura_convert" => {
            expect_numbers(name, numbers, 4)?;
            let pixels = bytes_of(data, name)?;
            let (_, _, channels) = layout(numbers, 0, pixels.len(), name)?;
            let target = numbers[3];
            if !(1..=4).contains(&target) {
                return Err(image_error(
                    name,
                    format!("requires 1 to 4 channels, found `{target}`"),
                ));
            }
            Ok(bytes_value(image_codec::convert_channels(
                &pixels,
                channels,
                target as usize,
            )))
        }
        "image::__aura_resize" => {
            expect_numbers(name, numbers, 6)?;
            let pixels = bytes_of(data, name)?;
            let (width, height, channels) = layout(numbers, 0, pixels.len(), name)?;
            let new_width = size(numbers, 3, 1, name, "a width")?;
            let new_height = size(numbers, 4, 1, name, "a height")?;
            let fits = new_width
                .checked_mul(new_height)
                .is_some_and(|pixels| pixels as i64 <= image_codec::MAX_PIXEL_LIMIT);
            if !fits {
                return Err(image_error(
                    name,
                    format!(
                        "requires at most {} pixels, found {new_width} by {new_height}",
                        image_codec::MAX_PIXEL_LIMIT
                    ),
                ));
            }
            let filter = Filter::from_code(numbers[5]).ok_or_else(|| {
                image_error(name, format!("knows no filter numbered `{}`", numbers[5]))
            })?;
            Ok(bytes_value(image_codec::resize(
                &pixels, width, height, channels, new_width, new_height, filter,
            )))
        }
        "image::__aura_crop" => {
            expect_numbers(name, numbers, 7)?;
            let pixels = bytes_of(data, name)?;
            let (width, height, channels) = layout(numbers, 0, pixels.len(), name)?;
            let left = size(numbers, 3, 0, name, "a left edge")?;
            let top = size(numbers, 4, 0, name, "a top edge")?;
            let crop_width = size(numbers, 5, 1, name, "a width")?;
            let crop_height = size(numbers, 6, 1, name, "a height")?;
            let inside = left
                .checked_add(crop_width)
                .is_some_and(|right| right <= width)
                && top
                    .checked_add(crop_height)
                    .is_some_and(|bottom| bottom <= height);
            if !inside {
                return Err(image_error(
                    name,
                    format!(
                        "requires the part to lie inside the {width} by {height} image, found {crop_width} by {crop_height} at ({left}, {top})"
                    ),
                ));
            }
            Ok(bytes_value(image_codec::crop(
                &pixels,
                width,
                channels,
                left,
                top,
                crop_width,
                crop_height,
            )))
        }
        "image::__aura_flip" => {
            expect_numbers(name, numbers, 4)?;
            let pixels = bytes_of(data, name)?;
            let (width, _, channels) = layout(numbers, 0, pixels.len(), name)?;
            Ok(bytes_value(if numbers[3] != 0 {
                image_codec::flip_vertical(&pixels, width, channels)
            } else {
                image_codec::flip_horizontal(&pixels, width, channels)
            }))
        }
        "image::__aura_to_array" => {
            expect_numbers(name, numbers, 4)?;
            let pixels = bytes_of(data, name)?;
            let (width, height, channels) = layout(numbers, 0, pixels.len(), name)?;
            let planes = numbers[3] != 0;
            let values = image_codec::to_floats(&pixels, channels, planes);
            let shape: Box<[usize]> = if planes {
                [channels, height, width].into()
            } else {
                [height, width, channels].into()
            };
            ArrayValue::new(
                shape,
                ArrayStorage::Float32(values.into_boxed_slice().into()),
            )
            .map(Value::Array)
        }
        // (width, height, channels, pixels)
        "image::__aura_from_array" => {
            expect_numbers(name, numbers, 1)?;
            let Value::Array(array) = data else {
                return Err(image_error(name, "expects an `Array[float32]`"));
            };
            let planes = numbers[0] != 0;
            let (height, width, channels) = match (&*array.shape, planes) {
                ([height, width], _) => (*height, *width, 1),
                ([height, width, channels], false) => (*height, *width, *channels),
                ([channels, height, width], true) => (*height, *width, *channels),
                (shape, _) => {
                    return Err(image_error(
                        name,
                        format!("requires an Array of two or three axes, found shape {shape:?}"),
                    ))
                }
            };
            if !(1..=4).contains(&channels) {
                return Err(image_error(
                    name,
                    format!(
                        "requires 1 to 4 channels, found {channels} in shape {:?}",
                        array.shape
                    ),
                ));
            }
            if width == 0 || height == 0 {
                return Err(image_error(
                    name,
                    format!("requires at least one pixel, found shape {:?}", array.shape),
                ));
            }
            let elements = array.elements()?;
            let ArrayStorage::Float32(values) = &*elements else {
                return Err(image_error(name, "expects an `Array[float32]`"));
            };
            Ok(tuple(
                vec![int64_type(), int64_type(), int64_type(), bytes_type()],
                vec![
                    int(width as i64),
                    int(height as i64),
                    int(channels as i64),
                    bytes_value(image_codec::from_floats(values, channels, planes)),
                ],
            ))
        }
        _ => Err(image_error(name, "is not an image function")),
    }
}

/// The interpreter's entry for an `image` builtin: the large argument,
/// then the integers.
fn host_image(name: &str, args: Vec<Value>) -> Result<Value> {
    let Some((data, rest)) = args.split_first() else {
        return Err(image_error(name, "expects at least one argument"));
    };
    let numbers = rest
        .iter()
        .enumerate()
        .map(|(index, value)| image_number(value, name, index + 1))
        .collect::<Result<Vec<_>>>()?;
    evaluate_image_builtin(name, data, &numbers)
}

macro_rules! image_host_functions {
    ($($function:ident $name:literal;)*) => {
        $(
            pub fn $function(args: Vec<Value>, _program_args: Option<&[String]>) -> Result<Value> {
                host_image($name, args)
            }
        )*

        /// The interpreter's entry for an `image` builtin.
        pub fn image_host_function(name: &str) -> Option<host_builtins::HostBuiltinFn> {
            match name {
                $($name => Some($function),)*
                _ => None,
            }
        }

        /// Every `image` builtin's name.
        pub const IMAGE_HOST_FUNCTIONS: &[&str] = &[$($name),*];
    };
}

image_host_functions! {
    host_image_info "image::__aura_info";
    host_image_decode "image::__aura_decode";
    host_image_encode "image::__aura_encode";
    host_image_convert "image::__aura_convert";
    host_image_resize "image::__aura_resize";
    host_image_crop "image::__aura_crop";
    host_image_flip "image::__aura_flip";
    host_image_to_array "image::__aura_to_array";
    host_image_from_array "image::__aura_from_array";
}

#[cfg(test)]
#[path = "image_host_tests.rs"]
mod tests;
