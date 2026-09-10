//! Forged MIR must not manufacture access to an inactive union payload.

use aura_compiler::{emit_host_native_object, lower_source_to_mir, run_mir, MirModule};
use serde_json::{json, Value};

fn main_block_mut<'a>(encoded: &'a mut Value, label: &str) -> &'a mut Value {
    encoded["functions"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|function| function["name"] == "main")
        .unwrap()["blocks"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|block| block["label"] == label)
        .unwrap()
}

fn main_union_type(encoded: &Value) -> Value {
    encoded["functions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|function| function["name"] == "main")
        .unwrap()["local_types"]
        .as_array()
        .unwrap()
        .iter()
        .find(|local| local["name"] == "value")
        .unwrap()["ty"]
        .clone()
}

fn forged_pattern_mir(then_instructions: Vec<Value>) -> MirModule {
    let source = "def main():\n    value: int64 | str = 1\n    other: int64 | str = 1\n    flag = false\n    payload = 0\n";
    let mir = lower_source_to_mir(source).expect("baseline union locals must lower");
    let mut encoded = serde_json::to_value(mir).expect("MIR must serialize");
    let main = encoded["functions"]
        .as_array_mut()
        .expect("functions are serialized as an array")
        .iter_mut()
        .find(|function| function["name"] == "main")
        .expect("baseline has main");
    let union_type = main["local_types"]
        .as_array()
        .expect("locals are serialized as an array")
        .iter()
        .find(|local| local["name"] == "value")
        .expect("baseline has union local")["ty"]
        .clone();
    main["blocks"] = json!([
        {
            "label": "entry",
            "instructions": [
            { "Assign": { "target": "value", "value": { "UnionInject": {
                "value": { "Int": 1 }, "union_type": union_type.clone(),
                "member_type": { "Named": ["int64", []] }, "member_index": 0
            }}}},
            {
                "Assign": {
                    "target": "flag",
                    "value": { "UnionTagTest": {
                        "place": "value",
                        "union_type": union_type,
                        "member_index": 0
                    }}
                }
            }],
            "terminator": { "Branch": {
                "condition": { "Place": "flag" },
                "then_label": "matched",
                "else_label": "done"
            }}
        },
        {
            "label": "matched",
            "instructions": then_instructions,
            "terminator": { "Return": "Unit" }
        },
        {
            "label": "done",
            "instructions": [],
            "terminator": { "Return": "Unit" }
        }
    ]);
    serde_json::from_value(encoded).expect("forged pattern remains syntactically valid MIR")
}

#[test]
fn inactive_union_member_projection_requires_matching_true_edge_tag() {
    let mir = forged_pattern_mir(vec![json!({ "BeginLoan": {
        "loan": "payload_loan",
        "source": "value.__union_payload_1",
        "mutable": false
    }})]);
    let error = run_mir(&mir).expect_err("member 0 proof must not authorize member 1 payload");
    assert!(error.message.contains("matching tag proof"), "{error}");
}

#[test]
fn union_payload_cannot_be_taken_while_guard_loan_is_live() {
    let mut encoded = serde_json::to_value(forged_pattern_mir(Vec::new())).unwrap();
    let main = encoded["functions"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|function| function["name"] == "main")
        .unwrap();
    let union_type = main["local_types"]
        .as_array()
        .unwrap()
        .iter()
        .find(|local| local["name"] == "value")
        .unwrap()["ty"]
        .clone();
    main["blocks"][1]["instructions"] = json!([
        { "BeginLoan": { "loan": "payload_loan", "source": "value.__union_payload_0", "mutable": false }},
        { "Assign": { "target": "payload", "value": { "UnionTakePayload": {
            "place": "value", "union_type": union_type,
            "member_type": { "Named": ["int64", []] }, "member_index": 0
        }}}}
    ]);
    let mir: MirModule = serde_json::from_value(encoded).unwrap();
    let error = run_mir(&mir).expect_err("guard loan must end before owned payload take");
    assert!(error.message.contains("locked place"), "{error}");
}

#[test]
fn false_tag_edge_cannot_project_the_tested_payload() {
    let mut encoded = serde_json::to_value(forged_pattern_mir(Vec::new())).unwrap();
    main_block_mut(&mut encoded, "done")["instructions"] = json!([{ "BeginLoan": {
        "loan": "payload_loan", "source": "value.__union_payload_0", "mutable": false
    }}]);
    let mir: MirModule = serde_json::from_value(encoded).unwrap();
    let error = run_mir(&mir).expect_err("false tag edge must not authorize payload access");
    assert!(error.message.contains("matching tag proof"), "{error}");
}

#[test]
fn shared_payload_loan_cannot_escalate_to_mutable_reborrow() {
    let mir = forged_pattern_mir(vec![
        json!({ "BeginLoan": {
            "loan": "payload_loan", "source": "value.__union_payload_0", "mutable": false
        }}),
        json!({ "Reborrow": {
            "loan": "mutable_payload", "parent": "payload_loan",
            "projection": "", "mutable": true
        }}),
    ]);
    let error = run_mir(&mir).expect_err("shared payload must not grant mutable authority");
    assert!(error.message.contains("escalates shared parent"), "{error}");
}

#[test]
fn union_payload_cannot_be_taken_twice_on_one_path() {
    let mut encoded = serde_json::to_value(forged_pattern_mir(Vec::new())).unwrap();
    let union_type = main_union_type(&encoded);
    let take = json!({ "Assign": { "target": "payload", "value": {
        "UnionTakePayload": {
            "place": "value", "union_type": union_type,
            "member_type": { "Named": ["int64", []] }, "member_index": 0
        }
    }}});
    main_block_mut(&mut encoded, "matched")["instructions"] =
        Value::Array(vec![take.clone(), take]);
    let mir: MirModule = serde_json::from_value(encoded).unwrap();
    let error = run_mir(&mir).expect_err("one union storage generation can be taken once");
    assert!(error.message.contains("already-taken"), "{error}");
}

#[test]
fn union_tag_fact_does_not_survive_a_control_flow_join() {
    let mut encoded = serde_json::to_value(forged_pattern_mir(Vec::new())).unwrap();
    main_block_mut(&mut encoded, "matched")["terminator"] = json!({ "Goto": "join" });
    main_block_mut(&mut encoded, "done")["terminator"] = json!({ "Goto": "join" });
    let main = encoded["functions"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|function| function["name"] == "main")
        .unwrap();
    main["blocks"].as_array_mut().unwrap().push(json!({
        "label": "join",
        "instructions": [{ "BeginLoan": {
            "loan": "payload_loan", "source": "value.__union_payload_0", "mutable": false
        }}],
        "terminator": { "Return": "Unit" }
    }));
    let mir: MirModule = serde_json::from_value(encoded).unwrap();
    let error = run_mir(&mir).expect_err("join must retain only facts true on every predecessor");
    assert!(error.message.contains("matching tag proof"), "{error}");
}

#[test]
fn retagging_invalidates_the_previous_member_proof() {
    let mut encoded = serde_json::to_value(forged_pattern_mir(Vec::new())).unwrap();
    let union_type = main_union_type(&encoded);
    main_block_mut(&mut encoded, "matched")["instructions"] = json!([
        { "Assign": { "target": "value", "value": { "UnionInject": {
            "value": { "String": "changed" }, "union_type": union_type,
            "member_type": { "Named": ["str", []] }, "member_index": 1
        }}}},
        { "BeginLoan": {
            "loan": "payload_loan", "source": "value.__union_payload_0", "mutable": false
        }}
    ]);
    let mir: MirModule = serde_json::from_value(encoded).unwrap();
    let error = run_mir(&mir).expect_err("retagging must invalidate the prior tag fact");
    assert!(error.message.contains("matching tag proof"), "{error}");
}

#[test]
fn retagging_is_rejected_while_a_payload_loan_is_live() {
    let mut encoded = serde_json::to_value(forged_pattern_mir(Vec::new())).unwrap();
    let union_type = main_union_type(&encoded);
    main_block_mut(&mut encoded, "matched")["instructions"] = json!([
        { "BeginLoan": {
            "loan": "payload_loan", "source": "value.__union_payload_0", "mutable": false
        }},
        { "Assign": { "target": "value", "value": { "UnionInject": {
            "value": { "String": "changed" }, "union_type": union_type,
            "member_type": { "Named": ["str", []] }, "member_index": 1
        }}}}
    ]);
    let mir: MirModule = serde_json::from_value(encoded).unwrap();
    let error = run_mir(&mir).expect_err("active payload loan must lock the union root");
    assert!(error.message.contains("mutates locked place"), "{error}");
}

#[test]
fn malformed_payload_projection_index_is_rejected() {
    let mir = forged_pattern_mir(vec![json!({ "BeginLoan": {
        "loan": "payload_loan", "source": "value.__union_payload_00", "mutable": false
    }})]);
    let error = run_mir(&mir).expect_err("payload indices use canonical decimal spelling");
    assert!(
        error
            .message
            .contains("non-canonical MIR union payload projection"),
        "{error}"
    );
}

#[test]
fn payload_take_member_metadata_must_match_the_union_index() {
    let mut encoded = serde_json::to_value(forged_pattern_mir(Vec::new())).unwrap();
    let union_type = main_union_type(&encoded);
    main_block_mut(&mut encoded, "matched")["instructions"] = json!([{ "Assign": {
        "target": "payload", "value": { "UnionTakePayload": {
            "place": "value", "union_type": union_type,
            "member_type": { "Named": ["str", []] }, "member_index": 0
        }}
    }}]);
    let mir: MirModule = serde_json::from_value(encoded).unwrap();
    let error = run_mir(&mir).expect_err("take metadata must identify the indexed member");
    assert!(error.message.contains("index and type disagree"), "{error}");
}

#[test]
fn arm_local_payload_loan_cannot_be_returned() {
    let mut encoded = serde_json::to_value(forged_pattern_mir(Vec::new())).unwrap();
    main_block_mut(&mut encoded, "matched")["instructions"] = json!([
        { "BeginLoan": {
            "loan": "payload_loan", "source": "value.__union_payload_0", "mutable": false
        }},
        { "ReturnLoan": { "loan": "payload_loan", "origin": "value" }}
    ]);
    let mir: MirModule = serde_json::from_value(encoded).unwrap();
    let error = run_mir(&mir).expect_err("returned views cannot originate in an arm local");
    assert!(error.message.contains("non-parameter origin"), "{error}");
}

#[test]
fn whole_union_reinitialization_resets_taken_state() {
    let mut encoded = serde_json::to_value(forged_pattern_mir(Vec::new())).unwrap();
    let union_type = main_union_type(&encoded);
    let take = json!({ "Assign": { "target": "payload", "value": {
        "UnionTakePayload": {
            "place": "value", "union_type": union_type.clone(),
            "member_type": { "Named": ["int64", []] }, "member_index": 0
        }
    }}});
    main_block_mut(&mut encoded, "matched")["instructions"] = Value::Array(vec![
        take.clone(),
        json!({ "Assign": { "target": "value", "value": { "UnionInject": {
            "value": { "Int": 2 }, "union_type": union_type.clone(),
            "member_type": { "Named": ["int64", []] }, "member_index": 0
        }}}}),
        json!({ "Assign": { "target": "flag", "value": { "UnionTagTest": {
            "place": "value", "union_type": union_type, "member_index": 0
        }}}}),
    ]);
    main_block_mut(&mut encoded, "matched")["terminator"] = json!({ "Branch": {
        "condition": { "Place": "flag" }, "then_label": "retaken", "else_label": "done"
    }});
    let main = encoded["functions"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|function| function["name"] == "main")
        .unwrap();
    main["blocks"].as_array_mut().unwrap().push(json!({
        "label": "retaken", "instructions": [take], "terminator": { "Return": "Unit" }
    }));
    let mir: MirModule = serde_json::from_value(encoded).unwrap();
    run_mir(&mir).expect("a validated whole-union write creates a fresh take generation");
}

#[test]
fn moving_the_union_invalidates_its_tag_fact() {
    let mut encoded = serde_json::to_value(forged_pattern_mir(Vec::new())).unwrap();
    main_block_mut(&mut encoded, "matched")["instructions"] = json!([
        { "Assign": { "target": "other", "value": { "Use": { "MovePlace": "value" }}}},
        { "BeginLoan": {
            "loan": "payload_loan", "source": "value.__union_payload_0", "mutable": false
        }}
    ]);
    let mir: MirModule = serde_json::from_value(encoded).unwrap();
    let error = run_mir(&mir).expect_err("moving a union must invalidate its tag authority");
    assert!(error.message.contains("matching tag proof"), "{error}");
}

fn forged_enum_pattern_mir(source: &str, matched_instructions: Vec<Value>) -> MirModule {
    let mir = lower_source_to_mir(source).expect("baseline enum locals must lower");
    let mut encoded = serde_json::to_value(mir).expect("MIR must serialize");
    let main = encoded["functions"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|function| function["name"] == "main")
        .unwrap();
    main["blocks"] = json!([
        {
            "label": "entry", "instructions": [],
            "terminator": { "Match": {
                "scrutinee": { "Place": "boxed" },
                "arms": [{
                    "enum_name": "Boxed", "variant_name": "Item",
                    "wildcard": false, "label": "matched"
                }],
                "otherwise": "done"
            }}
        },
        {
            "label": "matched", "instructions": matched_instructions,
            "terminator": { "Return": "Unit" }
        },
        {
            "label": "done", "instructions": [],
            "terminator": { "Return": "Unit" }
        }
    ]);
    serde_json::from_value(encoded).expect("forged enum pattern remains syntactically valid MIR")
}

const ENUM_PATTERN_SOURCE: &str = "enum Boxed:\n    Item(int64)\n    Other(int64)\n    Empty\ndef main():\n    boxed = Boxed.Item(1)\n    payload = 0\n";

#[test]
fn enum_payload_projection_requires_the_matching_variant_successor() {
    let mir = forged_enum_pattern_mir(
        ENUM_PATTERN_SOURCE,
        vec![json!({ "BeginLoan": {
            "loan": "payload", "source": "boxed.__variant_payload_Other_0", "mutable": false
        }})],
    );
    let error = run_mir(&mir).expect_err("Item successor must not authorize Other payload");
    assert!(error.message.contains("matching variant proof"), "{error}");
}

#[test]
fn enum_payload_projection_index_must_be_canonical() {
    let mir = forged_enum_pattern_mir(
        ENUM_PATTERN_SOURCE,
        vec![json!({ "BeginLoan": {
            "loan": "payload", "source": "boxed.__variant_payload_Item_00", "mutable": false
        }})],
    );
    let error = run_mir(&mir).expect_err("enum payload index must be canonical decimal");
    assert!(
        error
            .message
            .contains("non-canonical MIR enum payload projection"),
        "{error}"
    );
}

#[test]
fn proven_enum_projection_still_requires_an_in_bounds_payload_index() {
    let mir = forged_enum_pattern_mir(
        ENUM_PATTERN_SOURCE,
        vec![
            json!({ "BeginLoan": { "loan": "source_loan", "source": "payload", "mutable": false }}),
            json!({ "ReadLoan": {
                "target": "boxed.__variant_payload_Item_999", "loan": "source_loan"
            }}),
            json!({ "EndLoan": { "loan": "source_loan" }}),
        ],
    );
    let error = run_mir(&mir).expect_err("a matching variant fact cannot authorize bad layout");
    assert!(error.message.contains("out of bounds"), "{error}");
}

#[test]
fn generic_enum_payload_projection_uses_substituted_type() {
    let source = "enum Boxed[T]:\n    Item(T)\n    Empty\ndef main():\n    boxed = Boxed[str].Item(\"text\")\n    payload = 0\n";
    let mir = forged_enum_pattern_mir(
        source,
        vec![json!({ "BeginLoan": {
            "loan": "payload", "source": "boxed.__variant_payload_Item_0", "mutable": false
        }})],
    );
    let error = run_mir(&mir).expect_err("Boxed[str] payload cannot back an int64 loan");
    assert!(
        error.message.contains("projects") && error.message.contains("expected"),
        "{error}"
    );
}

#[test]
fn enum_variant_fact_is_invalidated_by_reconstruction() {
    let mut encoded =
        serde_json::to_value(forged_enum_pattern_mir(ENUM_PATTERN_SOURCE, Vec::new())).unwrap();
    main_block_mut(&mut encoded, "matched")["instructions"] = json!([
        { "Assign": { "target": "boxed", "value": { "EnumVariant": {
            "enum_name": "Boxed", "variant_name": "Other", "payloads": [{ "Int": 2 }]
        }}}},
        { "BeginLoan": {
            "loan": "payload", "source": "boxed.__variant_payload_Item_0", "mutable": false
        }}
    ]);
    let mir: MirModule = serde_json::from_value(encoded).unwrap();
    let error = run_mir(&mir).expect_err("enum reconstruction invalidates prior variant proof");
    assert!(error.message.contains("matching variant proof"), "{error}");
}

#[test]
fn enum_payload_projection_on_otherwise_edge_is_rejected() {
    let mut encoded =
        serde_json::to_value(forged_enum_pattern_mir(ENUM_PATTERN_SOURCE, Vec::new())).unwrap();
    main_block_mut(&mut encoded, "done")["instructions"] = json!([{ "BeginLoan": {
        "loan": "payload", "source": "boxed.__variant_payload_Item_0", "mutable": false
    }}]);
    let mir: MirModule = serde_json::from_value(encoded).unwrap();
    let error = run_mir(&mir).expect_err("otherwise edge has no positive variant proof");
    assert!(error.message.contains("matching variant proof"), "{error}");
}

#[test]
fn shared_enum_payload_loan_cannot_escalate_to_mutable() {
    let mir = forged_enum_pattern_mir(
        ENUM_PATTERN_SOURCE,
        vec![
            json!({ "BeginLoan": {
                "loan": "payload", "source": "boxed.__variant_payload_Item_0", "mutable": false
            }}),
            json!({ "Reborrow": {
                "loan": "mutable_payload", "parent": "payload",
                "projection": "", "mutable": true
            }}),
        ],
    );
    let error = run_mir(&mir).expect_err("enum payload view cannot increase capability");
    assert!(error.message.contains("escalates shared parent"), "{error}");
}

#[test]
fn duplicate_enum_variant_metadata_is_rejected_before_projection() {
    let mut encoded =
        serde_json::to_value(forged_enum_pattern_mir(ENUM_PATTERN_SOURCE, Vec::new())).unwrap();
    let boxed = encoded["enums"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|item| item["name"] == "Boxed")
        .unwrap();
    boxed["variants"].as_array_mut().unwrap().push(json!({
        "name": "Item", "payloads": [{ "Named": ["str", []] }]
    }));
    let mir: MirModule = serde_json::from_value(encoded).unwrap();
    let error = run_mir(&mir).expect_err("duplicate variants make payload layout ambiguous");
    assert!(
        error.message.contains("duplicate variant `Item`"),
        "{error}"
    );
}

#[test]
fn enum_payload_projection_requires_embedded_layout_metadata() {
    let mir = forged_enum_pattern_mir(
        ENUM_PATTERN_SOURCE,
        vec![json!({ "BeginLoan": {
            "loan": "payload", "source": "boxed.__variant_payload_Item_0", "mutable": false
        }})],
    );
    let mut encoded = serde_json::to_value(mir).unwrap();
    encoded["enums"] = json!([]);
    let mir: MirModule = serde_json::from_value(encoded).unwrap();
    let error = run_mir(&mir).expect_err("payload projections require authoritative enum layout");
    assert!(error.message.contains("concrete payload layout"), "{error}");
}

#[test]
fn nested_enum_and_union_payload_proofs_compose() {
    let source = "enum Inner:\n    Data(int64 | str)\nenum Outer:\n    Nested(Inner)\ndef main():\n    value = Outer.Nested(Inner.Data(42))\n    match value:\n        case Outer.Nested(Inner.Data(int64 as number)):\n            print(number)\n        case Outer.Nested(Inner.Data(str as text)):\n            print(text)\n";
    let mir = lower_source_to_mir(source).expect("nested enum-union pattern must lower");
    let output = run_mir(&mir).expect("every nested projection has its own dominating proof");
    assert_eq!(output.stdout, "42\n");
}

fn forged_nested_enum_mir(with_wrong_inner_proof: bool) -> MirModule {
    let source = "enum Inner:\n    Data(int64)\n    Other(int64)\nenum Outer:\n    Nested(Inner)\ndef main():\n    value = Outer.Nested(Inner.Data(42))\n    number = 0\n";
    let mir = lower_source_to_mir(source).expect("nested enum baseline must lower");
    let mut encoded = serde_json::to_value(mir).unwrap();
    let main = encoded["functions"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|function| function["name"] == "main")
        .unwrap();
    let outer_terminator = if with_wrong_inner_proof {
        json!({ "Match": {
            "scrutinee": { "Place": "value.__variant_payload_Nested_0" },
            "arms": [{ "enum_name": "Inner", "variant_name": "Other",
                "wildcard": false, "label": "inner" }],
            "otherwise": "done"
        }})
    } else {
        json!({ "Goto": "inner" })
    };
    main["blocks"] = json!([
        { "label": "entry", "instructions": [], "terminator": { "Match": {
            "scrutinee": { "Place": "value" },
            "arms": [{ "enum_name": "Outer", "variant_name": "Nested",
                "wildcard": false, "label": "outer" }],
            "otherwise": "done"
        }}},
        { "label": "outer", "instructions": [], "terminator": outer_terminator },
        { "label": "inner", "instructions": [{ "BeginLoan": {
            "loan": "number",
            "source": "value.__variant_payload_Nested_0.__variant_payload_Data_0",
            "mutable": false
        }}], "terminator": { "Return": "Unit" }},
        { "label": "done", "instructions": [], "terminator": { "Return": "Unit" }}
    ]);
    serde_json::from_value(encoded).unwrap()
}

#[test]
fn nested_enum_projection_rejects_missing_inner_proof() {
    let error = run_mir(&forged_nested_enum_mir(false))
        .expect_err("outer proof alone cannot authorize an inner enum payload");
    assert!(error.message.contains("matching variant proof"), "{error}");
}

#[test]
fn nested_enum_projection_rejects_wrong_inner_proof() {
    let error = run_mir(&forged_nested_enum_mir(true))
        .expect_err("Other proof cannot authorize the Data payload");
    assert!(error.message.contains("matching variant proof"), "{error}");
}

#[test]
fn union_member_enum_and_nested_union_proofs_compose() {
    let source = "enum Inner:\n    Data(int64 | str)\ndef main():\n    value: Inner | None = Inner.Data(42)\n    match value:\n        case Inner as inner:\n            match inner:\n                case Inner.Data(int64 as number):\n                    print(number)\n                case Inner.Data(str as text):\n                    print(text)\n        case None:\n            pass\n";
    let mir = lower_source_to_mir(source).expect("union-enum-union pattern must lower");
    let output = run_mir(&mir).expect("nested proofs must follow the resolved union member place");
    assert_eq!(output.stdout, "42\n");
}

#[test]
fn union_tag_test_rejects_an_unproven_enum_payload_ancestor() {
    let source = "enum Inner:\n    Data(int64 | str)\nenum Outer:\n    Nested(Inner)\ndef main():\n    value = Outer.Nested(Inner.Data(42))\n    choice: int64 | str = 1\n    flag = false\n";
    let mir = lower_source_to_mir(source).expect("nested tag-test baseline must lower");
    let mut encoded = serde_json::to_value(mir).unwrap();
    let main = encoded["functions"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|function| function["name"] == "main")
        .unwrap();
    let union_type = main["local_types"]
        .as_array()
        .unwrap()
        .iter()
        .find(|local| local["name"] == "choice")
        .unwrap()["ty"]
        .clone();
    main["blocks"] = json!([
        { "label": "entry", "instructions": [], "terminator": { "Match": {
            "scrutinee": { "Place": "value" },
            "arms": [{ "enum_name": "Outer", "variant_name": "Nested",
                "wildcard": false, "label": "outer" }], "otherwise": "done"
        }}},
        { "label": "outer", "instructions": [{ "Assign": {
            "target": "flag", "value": { "UnionTagTest": {
                "place": "value.__variant_payload_Nested_0.__variant_payload_Data_0",
                "union_type": union_type, "member_index": 0
            }}
        }}], "terminator": { "Return": "Unit" }},
        { "label": "done", "instructions": [], "terminator": { "Return": "Unit" }}
    ]);
    let mir: MirModule = serde_json::from_value(encoded).unwrap();
    let error = run_mir(&mir).expect_err("tag test cannot bypass an unproven enum ancestor");
    assert!(error.message.contains("matching variant proof"), "{error}");
}

#[test]
fn projected_assignment_requires_a_dominating_enum_proof() {
    let mir = lower_source_to_mir(ENUM_PATTERN_SOURCE).unwrap();
    let mut encoded = serde_json::to_value(mir).unwrap();
    main_block_mut(&mut encoded, "entry")["instructions"] = json!([{ "Assign": {
        "target": "boxed.__variant_payload_Item_0", "value": { "Use": { "Int": 2 }}
    }}]);
    let error = run_mir(&serde_json::from_value(encoded).unwrap())
        .expect_err("a projected write needs the matching variant proof");
    assert!(error.message.contains("matching variant proof"), "{error}");
}

#[test]
fn read_loan_target_requires_a_dominating_enum_proof() {
    let mir = lower_source_to_mir(ENUM_PATTERN_SOURCE).unwrap();
    let mut encoded = serde_json::to_value(mir).unwrap();
    main_block_mut(&mut encoded, "entry")["instructions"]
        .as_array_mut()
        .unwrap()
        .extend([
            json!({ "BeginLoan": { "loan": "source_loan", "source": "payload", "mutable": false }}),
            json!({ "ReadLoan": { "target": "boxed.__variant_payload_Item_0", "loan": "source_loan" }}),
            json!({ "EndLoan": { "loan": "source_loan" }})
        ]);
    let error = run_mir(&serde_json::from_value(encoded).unwrap())
        .expect_err("ReadLoan cannot write an unproven payload projection");
    assert!(error.message.contains("matching variant proof"), "{error}");
}

fn forged_dotted_loan_access(instruction: Value, mutable: bool) -> MirModule {
    let mir = lower_source_to_mir(ENUM_PATTERN_SOURCE).unwrap();
    let mut encoded = serde_json::to_value(mir).unwrap();
    main_block_mut(&mut encoded, "entry")["instructions"]
        .as_array_mut()
        .unwrap()
        .extend([
            json!({ "BeginLoan": { "loan": "enum_loan", "source": "boxed", "mutable": mutable }}),
            instruction,
            json!({ "EndLoan": { "loan": "enum_loan" }}),
        ]);
    serde_json::from_value(encoded).unwrap()
}

#[test]
fn dotted_read_loan_requires_a_dominating_enum_proof() {
    let mir = forged_dotted_loan_access(
        json!({ "ReadLoan": {
            "target": "payload", "loan": "enum_loan.__variant_payload_Item_0"
        }}),
        false,
    );
    let error = run_mir(&mir).expect_err("a dotted loan read cannot bypass variant proof");
    assert!(error.message.contains("matching variant proof"), "{error}");
}

#[test]
fn dotted_write_loan_requires_a_dominating_enum_proof() {
    let mir = forged_dotted_loan_access(
        json!({ "WriteLoan": {
            "loan": "enum_loan.__variant_payload_Item_0", "value": { "Use": { "Int": 2 }}
        }}),
        true,
    );
    let error = run_mir(&mir).expect_err("a dotted loan write cannot bypass variant proof");
    assert!(error.message.contains("matching variant proof"), "{error}");
}

#[test]
fn variant_payload_requires_a_dominating_enum_fact() {
    let mut encoded =
        serde_json::to_value(lower_source_to_mir(ENUM_PATTERN_SOURCE).unwrap()).unwrap();
    main_block_mut(&mut encoded, "entry")["instructions"]
        .as_array_mut()
        .unwrap()
        .push(
            json!({ "Assign": { "target": "payload", "value": { "VariantPayload": {
                "scrutinee": { "Place": "boxed" }, "variant_name": "Item", "index": 0
            }}}}),
        );
    let error = run_mir(&serde_json::from_value(encoded).unwrap())
        .expect_err("direct payload extraction requires the matching successor fact");
    assert!(error.message.contains("matching variant proof"), "{error}");
}

#[test]
fn variant_payload_metadata_must_match_the_proven_variant_and_layout() {
    for (variant_name, index) in [("Other", 0), ("Item", 1)] {
        let mir = forged_enum_pattern_mir(
            ENUM_PATTERN_SOURCE,
            vec![
                json!({ "Assign": { "target": "payload", "value": { "VariantPayload": {
                    "scrutinee": { "Place": "boxed" }, "variant_name": variant_name, "index": index
                }}}}),
            ],
        );
        let error = run_mir(&mir).expect_err("payload metadata must match enum layout and proof");
        assert!(
            error.message.contains("matching variant proof")
                || error.message.contains("out of bounds"),
            "{variant_name}[{index}]: {error}"
        );
    }
}

#[test]
fn for_range_binding_requires_a_dominating_enum_proof() {
    let source =
        "enum Boxed:\n    Item(int64)\ndef main():\n    boxed = Boxed.Item(1)\n    values = range(1)\n";
    let mut encoded = serde_json::to_value(lower_source_to_mir(source).unwrap()).unwrap();
    let main = encoded["functions"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|function| function["name"] == "main")
        .unwrap();
    let initialization = main["blocks"]
        .as_array()
        .unwrap()
        .iter()
        .find(|block| block["label"] == main["entry"])
        .unwrap()["instructions"]
        .clone();
    main["blocks"] = json!([
        { "label": "entry", "instructions": initialization, "terminator": { "ForRange": {
            "binding": "boxed.__variant_payload_Item_0", "iterable": { "Place": "values" },
            "body_label": "body", "exit_label": "done"
        }}},
        { "label": "body", "instructions": [], "terminator": { "Goto": "done" }},
        { "label": "done", "instructions": [], "terminator": { "Return": "Unit" }}
    ]);
    let error = run_mir(&serde_json::from_value(encoded).unwrap())
        .expect_err("a loop binding cannot write an unproven payload projection");
    assert!(error.message.contains("matching variant proof"), "{error}");
}

#[test]
fn projected_assignment_requires_a_dominating_union_proof() {
    let mir = lower_source_to_mir("def main():\n    value: int64 | str = 1\n").unwrap();
    let mut encoded = serde_json::to_value(mir).unwrap();
    main_block_mut(&mut encoded, "entry")["instructions"] = json!([{ "Assign": {
        "target": "value.__union_payload_0", "value": { "Use": { "Int": 2 }}
    }}]);
    let error = run_mir(&serde_json::from_value(encoded).unwrap())
        .expect_err("a projected write needs the matching union proof");
    assert!(error.message.contains("matching tag proof"), "{error}");
}

fn forged_enum_parameter_projection(passing: &str, instruction: Value) -> MirModule {
    let source = "enum Boxed:\n    Item(int64)\ndef inspect(value: Boxed):\n    payload = 0\ndef main():\n    pass\n";
    let mut encoded = serde_json::to_value(lower_source_to_mir(source).unwrap()).unwrap();
    let function = encoded["functions"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|function| function["name"] == "inspect")
        .unwrap();
    function["params"][0]["passing"] = json!(passing);
    function["blocks"] = json!([
        { "label": "entry", "instructions": [], "terminator": { "Match": {
            "scrutinee": { "Place": "value" }, "arms": [{ "enum_name": "Boxed",
            "variant_name": "Item", "wildcard": false, "label": "matched" }],
            "otherwise": "done" }}},
        { "label": "matched", "instructions": [instruction], "terminator": { "Return": "Unit" }},
        { "label": "done", "instructions": [], "terminator": { "Return": "Unit" }}
    ]);
    serde_json::from_value(encoded).unwrap()
}

#[test]
fn projected_assignment_cannot_mutate_a_shared_parameter() {
    let mir = forged_enum_parameter_projection(
        "Borrow",
        json!({ "Assign": {
            "target": "value.__variant_payload_Item_0", "value": { "Use": { "Int": 2 }}
        }}),
    );
    let error = run_mir(&mir).expect_err("shared authority cannot mutate an enum payload");
    assert!(error.message.contains("mutable authority"), "{error}");
}

#[test]
fn projected_move_requires_owned_authority() {
    for passing in ["Borrow", "BorrowMut"] {
        let mir = forged_enum_parameter_projection(
            passing,
            json!({ "Eval": {
                "value": { "MovePlace": "value.__variant_payload_Item_0" }
            }}),
        );
        let error = run_mir(&mir).expect_err("borrowed authority cannot move an enum payload");
        assert!(
            error.message.contains("owned authority"),
            "{passing}: {error}"
        );
    }
}

#[test]
fn destructive_variant_payload_requires_owned_parameter_authority() {
    for passing in ["Borrow", "BorrowMut"] {
        let mir = forged_enum_parameter_projection(
            passing,
            json!({ "Assign": { "target": "payload", "value": { "VariantPayload": {
                "scrutinee": { "MovePlace": "value" }, "variant_name": "Item", "index": 0
            }}}}),
        );
        let error = run_mir(&mir).expect_err("borrowed roots cannot be destructively extracted");
        assert!(
            error.message.contains("owned authority"),
            "{passing}: {error}"
        );
    }
}

#[test]
fn consuming_match_requires_owned_parameter_authority() {
    for passing in ["Borrow", "BorrowMut"] {
        let mir = forged_enum_parameter_projection(passing, json!({ "Eval": { "value": "Unit" }}));
        let mut encoded = serde_json::to_value(mir).unwrap();
        let function = encoded["functions"]
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .find(|function| function["name"] == "inspect")
            .unwrap();
        let entry = function["entry"].as_str().unwrap().to_owned();
        let block = function["blocks"]
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .find(|block| block["label"] == entry)
            .unwrap();
        block["terminator"]["Match"]["scrutinee"] = json!({ "MovePlace": "value" });
        let error = run_mir(&serde_json::from_value(encoded).unwrap())
            .expect_err("a consuming match requires owned parameter authority");
        assert!(
            error.message.contains("owned authority"),
            "{passing}: {error}"
        );
    }
}

#[test]
fn tuple_take_requires_owned_parameter_authority() {
    let source = "def inspect(value: (int64, int64)):\n    payload = 0\ndef main():\n    pass\n";
    let mut encoded = serde_json::to_value(lower_source_to_mir(source).unwrap()).unwrap();
    let function = encoded["functions"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|function| function["name"] == "inspect")
        .unwrap();
    function["params"][0]["passing"] = json!("Borrow");
    let entry = function["entry"].as_str().unwrap().to_owned();
    function["blocks"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|block| block["label"] == entry)
        .unwrap()["instructions"]
        .as_array_mut()
        .unwrap()
        .push(json!({ "Assign": { "target": "payload", "value": {
            "TupleTakeElement": { "place": "value", "index": 0,
                "element_type": { "Named": ["int64", []] } }
        }}}));
    let error = run_mir(&serde_json::from_value(encoded).unwrap())
        .expect_err("a tuple take cannot consume a borrowed parameter");
    assert!(error.message.contains("owned authority"), "{error}");
}

#[test]
fn consuming_nested_noncopy_match_can_backtrack_before_payload_moves() {
    let source = include_str!(
        "fixtures/run-pass/consuming_nested_noncopy_match_backtracks_before_moving.au"
    );
    let mir = lower_source_to_mir(source).expect("maintained consuming match must lower");
    let output = run_mir(&mir).expect("failed alternatives must preserve later variant proofs");
    assert_eq!(output.stdout, "pair\n7\nsolo\n1\n0\n");
}

#[test]
fn destructive_variant_payload_cannot_take_the_same_noncopy_slot_twice() {
    let source = "enum Boxed:\n    Item(list[int64])\ndef main():\n    boxed = Boxed.Item([1])\n    payload = [0]\n";
    let take = json!({ "Assign": { "target": "payload", "value": { "VariantPayload": {
        "scrutinee": { "MovePlace": "boxed" }, "variant_name": "Item", "index": 0
    }}}});
    let mir = forged_enum_pattern_mir(source, vec![take.clone(), take]);
    let error = run_mir(&mir).expect_err("one nominal payload slot can be moved only once");
    assert!(
        error.message.contains("already-taken enum payload"),
        "{error}"
    );
}

#[test]
fn projected_mutation_and_move_accept_sufficient_authority() {
    let mutable = forged_enum_parameter_projection(
        "BorrowMut",
        json!({ "Assign": {
            "target": "value.__variant_payload_Item_0", "value": { "Use": { "Int": 2 }}
        }}),
    );
    run_mir(&mutable).expect("a matching proof plus mutable parameter authority is sufficient");

    let owned = forged_enum_parameter_projection(
        "Value",
        json!({ "Eval": {
            "value": { "MovePlace": "value.__variant_payload_Item_0" }
        }}),
    );
    run_mir(&owned).expect("a matching proof plus owned parameter authority is sufficient");
}

fn forged_borrowed_noncopy_laundering(extraction: Value) -> MirModule {
    let source = "enum Boxed:\n    Item(list[int64])\ndef inspect(value: Boxed):\n    pass\ndef main():\n    pass\n";
    let mut encoded = serde_json::to_value(lower_source_to_mir(source).unwrap()).unwrap();
    let function = encoded["functions"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|function| function["name"] == "inspect")
        .unwrap();
    function["params"][0]["passing"] = json!("Borrow");
    function["local_types"] = json!([{
        "name": "%t999", "ty": { "Named": ["list", [{ "Named": ["int64", []] }]] }
    }]);
    function["blocks"] = json!([
        { "label": "entry", "instructions": [], "terminator": { "Match": {
            "scrutinee": { "Place": "value" }, "arms": [{ "enum_name": "Boxed",
            "variant_name": "Item", "wildcard": false, "label": "matched" }],
            "otherwise": "done" }}},
        { "label": "matched", "instructions": [
            { "Assign": { "target": "%t999", "value": extraction }},
            { "Eval": { "value": { "MovePlace": "%t999" }}}
        ], "terminator": { "Return": "Unit" }},
        { "label": "done", "instructions": [], "terminator": { "Return": "Unit" }}
    ]);
    serde_json::from_value(encoded).unwrap()
}

#[test]
fn borrowed_variant_payload_temporary_cannot_be_laundered_into_owned_value() {
    let mir = forged_borrowed_noncopy_laundering(json!({ "VariantPayload": {
        "scrutinee": { "Place": "value" }, "variant_name": "Item", "index": 0
    }}));
    let error = run_mir(&mir).expect_err("a borrowed non-Copy payload temp must stay borrowed");
    assert!(error.message.contains("owned authority"), "{error}");
}

#[test]
fn borrowed_whole_value_temporary_cannot_be_laundered_into_owned_value() {
    let mir = forged_borrowed_noncopy_laundering(json!({ "Use": { "Place": "value" } }));
    let error = run_mir(&mir).expect_err("a borrowed non-Copy value temp must stay borrowed");
    assert!(error.message.contains("owned authority"), "{error}");
}

#[test]
fn borrowed_copy_value_may_be_copied_then_moved_from_its_temporary() {
    let source = "def inspect(value: int64):\n    pass\ndef main():\n    pass\n";
    let mut encoded = serde_json::to_value(lower_source_to_mir(source).unwrap()).unwrap();
    let function = encoded["functions"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|function| function["name"] == "inspect")
        .unwrap();
    function["params"][0]["passing"] = json!("Borrow");
    function["local_types"] = json!([{
        "name": "%t999", "ty": { "Named": ["int64", []] }
    }]);
    function["blocks"] = json!([{ "label": "entry", "instructions": [
        { "Assign": { "target": "%t999", "value": { "Use": { "Place": "value" } }}},
        { "Eval": { "value": { "MovePlace": "%t999" }}}
    ], "terminator": { "Return": "Unit" }}]);
    run_mir(&serde_json::from_value(encoded).unwrap())
        .expect("Copy values materialize independent owned temporaries");
}

fn assert_interpreter_and_native_reject_owned_laundering(mir: &MirModule) {
    let interpreted = run_mir(mir);
    let native = emit_host_native_object(mir);
    let interpreted = interpreted.expect_err("interpreter boundary must reject laundering");
    eprintln!("interpreter rejection: {interpreted}");
    assert!(
        interpreted.message.contains("owned authority") || interpreted.message.contains("contract"),
        "{interpreted}"
    );
    let native = native.expect_err("native boundary must reject laundering");
    eprintln!("native rejection: {native}");
    assert!(
        native.contains("owned authority") || native.contains("contract"),
        "{native}"
    );
}

#[test]
fn borrowed_noncopy_value_cannot_be_laundered_through_a_tuple() {
    let source = "enum Boxed:\n    Item(list[int64])\ndef inspect(value: Boxed):\n    pass\ndef main():\n    pass\n";
    let mut encoded = serde_json::to_value(lower_source_to_mir(source).unwrap()).unwrap();
    let function = encoded["functions"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|function| function["name"] == "inspect")
        .unwrap();
    function["params"][0]["passing"] = json!("Borrow");
    let boxed = json!({ "Named": ["Boxed", []] });
    function["local_types"] = json!([
        { "name": "%t998", "ty": { "Tuple": [boxed.clone()] }},
        { "name": "%t999", "ty": boxed.clone() }
    ]);
    function["blocks"] = json!([{ "label": "entry", "instructions": [
        { "Assign": { "target": "%t998", "value": { "TupleLiteral": {
            "elements": [{ "Place": "value" }], "element_types": [boxed.clone()]
        }}}},
        { "Assign": { "target": "%t999", "value": { "TupleTakeElement": {
            "place": "%t998", "index": 0, "element_type": boxed
        }}}},
        { "Eval": { "value": { "MovePlace": "%t999" }}}
    ], "terminator": { "Return": "Unit" }}]);
    assert_interpreter_and_native_reject_owned_laundering(
        &serde_json::from_value(encoded).unwrap(),
    );
}

#[test]
fn tuple_element_preserves_a_borrowed_noncopy_origin() {
    let (mut encoded, boxed) = forged_boxed_borrow_function("");
    let function = encoded["functions"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|function| function["name"] == "inspect")
        .unwrap();
    function["local_types"] = json!([
        { "name": "%t998", "ty": { "Tuple": [boxed.clone()] }},
        { "name": "%t999", "ty": boxed.clone() }
    ]);
    function["blocks"] = json!([{ "label": "entry", "instructions": [
        { "Assign": { "target": "%t998", "value": { "TupleLiteral": {
            "elements": [{ "Place": "value" }], "element_types": [boxed.clone()]
        }}}},
        { "Assign": { "target": "%t999", "value": { "TupleElement": {
            "tuple": { "Place": "%t998" }, "index": 0, "element_type": boxed
        }}}},
        { "Eval": { "value": { "MovePlace": "%t999" }}}
    ], "terminator": { "Return": "Unit" }}]);
    assert_interpreter_and_native_reject_owned_laundering(
        &serde_json::from_value(encoded).unwrap(),
    );
}

fn forged_whole_tuple_borrowed_escape(overwrite_sibling: bool) -> MirModule {
    let extra = "def consume_pair(value: own (Boxed, int64)):\n    pass\n";
    let (mut encoded, boxed) = forged_boxed_borrow_function(extra);
    let function = encoded["functions"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|function| function["name"] == "inspect")
        .unwrap();
    let tuple = json!({ "Tuple": [boxed.clone(), { "Named": ["int64", []] }] });
    function["local_types"] = json!([{ "name": "%t998", "ty": tuple.clone() }]);
    let mut instructions = vec![json!({ "Assign": { "target": "%t998", "value": {
        "TupleLiteral": { "elements": [{ "Place": "value" }, { "Int": 0 }],
            "element_types": [boxed, { "Named": ["int64", []] }] }
    }}})];
    if overwrite_sibling {
        instructions.push(json!({ "Assign": { "target": "%t998.1", "value": {
            "Use": { "Int": 1 }
        }}}));
    }
    instructions.push(json!({ "Assign": { "target": "%t999", "value": { "Call": {
        "callee": { "Name": "consume_pair" }, "args": [{ "name": null,
            "value": { "MovePlace": "%t998" }, "writeback_place": null }]
    }}}}));
    function["blocks"] = json!([{ "label": "entry", "instructions": instructions,
        "terminator": { "Return": "Unit" }}]);
    serde_json::from_value(encoded).unwrap()
}

#[test]
fn whole_tuple_move_observes_borrowed_descendant_origins() {
    assert_interpreter_and_native_reject_owned_laundering(&forged_whole_tuple_borrowed_escape(
        false,
    ));
}

#[test]
fn tuple_sibling_overwrite_preserves_borrowed_descendant_origin_for_whole_move() {
    assert_interpreter_and_native_reject_owned_laundering(&forged_whole_tuple_borrowed_escape(
        true,
    ));
}

#[test]
fn whole_tuple_cleanup_observes_borrowed_descendant_origins() {
    let mut encoded = serde_json::to_value(forged_whole_tuple_borrowed_escape(false)).unwrap();
    let inspect = encoded["functions"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|function| function["name"] == "inspect")
        .unwrap();
    let instructions = inspect["blocks"][0]["instructions"].as_array_mut().unwrap();
    instructions.pop();
    instructions.push(json!({ "PushCleanup": { "place": "%t998" }}));
    let mir: MirModule = serde_json::from_value(encoded).unwrap();
    let interpreted = run_mir(&mir).expect_err("cleanup cannot close borrowed descendants");
    assert!(
        interpreted.message.contains("borrowed value"),
        "{interpreted}"
    );
    let native = emit_host_native_object(&mir)
        .expect_err("native validation must reject cleanup of borrowed descendants");
    assert!(native.contains("borrowed value"), "{native}");
}

fn forged_construct_sibling_borrowed_escape(write_loan: bool) -> MirModule {
    let extra = "class Envelope:\n    item: Boxed\n    flag: int64\ndef consume_envelope(value: own Envelope):\n    pass\n";
    let (mut encoded, _boxed) = forged_boxed_borrow_function(extra);
    let function = encoded["functions"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|function| function["name"] == "inspect")
        .unwrap();
    function["local_types"] = json!([{ "name": "%t998", "ty": {
        "Named": ["Envelope", []]
    }}]);
    let mut instructions = vec![json!({ "Assign": { "target": "%t998", "value": {
        "Construct": { "class_name": "Envelope", "fields": [
            { "name": "item", "value": { "Place": "value" }},
            { "name": "flag", "value": { "Int": 0 }}
        ] }
    }}})];
    if write_loan {
        instructions.extend([
            json!({ "BeginLoan": { "loan": "flag_loan", "source": "%t998.flag",
                "mutable": true }}),
            json!({ "WriteLoan": { "loan": "flag_loan", "value": { "Use": { "Int": 1 }}}}),
            json!({ "EndLoan": { "loan": "flag_loan" }}),
        ]);
    } else {
        instructions.push(json!({ "Assign": { "target": "%t998.flag", "value": {
            "Use": { "Int": 1 }
        }}}));
    }
    instructions.push(json!({ "Assign": { "target": "%t999", "value": { "Call": {
        "callee": { "Name": "consume_envelope" }, "args": [{ "name": null,
            "value": { "MovePlace": "%t998" }, "writeback_place": null }]
    }}}}));
    function["blocks"] = json!([{ "label": "entry", "instructions": instructions,
        "terminator": { "Return": "Unit" }}]);
    serde_json::from_value(encoded).unwrap()
}

#[test]
fn construct_sibling_assignment_preserves_conservative_borrowed_origin() {
    assert_interpreter_and_native_reject_owned_laundering(
        &forged_construct_sibling_borrowed_escape(false),
    );
}

#[test]
fn construct_sibling_write_loan_preserves_conservative_borrowed_origin() {
    assert_interpreter_and_native_reject_owned_laundering(
        &forged_construct_sibling_borrowed_escape(true),
    );
}

#[test]
fn read_loan_of_borrowed_noncopy_value_cannot_be_laundered() {
    let source = "def inspect(value: list[int64]):\n    pass\ndef main():\n    pass\n";
    let mut encoded = serde_json::to_value(lower_source_to_mir(source).unwrap()).unwrap();
    let function = encoded["functions"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|function| function["name"] == "inspect")
        .unwrap();
    function["params"][0]["passing"] = json!("Borrow");
    function["local_types"] = json!([{
        "name": "%t999", "ty": { "Named": ["list", [{ "Named": ["int64", []] }]] }
    }]);
    function["blocks"] = json!([{ "label": "entry", "instructions": [
        { "BeginLoan": { "loan": "value_loan", "source": "value", "mutable": false }},
        { "ReadLoan": { "target": "%t999", "loan": "value_loan" }},
        { "EndLoan": { "loan": "value_loan" }},
        { "Eval": { "value": { "MovePlace": "%t999" }}}
    ], "terminator": { "Return": "Unit" }}]);
    assert_interpreter_and_native_reject_owned_laundering(
        &serde_json::from_value(encoded).unwrap(),
    );
}

#[test]
fn borrowed_noncopy_class_value_cannot_be_laundered_through_a_temporary() {
    let source = "class Holder:\n    values: list[int64]\ndef inspect(value: Holder):\n    pass\ndef main():\n    pass\n";
    let mut encoded = serde_json::to_value(lower_source_to_mir(source).unwrap()).unwrap();
    let function = encoded["functions"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|function| function["name"] == "inspect")
        .unwrap();
    function["params"][0]["passing"] = json!("Borrow");
    function["local_types"] = json!([{
        "name": "%t999", "ty": { "Named": ["Holder", []] }
    }]);
    function["blocks"] = json!([{ "label": "entry", "instructions": [
        { "Assign": { "target": "%t999", "value": { "Use": { "Place": "value" } }}},
        { "Eval": { "value": { "MovePlace": "%t999" }}}
    ], "terminator": { "Return": "Unit" }}]);
    assert_interpreter_and_native_reject_owned_laundering(
        &serde_json::from_value(encoded).unwrap(),
    );
}

#[test]
fn borrowed_copy_variant_payload_may_be_moved_from_its_temporary() {
    let mir = forged_enum_parameter_projection(
        "Borrow",
        json!({ "Assign": { "target": "payload", "value": { "VariantPayload": {
            "scrutinee": { "Place": "value" }, "variant_name": "Item", "index": 0
        }}}}),
    );
    let mut encoded = serde_json::to_value(mir).unwrap();
    let function = encoded["functions"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|function| function["name"] == "inspect")
        .unwrap();
    function["blocks"][1]["instructions"]
        .as_array_mut()
        .unwrap()
        .push(json!({ "Eval": { "value": { "MovePlace": "payload" }}}));
    run_mir(&serde_json::from_value(encoded).unwrap())
        .expect("Copy enum payloads materialize independent temporaries");
}

fn forged_boxed_borrow_function(extra_source: &str) -> (Value, Value) {
    let source = format!(
        "enum Boxed:\n    Item(list[int64])\n{extra_source}\ndef inspect(value: Boxed):\n    pass\ndef main():\n    pass\n"
    );
    let mut encoded = serde_json::to_value(lower_source_to_mir(&source).unwrap()).unwrap();
    let boxed = json!({ "Named": ["Boxed", []] });
    let function = encoded["functions"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|function| function["name"] == "inspect")
        .unwrap();
    function["params"][0]["passing"] = json!("Borrow");
    (encoded, boxed)
}

#[test]
fn dotted_aggregate_target_cannot_hide_a_borrowed_noncopy_origin() {
    let (mut encoded, boxed) = forged_boxed_borrow_function("");
    let function = encoded["functions"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|function| function["name"] == "inspect")
        .unwrap();
    function["local_types"] = json!([
        { "name": "%t998", "ty": { "Tuple": [boxed.clone()] }},
        { "name": "%t999", "ty": boxed.clone() }
    ]);
    function["blocks"] = json!([{ "label": "entry", "instructions": [
        { "Assign": { "target": "%t998.0", "value": { "Use": { "Place": "value" } }}},
        { "Assign": { "target": "%t999", "value": { "TupleTakeElement": {
            "place": "%t998", "index": 0, "element_type": boxed
        }}}},
        { "Eval": { "value": { "MovePlace": "%t999" }}}
    ], "terminator": { "Return": "Unit" }}]);
    assert_interpreter_and_native_reject_owned_laundering(
        &serde_json::from_value(encoded).unwrap(),
    );
}

#[test]
fn borrowed_origin_survives_a_cfg_join_with_an_owned_value() {
    let (mut encoded, boxed) = forged_boxed_borrow_function("");
    let function = encoded["functions"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|function| function["name"] == "inspect")
        .unwrap();
    function["params"].as_array_mut().unwrap().push(json!({
        "name": "flag", "passing": "Value", "ty": { "Named": ["bool", []] },
        "default_function": null
    }));
    function["local_types"] = json!([
        { "name": "owned", "ty": boxed.clone() },
        { "name": "%t999", "ty": boxed.clone() }
    ]);
    function["blocks"] = json!([
        { "label": "entry", "instructions": [], "terminator": { "Branch": {
            "condition": { "Place": "flag" }, "then_label": "borrowed", "else_label": "owned"
        }}},
        { "label": "borrowed", "instructions": [{ "Assign": {
            "target": "%t999", "value": { "Use": { "Place": "value" }}
        }}], "terminator": { "Goto": "join" }},
        { "label": "owned", "instructions": [{ "Assign": {
            "target": "%t999", "value": { "Use": { "Place": "owned" }}
        }}], "terminator": { "Goto": "join" }},
        { "label": "join", "instructions": [{ "Eval": {
            "value": { "MovePlace": "%t999" }
        }}], "terminator": { "Return": "Unit" }}
    ]);
    assert_interpreter_and_native_reject_owned_laundering(
        &serde_json::from_value(encoded).unwrap(),
    );
}

#[test]
fn overwriting_the_source_does_not_erase_a_derived_borrowed_origin() {
    let (mut encoded, boxed) = forged_boxed_borrow_function("");
    let function = encoded["functions"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|function| function["name"] == "inspect")
        .unwrap();
    function["local_types"] = json!([
        { "name": "owned", "ty": boxed.clone() },
        { "name": "%t999", "ty": boxed }
    ]);
    function["blocks"] = json!([{ "label": "entry", "instructions": [
        { "Assign": { "target": "%t999", "value": { "Use": { "Place": "value" } }}},
        { "Assign": { "target": "value", "value": { "Use": { "Place": "owned" } }}},
        { "Eval": { "value": { "MovePlace": "%t999" }}}
    ], "terminator": { "Return": "Unit" }}]);
    assert_interpreter_and_native_reject_owned_laundering(
        &serde_json::from_value(encoded).unwrap(),
    );
}

#[test]
fn borrowed_noncopy_place_cannot_bind_a_by_value_parameter() {
    let (mut encoded, _) = forged_boxed_borrow_function("def consume(item: Boxed):\n    pass\n");
    encoded["functions"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|function| function["name"] == "consume")
        .unwrap()["params"][0]["passing"] = json!("Value");
    let function = encoded["functions"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|function| function["name"] == "inspect")
        .unwrap();
    function["blocks"] = json!([{ "label": "entry", "instructions": [{ "Assign": {
        "target": "%t999", "value": { "Call": {
            "callee": { "Name": "consume" }, "args": [{
                "name": null, "value": { "Place": "value" }, "writeback_place": null
            }]
        }}
    }}], "terminator": { "Return": "Unit" }}]);
    assert_interpreter_and_native_reject_owned_laundering(
        &serde_json::from_value(encoded).unwrap(),
    );
}

#[test]
fn borrowed_noncopy_temporary_cannot_escape_through_return_place() {
    let (mut encoded, boxed) = forged_boxed_borrow_function("");
    let function = encoded["functions"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|function| function["name"] == "inspect")
        .unwrap();
    function["return_type"] = boxed.clone();
    function["local_types"] = json!([{ "name": "%t999", "ty": boxed }]);
    function["blocks"] = json!([{ "label": "entry", "instructions": [{ "Assign": {
        "target": "%t999", "value": { "Use": { "Place": "value" }}
    }}], "terminator": { "Return": { "Place": "%t999" }}}]);
    assert_interpreter_and_native_reject_owned_laundering(
        &serde_json::from_value(encoded).unwrap(),
    );
}

#[test]
fn borrowed_noncopy_value_cannot_escape_inside_a_list_wrapper() {
    let (mut encoded, boxed) = forged_boxed_borrow_function("");
    let function = encoded["functions"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|function| function["name"] == "inspect")
        .unwrap();
    function["local_types"] = json!([{
        "name": "%t999", "ty": { "Named": ["list", [boxed.clone()]] }
    }]);
    function["blocks"] = json!([{ "label": "entry", "instructions": [
        { "Assign": { "target": "%t999", "value": { "VecLiteral": {
            "elements": [{ "Place": "value" }], "element_type": boxed
        }}}},
        { "Eval": { "value": { "MovePlace": "%t999" }}}
    ], "terminator": { "Return": "Unit" }}]);
    assert_interpreter_and_native_reject_owned_laundering(
        &serde_json::from_value(encoded).unwrap(),
    );
}

#[test]
fn overwriting_a_tuple_sibling_preserves_borrowed_origin_of_other_element() {
    let (mut encoded, boxed) = forged_boxed_borrow_function("");
    let function = encoded["functions"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|function| function["name"] == "inspect")
        .unwrap();
    function["local_types"] = json!([
        { "name": "%t998", "ty": { "Tuple": [boxed.clone(), { "Named": ["int64", []] }] }},
        { "name": "%t999", "ty": boxed.clone() }
    ]);
    function["blocks"] = json!([{ "label": "entry", "instructions": [
        { "Assign": { "target": "%t998", "value": { "TupleLiteral": {
            "elements": [{ "Place": "value" }, { "Int": 0 }],
            "element_types": [boxed.clone(), { "Named": ["int64", []] }]
        }}}},
        { "Assign": { "target": "%t998.1", "value": { "Use": { "Int": 1 } }}},
        { "Assign": { "target": "%t999", "value": { "TupleTakeElement": {
            "place": "%t998", "index": 0, "element_type": boxed
        }}}},
        { "Eval": { "value": { "MovePlace": "%t999" }}}
    ], "terminator": { "Return": "Unit" }}]);
    assert_interpreter_and_native_reject_owned_laundering(
        &serde_json::from_value(encoded).unwrap(),
    );
}

#[test]
fn borrowed_noncopy_place_cannot_bind_an_owned_method_receiver() {
    let source = "class Holder:\n    values: list[int64]\n    def take(own self):\n        pass\ndef inspect(value: Holder):\n    pass\ndef main():\n    pass\n";
    let mut encoded = serde_json::to_value(lower_source_to_mir(source).unwrap()).unwrap();
    let function = encoded["functions"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|function| function["name"] == "inspect")
        .unwrap();
    function["params"][0]["passing"] = json!("Borrow");
    function["blocks"] = json!([{ "label": "entry", "instructions": [{ "Assign": {
        "target": "%t999", "value": { "Call": {
            "callee": { "Member": { "object": { "Place": "value" }, "field": "take",
                "receiver_place": null }}, "args": []
        }}
    }}], "terminator": { "Return": "Unit" }}]);
    assert_interpreter_and_native_reject_owned_laundering(
        &serde_json::from_value(encoded).unwrap(),
    );
}

#[test]
fn write_loan_cannot_launder_a_borrowed_noncopy_value_into_owned_storage() {
    let source = "def inspect(source: list[int64]):\n    pass\ndef main():\n    pass\n";
    let mut encoded = serde_json::to_value(lower_source_to_mir(source).unwrap()).unwrap();
    let function = encoded["functions"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|function| function["name"] == "inspect")
        .unwrap();
    function["params"][0]["passing"] = json!("Borrow");
    let list = json!({ "Named": ["list", [{ "Named": ["int64", []] }]] });
    function["local_types"] = json!([{ "name": "owned", "ty": list }]);
    function["blocks"] = json!([{ "label": "entry", "instructions": [
        { "BeginLoan": { "loan": "owned_loan", "source": "owned", "mutable": true }},
        { "WriteLoan": { "loan": "owned_loan", "value": { "Use": { "Place": "source" } }}},
        { "EndLoan": { "loan": "owned_loan" }},
        { "Eval": { "value": { "MovePlace": "owned" }}}
    ], "terminator": { "Return": "Unit" }}]);
    assert_interpreter_and_native_reject_owned_laundering(
        &serde_json::from_value(encoded).unwrap(),
    );
}

#[test]
fn fully_overwriting_tainted_tuple_element_clears_only_that_origin() {
    let (mut encoded, boxed) = forged_boxed_borrow_function("");
    let function = encoded["functions"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|function| function["name"] == "inspect")
        .unwrap();
    function["local_types"] = json!([
        { "name": "owned", "ty": boxed.clone() },
        { "name": "%t998", "ty": { "Tuple": [boxed.clone(), { "Named": ["int64", []] }] }},
        { "name": "%t999", "ty": boxed.clone() }
    ]);
    function["blocks"] = json!([{ "label": "entry", "instructions": [
        { "Assign": { "target": "%t998", "value": { "TupleLiteral": {
            "elements": [{ "Place": "value" }, { "Int": 0 }],
            "element_types": [boxed.clone(), { "Named": ["int64", []] }]
        }}}},
        { "Assign": { "target": "%t998.0", "value": { "Use": { "Place": "owned" } }}},
        { "Assign": { "target": "%t999", "value": { "TupleTakeElement": {
            "place": "%t998", "index": 0, "element_type": boxed
        }}}},
        { "Eval": { "value": { "MovePlace": "%t999" }}}
    ], "terminator": { "Return": "Unit" }}]);
    run_mir(&serde_json::from_value(encoded).unwrap())
        .expect("overwriting the tainted element replaces its borrowed provenance");
}

#[test]
fn borrowed_copy_value_may_call_an_owned_receiver_method() {
    let source = "copy class Point:\n    value: int64\n    def take(own self):\n        pass\ndef inspect(value: Point):\n    pass\ndef main():\n    pass\n";
    let mut encoded = serde_json::to_value(lower_source_to_mir(source).unwrap()).unwrap();
    let function = encoded["functions"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|function| function["name"] == "inspect")
        .unwrap();
    function["params"][0]["passing"] = json!("Borrow");
    function["blocks"] = json!([{ "label": "entry", "instructions": [{ "Assign": {
        "target": "%t999", "value": { "Call": {
            "callee": { "Member": { "object": { "Place": "value" }, "field": "take",
                "receiver_place": null }}, "args": []
        }}
    }}], "terminator": { "Return": "Unit" }}]);
    run_mir(&serde_json::from_value(encoded).unwrap())
        .expect("Copy receivers may materialize an owned method value from a shared input");
}

#[test]
fn cleanup_cannot_close_a_whole_shared_borrowed_value() {
    let source = "class Handle:\n    value: int64\n    def close(mut self):\n        pass\ndef inspect(value: Handle):\n    pass\ndef main():\n    pass\n";
    let mut encoded = serde_json::to_value(lower_source_to_mir(source).unwrap()).unwrap();
    let function = encoded["functions"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|function| function["name"] == "inspect")
        .unwrap();
    function["params"][0]["passing"] = json!("Borrow");
    function["blocks"] = json!([{ "label": "entry", "instructions": [
        { "PushCleanup": { "place": "value" }},
        { "PopCleanup": { "place": "value", "cancel_before_cleanup": false }}
    ], "terminator": { "Return": "Unit" }}]);
    let mir: MirModule = serde_json::from_value(encoded).unwrap();
    let interpreted = run_mir(&mir).expect_err("shared values cannot be cleanup targets");
    assert!(
        interpreted.message.contains("mutable authority"),
        "{interpreted}"
    );
    let native = emit_host_native_object(&mir).expect_err("native validation must reject cleanup");
    assert!(native.contains("mutable authority"), "{native}");
}

#[test]
fn consuming_closure_cannot_consume_a_shared_borrowed_capture() {
    let source = "class Holder:\n    values: list[int64]\ndef inspect(value: Holder):\n    take: def() -> int64 = lambda [value]: value.values.len()\n    take()\ndef main():\n    pass\n";
    let mut encoded = serde_json::to_value(lower_source_to_mir(source).unwrap()).unwrap();
    let lambda = encoded["functions"]
        .as_array()
        .unwrap()
        .iter()
        .find_map(|function| {
            function["name"]
                .as_str()
                .filter(|name| name.contains("__lambda"))
        })
        .unwrap()
        .to_owned();
    let function = encoded["functions"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|function| function["name"] == "inspect")
        .unwrap();
    function["params"][0]["passing"] = json!("Borrow");
    for local in function["local_types"].as_array_mut().unwrap() {
        if local["name"] == "take" {
            local["ty"] = json!({ "Closure": {
                "params": [], "return_type": { "Named": ["int64", []] },
                "captures": [], "call_kind": "Consuming"
            }});
        }
    }
    function["blocks"][0]["instructions"] = json!([
        { "Assign": { "target": "take", "value": { "Closure": {
            "function": lambda,
            "signature": { "Function": { "params": [],
                "return_type": { "Named": ["int64", []] }}},
            "captures": [{ "name": "value", "value": { "Place": "value" },
                "ty": { "Named": ["Holder", []] }, "passing": "Value",
                "source_place": null, "resolve_source_at_capture": false }],
            "consuming": true
        }}}},
        { "Assign": { "target": "%t0", "value": { "Call": {
            "callee": { "Value": { "Place": "take" }}, "args": []
        }}}},
        { "Eval": { "value": { "Place": "%t0" }}}
    ]);
    assert_interpreter_and_native_reject_owned_laundering(
        &serde_json::from_value(encoded).unwrap(),
    );
}

#[test]
fn mutable_parameter_cannot_write_back_a_borrowed_noncopy_origin() {
    let source = "class Holder:\n    values: list[int64]\ndef overwrite(source: Holder, target: mut Holder):\n    pass\ndef main():\n    pass\n";
    let mut encoded = serde_json::to_value(lower_source_to_mir(source).unwrap()).unwrap();
    let function = encoded["functions"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|function| function["name"] == "overwrite")
        .unwrap();
    function["params"][0]["passing"] = json!("Borrow");
    function["params"][1]["passing"] = json!("BorrowMut");
    function["blocks"] = json!([{ "label": "entry", "instructions": [{ "Assign": {
        "target": "target", "value": { "Use": { "Place": "source" }}
    }}], "terminator": { "Return": "Unit" }}]);
    assert_interpreter_and_native_reject_owned_laundering(
        &serde_json::from_value(encoded).unwrap(),
    );
}

#[test]
fn mutable_parameter_may_write_back_an_owned_noncopy_value() {
    let source = "class Holder:\n    values: list[int64]\ndef overwrite(source: own Holder, target: mut Holder):\n    pass\ndef main():\n    pass\n";
    let mut encoded = serde_json::to_value(lower_source_to_mir(source).unwrap()).unwrap();
    let function = encoded["functions"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|function| function["name"] == "overwrite")
        .unwrap();
    function["params"][0]["passing"] = json!("Value");
    function["params"][1]["passing"] = json!("BorrowMut");
    function["blocks"] = json!([{ "label": "entry", "instructions": [{ "Assign": {
        "target": "target", "value": { "Use": { "MovePlace": "source" }}
    }}], "terminator": { "Return": "Unit" }}]);
    run_mir(&serde_json::from_value(encoded).unwrap())
        .expect("owned values may flow through a mutable writeback");
}

#[test]
fn start_task_cannot_consume_a_closure_with_shared_borrowed_capture() {
    let source = "class Holder:\n    values: list[int64]\ndef consume(job: own def() -> int64):\n    pass\ndef inspect(value: Holder):\n    take: def() -> int64 = lambda: 1\n    take()\ndef main():\n    pass\n";
    let mut encoded = serde_json::to_value(lower_source_to_mir(source).unwrap()).unwrap();
    let function = encoded["functions"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|function| function["name"] == "inspect")
        .unwrap();
    function["params"][0]["passing"] = json!("Borrow");
    let lambda = function["blocks"][0]["instructions"]
        .as_array()
        .unwrap()
        .iter()
        .find_map(|instruction| {
            let value = &instruction["Assign"]["value"];
            value["Use"]["Function"]["name"]
                .as_str()
                .or_else(|| value["Closure"]["function"].as_str())
        })
        .unwrap()
        .to_owned();
    function["local_types"]
        .as_array_mut()
        .unwrap()
        .retain(|local| local["name"] != "%t1");
    for local in function["local_types"].as_array_mut().unwrap() {
        if local["name"] == "take" {
            local["ty"] = json!({ "Closure": {
                "params": [], "return_type": { "Named": ["int64", []] },
                "captures": [], "call_kind": "Consuming"
            }});
        }
    }
    function["blocks"][0]["instructions"] = json!([
        { "Assign": { "target": "take", "value": { "Closure": {
            "function": lambda, "signature": { "Function": { "params": [],
                "return_type": { "Named": ["int64", []] }}},
            "captures": [{ "name": "value", "value": { "Place": "value" },
                "ty": { "Named": ["Holder", []] }, "passing": "Value",
                "source_place": null, "resolve_source_at_capture": false }],
            "consuming": true
        }}}},
        { "Assign": { "target": "%t0", "value": { "StartTask": {
            "returns_handle": false, "result_is_copy": true, "stack_size": null,
            "task_group": "Unit", "function": { "Place": "take" }, "args": [],
            "span": { "line": 1, "column": 1 }
        }}}}
    ]);
    assert_interpreter_and_native_reject_owned_laundering(
        &serde_json::from_value(encoded).unwrap(),
    );
}

#[test]
fn start_task_cannot_outlive_a_repeatable_closure_shared_borrowed_capture() {
    let source = "class Holder:\n    values: list[int64]\ndef inspect(value: Holder):\n    take: def() -> int64 = lambda: 1\n    take()\ndef main():\n    pass\n";
    let mut encoded = serde_json::to_value(lower_source_to_mir(source).unwrap()).unwrap();
    let function = encoded["functions"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|function| function["name"] == "inspect")
        .unwrap();
    function["params"][0]["passing"] = json!("Borrow");
    let lambda = function["blocks"][0]["instructions"][0]["Assign"]["value"]["Use"]["Function"]
        ["name"]
        .as_str()
        .unwrap()
        .to_owned();
    function["local_types"]
        .as_array_mut()
        .unwrap()
        .retain(|local| local["name"] != "%t1");
    for local in function["local_types"].as_array_mut().unwrap() {
        if local["name"] == "take" {
            local["ty"] = json!({ "Closure": {
                "params": [], "return_type": { "Named": ["int64", []] },
                "captures": [], "call_kind": "Repeatable"
            }});
        }
    }
    function["blocks"][0]["instructions"] = json!([
        { "Assign": { "target": "take", "value": { "Closure": {
            "function": lambda, "signature": { "Function": { "params": [],
                "return_type": { "Named": ["int64", []] }}},
            "captures": [{ "name": "value", "value": { "Place": "value" },
                "ty": { "Named": ["Holder", []] }, "passing": "Borrow",
                "source_place": "value", "resolve_source_at_capture": false }],
            "consuming": false
        }}}},
        { "Assign": { "target": "%t0", "value": { "StartTask": {
            "returns_handle": false, "result_is_copy": true, "stack_size": null,
            "task_group": "Unit", "function": { "Place": "take" }, "args": [],
            "span": { "line": 1, "column": 1 }
        }}}}
    ]);
    assert_interpreter_and_native_reject_owned_laundering(
        &serde_json::from_value(encoded).unwrap(),
    );
}

#[test]
fn local_repeatable_closure_with_borrowed_capture_remains_valid() {
    let source = "class Holder:\n    values: list[int64]\ndef inspect(value: Holder) -> int64:\n    take: def() -> int64 = lambda [value]: value.values.len()\n    return take()\ndef main():\n    pass\n";
    let mir = lower_source_to_mir(source).expect("valid repeatable closure must lower");
    run_mir(&mir).expect("local repeatable closure capture must validate in MIR");
    emit_host_native_object(&mir).expect("local repeatable closure capture must emit natively");
}

fn forged_repeatable_borrowed_closure_escape(
    as_function_type: bool,
    via_return: bool,
) -> MirModule {
    let source = "class Holder:\n    values: list[int64]\nclass Runner:\n    job: def() -> int64\ndef inspect(value: Holder):\n    take: def() -> int64 = lambda [value]: value.values.len()\n    take()\ndef main():\n    pass\n";
    let mut encoded = serde_json::to_value(lower_source_to_mir(source).unwrap()).unwrap();
    let lambda = encoded["functions"]
        .as_array()
        .unwrap()
        .iter()
        .find_map(|function| {
            function["name"]
                .as_str()
                .filter(|name| name.contains("__lambda"))
        })
        .unwrap()
        .to_owned();
    let function = encoded["functions"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|function| function["name"] == "inspect")
        .unwrap();
    function["params"][0]["passing"] = json!("Borrow");
    let closure_type = json!({ "Closure": {
        "params": [], "return_type": { "Named": ["int64", []] },
        "captures": [], "call_kind": "Repeatable"
    }});
    let stored_type = if as_function_type {
        json!({ "Function": { "params": [], "return_type": { "Named": ["int64", []] }}})
    } else {
        closure_type.clone()
    };
    function["local_types"]
        .as_array_mut()
        .unwrap()
        .retain(|local| local["name"] != "%t1");
    for local in function["local_types"].as_array_mut().unwrap() {
        if local["name"] == "take" {
            local["ty"] = stored_type.clone();
        }
    }
    let closure = json!({ "Closure": {
        "function": lambda, "signature": { "Function": { "params": [],
            "return_type": { "Named": ["int64", []] }}},
        "captures": [{ "name": "value", "value": { "Place": "value" },
            "ty": { "Named": ["Holder", []] }, "passing": "Borrow",
            "source_place": "value", "resolve_source_at_capture": false }],
        "consuming": false
    }});
    if via_return {
        function["return_type"] = closure_type;
        function["blocks"][0]["instructions"] = json!([{ "Assign": {
            "target": "take", "value": closure
        }}]);
        function["blocks"][0]["terminator"] = json!({ "Return": { "Place": "take" }});
    } else {
        function["blocks"][0]["instructions"] = json!([
            { "Assign": { "target": "take", "value": closure }},
            { "Assign": { "target": "%t0", "value": { "StartTask": {
                "returns_handle": false, "result_is_copy": true, "stack_size": null,
                "task_group": "Unit", "function": { "Place": "take" }, "args": [],
                "span": { "line": 1, "column": 1 }
            }}}}
        ]);
    }
    serde_json::from_value(encoded).unwrap()
}

#[test]
fn forged_function_type_cannot_hide_borrowed_closure_from_start_task() {
    assert_interpreter_and_native_reject_owned_laundering(
        &forged_repeatable_borrowed_closure_escape(true, false),
    );
}

#[test]
fn repeatable_closure_with_borrowed_capture_cannot_escape_through_return() {
    assert_interpreter_and_native_reject_owned_laundering(
        &forged_repeatable_borrowed_closure_escape(false, true),
    );
}

#[test]
fn moved_repeatable_closure_with_borrowed_capture_cannot_escape_through_return() {
    let mir = forged_repeatable_borrowed_closure_escape(false, true);
    let mut encoded = serde_json::to_value(mir).unwrap();
    let function = encoded["functions"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|function| function["name"] == "inspect")
        .unwrap();
    function["blocks"][0]["terminator"] = json!({ "Return": { "MovePlace": "take" }});
    assert_interpreter_and_native_reject_owned_laundering(
        &serde_json::from_value(encoded).unwrap(),
    );
}

#[test]
fn tuple_wrapped_moved_borrowed_closure_cannot_escape_through_return() {
    let mir = forged_repeatable_borrowed_closure_escape(false, true);
    let mut encoded = serde_json::to_value(mir).unwrap();
    let function = encoded["functions"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|function| function["name"] == "inspect")
        .unwrap();
    let closure_type = function["return_type"].clone();
    function["return_type"] = json!({ "Tuple": [closure_type.clone()] });
    function["local_types"].as_array_mut().unwrap().push(json!({
        "name": "%t998", "ty": { "Tuple": [closure_type] }
    }));
    function["blocks"][0]["instructions"]
        .as_array_mut()
        .unwrap()
        .push(json!({
            "Assign": { "target": "%t998", "value": { "TupleLiteral": {
                "elements": [{ "MovePlace": "take" }], "element_types": [closure_type],
                "result_is_copy": false
            }}}
        }));
    function["blocks"][0]["terminator"] = json!({ "Return": { "MovePlace": "%t998" }});
    assert_interpreter_and_native_reject_owned_laundering(
        &serde_json::from_value(encoded).unwrap(),
    );
}

#[test]
fn overwriting_tuple_sibling_preserves_borrowed_closure_provenance() {
    let mut encoded =
        serde_json::to_value(forged_repeatable_borrowed_closure_escape(false, true)).unwrap();
    let function = encoded["functions"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|function| function["name"] == "inspect")
        .unwrap();
    let closure_type = function["return_type"].clone();
    function["return_type"] = json!({ "Named": ["unit", []] });
    function["local_types"].as_array_mut().unwrap().extend([
        json!({ "name": "%t998", "ty": { "Tuple": [closure_type.clone(),
            { "Named": ["int64", []] }] }}),
        json!({ "name": "%t997", "ty": closure_type.clone() }),
    ]);
    let instructions = function["blocks"][0]["instructions"]
        .as_array_mut()
        .unwrap();
    instructions.extend([
        json!({ "Assign": { "target": "%t998", "value": { "TupleLiteral": {
            "elements": [{ "MovePlace": "take" }, { "Int": 0 }],
            "element_types": [closure_type.clone(), { "Named": ["int64", []] }]
        }}}}),
        json!({ "Assign": { "target": "%t998.1", "value": { "Use": { "Int": 1 } }}}),
        json!({ "Assign": { "target": "%t997", "value": { "TupleTakeElement": {
            "place": "%t998", "index": 0, "element_type": closure_type
        }}}}),
        json!({ "Assign": { "target": "%t999", "value": { "StartTask": {
            "returns_handle": false, "result_is_copy": true, "stack_size": null,
            "task_group": "Unit", "function": { "Place": "%t997" }, "args": [],
            "span": { "line": 1, "column": 1 }
        }}}}),
    ]);
    function["blocks"][0]["terminator"] = json!({ "Return": "Unit" });
    assert_interpreter_and_native_reject_owned_laundering(
        &serde_json::from_value(encoded).unwrap(),
    );
}

#[test]
fn moved_alias_cannot_hide_borrowed_closure_from_start_task() {
    let mir = forged_repeatable_borrowed_closure_escape(true, false);
    let mut encoded = serde_json::to_value(mir).unwrap();
    let function = encoded["functions"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|function| function["name"] == "inspect")
        .unwrap();
    function["blocks"][0]["instructions"][1]["Assign"]["value"]["StartTask"]["function"] =
        json!({ "MovePlace": "take" });
    assert_interpreter_and_native_reject_owned_laundering(
        &serde_json::from_value(encoded).unwrap(),
    );
}

#[test]
fn borrowed_closure_cannot_cross_a_by_value_call_boundary() {
    let mir = forged_repeatable_borrowed_closure_escape(false, true);
    let mut encoded = serde_json::to_value(mir).unwrap();
    let mut consume = encoded["functions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|function| function["name"] == "inspect")
        .unwrap()
        .clone();
    consume["name"] = json!("consume");
    consume["params"] = json!([{ "name": "job", "ty": { "Function": {
        "params": [], "return_type": { "Named": ["int64", []] }
    }}, "passing": "Value" }]);
    consume["return_type"] = json!({ "Named": ["unit", []] });
    consume["local_types"] = json!([]);
    consume["blocks"] = json!([{ "label": "entry", "instructions": [],
        "terminator": { "Return": "Unit" }}]);
    encoded["functions"].as_array_mut().unwrap().push(consume);
    let function = encoded["functions"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|function| function["name"] == "inspect")
        .unwrap();
    function["return_type"] = json!({ "Named": ["unit", []] });
    function["blocks"][0]["instructions"]
        .as_array_mut()
        .unwrap()
        .push(json!({
            "Assign": { "target": "%t999", "value": { "Call": {
                "callee": { "Name": "consume" }, "args": [{ "name": null,
                    "value": { "Place": "take" }, "writeback_place": null }]
            }}}
        }));
    function["blocks"][0]["terminator"] = json!({ "Return": "Unit" });
    assert_interpreter_and_native_reject_owned_laundering(
        &serde_json::from_value(encoded).unwrap(),
    );
}

fn forged_untyped_indirect_borrowed_closure_call(with_unknown_type: bool) -> MirModule {
    let mut encoded =
        serde_json::to_value(forged_repeatable_borrowed_closure_escape(false, true)).unwrap();
    let mut consume = encoded["functions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|function| function["name"] == "inspect")
        .unwrap()
        .clone();
    let callable = json!({ "Function": { "params": [{
        "name": "job", "ty": { "Function": { "params": [],
            "return_type": { "Named": ["int64", []] }}},
        "passing": "Value", "has_default": false, "default_erased": false,
        "keyword_only": false
    }], "return_type": { "Named": ["unit", []] }}});
    consume["name"] = json!("consume");
    consume["params"] = json!([{ "name": "job", "ty": { "Function": {
        "params": [], "return_type": { "Named": ["int64", []] }
    }}, "passing": "Value" }]);
    consume["return_type"] = json!({ "Named": ["unit", []] });
    consume["local_types"] = json!([]);
    consume["blocks"] = json!([{ "label": "entry", "instructions": [],
        "terminator": { "Return": "Unit" }}]);
    encoded["functions"].as_array_mut().unwrap().push(consume);
    let inspect = encoded["functions"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|function| function["name"] == "inspect")
        .unwrap();
    inspect["return_type"] = json!({ "Named": ["unit", []] });
    if with_unknown_type {
        inspect["local_types"].as_array_mut().unwrap().push(json!({
            "name": "%t995", "ty": { "Named": ["Unknown", []] }
        }));
    }
    let instructions = inspect["blocks"][0]["instructions"].as_array_mut().unwrap();
    instructions.push(json!({ "Assign": { "target": "%t995", "value": { "Use": {
        "Function": { "name": "consume", "signature": callable }
    }}}}));
    instructions.push(json!({ "Assign": { "target": "%t999", "value": { "Call": {
        "callee": { "Value": { "Place": "%t995" }}, "args": [{ "name": null,
            "value": { "Place": "take" }, "writeback_place": null }]
    }}}}));
    inspect["blocks"][0]["terminator"] = json!({ "Return": "Unit" });
    serde_json::from_value(encoded).unwrap()
}

#[test]
fn unknown_temporary_cannot_erase_an_indirect_function_contract() {
    assert_interpreter_and_native_reject_owned_laundering(
        &forged_untyped_indirect_borrowed_closure_call(true),
    );
}

#[test]
fn undeclared_temporary_cannot_erase_an_indirect_function_contract() {
    assert_interpreter_and_native_reject_owned_laundering(
        &forged_untyped_indirect_borrowed_closure_call(false),
    );
}

fn forged_indirect_argument_binding(args: Value, start_task: bool) -> MirModule {
    let mut encoded =
        serde_json::to_value(forged_untyped_indirect_borrowed_closure_call(false)).unwrap();
    let inspect = encoded["functions"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|function| function["name"] == "inspect")
        .unwrap();
    let last = inspect["blocks"][0]["instructions"]
        .as_array_mut()
        .unwrap()
        .last_mut()
        .unwrap();
    if start_task {
        last["Assign"]["value"] = json!({ "StartTask": {
            "returns_handle": false, "result_is_copy": true, "stack_size": null,
            "task_group": "Unit", "function": { "Place": "%t995" }, "args": args,
            "span": { "line": 1, "column": 1 }
        }});
    } else {
        last["Assign"]["value"]["Call"]["args"] = args;
    }
    serde_json::from_value(encoded).unwrap()
}

fn assert_common_argument_binding_rejected(mir: &MirModule, expected: &str) {
    let interpreted = run_mir(mir).expect_err("interpreter common validation must reject binding");
    assert!(interpreted.message.contains(expected), "{interpreted}");
    let native = emit_host_native_object(mir)
        .expect_err("native common validation must reject binding before code generation");
    assert!(native.contains(expected), "{native}");
}

#[test]
fn indirect_call_rejects_a_missing_required_argument() {
    assert_common_argument_binding_rejected(
        &forged_indirect_argument_binding(json!([]), false),
        "omits required parameter",
    );
}

#[test]
fn indirect_start_task_rejects_a_missing_required_argument() {
    assert_common_argument_binding_rejected(
        &forged_indirect_argument_binding(json!([]), true),
        "omits required parameter",
    );
}

#[test]
fn indirect_call_rejects_unknown_and_duplicate_named_arguments() {
    let value = json!({ "Place": "take" });
    assert_common_argument_binding_rejected(
        &forged_indirect_argument_binding(
            json!([{ "name": "unknown", "value": value,
            "writeback_place": null }]),
            false,
        ),
        "unknown argument",
    );
    let value = json!({ "Place": "take" });
    assert_common_argument_binding_rejected(
        &forged_indirect_argument_binding(
            json!([
                { "name": "job", "value": value.clone(), "writeback_place": null },
                { "name": "job", "value": value, "writeback_place": null }
            ]),
            false,
        ),
        "more than once",
    );
}

#[test]
fn indirect_start_task_rejects_too_many_positional_arguments() {
    let arg = json!({ "name": null, "value": { "Place": "take" },
        "writeback_place": null });
    assert_common_argument_binding_rejected(
        &forged_indirect_argument_binding(json!([arg.clone(), arg]), true),
        "too many arguments",
    );
}

fn forged_untyped_authoritative_start_task(with_unknown_type: bool) -> MirModule {
    let mut encoded = serde_json::to_value(forged_untyped_indirect_borrowed_closure_call(
        with_unknown_type,
    ))
    .unwrap();
    let inspect = encoded["functions"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|function| function["name"] == "inspect")
        .unwrap();
    let last = inspect["blocks"][0]["instructions"]
        .as_array_mut()
        .unwrap()
        .last_mut()
        .unwrap();
    last["Assign"]["value"] = json!({ "StartTask": {
        "returns_handle": false, "result_is_copy": true, "stack_size": null,
        "task_group": "Unit", "function": { "Place": "%t995" },
        "args": [{ "name": null, "value": { "Place": "take" },
            "writeback_place": null }], "span": { "line": 1, "column": 1 }
    }});
    serde_json::from_value(encoded).unwrap()
}

#[test]
fn unknown_temporary_cannot_erase_a_start_task_function_contract() {
    assert_interpreter_and_native_reject_owned_laundering(
        &forged_untyped_authoritative_start_task(true),
    );
}

#[test]
fn undeclared_temporary_cannot_erase_a_start_task_function_contract() {
    assert_interpreter_and_native_reject_owned_laundering(
        &forged_untyped_authoritative_start_task(false),
    );
}

#[test]
fn tuple_wrapper_cannot_erase_an_authoritative_indirect_function_contract() {
    let mut encoded =
        serde_json::to_value(forged_untyped_indirect_borrowed_closure_call(false)).unwrap();
    let inspect = encoded["functions"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|function| function["name"] == "inspect")
        .unwrap();
    let forged = json!({ "Function": { "params": [{
        "name": "item", "ty": { "Named": ["int64", []] }, "passing": "Value",
        "has_default": false, "default_erased": false, "keyword_only": false
    }], "return_type": { "Named": ["unit", []] }}});
    inspect["local_types"].as_array_mut().unwrap().push(json!({
        "name": "%t996", "ty": { "Tuple": [forged.clone()] }
    }));
    let instructions = inspect["blocks"][0]["instructions"].as_array_mut().unwrap();
    instructions.insert(
        instructions.len() - 1,
        json!({ "Assign": { "target": "%t996", "value": { "TupleLiteral": {
            "elements": [{ "Place": "%t995" }], "element_types": [forged]
        }}}}),
    );
    let last = instructions.last_mut().unwrap();
    last["Assign"]["value"]["Call"]["callee"] = json!({ "Value": { "Place": "%t996.0" }});
    assert_interpreter_and_native_reject_owned_laundering(
        &serde_json::from_value(encoded).unwrap(),
    );
}

#[test]
fn construct_member_cannot_erase_an_authoritative_indirect_function_contract() {
    let mut encoded =
        serde_json::to_value(forged_untyped_indirect_borrowed_closure_call(false)).unwrap();
    let inspect = encoded["functions"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|function| function["name"] == "inspect")
        .unwrap();
    let forged = json!({ "Function": { "params": [{
        "name": "item", "ty": { "Named": ["int64", []] }, "passing": "Value",
        "has_default": false, "default_erased": false, "keyword_only": false
    }], "return_type": { "Named": ["unit", []] }}});
    inspect["local_types"].as_array_mut().unwrap().extend([
        json!({ "name": "%t996", "ty": { "Named": ["Runner", []] }}),
        json!({ "name": "%t997", "ty": forged }),
    ]);
    let instructions = inspect["blocks"][0]["instructions"].as_array_mut().unwrap();
    let call = instructions.pop().unwrap();
    instructions.extend([
        json!({ "Assign": { "target": "%t996", "value": { "Construct": {
            "class_name": "Runner", "fields": [{ "name": "job",
                "value": { "Place": "%t995" }}]
        }}}}),
        json!({ "Assign": { "target": "%t997", "value": { "Member": {
            "object": { "Place": "%t996" }, "field": "job"
        }}}}),
        call,
    ]);
    instructions.last_mut().unwrap()["Assign"]["value"]["Call"]["callee"] =
        json!({ "Value": { "Place": "%t997" }});
    assert_interpreter_and_native_reject_owned_laundering(
        &serde_json::from_value(encoded).unwrap(),
    );
}

#[test]
fn enum_payload_wrapper_cannot_erase_an_authoritative_callable_subtree() {
    let mut encoded =
        serde_json::to_value(forged_untyped_indirect_borrowed_closure_call(false)).unwrap();
    let inspect = encoded["functions"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|function| function["name"] == "inspect")
        .unwrap();
    let instructions = inspect["blocks"][0]["instructions"].as_array_mut().unwrap();
    let call = instructions.pop().unwrap();
    let actual =
        instructions.last().unwrap()["Assign"]["value"]["Use"]["Function"]["signature"].clone();
    let _ = instructions;
    let _ = inspect;
    let weak = json!({ "Function": { "params": [{
        "name": "job", "ty": { "Function": { "params": [],
            "return_type": { "Named": ["int64", []] }}},
        "passing": "Borrow", "has_default": false, "default_erased": false,
        "keyword_only": false
    }], "return_type": { "Named": ["unit", []] }}});
    let tuple = json!({ "Tuple": [actual.clone()] });
    encoded["enums"].as_array_mut().unwrap().push(json!({
        "name": "Carrier", "type_params": [], "variants": [{
            "name": "Item", "payloads": [tuple.clone()]
        }]
    }));
    let inspect = encoded["functions"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|function| function["name"] == "inspect")
        .unwrap();
    inspect["local_types"].as_array_mut().unwrap().extend([
        json!({ "name": "%t990", "ty": tuple.clone() }),
        json!({ "name": "%t991", "ty": { "Named": ["Carrier", []] }}),
        json!({ "name": "%t992", "ty": tuple.clone() }),
        json!({ "name": "%t993", "ty": weak }),
    ]);
    let instructions = inspect["blocks"][0]["instructions"].as_array_mut().unwrap();
    instructions.extend([
        json!({ "Assign": { "target": "%t990", "value": { "TupleLiteral": {
            "elements": [{ "MovePlace": "%t995" }], "element_types": [actual]
        }}}}),
        json!({ "Assign": { "target": "%t991", "value": { "EnumVariant": {
            "enum_name": "Carrier", "variant_name": "Item",
            "payloads": [{ "MovePlace": "%t990" }]
        }}}}),
    ]);
    let _ = instructions;
    inspect["blocks"][0]["terminator"] = json!({ "Match": {
        "scrutinee": { "Place": "%t991" }, "arms": [{ "enum_name": "Carrier",
            "variant_name": "Item", "wildcard": false, "label": "matched" }],
        "otherwise": "done"
    }});
    let mut call = call;
    call["Assign"]["value"]["Call"]["callee"] = json!({ "Value": { "Place": "%t993" }});
    inspect["blocks"].as_array_mut().unwrap().extend([
        json!({ "label": "matched", "instructions": [
            { "Assign": { "target": "%t992", "value": { "VariantPayload": {
                "scrutinee": { "Place": "%t991" }, "variant_name": "Item", "index": 0
            }}}},
            { "Assign": { "target": "%t993", "value": { "TupleElement": {
                "tuple": { "Place": "%t992" }, "index": 0, "element_type": weak
            }}}},
            call
        ], "terminator": { "Return": "Unit" }}),
        json!({ "label": "done", "instructions": [], "terminator": { "Return": "Unit" }}),
    ]);
    assert_interpreter_and_native_reject_owned_laundering(
        &serde_json::from_value(encoded).unwrap(),
    );
}

#[test]
fn union_payload_wrapper_cannot_erase_an_authoritative_callable_subtree() {
    let mut encoded =
        serde_json::to_value(forged_untyped_indirect_borrowed_closure_call(false)).unwrap();
    let inspect_index = encoded["functions"]
        .as_array()
        .unwrap()
        .iter()
        .position(|function| function["name"] == "inspect")
        .unwrap();
    let (call, actual) = {
        let instructions = encoded["functions"][inspect_index]["blocks"][0]["instructions"]
            .as_array_mut()
            .unwrap();
        let call = instructions.pop().unwrap();
        let actual =
            instructions.last().unwrap()["Assign"]["value"]["Use"]["Function"]["signature"].clone();
        (call, actual)
    };
    let union_source = "def main():\n    value: (def(own def() -> int64) -> None,) | None = None\n";
    let union_mir = serde_json::to_value(lower_source_to_mir(union_source).unwrap()).unwrap();
    let union_type = union_mir["functions"][0]["local_types"]
        .as_array()
        .unwrap()
        .iter()
        .find(|local| local["name"] == "value")
        .unwrap()["ty"]
        .clone();
    let (member_index, tuple) = union_type["Union"]["members"]
        .as_array()
        .unwrap()
        .iter()
        .enumerate()
        .find(|(_, member)| member.get("Tuple").is_some())
        .map(|(index, member)| (index, member.clone()))
        .unwrap();
    let weak = json!({ "Function": { "params": [{
        "name": "job", "ty": { "Function": { "params": [],
            "return_type": { "Named": ["int64", []] }}},
        "passing": "Borrow", "has_default": false, "default_erased": false,
        "keyword_only": false
    }], "return_type": { "Named": ["unit", []] }}});
    let inspect = &mut encoded["functions"][inspect_index];
    inspect["local_types"].as_array_mut().unwrap().extend([
        json!({ "name": "%t990", "ty": tuple.clone() }),
        json!({ "name": "%t991", "ty": union_type.clone() }),
        json!({ "name": "%t992", "ty": { "Named": ["bool", []] }}),
        json!({ "name": "%t993", "ty": tuple.clone() }),
        json!({ "name": "%t994", "ty": weak.clone() }),
    ]);
    inspect["blocks"][0]["instructions"]
        .as_array_mut()
        .unwrap()
        .extend([
            json!({ "Assign": { "target": "%t990", "value": { "TupleLiteral": {
                "elements": [{ "MovePlace": "%t995" }], "element_types": [actual]
            }}}}),
            json!({ "Assign": { "target": "%t991", "value": { "UnionInject": {
                "value": { "MovePlace": "%t990" }, "union_type": union_type.clone(),
                "member_type": tuple.clone(), "member_index": member_index
            }}}}),
            json!({ "Assign": { "target": "%t992", "value": { "UnionTagTest": {
                "place": "%t991", "union_type": union_type.clone(),
                "member_index": member_index
            }}}}),
        ]);
    inspect["blocks"][0]["terminator"] = json!({ "Branch": {
        "condition": { "Place": "%t992" }, "then_label": "matched", "else_label": "done"
    }});
    let mut call = call;
    call["Assign"]["value"]["Call"]["callee"] = json!({ "Value": { "Place": "%t994" }});
    inspect["blocks"].as_array_mut().unwrap().extend([
        json!({ "label": "matched", "instructions": [
            { "Assign": { "target": "%t993", "value": { "UnionTakePayload": {
                "place": "%t991", "union_type": union_type,
                "member_type": tuple.clone(), "member_index": member_index
            }}}},
            { "Assign": { "target": "%t994", "value": { "TupleElement": {
                "tuple": { "Place": "%t993" }, "index": 0, "element_type": weak
            }}}},
            call
        ], "terminator": { "Return": "Unit" }}),
        json!({ "label": "done", "instructions": [], "terminator": { "Return": "Unit" }}),
    ]);
    assert_interpreter_and_native_reject_owned_laundering(
        &serde_json::from_value(encoded).unwrap(),
    );
}

#[test]
fn list_get_option_payload_preserves_authoritative_callable_identity() {
    let source = "class Holder:\n    values: list[int64]\ndef consume(item: own Holder):\n    pass\ndef inspect(value: own Holder):\n    callbacks: list[def(own Holder) -> None] = [consume]\n    match callbacks.get(0):\n        case Some(callback):\n            callback(value)\n        case None:\n            pass\ndef main():\n    pass\n";
    let mut encoded = serde_json::to_value(lower_source_to_mir(source).unwrap()).unwrap();
    let inspect = encoded["functions"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|function| function["name"] == "inspect")
        .unwrap();
    inspect["params"][0]["passing"] = json!("Borrow");
    fn weaken_callable_passings(value: &mut Value) {
        match value {
            Value::Array(values) => values.iter_mut().for_each(weaken_callable_passings),
            Value::Object(fields) => {
                if fields.get("passing") == Some(&json!("Value")) && fields.contains_key("ty") {
                    fields.insert("passing".to_owned(), json!("Borrow"));
                }
                fields.values_mut().for_each(weaken_callable_passings);
            }
            _ => {}
        }
    }
    weaken_callable_passings(&mut inspect["local_types"]);
    assert_interpreter_and_native_reject_owned_laundering(
        &serde_json::from_value(encoded).unwrap(),
    );
}

#[test]
fn nested_list_get_tuple_payload_preserves_authoritative_callable_identity() {
    let source = "class Holder:\n    values: list[int64]\ndef consume(item: own Holder):\n    pass\ndef inspect(value: own Holder):\n    callbacks: list[(def(own Holder) -> None, int64)] = [(consume, 0)]\n    match callbacks.get(0):\n        case Some(entry):\n            callback: def(own Holder) -> None = entry[0]\n            callback(value)\n        case None:\n            pass\ndef main():\n    pass\n";
    let mut encoded = serde_json::to_value(lower_source_to_mir(source).unwrap()).unwrap();
    let inspect = encoded["functions"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|function| function["name"] == "inspect")
        .unwrap();
    inspect["params"][0]["passing"] = json!("Borrow");
    fn weaken_callable_passings(value: &mut Value) {
        match value {
            Value::Array(values) => values.iter_mut().for_each(weaken_callable_passings),
            Value::Object(fields) => {
                if fields.get("passing") == Some(&json!("Value")) && fields.contains_key("ty") {
                    fields.insert("passing".to_owned(), json!("Borrow"));
                }
                fields.values_mut().for_each(weaken_callable_passings);
            }
            _ => {}
        }
    }
    weaken_callable_passings(&mut inspect["local_types"]);
    assert_interpreter_and_native_reject_owned_laundering(
        &serde_json::from_value(encoded).unwrap(),
    );
}

fn dict_get_callable_source() -> &'static str {
    "class Holder:\n    values: list[int64]\ndef consume(item: own Holder):\n    pass\ndef inspect(value: own Holder):\n    callbacks: dict[str, def(own Holder) -> None] = {\"consume\": consume}\n    match callbacks.get(\"consume\"):\n        case Some(callback):\n            callback(value)\n        case None:\n            pass\ndef main():\n    pass\n"
}

#[test]
fn dict_get_option_payload_preserves_authoritative_value_callable_identity() {
    let mut encoded =
        serde_json::to_value(lower_source_to_mir(dict_get_callable_source()).unwrap()).unwrap();
    let inspect = encoded["functions"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|function| function["name"] == "inspect")
        .unwrap();
    inspect["params"][0]["passing"] = json!("Borrow");
    fn weaken_callable_passings(value: &mut Value) {
        match value {
            Value::Array(values) => values.iter_mut().for_each(weaken_callable_passings),
            Value::Object(fields) => {
                if fields.get("passing") == Some(&json!("Value")) && fields.contains_key("ty") {
                    fields.insert("passing".to_owned(), json!("Borrow"));
                }
                fields.values_mut().for_each(weaken_callable_passings);
            }
            _ => {}
        }
    }
    weaken_callable_passings(&mut inspect["local_types"]);
    assert_interpreter_and_native_reject_owned_laundering(
        &serde_json::from_value(encoded).unwrap(),
    );
}

#[test]
fn dict_get_callable_with_owned_argument_remains_valid() {
    let mir = lower_source_to_mir(dict_get_callable_source()).unwrap();
    run_mir(&mir).expect("owned callable dictionary extraction remains valid");
    emit_host_native_object(&mir).expect("native validation accepts owned dictionary extraction");
}

fn forged_higher_order_callable_downgrade(start_task: bool) -> MirModule {
    let source = "class Holder:\n    values: list[int64]\ndef inspect(value: Holder):\n    pass\ndef main():\n    pass\n";
    let mut encoded = serde_json::to_value(lower_source_to_mir(source).unwrap()).unwrap();
    let template = encoded["functions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|function| function["name"] == "inspect")
        .unwrap()
        .clone();
    let holder = json!({ "Named": ["Holder", []] });
    let actual_signature = json!({ "Function": { "params": [{
        "name": "item", "ty": holder.clone(), "passing": "Value",
        "has_default": false, "default_erased": false, "keyword_only": false
    }], "return_type": { "Named": ["unit", []] }}});
    let weakened_signature = json!({ "Function": { "params": [{
        "name": "item", "ty": holder.clone(), "passing": "Borrow",
        "has_default": false, "default_erased": false, "keyword_only": false
    }], "return_type": { "Named": ["unit", []] }}});
    let invoke_signature = json!({ "Function": { "params": [
        { "name": "callback", "ty": weakened_signature.clone(), "passing": "Borrow",
            "has_default": false, "default_erased": false, "keyword_only": false },
        { "name": "value", "ty": holder.clone(), "passing": "Borrow",
            "has_default": false, "default_erased": false, "keyword_only": false }
    ], "return_type": { "Named": ["unit", []] }}});
    let mut consume = template.clone();
    consume["name"] = json!("consume");
    consume["params"] = json!([{ "name": "item", "ty": holder.clone(),
        "passing": "Value" }]);
    consume["return_type"] = json!({ "Named": ["unit", []] });
    consume["local_types"] = json!([]);
    consume["blocks"] = json!([{ "label": "entry", "instructions": [],
        "terminator": { "Return": "Unit" }}]);
    let mut invoke = template.clone();
    invoke["name"] = json!("invoke");
    invoke["params"] = json!([
        { "name": "callback", "ty": weakened_signature.clone(), "passing": "Borrow" },
        { "name": "value", "ty": holder.clone(), "passing": "Borrow" }
    ]);
    invoke["return_type"] = json!({ "Named": ["unit", []] });
    invoke["local_types"] = json!([]);
    invoke["blocks"] = json!([{ "label": "entry", "instructions": [{ "Assign": {
        "target": "%t0", "value": { "Call": {
            "callee": { "Value": { "Place": "callback" }}, "args": [{ "name": null,
                "value": { "Place": "value" }, "writeback_place": null }]
        }}
    }}], "terminator": { "Return": "Unit" }}]);
    encoded["functions"]
        .as_array_mut()
        .unwrap()
        .extend([consume, invoke]);
    let inspect = encoded["functions"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|function| function["name"] == "inspect")
        .unwrap();
    inspect["params"][0]["passing"] = json!("Borrow");
    inspect["local_types"] = json!([{ "name": "callback", "ty": weakened_signature }]);
    let invoke_value = if start_task {
        json!({ "StartTask": { "returns_handle": false, "result_is_copy": true,
            "stack_size": null, "task_group": "Unit",
            "function": { "Function": { "name": "invoke", "signature": invoke_signature }}, "args": [
                { "name": null, "value": { "Place": "callback" }, "writeback_place": null },
                { "name": null, "value": { "Place": "value" }, "writeback_place": null }
            ], "span": { "line": 1, "column": 1 }} })
    } else {
        json!({ "Call": { "callee": { "Name": "invoke" }, "args": [
            { "name": null, "value": { "Place": "callback" }, "writeback_place": null },
            { "name": null, "value": { "Place": "value" }, "writeback_place": null }
        ]}})
    };
    inspect["blocks"] = json!([{ "label": "entry", "instructions": [
        { "Assign": { "target": "callback", "value": { "Use": { "Function": {
            "name": "consume", "signature": actual_signature
        }}}}},
        { "Assign": { "target": "%t0", "value": invoke_value }}
    ], "terminator": { "Return": "Unit" }}]);
    serde_json::from_value(encoded).unwrap()
}

fn assert_callable_downgrade_rejected(mir: &MirModule) {
    let interpreted = run_mir(mir).expect_err("interpreter must reject callable downgrade");
    assert!(interpreted.message.contains("contract"), "{interpreted}");
    let native = emit_host_native_object(mir).expect_err("native must reject callable downgrade");
    assert!(native.contains("contract"), "{native}");
}

#[test]
fn higher_order_call_cannot_downgrade_an_owned_callable_parameter() {
    assert_callable_downgrade_rejected(&forged_higher_order_callable_downgrade(false));
}

#[test]
fn higher_order_start_task_cannot_downgrade_an_owned_callable_parameter() {
    assert_callable_downgrade_rejected(&forged_higher_order_callable_downgrade(true));
}

fn forged_higher_order_parameter_metadata(start_task: bool, forged_default: bool) -> MirModule {
    let mut encoded =
        serde_json::to_value(forged_higher_order_callable_downgrade(start_task)).unwrap();
    let invoke = encoded["functions"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|function| function["name"] == "invoke")
        .unwrap();
    let expected = &mut invoke["params"][0]["ty"]["Function"]["params"][0];
    expected["passing"] = json!("Value");
    if forged_default {
        expected["has_default"] = json!(true);
    } else {
        expected["name"] = json!("renamed");
    }
    if start_task {
        let inspect = encoded["functions"]
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .find(|function| function["name"] == "inspect")
            .unwrap();
        let embedded = &mut inspect["blocks"][0]["instructions"][1]["Assign"]["value"]["StartTask"]
            ["function"]["Function"]["signature"]["Function"]["params"][0]["ty"]["Function"]
            ["params"][0];
        embedded["passing"] = json!("Value");
        if forged_default {
            embedded["has_default"] = json!(true);
        } else {
            embedded["name"] = json!("renamed");
        }
    }
    serde_json::from_value(encoded).unwrap()
}

#[test]
fn higher_order_call_rejects_a_renamed_authoritative_parameter() {
    assert_callable_downgrade_rejected(&forged_higher_order_parameter_metadata(false, false));
}

#[test]
fn higher_order_start_task_rejects_a_forged_default_contract() {
    assert_callable_downgrade_rejected(&forged_higher_order_parameter_metadata(true, true));
}

#[test]
fn start_task_authenticates_a_direct_function_operand_signature() {
    let mut encoded = serde_json::to_value(forged_higher_order_callable_downgrade(true)).unwrap();
    let inspect = encoded["functions"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|function| function["name"] == "inspect")
        .unwrap();
    inspect["blocks"][0]["instructions"][1]["Assign"]["value"]["StartTask"]["function"]
        ["Function"]["signature"]["Function"]["return_type"] = json!({ "Named": ["int64", []] });
    let mir: MirModule = serde_json::from_value(encoded).unwrap();
    let interpreted = run_mir(&mir).expect_err("interpreter must authenticate task function");
    assert!(interpreted.message.contains("return type"), "{interpreted}");
    let native = emit_host_native_object(&mir).expect_err("native must authenticate task function");
    assert!(native.contains("return type"), "{native}");
}

fn forge_higher_order_function_argument_signature(start_task: bool) -> MirModule {
    let mut encoded =
        serde_json::to_value(forged_higher_order_callable_downgrade(start_task)).unwrap();
    let inspect = encoded["functions"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|function| function["name"] == "inspect")
        .unwrap();
    let weakened = inspect["local_types"][0]["ty"].clone();
    inspect["blocks"][0]["instructions"][0]["Assign"]["value"]["Use"]["Function"]["signature"] =
        weakened;
    serde_json::from_value(encoded).unwrap()
}

#[test]
fn higher_order_call_authenticates_a_direct_function_argument() {
    let mir = forge_higher_order_function_argument_signature(false);
    let interpreted = run_mir(&mir).expect_err("interpreter must authenticate function argument");
    assert!(
        interpreted.message.contains("does not match declaration"),
        "{interpreted}"
    );
    let native =
        emit_host_native_object(&mir).expect_err("native must authenticate function argument");
    assert!(native.contains("does not match declaration"), "{native}");
}

#[test]
fn higher_order_start_task_authenticates_a_direct_function_argument() {
    let mir = forge_higher_order_function_argument_signature(true);
    let interpreted = run_mir(&mir).expect_err("interpreter must authenticate task argument");
    assert!(
        interpreted.message.contains("does not match declaration"),
        "{interpreted}"
    );
    let native = emit_host_native_object(&mir).expect_err("native must authenticate task argument");
    assert!(native.contains("does not match declaration"), "{native}");
}

fn forged_callable_loan_transfer(write: bool) -> MirModule {
    let mut encoded =
        serde_json::to_value(forged_untyped_indirect_borrowed_closure_call(false)).unwrap();
    let inspect = encoded["functions"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|function| function["name"] == "inspect")
        .unwrap();
    inspect["local_types"].as_array_mut().unwrap().push(json!({
        "name": "%t994", "ty": { "Function": { "params": [],
            "return_type": { "Named": ["unit", []] }}}
    }));
    inspect["local_types"].as_array_mut().unwrap().push(json!({
        "name": "%t995", "ty": { "Function": { "params": [{
            "name": "job", "ty": { "Function": { "params": [],
                "return_type": { "Named": ["int64", []] }}},
            "passing": "Value", "has_default": false,
            "default_erased": false, "keyword_only": false
        }], "return_type": { "Named": ["unit", []] }}}
    }));
    let instructions = inspect["blocks"][0]["instructions"].as_array_mut().unwrap();
    let call = instructions.pop().unwrap();
    if write {
        instructions.extend([
            json!({ "BeginLoan": { "loan": "slot_loan", "source": "%t994",
                "mutable": true }}),
            json!({ "WriteLoan": { "loan": "slot_loan", "value": { "Use": {
                "Place": "%t995" }} }}),
            json!({ "EndLoan": { "loan": "slot_loan" }}),
        ]);
    } else {
        instructions.extend([
            json!({ "BeginLoan": { "loan": "slot_loan", "source": "%t995",
                "mutable": false }}),
            json!({ "ReadLoan": { "target": "%t994", "loan": "slot_loan" }}),
            json!({ "EndLoan": { "loan": "slot_loan" }}),
        ]);
    }
    instructions.push(call);
    instructions.last_mut().unwrap()["Assign"]["value"]["Call"]["callee"] =
        json!({ "Value": { "Place": "%t994" }});
    serde_json::from_value(encoded).unwrap()
}

#[test]
fn read_loan_preserves_an_authoritative_callable_contract() {
    assert_interpreter_and_native_reject_owned_laundering(&forged_callable_loan_transfer(false));
}

#[test]
fn write_loan_preserves_an_authoritative_callable_contract() {
    assert_interpreter_and_native_reject_owned_laundering(&forged_callable_loan_transfer(true));
}

#[test]
fn aggregate_write_loan_preserves_authoritative_callable_subtrees() {
    let mut encoded =
        serde_json::to_value(forged_untyped_indirect_borrowed_closure_call(false)).unwrap();
    let inspect = encoded["functions"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|function| function["name"] == "inspect")
        .unwrap();
    let instructions = inspect["blocks"][0]["instructions"].as_array_mut().unwrap();
    let mut call = instructions.pop().unwrap();
    let weak = json!({ "Function": { "params": [{
        "name": "job", "ty": { "Function": { "params": [],
            "return_type": { "Named": ["int64", []] }}},
        "passing": "Borrow", "has_default": false, "default_erased": false,
        "keyword_only": false
    }], "return_type": { "Named": ["unit", []] }}});
    let tuple = json!({ "Tuple": [weak.clone()] });
    let _ = instructions;
    inspect["local_types"].as_array_mut().unwrap().extend([
        json!({ "name": "%t990", "ty": tuple.clone() }),
        json!({ "name": "%t993", "ty": weak.clone() }),
    ]);
    let instructions = inspect["blocks"][0]["instructions"].as_array_mut().unwrap();
    instructions.extend([
        json!({ "Assign": { "target": "%t990", "value": { "TupleLiteral": {
            "elements": [{ "Place": "%t995" }], "element_types": [weak.clone()]
        }}}}),
        json!({ "BeginLoan": { "loan": "slot_loan", "source": "%t990", "mutable": true }}),
        json!({ "WriteLoan": { "loan": "slot_loan", "value": { "TupleLiteral": {
            "elements": [{ "Place": "%t995" }], "element_types": [weak.clone()]
        }}}}),
        json!({ "EndLoan": { "loan": "slot_loan" }}),
        json!({ "Assign": { "target": "%t993", "value": { "TupleElement": {
            "tuple": { "Place": "%t990" }, "index": 0, "element_type": weak
        }}}}),
    ]);
    call["Assign"]["value"]["Call"]["callee"] = json!({ "Value": { "Place": "%t993" }});
    instructions.push(call);
    assert_interpreter_and_native_reject_owned_laundering(
        &serde_json::from_value(encoded).unwrap(),
    );
}

#[test]
fn aggregate_write_loan_accepts_a_matching_callable_subtree() {
    let source = "def consume(item: int64):\n    pass\ndef main():\n    callback: def(int64) -> None = consume\n    mut slot: (def(int64) -> None,) = (callback,)\n";
    let mut encoded = serde_json::to_value(lower_source_to_mir(source).unwrap()).unwrap();
    let main = encoded["functions"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|function| function["name"] == "main")
        .unwrap();
    let callable = main["local_types"]
        .as_array()
        .unwrap()
        .iter()
        .find(|local| local["name"] == "callback")
        .unwrap()["ty"]
        .clone();
    main["blocks"][0]["instructions"]
        .as_array_mut()
        .unwrap()
        .extend([
            json!({ "BeginLoan": { "loan": "slot_loan", "source": "slot", "mutable": true }}),
            json!({ "WriteLoan": { "loan": "slot_loan", "value": { "TupleLiteral": {
                "elements": [{ "Place": "callback" }], "element_types": [callable]
            }}}}),
            json!({ "EndLoan": { "loan": "slot_loan" }}),
        ]);
    let mir: MirModule = serde_json::from_value(encoded).unwrap();
    run_mir(&mir).expect("matching aggregate callable writeback remains valid");
    emit_host_native_object(&mir).expect("native validation accepts matching aggregate writeback");
}

#[test]
fn aggregate_write_loan_preserves_borrowed_noncopy_element_origins() {
    let source = "def inspect(source: list[int64]):\n    pass\ndef main():\n    pass\n";
    let mut encoded = serde_json::to_value(lower_source_to_mir(source).unwrap()).unwrap();
    let inspect = encoded["functions"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|function| function["name"] == "inspect")
        .unwrap();
    inspect["params"][0]["passing"] = json!("Borrow");
    let list = json!({ "Named": ["list", [{ "Named": ["int64", []] }]] });
    inspect["local_types"] = json!([
        { "name": "%t990", "ty": list.clone() },
        { "name": "%t991", "ty": { "Tuple": [list.clone()] }},
        { "name": "%t992", "ty": list.clone() }
    ]);
    inspect["blocks"] = json!([{ "label": "entry", "instructions": [
        { "Assign": { "target": "%t990", "value": { "VecLiteral": {
            "elements": [], "element_type": { "Named": ["int64", []] }
        }}}},
        { "Assign": { "target": "%t991", "value": { "TupleLiteral": {
            "elements": [{ "MovePlace": "%t990" }], "element_types": [list.clone()]
        }}}},
        { "BeginLoan": { "loan": "slot_loan", "source": "%t991", "mutable": true }},
        { "WriteLoan": { "loan": "slot_loan", "value": { "TupleLiteral": {
            "elements": [{ "Place": "source" }], "element_types": [list.clone()]
        }}}},
        { "EndLoan": { "loan": "slot_loan" }},
        { "Assign": { "target": "%t992", "value": { "TupleElement": {
            "tuple": { "Place": "%t991" }, "index": 0, "element_type": list
        }}}},
        { "Eval": { "value": { "MovePlace": "%t992" }}}
    ], "terminator": { "Return": "Unit" }}]);
    assert_interpreter_and_native_reject_owned_laundering(
        &serde_json::from_value(encoded).unwrap(),
    );
}

#[test]
fn forged_closure_metadata_cannot_downgrade_its_function_declaration() {
    let mut encoded = serde_json::to_value(forged_higher_order_callable_downgrade(false)).unwrap();
    let inspect = encoded["functions"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|function| function["name"] == "inspect")
        .unwrap();
    let weakened = inspect["local_types"][0]["ty"].clone();
    inspect["blocks"][0]["instructions"][0]["Assign"]["value"] = json!({ "Closure": {
        "function": "consume", "signature": weakened, "captures": [], "consuming": false
    }});
    assert_callable_downgrade_rejected(&serde_json::from_value(encoded).unwrap());
}

#[test]
fn forged_closure_hidden_capture_cannot_launder_a_shared_noncopy_value() {
    let source = "class Holder:\n    values: list[int64]\ndef consume(item: own Holder):\n    pass\ndef inspect(value: Holder):\n    pass\ndef main():\n    pass\n";
    let mut encoded = serde_json::to_value(lower_source_to_mir(source).unwrap()).unwrap();
    let inspect = encoded["functions"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|function| function["name"] == "inspect")
        .unwrap();
    inspect["params"][0]["passing"] = json!("Borrow");
    inspect["local_types"] = json!([{ "name": "%t999", "ty": {
        "Function": { "params": [], "return_type": { "Named": ["unit", []] }}
    }}]);
    inspect["blocks"] = json!([{ "label": "entry", "instructions": [{ "Assign": {
        "target": "%t999", "value": { "Closure": {
            "function": "consume", "signature": { "Function": { "params": [],
                "return_type": { "Named": ["unit", []] }}},
            "captures": [{ "name": "item", "value": { "Place": "value" },
                "ty": { "Named": ["Holder", []] }, "passing": "Value",
                "source_place": null, "resolve_source_at_capture": false }],
            "consuming": false
        }}
    }}], "terminator": { "Return": "Unit" }}]);
    assert_interpreter_and_native_reject_owned_laundering(
        &serde_json::from_value(encoded).unwrap(),
    );
}

#[test]
fn forged_closure_named_parameter_metadata_is_rejected_by_common_validation() {
    let source = "def consume(item: int64):\n    pass\ndef main():\n    offset = 1\n    callback: def(int64) -> None = lambda [offset] item: consume(item + offset)\n    callback(1)\n";
    let mut encoded = serde_json::to_value(lower_source_to_mir(source).unwrap()).unwrap();
    let closure = encoded["functions"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .flat_map(|function| function["blocks"].as_array_mut().unwrap())
        .flat_map(|block| block["instructions"].as_array_mut().unwrap())
        .find_map(|instruction| {
            instruction
                .as_object_mut()?
                .get_mut("Assign")?
                .as_object_mut()?
                .get_mut("value")?
                .as_object_mut()?
                .get_mut("Closure")?
                .as_object_mut()
        })
        .unwrap();
    let signature = closure["signature"].as_object_mut().unwrap();
    let signature_kind = if signature.contains_key("Function") {
        "Function"
    } else {
        "Closure"
    };
    let contract = signature.get_mut(signature_kind).unwrap();
    contract["params"][0]["name"] = json!("forged");
    contract["params"][0]["default_erased"] = json!(false);
    let mir: MirModule = serde_json::from_value(encoded).unwrap();
    let interpreted =
        run_mir(&mir).expect_err("common validation must reject forged parameter names");
    assert!(
        interpreted.message.contains("callable contract"),
        "{interpreted}"
    );
    let native = emit_host_native_object(&mir)
        .expect_err("native validation must reject forged parameter names at the common boundary");
    assert!(native.contains("callable contract"), "{native}");
}

#[test]
fn forged_copy_temp_type_cannot_hide_borrowed_noncopy_call_argument() {
    let source = "class Holder:\n    values: list[int64]\ndef consume(item: own Holder):\n    pass\ndef inspect(value: Holder):\n    pass\ndef main():\n    pass\n";
    let mut encoded = serde_json::to_value(lower_source_to_mir(source).unwrap()).unwrap();
    let function = encoded["functions"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|function| function["name"] == "inspect")
        .unwrap();
    function["params"][0]["passing"] = json!("Borrow");
    function["local_types"] = json!([{
        "name": "%t998", "ty": { "Named": ["int64", []] }
    }]);
    function["blocks"] = json!([{ "label": "entry", "instructions": [
        { "Assign": { "target": "%t998", "value": { "Use": { "Place": "value" } }}},
        { "Assign": { "target": "%t999", "value": { "Call": {
            "callee": { "Name": "consume" }, "args": [{ "name": null,
                "value": { "Place": "%t998" }, "writeback_place": null }]
        }}}}
    ], "terminator": { "Return": "Unit" }}]);
    assert_interpreter_and_native_reject_owned_laundering(
        &serde_json::from_value(encoded).unwrap(),
    );
}

#[test]
fn projected_cleanup_requires_a_dominating_variant_proof() {
    let source = "class Handle:\n    value: int64\n    def close(mut self):\n        pass\nenum Boxed:\n    Item(Handle)\ndef main():\n    boxed = Boxed.Item(Handle(value=1))\n";
    let mut encoded = serde_json::to_value(lower_source_to_mir(source).unwrap()).unwrap();
    main_block_mut(&mut encoded, "entry")["instructions"].as_array_mut().unwrap().extend([
        json!({ "PushCleanup": { "place": "boxed.__variant_payload_Item_0" }}),
        json!({ "PopCleanup": { "place": "boxed.__variant_payload_Item_0", "cancel_before_cleanup": false }})
    ]);
    let error = run_mir(&serde_json::from_value(encoded).unwrap())
        .expect_err("cleanup cannot access an unproven payload");
    assert!(error.message.contains("matching variant proof"), "{error}");
}

#[test]
fn projected_cleanup_accepts_a_proven_owned_local_payload() {
    let source = "class Handle:\n    value: int64\n    def close(mut self):\n        pass\nenum Boxed:\n    Item(Handle)\ndef main():\n    boxed = Boxed.Item(Handle(value=1))\n";
    let mut encoded = serde_json::to_value(lower_source_to_mir(source).unwrap()).unwrap();
    let main = encoded["functions"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|function| function["name"] == "main")
        .unwrap();
    let initialization = main["blocks"]
        .as_array()
        .unwrap()
        .iter()
        .find(|block| block["label"] == main["entry"])
        .unwrap()["instructions"]
        .clone();
    main["blocks"] = json!([
        { "label": "entry", "instructions": initialization, "terminator": { "Match": {
            "scrutinee": { "Place": "boxed" }, "arms": [{ "enum_name": "Boxed",
            "variant_name": "Item", "wildcard": false, "label": "matched" }],
            "otherwise": "done" }}},
        { "label": "matched", "instructions": [
            { "PushCleanup": { "place": "boxed.__variant_payload_Item_0" }},
            { "PopCleanup": { "place": "boxed.__variant_payload_Item_0", "cancel_before_cleanup": false }}
        ], "terminator": { "Return": "Unit" }},
        { "label": "done", "instructions": [], "terminator": { "Return": "Unit" }}
    ]);
    run_mir(&serde_json::from_value(encoded).unwrap())
        .expect("a proven owned local payload can be closed");
}

#[test]
fn enum_payload_view_cannot_be_returned_through_its_argument_origin() {
    let source = "class Profile:\n    name: int64\nenum Boxed:\n    Item(int64)\n    Empty\ndef inspect(profile: Profile) -> view int64 from profile:\n    return view profile.name\ndef main():\n    pass\n";
    let mut encoded = serde_json::to_value(lower_source_to_mir(source).unwrap()).unwrap();
    let function = encoded["functions"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|function| function["name"] == "inspect")
        .unwrap();
    function["params"][0]["ty"] = json!({ "Named": ["Boxed", []] });
    for local in function["local_types"].as_array_mut().unwrap() {
        if local["name"] == "profile" {
            local["ty"] = json!({ "Named": ["Boxed", []] });
        }
    }
    function["local_types"].as_array_mut().unwrap().extend([
        json!({ "name": "payload_loan", "ty": { "Named": ["int64", []] }}),
        json!({ "name": "result", "ty": { "Named": ["int64", []] }}),
    ]);
    function["entry"] = json!("entry");
    function["blocks"] = json!([
        { "label": "entry", "instructions": [], "terminator": { "Match": {
            "scrutinee": { "Place": "profile" }, "arms": [{ "enum_name": "Boxed",
            "variant_name": "Item", "wildcard": false, "label": "matched" }],
            "otherwise": "dead" }}},
        { "label": "matched", "instructions": [
            { "BeginLoan": { "loan": "payload_loan", "source": "profile.__variant_payload_Item_0", "mutable": false }},
            { "ReadLoan": { "target": "result", "loan": "payload_loan" }},
            { "ReturnLoan": { "loan": "payload_loan", "origin": "profile" }}
        ], "terminator": { "Return": { "Place": "result" }}},
        { "label": "dead", "instructions": [], "terminator": "Unreachable" }
    ]);
    let error = run_mir(&serde_json::from_value(encoded).unwrap())
        .expect_err("match payload views stay arm-local even through an argument origin");
    assert!(
        error.message.contains("arm-local payload projection"),
        "{error}"
    );
}

#[test]
fn class_and_enum_metadata_names_cannot_collide() {
    let mut encoded =
        serde_json::to_value(lower_source_to_mir(ENUM_PATTERN_SOURCE).unwrap()).unwrap();
    encoded["classes"].as_array_mut().unwrap().push(json!({
        "name": "Boxed", "type_params": [], "fields": [], "methods": []
    }));
    let error = run_mir(&serde_json::from_value(encoded).unwrap())
        .expect_err("a nominal metadata name must identify one declaration kind");
    assert!(
        error.message.contains("class") && error.message.contains("enum"),
        "{error}"
    );
}

fn weaken_function_callable_metadata(encoded: &mut Value, function_name: &str) {
    weaken_function_callable_metadata_preserving(encoded, function_name, &[]);
}

fn weaken_function_callable_metadata_preserving(
    encoded: &mut Value,
    function_name: &str,
    preserved_locals: &[&str],
) {
    // Forge the caller so that a borrowed value reaches an `own` callee slot
    // through a callable whose declared metadata has been weakened. Named
    // function operands keep their authentic signatures because the validator
    // authenticates them against declarations; only the authoritative identity
    // recovered through the container can expose the laundering.
    let function = encoded["functions"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|function| function["name"] == function_name)
        .unwrap();
    function["params"][0]["passing"] = json!("Borrow");
    fn weaken(value: &mut Value) {
        match value {
            Value::Array(values) => values.iter_mut().for_each(weaken),
            Value::Object(fields) => {
                if fields.contains_key("name") && fields.contains_key("signature") {
                    return;
                }
                if fields.keys().any(|key| {
                    matches!(
                        key.as_str(),
                        "BeginLoan"
                            | "BeginReturnedLoan"
                            | "Reborrow"
                            | "ReadLoan"
                            | "EndLoan"
                            | "ReturnLoan"
                    )
                }) {
                    return;
                }
                if fields.get("passing") == Some(&json!("Value")) && fields.contains_key("ty") {
                    fields.insert("passing".to_owned(), json!("Borrow"));
                }
                if fields.get("MovePlace") == Some(&json!("value")) {
                    fields.remove("MovePlace");
                    fields.insert("Place".to_owned(), json!("value"));
                }
                fields.values_mut().for_each(weaken);
            }
            _ => {}
        }
    }
    for local in function["local_types"].as_array_mut().unwrap() {
        let name = local["name"].as_str().unwrap_or_default().to_owned();
        if !preserved_locals.contains(&name.as_str()) {
            weaken(local);
        }
    }
    weaken(&mut function["blocks"]);
}

fn runtime_index_nested_tuple_get_source() -> &'static str {
    "class Holder:\n    values: list[int64]\ndef consume(item: own Holder):\n    pass\ndef inspect(value: own Holder, index: int64):\n    callbacks: list[(def(own Holder) -> None, int64)] = [(consume, 0)]\n    match callbacks.get(index):\n        case Some(entry):\n            callback: def(own Holder) -> None = entry[0]\n            callback(value)\n        case None:\n            pass\ndef main():\n    pass\n"
}

#[test]
fn runtime_index_nested_tuple_get_preserves_authoritative_callable_identity() {
    let mut encoded =
        serde_json::to_value(lower_source_to_mir(runtime_index_nested_tuple_get_source()).unwrap())
            .unwrap();
    weaken_function_callable_metadata(&mut encoded, "inspect");
    assert_interpreter_and_native_reject_owned_laundering(
        &serde_json::from_value(encoded).unwrap(),
    );
}

#[test]
fn runtime_index_nested_tuple_get_with_owned_argument_remains_valid() {
    let mir = lower_source_to_mir(runtime_index_nested_tuple_get_source()).unwrap();
    run_mir(&mir).expect("owned callable nested tuple extraction remains valid");
    emit_host_native_object(&mir).expect("native validation accepts nested tuple extraction");
}

fn runtime_index_nested_tuple_subscript_source() -> &'static str {
    "class Holder:\n    values: list[int64]\ndef consume(item: own Holder):\n    pass\ndef inspect(value: own Holder, index: int64):\n    callbacks: list[(def(own Holder) -> None, int64)] = [(consume, 0)]\n    entry = callbacks[index]\n    callback: def(own Holder) -> None = entry[0]\n    callback(value)\ndef main():\n    pass\n"
}

#[test]
fn runtime_index_nested_tuple_subscript_preserves_authoritative_callable_identity() {
    let mut encoded = serde_json::to_value(
        lower_source_to_mir(runtime_index_nested_tuple_subscript_source()).unwrap(),
    )
    .unwrap();
    weaken_function_callable_metadata(&mut encoded, "inspect");
    assert_interpreter_and_native_reject_owned_laundering(
        &serde_json::from_value(encoded).unwrap(),
    );
}

#[test]
fn runtime_index_nested_tuple_subscript_with_owned_argument_remains_valid() {
    let mir = lower_source_to_mir(runtime_index_nested_tuple_subscript_source()).unwrap();
    run_mir(&mir).expect("owned callable nested tuple subscript remains valid");
    emit_host_native_object(&mir).expect("native validation accepts nested tuple subscript");
}

fn runtime_index_nested_tuple_pop_source() -> &'static str {
    "class Holder:\n    values: list[int64]\ndef consume(item: own Holder):\n    pass\ndef inspect(value: own Holder, index: int64):\n    mut callbacks: list[(def(own Holder) -> None, int64)] = [(consume, 0)]\n    entry = callbacks.pop(index)\n    callback: def(own Holder) -> None = entry[0]\n    callback(value)\ndef main():\n    pass\n"
}

#[test]
fn runtime_index_nested_tuple_pop_preserves_authoritative_callable_identity() {
    let mut encoded =
        serde_json::to_value(lower_source_to_mir(runtime_index_nested_tuple_pop_source()).unwrap())
            .unwrap();
    weaken_function_callable_metadata(&mut encoded, "inspect");
    assert_interpreter_and_native_reject_owned_laundering(
        &serde_json::from_value(encoded).unwrap(),
    );
}

#[test]
fn runtime_index_nested_tuple_pop_with_owned_argument_remains_valid() {
    let mir = lower_source_to_mir(runtime_index_nested_tuple_pop_source()).unwrap();
    run_mir(&mir).expect("owned callable nested tuple pop remains valid");
    emit_host_native_object(&mir).expect("native validation accepts nested tuple pop");
}

fn dict_values_nested_tuple_source() -> &'static str {
    "class Holder:\n    values: list[int64]\ndef consume(item: own Holder):\n    pass\ndef inspect(value: own Holder, index: int64):\n    callbacks: dict[str, (def(own Holder) -> None, int64)] = {\"consume\": (consume, 0)}\n    entries = callbacks.values()\n    entry = entries[index]\n    callback: def(own Holder) -> None = entry[0]\n    callback(value)\ndef main():\n    pass\n"
}

#[test]
fn dict_values_nested_tuple_preserves_authoritative_callable_identity() {
    let mut encoded =
        serde_json::to_value(lower_source_to_mir(dict_values_nested_tuple_source()).unwrap())
            .unwrap();
    weaken_function_callable_metadata(&mut encoded, "inspect");
    assert_interpreter_and_native_reject_owned_laundering(
        &serde_json::from_value(encoded).unwrap(),
    );
}

#[test]
fn dict_values_nested_tuple_with_owned_argument_remains_valid() {
    let mir = lower_source_to_mir(dict_values_nested_tuple_source()).unwrap();
    run_mir(&mir).expect("owned callable dictionary values extraction remains valid");
    emit_host_native_object(&mir).expect("native validation accepts dictionary values extraction");
}

fn runtime_index_doubly_nested_list_source() -> &'static str {
    "class Holder:\n    values: list[int64]\ndef consume(item: own Holder):\n    pass\ndef inspect(value: own Holder, index: int64):\n    callbacks: list[list[(def(own Holder) -> None, int64)]] = [[(consume, 0)]]\n    match callbacks.get(index):\n        case Some(inner):\n            match inner.get(index):\n                case Some(entry):\n                    entry[0](value)\n                case None:\n                    pass\n        case None:\n            pass\ndef main():\n    pass\n"
}

#[test]
fn runtime_index_doubly_nested_list_preserves_authoritative_callable_identity() {
    let mut encoded = serde_json::to_value(
        lower_source_to_mir(runtime_index_doubly_nested_list_source()).unwrap(),
    )
    .unwrap();
    weaken_function_callable_metadata(&mut encoded, "inspect");
    assert_interpreter_and_native_reject_owned_laundering(
        &serde_json::from_value(encoded).unwrap(),
    );
}

#[test]
fn runtime_index_doubly_nested_list_with_owned_argument_remains_valid() {
    let mir = lower_source_to_mir(runtime_index_doubly_nested_list_source()).unwrap();
    run_mir(&mir).expect("owned callable doubly nested extraction remains valid");
    emit_host_native_object(&mir).expect("native validation accepts doubly nested extraction");
}

fn runtime_index_direct_tuple_call_source() -> &'static str {
    "class Holder:\n    values: list[int64]\ndef consume(item: own Holder):\n    pass\ndef inspect(value: own Holder, index: int64):\n    callbacks: list[(def(own Holder) -> None, int64)] = [(consume, 0)]\n    match callbacks.get(index):\n        case Some(entry):\n            entry[0](value)\n        case None:\n            pass\ndef main():\n    pass\n"
}

#[test]
fn runtime_index_direct_tuple_call_preserves_authoritative_callable_identity() {
    let mut encoded = serde_json::to_value(
        lower_source_to_mir(runtime_index_direct_tuple_call_source()).unwrap(),
    )
    .unwrap();
    weaken_function_callable_metadata(&mut encoded, "inspect");
    assert_interpreter_and_native_reject_owned_laundering(
        &serde_json::from_value(encoded).unwrap(),
    );
}

#[test]
fn runtime_index_direct_tuple_call_with_owned_argument_remains_valid() {
    let mir = lower_source_to_mir(runtime_index_direct_tuple_call_source()).unwrap();
    run_mir(&mir).expect("owned callable direct tuple call remains valid");
    emit_host_native_object(&mir).expect("native validation accepts direct tuple call");
}

fn runtime_index_two_functions_source() -> &'static str {
    "def double(value: int64) -> int64:\n    return value * 2\ndef triple(value: int64) -> int64:\n    return value * 3\ndef pick(index: int64) -> int64:\n    tools: list[def(int64) -> int64] = [double, triple]\n    match tools.get(index):\n        case Some(tool):\n            return tool(7)\n        case None:\n            return -1\ndef main():\n    print(pick(0))\n    print(pick(1))\n"
}

#[test]
fn runtime_index_over_distinct_functions_with_equal_contracts_remains_valid() {
    let mir = lower_source_to_mir(runtime_index_two_functions_source()).unwrap();
    run_mir(&mir).expect("distinct functions with one contract remain callable by runtime index");
    emit_host_native_object(&mir)
        .expect("native validation accepts distinct functions with one contract");
}

fn runtime_index_task_start_source() -> &'static str {
    "class Holder:\n    values: list[int64]\ndef consume(item: own Holder):\n    pass\ndef inspect(value: own Holder, index: int64):\n    callbacks: list[(def(own Holder) -> None, int64)] = [(consume, 0)]\n    match callbacks.get(index):\n        case Some(entry):\n            callback: def(own Holder) -> None = entry[0]\n            with TaskGroup() as group:\n                group.start(callback, value)\n        case None:\n            pass\ndef main():\n    pass\n"
}

#[test]
fn runtime_index_task_start_preserves_authoritative_callable_identity() {
    let mut encoded =
        serde_json::to_value(lower_source_to_mir(runtime_index_task_start_source()).unwrap())
            .unwrap();
    weaken_function_callable_metadata(&mut encoded, "inspect");
    assert_interpreter_and_native_reject_owned_laundering(
        &serde_json::from_value(encoded).unwrap(),
    );
}

#[test]
fn runtime_index_task_start_with_owned_argument_remains_valid() {
    let mir = lower_source_to_mir(runtime_index_task_start_source()).unwrap();
    run_mir(&mir).expect("owned callable task start remains valid");
    emit_host_native_object(&mir).expect("native validation accepts task start");
}

fn runtime_index_returned_callable_source() -> &'static str {
    "class Holder:\n    values: list[int64]\ndef consume(item: own Holder):\n    pass\ndef choose(index: int64) -> def(own Holder) -> None:\n    callbacks: list[(def(own Holder) -> None, int64)] = [(consume, 0)]\n    match callbacks.get(index):\n        case Some(entry):\n            return entry[0]\n        case None:\n            return consume\ndef inspect(value: own Holder, index: int64):\n    callback = choose(index)\n    callback(value)\ndef main():\n    pass\n"
}

#[test]
fn runtime_index_returned_callable_preserves_contract_at_return() {
    let mut encoded = serde_json::to_value(
        lower_source_to_mir(runtime_index_returned_callable_source()).unwrap(),
    )
    .unwrap();
    weaken_function_callable_metadata(&mut encoded, "inspect");
    // Weaken the producer's declared return contract so the caller's forged
    // local type appears consistent with it.
    let choose = encoded["functions"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|function| function["name"] == "choose")
        .unwrap();
    fn weaken(value: &mut Value) {
        match value {
            Value::Array(values) => values.iter_mut().for_each(weaken),
            Value::Object(fields) => {
                if fields.contains_key("name") && fields.contains_key("signature") {
                    return;
                }
                if fields.get("passing") == Some(&json!("Value")) && fields.contains_key("ty") {
                    fields.insert("passing".to_owned(), json!("Borrow"));
                }
                fields.values_mut().for_each(weaken);
            }
            _ => {}
        }
    }
    weaken(&mut choose["return_type"]);
    weaken(&mut choose["local_types"]);
    weaken(&mut choose["blocks"]);
    assert_interpreter_and_native_reject_owned_laundering(
        &serde_json::from_value(encoded).unwrap(),
    );
}

#[test]
fn runtime_index_returned_callable_with_owned_argument_remains_valid() {
    let mir = lower_source_to_mir(runtime_index_returned_callable_source()).unwrap();
    run_mir(&mir).expect("owned callable return remains valid");
    emit_host_native_object(&mir).expect("native validation accepts returned callable");
}

fn runtime_index_writeback_source() -> &'static str {
    "class Holder:\n    values: list[int64]\nclass Slot:\n    callback: def(own Holder) -> None\ndef consume(item: own Holder):\n    pass\ndef inspect(value: own Holder, index: int64):\n    callbacks: list[def(own Holder) -> None] = [consume]\n    mut slot = Slot(callback=consume)\n    view mut current = slot.callback\n    current = callbacks[index]\n    slot.callback(value)\ndef main():\n    pass\n"
}

#[test]
fn runtime_index_writeback_preserves_authoritative_callable_identity() {
    let mut encoded =
        serde_json::to_value(lower_source_to_mir(runtime_index_writeback_source()).unwrap())
            .unwrap();
    // The view binding keeps its authentic loan type; only the derived
    // temporaries and the call are forged.
    weaken_function_callable_metadata_preserving(&mut encoded, "inspect", &["current"]);
    assert_interpreter_and_native_reject_owned_laundering(
        &serde_json::from_value(encoded).unwrap(),
    );
}

#[test]
fn runtime_index_writeback_with_owned_argument_remains_valid() {
    let mir = lower_source_to_mir(runtime_index_writeback_source()).unwrap();
    run_mir(&mir).expect("owned callable writeback remains valid");
    emit_host_native_object(&mir).expect("native validation accepts callable writeback");
}

fn appended_element_runtime_index_source() -> &'static str {
    "class Holder:\n    values: list[int64]\ndef consume(item: own Holder):\n    pass\ndef discard(item: own Holder):\n    pass\ndef inspect(value: own Holder, index: int64):\n    mut callbacks: list[def(own Holder) -> None] = [consume]\n    callbacks.append(discard)\n    callbacks.insert(0, consume)\n    match callbacks.get(index):\n        case Some(callback):\n            callback(value)\n        case None:\n            pass\ndef main():\n    pass\n"
}

#[test]
fn appended_element_runtime_index_preserves_authoritative_contract() {
    let mut encoded =
        serde_json::to_value(lower_source_to_mir(appended_element_runtime_index_source()).unwrap())
            .unwrap();
    weaken_function_callable_metadata(&mut encoded, "inspect");
    assert_interpreter_and_native_reject_owned_laundering(
        &serde_json::from_value(encoded).unwrap(),
    );
}

#[test]
fn appended_element_runtime_index_with_owned_argument_remains_valid() {
    let mir = lower_source_to_mir(appended_element_runtime_index_source()).unwrap();
    run_mir(&mir).expect("distinct appended functions with one contract remain valid");
    emit_host_native_object(&mir).expect("native validation accepts appended functions");
}

fn dict_runtime_key_index_source() -> &'static str {
    "class Holder:\n    values: list[int64]\ndef consume(item: own Holder):\n    pass\ndef inspect(value: own Holder, key: str):\n    callbacks: dict[str, (def(own Holder) -> None, int64)] = {\"consume\": (consume, 0)}\n    entry = callbacks[key]\n    entry[0](value)\ndef main():\n    pass\n"
}

#[test]
fn dict_runtime_key_index_preserves_authoritative_callable_identity() {
    let mut encoded =
        serde_json::to_value(lower_source_to_mir(dict_runtime_key_index_source()).unwrap())
            .unwrap();
    weaken_function_callable_metadata(&mut encoded, "inspect");
    assert_interpreter_and_native_reject_owned_laundering(
        &serde_json::from_value(encoded).unwrap(),
    );
}

#[test]
fn dict_runtime_key_index_with_owned_argument_remains_valid() {
    let mir = lower_source_to_mir(dict_runtime_key_index_source()).unwrap();
    run_mir(&mir).expect("owned callable dictionary index remains valid");
    emit_host_native_object(&mir).expect("native validation accepts dictionary index");
}

#[test]
fn runtime_index_over_mixed_contracts_is_poisoned_and_rejected() {
    // Two functions with different contracts forged into one list: a runtime
    // selection cannot prove which contract applies, so the call is rejected
    // even though the declared element metadata claims a weak contract.
    let source = "class Holder:\n    values: list[int64]\ndef consume(item: own Holder):\n    pass\ndef peek(item: Holder):\n    pass\ndef inspect(value: Holder, index: int64):\n    callbacks: list[def(Holder) -> None] = [peek]\n    others: list[def(own Holder) -> None] = [consume]\n    match callbacks.get(index):\n        case Some(callback):\n            callback(value)\n        case None:\n            pass\ndef main():\n    pass\n";
    let mut encoded = serde_json::to_value(lower_source_to_mir(source).unwrap()).unwrap();
    let inspect = encoded["functions"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|function| function["name"] == "inspect")
        .unwrap();
    // Splice the owned-contract function operand into the borrowed-contract
    // list literal so both elements are possible runtime selections.
    fn find_function_operand(value: &Value, name: &str) -> Option<Value> {
        match value {
            Value::Array(values) => values
                .iter()
                .find_map(|value| find_function_operand(value, name)),
            Value::Object(fields) => {
                if let Some(function) = fields.get("Function").and_then(Value::as_object) {
                    if function.get("name") == Some(&json!(name))
                        && function.contains_key("signature")
                    {
                        return Some(value.clone());
                    }
                }
                fields
                    .values()
                    .find_map(|value| find_function_operand(value, name))
            }
            _ => None,
        }
    }
    let consume_operand =
        find_function_operand(&inspect["blocks"], "consume").expect("consume operand present");
    let mut spliced = false;
    for block in inspect["blocks"].as_array_mut().unwrap() {
        for instruction in block["instructions"].as_array_mut().unwrap() {
            if instruction["Assign"]["target"] == "callbacks" {
                let elements = instruction["Assign"]["value"]["VecLiteral"]["elements"]
                    .as_array_mut()
                    .unwrap();
                elements.push(consume_operand.clone());
                spliced = true;
            }
        }
    }
    assert!(spliced, "list literal spliced");
    let mir: MirModule = serde_json::from_value(encoded).unwrap();
    let interpreted = run_mir(&mir).expect_err("interpreter must reject a poisoned selection");
    assert!(interpreted.message.contains("contract"), "{interpreted}");
    let native =
        emit_host_native_object(&mir).expect_err("native must reject a poisoned selection");
    assert!(native.contains("contract"), "{native}");
}

#[test]
fn runtime_index_returned_callable_rejects_forged_caller_result_metadata() {
    // Only the caller's declared result metadata is weakened; the producer's
    // return contract stays authentic. The call result must carry the callee's
    // declared return contract so the forged local type cannot be trusted.
    let mut encoded = serde_json::to_value(
        lower_source_to_mir(runtime_index_returned_callable_source()).unwrap(),
    )
    .unwrap();
    weaken_function_callable_metadata(&mut encoded, "inspect");
    assert_interpreter_and_native_reject_owned_laundering(
        &serde_json::from_value(encoded).unwrap(),
    );
}

// ---------------------------------------------------------------------------
// Independent adversarial review findings (2026-09-10): every path that loses
// a callable's authoritative identity lets forged declared metadata be
// trusted. Each regression forges the minimal MIR shape and requires both
// backends to reject it while its owned-argument counterpart stays valid.
// ---------------------------------------------------------------------------

/// Forges `function_name` so that `borrowed_param` becomes a borrowed
/// parameter, every declared callable passing is weakened to borrow (except
/// preserved locals and authenticated function operands), and every
/// `MovePlace` of `borrowed_param` becomes a borrowing `Place`.
fn forge_borrowed_callable_caller(
    encoded: &mut Value,
    function_name: &str,
    borrowed_param: &str,
    preserved_locals: &[&str],
) {
    let function = encoded["functions"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|function| function["name"] == function_name)
        .unwrap();
    for param in function["params"].as_array_mut().unwrap() {
        if param["name"] == borrowed_param {
            param["passing"] = json!("Borrow");
        }
    }
    fn weaken(value: &mut Value, borrowed_param: &str) {
        match value {
            Value::Array(values) => values
                .iter_mut()
                .for_each(|value| weaken(value, borrowed_param)),
            Value::Object(fields) => {
                if fields.contains_key("name") && fields.contains_key("signature") {
                    return;
                }
                if fields.keys().any(|key| {
                    matches!(
                        key.as_str(),
                        "BeginLoan"
                            | "BeginReturnedLoan"
                            | "Reborrow"
                            | "ReadLoan"
                            | "EndLoan"
                            | "ReturnLoan"
                    )
                }) {
                    return;
                }
                if fields.get("passing") == Some(&json!("Value")) && fields.contains_key("ty") {
                    fields.insert("passing".to_owned(), json!("Borrow"));
                }
                if fields.get("MovePlace") == Some(&json!(borrowed_param)) {
                    fields.remove("MovePlace");
                    fields.insert("Place".to_owned(), json!(borrowed_param));
                }
                fields
                    .values_mut()
                    .for_each(|value| weaken(value, borrowed_param));
            }
            _ => {}
        }
    }
    for local in function["local_types"].as_array_mut().unwrap() {
        let name = local["name"].as_str().unwrap_or_default().to_owned();
        if !preserved_locals.contains(&name.as_str()) {
            weaken(local, borrowed_param);
        }
    }
    weaken(&mut function["blocks"], borrowed_param);
}

/// Weakens every callable passing inside one function's declared parameter
/// and return types, simulating a forged callee contract.
fn weaken_function_declaration(encoded: &mut Value, function_name: &str) {
    let function = encoded["functions"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|function| function["name"] == function_name)
        .unwrap();
    fn weaken(value: &mut Value) {
        match value {
            Value::Array(values) => values.iter_mut().for_each(weaken),
            Value::Object(fields) => {
                if fields.get("passing") == Some(&json!("Value")) && fields.contains_key("ty") {
                    fields.insert("passing".to_owned(), json!("Borrow"));
                }
                fields.values_mut().for_each(weaken);
            }
            _ => {}
        }
    }
    for param in function["params"].as_array_mut().unwrap() {
        weaken(&mut param["ty"]);
    }
    weaken(&mut function["return_type"]);
}

fn assert_valid_on_both_backends(source: &str, label: &str) {
    let mir = lower_source_to_mir(source).unwrap_or_else(|error| panic!("{label}: {error:?}"));
    run_mir(&mir).unwrap_or_else(|error| panic!("{label} remains valid: {error}"));
    emit_host_native_object(&mir)
        .unwrap_or_else(|error| panic!("{label} native validation remains valid: {error}"));
}

fn holder_prelude() -> &'static str {
    "class Holder:\n    values: list[int64]\ndef consume(item: own Holder):\n    pass\ndef peek(item: Holder):\n    pass\n"
}

// Finding 1: aggregate-typed call boundaries carry callable identities that
// must be checked structurally against the callee's declared parameter type.
fn tuple_boundary_source() -> String {
    format!(
        "{}def invoke(pair: (def(own Holder) -> None, int64), value: own Holder):\n    callback: def(own Holder) -> None = pair[0]\n    callback(value)\ndef inspect(value: own Holder):\n    pair = (consume, 0)\n    invoke(pair, value)\ndef main():\n    pass\n",
        holder_prelude()
    )
}

#[test]
fn tuple_call_boundary_rejects_weakened_callee_parameter_contract() {
    let mut encoded =
        serde_json::to_value(lower_source_to_mir(&tuple_boundary_source()).unwrap()).unwrap();
    weaken_function_declaration(&mut encoded, "invoke");
    forge_borrowed_callable_caller(&mut encoded, "invoke", "value", &[]);
    forge_borrowed_callable_caller(&mut encoded, "inspect", "value", &[]);
    assert_interpreter_and_native_reject_owned_laundering(
        &serde_json::from_value(encoded).unwrap(),
    );
}

#[test]
fn tuple_call_boundary_with_owned_argument_remains_valid() {
    assert_valid_on_both_backends(&tuple_boundary_source(), "tuple call boundary");
}

fn list_boundary_source() -> String {
    format!(
        "{}def invoke(callbacks: list[def(own Holder) -> None], value: own Holder):\n    callback = callbacks[0]\n    callback(value)\ndef inspect(value: own Holder):\n    callbacks: list[def(own Holder) -> None] = [consume]\n    invoke(callbacks, value)\ndef main():\n    pass\n",
        holder_prelude()
    )
}

#[test]
fn list_call_boundary_rejects_weakened_callee_parameter_contract() {
    let mut encoded =
        serde_json::to_value(lower_source_to_mir(&list_boundary_source()).unwrap()).unwrap();
    weaken_function_declaration(&mut encoded, "invoke");
    forge_borrowed_callable_caller(&mut encoded, "invoke", "value", &[]);
    forge_borrowed_callable_caller(&mut encoded, "inspect", "value", &[]);
    assert_interpreter_and_native_reject_owned_laundering(
        &serde_json::from_value(encoded).unwrap(),
    );
}

#[test]
fn list_call_boundary_with_owned_argument_remains_valid() {
    assert_valid_on_both_backends(&list_boundary_source(), "list call boundary");
}

fn list_return_boundary_source() -> String {
    format!(
        "{}def choose() -> list[def(own Holder) -> None]:\n    callbacks: list[def(own Holder) -> None] = [consume]\n    return callbacks\ndef inspect(value: own Holder):\n    callbacks = choose()\n    callback = callbacks[0]\n    callback(value)\ndef main():\n    pass\n",
        holder_prelude()
    )
}

#[test]
fn list_return_boundary_rejects_weakened_return_contract() {
    let mut encoded =
        serde_json::to_value(lower_source_to_mir(&list_return_boundary_source()).unwrap()).unwrap();
    weaken_function_declaration(&mut encoded, "choose");
    forge_borrowed_callable_caller(&mut encoded, "inspect", "value", &[]);
    assert_interpreter_and_native_reject_owned_laundering(
        &serde_json::from_value(encoded).unwrap(),
    );
}

#[test]
fn list_return_boundary_with_owned_argument_remains_valid() {
    assert_valid_on_both_backends(&list_return_boundary_source(), "list return boundary");
}

// Finding 2: closure captures must satisfy the closure declaration's
// capture parameter contracts.
fn closure_capture_source() -> String {
    format!(
        "{}def inspect(value: own Holder):\n    callback = consume\n    run: def(own Holder) -> None = lambda own item: callback(item)\n    run(value)\ndef main():\n    pass\n",
        holder_prelude()
    )
}

#[test]
fn closure_capture_rejects_weakened_capture_parameter_contract() {
    let mut encoded =
        serde_json::to_value(lower_source_to_mir(&closure_capture_source()).unwrap()).unwrap();
    let closure_name = encoded["functions"]
        .as_array()
        .unwrap()
        .iter()
        .map(|function| function["name"].as_str().unwrap().to_owned())
        .find(|name| name != "inspect" && name.contains("inspect"))
        .expect("lifted closure function");
    weaken_function_declaration(&mut encoded, &closure_name);
    forge_borrowed_callable_caller(&mut encoded, &closure_name, "item", &[]);
    forge_borrowed_callable_caller(&mut encoded, "inspect", "value", &[]);
    assert_interpreter_and_native_reject_owned_laundering(
        &serde_json::from_value(encoded).unwrap(),
    );
}

#[test]
fn closure_capture_with_owned_argument_remains_valid() {
    assert_valid_on_both_backends(&closure_capture_source(), "closure capture");
}

// Finding 3: a control-flow join must poison, not drop, disagreeing identities.
fn join_source() -> String {
    format!(
        "{}def inspect(value: own Holder, flag: bool, index: int64):\n    mut callbacks: list[def(own Holder) -> None] = []\n    if flag:\n        callbacks = [consume]\n    match callbacks.get(index):\n        case Some(callback):\n            callback(value)\n        case None:\n            pass\ndef main():\n    pass\n",
        holder_prelude()
    )
}

#[test]
fn control_flow_join_poisons_disagreeing_callable_identities() {
    let mut encoded = serde_json::to_value(lower_source_to_mir(&join_source()).unwrap()).unwrap();
    forge_borrowed_callable_caller(&mut encoded, "inspect", "value", &[]);
    assert_interpreter_and_native_reject_owned_laundering(
        &serde_json::from_value(encoded).unwrap(),
    );
}

#[test]
fn control_flow_join_with_owned_argument_remains_valid() {
    assert_valid_on_both_backends(&join_source(), "control-flow join");
}

// Finding 4: mutation with an identity-less operand must poison the receiver.
fn extend_source() -> String {
    format!(
        "{}def inspect(value: own Holder, index: int64):\n    mut callbacks: list[def(own Holder) -> None] = [consume]\n    extra: list[def(own Holder) -> None] = []\n    callbacks.extend(extra)\n    match callbacks.get(index):\n        case Some(callback):\n            callback(value)\n        case None:\n            pass\ndef main():\n    pass\n",
        holder_prelude()
    )
}

#[test]
fn extend_with_identity_less_operand_poisons_receiver_identities() {
    let mut encoded = serde_json::to_value(lower_source_to_mir(&extend_source()).unwrap()).unwrap();
    forge_borrowed_callable_caller(&mut encoded, "inspect", "value", &[]);
    assert_interpreter_and_native_reject_owned_laundering(
        &serde_json::from_value(encoded).unwrap(),
    );
}

#[test]
fn extend_with_identity_less_operand_remains_valid() {
    assert_valid_on_both_backends(&extend_source(), "extend with empty list");
}

// Finding 5: `try` must carry the Ok payload's identity.
fn try_source() -> String {
    format!(
        "{}enum ToolError:\n    Broken\ndef pick() -> Result[def(own Holder) -> None, ToolError]:\n    return Result.Ok(consume)\ndef inspect(value: own Holder) -> Result[None, ToolError]:\n    callback = try pick()\n    callback(value)\n    return Result.Ok(None)\ndef main():\n    pass\n",
        holder_prelude()
    )
}

#[test]
fn try_preserves_ok_payload_callable_identity() {
    let mut encoded = serde_json::to_value(lower_source_to_mir(&try_source()).unwrap()).unwrap();
    forge_borrowed_callable_caller(&mut encoded, "inspect", "value", &[]);
    assert_interpreter_and_native_reject_owned_laundering(
        &serde_json::from_value(encoded).unwrap(),
    );
}

#[test]
fn try_with_owned_argument_remains_valid() {
    assert_valid_on_both_backends(&try_source(), "try payload");
}

// Finding 7: a callee whose declared type is a type parameter or a
// non-callable must not skip argument validation.
#[test]
fn type_parameter_callee_requires_authoritative_contract() {
    let source = format!(
        "{}def invoke[T](item: T, value: own Holder):\n    pass\ndef inspect(value: own Holder):\n    invoke(consume, value)\ndef main():\n    pass\n",
        holder_prelude()
    );
    let mut encoded = serde_json::to_value(lower_source_to_mir(&source).unwrap()).unwrap();
    let invoke = encoded["functions"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|function| function["name"] == "invoke")
        .unwrap();
    invoke["params"][1]["passing"] = json!("Borrow");
    invoke["local_types"]
        .as_array_mut()
        .unwrap()
        .push(json!({ "name": "%t0", "ty": "Unit" }));
    invoke["blocks"][0]["instructions"] = json!([{ "Assign": { "target": "%t0", "value": { "Call": {
        "callee": { "Value": { "Place": "item" }},
        "args": [{ "name": null, "value": { "Place": "value" }, "writeback_place": null }]
    }}}}]);
    forge_borrowed_callable_caller(&mut encoded, "inspect", "value", &[]);
    let mir: MirModule = serde_json::from_value(encoded).unwrap();
    let interpreted = run_mir(&mir).expect_err("interpreter must reject a type-parameter callee");
    assert!(interpreted.message.contains("contract"), "{interpreted}");
    let native =
        emit_host_native_object(&mir).expect_err("native must reject a type-parameter callee");
    assert!(native.contains("contract"), "{native}");
}

// Finding 9: `items` results are key/value tuples.
fn items_source() -> String {
    format!(
        "{}def inspect(value: own Holder, index: int64):\n    callbacks: dict[str, (def(own Holder) -> None, int64)] = {{\"consume\": (consume, 0)}}\n    entries = callbacks.items()\n    match entries.get(index):\n        case Some(entry):\n            entry[1][0](value)\n        case None:\n            pass\ndef main():\n    pass\n",
        holder_prelude()
    )
}

#[test]
fn dict_items_nested_tuple_preserves_authoritative_callable_identity() {
    let mut encoded = serde_json::to_value(lower_source_to_mir(&items_source()).unwrap()).unwrap();
    forge_borrowed_callable_caller(&mut encoded, "inspect", "value", &[]);
    assert_interpreter_and_native_reject_owned_laundering(
        &serde_json::from_value(encoded).unwrap(),
    );
}

#[test]
fn dict_items_nested_tuple_with_owned_argument_remains_valid() {
    assert_valid_on_both_backends(&items_source(), "dict items");
}

// Finding 9 (continued): `copy` results carry every element identity verbatim,
// so a copied dictionary keeps its key/value sides and a copied nested tuple
// keeps its callable position.
fn dict_copy_source() -> String {
    format!(
        "{}def inspect(value: own Holder, index: int64):\n    callbacks: dict[str, (def(own Holder) -> None, int64)] = {{\"consume\": (consume, 0)}}\n    copied = callbacks.copy()\n    match copied.get(\"consume\"):\n        case Some(entry):\n            entry[0](value)\n        case None:\n            pass\ndef main():\n    pass\n",
        holder_prelude()
    )
}

#[test]
fn dict_copy_preserves_authoritative_callable_identity() {
    let mut encoded =
        serde_json::to_value(lower_source_to_mir(&dict_copy_source()).unwrap()).unwrap();
    forge_borrowed_callable_caller(&mut encoded, "inspect", "value", &[]);
    assert_interpreter_and_native_reject_owned_laundering(
        &serde_json::from_value(encoded).unwrap(),
    );
}

#[test]
fn dict_copy_with_owned_argument_remains_valid() {
    assert_valid_on_both_backends(&dict_copy_source(), "dict copy");
}

// A stored closure value carries its body's exposed contract as its identity,
// so calling it through a local (repeatable or consuming, with or without
// borrowed captures) stays valid while a forged caller is still rejected.
fn consuming_closure_with_view_capture_source() -> String {
    format!(
        "{}def take(text: own str):\n    pass\ndef inspect(value: own Holder):\n    mut seen: list[int64] = []\n    text = \"owned\"\n    run: def(own Holder) -> (None, None, None) = lambda [mut seen, own text] own item: (seen.append(1), take(text), consume(item))\n    run(value)\ndef main():\n    pass\n",
        holder_prelude()
    )
}

#[test]
fn consuming_closure_with_view_capture_remains_valid() {
    assert_valid_on_both_backends(
        &consuming_closure_with_view_capture_source(),
        "consuming closure with view capture",
    );
}

// Finding 10: module constants and task results carry declared identities.
fn module_constant_source() -> String {
    format!(
        "{}TOOLS: list[def(own Holder) -> None] = [consume]\ndef inspect(value: own Holder, index: int64):\n    match TOOLS.get(index):\n        case Some(callback):\n            callback(value)\n        case None:\n            pass\ndef main():\n    pass\n",
        holder_prelude()
    )
}

#[test]
fn module_constant_preserves_authoritative_callable_identity() {
    let mut encoded =
        serde_json::to_value(lower_source_to_mir(&module_constant_source()).unwrap()).unwrap();
    forge_borrowed_callable_caller(&mut encoded, "inspect", "value", &[]);
    assert_interpreter_and_native_reject_owned_laundering(
        &serde_json::from_value(encoded).unwrap(),
    );
}

#[test]
fn module_constant_with_owned_argument_remains_valid() {
    assert_valid_on_both_backends(&module_constant_source(), "module constant");
}

fn task_result_source() -> String {
    format!(
        "{}def make() -> def(own Holder) -> None:\n    return consume\ndef inspect(value: own Holder):\n    with TaskGroup() as group:\n        task = group.start(make)\n        match task.result():\n            case TaskResult.Ready(callback):\n                callback(value)\n            case _:\n                pass\ndef main():\n    pass\n",
        holder_prelude()
    )
}

#[test]
fn task_result_preserves_authoritative_callable_identity() {
    let mut encoded =
        serde_json::to_value(lower_source_to_mir(&task_result_source()).unwrap()).unwrap();
    forge_borrowed_callable_caller(&mut encoded, "inspect", "value", &[]);
    assert_interpreter_and_native_reject_owned_laundering(
        &serde_json::from_value(encoded).unwrap(),
    );
}

#[test]
fn task_result_with_owned_argument_remains_valid() {
    assert_valid_on_both_backends(&task_result_source(), "task result");
}

// The validator-internal wildcard segment is not a canonical MIR place.
#[test]
fn wildcard_element_segment_is_not_a_canonical_place() {
    let source = "def main():\n    values: list[int64] = [1]\n    print(values.len())\n";
    let mut encoded = serde_json::to_value(lower_source_to_mir(source).unwrap()).unwrap();
    let main = encoded["functions"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|function| function["name"] == "main")
        .unwrap();
    main["local_types"]
        .as_array_mut()
        .unwrap()
        .push(json!({ "name": "%t90", "ty": { "Named": ["int64", []] } }));
    main["blocks"][0]["instructions"]
        .as_array_mut()
        .unwrap()
        .insert(0, json!({ "Assign": { "target": "%t90", "value": { "Use": { "Place": "values.__any_element" }}}}));
    let mir: MirModule = serde_json::from_value(encoded).unwrap();
    run_mir(&mir).expect_err("interpreter must reject the wildcard segment");
    emit_host_native_object(&mir).expect_err("native must reject the wildcard segment");
}
