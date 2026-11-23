//! The Rust side of the image benchmark: the `image` crate.

use std::io::Cursor;
use std::time::Instant;

use image::codecs::jpeg::JpegEncoder;
use image::codecs::png::{CompressionType, FilterType, PngEncoder};
use image::imageops::{self, FilterType as Resample};
use image::{ExtendedColorType, ImageEncoder, ImageFormat, RgbImage};

const ROUNDS: u32 = 10;

/// Prints the fastest of ten runs of `work`, and a number computed from its
/// last result outside the timed part.
fn phase<T>(name: &str, mut work: impl FnMut() -> T, check: impl Fn(&T) -> u64) {
    let mut best = f64::INFINITY;
    let mut result = None;
    for _ in 0..ROUNDS {
        let started = Instant::now();
        result = Some(work());
        best = best.min(started.elapsed().as_secs_f64() * 1000.0);
    }
    let result = result.expect("at least one round runs");
    println!("{name} {best:.1} {}", check(&result));
}

fn total(bytes: &[u8]) -> u64 {
    bytes.iter().map(|&byte| u64::from(byte)).sum()
}

fn decode(data: &[u8], format: ImageFormat) -> RgbImage {
    image::load_from_memory_with_format(data, format)
        .expect("the file decodes")
        .into_rgb8()
}

fn main() {
    let workdir = std::env::args().nth(1).expect("usage: image_codec <workdir>");
    let png = std::fs::read(format!("{workdir}/photo.png")).expect("photo.png reads");
    let jpeg = std::fs::read(format!("{workdir}/photo.jpg")).expect("photo.jpg reads");
    let picture = decode(&png, ImageFormat::Png);
    let (width, height) = picture.dimensions();
    let pixels = u64::from(width) * u64::from(height) * 3;

    phase(
        "decode_png",
        || decode(&png, ImageFormat::Png),
        |image| total(image.as_raw()),
    );
    phase(
        "decode_jpeg",
        || decode(&jpeg, ImageFormat::Jpeg),
        |image| total(image.as_raw()) / pixels,
    );
    let encode_png = |compression: CompressionType| {
        let mut out = Vec::new();
        PngEncoder::new_with_quality(&mut out, compression, FilterType::Adaptive)
            .write_image(picture.as_raw(), width, height, ExtendedColorType::Rgb8)
            .expect("the image encodes");
        out
    };
    let length = |out: &Vec<u8>| out.len() as u64;
    phase("encode_png", || encode_png(CompressionType::Default), length);
    phase("encode_png_fast", || encode_png(CompressionType::Fast), length);
    phase(
        "encode_jpeg",
        || {
            let mut out = Cursor::new(Vec::new());
            JpegEncoder::new_with_quality(&mut out, 90)
                .write_image(picture.as_raw(), width, height, ExtendedColorType::Rgb8)
                .expect("the image encodes");
            out.into_inner()
        },
        length,
    );
    phase(
        "resize",
        || imageops::resize(&picture, width / 2, height / 2, Resample::Triangle),
        |small| total(small.as_raw()) / (pixels / 4),
    );
    phase(
        "to_floats",
        || {
            let raw = picture.as_raw();
            let plane = (width * height) as usize;
            let mut planes = vec![0f32; raw.len()];
            for (channel, target) in planes.chunks_exact_mut(plane).enumerate() {
                for (value, pixel) in target.iter_mut().zip(raw.chunks_exact(3)) {
                    *value = f32::from(pixel[channel]) / 255.0;
                }
            }
            planes
        },
        |planes| planes.iter().map(|&value| f64::from(value)).sum::<f64>() as u64,
    );
}
