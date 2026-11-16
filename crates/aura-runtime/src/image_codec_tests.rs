use super::*;

const NO_LIMIT: usize = usize::MAX;

/// A fixture written by `scripts/make-image-fixtures.py`, and the pixels
/// Pillow decodes it to.
fn fixture(name: &str) -> (Vec<u8>, Vec<u8>) {
    let directory = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/data/image/");
    let read = |file: String| {
        std::fs::read(format!("{directory}{file}"))
            .unwrap_or_else(|error| panic!("fixture {file} should read: {error}"))
    };
    (read(name.to_string()), read(format!("{name}.raw")))
}

fn gradient(width: usize, height: usize, channels: usize) -> Vec<u8> {
    let mut data = Vec::with_capacity(width * height * channels);
    for y in 0..height {
        for x in 0..width {
            for channel in 0..channels {
                data.push(((x * 7 + y * 13 + channel * 60) % 256) as u8);
            }
        }
    }
    data
}

fn largest_difference(left: &[u8], right: &[u8]) -> u8 {
    assert_eq!(left.len(), right.len());
    left.iter()
        .zip(right)
        .map(|(left, right)| left.abs_diff(*right))
        .max()
        .unwrap_or(0)
}

#[test]
fn png_fixtures_decode_to_the_pixels_pillow_reads() {
    for (name, channels) in [
        ("rgb.png", 3),
        ("rgba_interlaced.png", 4),
        ("gray.png", 1),
        ("gray_alpha.png", 2),
        ("palette.png", 3),
        ("palette_transparent.png", 4),
        ("bilevel.png", 1),
        ("gray16.png", 1),
    ] {
        let (file, expected) = fixture(name);
        let head = header(&file).expect("header reads");
        assert_eq!(
            (head.format, head.width, head.height, head.channels),
            (Format::Png, 16, 12, channels),
            "{name}"
        );
        let pixels = decode(&file, NO_LIMIT).expect("fixture decodes");
        assert_eq!(
            (pixels.width, pixels.height, pixels.channels),
            (16, 12, channels),
            "{name}"
        );
        assert_eq!(pixels.data, expected, "{name}");
    }
}

#[test]
fn jpeg_fixtures_decode_close_to_the_pixels_pillow_reads() {
    for (name, channels) in [
        ("baseline.jpg", 3),
        ("progressive.jpg", 3),
        ("full_chroma.jpg", 3),
        ("restart.jpg", 3),
        ("gray.jpg", 1),
        ("cmyk.jpg", 3),
    ] {
        let (file, expected) = fixture(name);
        let head = header(&file).expect("header reads");
        assert_eq!(
            (head.format, head.width, head.height, head.channels),
            (Format::Jpeg, 16, 12, channels),
            "{name}"
        );
        let pixels = decode(&file, NO_LIMIT).expect("fixture decodes");
        assert_eq!(pixels.channels, channels, "{name}");
        // Decoders may round the inverse transform and the colour
        // conversion differently.
        let difference = largest_difference(&pixels.data, &expected);
        assert!(difference <= 6, "{name} differs by {difference}");
    }
}

#[test]
fn png_round_trips_every_channel_count_at_every_level() {
    for channels in 1..=4 {
        let data = gradient(23, 9, channels);
        for level in 0..=9 {
            let file = encode_png(23, 9, channels, &data, level).expect("encodes");
            assert_eq!(sniff(&file), Some(Format::Png));
            let pixels = decode(&file, NO_LIMIT).expect("decodes");
            assert_eq!(
                (pixels.width, pixels.height, pixels.channels),
                (23, 9, channels)
            );
            assert_eq!(pixels.data, data);
        }
    }
}

#[test]
fn png_levels_trade_size() {
    let data = gradient(64, 64, 3);
    let stored = encode_png(64, 64, 3, &data, 0).expect("encodes");
    let packed = encode_png(64, 64, 3, &data, 9).expect("encodes");
    assert!(stored.len() > data.len());
    assert!(packed.len() < stored.len() / 2);
}

#[test]
fn jpeg_round_trips_gray_and_rgb_within_tolerance() {
    for channels in [1, 3] {
        // A smooth image, which JPEG keeps well.
        let mut data = Vec::new();
        for y in 0..40usize {
            for x in 0..56usize {
                for channel in 0..channels {
                    data.push((60 + x * 2 + y + channel * 20) as u8);
                }
            }
        }
        let file = encode_jpeg(56, 40, channels, &data, 95).expect("encodes");
        assert_eq!(sniff(&file), Some(Format::Jpeg));
        let pixels = decode(&file, NO_LIMIT).expect("decodes");
        assert_eq!(
            (pixels.width, pixels.height, pixels.channels),
            (56, 40, channels)
        );
        let difference = largest_difference(&pixels.data, &data);
        assert!(difference <= 12, "{channels} channels differ by {difference}");
    }
}

#[test]
fn jpeg_quality_trades_size() {
    let data = gradient(64, 64, 3);
    let small = encode_jpeg(64, 64, 3, &data, 20).expect("encodes");
    let large = encode_jpeg(64, 64, 3, &data, 95).expect("encodes");
    assert!(small.len() < large.len());
}

#[test]
fn formats_without_alpha_refuse_it() {
    for channels in [2, 4] {
        let data = gradient(4, 4, channels);
        for result in [
            encode_jpeg(4, 4, channels, &data, 90),
            encode_pnm(4, 4, channels, &data),
        ] {
            let error = result.expect_err("alpha is refused");
            assert_eq!(error.code(), 2);
            assert!(error.to_string().contains("no alpha channel"));
        }
    }
}

#[test]
fn jpeg_refuses_sides_past_its_limit() {
    let data = vec![0u8; 65536];
    let error = encode_jpeg(65536, 1, 1, &data, 90).expect_err("too wide");
    assert!(matches!(error, ImageError::Unsupported(_)));
    assert!(error.to_string().contains("65535"));
}

#[test]
fn pnm_round_trips_gray_and_rgb() {
    for channels in [1, 3] {
        let data = gradient(7, 5, channels);
        let file = encode_pnm(7, 5, channels, &data).expect("encodes");
        assert!(file.starts_with(if channels == 1 { b"P5\n7 5\n255\n" } else { b"P6\n7 5\n255\n" }));
        assert_eq!(sniff(&file), Some(Format::Pnm));
        let head = header(&file).expect("header reads");
        assert_eq!(
            (head.format, head.width, head.height, head.channels),
            (Format::Pnm, 7, 5, channels)
        );
        let pixels = decode(&file, NO_LIMIT).expect("decodes");
        assert_eq!(pixels.data, data);
    }
}

#[test]
fn pnm_reads_text_forms_comments_and_bitmaps() {
    let text_gray = b"P2\n# a comment\n3 2\n# another\n15\n0 5 15\n15 10 0\n";
    let pixels = decode(text_gray, NO_LIMIT).expect("decodes");
    assert_eq!((pixels.width, pixels.height, pixels.channels), (3, 2, 1));
    assert_eq!(pixels.data, [0, 85, 255, 255, 170, 0]);

    let text_rgb = b"P3 2 1 255 255 0 0 0 128 255";
    let pixels = decode(text_rgb, NO_LIMIT).expect("decodes");
    assert_eq!((pixels.width, pixels.height, pixels.channels), (2, 1, 3));
    assert_eq!(pixels.data, [255, 0, 0, 0, 128, 255]);

    let text_bits = b"P1\n4 2\n0110\n1 0 0 1\n";
    let pixels = decode(text_bits, NO_LIMIT).expect("decodes");
    assert_eq!(pixels.data, [255, 0, 0, 255, 0, 255, 255, 0]);

    // Ten pixels a row: two bytes a row, the second padded.
    let mut binary_bits = b"P4\n10 2\n".to_vec();
    binary_bits.extend_from_slice(&[0b1000_0000, 0b0100_0000, 0b0000_0001, 0b1000_0000]);
    let pixels = decode(&binary_bits, NO_LIMIT).expect("decodes");
    assert_eq!((pixels.width, pixels.height, pixels.channels), (10, 2, 1));
    assert_eq!(
        pixels.data,
        [
            0, 255, 255, 255, 255, 255, 255, 255, 255, 0, //
            255, 255, 255, 255, 255, 255, 255, 0, 0, 255
        ]
    );
}

#[test]
fn pnm_scales_other_maximum_values() {
    let mut low = b"P5\n2 1\n3\n".to_vec();
    low.extend_from_slice(&[1, 3]);
    assert_eq!(decode(&low, NO_LIMIT).expect("decodes").data, [85, 255]);

    let mut wide = b"P6\n1 1\n65535\n".to_vec();
    wide.extend_from_slice(&[0xff, 0xff, 0x80, 0x00, 0x00, 0x00]);
    assert_eq!(decode(&wide, NO_LIMIT).expect("decodes").data, [255, 128, 0]);
}

#[test]
fn malformed_pnm_is_invalid_data() {
    let cases: [(&[u8], &str); 11] = [
        (b"P6\n2 2\n255\n\x00\x00\x00", "ends early"),
        (b"P5\n2 2\n255", "ends early"),
        (b"P5\n2 2\n255x", "expected whitespace"),
        (b"P5\n2 x\n255\n", "expected a number"),
        (b"P5\n2 2\n0\n\x00\x00\x00\x00", "maximum value"),
        (b"P5\n2 2\n70000\n", "maximum value"),
        (b"P5\n99999999999999999999999 2\n255\n", "too large"),
        (b"P2\n2 1\n9\n3 12\n", "above the maximum"),
        (b"P5\n1 1\n3\n\x09", "above the maximum"),
        (b"P1\n2 1\n0 2\n", "must be 0 or 1"),
        (b"P5\n0 4\n255\n", "no pixels"),
    ];
    for (data, message) in cases {
        let error = decode(data, NO_LIMIT).expect_err("is refused");
        assert_eq!(error.code(), 1, "{error}");
        assert!(error.to_string().contains(message), "{error}");
    }
    for data in [
        b"P1\n3 3\n0 1".as_slice(),
        b"P2\n3 3\n9\n1 2",
        b"P4\n9 2\n\x00\x00\x00",
        b"P6\n1 1\n300\n\x00\x00",
        b"P1\n2 2\n0",
        b"P2\n2 2\n9\n1",
    ] {
        let error = decode(data, NO_LIMIT).expect_err("is refused");
        assert!(error.to_string().contains("ends early"), "{error}");
    }
}

#[test]
fn unknown_and_empty_data_are_told_apart() {
    assert_eq!(decode(b"", NO_LIMIT).expect_err("empty").code(), 1);
    assert_eq!(header(b"").expect_err("empty").code(), 1);
    let error = decode(b"GIF89a....", NO_LIMIT).expect_err("unknown");
    assert_eq!(error.code(), 2);
    assert!(error.to_string().contains("not a PNG, JPEG or Netpbm"));
    assert_eq!(sniff(b"P7\n"), None);
    assert_eq!(sniff(b"P6"), None);
    assert_eq!(sniff(b"P6#"), Some(Format::Pnm));
}

#[test]
fn truncated_and_corrupt_files_are_invalid_data() {
    for name in ["rgb.png", "rgba_interlaced.png", "baseline.jpg", "progressive.jpg"] {
        let (file, _) = fixture(name);
        for keep in [9, file.len() / 3, file.len() / 2] {
            // A decoder may fill a short JPEG with gray, but it must not
            // trap.
            if let Err(error) = decode(&file[..keep], NO_LIMIT) {
                assert_ne!(error.code(), 3, "{name}: {error}");
            }
        }
    }
    let (mut png, _) = fixture("rgb.png");
    assert!(header(&png[..20]).is_err());
    // Break the header's checksum.
    png[20] ^= 0xff;
    let error = decode(&png, NO_LIMIT).expect_err("corrupt");
    assert_eq!(error.code(), 1);
    assert!(error.to_string().starts_with("invalid PNG"));
    let error = decode(&[0xff, 0xd8, 0xff, 0xd9], NO_LIMIT).expect_err("no frame");
    assert_eq!(error.code(), 1);
    assert!(header(&[0xff, 0xd8, 0xff, 0xd9]).is_err());
}

#[test]
fn every_byte_flip_of_a_small_file_is_handled() {
    for name in [
        "palette_transparent.png",
        "rgba_interlaced.png",
        "gray16.png",
        "bilevel.png",
        "baseline.jpg",
        "progressive.jpg",
        "full_chroma.jpg",
        "restart.jpg",
        "gray.jpg",
        "cmyk.jpg",
    ] {
        let (file, _) = fixture(name);
        for index in 0..file.len() * 3 {
            let mut changed = file.clone();
            changed[index / 3] ^= [0x55, 0x01, 0x80][index % 3];
            let _ = header(&changed);
            if let Ok(pixels) = decode(&changed, 1 << 16) {
                assert_eq!(
                    pixels.data.len(),
                    pixels.width * pixels.height * pixels.channels
                );
            }
        }
    }
}

#[test]
fn the_pixel_limit_stops_a_decode_before_it_starts() {
    for name in ["rgb.png", "baseline.jpg"] {
        let (file, _) = fixture(name);
        assert!(decode(&file, 16 * 12).is_ok());
        let error = decode(&file, 16 * 12 - 1).expect_err("over the limit");
        assert_eq!(error, ImageError::TooLarge { limit_pixels: 191 });
        assert_eq!(error.code(), 3);
        assert_eq!(error.to_string(), "the image has more than 191 pixels");
    }
    let error = decode(b"P5\n4 4\n255\n", 15).expect_err("over the limit");
    assert_eq!(error.code(), 3);
    let error =
        decode(b"P5\n4294967296 4294967296\n255\n", NO_LIMIT).expect_err("over the limit");
    assert_eq!(error.code(), 3);
}

#[test]
fn a_png_that_claims_a_huge_size_is_refused_without_the_memory() {
    // A header for 60000 by 60000 RGBA, with no pixel data behind it.
    let data = gradient(1, 1, 4);
    let mut file = encode_png(1, 1, 4, &data, 6).expect("encodes");
    file[16..20].copy_from_slice(&60000u32.to_be_bytes());
    file[20..24].copy_from_slice(&60000u32.to_be_bytes());
    assert!(decode(&file, 1 << 20).is_err());
    assert!(decode(&file, NO_LIMIT).is_err());
}

#[test]
fn format_and_filter_codes_round_trip() {
    for format in [Format::Png, Format::Jpeg, Format::Pnm] {
        assert_eq!(Format::from_code(format.code()), Some(format));
    }
    assert_eq!(Format::from_code(3), None);
    assert_eq!(Filter::from_code(0), Some(Filter::Nearest));
    assert_eq!(Filter::from_code(1), Some(Filter::Bilinear));
    assert_eq!(Filter::from_code(2), Some(Filter::Bicubic));
    assert_eq!(Filter::from_code(3), None);
    assert_eq!(invalid("a").code(), 1);
    assert_eq!(unsupported("b").to_string(), "b");
}

#[test]
fn channel_conversion_covers_every_pair() {
    let rgba = [10u8, 200, 30, 128, 255, 255, 255, 0];
    assert_eq!(convert_channels(&rgba, 4, 4), rgba);
    assert_eq!(convert_channels(&rgba, 4, 3), [10, 200, 30, 255, 255, 255]);
    // 0.299 * 10 + 0.587 * 200 + 0.114 * 30 = 123.81
    assert_eq!(convert_channels(&rgba, 4, 1), [124, 255]);
    assert_eq!(convert_channels(&rgba, 4, 2), [124, 128, 255, 0]);
    let rgb = [10u8, 200, 30];
    assert_eq!(convert_channels(&rgb, 3, 4), [10, 200, 30, 255]);
    assert_eq!(convert_channels(&rgb, 3, 2), [124, 255]);
    assert_eq!(convert_channels(&rgb, 3, 1), [124]);
    let gray_alpha = [77u8, 9];
    assert_eq!(convert_channels(&gray_alpha, 2, 1), [77]);
    assert_eq!(convert_channels(&gray_alpha, 2, 3), [77, 77, 77]);
    assert_eq!(convert_channels(&gray_alpha, 2, 4), [77, 77, 77, 9]);
    let gray = [77u8];
    assert_eq!(convert_channels(&gray, 1, 2), [77, 255]);
    assert_eq!(convert_channels(&gray, 1, 3), [77, 77, 77]);
    assert_eq!(convert_channels(&gray, 1, 4), [77, 77, 77, 255]);
    // Gray stays gray through colour and back.
    for value in 0..=255u8 {
        assert_eq!(luma(value, value, value), value);
    }
}

#[test]
fn nearest_resize_repeats_and_drops_pixels() {
    let data = [1u8, 2, 3, 4];
    assert_eq!(
        resize(&data, 2, 2, 1, 4, 4, Filter::Nearest),
        [1, 1, 2, 2, 1, 1, 2, 2, 3, 3, 4, 4, 3, 3, 4, 4]
    );
    let wide = [10u8, 20, 30, 40, 50, 60, 70, 80];
    assert_eq!(resize(&wide, 8, 1, 1, 4, 1, Filter::Nearest), [20, 40, 60, 80]);
    assert_eq!(resize(&wide, 4, 1, 2, 2, 1, Filter::Nearest), [30, 40, 70, 80]);
    assert_eq!(resize(&data, 2, 2, 1, 2, 2, Filter::Bicubic), data);
}

#[test]
fn smooth_resize_keeps_flat_images_flat_and_averages_when_shrinking() {
    for filter in [Filter::Bilinear, Filter::Bicubic] {
        let flat = vec![93u8; 9 * 7 * 3];
        for (width, height) in [(4, 3), (20, 15), (9, 30), (1, 1)] {
            let resized = resize(&flat, 9, 7, 3, width, height, filter);
            assert_eq!(resized.len(), width * height * 3);
            assert!(resized.iter().all(|&value| value == 93), "{filter:?}");
        }
    }
    // Halving a checkerboard gives its mean.
    let board: Vec<u8> = (0..64).map(|i| if (i / 8 + i % 8) % 2 == 0 { 0 } else { 200 }).collect();
    let halved = resize(&board, 8, 8, 1, 4, 4, Filter::Bilinear);
    // The filter is cut off at the border, so the corners lean a little.
    assert!(halved.iter().all(|&value| (98..=102).contains(&value)), "{halved:?}");
    assert_eq!(halved[5], 100);
    // Doubling a ramp stays between its ends and keeps its order.
    let ramp = [0u8, 100, 200];
    let doubled = resize(&ramp, 3, 1, 1, 6, 1, Filter::Bilinear);
    assert_eq!(doubled, [0, 25, 75, 125, 175, 200]);
    let cubic = resize(&ramp, 3, 1, 1, 6, 1, Filter::Bicubic);
    assert!(cubic.windows(2).all(|pair| pair[0] <= pair[1]), "{cubic:?}");
    // Bicubic overshoot clamps instead of wrapping.
    let edge = [0u8, 0, 255, 255];
    let sharp = resize(&edge, 4, 1, 1, 8, 1, Filter::Bicubic);
    assert_eq!((sharp[0], sharp[7]), (0, 255));
}

#[test]
fn resize_matches_pillow_on_a_small_image() {
    // Image.frombytes("L", (4, 2), bytes).resize((3, 2), Image.BILINEAR)
    let data = [0u8, 50, 100, 150, 200, 150, 100, 50];
    let resized = resize(&data, 4, 2, 1, 3, 2, Filter::Bilinear);
    assert_eq!(resized, [15, 75, 135, 185, 125, 65]);
}

#[test]
fn crop_and_flips_move_whole_pixels() {
    // 3 by 2, two channels.
    let data = [1u8, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12];
    assert_eq!(crop(&data, 3, 2, 1, 0, 2, 2), [3, 4, 5, 6, 9, 10, 11, 12]);
    assert_eq!(crop(&data, 3, 2, 0, 1, 1, 1), [7, 8]);
    assert_eq!(
        flip_horizontal(&data, 3, 2),
        [5, 6, 3, 4, 1, 2, 11, 12, 9, 10, 7, 8]
    );
    assert_eq!(
        flip_vertical(&data, 3, 2),
        [7, 8, 9, 10, 11, 12, 1, 2, 3, 4, 5, 6]
    );
}

#[test]
fn floats_round_trip_in_both_layouts() {
    // 2 by 1, three channels.
    let data = [0u8, 51, 102, 153, 204, 255];
    let interleaved = to_floats(&data, 3, false);
    assert_eq!(interleaved, [0.0, 0.2, 0.4, 0.6, 0.8, 1.0]);
    let planes = to_floats(&data, 3, true);
    assert_eq!(planes, [0.0, 0.6, 0.2, 0.8, 0.4, 1.0]);
    assert_eq!(from_floats(&interleaved, 3, false), data);
    assert_eq!(from_floats(&planes, 3, true), data);
    assert_eq!(to_floats(&data, 1, true), interleaved);
    assert_eq!(from_floats(&interleaved, 1, true), data);
    let all: Vec<u8> = (0..=255).collect();
    assert_eq!(from_floats(&to_floats(&all, 1, false), 1, false), all);
    assert_eq!(
        from_floats(&[-1.0, 2.0, f32::NAN, 0.5, f32::INFINITY], 1, false),
        [0, 255, 0, 128, 255]
    );
}

#[test]
fn a_decoder_panic_is_reported_as_invalid_data() {
    let error = guarded::<()>(|| panic!("decoder bug")).expect_err("is an error");
    assert_eq!(error.code(), 1);
    assert!(error.to_string().contains("could not read the data"));
    assert_eq!(guarded(|| Ok(7)), Ok(7));
}

/// A smooth picture with some texture, large enough to encode in strips.
fn picture(width: usize, height: usize, channels: usize) -> Vec<u8> {
    let mut data = Vec::with_capacity(width * height * channels);
    for y in 0..height {
        for x in 0..width {
            for channel in 0..channels {
                data.push((90 + (x / 3 + y / 2 + channel * 30) % 120 + (x * y) % 7) as u8);
            }
        }
    }
    data
}

#[test]
fn jpeg_strips_decode_to_the_pixels_of_one_piece() {
    for (width, height, channels) in [(640, 480, 3), (333, 517, 3), (640, 250, 1), (301, 999, 1)] {
        let data = picture(width, height, channels);
        let whole = encode_jpeg_on(width, height, channels, &data, 85, 1).expect("encodes");
        let expected = decode(&whole, NO_LIMIT).expect("decodes");
        for threads in [2, 3, 4, 16, 64] {
            let strips =
                encode_jpeg_on(width, height, channels, &data, 85, threads).expect("encodes");
            assert_ne!(strips, whole, "{width}x{height} on {threads} threads");
            // The restart interval is declared once.
            assert_eq!(
                strips.windows(4).filter(|bytes| *bytes == [0xff, 0xdd, 0x00, 0x04]).count(),
                1
            );
            let head = header(&strips).expect("header reads");
            assert_eq!((head.width, head.height, head.channels), (width, height, channels));
            let pixels = decode(&strips, NO_LIMIT).expect("decodes");
            assert_eq!(pixels, expected, "{width}x{height} on {threads} threads");
        }
    }
}

#[test]
fn small_and_very_wide_jpegs_are_encoded_in_one_piece() {
    let small = picture(100, 100, 3);
    assert_eq!(
        encode_jpeg_on(100, 100, 3, &small, 80, 8).expect("encodes"),
        encode_jpeg_on(100, 100, 3, &small, 80, 1).expect("encodes")
    );
    // Two strips of this image would each hold more units than a restart
    // interval can count.
    let wide = vec![128u8; 65535 * 128];
    assert_eq!(
        encode_jpeg_on(65535, 128, 1, &wide, 50, 2).expect("encodes"),
        encode_jpeg_on(65535, 128, 1, &wide, 50, 1).expect("encodes")
    );
    // The public entry uses the machine's threads and gives a file that
    // decodes.
    let data = picture(640, 480, 3);
    let file = encode_jpeg(640, 480, 3, &data, 85).expect("encodes");
    assert_eq!(decode(&file, NO_LIMIT).expect("decodes").data.len(), data.len());
}

#[test]
fn strips_that_are_not_baseline_jpegs_do_not_join() {
    let data = picture(64, 64, 3);
    let good = encode_jpeg_on(64, 64, 3, &data, 80, 1).expect("encodes");
    assert!(jpeg_layout(&good).is_some());
    assert!(join_jpeg_strips(&[good.clone(), good.clone()], 128, 16).is_some());

    // No end marker.
    let cut = good[..good.len() - 2].to_vec();
    assert!(jpeg_layout(&cut).is_none());
    assert!(join_jpeg_strips(&[good.clone(), cut.clone()], 128, 16).is_none());
    assert!(join_jpeg_strips(&[cut, good.clone()], 128, 16).is_none());
    // Not a marker where one must be.
    let mut broken = good.clone();
    broken[2] = 0x00;
    assert!(jpeg_layout(&broken).is_none());
    // Ends inside the headers.
    assert!(jpeg_layout(&good[..40]).is_none());
    assert!(jpeg_layout(&[0xff, 0xd8]).is_none());
    assert!(jpeg_layout(&[0xff, 0xd8, 0xff]).is_none());
    // A scan with no frame header before it, and a scan header that runs
    // past the end.
    assert!(jpeg_layout(&[0xff, 0xd8, 0xff, 0xda, 0x00, 0x02, 0xff, 0xd9]).is_none());
    assert!(jpeg_layout(&[0xff, 0xd8, 0xff, 0xc0, 0x00, 0x02, 0xff, 0xda, 0x00, 0x09, 0xff, 0xd9])
        .is_none());
}

#[test]
fn resize_handles_every_order_and_tiny_sources() {
    let data = picture(40, 30, 3);
    for filter in [Filter::Bilinear, Filter::Bicubic] {
        for (width, height) in [(20, 15), (80, 15), (20, 60), (80, 60), (7, 29), (39, 3), (1, 1)] {
            let resized = resize(&data, 40, 30, 3, width, height, filter);
            assert_eq!(resized.len(), width * height * 3, "{filter:?}");
            // A resample of values between 90 and 216 stays near them.
            assert!(resized.iter().all(|&value| (70..=236).contains(&value)), "{filter:?}");
        }
        for channels in 1..=4 {
            let one = vec![77u8; channels];
            assert_eq!(
                resize(&one, 1, 1, channels, 5, 4, filter),
                vec![77u8; 20 * channels]
            );
            // The ends keep their side's value; bicubic may sharpen past it.
            let pair = [vec![100u8; channels], vec![150u8; channels]].concat();
            let stretched = resize(&pair, 2, 1, channels, 4, 1, filter);
            assert!(stretched[..channels].iter().all(|&value| (90..=100).contains(&value)));
            assert!(stretched[3 * channels..].iter().all(|&value| (150..=160).contains(&value)));
        }
    }
}

#[test]
fn resize_on_several_threads_gives_the_pixels_of_one() {
    let data = picture(700, 500, 3);
    for filter in [Filter::Bilinear, Filter::Bicubic] {
        for (width, height) in [(350, 250), (1400, 300), (300, 1000), (900, 900)] {
            let one = resize_on(&data, 700, 500, 3, width, height, filter, 1);
            for threads in [2, 3, 7, 64] {
                assert_eq!(
                    resize_on(&data, 700, 500, 3, width, height, filter, threads),
                    one,
                    "{filter:?} to {width}x{height} on {threads} threads"
                );
            }
            assert_eq!(resize(&data, 700, 500, 3, width, height, filter), one);
        }
    }
    assert!(thread_limit() >= 1);
}

#[test]
fn filters_read_only_the_samples_in_reach() {
    // Halving with a triangle reads four samples a pixel, not five.
    let half = axis_weights(1600, 800, Filter::Bilinear);
    assert_eq!(half.taps, 4);
    assert_eq!(&half.weights[4..8], [0.125, 0.375, 0.375, 0.125]);
    assert_eq!(half.starts[1], 1);
    // The first pixel has no sample to its left.
    assert_eq!(half.starts[0], 0);
    // Its three samples share the whole weight: 3/7, 3/7 and 1/7.
    assert!((half.weights[0] - 3.0 / 7.0).abs() < 1e-6 && half.weights[3] == 0.0);
    assert!((half.weights[2] - 1.0 / 7.0).abs() < 1e-6);
    // Doubling reads two.
    let double = axis_weights(4, 8, Filter::Bilinear);
    assert_eq!(double.taps, 2);
    assert_eq!(&double.weights[2..4], [0.75, 0.25]);
    for weights in [half, double, axis_weights(5, 3, Filter::Bicubic)] {
        for position in weights.weights.chunks_exact(weights.taps) {
            assert!((position.iter().sum::<f32>() - 1.0).abs() < 1e-5);
        }
        assert!(weights.starts.iter().all(|start| start + weights.taps <= 1600));
    }
}
