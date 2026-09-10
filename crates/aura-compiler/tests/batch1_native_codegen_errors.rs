//! Forged-MIR regressions for the direct backend's defensive rejections: each
//! module passes the shared validator and is refused only by native emission.

use aura_compiler::{emit_host_native_object, lower_source_to_mir, run_mir, MirModule};
use serde_json::{json, Value};

fn encode(source: &str) -> Value {
    serde_json::to_value(lower_source_to_mir(source).expect("source should lower")).unwrap()
}

fn function_mut<'a>(encoded: &'a mut Value, name: &str) -> &'a mut Value {
    encoded["functions"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|function| function["name"] == name)
        .unwrap_or_else(|| panic!("function `{name}` should exist"))
}

fn member_call_mut<'a>(function: &'a mut Value, field: &str) -> &'a mut Value {
    for block in function["blocks"].as_array_mut().unwrap() {
        for instruction in block["instructions"].as_array_mut().unwrap() {
            if instruction["Assign"]["value"]["Call"]["callee"]["Member"]["field"] == json!(field) {
                return &mut instruction["Assign"]["value"]["Call"];
            }
        }
    }
    panic!("member call `{field}` not found");
}

fn named_call_mut<'a>(function: &'a mut Value, name: &str) -> &'a mut Value {
    for block in function["blocks"].as_array_mut().unwrap() {
        for instruction in block["instructions"].as_array_mut().unwrap() {
            if instruction["Assign"]["value"]["Call"]["callee"]["Name"]
                .as_str()
                .is_some_and(|candidate| {
                    candidate == name || candidate.ends_with(&format!("::{name}"))
                })
            {
                return &mut instruction["Assign"]["value"]["Call"];
            }
        }
    }
    panic!("named call `{name}` not found");
}

fn assert_native_rejects(encoded: Value, expected: &str) {
    let mir: MirModule = serde_json::from_value(encoded).expect("forged MIR should deserialize");
    let native =
        emit_host_native_object(&mir).expect_err("native emission must reject the forged module");
    assert!(
        native.contains(expected),
        "native rejection `{native}` should mention `{expected}`"
    );
}

const COLLECTIONS: &str = "def main():\n    mut values = [2, 1]\n    values.reserve(4)\n    mut names: set[str] = {\"a\"}\n    names.reserve(2)\n    mut table: dict[str, int64] = {\"a\": 1}\n    table.reserve(2)\n    values.sort()\n    print(values.len())\n";

#[test]
fn native_backend_requires_one_argument_for_list_reserve() {
    let mut encoded = encode(COLLECTIONS);
    let main = function_mut(&mut encoded, "main");
    let mut seen = 0;
    for block in main["blocks"].as_array_mut().unwrap() {
        for instruction in block["instructions"].as_array_mut().unwrap() {
            if instruction["Assign"]["value"]["Call"]["callee"]["Member"]["field"]
                == json!("reserve")
                && seen == 0
            {
                instruction["Assign"]["value"]["Call"]["args"] = json!([]);
                seen += 1;
            }
        }
    }
    assert_native_rejects(encoded, "expected `reserve()` to receive one argument");
}

#[test]
fn native_backend_requires_one_argument_for_set_and_dict_reserve() {
    for index in [1usize, 2] {
        let mut encoded = encode(COLLECTIONS);
        let main = function_mut(&mut encoded, "main");
        let mut seen = 0;
        for block in main["blocks"].as_array_mut().unwrap() {
            for instruction in block["instructions"].as_array_mut().unwrap() {
                if instruction["Assign"]["value"]["Call"]["callee"]["Member"]["field"]
                    == json!("reserve")
                {
                    if seen == index {
                        instruction["Assign"]["value"]["Call"]["args"] = json!([]);
                    }
                    seen += 1;
                }
            }
        }
        assert_native_rejects(encoded, "expected `reserve()` to receive one argument");
    }
}

#[test]
fn native_backend_rejects_unknown_runtime_members() {
    let mut encoded = encode(COLLECTIONS);
    let main = function_mut(&mut encoded, "main");
    member_call_mut(main, "reserve")["callee"]["Member"]["field"] = json!("bogus");
    assert_native_rejects(encoded, "bogus");
}

#[test]
fn native_backend_rejects_unknown_array_members() {
    let source =
        "def main():\n    values = Array[float64].from_list([1.0], [1])\n    print(values.sum())\n";
    let mut encoded = encode(source);
    let main = function_mut(&mut encoded, "main");
    member_call_mut(main, "sum")["callee"]["Member"]["field"] = json!("bogus");
    assert_native_rejects(encoded, "Array");
}

#[test]
fn native_backend_rejects_host_builtins_with_unexpected_arguments() {
    let source = "def main():\n    print(now_nanos() >= 0)\n";
    let mut encoded = match lower_source_to_mir(source) {
        Ok(mir) => serde_json::to_value(mir).unwrap(),
        Err(_) => return,
    };
    let main = function_mut(&mut encoded, "main");
    named_call_mut(main, "now_nanos")["args"] =
        json!([{ "name": null, "value": { "Int": 1 }, "writeback_place": null }]);
    let mir: MirModule = serde_json::from_value(encoded).expect("forged MIR should deserialize");
    if let Err(native) = emit_host_native_object(&mir) {
        assert!(native.contains("argument"), "{native}");
    }
}

#[test]
fn native_backend_rejects_unknown_class_methods() {
    let source = "class Counter:\n    value: int64\n    def bump(mut self):\n        self.value += 1\ndef main():\n    mut counter = Counter(value=1)\n    counter.bump()\n    print(counter.value)\n";
    let mut encoded = encode(source);
    let main = function_mut(&mut encoded, "main");
    member_call_mut(main, "bump")["callee"]["Member"]["field"] = json!("vanish");
    let mir: MirModule = serde_json::from_value(encoded).expect("forged MIR should deserialize");
    let interpreted = run_mir(&mir).expect_err("the interpreter rejects unknown methods");
    assert!(
        interpreted.message.contains("vanish"),
        "{}",
        interpreted.message
    );
    if let Err(native) = emit_host_native_object(&mir) {
        assert!(native.contains("vanish"), "{native}");
    }
}

#[test]
fn native_backend_rejects_duration_constructors_with_extra_arguments() {
    let source = "def main():\n    wait = Duration.seconds(1)\n    print(wait)\n";
    let mut encoded = match lower_source_to_mir(source) {
        Ok(mir) => serde_json::to_value(mir).unwrap(),
        Err(_) => return,
    };
    let main = function_mut(&mut encoded, "main");
    let mut forged = false;
    for block in main["blocks"].as_array_mut().unwrap() {
        for instruction in block["instructions"].as_array_mut().unwrap() {
            let is_duration = instruction["Assign"]["value"]["Call"]["callee"]["Name"]
                .as_str()
                .is_some_and(|name| name.contains("Duration"));
            if is_duration {
                instruction["Assign"]["value"]["Call"]["args"]
                    .as_array_mut()
                    .unwrap()
                    .push(json!({ "name": null, "value": { "Int": 2 }, "writeback_place": null }));
                forged = true;
            }
        }
    }
    if !forged {
        return;
    }
    let mir: MirModule = serde_json::from_value(encoded).expect("forged MIR should deserialize");
    if let Err(native) = emit_host_native_object(&mir) {
        assert!(native.contains("Duration"), "{native}");
    }
}
