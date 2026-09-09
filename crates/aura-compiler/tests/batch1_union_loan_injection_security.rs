//! Union injection must not relabel borrowed member storage as union storage.

use aura_compiler::{check_source, lower_source_to_mir, run_mir, MirModule};
use serde_json::Value;

const VIEW_HELPER: &str = "class Profile:\n    name: str\ndef profile_name(profile: Profile) -> view str from profile:\n    return view profile.name\n";

#[test]
fn shared_member_view_cannot_be_passed_as_a_borrowed_union() {
    let source = format!(
        "{VIEW_HELPER}def accept(value: str | None) -> int64:\n    return 42\ndef main():\n    profile = Profile(name=\"Ada\")\n    view name = profile_name(profile)\n    print(accept(value=name))\n"
    );

    let error = match check_source(&source) {
        Err(error) => error,
        Ok(_) => panic!("a member view cannot be relabeled with a union descriptor"),
    };
    assert!(
        matches!(error.code.as_str(), "AU2010" | "AU3003"),
        "{error}"
    );
}

#[test]
fn argument_origin_union_view_cannot_derive_from_member_layout() {
    let source = format!(
        "{VIEW_HELPER}def preserve(value: str | None) -> view (str | None) from value:\n    return view value\ndef main():\n    profile = Profile(name=\"Ada\")\n    view name = profile_name(profile)\n    view result = preserve(value=name)\n    print(result)\n"
    );

    let error = match check_source(&source) {
        Err(error) => error,
        Ok(_) => panic!("a returned union view requires an actual union-typed origin place"),
    };
    assert!(
        matches!(error.code.as_str(), "AU2010" | "AU3003"),
        "{error}"
    );
}

#[test]
fn checked_mir_does_not_execute_a_member_place_as_union_storage() {
    let source = format!(
        "{VIEW_HELPER}def accept(value: str | None) -> int64:\n    return 42\ndef main():\n    profile = Profile(name=\"Ada\")\n    view name = profile_name(profile)\n    print(accept(value=name))\n"
    );
    if let Ok(mir) = lower_source_to_mir(&source) {
        let error = run_mir(&mir)
            .expect_err("MIR validation must contain any checker gap before execution");
        assert!(
            error.message.contains("type") || error.message.contains("loan"),
            "{error}"
        );
    }
}

#[test]
fn ordinary_noncopy_member_local_is_not_cloned_into_a_borrowed_union() {
    let source = "def accept(value: str | None) -> int64:\n    return 42\ndef main():\n    name = \"Ada\"\n    print(accept(value=name))\n";
    let error = match check_source(source) {
        Err(error) => error,
        Ok(_) => panic!("borrowing a union must not implicitly clone a non-Copy member local"),
    };
    assert!(
        matches!(error.code.as_str(), "AU2010" | "AU3003"),
        "{error}"
    );
}

#[test]
fn copy_member_local_borrow_materializes_a_real_union_wrapper() {
    let source = "def accept(value: int64 | None) -> int64:\n    return 42\ndef main():\n    number = 1\n    print(accept(value=number))\n";
    check_source(source).expect("a Copy member may be snapshotted for borrowed injection");
    let mir = lower_source_to_mir(source).expect("Copy injection must lower");
    let output = run_mir(&mir).expect("the borrowed call must observe real union storage");
    assert_eq!(output.stdout, "42\n");
}

#[test]
fn exact_union_view_can_be_passed_to_a_borrowed_union() {
    let source = "def accept(value: int64 | None) -> int64:\n    return 42\ndef main():\n    value: int64 | None = 1\n    view borrowed = value\n    print(accept(value=borrowed))\n";
    check_source(source).expect("an exact-union view already points at union storage");
    let mir = lower_source_to_mir(source).expect("exact-union view must lower unchanged");
    let output = run_mir(&mir).expect("exact-union view must pass validation");
    assert_eq!(output.stdout, "42\n");
}

#[test]
fn returned_origin_rejects_copy_member_to_union_adaptation() {
    let source = "def preserve(value: int64 | None) -> view (int64 | None) from value:\n    return view value\ndef main():\n    number = 1\n    view result = preserve(value=number)\n    print(result)\n";
    let error = match check_source(source) {
        Err(error) => error,
        Ok(_) => panic!("a returned union view cannot originate from Copy member layout"),
    };
    assert!(
        matches!(error.code.as_str(), "AU2010" | "AU3003"),
        "{error}"
    );
}

fn find_accept_call(value: &mut Value) -> Option<&mut serde_json::Map<String, Value>> {
    match value {
        Value::Object(object) => {
            let is_accept = object.get("Call").is_some_and(|call| {
                serde_json::to_string(&call.get("callee"))
                    .is_ok_and(|encoded| encoded.contains("accept"))
            });
            if is_accept {
                return object.get_mut("Call").and_then(Value::as_object_mut);
            }
            object.values_mut().find_map(find_accept_call)
        }
        Value::Array(items) => items.iter_mut().find_map(find_accept_call),
        _ => None,
    }
}

fn find_indirect_call(value: &mut Value) -> Option<&mut serde_json::Map<String, Value>> {
    match value {
        Value::Object(object) => {
            let is_indirect = object.get("Call").is_some_and(|call| {
                call.get("callee")
                    .and_then(|callee| callee.get("Value"))
                    .is_some()
            });
            if is_indirect {
                return object.get_mut("Call").and_then(Value::as_object_mut);
            }
            object.values_mut().find_map(find_indirect_call)
        }
        Value::Array(items) => items.iter_mut().find_map(find_indirect_call),
        _ => None,
    }
}

#[test]
fn malformed_mir_rejects_member_operand_for_exact_union_parameter() {
    let source = "def accept(value: int64 | None) -> int64:\n    return 42\ndef main():\n    value: int64 | None = 1\n    print(accept(value=value))\n";
    let mir = lower_source_to_mir(source).expect("valid exact-union call must lower");
    let mut encoded = serde_json::to_value(mir).expect("MIR must serialize");
    let call = find_accept_call(&mut encoded).expect("lowered MIR must call accept");
    let args = call["args"].as_array_mut().expect("call args are an array");
    args[0]["value"] = serde_json::json!({ "Int": 1 });
    let forged: MirModule =
        serde_json::from_value(encoded).expect("forged call remains syntactically valid MIR");

    let error = run_mir(&forged)
        .expect_err("common MIR validation must reject member layout for a union parameter");
    assert!(
        error.message.contains("exact union type")
            && error.message.contains("parameter")
            && error.message.contains("value"),
        "{error}"
    );
}

#[test]
fn malformed_mir_rejects_member_operand_for_indirect_union_parameter() {
    let source = "def accept(value: int64 | None) -> int64:\n    return 42\ndef main():\n    value: int64 | None = 1\n    selected: def(int64 | None) -> int64 = accept\n    print(selected(value))\n";
    let mir = lower_source_to_mir(source).expect("valid indirect union call must lower");
    let mut encoded = serde_json::to_value(mir).expect("MIR must serialize");
    let call = find_indirect_call(&mut encoded).expect("lowered MIR must contain an indirect call");
    let args = call["args"].as_array_mut().expect("call args are an array");
    args[0]["value"] = serde_json::json!({ "Int": 1 });
    let forged: MirModule =
        serde_json::from_value(encoded).expect("forged call remains syntactically valid MIR");

    let error = run_mir(&forged)
        .expect_err("common MIR validation must enforce the indirect function signature");
    assert!(
        error.message.contains("exact union type")
            && error.message.contains("indirect")
            && error.message.contains("parameter 1"),
        "{error}"
    );
}

#[test]
fn malformed_mir_rejects_member_operand_for_method_union_parameter() {
    let source = "class Acceptor:\n    def accept(self, value: int64 | None) -> int64:\n        return 42\ndef main():\n    receiver = Acceptor()\n    value: int64 | None = 1\n    print(receiver.accept(value))\n";
    let mir = lower_source_to_mir(source).expect("valid method union call must lower");
    let mut encoded = serde_json::to_value(mir).expect("MIR must serialize");
    let call = find_accept_call(&mut encoded).expect("lowered MIR must call the method");
    let args = call["args"].as_array_mut().expect("call args are an array");
    args[0]["value"] = serde_json::json!({ "Int": 1 });
    let forged: MirModule =
        serde_json::from_value(encoded).expect("forged call remains syntactically valid MIR");

    let error = run_mir(&forged)
        .expect_err("common MIR validation must enforce the method declaration contract");
    assert!(
        error.message.contains("exact union type")
            && error.message.contains("Acceptor.accept")
            && error.message.contains("value"),
        "{error}"
    );
}
