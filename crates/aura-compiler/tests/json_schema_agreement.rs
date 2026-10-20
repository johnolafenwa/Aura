//! Checks that each derived JSON Schema accepts exactly the inputs its
//! derived decoder accepts.
//!
//! Each case names a codec fixture, a type in it, and one valid input. The
//! test mutates the input at every position, decodes every variant with the
//! compiled program, and validates it against the type's derived schema with
//! the small JSON Schema 2020-12 validator below. The two verdicts must match.

use std::fs;
use std::path::Path;
use std::thread;

use serde_json::{json, Map, Value};

struct Case {
    fixture: &'static str,
    type_name: &'static str,
    sample: &'static str,
    /// Keys whose values the test leaves alone, because their type has an
    /// explicit codec that may be stricter than its schema.
    keep: &'static [&'static str],
}

const CASES: &[Case] = &[
    Case {
        fixture: "crates/aura-compiler/tests/fixtures/run-pass/json_codec_derived.au",
        type_name: "Scalars",
        sample: r#"{"flag": true, "small": -5, "medium": 70000, "large": 9000000000, "count": 65535, "ratio": 2.5, "narrow": 0.5, "name": "mixed", "raw": [null]}"#,
        keep: &[],
    },
    Case {
        fixture: "crates/aura-compiler/tests/fixtures/run-pass/json_codec_derived.au",
        type_name: "Node",
        sample: r#"{"name": "root", "span": [1, 2], "tags": [{"label": "a", "aliases": ["b"], "single": ["c"], "weight": 9}], "scores": {"x": 1.5}, "price": null, "next": {"name": "leaf", "span": [3, 4], "next": null}}"#,
        keep: &["price"],
    },
    Case {
        fixture: "crates/aura-compiler/tests/fixtures/run-pass/json_codec_derived.au",
        type_name: "Empty",
        sample: "{}",
        keep: &[],
    },
    Case {
        fixture: "crates/aura-compiler/tests/fixtures/run-pass/json_codec_derived_enums.au",
        type_name: "Level",
        sample: r#""Medium""#,
        keep: &[],
    },
    Case {
        fixture: "crates/aura-compiler/tests/fixtures/run-pass/json_codec_derived_enums.au",
        type_name: "Shape",
        sample: r#"{"type": "Group", "name": "g", "members": [{"type": "Circle", "radius": 1.5}, {"type": "Rect", "width": 2, "height": 3}, {"type": "Empty"}]}"#,
        keep: &[],
    },
    Case {
        fixture: "crates/aura-compiler/tests/fixtures/run-pass/json_codec_derived_enums.au",
        type_name: "Reading",
        sample: r#"{"level": "High", "value": "seven", "tags": [true, 0.5], "shape": {"type": "Circle", "radius": 2}}"#,
        keep: &[],
    },
    Case {
        fixture: "crates/aura-compiler/tests/fixtures/run-pass/json_codec_derived_import_forms.au",
        type_name: "Shape",
        sample: r#"{"points": [{"x": 1, "y": 2}], "extra": {"a": [true]}}"#,
        keep: &[],
    },
    Case {
        fixture: "crates/aura-compiler/tests/fixtures/run-pass/json_codec_described.au",
        type_name: "Request",
        sample: r#"{"query": "aura", "mode": "Fast", "limit": 5}"#,
        keep: &[],
    },
    Case {
        fixture: "examples/json/derived_codec.au",
        type_name: "SearchArgs",
        sample: r#"{"query": "aura", "limit": 3, "filters": [{"field": "lang", "values": ["en"]}], "sort": "Newest", "cursor": "c1"}"#,
        keep: &[],
    },
];

#[test]
fn derived_schemas_accept_exactly_what_derived_decoders_accept() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    for case in CASES {
        let path = root.join(case.fixture);
        let source = fs::read_to_string(&path)
            .unwrap_or_else(|error| panic!("cannot read {}: {error}", path.display()));
        let sample: Value = serde_json::from_str(case.sample).expect("the sample is JSON");
        let mut inputs = vec![sample.clone()];
        mutate(&sample, case.keep, &mut |variant| inputs.push(variant));

        let program = harness(&source, case.type_name, &inputs);
        let output = run_on_large_stack(program);
        let mut lines = output.lines();
        let schema: Value =
            serde_json::from_str(lines.next().expect("a schema line")).expect("the schema is JSON");
        let verdicts = lines.collect::<Vec<_>>();
        assert_eq!(
            verdicts.len(),
            inputs.len(),
            "{}: one verdict per input",
            case.type_name
        );
        assert_eq!(verdicts[0], "ok", "{}: the sample decodes", case.type_name);

        let mut accepted = 0;
        for (input, verdict) in inputs.iter().zip(verdicts) {
            let decoded = verdict == "ok";
            let valid = Validator { root: &schema }.valid(&schema, input);
            assert_eq!(
                decoded,
                valid,
                "{}: the decoder {} and the schema {} {input}\nschema: {schema}",
                case.type_name,
                if decoded { "accepts" } else { "rejects" },
                if valid { "accepts" } else { "rejects" },
            );
            accepted += usize::from(decoded);
        }
        assert!(
            accepted < inputs.len(),
            "{}: some inputs are rejected",
            case.type_name
        );
    }
}

/// Builds a program from the fixture: its declarations, with its `main`
/// renamed, and a new `main` that prints the schema and then `ok` or `err`
/// for each input.
fn harness(source: &str, type_name: &str, inputs: &[Value]) -> String {
    let declarations = source.replace("def main():", "def fixture_main():");
    let mut program = format!("import json as agreement_json\n{declarations}\n\n");
    program.push_str(&format!(
        "def agreement_check(text: str):\n\
         \x20   match agreement_json.decode_text[{type_name}](text):\n\
         \x20       case Result.Ok(_):\n\
         \x20           print(\"ok\")\n\
         \x20       case Result.Err(_):\n\
         \x20           print(\"err\")\n\n\n\
         def main():\n\
         \x20   print(agreement_json.dumps(agreement_json.schema[{type_name}]()))\n"
    ));
    for input in inputs {
        let text = input.to_string().replace('\\', "\\\\").replace('"', "\\\"");
        program.push_str(&format!("    agreement_check(\"{text}\")\n"));
    }
    program
}

fn run_on_large_stack(program: String) -> String {
    thread::Builder::new()
        .stack_size(64 * 1024 * 1024)
        .spawn(move || aura_compiler::run_source(&program))
        .expect("spawn the program thread")
        .join()
        .unwrap_or_else(|payload| std::panic::resume_unwind(payload))
        .unwrap_or_else(|error| panic!("the harness program runs: {error}"))
        .stdout
}

/// Values that replace each position of the sample in turn.
fn replacements() -> Vec<Value> {
    vec![
        Value::Null,
        json!(true),
        json!(0),
        json!(-1),
        json!(7),
        json!(1.5),
        json!(300),
        json!(70000),
        json!(4294967296_i64),
        json!("x"),
        json!("Low"),
        json!("Circle"),
        json!([]),
        json!([1]),
        json!({}),
        json!({"type": "Empty"}),
        json!({"type": "Circle", "radius": 1}),
        json!({"x": 1, "y": 2}),
    ]
}

/// Calls `emit` with each variant of `value` that differs at one position:
/// a replaced value, a removed or added object key, or a shorter or longer
/// array.
fn mutate(value: &Value, keep: &[&str], emit: &mut dyn FnMut(Value)) {
    for replacement in replacements() {
        if &replacement != value {
            emit(replacement);
        }
    }
    match value {
        Value::Object(fields) => {
            let mut added = fields.clone();
            added.insert("zz".to_string(), json!(1));
            emit(Value::Object(added));
            for key in fields.keys() {
                let mut removed = fields.clone();
                removed.remove(key);
                emit(Value::Object(removed));
                if keep.contains(&key.as_str()) {
                    continue;
                }
                mutate(&fields[key], keep, &mut |variant| {
                    let mut changed = fields.clone();
                    changed.insert(key.clone(), variant);
                    emit(Value::Object(changed));
                });
            }
        }
        Value::Array(items) => {
            if let Some(first) = items.first() {
                let mut longer = items.clone();
                longer.push(first.clone());
                emit(Value::Array(longer));
                emit(Value::Array(items[..items.len() - 1].to_vec()));
            }
            for (position, item) in items.iter().enumerate() {
                mutate(item, keep, &mut |variant| {
                    let mut changed = items.clone();
                    changed[position] = variant;
                    emit(Value::Array(changed));
                });
            }
        }
        _ => {}
    }
}

/// A JSON Schema 2020-12 validator for the keywords derived schemas use. An
/// unknown keyword fails the test, so the validator never skips a rule.
struct Validator<'a> {
    root: &'a Value,
}

impl Validator<'_> {
    fn valid(&self, schema: &Value, value: &Value) -> bool {
        let schema = match schema {
            Value::Bool(accept) => return *accept,
            Value::Object(schema) => schema,
            other => panic!("a schema is an object or a boolean, found {other}"),
        };
        schema
            .iter()
            .all(|(keyword, argument)| self.keyword(schema, keyword, argument, value))
    }

    fn keyword(
        &self,
        schema: &Map<String, Value>,
        keyword: &str,
        argument: &Value,
        value: &Value,
    ) -> bool {
        match keyword {
            "$defs" | "description" => true,
            "$ref" => {
                let name = argument
                    .as_str()
                    .and_then(|reference| reference.strip_prefix("#/$defs/"))
                    .expect("a local `$defs` reference");
                self.valid(&self.root["$defs"][name], value)
            }
            "type" => match argument.as_str().expect("a single type name") {
                "null" => value.is_null(),
                "boolean" => value.is_boolean(),
                "integer" => value
                    .as_f64()
                    .is_some_and(|number| value.is_number() && number.fract() == 0.0),
                "number" => value.is_number(),
                "string" => value.is_string(),
                "array" => value.is_array(),
                "object" => value.is_object(),
                other => panic!("unknown type `{other}`"),
            },
            "enum" => argument
                .as_array()
                .expect("an `enum` array")
                .iter()
                .any(|option| equal(option, value)),
            "const" => equal(argument, value),
            "minimum" => value
                .as_f64()
                .is_none_or(|number| number >= argument.as_f64().expect("a number")),
            "maximum" => value
                .as_f64()
                .is_none_or(|number| number <= argument.as_f64().expect("a number")),
            "anyOf" => array(argument)
                .iter()
                .any(|option| self.valid(option, value)),
            "oneOf" => {
                array(argument)
                    .iter()
                    .filter(|option| self.valid(option, value))
                    .count()
                    == 1
            }
            "allOf" => array(argument)
                .iter()
                .all(|option| self.valid(option, value)),
            "properties" => {
                let Some(fields) = value.as_object() else {
                    return true;
                };
                argument
                    .as_object()
                    .expect("a `properties` object")
                    .iter()
                    .all(|(key, property)| {
                        fields
                            .get(key)
                            .is_none_or(|item| self.valid(property, item))
                    })
            }
            "required" => {
                let Some(fields) = value.as_object() else {
                    return true;
                };
                array(argument)
                    .iter()
                    .all(|key| fields.contains_key(key.as_str().expect("a key name")))
            }
            "additionalProperties" => {
                let Some(fields) = value.as_object() else {
                    return true;
                };
                let declared = schema.get("properties").and_then(Value::as_object);
                fields
                    .iter()
                    .filter(|(key, _)| declared.is_none_or(|declared| !declared.contains_key(*key)))
                    .all(|(_, item)| self.valid(argument, item))
            }
            "prefixItems" => {
                let Some(items) = value.as_array() else {
                    return true;
                };
                array(argument)
                    .iter()
                    .zip(items)
                    .all(|(schema, item)| self.valid(schema, item))
            }
            "items" => {
                let Some(items) = value.as_array() else {
                    return true;
                };
                let skip = schema
                    .get("prefixItems")
                    .map_or(0, |prefix| array(prefix).len());
                items
                    .iter()
                    .skip(skip)
                    .all(|item| self.valid(argument, item))
            }
            "minItems" => value
                .as_array()
                .is_none_or(|items| items.len() as u64 >= argument.as_u64().expect("a count")),
            other => panic!("the validator does not implement `{other}`"),
        }
    }
}

fn array(argument: &Value) -> &Vec<Value> {
    argument.as_array().expect("an array argument")
}

/// JSON equality, with numbers compared by value.
fn equal(left: &Value, right: &Value) -> bool {
    match (left, right) {
        (Value::Number(_), Value::Number(_)) => left.as_f64() == right.as_f64(),
        _ => left == right,
    }
}
