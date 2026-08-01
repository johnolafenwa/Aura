use super::*;

const BYTE_CODEC_SAFETY_CEILING: usize = i32::MAX as usize;

fn data_error(error: BytesDataError) -> BytesCodecError {
    BytesCodecError::Data(error)
}

fn allocation_error() -> BytesCodecError {
    BytesCodecError::Resource(BytesResourceError::AllocationFailed)
}

fn output_too_large_error() -> BytesResourceError {
    BytesResourceError::OutputTooLarge {
        maximum: BYTE_CODEC_SAFETY_CEILING,
    }
}

#[test]
fn codec_errors_have_stable_user_facing_messages_and_categories() {
    for (error, expected) in [
        (
            BytesDataError::InvalidUtf8 { index: 7 },
            "invalid UTF-8 at byte 7",
        ),
        (
            BytesDataError::InvalidHexLength { length: 3 },
            "hexadecimal input must contain an even number of bytes, found 3",
        ),
        (
            BytesDataError::InvalidHexDigit {
                index: 2,
                byte: b'?',
            },
            "invalid hexadecimal byte 0x3f at byte 2",
        ),
        (
            BytesDataError::InvalidBase64 { index: 4 },
            "invalid base64 encoding at byte 4",
        ),
    ] {
        assert_eq!(error.to_string(), expected);
        assert_eq!(data_error(error).to_string(), expected);
    }

    let too_large = output_too_large_error();
    assert_eq!(
        too_large.to_string(),
        format!(
            "byte-codec output exceeds Aura's byte-codec safety ceiling of {} bytes",
            BYTE_CODEC_SAFETY_CEILING
        )
    );
    assert_eq!(
        BytesCodecError::Resource(too_large.clone()).to_string(),
        too_large.to_string()
    );
    assert_eq!(
        BytesResourceError::AllocationFailed.to_string(),
        "memory allocation failed while processing bytes"
    );
}

#[test]
fn expanded_length_checks_pin_the_exact_codec_safety_ceiling_without_allocating() {
    let largest_hex_input = BYTE_CODEC_SAFETY_CEILING / 2;
    assert_eq!(
        hex_encoded_len(largest_hex_input),
        Ok(BYTE_CODEC_SAFETY_CEILING - 1)
    );
    assert_eq!(
        hex_encoded_len(largest_hex_input + 1),
        Err(output_too_large_error())
    );
    assert_eq!(hex_encoded_len(usize::MAX), Err(output_too_large_error()));

    let largest_base64_input = (BYTE_CODEC_SAFETY_CEILING / 4) * 3;
    let largest_base64_output = (BYTE_CODEC_SAFETY_CEILING / 4) * 4;
    assert_eq!(
        base64_encoded_len(largest_base64_input),
        Ok(largest_base64_output)
    );
    assert_eq!(
        base64_encoded_len(largest_base64_input + 1),
        Err(output_too_large_error())
    );
    assert_eq!(
        base64_encoded_len(usize::MAX),
        Err(output_too_large_error())
    );
}

#[test]
fn string_conversion_uses_exact_utf8_bytes_and_does_not_modify_the_input() {
    let text = String::from("\u{feff}Aura\0 — café — 🌌");
    let original = text.clone();

    let bytes = string_to_bytes(&text).unwrap();

    assert_eq!(bytes, text.as_bytes());
    assert_eq!(&bytes[..3], &[0xef, 0xbb, 0xbf]);
    assert_eq!(string_from_bytes(&bytes), Ok(text.clone()));
    assert_eq!(text, original);
}

#[test]
fn string_from_bytes_rejects_invalid_utf8_at_the_first_invalid_byte() {
    for (bytes, index) in [
        (vec![0xff], 0),
        (vec![b'a', b'b', 0xff], 2),
        (vec![b'a', 0xf0, 0x9f, 0x8c], 1),
        (vec![b'a', 0xe2, 0x28, 0xa1], 1),
    ] {
        assert_eq!(
            string_from_bytes(&bytes),
            Err(data_error(BytesDataError::InvalidUtf8 { index }))
        );
    }
}

#[test]
fn hex_encoding_is_lowercase_and_decoding_accepts_both_cases() {
    let bytes = [0x00, 0x01, 0x0f, 0x10, 0xab, 0xcd, 0xef, 0xff];

    assert_eq!(hex_encode(&bytes).unwrap(), "00010f10abcdefff");
    assert_eq!(
        hex_decode("00010F10AbCdEfFf"),
        Ok(Vec::from(bytes.as_slice()))
    );
    assert_eq!(hex_encode(&[]), Ok(String::new()));
    assert_eq!(hex_decode(""), Ok(Vec::new()));
}

#[test]
fn hex_decoding_is_strict_and_reports_byte_offsets_before_allocating() {
    assert_eq!(
        with_allocation_budget(0, || hex_decode("abc")),
        Err(data_error(BytesDataError::InvalidHexLength { length: 3 }))
    );
    assert_eq!(
        with_allocation_budget(0, || hex_decode("?")),
        Err(data_error(BytesDataError::InvalidHexLength { length: 1 }))
    );
    assert_eq!(
        with_allocation_budget(0, || hex_decode("0x00")),
        Err(data_error(BytesDataError::InvalidHexDigit {
            index: 1,
            byte: b'x',
        }))
    );
    assert_eq!(
        hex_decode("00 0"),
        Err(data_error(BytesDataError::InvalidHexDigit {
            index: 2,
            byte: b' ',
        }))
    );
    assert_eq!(
        hex_decode("é"),
        Err(data_error(BytesDataError::InvalidHexDigit {
            index: 0,
            byte: 0xc3,
        }))
    );
}

#[test]
fn every_byte_round_trips_through_hex() {
    let bytes: Vec<u8> = (u8::MIN..=u8::MAX).collect();

    assert_eq!(hex_decode(&hex_encode(&bytes).unwrap()), Ok(bytes));
}

#[test]
fn base64_matches_the_rfc_4648_vectors() {
    for (plain, encoded) in [
        ("", ""),
        ("f", "Zg=="),
        ("fo", "Zm8="),
        ("foo", "Zm9v"),
        ("foob", "Zm9vYg=="),
        ("fooba", "Zm9vYmE="),
        ("foobar", "Zm9vYmFy"),
    ] {
        assert_eq!(base64_encode(plain.as_bytes()), Ok(encoded.to_string()));
        assert_eq!(base64_decode(encoded), Ok(plain.as_bytes().to_vec()));
    }
}

#[test]
fn base64_decoder_allocates_only_the_exact_decoded_length() {
    for (encoded, decoded_len) in [
        ("", 0),
        ("Zg==", 1),
        ("Zm8=", 2),
        ("Zm9v", 3),
        ("Zm9vYg==", 4),
    ] {
        assert_eq!(
            validate_base64(encoded).unwrap().buffer_len,
            decoded_len,
            "unexpected decode buffer length for {encoded:?}"
        );
    }
}

#[test]
fn base64_round_trips_arbitrary_bytes() {
    let bytes: Vec<u8> = (u8::MIN..=u8::MAX).collect();
    let encoded = base64_encode(&bytes).unwrap();

    assert_eq!(base64_decode(&encoded), Ok(bytes));
}

#[test]
fn base64_decoding_requires_standard_canonical_encoding_and_exact_offsets() {
    for (encoded, index) in [
        ("Zg", 2),
        ("Zg=", 3),
        ("AA=", 3),
        ("AAA==", 4),
        ("AAA===", 4),
        ("-w==", 0),
        ("Zm9v\n", 4),
        ("Zg===", 4),
        ("Zh==", 1),
        ("Zm9=", 2),
        ("Zg==A", 4),
        ("=AAA", 0),
        ("====", 0),
    ] {
        assert_eq!(
            with_allocation_budget(0, || base64_decode(encoded)),
            Err(data_error(BytesDataError::InvalidBase64 { index })),
            "unexpected result for {encoded:?}"
        );
    }
}

#[test]
fn sha256_returns_raw_standard_digest_bytes() {
    let empty = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";
    let abc = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";

    assert_eq!(sha256_bytes(&[]).unwrap().len(), 32);
    assert_eq!(hex_encode(&sha256_bytes(&[]).unwrap()).unwrap(), empty);
    assert_eq!(hex_encode(&sha256_bytes(b"abc").unwrap()).unwrap(), abc);
    assert_eq!(sha256_string("abc"), sha256_bytes(b"abc"));
}

#[test]
fn hashing_a_string_uses_its_utf8_representation() {
    let text = "café 🌌";

    assert_eq!(
        sha256_string(text),
        sha256_bytes(&string_to_bytes(text).unwrap())
    );
}

#[test]
fn every_allocating_operation_reports_a_resource_failure_without_partial_output() {
    assert_eq!(
        with_allocation_budget(0, || string_to_bytes("a")),
        Err(allocation_error())
    );
    assert_eq!(
        with_allocation_budget(0, || string_from_bytes(b"a")),
        Err(allocation_error())
    );
    assert_eq!(
        with_allocation_budget(0, || hex_encode(&[0])),
        Err(allocation_error())
    );
    assert_eq!(
        with_allocation_budget(0, || hex_decode("00")),
        Err(allocation_error())
    );
    assert_eq!(
        with_allocation_budget(0, || base64_encode(b"a")),
        Err(allocation_error())
    );
    assert_eq!(
        with_allocation_budget(0, || base64_decode("YQ==")),
        Err(allocation_error())
    );
    assert_eq!(
        with_allocation_budget(0, || sha256_bytes(b"a")),
        Err(allocation_error())
    );
    assert_eq!(
        with_allocation_budget(0, || sha256_string("a")),
        Err(allocation_error())
    );

    assert_eq!(
        with_allocation_budget(0, || string_to_bytes("")),
        Ok(Vec::new())
    );
    assert_eq!(
        with_allocation_budget(0, || hex_encode(&[])),
        Ok(String::new())
    );
}

#[test]
fn allocation_budget_and_host_capacity_failures_use_the_same_resource_category() {
    assert_eq!(
        with_allocation_budget(1, || string_to_bytes("a")),
        Ok(vec![b'a'])
    );
    assert_eq!(
        try_byte_buffer(usize::MAX),
        Err(BytesResourceError::AllocationFailed)
    );
}
