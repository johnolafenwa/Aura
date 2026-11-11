use super::*;

fn sample() -> Vec<u8> {
    (0..50_000u32)
        .flat_map(|index| format!("row {} of the sample\n", index % 97).into_bytes())
        .collect()
}

#[test]
fn compress_then_decompress_returns_the_input_at_every_level() {
    let data = sample();
    for level in 0..=9 {
        let packed = gzip_compress(&data, level).expect("compression should succeed");
        assert_eq!(&packed[..2], &[0x1f, 0x8b], "level {level}");
        assert_eq!(gzip_decompress(&packed, data.len()), Ok(data.clone()));
    }
    let stored = gzip_compress(&data, 0).unwrap();
    let smallest = gzip_compress(&data, 9).unwrap();
    assert!(stored.len() > data.len());
    assert!(smallest.len() < data.len() / 10);
}

#[test]
fn empty_data_round_trips() {
    let packed = gzip_compress(&[], 6).unwrap();
    assert_eq!(packed.len(), 20);
    assert_eq!(gzip_decompress(&packed, 0), Ok(Vec::new()));
}

#[test]
fn decompress_reads_a_stream_another_tool_wrote() {
    // Python: gzip.compress(b'hello aura\n', mtime=0)
    let packed = [
        0x1f, 0x8b, 0x08, 0x00, 0x00, 0x00, 0x00, 0x00, 0x02, 0xff, 0xcb, 0x48, 0xcd, 0xc9, 0xc9,
        0x57, 0x48, 0x2c, 0x2d, 0x4a, 0xe4, 0x02, 0x00, 0x4e, 0xf6, 0xea, 0xc2, 0x0b, 0x00, 0x00,
        0x00,
    ];
    assert_eq!(gzip_decompress(&packed, 64), Ok(b"hello aura\n".to_vec()));
}

#[test]
fn decompress_joins_every_member_of_the_stream() {
    let mut packed = gzip_compress(b"first ", 6).unwrap();
    packed.extend(gzip_compress(b"second", 1).unwrap());
    assert_eq!(gzip_decompress(&packed, 64), Ok(b"first second".to_vec()));
}

#[test]
fn decompress_stops_at_the_limit() {
    let data = vec![0u8; 1 << 20];
    let packed = gzip_compress(&data, 6).unwrap();
    assert_eq!(gzip_decompress(&packed, data.len()), Ok(data.clone()));
    assert_eq!(
        gzip_decompress(&packed, data.len() - 1),
        Err(GzipError::OutputTooLarge {
            limit_bytes: data.len() - 1
        })
    );
    assert_eq!(
        gzip_decompress(&packed, 0),
        Err(GzipError::OutputTooLarge { limit_bytes: 0 })
    );
    assert_eq!(
        GzipError::OutputTooLarge { limit_bytes: 10 }.to_string(),
        "gzip output exceeds the 10-byte limit"
    );
}

#[test]
fn decompress_rejects_data_that_is_not_a_whole_gzip_stream() {
    let packed = gzip_compress(&sample(), 6).unwrap();
    let invalid = |data: &[u8]| match gzip_decompress(data, 1 << 30) {
        Err(GzipError::InvalidData(message)) => message,
        other => panic!("expected invalid data, found {other:?}"),
    };
    assert_eq!(invalid(&[]), "the input is empty");
    invalid(b"plain text, not gzip");
    invalid(&packed[..packed.len() / 2]);
    invalid(&packed[..packed.len() - 1]);
    let mut wrong_checksum = packed.clone();
    let checksum_at = wrong_checksum.len() - 8;
    wrong_checksum[checksum_at] ^= 0xff;
    invalid(&wrong_checksum);
    let mut wrong_body = packed.clone();
    wrong_body[packed.len() / 2] ^= 0x55;
    invalid(&wrong_body);
    let mut trailing = packed;
    trailing.extend_from_slice(b"junk after the stream");
    let message = invalid(&trailing);
    assert!(GzipError::InvalidData(message.clone())
        .to_string()
        .starts_with("invalid gzip data: "));
}
