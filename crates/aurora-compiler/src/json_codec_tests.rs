use super::*;

fn parse_ok(source: &str) -> JsonValue {
    parse(source).unwrap_or_else(|error| panic!("`{source}` should parse: {error:?}"))
}

#[test]
fn codec_errors_have_stable_user_facing_messages() {
    for (error, expected) in [
        (
            JsonCodecError::Syntax {
                message: "expected value".to_string(),
                line: 3,
                column: 7,
                offset: 41,
            },
            "JSON syntax error at 3:7: expected value",
        ),
        (
            JsonCodecError::NumberOutOfRange {
                line: 4,
                column: 2,
                offset: 52,
            },
            "JSON number at 4:2 is outside float64 range",
        ),
        (
            JsonCodecError::NestingTooDeep {
                limit: 128,
                line: 5,
                column: 9,
                offset: 63,
            },
            "JSON nesting at 5:9 exceeds the maximum depth of 128",
        ),
        (
            JsonCodecError::NestingTooDeep {
                limit: 128,
                line: 0,
                column: 0,
                offset: 0,
            },
            "JSON value exceeds the maximum depth of 128",
        ),
        (
            JsonCodecError::InputTooLarge {
                actual_bytes: 70,
                limit_bytes: 64,
            },
            "JSON input is 70 bytes, exceeding the 64-byte limit",
        ),
        (
            JsonCodecError::InvalidIndent {
                indent: 17,
                maximum: 16,
            },
            "JSON indent must be between 0 and 16, found 17",
        ),
        (
            JsonCodecError::NonFiniteNumber,
            "JSON cannot encode NaN or an infinite float",
        ),
        (
            JsonCodecError::OutputTooLarge { limit_bytes: 64 },
            "JSON output exceeds the 64-byte limit",
        ),
        (
            JsonCodecError::MaterializationTooLarge { limit: 262_144 },
            "JSON value exceeds the maximum materialized node count of 262144",
        ),
        (
            JsonCodecError::AllocationFailed,
            "memory allocation failed while processing JSON",
        ),
    ] {
        assert_eq!(error.to_string(), expected);
    }
}

#[test]
fn exact_number_lexemes_select_int_or_float_without_losing_i64_boundaries() {
    for (source, expected) in [
        ("0", JsonValue::Int(0)),
        ("-0", JsonValue::Int(0)),
        ("9223372036854775807", JsonValue::Int(i64::MAX)),
        ("-9223372036854775808", JsonValue::Int(i64::MIN)),
        ("1.0", JsonValue::Int(1)),
        ("1e0", JsonValue::Int(1)),
        ("1e3", JsonValue::Int(1000)),
        ("1.5e1", JsonValue::Int(15)),
        ("10e-1", JsonValue::Int(1)),
        ("1e-1", JsonValue::Float(0.1)),
        ("-0.0", JsonValue::Int(0)),
        ("9223372036854775807.0", JsonValue::Int(i64::MAX)),
        ("-9223372036854775808.0", JsonValue::Int(i64::MIN)),
        (
            "9223372036854775808",
            JsonValue::Float(9_223_372_036_854_776_000.0),
        ),
    ] {
        assert_eq!(parse_ok(source), expected, "wrong class for `{source}`");
    }

    assert!(matches!(
        parse_ok("9007199254740991.1"),
        JsonValue::Float(_)
    ));
}

#[test]
fn parse_materializes_every_json_value_kind_and_preserves_object_order() {
    assert_eq!(
        parse_ok(r#"{"second":[null,true,false,"text"],"first":0}"#),
        JsonValue::Object(vec![
            (
                "second".to_string(),
                JsonValue::Array(vec![
                    JsonValue::Null,
                    JsonValue::Bool(true),
                    JsonValue::Bool(false),
                    JsonValue::String("text".to_string()),
                ]),
            ),
            ("first".to_string(), JsonValue::Int(0)),
        ])
    );
}

#[test]
fn exponent_lexemes_beyond_i128_preserve_overflow_and_underflow_semantics() {
    let exponent = "9".repeat(80);

    for source in [format!("1e{exponent}"), format!("1e+{exponent}")] {
        assert_eq!(
            parse(&source).unwrap_err(),
            JsonCodecError::NumberOutOfRange {
                line: 1,
                column: 1,
                offset: 0,
            },
            "`{source}` must not be accepted as a finite float"
        );
    }

    for source in [
        format!("1e-{exponent}"),
        format!("-1e-{exponent}"),
        "1e-9999".to_string(),
    ] {
        let JsonValue::Float(value) = parse_ok(&source) else {
            panic!("`{source}` should underflow to a finite float");
        };
        assert!(
            value == 0.0 && value.is_finite(),
            "`{source}` should underflow to finite zero"
        );
        assert_eq!(
            value.is_sign_negative(),
            source.starts_with('-'),
            "`{source}` should preserve the sign of its underflowed zero"
        );
    }

    assert_eq!(
        parse_ok(&format!("0e{exponent}")),
        JsonValue::Int(0),
        "an exact zero remains integral even when its exponent exceeds i128"
    );
}

#[test]
fn parse_errors_are_structured_with_exact_source_locations() {
    let syntax = parse("{\n  \"ok\": true,\n  ]").unwrap_err();
    assert!(matches!(
        syntax,
        JsonCodecError::Syntax {
            line: 3,
            column: 3,
            ..
        }
    ));

    let number = parse("{\"n\": 1e400}").unwrap_err();
    assert_eq!(
        number,
        JsonCodecError::NumberOutOfRange {
            line: 1,
            column: 7,
            offset: 6,
        }
    );

    let unicode = parse(r#"{"é": ]}"#).unwrap_err();
    let JsonCodecError::Syntax {
        message,
        line,
        column,
        offset,
    } = unicode
    else {
        panic!("expected a syntax error");
    };
    assert_eq!((line, column, offset), (1, 7, 7));
    assert_eq!(message, "expected value");

    assert_eq!(
        parse(r#"{"é":1e400}"#).unwrap_err(),
        JsonCodecError::NumberOutOfRange {
            line: 1,
            column: 6,
            offset: 6,
        }
    );
}

#[test]
fn eof_syntax_errors_point_immediately_after_the_final_source_scalar() {
    assert_eq!(
        parse(r#"{"x":"#).unwrap_err(),
        JsonCodecError::Syntax {
            message: "EOF while parsing a value".to_string(),
            line: 1,
            column: 6,
            offset: 5,
        }
    );
    assert_eq!(
        parse(r#"{"é":"#).unwrap_err(),
        JsonCodecError::Syntax {
            message: "EOF while parsing a value".to_string(),
            line: 1,
            column: 6,
            offset: 6,
        }
    );
}

#[test]
fn syntax_errors_take_precedence_over_later_number_like_text() {
    assert!(matches!(
        parse("x1e400"),
        Err(JsonCodecError::Syntax {
            line: 1,
            column: 1,
            offset: 0,
            ..
        })
    ));
}

#[test]
fn depth_limit_precedes_later_parse_validation() {
    let source = format!("x{}", "[".repeat(MAX_JSON_DEPTH + 1));
    assert_eq!(
        parse(&source).unwrap_err(),
        JsonCodecError::NestingTooDeep {
            limit: MAX_JSON_DEPTH,
            line: 1,
            column: 130,
            offset: 129,
        }
    );
}

#[test]
fn input_and_depth_limits_accept_the_exact_boundary_without_large_allocations() {
    assert_eq!(check_input_len(MAX_JSON_INPUT_BYTES), Ok(()));
    assert_eq!(
        check_input_len(MAX_JSON_INPUT_BYTES + 1),
        Err(JsonCodecError::InputTooLarge {
            actual_bytes: (MAX_JSON_INPUT_BYTES + 1) as u64,
            limit_bytes: MAX_JSON_INPUT_BYTES as u64,
        })
    );

    let at_limit = format!(
        "{}0{}",
        "[".repeat(MAX_JSON_DEPTH),
        "]".repeat(MAX_JSON_DEPTH)
    );
    assert!(parse(&at_limit).is_ok());
    let beyond = format!(
        "{}0{}",
        "[".repeat(MAX_JSON_DEPTH + 1),
        "]".repeat(MAX_JSON_DEPTH + 1)
    );
    assert!(matches!(
        parse(&beyond),
        Err(JsonCodecError::NestingTooDeep {
            limit: MAX_JSON_DEPTH,
            line: 1,
            column: 129,
            offset: 128,
        })
    ));
}

#[test]
fn materialized_value_node_limit_accepts_the_exact_boundary_and_rejects_the_next_node() {
    fn flat_null_array(element_count: usize) -> String {
        let mut source = String::with_capacity(element_count.saturating_mul(5).saturating_add(2));
        source.push('[');
        for index in 0..element_count {
            if index > 0 {
                source.push(',');
            }
            source.push_str("null");
        }
        source.push(']');
        source
    }

    let at_limit = flat_null_array(MAX_JSON_VALUE_NODES - 1);
    let JsonValue::Array(values) =
        parse(&at_limit).expect("the exact materialized-node boundary should parse")
    else {
        panic!("expected an array at the node boundary");
    };
    assert_eq!(values.len() + 1, MAX_JSON_VALUE_NODES);

    let beyond_limit = flat_null_array(MAX_JSON_VALUE_NODES);
    assert_eq!(
        parse(&beyond_limit).unwrap_err(),
        JsonCodecError::MaterializationTooLarge {
            limit: MAX_JSON_VALUE_NODES,
        }
    );
}

#[test]
fn full_parse_and_dumps_entry_points_enforce_exact_byte_cap_boundaries() {
    let mut input_bytes = Vec::with_capacity(MAX_JSON_INPUT_BYTES + 1);
    input_bytes.resize(MAX_JSON_INPUT_BYTES, b' ');
    input_bytes[..4].copy_from_slice(b"null");
    let mut input = String::from_utf8(input_bytes).expect("test input is ASCII");

    assert_eq!(
        parse(&input).expect("the exact input-byte boundary should parse"),
        JsonValue::Null
    );
    input.push(' ');
    assert_eq!(
        parse(&input).unwrap_err(),
        JsonCodecError::InputTooLarge {
            actual_bytes: (MAX_JSON_INPUT_BYTES + 1) as u64,
            limit_bytes: MAX_JSON_INPUT_BYTES as u64,
        }
    );
    drop(input);

    let payload_bytes = MAX_JSON_OUTPUT_BYTES
        .checked_sub(2)
        .expect("the output cap leaves room for JSON string quotes");
    let mut string_bytes = Vec::with_capacity(payload_bytes + 1);
    string_bytes.resize(payload_bytes, b'x');
    let mut value =
        JsonValue::String(String::from_utf8(string_bytes).expect("test payload is ASCII"));

    let output = dumps(&value, None).expect("the exact output-byte boundary should serialize");
    assert_eq!(output.len(), MAX_JSON_OUTPUT_BYTES);
    assert_eq!(output.as_bytes().first(), Some(&b'"'));
    assert_eq!(output.as_bytes().last(), Some(&b'"'));
    drop(output);

    if let JsonValue::String(payload) = &mut value {
        payload.push('x');
    } else {
        unreachable!("the test constructs a JSON string");
    }
    assert_eq!(
        dumps(&value, None).unwrap_err(),
        JsonCodecError::OutputTooLarge {
            limit_bytes: MAX_JSON_OUTPUT_BYTES as u64,
        }
    );

    if let JsonValue::String(payload) = &mut value {
        payload.truncate(MAX_JSON_OUTPUT_BYTES - 4);
    } else {
        unreachable!("the test constructs a JSON string");
    }
    let numeric_overflow = JsonValue::Array(vec![value, JsonValue::Int(0)]);
    assert_eq!(
        dumps(&numeric_overflow, None).unwrap_err(),
        JsonCodecError::OutputTooLarge {
            limit_bytes: MAX_JSON_OUTPUT_BYTES as u64,
        },
        "the output limit must also be enforced while formatting a number"
    );
}

#[test]
fn duplicate_object_keys_keep_the_first_slot_and_last_value() {
    let JsonValue::Object(entries) = parse_ok(r#"{"b":1,"a":2,"\u0062":3}"#) else {
        panic!("expected object");
    };
    assert_eq!(
        entries,
        vec![
            ("b".to_string(), JsonValue::Int(3)),
            ("a".to_string(), JsonValue::Int(2)),
        ]
    );
}

#[test]
fn empty_objects_parse_as_empty_objects() {
    assert_eq!(parse_ok("{}"), JsonValue::Object(Vec::new()));
    assert_eq!(parse_ok("{ \n }"), JsonValue::Object(Vec::new()));
}

#[test]
fn large_unique_objects_use_one_hash_index_probe_per_entry() {
    const ENTRY_COUNT: usize = 20_000;

    let mut source = String::with_capacity(ENTRY_COUNT * 16);
    source.push('{');
    for index in 0..ENTRY_COUNT {
        if index > 0 {
            source.push(',');
        }
        source.push('"');
        source.push_str("unique_");
        source.push_str(&index.to_string());
        source.push_str("\":");
        source.push_str(&index.to_string());
    }
    source.push('}');

    reset_object_key_probe_count();
    let JsonValue::Object(entries) = parse_ok(&source) else {
        panic!("expected a large object");
    };
    let probes = object_key_probe_count();

    assert_eq!(entries.len(), ENTRY_COUNT);
    assert_eq!(entries[0].0, "unique_0");
    assert_eq!(entries[ENTRY_COUNT - 1].0, "unique_19999");
    assert!(
        probes == ENTRY_COUNT,
        "{ENTRY_COUNT} unique keys required {probes} hash-index probes"
    );
}

#[test]
fn tree_construction_allocation_failures_keep_the_allocation_error_category() {
    assert_eq!(
        JsonCodecError::AllocationFailed.to_string(),
        "memory allocation failed while processing JSON"
    );
    let serde_error =
        <serde_json::Error as serde::de::Error>::custom(JSON_ALLOCATION_FAILED_SENTINEL);
    assert_eq!(
        deserialize_error("null", serde_error),
        JsonCodecError::AllocationFailed
    );

    assert_eq!(
        ObjectBuilder::try_with_capacity(usize::MAX)
            .map(|_| ())
            .map_err(allocation_error)
            .unwrap_err(),
        JsonCodecError::AllocationFailed
    );
    assert_eq!(
        try_vec_with_capacity::<JsonValue>(usize::MAX)
            .map(|_| ())
            .map_err(allocation_error)
            .unwrap_err(),
        JsonCodecError::AllocationFailed
    );
    assert_eq!(
        try_owned_string_with_capacity("", usize::MAX)
            .map(|_| ())
            .map_err(allocation_error)
            .unwrap_err(),
        JsonCodecError::AllocationFailed
    );
}

#[test]
fn decoder_adapter_diagnostics_remain_specific_and_source_anchored() {
    struct JsonValueExpectation;

    impl std::fmt::Display for JsonValueExpectation {
        fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            let mut budget = JsonMaterializationBudget::new();
            <JsonValueVisitor<'_> as serde::de::Visitor<'_>>::expecting(
                &JsonValueVisitor {
                    budget: &mut budget,
                },
                formatter,
            )
        }
    }

    struct FallibleStringExpectation;

    impl std::fmt::Display for FallibleStringExpectation {
        fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            <FallibleStringVisitor as serde::de::Visitor<'_>>::expecting(
                &FallibleStringVisitor,
                formatter,
            )
        }
    }

    struct JsonMapKeyExpectation;

    impl std::fmt::Display for JsonMapKeyExpectation {
        fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            <JsonMapKeyVisitor as serde::de::Visitor<'_>>::expecting(&JsonMapKeyVisitor, formatter)
        }
    }

    assert_eq!(JsonValueExpectation.to_string(), "a JSON value");
    assert_eq!(FallibleStringExpectation.to_string(), "a JSON string");
    assert_eq!(
        JsonMapKeyExpectation.to_string(),
        "a JSON object key or arbitrary-precision number marker"
    );

    let invalid_marker = <JsonMapKeyVisitor as serde::de::Visitor<'_>>::visit_str::<
        serde::de::value::Error,
    >(JsonMapKeyVisitor, "not-the-private-number-marker");
    let invalid_marker = match invalid_marker {
        Ok(_) => panic!("only serde_json's private number marker is valid in this adapter"),
        Err(error) => error,
    };
    assert_eq!(
        invalid_marker.to_string(),
        "invalid serde_json arbitrary-precision number marker"
    );

    let upstream =
        <serde_json::Error as serde::de::Error>::custom("unexpected decoder adapter failure");
    assert_eq!(
        deserialize_error("null", upstream),
        JsonCodecError::Syntax {
            message: "unexpected decoder adapter failure".to_string(),
            line: 1,
            column: 5,
            offset: 4,
        },
        "a decoder-stage error without a serde location must anchor after the source"
    );
}

#[test]
fn serde_arbitrary_precision_marker_is_preserved_as_a_real_object_key() {
    const MARKER: &str = "$serde_json::private::Number";
    let source = r#"{"$serde_json::private::Number":"123","other":4}"#;

    assert_eq!(
        parse_ok(source),
        JsonValue::Object(vec![
            (MARKER.to_string(), JsonValue::String("123".to_string())),
            ("other".to_string(), JsonValue::Int(4)),
        ])
    );

    let marker_only = JsonValue::object(vec![(
        MARKER.to_string(),
        JsonValue::String("-9223372036854775808".to_string()),
    )]);
    assert_eq!(
        parse_ok(&dumps(&marker_only, None).unwrap()),
        marker_only,
        "the reserved-looking key must round-trip as ordinary user data"
    );
}

#[test]
fn dumps_is_sorted_compact_or_exactly_indented_and_preserves_integer_spelling() {
    let value = JsonValue::object(vec![
        ("z".to_string(), JsonValue::Int(1)),
        (
            "a".to_string(),
            JsonValue::Array(vec![JsonValue::Bool(true), JsonValue::Float(1.5)]),
        ),
    ]);
    assert_eq!(dumps(&value, None).unwrap(), r#"{"a":[true,1.5],"z":1}"#);
    assert_eq!(
        dumps(&value, Some(2)).unwrap(),
        "{\n  \"a\": [\n    true,\n    1.5\n  ],\n  \"z\": 1\n}"
    );
    assert_eq!(dumps(&JsonValue::Float(1.0), None).unwrap(), "1.0");
    assert_eq!(dumps(&JsonValue::Float(-0.0), None).unwrap(), "-0.0");
    assert_eq!(
        dumps(&JsonValue::String("é\"\\\n\u{0001}/".to_string()), None).unwrap(),
        "\"é\\\"\\\\\\n\\u0001/\""
    );
    assert_eq!(
        dumps(&JsonValue::Array(vec![JsonValue::Null]), Some(0)).unwrap(),
        "[\nnull\n]"
    );
    assert_eq!(
        dumps(&JsonValue::object(Vec::new()), Some(MAX_JSON_INDENT)).unwrap(),
        "{}"
    );
    assert_eq!(
        dumps(
            &JsonValue::Object(vec![
                ("a".to_string(), JsonValue::Int(1)),
                ("b".to_string(), JsonValue::Int(2)),
                ("a".to_string(), JsonValue::Int(3)),
                ("b".to_string(), JsonValue::Int(3)),
            ]),
            None,
        )
        .unwrap(),
        r#"{"a":3,"b":3}"#,
        "sorting must retain the final value for duplicate first and later key groups"
    );
}

#[test]
fn iterative_dumps_handles_empty_and_nonempty_containers_in_both_layouts() {
    let value = JsonValue::Array(vec![
        JsonValue::Array(Vec::new()),
        JsonValue::Object(Vec::new()),
        JsonValue::Array(vec![JsonValue::Null, JsonValue::Bool(false)]),
        JsonValue::Object(vec![(
            "key".to_string(),
            JsonValue::String("value".to_string()),
        )]),
    ]);

    assert_eq!(
        dumps(&value, None).unwrap(),
        r#"[[],{},[null,false],{"key":"value"}]"#
    );
    assert_eq!(
        dumps(&value, Some(2)).unwrap(),
        "[\n  [],\n  {},\n  [\n    null,\n    false\n  ],\n  {\n    \"key\": \"value\"\n  }\n]"
    );
}

#[test]
fn dumps_uses_canonical_escapes_for_named_and_other_control_characters() {
    for (value, expected) in [
        ("\u{0008}", r#""\b""#),
        ("\u{000c}", r#""\f""#),
        ("\r", r#""\r""#),
        ("\t", r#""\t""#),
        ("\u{0000}x", r#""\u0000x""#),
        ("x\u{0001}y", r#""x\u0001y""#),
        ("x\u{001f}", r#""x\u001f""#),
    ] {
        assert_eq!(
            dumps(&JsonValue::String(value.to_string()), None).unwrap(),
            expected,
            "wrong JSON escape for {value:?}"
        );
    }
}

#[test]
fn dumps_rejects_invalid_indent_nonfinite_values_depth_and_output_overflow() {
    for indent in [-1, 17] {
        assert_eq!(
            dumps(&JsonValue::Null, Some(indent)).unwrap_err(),
            JsonCodecError::InvalidIndent {
                indent,
                maximum: MAX_JSON_INDENT,
            }
        );
    }
    assert_eq!(
        dumps(&JsonValue::Float(f64::NAN), None).unwrap_err(),
        JsonCodecError::NonFiniteNumber
    );
    for value in [f64::INFINITY, f64::NEG_INFINITY] {
        assert_eq!(
            dumps(&JsonValue::Float(value), Some(2)).unwrap_err(),
            JsonCodecError::NonFiniteNumber
        );
    }

    let mut too_deep = JsonValue::Null;
    for _ in 0..=MAX_JSON_DEPTH {
        too_deep = JsonValue::Array(vec![too_deep]);
    }
    assert_eq!(
        dumps(&too_deep, None).unwrap_err(),
        JsonCodecError::NestingTooDeep {
            limit: MAX_JSON_DEPTH,
            line: 0,
            column: 0,
            offset: 0,
        }
    );

    assert_eq!(check_output_len(MAX_JSON_OUTPUT_BYTES, 0), Ok(()));
    assert_eq!(
        check_output_len(MAX_JSON_OUTPUT_BYTES, 1),
        Err(JsonCodecError::OutputTooLarge {
            limit_bytes: MAX_JSON_OUTPUT_BYTES as u64,
        })
    );

    let mut impossible = String::new();
    let host_error = impossible.try_reserve(usize::MAX).unwrap_err();
    assert_eq!(
        allocation_error(host_error),
        JsonCodecError::AllocationFailed
    );
}

#[test]
fn dump_object_sort_scratch_allocation_failure_is_reported() {
    assert_eq!(
        sorted_object_entries_with_capacity(&[], usize::MAX).unwrap_err(),
        JsonCodecError::AllocationFailed
    );
}

#[test]
fn bounded_numeric_writer_flush_is_transparent_and_preserves_followup_writes() {
    let mut output = BoundedOutput::default();
    let mut writer = BoundedIoWriter {
        output: &mut output,
        failure: None,
    };

    std::io::Write::write_all(&mut writer, b"12").unwrap();
    std::io::Write::flush(&mut writer).unwrap();
    std::io::Write::write_all(&mut writer, b"3").unwrap();
    drop(writer);

    assert_eq!(output.text, "123");
}
