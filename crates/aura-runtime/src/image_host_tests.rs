use super::*;

fn bytes(values: &[u8]) -> Value {
    bytes_value(values.to_vec())
}

fn ints(values: &[i64]) -> Vec<Value> {
    values.iter().map(|value| int(*value)).collect()
}

fn call(name: &str, data: Value, numbers: &[i64]) -> Result<Value> {
    let mut args = vec![data];
    args.extend(ints(numbers));
    image_host_function(name).expect("the function exists")(args, None)
}

fn elements(value: Value) -> Vec<Value> {
    match value {
        Value::Tuple(tuple) => {
            assert_eq!(tuple.element_types.len(), tuple.elements.len());
            tuple.elements
        }
        other => panic!("expected a tuple, found {other:?}"),
    }
}

fn number(value: &Value) -> i64 {
    image_number(value, "image::test", 0).expect("an integer")
}

fn byte_list(value: &Value) -> Vec<u8> {
    bytes_of(value, "image::test")
        .expect("a byte list")
        .into_owned()
}

fn text(value: &Value) -> String {
    match value {
        Value::String(text) => text.as_str().to_string(),
        other => panic!("expected text, found {other:?}"),
    }
}

fn message(result: Result<Value>) -> String {
    result.expect_err("the call is refused").message
}

/// 3 by 2 RGB.
const PIXELS: [u8; 18] = [
    0, 10, 20, 30, 40, 50, 60, 70, 80, //
    90, 100, 110, 120, 130, 140, 150, 160, 170,
];

#[test]
fn every_format_encodes_and_decodes_through_the_host_functions() {
    for (format, lossless, option) in [(0, true, 6), (1, false, 100), (2, true, 0)] {
        let encoded = elements(
            call(
                "image::__aura_encode",
                bytes(&PIXELS),
                &[format, 3, 2, 3, option],
            )
            .expect("encodes"),
        );
        assert_eq!(number(&encoded[0]), 0);
        assert_eq!(text(&encoded[2]), "");
        let file = byte_list(&encoded[1]);

        let info = elements(call("image::__aura_info", bytes(&file), &[]).expect("reads"));
        let found: Vec<i64> = info[..4].iter().map(number).collect();
        assert_eq!(found, [format, 3, 2, 3]);
        assert_eq!(text(&info[4]), "");

        let decoded =
            elements(call("image::__aura_decode", bytes(&file), &[1 << 20]).expect("decodes"));
        let found: Vec<i64> = decoded[..4].iter().map(number).collect();
        assert_eq!(found, [0, 3, 2, 3]);
        let pixels = byte_list(&decoded[4]);
        assert_eq!(pixels.len(), PIXELS.len());
        if lossless {
            assert_eq!(pixels, PIXELS);
        }
    }
}

#[test]
fn decode_failures_come_back_as_status_numbers() {
    let invalid = elements(call("image::__aura_decode", bytes(b""), &[100]).expect("returns"));
    assert_eq!(number(&invalid[0]), 1);
    assert_eq!(text(&invalid[5]), "the input is empty");
    assert!(byte_list(&invalid[4]).is_empty());

    let unknown =
        elements(call("image::__aura_decode", bytes(b"GIF89a"), &[100]).expect("returns"));
    assert_eq!(number(&unknown[0]), 2);

    let large = elements(
        call("image::__aura_decode", bytes(b"P5\n20 20\n255\n"), &[399]).expect("returns"),
    );
    assert_eq!(number(&large[0]), 3);
    assert_eq!(number(&large[1]), 399);
    assert_eq!(text(&large[5]), "the image has more than 399 pixels");

    let info = elements(call("image::__aura_info", bytes(b"GIF89a"), &[]).expect("returns"));
    assert_eq!(number(&info[0]), -2);
    assert!(text(&info[4]).contains("not a PNG"));

    for limit in [0, -4, image_codec::MAX_PIXEL_LIMIT + 1] {
        assert!(
            message(call("image::__aura_decode", bytes(b"P5"), &[limit]))
                .contains("requires a pixel limit from 1 to")
        );
    }
}

#[test]
fn encode_failures_and_bad_options_are_told_apart() {
    let rgba = [0u8; 8];
    for format in [1, 2] {
        let refused = elements(
            call("image::__aura_encode", bytes(&rgba), &[format, 2, 1, 4, 90]).expect("returns"),
        );
        assert_eq!(number(&refused[0]), 2);
        assert!(text(&refused[2]).contains("no alpha channel"));
        assert!(byte_list(&refused[1]).is_empty());
    }
    let encode = |numbers: &[i64]| message(call("image::__aura_encode", bytes(&PIXELS), numbers));
    assert!(encode(&[0, 3, 2, 3, 10]).contains("PNG level from 0 to 9"));
    assert!(encode(&[0, 3, 2, 3, -1]).contains("PNG level from 0 to 9"));
    assert!(encode(&[1, 3, 2, 3, 0]).contains("JPEG quality from 1 to 100"));
    assert!(encode(&[1, 3, 2, 3, 101]).contains("JPEG quality from 1 to 100"));
    assert!(encode(&[7, 3, 2, 3, 0]).contains("knows no format numbered `7`"));
}

#[test]
fn a_layout_that_does_not_match_the_samples_is_refused() {
    let convert = |numbers: &[i64]| message(call("image::__aura_convert", bytes(&PIXELS), numbers));
    assert_eq!(
        convert(&[3, 3, 3, 1]),
        "`image.__aura_convert` requires `pixels` to hold width * height * channels = 3 * 3 * 3 bytes, found 18"
    );
    assert!(convert(&[0, 2, 3, 1]).contains("a width of at least 1, found `0`"));
    assert!(convert(&[3, -2, 3, 1]).contains("a height of at least 1"));
    assert!(convert(&[3, 2, 5, 1]).contains("1 to 4 channels, found `5`"));
    assert!(convert(&[3, 2, 0, 1]).contains("1 to 4 channels, found `0`"));
    assert!(convert(&[3, 2, 3, 9]).contains("1 to 4 channels, found `9`"));
    assert!(convert(&[i64::MAX, i64::MAX, 3, 1]).contains("found 18"));
    assert!(convert(&[3, 2, 3]).contains("expects 5 arguments, found 4"));
    assert!(message(call(
        "image::__aura_convert",
        Value::Bool(true),
        &[3, 2, 3, 1]
    ))
    .contains("list[uint8]"));
}

#[test]
fn the_pixel_operations_return_new_samples() {
    let gray = call("image::__aura_convert", bytes(&PIXELS), &[3, 2, 3, 1]).expect("converts");
    assert_eq!(byte_list(&gray), [8, 38, 68, 98, 128, 158]);

    let wide = call("image::__aura_resize", bytes(&PIXELS), &[3, 2, 3, 6, 2, 0]).expect("resizes");
    assert_eq!(byte_list(&wide).len(), 36);
    assert_eq!(&byte_list(&wide)[..6], [0, 10, 20, 0, 10, 20]);
    for filter in [1, 2] {
        let small = call(
            "image::__aura_resize",
            bytes(&PIXELS),
            &[3, 2, 3, 1, 1, filter],
        )
        .expect("resizes");
        assert_eq!(byte_list(&small).len(), 3);
    }

    let part = call("image::__aura_crop", bytes(&PIXELS), &[3, 2, 3, 1, 1, 2, 1]).expect("crops");
    assert_eq!(byte_list(&part), [120, 130, 140, 150, 160, 170]);

    let mirrored = call("image::__aura_flip", bytes(&PIXELS), &[3, 2, 3, 0]).expect("flips");
    assert_eq!(&byte_list(&mirrored)[..3], [60, 70, 80]);
    let turned = call("image::__aura_flip", bytes(&PIXELS), &[3, 2, 3, 1]).expect("flips");
    assert_eq!(&byte_list(&turned)[..3], [90, 100, 110]);
}

#[test]
fn resize_and_crop_check_their_targets() {
    let resize = |numbers: &[i64]| message(call("image::__aura_resize", bytes(&PIXELS), numbers));
    assert!(resize(&[3, 2, 3, 0, 4, 1]).contains("a width of at least 1"));
    assert!(resize(&[3, 2, 3, 4, 0, 1]).contains("a height of at least 1"));
    assert!(resize(&[3, 2, 3, 4, 4, 3]).contains("knows no filter numbered `3`"));
    assert!(resize(&[3, 2, 3, 1 << 20, 1 << 20, 1]).contains("requires at most"));
    assert!(resize(&[3, 2, 3, i64::MAX, i64::MAX, 1]).contains("requires at most"));
    // The product fits a usize but not an int64.
    assert!(resize(&[3, 2, 3, 1 << 32, (1 << 31) + 1, 1]).contains("requires at most"));

    let crop = |numbers: &[i64]| message(call("image::__aura_crop", bytes(&PIXELS), numbers));
    assert!(crop(&[3, 2, 3, -1, 0, 1, 1]).contains("a left edge of at least 0"));
    assert!(crop(&[3, 2, 3, 0, -1, 1, 1]).contains("a top edge of at least 0"));
    assert!(crop(&[3, 2, 3, 0, 0, 0, 1]).contains("a width of at least 1"));
    assert!(crop(&[3, 2, 3, 0, 0, 1, 0]).contains("a height of at least 1"));
    assert_eq!(
        crop(&[3, 2, 3, 2, 0, 2, 1]),
        "`image.__aura_crop` requires the part to lie inside the 3 by 2 image, found 2 by 1 at (2, 0)"
    );
    assert!(crop(&[3, 2, 3, 0, 1, 1, 2]).contains("lie inside"));
    assert!(crop(&[3, 2, 3, i64::MAX, 0, i64::MAX, 1]).contains("lie inside"));
    assert!(crop(&[3, 2, 3, 0, i64::MAX, 1, i64::MAX]).contains("lie inside"));
}

fn float_array(values: &[f32], shape: &[usize]) -> Value {
    Value::Array(
        ArrayValue::new(
            shape.into(),
            ArrayStorage::Float32(values.to_vec().into_boxed_slice().into()),
        )
        .expect("the array is well formed"),
    )
}

fn array_parts(value: Value) -> (Vec<usize>, Vec<f32>) {
    let Value::Array(array) = value else {
        panic!("expected an Array");
    };
    let ArrayStorage::Float32(values) = array.storage() else {
        panic!("expected float32");
    };
    (array.shape.to_vec(), values.to_vec())
}

#[test]
fn arrays_carry_the_samples_in_both_layouts() {
    let (shape, values) = array_parts(
        call("image::__aura_to_array", bytes(&PIXELS), &[3, 2, 3, 0]).expect("converts"),
    );
    assert_eq!(shape, [2, 3, 3]);
    assert_eq!(values[1], 10.0 / 255.0);
    let back = elements(
        call(
            "image::__aura_from_array",
            float_array(&values, &shape),
            &[0],
        )
        .expect("converts"),
    );
    let layout: Vec<i64> = back[..3].iter().map(number).collect();
    assert_eq!(layout, [3, 2, 3]);
    assert_eq!(byte_list(&back[3]), PIXELS);

    let (shape, values) = array_parts(
        call("image::__aura_to_array", bytes(&PIXELS), &[3, 2, 3, 1]).expect("converts"),
    );
    assert_eq!(shape, [3, 2, 3]);
    assert_eq!(values[1], 30.0 / 255.0);
    let back = elements(
        call(
            "image::__aura_from_array",
            float_array(&values, &shape),
            &[1],
        )
        .expect("converts"),
    );
    let layout: Vec<i64> = back[..3].iter().map(number).collect();
    assert_eq!(layout, [3, 2, 3]);
    assert_eq!(byte_list(&back[3]), PIXELS);

    // Two axes are one gray channel in either layout.
    for planes in [0, 1] {
        let gray = elements(
            call(
                "image::__aura_from_array",
                float_array(&[0.0, 1.0, 0.5, 2.0, -1.0, 0.25], &[2, 3]),
                &[planes],
            )
            .expect("converts"),
        );
        let layout: Vec<i64> = gray[..3].iter().map(number).collect();
        assert_eq!(layout, [3, 2, 1]);
        assert_eq!(byte_list(&gray[3]), [0, 255, 128, 255, 0, 64]);
    }
}

#[test]
fn from_array_reads_a_view_in_its_own_order() {
    let Value::Array(array) = float_array(&[0.0, 0.2, 0.4, 0.6, 0.8, 1.0], &[2, 3]) else {
        unreachable!()
    };
    let turned = array.transpose().expect("transposes");
    let image =
        elements(call("image::__aura_from_array", Value::Array(turned), &[0]).expect("converts"));
    let layout: Vec<i64> = image[..3].iter().map(number).collect();
    assert_eq!(layout, [2, 3, 1]);
    assert_eq!(byte_list(&image[3]), [0, 153, 51, 204, 102, 255]);
}

#[test]
fn from_array_refuses_other_shapes_and_dtypes() {
    let from =
        |array: Value, planes: i64| message(call("image::__aura_from_array", array, &[planes]));
    assert!(from(float_array(&[0.0; 4], &[4]), 0).contains("two or three axes, found shape [4]"));
    assert!(from(float_array(&[0.0; 16], &[2, 2, 2, 2]), 0).contains("two or three axes"));
    assert!(from(float_array(&[0.0; 10], &[1, 2, 5]), 0).contains("1 to 4 channels, found 5"));
    assert!(from(float_array(&[0.0; 10], &[5, 2, 1]), 1).contains("1 to 4 channels, found 5"));
    assert!(from(float_array(&[], &[0, 4, 3]), 0).contains("at least one pixel"));
    assert!(from(float_array(&[], &[2, 0]), 0).contains("at least one pixel"));
    assert!(from(Value::Bool(true), 0).contains("expects an `Array[float32]`"));
    let wide = Value::Array(
        ArrayValue::new(
            [1usize, 1].into(),
            ArrayStorage::Float64(vec![0.5f64].into_boxed_slice().into()),
        )
        .expect("the array is well formed"),
    );
    assert!(from(wide, 0).contains("expects an `Array[float32]`"));
}

#[test]
fn arguments_of_the_wrong_kind_are_refused() {
    for name in IMAGE_HOST_FUNCTIONS {
        let function = image_host_function(name).expect("listed functions exist");
        assert!(message(function(Vec::new(), None)).contains("expects at least one argument"));
        // Text where an integer goes.
        let refused = function(vec![bytes(&PIXELS), Value::string("three")], None);
        let error = refused.expect_err("text is no integer");
        assert_eq!(error.code, "AU4001");
        assert!(
            error.message.contains("expects argument 2 to be `int64`"),
            "{name}"
        );
    }
    assert!(message(call("image::__aura_info", bytes(&PIXELS), &[1]))
        .contains("expects 1 arguments, found 2"));
    assert!(image_host_function("image::__aura_missing").is_none());
    assert!(message(evaluate_image_builtin(
        "image::__aura_missing",
        &Value::Unit,
        &[]
    ))
    .contains("is not an image function"));
    assert_eq!(
        failure(&ImageError::Unsupported("x".to_string())),
        (2, 0, "x".to_string())
    );
}
