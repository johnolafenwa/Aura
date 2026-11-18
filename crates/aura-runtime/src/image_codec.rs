//! Image decoding, encoding and pixel operations for the `image` module.
//!
//! An image here is 8-bit samples, row by row, with the channels of a pixel
//! next to each other: one channel is gray, two is gray and alpha, three is
//! red, green, blue, and four adds alpha.

use std::fmt;
use std::io::Cursor;

use zune_jpeg::zune_core::bytestream::ZCursor;

/// The most pixels `image.decode` accepts when the caller names no limit.
pub const DEFAULT_MAX_PIXELS: i64 = 1 << 28;

/// The largest pixel limit a caller may name: four channels of it still fit
/// a byte list.
pub const MAX_PIXEL_LIMIT: i64 = (i32::MAX / 4) as i64;

/// The PNG compression level `image.encode_png` uses when the caller names
/// none.
pub const DEFAULT_PNG_LEVEL: i64 = 6;

/// The JPEG quality `image.encode_jpeg` uses when the caller names none.
pub const DEFAULT_JPEG_QUALITY: i64 = 90;

/// A file format the module reads and writes.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Format {
    Png,
    Jpeg,
    /// The Netpbm family: PBM, PGM and PPM, text or binary.
    Pnm,
}

impl Format {
    /// The number the Aura source of the module knows the format by.
    pub const fn code(self) -> i64 {
        match self {
            Self::Png => 0,
            Self::Jpeg => 1,
            Self::Pnm => 2,
        }
    }

    pub const fn from_code(code: i64) -> Option<Self> {
        match code {
            0 => Some(Self::Png),
            1 => Some(Self::Jpeg),
            2 => Some(Self::Pnm),
            _ => None,
        }
    }
}

/// Why an image could not be decoded or encoded.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ImageError {
    /// The bytes are not a complete, well-formed image.
    InvalidData(String),
    /// The image is well formed, but this module does not handle it.
    Unsupported(String),
    /// The image has more pixels than the caller's limit.
    TooLarge { limit_pixels: usize },
}

impl ImageError {
    /// The number the Aura source of the module knows the error by.
    pub const fn code(&self) -> i64 {
        match self {
            Self::InvalidData(_) => 1,
            Self::Unsupported(_) => 2,
            Self::TooLarge { .. } => 3,
        }
    }
}

impl fmt::Display for ImageError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidData(message) | Self::Unsupported(message) => f.write_str(message),
            Self::TooLarge { limit_pixels } => {
                write!(f, "the image has more than {limit_pixels} pixels")
            }
        }
    }
}

fn invalid(message: impl Into<String>) -> ImageError {
    ImageError::InvalidData(message.into())
}

fn unsupported(message: impl Into<String>) -> ImageError {
    ImageError::Unsupported(message.into())
}

/// Decoded pixels.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Pixels {
    pub width: usize,
    pub height: usize,
    pub channels: usize,
    pub data: Vec<u8>,
}

/// What an image's header says, without decoding its pixels.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Header {
    pub format: Format,
    pub width: usize,
    pub height: usize,
    pub channels: usize,
}

/// The format `data` starts as, from its first bytes.
pub fn sniff(data: &[u8]) -> Option<Format> {
    if data.starts_with(b"\x89PNG\r\n\x1a\n") {
        Some(Format::Png)
    } else if data.starts_with(&[0xff, 0xd8, 0xff]) {
        Some(Format::Jpeg)
    } else if data.len() >= 3
        && data[0] == b'P'
        && (b'1'..=b'6').contains(&data[1])
        && (data[2].is_ascii_whitespace() || data[2] == b'#')
    {
        Some(Format::Pnm)
    } else {
        None
    }
}

fn sniffed(data: &[u8]) -> Result<Format, ImageError> {
    if data.is_empty() {
        return Err(invalid("the input is empty"));
    }
    sniff(data).ok_or_else(|| unsupported("the data is not a PNG, JPEG or Netpbm image"))
}

/// Checks a `width` by `height` image of `channels` against the invariants
/// every image keeps: no empty side, and a byte count that fits a list.
fn checked_pixels(width: usize, height: usize, limit_pixels: usize) -> Result<usize, ImageError> {
    if width == 0 || height == 0 {
        return Err(invalid("the image has no pixels"));
    }
    match width.checked_mul(height) {
        Some(pixels) if pixels <= limit_pixels => Ok(pixels),
        _ => Err(ImageError::TooLarge { limit_pixels }),
    }
}

/// The size, channel count and format of the image in `data`.
pub fn header(data: &[u8]) -> Result<Header, ImageError> {
    let format = sniffed(data)?;
    let (width, height, channels) = match format {
        Format::Png => {
            // The transparency key and the palette come after the
            // first header, so read up to the pixel data.
            let reader = png_decoder(data, 64 << 20)
                .read_info()
                .map_err(png_decode_error)?;
            let channels = png_channels(reader.output_color_type().0);
            let info = reader.info();
            (info.width as usize, info.height as usize, channels)
        }
        Format::Jpeg => {
            let mut decoder = zune_jpeg::JpegDecoder::new(ZCursor::new(data));
            decoder.decode_headers().map_err(jpeg_decode_error)?;
            let (width, height) = decoder.dimensions().ok_or_else(jpeg_without_frame)?;
            (width, height, jpeg_output_channels(&decoder))
        }
        Format::Pnm => {
            let header = PnmHeader::parse(data)?;
            (header.width, header.height, header.channels())
        }
    };
    checked_pixels(width, height, usize::MAX)?;
    Ok(Header {
        format,
        width,
        height,
        channels,
    })
}

/// The pixels of the image in `data`, which may be PNG, JPEG or Netpbm.
/// An image of more than `limit_pixels` pixels is refused before any of it
/// is decoded.
pub fn decode(data: &[u8], limit_pixels: usize) -> Result<Pixels, ImageError> {
    match sniffed(data)? {
        Format::Png => guarded(|| decode_png(data, limit_pixels)),
        Format::Jpeg => guarded(|| decode_jpeg(data, limit_pixels)),
        Format::Pnm => decode_pnm(data, limit_pixels),
    }
}

/// Runs a decoder from another crate. Should one panic on a file it did
/// not expect, the file is reported as invalid and the program goes on.
fn guarded<T>(work: impl FnOnce() -> Result<T, ImageError>) -> Result<T, ImageError> {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(work)).unwrap_or_else(|_| {
        Err(invalid(
            "invalid image: the decoder could not read the data",
        ))
    })
}

// ---------------------------------------------------------------- PNG

fn png_decoder(data: &[u8], limit_bytes: usize) -> png::Decoder<Cursor<&[u8]>> {
    let mut decoder =
        png::Decoder::new_with_limits(Cursor::new(data), png::Limits { bytes: limit_bytes });
    // Palettes, low bit depths and transparency keys expand to plain
    // samples, and 16-bit samples keep their high byte.
    decoder.set_transformations(png::Transformations::normalize_to_color8());
    decoder
}

fn png_channels(color: png::ColorType) -> usize {
    match color {
        png::ColorType::Grayscale | png::ColorType::Indexed => 1,
        png::ColorType::GrayscaleAlpha => 2,
        png::ColorType::Rgb => 3,
        png::ColorType::Rgba => 4,
    }
}

fn png_decode_error(error: png::DecodingError) -> ImageError {
    match error {
        png::DecodingError::LimitsExceeded => {
            unsupported("the PNG needs more memory than its pixel limit allows")
        }
        other => invalid(format!("invalid PNG: {other}")),
    }
}

fn decode_png(data: &[u8], limit_pixels: usize) -> Result<Pixels, ImageError> {
    let mut decoder = png_decoder(data, usize::MAX);
    let info = decoder.read_header_info().map_err(png_decode_error)?;
    let (width, height) = (info.width as usize, info.height as usize);
    let pixels = checked_pixels(width, height, limit_pixels)?;
    // The decoder may hold the frame at 16 bits and four channels while it
    // works, plus text and profile chunks.
    let mut decoder = png_decoder(data, pixels.saturating_mul(16).saturating_add(64 << 20));
    decoder.set_ignore_text_chunk(true);
    decoder.set_ignore_iccp_chunk(true);
    let mut reader = decoder.read_info().map_err(png_decode_error)?;
    let size = reader
        .output_buffer_size()
        .ok_or(ImageError::TooLarge { limit_pixels })?;
    let mut buffer = vec![0u8; size];
    let frame = reader.next_frame(&mut buffer).map_err(png_decode_error)?;
    let channels = png_channels(frame.color_type);
    buffer.truncate(pixels * channels);
    Ok(Pixels {
        width,
        height,
        channels,
        data: buffer,
    })
}

/// The pixels as a PNG file. `level` runs from 0, stored, to 9, smallest.
pub fn encode_png(
    width: usize,
    height: usize,
    channels: usize,
    data: &[u8],
    level: u32,
) -> Result<Vec<u8>, ImageError> {
    let (png_width, png_height) = match (u32::try_from(width), u32::try_from(height)) {
        (Ok(width), Ok(height)) => (width, height),
        _ => return Err(unsupported("the image is too large for PNG")),
    };
    let color = match channels {
        1 => png::ColorType::Grayscale,
        2 => png::ColorType::GrayscaleAlpha,
        3 => png::ColorType::Rgb,
        _ => png::ColorType::Rgba,
    };
    let compression = match level {
        0 => png::Compression::NoCompression,
        1 => png::Compression::Fastest,
        2 | 3 => png::Compression::Fast,
        4..=6 => png::Compression::Balanced,
        _ => png::Compression::High,
    };
    let mut output = Vec::with_capacity(data.len() / 2 + 128);
    let mut encoder = png::Encoder::new(&mut output, png_width, png_height);
    encoder.set_color(color);
    encoder.set_depth(png::BitDepth::Eight);
    encoder.set_compression(compression);
    let mut writer = encoder.write_header().map_err(png_failed)?;
    writer.write_image_data(data).map_err(png_failed)?;
    writer.finish().map_err(png_failed)?;
    Ok(output)
}

fn png_failed(error: png::EncodingError) -> ImageError {
    invalid(format!("PNG encoding failed: {error}"))
}

// ---------------------------------------------------------------- JPEG

fn jpeg_decode_error(error: zune_jpeg::errors::DecodeErrors) -> ImageError {
    use zune_jpeg::errors::DecodeErrors;
    match error {
        DecodeErrors::Unsupported(what) => unsupported(format!("unsupported JPEG: {what:?}")),
        other => invalid(format!("invalid JPEG: {other:?}")),
    }
}

fn jpeg_output_channels(decoder: &zune_jpeg::JpegDecoder<ZCursor<&[u8]>>) -> usize {
    match decoder.input_colorspace() {
        Some(zune_jpeg::zune_core::colorspace::ColorSpace::Luma) => 1,
        _ => 3,
    }
}

fn decode_jpeg(data: &[u8], limit_pixels: usize) -> Result<Pixels, ImageError> {
    use zune_jpeg::zune_core::colorspace::ColorSpace;
    use zune_jpeg::zune_core::options::DecoderOptions;

    let mut decoder = zune_jpeg::JpegDecoder::new(ZCursor::new(data));
    decoder.decode_headers().map_err(jpeg_decode_error)?;
    let (width, height) = decoder.dimensions().ok_or_else(jpeg_without_frame)?;
    let pixels = checked_pixels(width, height, limit_pixels)?;
    let channels = jpeg_output_channels(&decoder);
    let colorspace = if channels == 1 {
        ColorSpace::Luma
    } else {
        ColorSpace::RGB
    };
    let options = DecoderOptions::default()
        .set_max_width(width)
        .set_max_height(height)
        .jpeg_set_out_colorspace(colorspace);
    let mut decoder = zune_jpeg::JpegDecoder::new_with_options(ZCursor::new(data), options);
    let mut buffer = vec![0u8; pixels * channels];
    decoder
        .decode_into(&mut buffer)
        .map_err(jpeg_decode_error)?;
    Ok(Pixels {
        width,
        height,
        channels,
        data: buffer,
    })
}

/// The pixels as a baseline JPEG file. `quality` runs from 1 to 100. Only
/// gray and RGB images have a JPEG form. A large image is encoded in
/// horizontal strips on several threads.
pub fn encode_jpeg(
    width: usize,
    height: usize,
    channels: usize,
    data: &[u8],
    quality: u8,
) -> Result<Vec<u8>, ImageError> {
    encode_jpeg_on(width, height, channels, data, quality, thread_limit())
}

/// The threads the codecs and `resize` may use: `AURA_ARRAY_THREADS` when
/// it is set, and otherwise the CPUs the process may run on, up to 16.
fn thread_limit() -> usize {
    crate::array_simd::array_thread_limit()
}

/// The fewest pixels a strip holds: below this a thread costs more than it
/// saves.
const JPEG_STRIP_MIN_PIXELS: usize = 1 << 16;

/// The most strips an image is cut into.
const JPEG_MAX_STRIPS: usize = 16;

/// The rows of pixels a strip is a multiple of: one block row of colour at
/// half resolution, and two block rows of gray.
const JPEG_STRIP_ROWS: usize = 16;

fn encode_jpeg_on(
    width: usize,
    height: usize,
    channels: usize,
    data: &[u8],
    quality: u8,
    threads: usize,
) -> Result<Vec<u8>, ImageError> {
    use libjpeg_turbo_rs::{PixelFormat, Subsampling};

    let (jpeg_width, jpeg_height) = match (u16::try_from(width), u16::try_from(height)) {
        (Ok(width), Ok(height)) => (width, height),
        _ => return Err(unsupported("JPEG holds at most 65535 pixels a side")),
    };
    let (color, sampling) =
        match channels {
            1 => (PixelFormat::Grayscale, Subsampling::S444),
            3 => (PixelFormat::Rgb, Subsampling::S420),
            _ => return Err(unsupported(
                "JPEG has no alpha channel: convert the image with `to_rgb()` or `to_gray()` first",
            )),
        };
    let encode = |rows: &[u8], height: u16| {
        libjpeg_turbo_rs::compress(
            rows,
            usize::from(jpeg_width),
            usize::from(height),
            color,
            quality,
            sampling,
        )
        .map_err(|error| invalid(format!("JPEG encoding failed: {error}")))
    };

    // A strip is a whole number of block rows, so its data is exactly what
    // the whole image would hold between two restart markers. The units in
    // a strip must fit the marker's 16-bit count.
    let unit_rows = height.div_ceil(JPEG_STRIP_ROWS);
    let units_across = if channels == 3 {
        width.div_ceil(16)
    } else {
        width.div_ceil(8) * 2
    };
    let strips = threads
        .min(JPEG_MAX_STRIPS)
        .min(width * height / JPEG_STRIP_MIN_PIXELS)
        .min(unit_rows);
    let strip_unit_rows = unit_rows.div_ceil(strips.max(1));
    let interval = units_across * strip_unit_rows;
    if strips < 2 || interval > usize::from(u16::MAX) {
        return encode(data, jpeg_height);
    }
    let strip_bytes = strip_unit_rows * JPEG_STRIP_ROWS * width * channels;
    let row_bytes = width * channels;
    let parts: Vec<Result<Vec<u8>, ImageError>> = std::thread::scope(|scope| {
        let mut chunks = data.chunks(strip_bytes);
        let first = chunks.next().unwrap_or_default();
        let workers: Vec<_> = chunks
            .map(|rows| scope.spawn(move || encode(rows, (rows.len() / row_bytes) as u16)))
            .collect();
        let mut parts = vec![encode(first, (first.len() / row_bytes) as u16)];
        parts.extend(
            workers
                .into_iter()
                .map(|worker| worker.join().ok().unwrap_or(Err(jpeg_strip_failed()))),
        );
        parts
    });
    let parts = parts.into_iter().collect::<Result<Vec<_>, _>>()?;
    join_jpeg_strips(&parts, jpeg_height, interval as u16).ok_or_else(jpeg_strip_failed)
}

fn jpeg_without_frame() -> ImageError {
    invalid("the JPEG has no frame header")
}

fn jpeg_strip_failed() -> ImageError {
    invalid("JPEG encoding failed: a strip is not a baseline JPEG")
}

/// Where a JPEG file's frame header and its scan data lie.
struct JpegLayout {
    /// The offset of the two height bytes in the frame header.
    height_at: usize,
    /// The offset of the scan header's marker.
    scan_header: usize,
    /// The scan data, up to the end marker.
    scan_data: std::ops::Range<usize>,
}

fn jpeg_layout(file: &[u8]) -> Option<JpegLayout> {
    let mut at = 2;
    let mut height_at = None;
    loop {
        let marker = *file.get(at + 1)?;
        if *file.get(at)? != 0xff {
            return None;
        }
        let length = usize::from(u16::from_be_bytes([*file.get(at + 2)?, *file.get(at + 3)?]));
        match marker {
            // The baseline frame header: length, precision, height.
            0xc0 => height_at = Some(at + 5),
            0xda => {
                let start = at + 2 + length;
                let end = file.len().checked_sub(2)?;
                if start > end || file[end..] != [0xff, 0xd9] {
                    return None;
                }
                return Some(JpegLayout {
                    height_at: height_at?,
                    scan_header: at,
                    scan_data: start..end,
                });
            }
            _ => {}
        }
        at += 2 + length;
    }
}

/// One JPEG file from strips encoded on their own: the first strip's
/// headers with the whole height, a restart interval of one strip, and
/// the strips' scan data with restart markers between them. Each strip's
/// data ends on a whole byte and starts its predictions afresh, which is
/// what a restart marker promises a decoder.
fn join_jpeg_strips(parts: &[Vec<u8>], height: u16, interval: u16) -> Option<Vec<u8>> {
    let first = &parts[0];
    let layout = jpeg_layout(first)?;
    let mut output = Vec::with_capacity(parts.iter().map(Vec::len).sum::<usize>() + 8);
    output.extend_from_slice(&first[..layout.scan_header]);
    output[layout.height_at..layout.height_at + 2].copy_from_slice(&height.to_be_bytes());
    output.extend_from_slice(&[0xff, 0xdd, 0x00, 0x04]);
    output.extend_from_slice(&interval.to_be_bytes());
    output.extend_from_slice(&first[layout.scan_header..layout.scan_data.end]);
    for (index, part) in parts.iter().enumerate().skip(1) {
        output.extend_from_slice(&[0xff, 0xd0 + ((index - 1) % 8) as u8]);
        output.extend_from_slice(&part[jpeg_layout(part)?.scan_data]);
    }
    output.extend_from_slice(&[0xff, 0xd9]);
    Some(output)
}

// ---------------------------------------------------------------- Netpbm

struct PnmHeader {
    /// The digit after `P`, 1 to 6.
    kind: u8,
    width: usize,
    height: usize,
    max_value: usize,
    /// Where the samples start.
    body: usize,
}

struct PnmCursor<'a> {
    data: &'a [u8],
    at: usize,
}

impl PnmCursor<'_> {
    /// Moves past whitespace and `#` comments.
    fn skip_space(&mut self) {
        while let Some(&byte) = self.data.get(self.at) {
            if byte == b'#' {
                while self.data.get(self.at).is_some_and(|byte| *byte != b'\n') {
                    self.at += 1;
                }
            } else if byte.is_ascii_whitespace() {
                self.at += 1;
            } else {
                break;
            }
        }
    }

    /// The next decimal number.
    fn number(&mut self) -> Result<usize, ImageError> {
        self.skip_space();
        let start = self.at;
        let mut value = 0usize;
        while let Some(digit) = self.data.get(self.at).filter(|byte| byte.is_ascii_digit()) {
            value = value
                .checked_mul(10)
                .and_then(|value| value.checked_add(usize::from(digit - b'0')))
                .ok_or_else(|| invalid("invalid Netpbm image: a number is too large"))?;
            self.at += 1;
        }
        if self.at == start {
            return Err(invalid(if start >= self.data.len() {
                "invalid Netpbm image: the data ends early"
            } else {
                "invalid Netpbm image: expected a number"
            }));
        }
        Ok(value)
    }
}

impl PnmHeader {
    fn parse(data: &[u8]) -> Result<Self, ImageError> {
        let kind = data[1] - b'0';
        let mut cursor = PnmCursor { data, at: 2 };
        let width = cursor.number()?;
        let height = cursor.number()?;
        let max_value = if kind == 1 || kind == 4 {
            1
        } else {
            cursor.number()?
        };
        if max_value == 0 || max_value > 65535 {
            return Err(invalid(
                "invalid Netpbm image: the maximum value must be from 1 to 65535",
            ));
        }
        // One whitespace byte ends a binary header.
        if kind >= 4 {
            match data.get(cursor.at) {
                Some(byte) if byte.is_ascii_whitespace() => cursor.at += 1,
                Some(_) => {
                    return Err(invalid(
                        "invalid Netpbm image: expected whitespace after the header",
                    ))
                }
                None => return Err(invalid("invalid Netpbm image: the data ends early")),
            }
        }
        Ok(Self {
            kind,
            width,
            height,
            max_value,
            body: cursor.at,
        })
    }

    const fn channels(&self) -> usize {
        if self.kind == 3 || self.kind == 6 {
            3
        } else {
            1
        }
    }
}

/// A sample of `0..=max_value` on the 0 to 255 scale, rounded.
fn scale_sample(value: usize, max_value: usize) -> Result<u8, ImageError> {
    if value > max_value {
        return Err(invalid(
            "invalid Netpbm image: a sample is above the maximum value",
        ));
    }
    Ok(((value * 255 + max_value / 2) / max_value) as u8)
}

fn decode_pnm(data: &[u8], limit_pixels: usize) -> Result<Pixels, ImageError> {
    let header = PnmHeader::parse(data)?;
    let (width, height, channels) = (header.width, header.height, header.channels());
    let pixels = checked_pixels(width, height, limit_pixels)?;
    let samples = pixels * channels;
    let body = &data[header.body..];
    let short = || invalid("invalid Netpbm image: the data ends early");
    let mut output = Vec::new();
    match header.kind {
        // Text bitmaps: one digit a pixel, 1 is black, whitespace optional.
        1 => {
            let mut cursor = PnmCursor {
                data,
                at: header.body,
            };
            // A text image cannot hold more samples than it has bytes.
            if samples > body.len() {
                return Err(short());
            }
            output.reserve_exact(samples);
            for _ in 0..samples {
                cursor.skip_space();
                match data.get(cursor.at) {
                    Some(b'0') => output.push(255),
                    Some(b'1') => output.push(0),
                    Some(_) => {
                        return Err(invalid(
                            "invalid Netpbm image: a bitmap sample must be 0 or 1",
                        ))
                    }
                    None => return Err(short()),
                }
                cursor.at += 1;
            }
        }
        2 | 3 => {
            let mut cursor = PnmCursor {
                data,
                at: header.body,
            };
            if samples > body.len() {
                return Err(short());
            }
            output.reserve_exact(samples);
            for _ in 0..samples {
                output.push(scale_sample(cursor.number()?, header.max_value)?);
            }
        }
        // Binary bitmaps: rows of bits, most significant first, each row
        // padded to a whole byte.
        4 => {
            let row_bytes = width.div_ceil(8);
            let needed = row_bytes.checked_mul(height).ok_or_else(short)?;
            let rows = body.get(..needed).ok_or_else(short)?;
            output.reserve_exact(samples);
            for row in rows.chunks_exact(row_bytes) {
                for x in 0..width {
                    let bit = row[x / 8] >> (7 - x % 8) & 1;
                    output.push(if bit == 1 { 0 } else { 255 });
                }
            }
        }
        _ if header.max_value == 255 => {
            output.extend_from_slice(body.get(..samples).ok_or_else(short)?);
        }
        _ if header.max_value < 256 => {
            let raw = body.get(..samples).ok_or_else(short)?;
            output.reserve_exact(samples);
            for &sample in raw {
                output.push(scale_sample(usize::from(sample), header.max_value)?);
            }
        }
        // Two bytes a sample, most significant first.
        _ => {
            let needed = samples.checked_mul(2).ok_or_else(short)?;
            let raw = body.get(..needed).ok_or_else(short)?;
            output.reserve_exact(samples);
            for pair in raw.chunks_exact(2) {
                let value = usize::from(pair[0]) << 8 | usize::from(pair[1]);
                output.push(scale_sample(value, header.max_value)?);
            }
        }
    }
    Ok(Pixels {
        width,
        height,
        channels,
        data: output,
    })
}

/// The pixels as a binary Netpbm file: PGM for gray, PPM for RGB.
pub fn encode_pnm(
    width: usize,
    height: usize,
    channels: usize,
    data: &[u8],
) -> Result<Vec<u8>, ImageError> {
    let kind =
        match channels {
            1 => 5,
            3 => 6,
            _ => return Err(unsupported(
                "PPM has no alpha channel: convert the image with `to_rgb()` or `to_gray()` first",
            )),
        };
    let mut output = format!("P{kind}\n{width} {height}\n255\n").into_bytes();
    output.reserve_exact(data.len());
    output.extend_from_slice(data);
    Ok(output)
}

// ---------------------------------------------------------------- pixels

/// Gray from red, green and blue with the BT.601 weights, rounded.
#[inline]
fn luma(red: u8, green: u8, blue: u8) -> u8 {
    ((u32::from(red) * 19595 + u32::from(green) * 38470 + u32::from(blue) * 7471 + 32768) >> 16)
        as u8
}

/// `data` with `from` channels a pixel as `to` channels a pixel. A new
/// alpha channel is opaque, and a dropped one is discarded.
pub fn convert_channels(data: &[u8], from: usize, to: usize) -> Vec<u8> {
    if from == to {
        return data.to_vec();
    }
    let pixels = data.len() / from;
    let mut output = Vec::with_capacity(pixels * to);
    for pixel in data.chunks_exact(from) {
        let (red, green, blue, alpha) = match from {
            1 => (pixel[0], pixel[0], pixel[0], 255),
            2 => (pixel[0], pixel[0], pixel[0], pixel[1]),
            3 => (pixel[0], pixel[1], pixel[2], 255),
            _ => (pixel[0], pixel[1], pixel[2], pixel[3]),
        };
        let gray = || {
            if from <= 2 {
                red
            } else {
                luma(red, green, blue)
            }
        };
        match to {
            1 => output.push(gray()),
            2 => output.extend_from_slice(&[gray(), alpha]),
            3 => output.extend_from_slice(&[red, green, blue]),
            _ => output.extend_from_slice(&[red, green, blue, alpha]),
        }
    }
    output
}

/// How `resize` computes a pixel from the pixels around it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Filter {
    Nearest,
    Bilinear,
    Bicubic,
}

impl Filter {
    pub const fn from_code(code: i64) -> Option<Self> {
        match code {
            0 => Some(Self::Nearest),
            1 => Some(Self::Bilinear),
            2 => Some(Self::Bicubic),
            _ => None,
        }
    }

    /// How far from its centre the filter reaches, in source pixels at
    /// scale one.
    const fn support(self) -> f64 {
        match self {
            Self::Nearest => 0.0,
            Self::Bilinear => 1.0,
            Self::Bicubic => 2.0,
        }
    }

    fn weight(self, distance: f64) -> f64 {
        let x = distance.abs();
        match self {
            Self::Nearest => 0.0,
            Self::Bilinear => (1.0 - x).max(0.0),
            Self::Bicubic => {
                // The Keys kernel with a = -0.5, as Pillow uses.
                const A: f64 = -0.5;
                if x < 1.0 {
                    ((A + 2.0) * x - (A + 3.0)) * x * x + 1.0
                } else if x < 2.0 {
                    (((x - 5.0) * x + 8.0) * x - 4.0) * A
                } else {
                    0.0
                }
            }
        }
    }
}

/// For each output position along one axis: the first source position it
/// reads and the weights of the positions from there on. The weights of a
/// position sum to one. When the axis shrinks the filter widens, so every
/// source pixel counts.
struct AxisWeights {
    starts: Vec<usize>,
    weights: Vec<f32>,
    taps: usize,
}

fn axis_weights(source: usize, target: usize, filter: Filter) -> AxisWeights {
    let scale = source as f64 / target as f64;
    let stretch = scale.max(1.0);
    let support = filter.support() * stretch;
    // The source positions whose centres lie inside the filter.
    let reach = |position: usize| {
        let centre = (position as f64 + 0.5) * scale;
        let first = ((centre - support + 0.5).floor().max(0.0) as usize).min(source - 1);
        let last = ((centre + support + 0.5).floor() as usize).clamp(first + 1, source);
        (centre, first, last)
    };
    let taps = (0..target)
        .map(|position| {
            let (_, first, last) = reach(position);
            last - first
        })
        .max()
        .unwrap_or(1);
    let mut starts = Vec::with_capacity(target);
    let mut weights = Vec::with_capacity(target * taps);
    for position in 0..target {
        let (centre, first, last) = reach(position);
        // Every position reads `taps` samples, the ones past its reach
        // with no weight, and none past the end of the axis.
        let start = first.min(source - taps);
        let begin = weights.len();
        let mut total = 0.0f64;
        for index in start..start + taps {
            let weight = if (first..last).contains(&index) {
                filter.weight((index as f64 + 0.5 - centre) / stretch)
            } else {
                0.0
            };
            total += weight;
            weights.push(weight as f32);
        }
        // The sample nearest the centre always carries weight.
        for weight in &mut weights[begin..] {
            *weight = (f64::from(*weight) / total) as f32;
        }
        starts.push(start);
    }
    AxisWeights {
        starts,
        weights,
        taps,
    }
}

#[inline]
fn to_sample(value: f32) -> u8 {
    // `as` saturates, so values below 0 and above 255 clamp.
    (value + 0.5) as u8
}

/// The fewest output samples a thread of `resize` should have to itself.
const RESIZE_THREAD_SAMPLES: usize = 1 << 18;

/// Fills `output`, whose rows are `row_len` long, by calling `fill` with
/// the number of a first row and the rows from there. A large output is
/// filled as runs of rows on several threads.
fn fill_rows(
    output: &mut [f32],
    row_len: usize,
    threads: usize,
    fill: impl Fn(usize, &mut [f32]) + Sync,
) {
    let rows = output.len() / row_len;
    let threads = threads.min(output.len() / RESIZE_THREAD_SAMPLES).min(rows);
    if threads < 2 {
        return fill(0, output);
    }
    let share = rows.div_ceil(threads);
    std::thread::scope(|scope| {
        let mut parts = output.chunks_mut(share * row_len).enumerate();
        let first = parts.next();
        for (index, part) in parts {
            let fill = &fill;
            scope.spawn(move || fill(index * share, part));
        }
        if let Some((_, part)) = first {
            fill(0, part);
        }
    });
}

/// Rows of `C` channels a pixel resampled across: each output row is
/// `weights.starts.len()` pixels.
fn resample_across<T: Copy + Into<f32> + Sync, const C: usize>(
    source: &[T],
    width: usize,
    weights: &AxisWeights,
    threads: usize,
) -> Vec<f32> {
    let new_width = weights.starts.len();
    let rows = source.len() / (width * C);
    let mut output = vec![0f32; rows * new_width * C];
    fill_rows(&mut output, new_width * C, threads, |first_row, part| {
        let source = &source[first_row * width * C..];
        for (target, row) in part
            .chunks_exact_mut(new_width * C)
            .zip(source.chunks_exact(width * C))
        {
            for ((sums, first), taps) in target
                .chunks_exact_mut(C)
                .zip(&weights.starts)
                .zip(weights.weights.chunks_exact(weights.taps))
            {
                let mut pixel = [0f32; C];
                let reach = &row[first * C..][..weights.taps * C];
                for (sample, &weight) in reach.chunks_exact(C).zip(taps) {
                    for channel in 0..C {
                        pixel[channel] += weight * sample[channel].into();
                    }
                }
                sums.copy_from_slice(&pixel);
            }
        }
    });
    output
}

/// Rows of `row_len` samples resampled down the image.
fn resample_down<T: Copy + Into<f32> + Sync>(
    source: &[T],
    row_len: usize,
    weights: &AxisWeights,
    threads: usize,
) -> Vec<f32> {
    let mut output = vec![0f32; row_len * weights.starts.len()];
    fill_rows(&mut output, row_len, threads, |first_row, part| {
        for ((sums, first), taps) in part
            .chunks_exact_mut(row_len)
            .zip(&weights.starts[first_row..])
            .zip(weights.weights[first_row * weights.taps..].chunks_exact(weights.taps))
        {
            for (tap, &weight) in taps.iter().enumerate() {
                let row = &source[(first + tap) * row_len..][..row_len];
                for (sum, &value) in sums.iter_mut().zip(row) {
                    *sum += weight * value.into();
                }
            }
        }
    });
    output
}

fn resample_across_any<T: Copy + Into<f32> + Sync>(
    source: &[T],
    width: usize,
    channels: usize,
    weights: &AxisWeights,
    threads: usize,
) -> Vec<f32> {
    match channels {
        1 => resample_across::<T, 1>(source, width, weights, threads),
        2 => resample_across::<T, 2>(source, width, weights, threads),
        3 => resample_across::<T, 3>(source, width, weights, threads),
        _ => resample_across::<T, 4>(source, width, weights, threads),
    }
}

/// The image resampled to `new_width` by `new_height`. No side may be zero.
pub fn resize(
    data: &[u8],
    width: usize,
    height: usize,
    channels: usize,
    new_width: usize,
    new_height: usize,
    filter: Filter,
) -> Vec<u8> {
    resize_on(
        data,
        width,
        height,
        channels,
        new_width,
        new_height,
        filter,
        thread_limit(),
    )
}

#[allow(clippy::too_many_arguments)]
fn resize_on(
    data: &[u8],
    width: usize,
    height: usize,
    channels: usize,
    new_width: usize,
    new_height: usize,
    filter: Filter,
    threads: usize,
) -> Vec<u8> {
    if width == new_width && height == new_height {
        return data.to_vec();
    }
    if filter == Filter::Nearest {
        let columns: Vec<usize> = (0..new_width)
            .map(|x| ((x * 2 + 1) * width / (new_width * 2)).min(width - 1) * channels)
            .collect();
        let mut output = Vec::with_capacity(new_width * new_height * channels);
        for y in 0..new_height {
            let source_y = ((y * 2 + 1) * height / (new_height * 2)).min(height - 1);
            let row = &data[source_y * width * channels..][..width * channels];
            for &column in &columns {
                output.extend_from_slice(&row[column..column + channels]);
            }
        }
        return output;
    }
    let across = axis_weights(width, new_width, filter);
    let down = axis_weights(height, new_height, filter);
    // Each pass costs its output samples times its taps, and the pass
    // across costs about twice the pass down, which works on whole rows.
    // The pass that runs second works on what the first left, so the
    // cheaper order depends on which axis shrinks more.
    let across_first = 2 * new_width * height * across.taps + new_width * new_height * down.taps;
    let down_first = width * new_height * down.taps + 2 * new_width * new_height * across.taps;
    let resampled = if down_first <= across_first {
        let rows = resample_down(data, width * channels, &down, threads);
        resample_across_any(&rows, width, channels, &across, threads)
    } else {
        let rows = resample_across_any(data, width, channels, &across, threads);
        resample_down(&rows, new_width * channels, &down, threads)
    };
    resampled.iter().map(|&value| to_sample(value)).collect()
}

/// The `crop_width` by `crop_height` part of the image whose top left
/// pixel is (`left`, `top`). The part must lie inside the image.
pub fn crop(
    data: &[u8],
    width: usize,
    channels: usize,
    left: usize,
    top: usize,
    crop_width: usize,
    crop_height: usize,
) -> Vec<u8> {
    let mut output = Vec::with_capacity(crop_width * crop_height * channels);
    for y in top..top + crop_height {
        let start = (y * width + left) * channels;
        output.extend_from_slice(&data[start..start + crop_width * channels]);
    }
    output
}

/// The image mirrored left to right.
pub fn flip_horizontal(data: &[u8], width: usize, channels: usize) -> Vec<u8> {
    let mut output = Vec::with_capacity(data.len());
    for row in data.chunks_exact(width * channels) {
        for pixel in row.chunks_exact(channels).rev() {
            output.extend_from_slice(pixel);
        }
    }
    output
}

/// The image mirrored top to bottom.
pub fn flip_vertical(data: &[u8], width: usize, channels: usize) -> Vec<u8> {
    let mut output = Vec::with_capacity(data.len());
    for row in data.chunks_exact(width * channels).rev() {
        output.extend_from_slice(row);
    }
    output
}

/// The samples as floats from 0 to 1. With `planes`, each channel is one
/// plane and the planes follow one another (channels, height, width);
/// without, the order is the image's own (height, width, channels).
pub fn to_floats(data: &[u8], channels: usize, planes: bool) -> Vec<f32> {
    if !planes || channels == 1 {
        return data.iter().map(|&value| f32::from(value) / 255.0).collect();
    }
    let pixels = data.len() / channels;
    let mut output = vec![0f32; data.len()];
    for (channel, plane) in output.chunks_exact_mut(pixels).enumerate() {
        for (target, pixel) in plane.iter_mut().zip(data.chunks_exact(channels)) {
            *target = f32::from(pixel[channel]) / 255.0;
        }
    }
    output
}

/// The reverse of [`to_floats`]: each float times 255, rounded, and kept
/// inside 0 to 255. A NaN becomes 0.
pub fn from_floats(values: &[f32], channels: usize, planes: bool) -> Vec<u8> {
    let sample = |value: f32| to_sample(value * 255.0);
    if !planes || channels == 1 {
        return values.iter().map(|&value| sample(value)).collect();
    }
    let pixels = values.len() / channels;
    let mut output = vec![0u8; values.len()];
    for (channel, plane) in values.chunks_exact(pixels).enumerate() {
        for (pixel, &value) in output.chunks_exact_mut(channels).zip(plane) {
            pixel[channel] = sample(value);
        }
    }
    output
}

#[cfg(test)]
#[path = "image_codec_tests.rs"]
mod tests;
