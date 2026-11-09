use super::*;
use crate::json_codec::{self, JsonValue};
use std::borrow::Cow;

fn tape(source: &str) -> Arc<JsonTape> {
    JsonTape::parse(
        source,
        json_codec::MAX_JSON_DEPTH,
        json_codec::MAX_JSON_VALUE_NODES,
    )
    .unwrap_or_else(|| panic!("{source} parses into a tape"))
}

fn root(source: &str) -> JsonNode {
    JsonNode::root(tape(source))
}

fn tree(node: &JsonNode) -> JsonValue {
    node.to_json_value::<()>(0, &mut || Ok(()), &mut |depth| Ok(depth + 1))
        .expect("converts")
}

fn dumps(node: &JsonNode, indent: Option<usize>) -> Option<String> {
    let mut output = BoundedOutput::default();
    let mut remaining = usize::MAX;
    node.write(&mut output, indent, 0, &mut remaining)?;
    Some(output.text)
}

#[test]
fn a_tape_reads_back_the_tree_a_parse_builds() {
    let sources = [
        "null",
        " true ",
        "false",
        "0",
        "-12",
        "123456789012345678",
        "1234567890123456789012",
        "1.0",
        "-0.0",
        "2.5e3",
        "1E-2",
        "\"\"",
        r#""plain""#,
        r#""a\"b\\c\/d\b\f\n\r\t""#,
        r#""\u00e9 and \ud83d\ude00 then text""#,
        "[]",
        "{}",
        "[1, [2, [3, []]], {}]",
        r#"{"b": [1, 2.5, "x", null, true, {}], "a": {"k": false}, "c": "\n"}"#,
        r#"[{"z": 1, "y": [{"x": null}]}, "tail"]"#,
    ];
    for source in sources {
        let node = root(source);
        let expected = json_codec::parse(source).expect("the source parses");
        assert_eq!(tree(&node), expected, "{source}");
        for indent in [None, Some(0), Some(2)] {
            assert_eq!(
                dumps(&node, indent),
                Some(json_codec::dumps(&expected, indent.map(|indent| indent as i64)).unwrap()),
                "{source} {indent:?}"
            );
        }
        assert_eq!(node.span(), node.tape().node_count(), "{source}");
    }
}

#[test]
fn a_tape_refuses_what_it_leaves_to_the_tree_parse() {
    let max = json_codec::MAX_JSON_DEPTH;
    let nodes = json_codec::MAX_JSON_VALUE_NODES;
    for source in [
        "",
        " ",
        "[1,",
        "[1 2]",
        "{\"a\" 1}",
        "{1: 2}",
        "{\"a\": 1,}",
        "{\"a\": 1 \"b\": 2}",
        "tru",
        "nul",
        "fals",
        "01",
        "-",
        "1.",
        "1e",
        "1e+",
        "+1",
        "1e999",
        "\"open",
        "\"bad \\x escape\"",
        "\"\\u12\"",
        "\"\\ud800\"",
        "\"\\ud800\\u0041\"",
        "\"\\udc00\"",
        "\"tab\there\"",
        "\"\\n then \u{1}\"",
        "[] []",
        "@",
        // Repeated keys, in a small object and in a large one.
        r#"{"a": 1, "a": 2}"#,
    ] {
        assert!(JsonTape::parse(source, max, nodes).is_none(), "{source:?}");
    }
    // Keys that share a length and their first bytes are told apart.
    assert!(JsonTape::parse(r#"{"abcdefgX": 1, "abcdefgY": 2}"#, max, nodes).is_some());
    assert!(JsonTape::parse(r#"{"abcdefgX": 1, "abcdefgX": 2}"#, max, nodes).is_none());
    let many: Vec<String> = (0..20).map(|key| format!("\"k{key}\": {key}")).collect();
    let large = format!("{{{}}}", many.join(","));
    assert!(JsonTape::parse(&large, max, nodes).is_some());
    let repeated = format!("{{{}, \"k3\": 0}}", many.join(","));
    assert!(JsonTape::parse(&repeated, max, nodes).is_none());

    // Nesting and value counts past their limits.
    assert!(JsonTape::parse("[[[1]]]", 3, nodes).is_some());
    assert!(JsonTape::parse("[[[1]]]", 2, nodes).is_none());
    assert!(JsonTape::parse("{\"a\": {\"b\": 1}}", 1, nodes).is_none());
    assert!(JsonTape::parse("[1, 2, 3]", max, 4).is_some());
    assert!(JsonTape::parse("[1, 2, 3]", max, 3).is_none());
}

#[test]
fn a_tape_values_payload_is_one_level_of_tape_values() {
    let node =
        root(r#"{"name": "ada", "tags": ["x", "y"], "n": 3, "f": 0.5, "t": true, "z": null}"#);
    assert_eq!(node.tag(), JsonTag::Object);
    assert_eq!(node.index(), 0);
    let Some(Value::Map(fields)) = node.payload() else {
        panic!("an object's payload is a dict");
    };
    assert_eq!(&*fields.key_type, &aura_core::types::Type::named("str"));
    assert_eq!(
        &*fields.value_type,
        &aura_core::types::Type::named("json.Value")
    );
    let keys: Vec<String> = fields
        .entries
        .iter()
        .map(|(key, _)| match key {
            Value::String(text) => text.to_string(),
            other => panic!("a str key, not {other:?}"),
        })
        .collect();
    assert_eq!(keys, ["name", "tags", "n", "f", "t", "z"]);
    let field = |name: &str| -> EnumVariantValue {
        match fields.entries.value_of(&Value::String(Text::from(name))) {
            Some(Value::EnumVariant(variant)) => variant.clone(),
            other => panic!("a variant for {name}, not {other:?}"),
        }
    };
    let name = field("name");
    assert_eq!(name.variant_name.as_str(), "String");
    assert_eq!(name.payloads.json().and_then(JsonNode::as_str), Some("ada"));
    assert_eq!(
        name.payloads.get(0).as_deref(),
        Some(&Value::String(Text::from("ada")))
    );
    let tags = field("tags");
    assert_eq!(tags.variant_name.as_str(), "Array");
    let Some(Cow::Owned(Value::Vec(list))) = tags.payloads.get(0) else {
        panic!("an array's payload is a list");
    };
    assert_eq!(list.elements.len(), 2);
    assert_eq!(
        field("n").payloads.get(0).as_deref(),
        Some(&Value::Int(IntegerValue::from_i64(3)))
    );
    assert_eq!(
        field("f").payloads.get(0).as_deref(),
        Some(&Value::Float(0.5))
    );
    assert_eq!(
        field("t").payloads.get(0).as_deref(),
        Some(&Value::Bool(true))
    );
    let null = field("z");
    assert_eq!(null.variant_name.as_str(), "Null");
    assert!(null.payloads.is_empty());
    assert!(null.payloads.json().is_none());
    // A scalar has no string, and a payload past the first is missing.
    assert_eq!(field("n").payloads.json().and_then(JsonNode::as_str), None);
    assert!(name.payloads.get(1).is_none());
    assert!(root("null").payload().is_none());
    assert_eq!(root("false").payload(), Some(Value::Bool(false)));
}

#[test]
fn a_tape_value_copies_by_sharing_and_writes_by_copying_its_level() {
    let Value::EnumVariant(mut doc) = root(r#"{"a": [1, 2], "b": "x"}"#).into_value() else {
        panic!("a variant");
    };
    assert_eq!(doc.variant_name.as_str(), "Object");
    assert!(doc.payloads.copy_flat(|_| false));
    assert!(doc.payloads.typed().is_none());
    assert!(format!("{:?}", doc.payloads).contains("Map"));
    let copy = Value::EnumVariant(doc.clone()).clone();
    let Value::EnumVariant(copy_variant) = &copy else {
        panic!("a variant");
    };
    let node = doc.payloads.json().expect("a tape value");
    assert!(Arc::ptr_eq(
        node.tape(),
        copy_variant.payloads.json().unwrap().tape()
    ));
    assert!(format!("{node:?}").contains("JsonNode"));
    assert!(format!("{:?}", node.tape()).contains("JsonTape"));
    assert_eq!(&copy_variant.payloads, &doc.payloads);
    assert_eq!(copy_variant.payloads.iter().count(), 1);
    assert!(doc.payloads.take_box(0).is_none());

    // A write makes the payload a value first; the copy keeps the tape.
    let payloads = doc.payloads.values_mut();
    let Value::Map(fields) = &mut payloads[0] else {
        panic!("a dict");
    };
    fields
        .entries
        .insert(Value::String(Text::from("c")), root("true").into_value());
    assert!(doc.payloads.json().is_none());
    assert_ne!(&copy_variant.payloads, &doc.payloads);
    assert_eq!(
        dumps(copy_variant.payloads.json().unwrap(), None).as_deref(),
        Some(r#"{"a":[1,2],"b":"x"}"#)
    );

    // Taking the payload, or all of them, reads it out of the tape.
    let Value::EnumVariant(mut taken) = copy.clone() else {
        panic!("a variant");
    };
    assert!(matches!(taken.payloads.take(0), Some(Value::Map(_))));
    assert_eq!(taken.payloads.get(0).as_deref(), Some(&Value::Unit));
    let Value::EnumVariant(whole) = copy else {
        panic!("a variant");
    };
    assert!(matches!(
        whole.payloads.into_vec().as_slice(),
        [Value::Map(_)]
    ));

    // Equal values in different tapes are equal; different ones are not.
    assert_eq!(root("[1, \"a\"]"), root("[1,\"a\"]"));
    assert_ne!(root("[1]"), root("[2]"));
}

#[test]
fn a_tape_write_stops_at_its_budget_and_depth() {
    let node = root("[1, [2, 3]]");
    let mut output = BoundedOutput::default();
    let mut remaining = 3;
    assert!(node.write(&mut output, None, 0, &mut remaining).is_none());
    let mut remaining = 5;
    let mut output = BoundedOutput::default();
    assert!(node.write(&mut output, None, 0, &mut remaining).is_some());
    assert_eq!(remaining, 0);
    let mut output = BoundedOutput::default();
    let mut remaining = 5;
    let deep = json_codec::MAX_JSON_DEPTH;
    assert!(node
        .write(&mut output, None, deep, &mut remaining)
        .is_none());
    let object = root("{\"a\": 1}");
    let mut output = BoundedOutput::default();
    assert!(object
        .write(&mut output, None, deep, &mut remaining)
        .is_none());

    // The conversion to a tree stops at the first error either hook gives.
    let mut visits = 0;
    let stopped = node.to_json_value(
        0,
        &mut || {
            visits += 1;
            if visits > 2 {
                Err("budget")
            } else {
                Ok(())
            }
        },
        &mut |depth| Ok(depth + 1),
    );
    assert_eq!(stopped, Err("budget"));
    let too_deep = object.to_json_value(0, &mut || Ok(()), &mut |_| Err("depth"));
    assert_eq!(too_deep, Err("depth"));
    assert_eq!(JsonTag::False.variant_name(), "Bool");
}

#[test]
fn a_huge_document_does_not_keep_its_buffers_for_the_next_parse() {
    let huge = format!("[{}]", vec!["0"; SCRATCH_NODES_KEPT + 1].join(","));
    assert_eq!(root(&huge).span(), SCRATCH_NODES_KEPT + 2);
    let kept = SCRATCH.with(|scratch| {
        let buffers = scratch.take();
        let capacity = buffers.0.capacity();
        scratch.set(buffers);
        capacity
    });
    assert!(kept <= SCRATCH_NODES_KEPT, "{kept}");
    // A small one afterwards parses as before.
    assert_eq!(
        tree(&root("[1]")),
        JsonValue::Array(vec![JsonValue::Int(1)])
    );
}

fn object_entries_of(source: &str) -> DictEntries {
    DictEntries::from_json_object(root(source))
}

#[test]
fn an_objects_dict_reads_the_tape_until_a_read_needs_values() {
    let entries = object_entries_of(r#"{"a": 1, "b": "x", "c": [true]}"#);
    assert_eq!(entries.len(), 3);
    assert!(entries.unrealized_json().is_some());
    assert!(entries.is_shared(), "a write copies first");
    let node = entries.unrealized_json().unwrap();
    assert_eq!(node.entry_count(), 3);
    assert!(matches!(
        node.lookup_str("a"),
        Some(Value::EnumVariant(variant)) if variant.variant_name.as_str() == "Int"
    ));
    assert!(node.lookup_str("missing").is_none());
    // Scalars and arrays hold no entries and look nothing up.
    assert_eq!(root("[1, 2]").entry_count(), 2);
    assert_eq!(root("7").entry_count(), 0);
    assert!(root("[1]").lookup_str("a").is_none());
    assert!(root("[1]").object_entries().is_empty());

    // A clone shares the tape; reading entries as values makes them once,
    // for every clone.
    let copy = entries.clone();
    assert_eq!(
        entries.value_of_str("b"),
        Some(&Value::EnumVariant(EnumVariantValue {
            enum_name: RuntimeName::from_static("json.Value"),
            variant_name: RuntimeName::from_static("String"),
            payloads: VariantPayloads::Json(JsonNode {
                tape: Arc::clone(node.tape()),
                index: 4,
            }),
        }))
    );
    assert!(entries.unrealized_json().is_none());
    assert!(copy.unrealized_json().is_none());
    assert_eq!(entries.len(), 3);
    assert_eq!(entries.capacity(), 3);
    assert_eq!(entries.as_ptr(), copy.as_ptr());
    assert!(entries.is_shared());

    // A write takes storage of its own and leaves the other handle as it was.
    let mut written = copy.clone();
    written.insert(Value::String(Text::from("d")), root("null").into_value());
    assert_eq!(written.len(), 4);
    assert_eq!(copy.len(), 3);
    assert_ne!(written.as_ptr(), copy.as_ptr());
    let mut fresh = object_entries_of(r#"{"k": 1}"#);
    fresh.try_reserve(4).expect("room");
    assert!(fresh.unrealized_json().is_none());
    assert_eq!(object_entries_of(r#"{"k": 1}"#).into_vec().len(), 1);
    let mut cleared = object_entries_of(r#"{"k": 1}"#);
    cleared.clear();
    assert!(cleared.is_empty());

    // An object past the index threshold reads its entries by hash.
    let many: Vec<String> = (0..12).map(|key| format!("\"k{key}\": {key}")).collect();
    let large = object_entries_of(&format!("{{{}}}", many.join(",")));
    assert_eq!(large.len(), 12);
    assert!(large.value_of_str("k11").is_some());
    assert!(large.value_of_str("k12").is_none());
}
