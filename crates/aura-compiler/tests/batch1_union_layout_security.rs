//! Adversarial contracts for the explicit-tag union layout and drop plans
//! (ADR-0052 A9): every union a module operates on carries one validated
//! plan shared by both backends, and forged or stale plans fail closed.

use aura_compiler::union_layout::{plan_union_layout, MirUnionLayout, UNION_LAYOUT_VERSION};
use aura_compiler::{emit_host_native_object, lower_source_to_mir, run_mir, MirModule};
use serde_json::{json, Value};

const SOURCE: &str = "def describe(value: int64 | str | None) -> int64:\n    if value is None:\n        return 0\n    match value:\n        case int64 as number:\n            return number\n        case str as text:\n            return text.len()\n\ndef main():\n    print(describe(4))\n    print(describe(\"abc\"))\n    print(describe(None))\n";

fn lowered() -> MirModule {
    lower_source_to_mir(SOURCE).expect("the union program must lower")
}

fn encoded() -> Value {
    serde_json::to_value(lowered()).expect("MIR serializes")
}

fn decode(value: Value) -> MirModule {
    serde_json::from_value(value).expect("MIR deserializes")
}

fn assert_rejected(module: MirModule, expected: &str) {
    let error = run_mir(&module).expect_err("forged layout metadata must not execute");
    assert!(
        error.message.contains(expected),
        "unexpected rejection: {}",
        error.message
    );
    let native =
        emit_host_native_object(&module).expect_err("the direct backend shares the rejection");
    assert!(
        native.contains(expected),
        "unexpected native rejection: {native}"
    );
}

#[test]
fn lowered_modules_plan_every_union_they_use() {
    let module = lowered();
    let plan = module
        .unions
        .iter()
        .find(|plan| plan.union_type.to_string() == "int64 | str | None")
        .expect("the parameter union has a plan");
    assert_eq!(plan.members.len(), 3);
    assert_eq!(plan.tag_width, 1);
    assert_eq!(plan.layout_version, UNION_LAYOUT_VERSION);
    assert_eq!(
        plan.members
            .iter()
            .map(|member| member.tag)
            .collect::<Vec<_>>(),
        vec![0, 1, 2]
    );
    let text = plan
        .members
        .iter()
        .find(|member| member.ty.to_string() == "str")
        .expect("str member");
    assert!(text.needs_drop && !text.copy, "str payloads own cleanup");
    let number = plan
        .members
        .iter()
        .find(|member| member.ty.to_string() == "int64")
        .expect("int64 member");
    assert!(number.copy && !number.needs_drop);
    assert!(!plan.copy, "a union with a str member is not Copy");
    if let aura_compiler::sema::Type::Union(union) = &plan.union_type {
        assert_eq!(
            plan,
            &plan_union_layout(union, |ty| ty.to_string() == "int64")
        );
    } else {
        panic!("plan describes a union");
    }
    let output = run_mir(&module).expect("the planned module runs");
    assert_eq!(output.stdout, "4\n3\n0\n");
}

#[test]
fn plans_survive_the_public_mir_round_trip() {
    let module = lowered();
    let round_tripped = decode(serde_json::to_value(&module).unwrap());
    assert_eq!(round_tripped.unions, module.unions);
    assert_eq!(run_mir(&round_tripped).unwrap().stdout, "4\n3\n0\n");
}

#[test]
fn a_module_without_plans_is_stale_and_rejected() {
    let mut encoded = encoded();
    encoded["unions"] = json!([]);
    assert_rejected(decode(encoded), "has no layout plan");
}

#[test]
fn a_reordered_member_plan_is_rejected() {
    let mut module = lowered();
    let plan = module.unions.iter_mut().next().expect("a plan");
    plan.members.swap(0, 1);
    assert_rejected(module, "canonical member");
}

#[test]
fn a_forged_tag_width_is_rejected() {
    let mut module = lowered();
    module.unions[0].tag_width = 4;
    assert_rejected(module, "layout rule");
}

#[test]
fn a_stale_layout_version_is_rejected() {
    let mut module = lowered();
    module.unions[0].layout_version = UNION_LAYOUT_VERSION + 1;
    assert_rejected(module, "layout version");
}

#[test]
fn a_plan_claiming_a_non_copy_member_is_copy_is_rejected() {
    let mut module = lowered();
    let plan: &mut MirUnionLayout = &mut module.unions[0];
    for member in &mut plan.members {
        member.copy = true;
        member.needs_drop = false;
    }
    plan.copy = true;
    assert_rejected(module, "marks non-Copy member");
}

#[test]
fn a_duplicate_plan_is_rejected() {
    let mut module = lowered();
    let duplicate = module.unions[0].clone();
    module.unions.push(duplicate);
    assert_rejected(module, "duplicate union layout plan");
}
