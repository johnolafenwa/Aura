use std::collections::BTreeSet;
use std::collections::HashMap;
use std::path::PathBuf;

use cranelift_codegen::ir::types;
use cranelift_codegen::ir::InstBuilder;
use cranelift_codegen::Context;
use cranelift_frontend::FunctionBuilderContext;

use super::{
    box_thunk_value, builtin_opaque_member_return_type, collect_spawn_targets,
    collect_type_params_from_type, direct_field_type, direct_type, direct_type_to_type,
    emit_host_object, emit_host_object_with_metadata, ensure_direct_type, infer_operand_type,
    infer_rvalue_type, infer_select_binding_type, infer_try_type, is_numeric_type_name,
    main_signature, mangle_symbol, mangle_thunk_symbol, render_direct_type,
    runtime_type_is_wildcard, signature_for, thunk_signature, thunk_string_constant,
    unbox_thunk_value, validate_function, validate_operand, DirectType, NativeCodegen,
    PlainClassField, PlainClassType, ScalarKind,
};
use crate::ast::{BinaryOp, UnaryOp};
use crate::diag::Span;
use crate::mir::MirReceiverKind;
use crate::mir::{
    BasicBlock, CallTarget, Instruction, MirArg, MirFormatPart, MirFunction, MirLocalType,
    MirMapEntry, MirParam, MirSelectArm, MirSelectKind, Operand, Rvalue, Terminator,
};
use crate::sema::Type;
use crate::{lower_path_to_mir, lower_source_to_mir};

fn scalar_kind_for_tests(ty: &Type) -> Option<ScalarKind> {
    direct_type(ty, &HashMap::new()).and_then(|ty| ty.scalar_kind())
}

#[test]
fn direct_backend_emits_object_for_supported_scalar_program() {
    let source = "def helper(value: int32) -> int32:\n    return value + 2\n\ndef main() -> int32:\n    mut current: int32 = 1\n    if current < 5:\n        current = helper(value=current)\n    print(current)\n    return 0\n";

    let mir = lower_source_to_mir(source).expect("source should lower to MIR");
    let object = emit_host_object(&mir).expect("direct backend should emit an object");

    assert!(!object.is_empty());
}

#[test]
fn direct_field_type_rejects_malformed_builtin_map_entry_shapes_without_panicking() {
    let malformed = DirectType::Opaque(Type::Named(
        "MapEntry".to_string(),
        vec![Type::named("String")],
    ));
    assert!(direct_field_type(&malformed, "key", &HashMap::new()).is_none());
    assert!(direct_field_type(&malformed, "value", &HashMap::new()).is_none());
}

#[test]
fn direct_backend_emits_retain_and_release_hooks_for_opaque_call_and_local_flow() {
    let source = r#"
def echo(value: String):
    print(value)

def main() -> int32:
    mut text = "hello"
    echo(text)
    text = "goodbye"
    print(text)
    return 0
"#;

    let mir = lower_source_to_mir(source).expect("source should lower to MIR");
    let object = emit_host_object(&mir).expect("direct backend should emit an object");
    let rendered = String::from_utf8_lossy(&object);

    assert!(rendered.contains("aurora_direct_retain_value"));
    assert!(rendered.contains("aurora_direct_release_value"));
}

#[test]
fn direct_backend_emits_object_for_plain_class_programs() {
    let source = include_str!("../../../examples/point.au");
    let mir = lower_source_to_mir(source).expect("point example should lower to MIR");
    let object = emit_host_object(&mir).expect("plain classes should now be supported directly");

    assert!(!object.is_empty());
}

#[test]
fn direct_backend_emits_object_for_trait_impl_dispatch() {
    let source = include_str!("../../../examples/traits/greeter.au");
    let mir = lower_source_to_mir(source).expect("trait example should lower to MIR");
    let object = emit_host_object(&mir).expect("trait impl dispatch should now compile directly");

    assert!(!object.is_empty());
}

#[test]
fn direct_backend_emits_object_for_extended_feature_examples() {
    let examples = [
        (
            "collections/vec_polish",
            include_str!("../../../examples/collections/vec_polish.au"),
        ),
        (
            "collections/map_basics",
            include_str!("../../../examples/collections/map_basics.au"),
        ),
        (
            "collections/set_basics",
            include_str!("../../../examples/collections/set_basics.au"),
        ),
        (
            "control_flow/match_literals",
            include_str!("../../../examples/control_flow/match_literals.au"),
        ),
        (
            "concurrency/queues_spawn",
            include_str!("../../../examples/concurrency/queues_spawn.au"),
        ),
        (
            "concurrency/queue_timeout",
            include_str!("../../../examples/concurrency/queue_timeout.au"),
        ),
        (
            "concurrency/select_timeout_named",
            include_str!("../../../examples/concurrency/select_timeout_named.au"),
        ),
        (
            "error_handling/try_result",
            include_str!("../../../examples/error_handling/try_result.au"),
        ),
        (
            "numbers/numeric_builtins",
            include_str!("../../../examples/numbers/numeric_builtins.au"),
        ),
        (
            "strings/string_methods",
            include_str!("../../../examples/strings/string_methods.au"),
        ),
        (
            "strings/string_parsing_and_formatting",
            include_str!("../../../examples/strings/string_parsing_and_formatting.au"),
        ),
        (
            "traits/operator_traits",
            include_str!("../../../examples/traits/operator_traits.au"),
        ),
        (
            "traits/ordering_traits",
            include_str!("../../../examples/traits/ordering_traits.au"),
        ),
        (
            "basics/borrowed_lifetime_labels",
            include_str!("../../../examples/basics/borrowed_lifetime_labels.au"),
        ),
        (
            "traits/generic_trait_bounds",
            include_str!("../../../examples/traits/generic_trait_bounds.au"),
        ),
        (
            "traits/specialized_trait_dispatch",
            include_str!("../../../examples/traits/specialized_trait_dispatch.au"),
        ),
    ];

    for (name, source) in examples {
        let mir = lower_source_to_mir(source).expect("example should lower to MIR");
        let object = emit_host_object(&mir).expect("example should emit direct object");
        assert!(!object.is_empty(), "{name}");
    }
}

#[test]
fn direct_backend_emits_object_for_runtime_member_surface_matrix() {
    let source = r#"
def worker(value: int32) -> int32:
    return value + 1

def main() -> int32:
    text = "  Aurora Repo  "
    trimmed = text.trim()
    print(trimmed.len())
    print(trimmed.contains("Repo"))
    print(trimmed.starts_with("Aurora"))
    print(trimmed.ends_with("Repo"))
    print(trimmed.replace("Repo", "Lang"))
    print(trimmed.to_lower())
    print(trimmed.to_upper())
    words = trimmed.split(" ")
    print("/".join(words))
    match trimmed.strip_prefix("Aurora "):
        case Some(rest):
            print(rest)
        case None:
            print("missing")
    match trimmed.strip_suffix(" Repo"):
        case Some(rest):
            print(rest)
        case None:
            print("missing")

    mut numbers = [1, 2]
    print(numbers.len())
    print(numbers.is_empty())
    mut clone_numbers = numbers.clone()
    clone_numbers.push(3)
    print(clone_numbers.pop())
    print(clone_numbers.get(0))
    print(clone_numbers[1])
    print(clone_numbers.set(0, 9))
    clone_numbers[1] = 8
    print(clone_numbers.remove(0))
    print(clone_numbers.swap(0, 0))
    print(clone_numbers.contains(8))
    print(clone_numbers.insert(1, 7))
    clone_numbers.reverse()
    clone_numbers.extend([5, 6])
    clone_numbers.clear()
    print(clone_numbers.is_empty())

    mut counts = {"a": 1}
    print(counts.len())
    print(counts.is_empty())
    copy_counts = counts.clone()
    print(copy_counts.get("a"))
    print(copy_counts["a"])
    print(counts.set("a", 2))
    counts["b"] = 3
    print(counts.remove("a"))
    print(counts.contains_key("b"))
    print(counts.keys().len())
    print(counts.values().len())
    print(counts.items().len())
    print(counts.entries().len())
    counts.extend({"c": 4})
    counts.clear()
    print(counts.is_empty())

    mut seen = Set{"x"}
    print(seen.len())
    print(seen.is_empty())
    copy_seen = seen.clone()
    print(copy_seen.contains("x"))
    print(seen.insert("y"))
    print(seen.remove("x"))
    print(seen.contains("y"))

    jobs: Queue[int32] = queue()
    jobs_copy = jobs
    print(jobs_copy.put(1))
    print(jobs.get())
    jobs.close()

    task = spawn worker(4)
    task_copy = task
    print(task_copy.result())

    with tasks() as group:
        group.cancel()

    return 0
"#;

    let mir = lower_source_to_mir(source).expect("runtime member matrix should lower to MIR");
    let object = emit_host_object(&mir).expect("runtime member matrix should compile directly");

    assert!(!object.is_empty());
}

#[test]
fn direct_backend_emits_object_for_call_writeback_and_cleanup_surface() {
    let source = r#"
class Counter:
    value: int32

class Resource:
    closed: bool = false
    def close(borrow mut self):
        self.closed = true

def bump(counter: borrow mut Counter, amount: int32 = 2) -> int32:
    counter.value += amount
    return counter.value

def copy_into(source: borrow Counter, target: borrow mut Counter):
    target.value = source.value

def worker(value: int32) -> int32:
    return value + 1

def main() -> int32:
    mut first = Counter(value=1)
    mut second = Counter(value=0)
    print(bump(counter=first))
    copy_into(source=first, target=second)
    print(second.value)

    mut total = 0
    for i in range(stop=3):
        total += i
    print(total)

    print(abs(-3))
    print(min(8, 2))
    print(max(8, 2))
    print(parse_int32("12"))
    print(cancelled())

    jobs: Queue[int32] = queue()
    print(jobs.put(7))
    select:
        case value = jobs.get():
            print(value)
        case after(duration=1ms):
            print(99)
    jobs.close()

    sleep(0ms)

    with Resource() as resource:
        print(resource.closed)

    task = spawn worker(4)
    print(task.result())

    with tasks() as group:
        group.cancel()

    return second.value
"#;

    let mir = lower_source_to_mir(source).expect("writeback/cleanup matrix should lower to MIR");
    let object = emit_host_object(&mir).expect("writeback/cleanup matrix should compile directly");
    assert!(!object.is_empty());
}

#[test]
fn direct_backend_emits_object_for_supported_cleanup_and_explicit_task_group_close() {
    let source = r#"
class Resource:
    closed: bool = false
    def close(borrow mut self):
        self.closed = true

def main() -> int32:
    with Resource() as resource:
        print(resource.closed)

    with tasks() as group:
        group.cancel()

    return 0
"#;

    let mir = lower_source_to_mir(source).expect("cleanup surface should lower to MIR");
    let object = emit_host_object(&mir).expect("cleanup surface should compile directly");

    assert!(!object.is_empty());
}

fn module_with_main_call(call: Rvalue) -> crate::mir::MirModule {
    crate::mir::MirModule {
        functions: vec![MirFunction {
            name: "main".to_string(),
            module_name: "<test>".to_string(),
            span: Span::new(1, 1),
            receiver: None,
            params: Vec::new(),
            local_types: vec![MirLocalType {
                name: "%t0".to_string(),
                ty: Type::named("int32"),
            }],
            return_type: Type::named("int32"),
            entry: "entry".to_string(),
            blocks: vec![BasicBlock {
                label: "entry".to_string(),
                instructions: vec![Instruction::Assign {
                    target: "%t0".to_string(),
                    value: call,
                }],
                terminator: Terminator::Return(Operand::Int(0)),
            }],
        }],
        classes: Vec::new(),
        trait_impls: Vec::new(),
        top_level: None,
    }
}

fn module_with_main_member_call(
    object_name: &str,
    object_ty: Type,
    object_value: Rvalue,
    field: &str,
    args: Vec<MirArg>,
) -> crate::mir::MirModule {
    module_with_main_member_call_result_type(
        object_name,
        object_ty,
        object_value,
        Type::named("int32"),
        field,
        args,
    )
}

fn module_with_main_member_call_result_type(
    object_name: &str,
    object_ty: Type,
    object_value: Rvalue,
    result_ty: Type,
    field: &str,
    args: Vec<MirArg>,
) -> crate::mir::MirModule {
    crate::mir::MirModule {
        functions: vec![MirFunction {
            name: "main".to_string(),
            module_name: "<test>".to_string(),
            span: Span::new(1, 1),
            receiver: None,
            params: Vec::new(),
            local_types: vec![
                MirLocalType {
                    name: object_name.to_string(),
                    ty: object_ty,
                },
                MirLocalType {
                    name: "%t0".to_string(),
                    ty: result_ty,
                },
            ],
            return_type: Type::named("int32"),
            entry: "entry".to_string(),
            blocks: vec![BasicBlock {
                label: "entry".to_string(),
                instructions: vec![
                    Instruction::Assign {
                        target: object_name.to_string(),
                        value: object_value,
                    },
                    Instruction::Assign {
                        target: "%t0".to_string(),
                        value: Rvalue::Call {
                            callee: CallTarget::Member {
                                object: Operand::Place(object_name.to_string()),
                                field: field.to_string(),
                                receiver_place: Some(object_name.to_string()),
                            },
                            args,
                        },
                    },
                ],
                terminator: Terminator::Return(Operand::Int(0)),
            }],
        }],
        classes: Vec::new(),
        trait_impls: Vec::new(),
        top_level: None,
    }
}

#[test]
fn direct_backend_internal_collection_member_surface_compiles() {
    let vec_index_option = module_with_main_member_call_result_type(
        "values",
        Type::Named("Vec".to_string(), vec![Type::named("int32")]),
        Rvalue::VecLiteral {
            element_type: Type::named("int32"),
            elements: vec![Operand::Int(1), Operand::Int(2)],
        },
        Type::Named("Option".to_string(), vec![Type::named("int32")]),
        "__index_option",
        vec![MirArg {
            name: None,
            value: Operand::Int(1),
            writeback_place: None,
        }],
    );
    assert!(!emit_host_object(&vec_index_option)
        .expect("internal vec optional indexing should compile directly")
        .is_empty());

    let vec_set_index = module_with_main_member_call_result_type(
        "values",
        Type::Named("Vec".to_string(), vec![Type::named("int32")]),
        Rvalue::VecLiteral {
            element_type: Type::named("int32"),
            elements: vec![Operand::Int(1), Operand::Int(2)],
        },
        Type::Unit,
        "__set_index",
        vec![
            MirArg {
                name: None,
                value: Operand::Int(0),
                writeback_place: None,
            },
            MirArg {
                name: None,
                value: Operand::Int(9),
                writeback_place: None,
            },
            MirArg {
                name: None,
                value: Operand::Int(1),
                writeback_place: None,
            },
            MirArg {
                name: None,
                value: Operand::Int(1),
                writeback_place: None,
            },
        ],
    );
    assert!(!emit_host_object(&vec_set_index)
        .expect("internal vec indexed assignment should compile directly")
        .is_empty());

    let map_index = module_with_main_member_call_result_type(
        "counts",
        Type::Named(
            "Map".to_string(),
            vec![Type::named("String"), Type::named("int32")],
        ),
        Rvalue::MapLiteral {
            key_type: Type::named("String"),
            value_type: Type::named("int32"),
            entries: vec![MirMapEntry {
                key: Operand::String("a".to_string()),
                value: Operand::Int(1),
            }],
        },
        Type::named("int32"),
        "__index",
        vec![
            MirArg {
                name: None,
                value: Operand::String("a".to_string()),
                writeback_place: None,
            },
            MirArg {
                name: None,
                value: Operand::Int(1),
                writeback_place: None,
            },
            MirArg {
                name: None,
                value: Operand::Int(1),
                writeback_place: None,
            },
        ],
    );
    assert!(!emit_host_object(&map_index)
        .expect("internal map indexing should compile directly")
        .is_empty());

    let map_set_index = module_with_main_member_call_result_type(
        "counts",
        Type::Named(
            "Map".to_string(),
            vec![Type::named("String"), Type::named("int32")],
        ),
        Rvalue::MapLiteral {
            key_type: Type::named("String"),
            value_type: Type::named("int32"),
            entries: vec![MirMapEntry {
                key: Operand::String("a".to_string()),
                value: Operand::Int(1),
            }],
        },
        Type::Unit,
        "__set_index",
        vec![
            MirArg {
                name: None,
                value: Operand::String("b".to_string()),
                writeback_place: None,
            },
            MirArg {
                name: None,
                value: Operand::Int(2),
                writeback_place: None,
            },
            MirArg {
                name: None,
                value: Operand::Int(1),
                writeback_place: None,
            },
            MirArg {
                name: None,
                value: Operand::Int(1),
                writeback_place: None,
            },
        ],
    );
    assert!(!emit_host_object(&map_set_index)
        .expect("internal map indexed assignment should compile directly")
        .is_empty());

    let set_index_option = module_with_main_member_call_result_type(
        "seen",
        Type::Named("Set".to_string(), vec![Type::named("String")]),
        Rvalue::SetLiteral {
            element_type: Type::named("String"),
            elements: vec![Operand::String("x".to_string())],
        },
        Type::Named("Option".to_string(), vec![Type::named("String")]),
        "__index_option",
        vec![MirArg {
            name: None,
            value: Operand::Int(0),
            writeback_place: None,
        }],
    );
    assert!(!emit_host_object(&set_index_option)
        .expect("internal set optional indexing should compile directly")
        .is_empty());
}

#[test]
fn direct_backend_internal_collection_member_errors_are_reported() {
    let cases = [
        (
            module_with_main_member_call_result_type(
                "values",
                Type::Named("Vec".to_string(), vec![Type::named("int32")]),
                Rvalue::VecLiteral {
                    element_type: Type::named("int32"),
                    elements: vec![Operand::Int(1)],
                },
                Type::Named("Option".to_string(), vec![Type::named("int32")]),
                "__index_option",
                Vec::new(),
            ),
            "internal optional vector indexing",
        ),
        (
            module_with_main_member_call_result_type(
                "values",
                Type::Named("Vec".to_string(), vec![Type::named("int32")]),
                Rvalue::VecLiteral {
                    element_type: Type::named("int32"),
                    elements: vec![Operand::Int(1)],
                },
                Type::named("int32"),
                "__index",
                vec![MirArg {
                    name: None,
                    value: Operand::Int(0),
                    writeback_place: None,
                }],
            ),
            "internal vector indexing",
        ),
        (
            module_with_main_member_call_result_type(
                "counts",
                Type::Named(
                    "Map".to_string(),
                    vec![Type::named("String"), Type::named("int32")],
                ),
                Rvalue::MapLiteral {
                    key_type: Type::named("String"),
                    value_type: Type::named("int32"),
                    entries: vec![MirMapEntry {
                        key: Operand::String("a".to_string()),
                        value: Operand::Int(1),
                    }],
                },
                Type::Unit,
                "__set_index",
                vec![MirArg {
                    name: None,
                    value: Operand::String("a".to_string()),
                    writeback_place: None,
                }],
            ),
            "internal map indexed assignment",
        ),
        (
            module_with_main_member_call_result_type(
                "seen",
                Type::Named("Set".to_string(), vec![Type::named("String")]),
                Rvalue::SetLiteral {
                    element_type: Type::named("String"),
                    elements: vec![Operand::String("x".to_string())],
                },
                Type::Named("Option".to_string(), vec![Type::named("String")]),
                "__index_option",
                Vec::new(),
            ),
            "internal optional set indexing",
        ),
    ];

    for (module, expected) in cases {
        let error = emit_host_object(&module).expect_err("invalid internal collection member call");
        assert!(
            error.contains(expected),
            "expected `{expected}` in `{error}`"
        );
    }
}

#[test]
fn direct_backend_runtime_member_matrix_covers_remaining_string_collection_and_runtime_paths() {
    let string_ty = Type::named("String");
    let string_value = Rvalue::Use(Operand::String("Aurora".to_string()));
    let vec_ty = Type::Named("Vec".to_string(), vec![Type::named("int32")]);
    let vec_value = Rvalue::VecLiteral {
        element_type: Type::named("int32"),
        elements: vec![Operand::Int(1), Operand::Int(2)],
    };
    let map_ty = Type::Named(
        "Map".to_string(),
        vec![Type::named("String"), Type::named("int32")],
    );
    let map_value = Rvalue::MapLiteral {
        key_type: Type::named("String"),
        value_type: Type::named("int32"),
        entries: vec![MirMapEntry {
            key: Operand::String("count".to_string()),
            value: Operand::Int(1),
        }],
    };
    let set_ty = Type::Named("Set".to_string(), vec![Type::named("String")]);
    let set_value = Rvalue::SetLiteral {
        element_type: Type::named("String"),
        elements: vec![Operand::String("ready".to_string())],
    };
    let channel_ty = Type::Named("Queue".to_string(), vec![Type::named("int32")]);
    let channel_value = Rvalue::Call {
        callee: CallTarget::Name("queue".to_string()),
        args: Vec::new(),
    };
    let task_group_ty = Type::named("TaskGroup");
    let task_group_value = Rvalue::Call {
        callee: CallTarget::Name("tasks".to_string()),
        args: Vec::new(),
    };

    let cases = vec![
        (
            "String.contains",
            module_with_main_member_call_result_type(
                "text",
                string_ty.clone(),
                string_value.clone(),
                Type::named("bool"),
                "contains",
                vec![MirArg {
                    name: None,
                    value: Operand::String("ror".to_string()),
                    writeback_place: None,
                }],
            ),
        ),
        (
            "String.starts_with",
            module_with_main_member_call_result_type(
                "text",
                string_ty.clone(),
                string_value.clone(),
                Type::named("bool"),
                "starts_with",
                vec![MirArg {
                    name: None,
                    value: Operand::String("Aur".to_string()),
                    writeback_place: None,
                }],
            ),
        ),
        (
            "String.ends_with",
            module_with_main_member_call_result_type(
                "text",
                string_ty.clone(),
                string_value.clone(),
                Type::named("bool"),
                "ends_with",
                vec![MirArg {
                    name: None,
                    value: Operand::String("ora".to_string()),
                    writeback_place: None,
                }],
            ),
        ),
        (
            "String.split",
            module_with_main_member_call_result_type(
                "text",
                string_ty.clone(),
                string_value.clone(),
                Type::Named("Vec".to_string(), vec![Type::named("String")]),
                "split",
                vec![MirArg {
                    name: None,
                    value: Operand::String("r".to_string()),
                    writeback_place: None,
                }],
            ),
        ),
        (
            "String.replace",
            module_with_main_member_call_result_type(
                "text",
                string_ty.clone(),
                string_value.clone(),
                Type::named("String"),
                "replace",
                vec![
                    MirArg {
                        name: None,
                        value: Operand::String("Aur".to_string()),
                        writeback_place: None,
                    },
                    MirArg {
                        name: None,
                        value: Operand::String("Our".to_string()),
                        writeback_place: None,
                    },
                ],
            ),
        ),
        (
            "String.to_lower",
            module_with_main_member_call_result_type(
                "text",
                string_ty.clone(),
                string_value.clone(),
                Type::named("String"),
                "to_lower",
                Vec::new(),
            ),
        ),
        (
            "String.to_upper",
            module_with_main_member_call_result_type(
                "text",
                string_ty.clone(),
                string_value.clone(),
                Type::named("String"),
                "to_upper",
                Vec::new(),
            ),
        ),
        (
            "String.strip_prefix",
            module_with_main_member_call_result_type(
                "text",
                string_ty.clone(),
                string_value.clone(),
                Type::Named("Option".to_string(), vec![Type::named("String")]),
                "strip_prefix",
                vec![MirArg {
                    name: None,
                    value: Operand::String("Au".to_string()),
                    writeback_place: None,
                }],
            ),
        ),
        (
            "String.strip_suffix",
            module_with_main_member_call_result_type(
                "text",
                string_ty.clone(),
                string_value.clone(),
                Type::Named("Option".to_string(), vec![Type::named("String")]),
                "strip_suffix",
                vec![MirArg {
                    name: None,
                    value: Operand::String("ra".to_string()),
                    writeback_place: None,
                }],
            ),
        ),
        (
            "String.trim",
            module_with_main_member_call_result_type(
                "text",
                string_ty.clone(),
                Rvalue::Use(Operand::String("  Aurora  ".to_string())),
                Type::named("String"),
                "trim",
                Vec::new(),
            ),
        ),
        (
            "Vec.len",
            module_with_main_member_call_result_type(
                "values",
                vec_ty.clone(),
                vec_value.clone(),
                Type::named("int32"),
                "len",
                Vec::new(),
            ),
        ),
        (
            "Vec.is_empty",
            module_with_main_member_call_result_type(
                "values",
                vec_ty.clone(),
                vec_value.clone(),
                Type::named("bool"),
                "is_empty",
                Vec::new(),
            ),
        ),
        (
            "Vec.push",
            module_with_main_member_call_result_type(
                "values",
                vec_ty.clone(),
                vec_value.clone(),
                Type::Unit,
                "push",
                vec![MirArg {
                    name: None,
                    value: Operand::Int(3),
                    writeback_place: None,
                }],
            ),
        ),
        (
            "Vec.pop",
            module_with_main_member_call_result_type(
                "values",
                vec_ty.clone(),
                vec_value.clone(),
                Type::Named("Option".to_string(), vec![Type::named("int32")]),
                "pop",
                Vec::new(),
            ),
        ),
        (
            "Vec.get",
            module_with_main_member_call_result_type(
                "values",
                vec_ty.clone(),
                vec_value.clone(),
                Type::Named("Option".to_string(), vec![Type::named("int32")]),
                "get",
                vec![MirArg {
                    name: None,
                    value: Operand::Int(0),
                    writeback_place: None,
                }],
            ),
        ),
        (
            "Vec.set",
            module_with_main_member_call_result_type(
                "values",
                vec_ty.clone(),
                vec_value.clone(),
                Type::Named("Option".to_string(), vec![Type::named("int32")]),
                "set",
                vec![
                    MirArg {
                        name: None,
                        value: Operand::Int(0),
                        writeback_place: None,
                    },
                    MirArg {
                        name: None,
                        value: Operand::Int(9),
                        writeback_place: None,
                    },
                ],
            ),
        ),
        (
            "Vec.remove",
            module_with_main_member_call_result_type(
                "values",
                vec_ty.clone(),
                vec_value.clone(),
                Type::Named("Option".to_string(), vec![Type::named("int32")]),
                "remove",
                vec![MirArg {
                    name: None,
                    value: Operand::Int(1),
                    writeback_place: None,
                }],
            ),
        ),
        (
            "Vec.swap",
            module_with_main_member_call_result_type(
                "values",
                vec_ty.clone(),
                vec_value.clone(),
                Type::named("bool"),
                "swap",
                vec![
                    MirArg {
                        name: None,
                        value: Operand::Int(0),
                        writeback_place: None,
                    },
                    MirArg {
                        name: None,
                        value: Operand::Int(1),
                        writeback_place: None,
                    },
                ],
            ),
        ),
        (
            "Vec.contains",
            module_with_main_member_call_result_type(
                "values",
                vec_ty.clone(),
                vec_value.clone(),
                Type::named("bool"),
                "contains",
                vec![MirArg {
                    name: None,
                    value: Operand::Int(2),
                    writeback_place: None,
                }],
            ),
        ),
        (
            "Vec.insert",
            module_with_main_member_call_result_type(
                "values",
                vec_ty.clone(),
                vec_value.clone(),
                Type::named("bool"),
                "insert",
                vec![
                    MirArg {
                        name: None,
                        value: Operand::Int(1),
                        writeback_place: None,
                    },
                    MirArg {
                        name: None,
                        value: Operand::Int(5),
                        writeback_place: None,
                    },
                ],
            ),
        ),
        (
            "Vec.clear",
            module_with_main_member_call_result_type(
                "values",
                vec_ty.clone(),
                vec_value.clone(),
                Type::Unit,
                "clear",
                Vec::new(),
            ),
        ),
        (
            "Vec.reverse",
            module_with_main_member_call_result_type(
                "values",
                vec_ty.clone(),
                vec_value.clone(),
                Type::Unit,
                "reverse",
                Vec::new(),
            ),
        ),
        (
            "Map.len",
            module_with_main_member_call_result_type(
                "counts",
                map_ty.clone(),
                map_value.clone(),
                Type::named("int32"),
                "len",
                Vec::new(),
            ),
        ),
        (
            "Map.is_empty",
            module_with_main_member_call_result_type(
                "counts",
                map_ty.clone(),
                map_value.clone(),
                Type::named("bool"),
                "is_empty",
                Vec::new(),
            ),
        ),
        (
            "Map.get",
            module_with_main_member_call_result_type(
                "counts",
                map_ty.clone(),
                map_value.clone(),
                Type::Named("Option".to_string(), vec![Type::named("int32")]),
                "get",
                vec![MirArg {
                    name: None,
                    value: Operand::String("count".to_string()),
                    writeback_place: None,
                }],
            ),
        ),
        (
            "Map.set",
            module_with_main_member_call_result_type(
                "counts",
                map_ty.clone(),
                map_value.clone(),
                Type::Named("Option".to_string(), vec![Type::named("int32")]),
                "set",
                vec![
                    MirArg {
                        name: None,
                        value: Operand::String("count".to_string()),
                        writeback_place: None,
                    },
                    MirArg {
                        name: None,
                        value: Operand::Int(2),
                        writeback_place: None,
                    },
                ],
            ),
        ),
        (
            "Map.remove",
            module_with_main_member_call_result_type(
                "counts",
                map_ty.clone(),
                map_value.clone(),
                Type::Named("Option".to_string(), vec![Type::named("int32")]),
                "remove",
                vec![MirArg {
                    name: None,
                    value: Operand::String("count".to_string()),
                    writeback_place: None,
                }],
            ),
        ),
        (
            "Map.contains_key",
            module_with_main_member_call_result_type(
                "counts",
                map_ty.clone(),
                map_value.clone(),
                Type::named("bool"),
                "contains_key",
                vec![MirArg {
                    name: None,
                    value: Operand::String("count".to_string()),
                    writeback_place: None,
                }],
            ),
        ),
        (
            "Map.keys",
            module_with_main_member_call_result_type(
                "counts",
                map_ty.clone(),
                map_value.clone(),
                Type::Named("Vec".to_string(), vec![Type::named("String")]),
                "keys",
                Vec::new(),
            ),
        ),
        (
            "Map.values",
            module_with_main_member_call_result_type(
                "counts",
                map_ty.clone(),
                map_value.clone(),
                Type::Named("Vec".to_string(), vec![Type::named("int32")]),
                "values",
                Vec::new(),
            ),
        ),
        (
            "Map.items",
            module_with_main_member_call_result_type(
                "counts",
                map_ty.clone(),
                map_value.clone(),
                Type::Named(
                    "Vec".to_string(),
                    vec![Type::Named(
                        "MapEntry".to_string(),
                        vec![Type::named("String"), Type::named("int32")],
                    )],
                ),
                "items",
                Vec::new(),
            ),
        ),
        (
            "Map.entries",
            module_with_main_member_call_result_type(
                "counts",
                map_ty.clone(),
                map_value.clone(),
                Type::Named(
                    "Vec".to_string(),
                    vec![Type::Named(
                        "MapEntry".to_string(),
                        vec![Type::named("String"), Type::named("int32")],
                    )],
                ),
                "entries",
                Vec::new(),
            ),
        ),
        (
            "Map.clear",
            module_with_main_member_call_result_type(
                "counts",
                map_ty.clone(),
                map_value.clone(),
                Type::Unit,
                "clear",
                Vec::new(),
            ),
        ),
        (
            "Set.len",
            module_with_main_member_call_result_type(
                "seen",
                set_ty.clone(),
                set_value.clone(),
                Type::named("int32"),
                "len",
                Vec::new(),
            ),
        ),
        (
            "Set.is_empty",
            module_with_main_member_call_result_type(
                "seen",
                set_ty.clone(),
                set_value.clone(),
                Type::named("bool"),
                "is_empty",
                Vec::new(),
            ),
        ),
        (
            "Set.contains",
            module_with_main_member_call_result_type(
                "seen",
                set_ty.clone(),
                set_value.clone(),
                Type::named("bool"),
                "contains",
                vec![MirArg {
                    name: None,
                    value: Operand::String("ready".to_string()),
                    writeback_place: None,
                }],
            ),
        ),
        (
            "Set.insert",
            module_with_main_member_call_result_type(
                "seen",
                set_ty.clone(),
                set_value.clone(),
                Type::named("bool"),
                "insert",
                vec![MirArg {
                    name: None,
                    value: Operand::String("go".to_string()),
                    writeback_place: None,
                }],
            ),
        ),
        (
            "Set.remove",
            module_with_main_member_call_result_type(
                "seen",
                set_ty.clone(),
                set_value.clone(),
                Type::named("bool"),
                "remove",
                vec![MirArg {
                    name: None,
                    value: Operand::String("ready".to_string()),
                    writeback_place: None,
                }],
            ),
        ),
        (
            "Queue.put",
            module_with_main_member_call_result_type(
                "jobs",
                channel_ty.clone(),
                channel_value.clone(),
                Type::Named(
                    "Result".to_string(),
                    vec![
                        Type::Unit,
                        Type::Named("SendError".to_string(), vec![Type::named("int32")]),
                    ],
                ),
                "put",
                vec![MirArg {
                    name: None,
                    value: Operand::Int(1),
                    writeback_place: None,
                }],
            ),
        ),
        (
            "Queue.get",
            module_with_main_member_call_result_type(
                "jobs",
                channel_ty.clone(),
                channel_value.clone(),
                Type::Named("Option".to_string(), vec![Type::named("int32")]),
                "get",
                Vec::new(),
            ),
        ),
        (
            "Queue.close",
            module_with_main_member_call_result_type(
                "jobs",
                channel_ty.clone(),
                channel_value,
                Type::Unit,
                "close",
                Vec::new(),
            ),
        ),
        (
            "TaskGroup.cancel",
            module_with_main_member_call_result_type(
                "group",
                task_group_ty.clone(),
                task_group_value.clone(),
                Type::Unit,
                "cancel",
                Vec::new(),
            ),
        ),
    ];

    for (name, module) in cases {
        let object = emit_host_object(&module).expect("runtime member surface should compile");
        assert!(!object.is_empty(), "{name}");
    }
}

#[test]
fn direct_backend_runtime_member_arity_errors_cover_string_collection_and_runtime_paths() {
    let string_ty = Type::named("String");
    let string_value = Rvalue::Use(Operand::String("Aurora".to_string()));
    let vec_ty = Type::Named("Vec".to_string(), vec![Type::named("int32")]);
    let vec_value = Rvalue::VecLiteral {
        element_type: Type::named("int32"),
        elements: vec![Operand::Int(1), Operand::Int(2)],
    };
    let map_ty = Type::Named(
        "Map".to_string(),
        vec![Type::named("String"), Type::named("int32")],
    );
    let map_value = Rvalue::MapLiteral {
        key_type: Type::named("String"),
        value_type: Type::named("int32"),
        entries: vec![MirMapEntry {
            key: Operand::String("count".to_string()),
            value: Operand::Int(1),
        }],
    };
    let set_ty = Type::Named("Set".to_string(), vec![Type::named("String")]);
    let set_value = Rvalue::SetLiteral {
        element_type: Type::named("String"),
        elements: vec![Operand::String("ready".to_string())],
    };
    let channel_ty = Type::Named("Queue".to_string(), vec![Type::named("int32")]);
    let channel_value = Rvalue::Call {
        callee: CallTarget::Name("queue".to_string()),
        args: Vec::new(),
    };
    let task_group_ty = Type::named("TaskGroup");
    let task_group_value = Rvalue::Call {
        callee: CallTarget::Name("tasks".to_string()),
        args: Vec::new(),
    };

    let cases = vec![
        (
            module_with_main_member_call_result_type(
                "text",
                string_ty.clone(),
                string_value.clone(),
                Type::named("int32"),
                "len",
                vec![MirArg {
                    name: None,
                    value: Operand::Int(1),
                    writeback_place: None,
                }],
            ),
            "expected `len()` to take no arguments",
        ),
        (
            module_with_main_member_call_result_type(
                "text",
                string_ty.clone(),
                string_value.clone(),
                Type::named("bool"),
                "contains",
                Vec::new(),
            ),
            "expected `contains`() to receive one string argument",
        ),
        (
            module_with_main_member_call_result_type(
                "text",
                string_ty.clone(),
                string_value.clone(),
                Type::named("String"),
                "replace",
                vec![MirArg {
                    name: None,
                    value: Operand::String("Aur".to_string()),
                    writeback_place: None,
                }],
            ),
            "expected `replace()` to receive `from` and `to` string arguments",
        ),
        (
            module_with_main_member_call_result_type(
                "text",
                string_ty.clone(),
                string_value.clone(),
                Type::named("String"),
                "trim",
                vec![MirArg {
                    name: None,
                    value: Operand::String("x".to_string()),
                    writeback_place: None,
                }],
            ),
            "expected `trim()` to take no arguments",
        ),
        (
            module_with_main_member_call_result_type(
                "text",
                string_ty.clone(),
                string_value,
                Type::named("String"),
                "unknown",
                Vec::new(),
            ),
            "does not know runtime member `String.unknown`",
        ),
        (
            module_with_main_member_call_result_type(
                "values",
                vec_ty.clone(),
                vec_value.clone(),
                Type::named("bool"),
                "push",
                Vec::new(),
            ),
            "expected `push()` to receive one argument",
        ),
        (
            module_with_main_member_call_result_type(
                "values",
                vec_ty.clone(),
                vec_value.clone(),
                Type::named("int32"),
                "__index",
                vec![MirArg {
                    name: None,
                    value: Operand::Int(0),
                    writeback_place: None,
                }],
            ),
            "expected internal vector indexing",
        ),
        (
            module_with_main_member_call_result_type(
                "values",
                vec_ty.clone(),
                vec_value.clone(),
                Type::Unit,
                "__set_index",
                vec![MirArg {
                    name: None,
                    value: Operand::Int(0),
                    writeback_place: None,
                }],
            ),
            "expected internal indexed assignment",
        ),
        (
            module_with_main_member_call_result_type(
                "values",
                vec_ty.clone(),
                vec_value.clone(),
                Type::named("bool"),
                "swap",
                vec![MirArg {
                    name: None,
                    value: Operand::Int(0),
                    writeback_place: None,
                }],
            ),
            "expected `swap()` to receive two index arguments",
        ),
        (
            module_with_main_member_call_result_type(
                "values",
                vec_ty.clone(),
                vec_value.clone(),
                Type::named("bool"),
                "unknown",
                Vec::new(),
            ),
            "does not know runtime member `Vec.unknown`",
        ),
        (
            module_with_main_member_call_result_type(
                "counts",
                map_ty.clone(),
                map_value.clone(),
                Type::named("int32"),
                "__index",
                vec![MirArg {
                    name: None,
                    value: Operand::String("count".to_string()),
                    writeback_place: None,
                }],
            ),
            "expected internal map indexing",
        ),
        (
            module_with_main_member_call_result_type(
                "counts",
                map_ty.clone(),
                map_value.clone(),
                Type::Unit,
                "__set_index",
                vec![MirArg {
                    name: None,
                    value: Operand::String("count".to_string()),
                    writeback_place: None,
                }],
            ),
            "expected internal map indexed assignment",
        ),
        (
            module_with_main_member_call_result_type(
                "counts",
                map_ty.clone(),
                map_value.clone(),
                Type::named("bool"),
                "contains_key",
                Vec::new(),
            ),
            "expected `contains_key()` to receive one key argument",
        ),
        (
            module_with_main_member_call_result_type(
                "counts",
                map_ty.clone(),
                map_value.clone(),
                Type::Named("Vec".to_string(), vec![Type::named("String")]),
                "keys",
                vec![MirArg {
                    name: None,
                    value: Operand::Int(1),
                    writeback_place: None,
                }],
            ),
            "expected `keys()` to take no arguments",
        ),
        (
            module_with_main_member_call_result_type(
                "counts",
                map_ty.clone(),
                map_value.clone(),
                Type::Named("Vec".to_string(), vec![Type::named("int32")]),
                "entries",
                vec![MirArg {
                    name: None,
                    value: Operand::Int(1),
                    writeback_place: None,
                }],
            ),
            "expected `entries`() to take no arguments",
        ),
        (
            module_with_main_member_call_result_type(
                "counts",
                map_ty.clone(),
                map_value.clone(),
                Type::named("bool"),
                "unknown",
                Vec::new(),
            ),
            "does not know runtime member `Map.unknown`",
        ),
        (
            module_with_main_member_call_result_type(
                "seen",
                set_ty.clone(),
                set_value.clone(),
                Type::named("bool"),
                "contains",
                Vec::new(),
            ),
            "expected `contains()` to receive one value argument",
        ),
        (
            module_with_main_member_call_result_type(
                "seen",
                set_ty.clone(),
                set_value.clone(),
                Type::Named("Option".to_string(), vec![Type::named("String")]),
                "__index_option",
                Vec::new(),
            ),
            "expected internal optional set indexing",
        ),
        (
            module_with_main_member_call_result_type(
                "seen",
                set_ty.clone(),
                set_value.clone(),
                Type::named("bool"),
                "unknown",
                Vec::new(),
            ),
            "does not know runtime member `Set.unknown`",
        ),
        (
            module_with_main_member_call_result_type(
                "jobs",
                channel_ty.clone(),
                channel_value.clone(),
                Type::Named("Option".to_string(), vec![Type::named("int32")]),
                "get",
                vec![
                    MirArg {
                        name: None,
                        value: Operand::Int(1),
                        writeback_place: None,
                    },
                    MirArg {
                        name: None,
                        value: Operand::Int(2),
                        writeback_place: None,
                    },
                ],
            ),
            "expected `get()` or `get(timeout=...)`",
        ),
        (
            module_with_main_member_call_result_type(
                "jobs",
                channel_ty.clone(),
                channel_value.clone(),
                Type::Unit,
                "close",
                vec![MirArg {
                    name: None,
                    value: Operand::Int(1),
                    writeback_place: None,
                }],
            ),
            "expected `close()` to take no arguments",
        ),
        (
            module_with_main_member_call_result_type(
                "jobs",
                channel_ty.clone(),
                channel_value.clone(),
                Type::named("bool"),
                "unknown",
                Vec::new(),
            ),
            "does not know runtime member `Queue.unknown`",
        ),
        (
            module_with_main_member_call_result_type(
                "group",
                task_group_ty.clone(),
                task_group_value.clone(),
                Type::Unit,
                "cancel",
                vec![MirArg {
                    name: None,
                    value: Operand::Int(1),
                    writeback_place: None,
                }],
            ),
            "expected `cancel()` to take no arguments",
        ),
        (
            module_with_main_member_call_result_type(
                "group",
                task_group_ty.clone(),
                task_group_value.clone(),
                Type::Unit,
                "close",
                vec![MirArg {
                    name: None,
                    value: Operand::Int(1),
                    writeback_place: None,
                }],
            ),
            "expected `close()` to take no arguments",
        ),
        (
            module_with_main_member_call_result_type(
                "group",
                task_group_ty,
                task_group_value,
                Type::named("bool"),
                "unknown",
                Vec::new(),
            ),
            "does not know runtime member `TaskGroup.unknown`",
        ),
    ];

    for (module, expected) in cases {
        let error = emit_host_object(&module).expect_err("invalid runtime member call");
        assert!(
            error.contains(expected),
            "expected `{expected}` in `{error}`"
        );
    }
}

#[test]
fn direct_backend_manual_select_surface_compiles() {
    let channel_ty = Type::Named("Queue".to_string(), vec![Type::named("int32")]);
    let recv_binding_ty = Type::Named("Option".to_string(), vec![Type::named("int32")]);
    let send_binding_ty = Type::Named(
        "Result".to_string(),
        vec![
            Type::Unit,
            Type::Named("SendError".to_string(), vec![Type::named("int32")]),
        ],
    );

    let module = crate::mir::MirModule {
        functions: vec![MirFunction {
            name: "main".to_string(),
            module_name: "<test>".to_string(),
            span: Span::new(1, 1),
            receiver: None,
            params: Vec::new(),
            local_types: vec![
                MirLocalType {
                    name: "jobs".to_string(),
                    ty: channel_ty.clone(),
                },
                MirLocalType {
                    name: "received".to_string(),
                    ty: recv_binding_ty.clone(),
                },
                MirLocalType {
                    name: "sent".to_string(),
                    ty: send_binding_ty.clone(),
                },
            ],
            return_type: Type::named("int32"),
            entry: "entry".to_string(),
            blocks: vec![
                BasicBlock {
                    label: "entry".to_string(),
                    instructions: vec![Instruction::Assign {
                        target: "jobs".to_string(),
                        value: Rvalue::Call {
                            callee: CallTarget::Name("queue".to_string()),
                            args: Vec::new(),
                        },
                    }],
                    terminator: Terminator::Select {
                        arms: vec![
                            MirSelectArm {
                                binding: Some("received".to_string()),
                                kind: MirSelectKind::Recv {
                                    channel: Operand::Place("jobs".to_string()),
                                },
                                label: "recv".to_string(),
                            },
                            MirSelectArm {
                                binding: Some("sent".to_string()),
                                kind: MirSelectKind::Send {
                                    channel: Operand::Place("jobs".to_string()),
                                    value: Operand::Int(7),
                                },
                                label: "send".to_string(),
                            },
                        ],
                        otherwise: "otherwise".to_string(),
                    },
                },
                BasicBlock {
                    label: "recv".to_string(),
                    instructions: Vec::new(),
                    terminator: Terminator::Return(Operand::Int(0)),
                },
                BasicBlock {
                    label: "send".to_string(),
                    instructions: Vec::new(),
                    terminator: Terminator::Return(Operand::Int(0)),
                },
                BasicBlock {
                    label: "otherwise".to_string(),
                    instructions: Vec::new(),
                    terminator: Terminator::Return(Operand::Int(0)),
                },
            ],
        }],
        classes: Vec::new(),
        trait_impls: Vec::new(),
        top_level: None,
    };
    assert!(!emit_host_object(&module)
        .expect("manual recv/send select should compile directly")
        .is_empty());

    let timeout_module = crate::mir::MirModule {
        functions: vec![MirFunction {
            name: "main".to_string(),
            module_name: "<test>".to_string(),
            span: Span::new(1, 1),
            receiver: None,
            params: Vec::new(),
            local_types: vec![
                MirLocalType {
                    name: "jobs".to_string(),
                    ty: channel_ty,
                },
                MirLocalType {
                    name: "received".to_string(),
                    ty: recv_binding_ty,
                },
            ],
            return_type: Type::named("int32"),
            entry: "entry".to_string(),
            blocks: vec![
                BasicBlock {
                    label: "entry".to_string(),
                    instructions: vec![Instruction::Assign {
                        target: "jobs".to_string(),
                        value: Rvalue::Call {
                            callee: CallTarget::Name("queue".to_string()),
                            args: Vec::new(),
                        },
                    }],
                    terminator: Terminator::Select {
                        arms: vec![
                            MirSelectArm {
                                binding: Some("received".to_string()),
                                kind: MirSelectKind::Recv {
                                    channel: Operand::Place("jobs".to_string()),
                                },
                                label: "recv".to_string(),
                            },
                            MirSelectArm {
                                binding: None,
                                kind: MirSelectKind::After {
                                    duration: Operand::Duration(1),
                                },
                                label: "after".to_string(),
                            },
                        ],
                        otherwise: "otherwise".to_string(),
                    },
                },
                BasicBlock {
                    label: "recv".to_string(),
                    instructions: Vec::new(),
                    terminator: Terminator::Return(Operand::Int(0)),
                },
                BasicBlock {
                    label: "after".to_string(),
                    instructions: Vec::new(),
                    terminator: Terminator::Return(Operand::Int(0)),
                },
                BasicBlock {
                    label: "otherwise".to_string(),
                    instructions: Vec::new(),
                    terminator: Terminator::Return(Operand::Int(0)),
                },
            ],
        }],
        classes: Vec::new(),
        trait_impls: Vec::new(),
        top_level: None,
    };
    assert!(!emit_host_object(&timeout_module)
        .expect("manual recv/after select should compile directly")
        .is_empty());
}

#[test]
fn native_codegen_direct_error_paths_cover_missing_entry_wrapper_and_return_type_cases() {
    let source = r#"
class Counter:
    value: int32

    def current(borrow self) -> int32:
        return self.value

def helper(value: int32) -> int32:
    return value + 1

def main() -> int32:
    return helper(1)
"#;
    let mir = lower_source_to_mir(source).expect("source should lower to MIR");

    let method = mir
        .functions
        .iter()
        .find(|function| function.receiver.is_some())
        .cloned()
        .expect("method should be lowered as a function");
    let helper = mir
        .functions
        .iter()
        .find(|function| function.name == "helper")
        .cloned()
        .expect("helper function should exist");
    let main = mir
        .functions
        .iter()
        .find(|function| function.name == "main")
        .cloned()
        .expect("main function should exist");

    let mut method_codegen = NativeCodegen::new(&mir, "/tmp/native_codegen_errors.au", source)
        .expect("codegen should initialize");
    let thunk_error = method_codegen
        .define_function_thunk(&method)
        .expect_err("methods should still reject direct spawn thunks");
    assert!(thunk_error.contains("does not yet support spawn thunks for methods"));

    let mut broken_main = main.clone();
    broken_main.entry = "missing_block".to_string();
    let mut broken_entry_codegen =
        NativeCodegen::new(&mir, "/tmp/native_codegen_errors.au", source)
            .expect("codegen should initialize");
    let entry_error = broken_entry_codegen
        .define_function(&broken_main)
        .expect_err("missing entry blocks should be reported");
    assert!(entry_error.contains("could not find entry block `missing_block`"));

    let mut no_main_codegen = NativeCodegen::new(&mir, "/tmp/native_codegen_errors.au", source)
        .expect("codegen should initialize");
    no_main_codegen.functions.clear();
    let wrapper_error = no_main_codegen
        .define_main_wrapper()
        .expect_err("missing entrypoints should fail main wrapper generation");
    assert!(wrapper_error.contains("requires a `main` function or top-level script"));

    let mut missing_return_codegen =
        NativeCodegen::new(&mir, "/tmp/native_codegen_errors.au", source)
            .expect("codegen should initialize");
    missing_return_codegen
        .function_return_types
        .remove(&helper.name);
    let return_error = missing_return_codegen
        .define_function_thunk(&helper)
        .expect_err("missing thunk return types should fail");
    assert!(return_error.contains("does not know return type for `helper`"));
}

#[test]
fn direct_backend_builtin_call_surface_compiles_across_success_and_error_matrix() {
    let success_source = r#"
def main() -> int32:
    print(7)
    print(3.5)
    print(true)
    print(None)
    text = "  Aurora repo  "
    print(text)
    print(f"value={text}")
    jobs: Queue[int32] = queue()
    group = tasks()
    ready = cancelled()
    sleep(0ms)
    value = abs(-7)
    floor = min(1, 2)
    ceil = max(1, 2)
    root = sqrt(9.0)
    parsed32 = parse_int32("7")
    parsed64 = parse_int64("7")
    parsedf = parse_float64("7.0")
    values: Vec[int32] = Vec[int32]()
    names: Set[String] = Set[String]()
    counts: Map[String, int32] = Map[String, int32]()
    short = range(3)
    long = range(start=1, stop=4)
    print(ready)
    return value + floor + ceil + root as int32
"#;
    let success_mir =
        lower_source_to_mir(success_source).expect("builtin matrix source should lower");
    let object =
        emit_host_object(&success_mir).expect("builtin matrix source should emit direct code");
    assert!(!object.is_empty());

    let error_cases = [
        (
            "print missing arg",
            module_with_main_call(Rvalue::Call {
                callee: CallTarget::Name("print".to_string()),
                args: vec![],
            }),
            "expected `print` to receive one argument",
        ),
        (
            "channel extra arg",
            module_with_main_call(Rvalue::Call {
                callee: CallTarget::Name("queue".to_string()),
                args: vec![
                    MirArg {
                        name: None,
                        value: Operand::Int(1),
                        writeback_place: None,
                    },
                    MirArg {
                        name: None,
                        value: Operand::Int(2),
                        writeback_place: None,
                    },
                ],
            }),
            "expected `queue()` to take at most one capacity argument",
        ),
        (
            "tasks extra arg",
            module_with_main_call(Rvalue::Call {
                callee: CallTarget::Name("tasks".to_string()),
                args: vec![MirArg {
                    name: None,
                    value: Operand::Int(1),
                    writeback_place: None,
                }],
            }),
            "expected `tasks()` to take no arguments",
        ),
        (
            "cancelled extra arg",
            module_with_main_call(Rvalue::Call {
                callee: CallTarget::Name("cancelled".to_string()),
                args: vec![MirArg {
                    name: None,
                    value: Operand::Int(1),
                    writeback_place: None,
                }],
            }),
            "expected `cancelled()` to take no arguments",
        ),
        (
            "sleep missing arg",
            module_with_main_call(Rvalue::Call {
                callee: CallTarget::Name("sleep".to_string()),
                args: vec![],
            }),
            "expected `sleep()` to receive one duration argument",
        ),
        (
            "abs missing arg",
            module_with_main_call(Rvalue::Call {
                callee: CallTarget::Name("abs".to_string()),
                args: vec![],
            }),
            "expected `abs()` to receive one argument",
        ),
        (
            "parse_int32 missing arg",
            module_with_main_call(Rvalue::Call {
                callee: CallTarget::Name("parse_int32".to_string()),
                args: vec![],
            }),
            "expected `parse_int32`() to receive one string argument",
        ),
        (
            "min missing arg",
            module_with_main_call(Rvalue::Call {
                callee: CallTarget::Name("min".to_string()),
                args: vec![MirArg {
                    name: None,
                    value: Operand::Int(1),
                    writeback_place: None,
                }],
            }),
            "expected `min`() to receive two arguments",
        ),
        (
            "sqrt missing arg",
            module_with_main_call(Rvalue::Call {
                callee: CallTarget::Name("sqrt".to_string()),
                args: vec![],
            }),
            "expected `sqrt()` to receive one argument",
        ),
        (
            "Vec extra arg",
            module_with_main_call(Rvalue::Call {
                callee: CallTarget::Name("Vec".to_string()),
                args: vec![MirArg {
                    name: None,
                    value: Operand::Int(1),
                    writeback_place: None,
                }],
            }),
            "expected `Vec`() to take no arguments",
        ),
        (
            "range too many args",
            module_with_main_call(Rvalue::Call {
                callee: CallTarget::Name("range".to_string()),
                args: vec![
                    MirArg {
                        name: None,
                        value: Operand::Int(1),
                        writeback_place: None,
                    },
                    MirArg {
                        name: None,
                        value: Operand::Int(2),
                        writeback_place: None,
                    },
                    MirArg {
                        name: None,
                        value: Operand::Int(3),
                        writeback_place: None,
                    },
                ],
            }),
            "expected `range()` to receive one or two arguments",
        ),
        (
            "range bad named arg",
            module_with_main_call(Rvalue::Call {
                callee: CallTarget::Name("range".to_string()),
                args: vec![
                    MirArg {
                        name: Some("middle".to_string()),
                        value: Operand::Int(1),
                        writeback_place: None,
                    },
                    MirArg {
                        name: Some("stop".to_string()),
                        value: Operand::Int(3),
                        writeback_place: None,
                    },
                ],
            }),
            "does not recognize `range()` argument `middle`",
        ),
        (
            "range missing stop",
            module_with_main_call(Rvalue::Call {
                callee: CallTarget::Name("range".to_string()),
                args: vec![MirArg {
                    name: Some("start".to_string()),
                    value: Operand::Int(1),
                    writeback_place: None,
                }],
            }),
            "expected `range()` to receive a `stop` argument",
        ),
    ];

    for (label, module, expected) in error_cases {
        let error = emit_host_object(&module)
            .expect_err(&format!("{label} should be rejected by direct codegen"));
        assert!(
            error.contains(expected),
            "{label} reported `{error}` instead of containing `{expected}`"
        );
    }
}

#[test]
fn direct_backend_for_range_and_spawn_error_surface_reports_expected_diagnostics() {
    let invalid_for_range = crate::mir::MirModule {
        functions: vec![MirFunction {
            name: "main".to_string(),
            module_name: "<test>".to_string(),
            span: Span::new(1, 1),
            receiver: None,
            params: Vec::new(),
            local_types: vec![MirLocalType {
                name: "item".to_string(),
                ty: Type::named("int32"),
            }],
            return_type: Type::named("int32"),
            entry: "entry".to_string(),
            blocks: vec![
                BasicBlock {
                    label: "entry".to_string(),
                    instructions: Vec::new(),
                    terminator: Terminator::ForRange {
                        binding: "item".to_string(),
                        iterable: Operand::Int(0),
                        body_label: "body".to_string(),
                        exit_label: "exit".to_string(),
                    },
                },
                BasicBlock {
                    label: "body".to_string(),
                    instructions: Vec::new(),
                    terminator: Terminator::Goto("exit".to_string()),
                },
                BasicBlock {
                    label: "exit".to_string(),
                    instructions: Vec::new(),
                    terminator: Terminator::Return(Operand::Int(0)),
                },
            ],
        }],
        classes: Vec::new(),
        trait_impls: Vec::new(),
        top_level: None,
    };
    let for_range_error = emit_host_object(&invalid_for_range)
        .expect_err("non-place for-range iterables should be rejected");
    assert!(for_range_error.contains("requires `for range` iterables to live in a place"));

    let spawn_source = r#"
def worker(value: int32) -> int32:
    return value

def main() -> int32:
    return 0
"#;
    let mut spawn_mir = lower_source_to_mir(spawn_source).expect("spawn source should lower");
    let main = spawn_mir
        .functions
        .iter_mut()
        .find(|function| function.name == "main")
        .expect("main function should exist");
    main.blocks = vec![BasicBlock {
        label: "entry".to_string(),
        instructions: vec![Instruction::Assign {
            target: "%task".to_string(),
            value: Rvalue::Spawn {
                detached: false,
                task_group: None,
                function: "worker".to_string(),
                args: vec![MirArg {
                    name: None,
                    value: Operand::Int(1),
                    writeback_place: None,
                }],
            },
        }],
        terminator: Terminator::Return(Operand::Int(0)),
    }];
    main.local_types = vec![MirLocalType {
        name: "%task".to_string(),
        ty: Type::Named("Task".to_string(), vec![Type::named("int32")]),
    }];

    let main = spawn_mir
        .functions
        .iter()
        .find(|function| function.name == "main")
        .cloned()
        .expect("main function should exist");

    let mut missing_thunk_codegen = NativeCodegen::new(
        &spawn_mir,
        "/tmp/direct_spawn_missing_thunk.au",
        spawn_source,
    )
    .expect("codegen should initialize");
    missing_thunk_codegen.function_thunks.remove("worker");
    let missing_thunk_error = missing_thunk_codegen
        .define_function(&main)
        .expect_err("spawn should reject missing thunks");
    assert!(missing_thunk_error.contains("does not know spawn thunk for `worker`"));

    let mut missing_return_codegen = NativeCodegen::new(
        &spawn_mir,
        "/tmp/direct_spawn_missing_return.au",
        spawn_source,
    )
    .expect("codegen should initialize");
    missing_return_codegen
        .function_return_types
        .remove("worker");
    let missing_return_error = missing_return_codegen
        .define_function(&main)
        .expect_err("spawn should reject missing task return metadata");
    assert!(missing_return_error.contains("does not know return type for `worker`"));

    let mut borrowed_spawn_mir = spawn_mir.clone();
    let borrowed_main_mut = borrowed_spawn_mir
        .functions
        .iter_mut()
        .find(|function| function.name == "main")
        .expect("main function should exist");
    borrowed_main_mut.blocks[0].instructions[0] = Instruction::Assign {
        target: "%task".to_string(),
        value: Rvalue::Spawn {
            detached: false,
            task_group: None,
            function: "worker".to_string(),
            args: vec![MirArg {
                name: None,
                value: Operand::Int(1),
                writeback_place: Some("value".to_string()),
            }],
        },
    };
    let borrowed_main = borrowed_spawn_mir
        .functions
        .iter()
        .find(|function| function.name == "main")
        .cloned()
        .expect("main function should exist");
    let mut borrowed_spawn_codegen = NativeCodegen::new(
        &borrowed_spawn_mir,
        "/tmp/direct_spawn_borrowed_arg.au",
        spawn_source,
    )
    .expect("codegen should initialize");
    let borrowed_error = borrowed_spawn_codegen
        .define_function(&borrowed_main)
        .expect_err("spawn should reject borrowed arguments");
    assert!(borrowed_error.contains("does not yet support borrowed spawn arguments"));
}

#[test]
fn direct_backend_emits_object_for_member_call_surface_matrix() {
    let source = r#"
trait Named:
    def tag(self) -> String

impl Named for int32:
    def tag(self) -> String:
        return "number"

class Counter:
    value: int32

    def read(borrow self) -> int32:
        return self.value

    def bump(borrow mut self):
        self.value += 1

def worker(value: int32) -> int32:
    return value + 1

def main() -> int32:
    text = "  Aurora repo  "
    cloned = text.clone()
    length = text.len()
    has_repo = text.contains("repo")
    starts = text.starts_with("  Au")
    ends = text.ends_with("  ")
    parts = text.split(" ")
    replaced = text.replace("repo", "lang")
    lowered = text.to_lower()
    uppered = text.to_upper()
    stripped_prefix = text.strip_prefix("  ")
    stripped_suffix = text.strip_suffix("  ")
    trimmed = text.trim()
    joined = ", ".join(parts)

    value = 7
    label = value.to_string()
    tagged = value.tag()
    root = 9.0.sqrt()

    mut values = [1, 2, 3]
    empty = values.is_empty()
    length2 = values.len()
    values.push(4)
    popped = values.pop()
    first = values.get(0)
    direct = values[0]
    previous = values.set(0, 9)
    removed = values.remove(0)
    swapped = values.swap(0, 1)
    contains = values.contains(2)
    inserted = values.insert(0, 5)
    values.reverse()
    other_values = [8, 9]
    values.extend(other_values)
    values.clear()

    mut counts = {"a": 1, "b": 2}
    map_empty = counts.is_empty()
    map_len = counts.len()
    current = counts.get("a")
    direct_count = counts["a"]
    previous_count = counts.set("a", 3)
    removed_count = counts.remove("b")
    has_key = counts.contains_key("a")
    keys = counts.keys()
    vals = counts.values()
    entries = counts.entries()
    items = counts.items()
    counts.extend({"c": 4})
    counts.clear()

    mut names = Set[String]()
    set_empty = names.is_empty()
    names.insert("aurora")
    names.insert("repo")
    set_len = names.len()
    has_name = names.contains("aurora")
    removed_name = names.remove("repo")

    jobs: Queue[int32] = queue()
    send_result = jobs.put(1)
    recv_result = jobs.get()
    jobs.close()

    group = tasks()
    group.cancel()

    task = spawn worker(value=1)
    joined_task = task.result()

    mut counter = Counter(value=1)
    current_value = counter.read()
    counter.bump()
    latest = counter.value

    print(cloned)
    print(length)
    print(has_repo)
    print(starts)
    print(ends)
    print(replaced)
    print(lowered)
    print(uppered)
    print(joined)
    print(label)
    print(tagged)
    print(root)
    print(empty)
    print(length2)
    print(contains)
    print(inserted)
    print(map_empty)
    print(map_len)
    print(has_key)
    print(set_empty)
    print(set_len)
    print(has_name)
    print(current_value)
    print(latest)
    print(joined_task)
    return direct + direct_count
"#;
    let mir = lower_source_to_mir(source).expect("member-call matrix source should lower");
    let object = emit_host_object(&mir).expect("member-call matrix should emit direct code");
    assert!(!object.is_empty());
}

#[test]
fn direct_backend_member_call_error_surface_reports_expected_diagnostics() {
    let one_arg = vec![MirArg {
        name: None,
        value: Operand::Int(1),
        writeback_place: None,
    }];
    let two_args = vec![
        MirArg {
            name: None,
            value: Operand::Int(1),
            writeback_place: None,
        },
        MirArg {
            name: None,
            value: Operand::Int(2),
            writeback_place: None,
        },
    ];
    let string_object = || Rvalue::Use(Operand::String("aurora".to_string()));
    let vec_object = || Rvalue::VecLiteral {
        element_type: Type::named("int32"),
        elements: vec![Operand::Int(1), Operand::Int(2)],
    };
    let map_object = || Rvalue::MapLiteral {
        key_type: Type::named("String"),
        value_type: Type::named("int32"),
        entries: vec![MirMapEntry {
            key: Operand::String("a".to_string()),
            value: Operand::Int(1),
        }],
    };
    let set_object = || Rvalue::SetLiteral {
        element_type: Type::named("String"),
        elements: vec![Operand::String("aurora".to_string())],
    };
    let channel_object = || Rvalue::Call {
        callee: CallTarget::Name("queue".to_string()),
        args: vec![],
    };

    let error_cases = [
        (
            "float sqrt extra arg",
            module_with_main_member_call(
                "value",
                Type::named("float64"),
                Rvalue::Use(Operand::Float(9.0)),
                "sqrt",
                one_arg.clone(),
            ),
            "expected `sqrt()` to take no arguments",
        ),
        (
            "string clone extra arg",
            module_with_main_member_call(
                "text",
                Type::named("String"),
                string_object(),
                "clone",
                one_arg.clone(),
            ),
            "expected `clone()` to take no arguments",
        ),
        (
            "scalar to_string extra arg",
            module_with_main_member_call(
                "value",
                Type::named("int32"),
                Rvalue::Use(Operand::Int(7)),
                "to_string",
                one_arg.clone(),
            ),
            "expected `to_string()` to take no arguments",
        ),
        (
            "string len extra arg",
            module_with_main_member_call(
                "text",
                Type::named("String"),
                string_object(),
                "len",
                one_arg.clone(),
            ),
            "expected `len()` to take no arguments",
        ),
        (
            "string contains missing arg",
            module_with_main_member_call(
                "text",
                Type::named("String"),
                string_object(),
                "contains",
                vec![],
            ),
            "expected `contains`() to receive one string argument",
        ),
        (
            "vec len extra arg",
            module_with_main_member_call(
                "values",
                Type::Named("Vec".to_string(), vec![Type::named("int32")]),
                vec_object(),
                "len",
                one_arg.clone(),
            ),
            "expected `len()` to take no arguments",
        ),
        (
            "vec push missing arg",
            module_with_main_member_call(
                "values",
                Type::Named("Vec".to_string(), vec![Type::named("int32")]),
                vec_object(),
                "push",
                vec![],
            ),
            "expected `push()` to receive one argument",
        ),
        (
            "vec clear extra arg",
            module_with_main_member_call(
                "values",
                Type::Named("Vec".to_string(), vec![Type::named("int32")]),
                vec_object(),
                "clear",
                one_arg.clone(),
            ),
            "expected `clear()` to take no arguments",
        ),
        (
            "map len extra arg",
            module_with_main_member_call(
                "counts",
                Type::Named(
                    "Map".to_string(),
                    vec![Type::named("String"), Type::named("int32")],
                ),
                map_object(),
                "len",
                one_arg.clone(),
            ),
            "expected `len()` to take no arguments",
        ),
        (
            "map set missing arg",
            module_with_main_member_call(
                "counts",
                Type::Named(
                    "Map".to_string(),
                    vec![Type::named("String"), Type::named("int32")],
                ),
                map_object(),
                "set",
                one_arg.clone(),
            ),
            "expected `set()` to receive key and value",
        ),
        (
            "set contains missing arg",
            module_with_main_member_call(
                "names",
                Type::Named("Set".to_string(), vec![Type::named("String")]),
                set_object(),
                "contains",
                vec![],
            ),
            "expected `contains()` to receive one value argument",
        ),
        (
            "queue get extra arg",
            module_with_main_member_call(
                "jobs",
                Type::Named("Queue".to_string(), vec![Type::named("int32")]),
                channel_object(),
                "get",
                two_args.clone(),
            ),
            "expected `get()` or `get(timeout=...)`",
        ),
        (
            "queue put missing arg",
            module_with_main_member_call(
                "jobs",
                Type::Named("Queue".to_string(), vec![Type::named("int32")]),
                channel_object(),
                "put",
                vec![],
            ),
            "expected `put()` to receive one argument",
        ),
        (
            "vec swap missing arg",
            module_with_main_member_call(
                "values",
                Type::Named("Vec".to_string(), vec![Type::named("int32")]),
                vec_object(),
                "swap",
                one_arg.clone(),
            ),
            "expected `swap()` to receive two index arguments",
        ),
        (
            "map extend missing arg",
            module_with_main_member_call(
                "counts",
                Type::Named(
                    "Map".to_string(),
                    vec![Type::named("String"), Type::named("int32")],
                ),
                map_object(),
                "extend",
                vec![],
            ),
            "expected `extend()` to receive one map argument",
        ),
        (
            "set remove missing arg",
            module_with_main_member_call(
                "names",
                Type::Named("Set".to_string(), vec![Type::named("String")]),
                set_object(),
                "remove",
                vec![],
            ),
            "expected `remove()` to receive one value argument",
        ),
        (
            "unknown runtime member",
            module_with_main_member_call(
                "text",
                Type::named("String"),
                string_object(),
                "missing",
                vec![],
            ),
            "does not know runtime member `String.missing`",
        ),
        (
            "unknown scalar member",
            module_with_main_member_call(
                "value",
                Type::named("int32"),
                Rvalue::Use(Operand::Int(7)),
                "missing",
                vec![],
            ),
            "does not support member call `.missing` on `int32`",
        ),
        (
            "string replace missing arg",
            module_with_main_member_call(
                "text",
                Type::named("String"),
                string_object(),
                "replace",
                one_arg.clone(),
            ),
            "expected `replace()` to receive `from` and `to` string arguments",
        ),
        (
            "vec insert missing arg",
            module_with_main_member_call(
                "values",
                Type::Named("Vec".to_string(), vec![Type::named("int32")]),
                vec_object(),
                "insert",
                one_arg.clone(),
            ),
            "expected `insert()` to receive index and value",
        ),
        (
            "map items extra arg",
            module_with_main_member_call(
                "counts",
                Type::Named(
                    "Map".to_string(),
                    vec![Type::named("String"), Type::named("int32")],
                ),
                map_object(),
                "items",
                one_arg.clone(),
            ),
            "expected `items`() to take no arguments",
        ),
        (
            "set len extra arg",
            module_with_main_member_call(
                "names",
                Type::Named("Set".to_string(), vec![Type::named("String")]),
                set_object(),
                "len",
                one_arg,
            ),
            "expected `len()` to take no arguments",
        ),
        (
            "string join missing arg",
            module_with_main_member_call(
                "text",
                Type::named("String"),
                string_object(),
                "join",
                vec![],
            ),
            "expected `join()` to receive one vector argument",
        ),
        (
            "string trim extra arg",
            module_with_main_member_call(
                "text",
                Type::named("String"),
                string_object(),
                "trim",
                two_args[..1].to_vec(),
            ),
            "expected `trim()` to take no arguments",
        ),
    ];

    for (label, module, expected) in error_cases {
        let error = emit_host_object(&module)
            .expect_err(&format!("{label} should be rejected by direct codegen"));
        assert!(
            error.contains(expected),
            "{label} reported `{error}` instead of containing `{expected}`"
        );
    }
}

#[test]
fn direct_backend_operand_and_construct_error_surface_reports_expected_diagnostics() {
    let large_int_module = module_with_main_call(Rvalue::Use(Operand::Int((i64::MAX as u128) + 1)));
    let large_int_object =
        emit_host_object(&large_int_module).expect("large integer operands should box");
    assert!(!large_int_object.is_empty());

    let large_duration_module =
        module_with_main_call(Rvalue::Use(Operand::Duration((i64::MAX as i128) + 1)));
    let duration_error = emit_host_object(&large_duration_module)
        .expect_err("oversized duration literals should be rejected");
    assert!(duration_error.contains("duration constants that fit in host i64"));

    let missing_place_module =
        module_with_main_call(Rvalue::Use(Operand::Place("missing".to_string())));
    let missing_place_error =
        emit_host_object(&missing_place_module).expect_err("unknown locals should be rejected");
    assert!(missing_place_error.contains("does not know local `missing`"));

    let empty_place_module = module_with_main_call(Rvalue::Use(Operand::Place(String::new())));
    let empty_place_error =
        emit_host_object(&empty_place_module).expect_err("empty places should be rejected");
    assert!(empty_place_error.contains("does not know local"));

    let pair_class = crate::mir::MirClass {
        name: "Pair".to_string(),
        type_params: Vec::new(),
        fields: vec![
            crate::mir::MirClassField {
                name: "left".to_string(),
                ty: Type::named("int32"),
            },
            crate::mir::MirClassField {
                name: "right".to_string(),
                ty: Type::named("int32"),
            },
        ],
        methods: Vec::new(),
    };
    let missing_field_module = crate::mir::MirModule {
        functions: vec![MirFunction {
            name: "main".to_string(),
            module_name: "<test>".to_string(),
            span: Span::new(1, 1),
            receiver: None,
            params: Vec::new(),
            local_types: Vec::new(),
            return_type: Type::named("int32"),
            entry: "entry".to_string(),
            blocks: vec![BasicBlock {
                label: "entry".to_string(),
                instructions: vec![Instruction::Assign {
                    target: "%t0".to_string(),
                    value: Rvalue::Construct {
                        class_name: "Pair".to_string(),
                        fields: vec![crate::mir::MirFieldInit {
                            name: "left".to_string(),
                            value: Operand::Int(1),
                        }],
                    },
                }],
                terminator: Terminator::Return(Operand::Int(0)),
            }],
        }],
        classes: vec![pair_class.clone()],
        trait_impls: Vec::new(),
        top_level: None,
    };
    let missing_field_error = emit_host_object(&missing_field_module)
        .expect_err("plain-class construction should require all fields");
    assert!(missing_field_error.contains("construction for `Pair` is missing field `right`"));

    let non_class_construct_module = module_with_main_call(Rvalue::Construct {
        class_name: "int32".to_string(),
        fields: Vec::new(),
    });
    let non_class_construct_error = emit_host_object(&non_class_construct_module)
        .expect_err("constructing scalar types should be rejected");
    assert!(non_class_construct_error.contains("could not construct non-class type `int32`"));

    let missing_field_access_module = crate::mir::MirModule {
        functions: vec![MirFunction {
            name: "main".to_string(),
            module_name: "<test>".to_string(),
            span: Span::new(1, 1),
            receiver: None,
            params: Vec::new(),
            local_types: vec![
                MirLocalType {
                    name: "pair".to_string(),
                    ty: Type::named("Pair"),
                },
                MirLocalType {
                    name: "%t0".to_string(),
                    ty: Type::named("int32"),
                },
            ],
            return_type: Type::named("int32"),
            entry: "entry".to_string(),
            blocks: vec![BasicBlock {
                label: "entry".to_string(),
                instructions: vec![
                    Instruction::Assign {
                        target: "pair".to_string(),
                        value: Rvalue::Construct {
                            class_name: "Pair".to_string(),
                            fields: vec![
                                crate::mir::MirFieldInit {
                                    name: "left".to_string(),
                                    value: Operand::Int(1),
                                },
                                crate::mir::MirFieldInit {
                                    name: "right".to_string(),
                                    value: Operand::Int(2),
                                },
                            ],
                        },
                    },
                    Instruction::Assign {
                        target: "%t0".to_string(),
                        value: Rvalue::Use(Operand::Place("pair.missing".to_string())),
                    },
                ],
                terminator: Terminator::Return(Operand::Int(0)),
            }],
        }],
        classes: vec![pair_class],
        trait_impls: Vec::new(),
        top_level: None,
    };
    let missing_field_access_error = emit_host_object(&missing_field_access_module)
        .expect_err("unknown plain-class fields should be rejected");
    assert!(missing_field_access_error.contains("does not know field `missing`"));
}

#[test]
fn native_codegen_reports_invalid_non_boolean_branch_conditions() {
    let invalid_module = crate::mir::MirModule {
        functions: vec![MirFunction {
            name: "main".to_string(),
            module_name: "<test>".to_string(),
            span: Span::new(1, 1),
            receiver: None,
            params: Vec::new(),
            local_types: vec![MirLocalType {
                name: "%cond".to_string(),
                ty: Type::named("float64"),
            }],
            return_type: Type::named("int32"),
            entry: "entry".to_string(),
            blocks: vec![
                BasicBlock {
                    label: "entry".to_string(),
                    instructions: vec![Instruction::Assign {
                        target: "%cond".to_string(),
                        value: Rvalue::Use(Operand::Float(1.25)),
                    }],
                    terminator: Terminator::Branch {
                        condition: Operand::Place("%cond".to_string()),
                        then_label: "then".to_string(),
                        else_label: "else".to_string(),
                    },
                },
                BasicBlock {
                    label: "then".to_string(),
                    instructions: Vec::new(),
                    terminator: Terminator::Return(Operand::Int(1)),
                },
                BasicBlock {
                    label: "else".to_string(),
                    instructions: Vec::new(),
                    terminator: Terminator::Return(Operand::Int(0)),
                },
            ],
        }],
        classes: Vec::new(),
        trait_impls: Vec::new(),
        top_level: None,
    };

    let error = emit_host_object(&invalid_module)
        .expect_err("non-boolean branch conditions should be rejected by direct codegen");
    assert!(error.contains("cannot use `float64` as a branch condition"));
}

#[test]
fn native_codegen_thunk_helpers_cover_roundtrip_paths() {
    let source =
        "class Pair:\n    left: int32\n    right: bool\n\ndef main() -> int32:\n    return 0\n";
    let mir = lower_source_to_mir(source).expect("thunk helper source should lower");
    let mut codegen = NativeCodegen::new(&mir, "/tmp/thunk_helpers.au", source)
        .expect("codegen should initialize");

    let mut ctx = Context::new();
    ctx.func.signature = cranelift_codegen::ir::Signature::new(codegen.call_conv);
    let mut builder_ctx = FunctionBuilderContext::new();
    let mut builder = cranelift_frontend::FunctionBuilder::new(&mut ctx.func, &mut builder_ctx);
    let block = builder.create_block();
    builder.switch_to_block(block);
    builder.seal_block(block);

    let (_first_ptr, _first_len) = thunk_string_constant(&mut codegen, &mut builder, b"aurora")
        .expect("first string constant should lower");
    let (_second_ptr, _second_len) = thunk_string_constant(&mut codegen, &mut builder, b"aurora")
        .expect("duplicate string constant should reuse existing data");
    assert_eq!(codegen.string_data.len(), 1);

    let opaque_raw = builder.ins().iconst(types::I64, 7);
    let int_raw = builder.ins().iconst(types::I64, 11);
    let bool_raw = builder.ins().iconst(types::I64, 1);
    let float_raw = builder.ins().f64const(3.5);

    let opaque_boxed = box_thunk_value(
        &mut codegen,
        &mut builder,
        &[opaque_raw],
        &DirectType::Opaque(Type::named("String")),
    )
    .expect("opaque thunk values should pass through");
    let opaque_unboxed = unbox_thunk_value(
        &mut codegen,
        &mut builder,
        opaque_boxed,
        &DirectType::Opaque(Type::named("String")),
    )
    .expect("opaque thunk values should unbox directly");
    assert_eq!(opaque_unboxed.len(), 1);

    let boxed_int = box_thunk_value(
        &mut codegen,
        &mut builder,
        &[int_raw],
        &DirectType::Scalar(ScalarKind::Int32),
    )
    .expect("int thunk values should box");
    let unboxed_int = unbox_thunk_value(
        &mut codegen,
        &mut builder,
        boxed_int,
        &DirectType::Scalar(ScalarKind::Int32),
    )
    .expect("int thunk values should unbox");
    assert_eq!(unboxed_int.len(), 1);

    let boxed_float = box_thunk_value(
        &mut codegen,
        &mut builder,
        &[float_raw],
        &DirectType::Scalar(ScalarKind::Float64),
    )
    .expect("float thunk values should box");
    let unboxed_float = unbox_thunk_value(
        &mut codegen,
        &mut builder,
        boxed_float,
        &DirectType::Scalar(ScalarKind::Float64),
    )
    .expect("float thunk values should unbox");
    assert_eq!(unboxed_float.len(), 1);

    let boxed_bool = box_thunk_value(
        &mut codegen,
        &mut builder,
        &[bool_raw],
        &DirectType::Scalar(ScalarKind::Bool),
    )
    .expect("bool thunk values should box");
    let unboxed_bool = unbox_thunk_value(
        &mut codegen,
        &mut builder,
        boxed_bool,
        &DirectType::Scalar(ScalarKind::Bool),
    )
    .expect("bool thunk values should unbox");
    assert_eq!(unboxed_bool.len(), 1);

    let boxed_unit = box_thunk_value(
        &mut codegen,
        &mut builder,
        &[],
        &DirectType::Scalar(ScalarKind::Unit),
    )
    .expect("unit thunk values should box");
    let unboxed_unit = unbox_thunk_value(
        &mut codegen,
        &mut builder,
        boxed_unit,
        &DirectType::Scalar(ScalarKind::Unit),
    )
    .expect("unit thunk values should unbox");
    assert_eq!(unboxed_unit.len(), 1);
    let opaque_missing = box_thunk_value(
        &mut codegen,
        &mut builder,
        &[],
        &DirectType::Opaque(Type::named("String")),
    )
    .expect_err("opaque thunk boxing should require a raw value");
    assert!(opaque_missing.contains("spawn thunk expected an opaque value"));

    let pair_ty = DirectType::PlainClass(PlainClassType {
        class_name: "Pair".to_string(),
        fields: vec![
            PlainClassField {
                name: "left".to_string(),
                ty: DirectType::Scalar(ScalarKind::Int32),
            },
            PlainClassField {
                name: "right".to_string(),
                ty: DirectType::Scalar(ScalarKind::Bool),
            },
        ],
    });
    let boxed_pair = box_thunk_value(&mut codegen, &mut builder, &[int_raw, bool_raw], &pair_ty)
        .expect("plain class thunk values should box recursively");
    let unboxed_pair = unbox_thunk_value(&mut codegen, &mut builder, boxed_pair, &pair_ty)
        .expect("plain class thunk values should unbox recursively");
    assert_eq!(unboxed_pair.len(), 2);
    assert!(codegen.string_data.len() >= 3);

    builder.ins().return_(&[]);
    builder.finalize();
}

#[test]
fn direct_backend_emits_object_for_module_examples() {
    let repo_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("compiler crate should live under repo root")
        .parent()
        .expect("compiler crate should live under repo root")
        .to_path_buf();
    let examples = [
        repo_root.join("examples/modules/namespace_import_types.au"),
        repo_root.join("examples/modules/trait_impl_imports.au"),
    ];

    for path in examples {
        let mir = lower_path_to_mir(&path).expect("module example should lower to MIR");
        let object = emit_host_object(&mir).expect("module example should emit direct object");
        assert!(!object.is_empty(), "{}", path.display());
    }
}

#[test]
fn direct_backend_emits_object_for_broad_maintained_example_surface() {
    let repo_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("compiler crate should live under repo root")
        .parent()
        .expect("compiler crate should live under repo root")
        .to_path_buf();
    let paths = [
        "examples/basics/top_level_script.au",
        "examples/basics/main_function.au",
        "examples/basics/mutable_bindings.au",
        "examples/basics/default_arguments.au",
        "examples/basics/pass_keyword.au",
        "examples/basics/borrow_parameters.au",
        "examples/basics/named_arguments.au",
        "examples/basics/named_builtin_arguments.au",
        "examples/basics/none_values.au",
        "examples/basics/simple_example.au",
        "examples/classes/point_distance.au",
        "examples/classes/default_fields.au",
        "examples/classes/methods.au",
        "examples/classes/copy_class.au",
        "examples/classes/indirect_recursive.au",
        "examples/classes/mutating_methods.au",
        "examples/control_flow/if_elif_else.au",
        "examples/control_flow/for_range.au",
        "examples/control_flow/while_break_continue.au",
        "examples/control_flow/boolean_logic.au",
        "examples/enums/result_match.au",
        "examples/enums/result_option.au",
        "examples/enums/explicit_type_args.au",
        "examples/enums/match_borrow.au",
        "examples/error_handling/try_result.au",
        "examples/generics/box_and_wrapper.au",
        "examples/generics/generic_constructor_specialization.au",
        "examples/traits/greeter.au",
        "examples/traits/multiple_bounds.au",
        "examples/traits/generic_trait_impl.au",
        "examples/traits/specialized_trait_dispatch.au",
        "examples/traits/trait_associated_factory.au",
        "examples/traits/operator_traits.au",
        "examples/traits/ordering_traits.au",
        "examples/basics/borrowed_lifetime_labels.au",
        "examples/traits/generic_trait_bounds.au",
        "examples/numbers/float_sqrt.au",
        "examples/numbers/float32_values.au",
        "examples/numbers/numeric_casts.au",
        "examples/numbers/uint128_values.au",
        "examples/numbers/unary_minus.au",
        "examples/strings/string_clone.au",
        "examples/strings/f_strings.au",
        "examples/strings/borrow_str.au",
        "examples/strings/string_methods.au",
        "examples/strings/string_parsing_and_formatting.au",
        "examples/concurrency/queues_spawn.au",
        "examples/concurrency/queue_iteration.au",
        "examples/concurrency/select_send.au",
        "examples/concurrency/select_timeout_named.au",
        "examples/concurrency/spawn_detached.au",
        "examples/concurrency/task_group_cancel.au",
        "examples/concurrency/task_group_select.au",
        "examples/resources/with_resource.au",
        "examples/modules/namespace_import_types.au",
        "examples/modules/trait_impl_imports.au",
        "examples/basic_addition.au",
        "examples/control_flow.au",
        "examples/point.au",
        "examples/simple_addition.au",
        "examples/top_level_addition.au",
    ];

    for relative in paths {
        let path = repo_root.join(relative);
        let mir = lower_path_to_mir(&path).expect("maintained example should lower to MIR");
        let object = emit_host_object(&mir).expect("maintained example should emit direct object");
        assert!(!object.is_empty(), "{}", path.display());
    }
}

fn assert_direct_backend_emits_object_for_scratch_repro(relative: &str) {
    let repo_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("compiler crate should live under repo root")
        .parent()
        .expect("compiler crate should live under repo root")
        .to_path_buf();
    let path = repo_root.join(relative);
    let mir = lower_path_to_mir(&path).expect("scratch repro should lower to MIR");
    let object = emit_host_object(&mir).expect("scratch repro should emit direct object");
    assert!(!object.is_empty(), "{}", path.display());
}

#[test]
fn direct_backend_emits_object_for_generic_trait_bound_returning_int_repro() {
    assert_direct_backend_emits_object_for_scratch_repro(
        "test_edge/gt_26_generic_fn_with_trait_bound_returns_int.au",
    );
}

#[test]
fn direct_backend_emits_object_for_multi_param_trait_bound_repro() {
    assert_direct_backend_emits_object_for_scratch_repro(
        "test_edge/gt_49_trait_bound_with_multiple_params.au",
    );
}

#[test]
fn direct_backend_emits_object_for_generic_sort_trait_bound_repro() {
    assert_direct_backend_emits_object_for_scratch_repro(
        "test_edge/test_complex_15_generic_sort.au",
    );
}

#[test]
fn direct_backend_emits_object_for_multiple_trait_methods_repro() {
    assert_direct_backend_emits_object_for_scratch_repro(
        "test_edge/test_trait_multiple_methods.au",
    );
}

#[test]
fn mangle_symbol_rewrites_non_alphanumeric_characters() {
    assert_eq!(mangle_symbol("main"), "aurora_fn_main");
    assert_eq!(
        mangle_symbol("helpers.math.double"),
        "aurora_fn_helpers_math_double"
    );
}

#[test]
fn direct_type_supports_plain_classes_and_scalars() {
    let source = include_str!("../../../examples/classes/methods.au");
    let mir = lower_source_to_mir(source).expect("methods example should lower");
    let classes = mir
        .classes
        .iter()
        .cloned()
        .map(|class| (class.name.clone(), class))
        .collect::<HashMap<_, _>>();

    assert_eq!(
        scalar_kind_for_tests(&Type::named("int32")),
        Some(ScalarKind::Int32)
    );
    assert_eq!(
        scalar_kind_for_tests(&Type::named("float64")),
        Some(ScalarKind::Float64)
    );
    assert_eq!(
        scalar_kind_for_tests(&Type::named("bool")),
        Some(ScalarKind::Bool)
    );
    assert_eq!(scalar_kind_for_tests(&Type::Unit), Some(ScalarKind::Unit));

    let counter = direct_type(&Type::named("Counter"), &classes).expect("Counter should be direct");
    assert_eq!(render_direct_type(&counter), "Counter");
    assert_eq!(counter.value_count(), 1);
}

#[test]
fn infer_operand_and_rvalue_types_track_plain_classes() {
    let mut variable_types = HashMap::new();
    variable_types.insert("flag".to_string(), DirectType::Scalar(ScalarKind::Bool));
    variable_types.insert("number".to_string(), DirectType::Scalar(ScalarKind::Int32));
    variable_types.insert(
        "point".to_string(),
        DirectType::PlainClass(super::PlainClassType {
            class_name: "Point".to_string(),
            fields: vec![
                super::PlainClassField {
                    name: "x".to_string(),
                    ty: DirectType::Scalar(ScalarKind::Float64),
                },
                super::PlainClassField {
                    name: "y".to_string(),
                    ty: DirectType::Scalar(ScalarKind::Float64),
                },
            ],
        }),
    );
    let mut returns = HashMap::new();
    returns.insert(
        "helper".to_string(),
        DirectType::Scalar(ScalarKind::Float64),
    );
    let classes = HashMap::new();

    assert_eq!(
        infer_operand_type(
            &Operand::Place("flag".to_string()),
            &variable_types,
            &HashMap::new()
        ),
        Some(DirectType::Scalar(ScalarKind::Bool))
    );
    assert_eq!(
        infer_operand_type(
            &Operand::Place("point.x".to_string()),
            &variable_types,
            &HashMap::new()
        ),
        Some(DirectType::Scalar(ScalarKind::Float64))
    );
    assert_eq!(
        infer_rvalue_type(
            &Rvalue::Unary {
                op: UnaryOp::Not,
                value: Operand::Place("flag".to_string()),
                span: Span::new(1, 1),
            },
            &variable_types,
            &returns,
            &classes,
        ),
        Some(DirectType::Scalar(ScalarKind::Bool))
    );
    assert_eq!(
        infer_rvalue_type(
            &Rvalue::Binary {
                op: BinaryOp::Add,
                left: Operand::Place("number".to_string()),
                right: Operand::Int(2),
                span: Span::new(1, 1),
            },
            &variable_types,
            &returns,
            &classes,
        ),
        Some(DirectType::Scalar(ScalarKind::Int32))
    );
    assert_eq!(
        infer_rvalue_type(
            &Rvalue::Call {
                callee: CallTarget::Name("print".to_string()),
                args: vec![MirArg {
                    name: None,
                    value: Operand::Bool(true),
                    writeback_place: None,
                }],
            },
            &variable_types,
            &returns,
            &classes,
        ),
        Some(DirectType::Scalar(ScalarKind::Unit))
    );
    assert_eq!(
        infer_rvalue_type(
            &Rvalue::FormatString {
                parts: vec![MirFormatPart::Literal("hello".to_string())],
            },
            &variable_types,
            &returns,
            &classes,
        ),
        Some(DirectType::Opaque(Type::named("String")))
    );
    assert_eq!(
        infer_rvalue_type(
            &Rvalue::Cast {
                value: Operand::Place("number".to_string()),
                ty: Type::named("float64"),
                span: Span::new(1, 1),
            },
            &variable_types,
            &returns,
            &classes,
        ),
        Some(DirectType::Scalar(ScalarKind::Float64))
    );
    assert_eq!(
        infer_rvalue_type(
            &Rvalue::VecLiteral {
                element_type: Type::named("int32"),
                elements: vec![Operand::Int(1), Operand::Int(2)],
            },
            &variable_types,
            &returns,
            &classes,
        ),
        Some(DirectType::Opaque(Type::Named(
            "Vec".to_string(),
            vec![Type::named("int32")],
        )))
    );
    assert_eq!(
        infer_rvalue_type(
            &Rvalue::MapLiteral {
                key_type: Type::named("String"),
                value_type: Type::named("int32"),
                entries: vec![MirMapEntry {
                    key: Operand::String("count".to_string()),
                    value: Operand::Int(1),
                }],
            },
            &variable_types,
            &returns,
            &classes,
        ),
        Some(DirectType::Opaque(Type::Named(
            "Map".to_string(),
            vec![Type::named("String"), Type::named("int32")],
        )))
    );
    assert_eq!(
        infer_rvalue_type(
            &Rvalue::SetLiteral {
                element_type: Type::named("String"),
                elements: vec![Operand::String("x".to_string())],
            },
            &variable_types,
            &returns,
            &classes,
        ),
        Some(DirectType::Opaque(Type::Named(
            "Set".to_string(),
            vec![Type::named("String")],
        )))
    );
    for (name, expected) in [
        ("range", DirectType::Opaque(Type::named("Range"))),
        (
            "queue",
            DirectType::Opaque(Type::Named(
                "Queue".to_string(),
                vec![Type::named("Unknown")],
            )),
        ),
        (
            "Vec",
            DirectType::Opaque(Type::Named("Vec".to_string(), vec![Type::named("Unknown")])),
        ),
        (
            "Set",
            DirectType::Opaque(Type::Named("Set".to_string(), vec![Type::named("Unknown")])),
        ),
        (
            "Map",
            DirectType::Opaque(Type::Named(
                "Map".to_string(),
                vec![Type::named("Unknown"), Type::named("Unknown")],
            )),
        ),
        ("tasks", DirectType::Opaque(Type::named("TaskGroup"))),
        ("cancelled", DirectType::Scalar(ScalarKind::Bool)),
        ("sleep", DirectType::Scalar(ScalarKind::Unit)),
        (
            "parse_int32",
            DirectType::Opaque(Type::Named(
                "Result".to_string(),
                vec![Type::named("int32"), Type::named("String")],
            )),
        ),
        (
            "parse_int64",
            DirectType::Opaque(Type::Named(
                "Result".to_string(),
                vec![Type::named("int64"), Type::named("String")],
            )),
        ),
        (
            "parse_float64",
            DirectType::Opaque(Type::Named(
                "Result".to_string(),
                vec![Type::named("float64"), Type::named("String")],
            )),
        ),
    ] {
        assert_eq!(
            infer_rvalue_type(
                &Rvalue::Call {
                    callee: CallTarget::Name(name.to_string()),
                    args: Vec::new(),
                },
                &variable_types,
                &returns,
                &classes,
            ),
            Some(expected),
            "expected builtin `{name}` to infer correctly",
        );
    }
}

#[test]
fn validate_operand_accepts_nested_places() {
    validate_operand(&Operand::Place("point.x".to_string()))
        .expect("nested places should now validate directly");
}

#[test]
fn ensure_direct_type_maps_runtime_backed_types_to_opaque_values() {
    let ty = ensure_direct_type(&Type::named("String"), &HashMap::new(), "test type")
        .expect("runtime-backed types should still be representable directly");
    assert_eq!(ty, DirectType::Opaque(Type::named("String")));
}

#[test]
fn signature_helpers_flatten_plain_class_abi_types() {
    let mut classes = HashMap::new();
    classes.insert(
        "Point".to_string(),
        crate::mir::MirClass {
            name: "Point".to_string(),
            type_params: Vec::new(),
            fields: vec![
                crate::mir::MirClassField {
                    name: "x".to_string(),
                    ty: Type::named("float64"),
                },
                crate::mir::MirClassField {
                    name: "y".to_string(),
                    ty: Type::named("float64"),
                },
            ],
            methods: Vec::new(),
        },
    );
    let function = MirFunction {
        name: "demo".to_string(),
        module_name: "<test>".to_string(),
        span: crate::diag::Span::new(1, 1),
        receiver: Some(MirReceiverKind::Borrow),
        params: vec![crate::mir::MirParam {
            name: "other".to_string(),
            passing: MirReceiverKind::Value,
            ty: Type::named("Point"),
        }],
        local_types: vec![crate::mir::MirLocalType {
            name: "self".to_string(),
            ty: Type::named("Point"),
        }],
        return_type: Type::named("float64"),
        entry: "entry".to_string(),
        blocks: Vec::new(),
    };

    let sig = signature_for(
        &function,
        &classes,
        cranelift_codegen::isa::CallConv::SystemV,
    )
    .expect("signature should flatten point receiver and param");
    let main_sig = main_signature(cranelift_codegen::isa::CallConv::SystemV);

    assert_eq!(sig.params.len(), 4);
    assert_eq!(sig.returns.len(), 1);
    assert_eq!(main_sig.returns.len(), 1);

    let bool_ty = DirectType::Scalar(ScalarKind::Bool);
    assert_eq!(bool_ty.abi_types(), vec![cranelift_codegen::ir::types::I64]);
    assert_eq!(bool_ty.value_count(), 1);
    assert_eq!(bool_ty.scalar_kind(), Some(ScalarKind::Bool));
    assert!(!ScalarKind::Bool.is_float());
    assert!(ScalarKind::Float64.is_float());

    let point_ty = DirectType::PlainClass(PlainClassType {
        class_name: "Point".to_string(),
        fields: vec![
            PlainClassField {
                name: "x".to_string(),
                ty: DirectType::Scalar(ScalarKind::Float64),
            },
            PlainClassField {
                name: "visible".to_string(),
                ty: DirectType::Scalar(ScalarKind::Bool),
            },
        ],
    });
    assert_eq!(
        point_ty.abi_types(),
        vec![
            cranelift_codegen::ir::types::F64,
            cranelift_codegen::ir::types::I64,
        ]
    );
    assert_eq!(point_ty.value_count(), 2);
    assert_eq!(
        point_ty.field_slice("x"),
        Some((0, 1, DirectType::Scalar(ScalarKind::Float64)))
    );
    assert_eq!(
        point_ty.field_slice("visible"),
        Some((1, 2, DirectType::Scalar(ScalarKind::Bool)))
    );
    assert_eq!(point_ty.field_slice("missing"), None);
    assert_eq!(render_direct_type(&point_ty), "Point");
}

#[test]
fn builtin_member_and_select_type_helpers_cover_collection_runtime_surface() {
    let classes = HashMap::new();

    assert_eq!(
        builtin_opaque_member_return_type(
            &Type::Named("String".to_string(), vec![]),
            "split",
            &classes,
        ),
        Some(DirectType::Opaque(Type::Named(
            "Vec".to_string(),
            vec![Type::named("String")],
        )))
    );
    assert_eq!(
        builtin_opaque_member_return_type(
            &Type::Named("Vec".to_string(), vec![Type::named("int32")]),
            "insert",
            &classes,
        ),
        Some(DirectType::Scalar(ScalarKind::Bool))
    );
    assert_eq!(
        builtin_opaque_member_return_type(
            &Type::Named(
                "Map".to_string(),
                vec![Type::named("String"), Type::named("int32")],
            ),
            "items",
            &classes,
        ),
        Some(DirectType::Opaque(Type::Named(
            "Vec".to_string(),
            vec![Type::Named(
                "MapEntry".to_string(),
                vec![Type::named("String"), Type::named("int32")],
            )],
        )))
    );
    assert_eq!(
        builtin_opaque_member_return_type(
            &Type::Named(
                "Map".to_string(),
                vec![Type::named("String"), Type::named("int32")],
            ),
            "entries",
            &classes,
        ),
        Some(DirectType::Opaque(Type::Named(
            "Vec".to_string(),
            vec![Type::Named(
                "MapEntry".to_string(),
                vec![Type::named("String"), Type::named("int32")],
            )],
        )))
    );
    assert_eq!(
        builtin_opaque_member_return_type(
            &Type::Named("Set".to_string(), vec![Type::named("String")]),
            "__index_option",
            &classes,
        ),
        Some(DirectType::Opaque(Type::Named(
            "Option".to_string(),
            vec![Type::named("String")],
        )))
    );
    assert_eq!(
        builtin_opaque_member_return_type(
            &Type::Named("Task".to_string(), vec![Type::named("int32")]),
            "result",
            &classes,
        ),
        Some(DirectType::Scalar(ScalarKind::Int32))
    );
    assert_eq!(
        builtin_opaque_member_return_type(
            &Type::Named("Set".to_string(), vec![Type::named("String")]),
            "clone",
            &classes,
        ),
        Some(DirectType::Opaque(Type::Named(
            "Set".to_string(),
            vec![Type::named("String")],
        )))
    );
    assert_eq!(
        builtin_opaque_member_return_type(
            &Type::Named("TaskGroup".to_string(), vec![]),
            "start",
            &classes,
        ),
        None
    );
    assert_eq!(
        builtin_opaque_member_return_type(
            &Type::Named("TaskGroup".to_string(), vec![]),
            "missing",
            &classes,
        ),
        None
    );
    for (object_ty, field, expected) in [
        (
            Type::Named("String".to_string(), vec![]),
            "replace",
            DirectType::Opaque(Type::named("String")),
        ),
        (
            Type::Named("String".to_string(), vec![]),
            "strip_prefix",
            DirectType::Opaque(Type::Named(
                "Option".to_string(),
                vec![Type::named("String")],
            )),
        ),
        (
            Type::Named("Vec".to_string(), vec![Type::named("int32")]),
            "clear",
            DirectType::Scalar(ScalarKind::Unit),
        ),
        (
            Type::Named("Vec".to_string(), vec![Type::named("int32")]),
            "__index",
            DirectType::Scalar(ScalarKind::Int32),
        ),
        (
            Type::Named(
                "Map".to_string(),
                vec![Type::named("String"), Type::named("int32")],
            ),
            "values",
            DirectType::Opaque(Type::Named("Vec".to_string(), vec![Type::named("int32")])),
        ),
        (
            Type::Named("Set".to_string(), vec![Type::named("String")]),
            "remove",
            DirectType::Scalar(ScalarKind::Bool),
        ),
        (
            Type::Named("Queue".to_string(), vec![Type::named("int32")]),
            "put",
            DirectType::Opaque(Type::Named(
                "Result".to_string(),
                vec![
                    Type::Unit,
                    Type::Named("SendError".to_string(), vec![Type::named("int32")]),
                ],
            )),
        ),
        (
            Type::Named("TaskGroup".to_string(), vec![]),
            "cancel",
            DirectType::Scalar(ScalarKind::Unit),
        ),
        (
            Type::Named("Task".to_string(), vec![Type::named("int32")]),
            "result",
            DirectType::Scalar(ScalarKind::Int32),
        ),
    ] {
        assert_eq!(
            builtin_opaque_member_return_type(&object_ty, field, &classes),
            Some(expected),
            "expected `{object_ty}.{field}` to infer correctly",
        );
    }

    let recv_arm = MirSelectArm {
        binding: Some("value".to_string()),
        kind: MirSelectKind::Recv {
            channel: Operand::Place("jobs".to_string()),
        },
        label: "recv".to_string(),
    };
    let send_arm = MirSelectArm {
        binding: Some("status".to_string()),
        kind: MirSelectKind::Send {
            channel: Operand::Place("jobs".to_string()),
            value: Operand::Int(7),
        },
        label: "send".to_string(),
    };
    let after_arm = MirSelectArm {
        binding: None,
        kind: MirSelectKind::After {
            duration: Operand::Duration(10),
        },
        label: "after".to_string(),
    };
    let variable_types = HashMap::from([(
        "jobs".to_string(),
        DirectType::Opaque(Type::Named("Queue".to_string(), vec![Type::named("int32")])),
    )]);

    assert_eq!(
        infer_select_binding_type(&recv_arm, &variable_types, &classes),
        Some(DirectType::Opaque(Type::Named(
            "Option".to_string(),
            vec![Type::named("int32")],
        )))
    );
    assert_eq!(
        infer_select_binding_type(&send_arm, &variable_types, &classes),
        Some(DirectType::Opaque(Type::Named(
            "Result".to_string(),
            vec![
                Type::Unit,
                Type::Named("SendError".to_string(), vec![Type::named("int32")]),
            ],
        )))
    );
    assert_eq!(
        infer_select_binding_type(&after_arm, &variable_types, &classes),
        Some(DirectType::Scalar(ScalarKind::Unit))
    );
    let fallback_send_arm = MirSelectArm {
        binding: Some("status".to_string()),
        kind: MirSelectKind::Send {
            channel: Operand::Place("flag".to_string()),
            value: Operand::Int(7),
        },
        label: "send".to_string(),
    };
    let fallback_types =
        HashMap::from([("flag".to_string(), DirectType::Scalar(ScalarKind::Bool))]);
    assert_eq!(
        infer_select_binding_type(&fallback_send_arm, &fallback_types, &classes),
        Some(DirectType::Opaque(Type::Named(
            "Result".to_string(),
            vec![
                Type::Unit,
                Type::Named("SendError".to_string(), vec![Type::named("Unknown")]),
            ],
        )))
    );
    let fallback_recv_arm = MirSelectArm {
        binding: Some("value".to_string()),
        kind: MirSelectKind::Recv {
            channel: Operand::Place("flag".to_string()),
        },
        label: "recv".to_string(),
    };
    assert_eq!(
        infer_select_binding_type(&fallback_recv_arm, &fallback_types, &classes),
        Some(DirectType::Opaque(Type::Named(
            "Option".to_string(),
            vec![Type::named("Unknown")],
        )))
    );
    let invalid_after_arm = MirSelectArm {
        binding: None,
        kind: MirSelectKind::After {
            duration: Operand::Place("missing".to_string()),
        },
        label: "after".to_string(),
    };
    assert_eq!(
        infer_select_binding_type(&invalid_after_arm, &fallback_types, &classes),
        None
    );
}

#[test]
fn direct_field_try_and_spawn_helpers_cover_remaining_direct_inference_paths() {
    let classes = HashMap::from([(
        "Entry".to_string(),
        crate::mir::MirClass {
            name: "Entry".to_string(),
            type_params: Vec::new(),
            fields: vec![crate::mir::MirClassField {
                name: "name".to_string(),
                ty: Type::named("String"),
            }],
            methods: Vec::new(),
        },
    )]);
    let variable_types = HashMap::from([
        (
            "result".to_string(),
            DirectType::Opaque(Type::Named(
                "Result".to_string(),
                vec![Type::named("int32"), Type::named("String")],
            )),
        ),
        (
            "maybe".to_string(),
            DirectType::Opaque(Type::Named(
                "Option".to_string(),
                vec![Type::named("Entry")],
            )),
        ),
    ]);

    assert_eq!(
        direct_field_type(
            &DirectType::Opaque(Type::Named(
                "MapEntry".to_string(),
                vec![Type::named("String"), Type::named("int32")],
            )),
            "key",
            &classes,
        ),
        Some(DirectType::Opaque(Type::named("String")))
    );
    assert_eq!(
        direct_field_type(
            &DirectType::Opaque(Type::Named(
                "MapEntry".to_string(),
                vec![Type::named("String"), Type::named("int32")],
            )),
            "value",
            &classes,
        ),
        Some(DirectType::Scalar(ScalarKind::Int32))
    );
    assert_eq!(
        direct_field_type(
            &DirectType::Opaque(Type::Named("Entry".to_string(), vec![])),
            "name",
            &classes,
        ),
        Some(DirectType::Opaque(Type::named("String")))
    );
    assert_eq!(
        direct_field_type(
            &DirectType::Opaque(Type::Named("Entry".to_string(), vec![Type::named("int32")])),
            "name",
            &classes,
        ),
        None
    );
    assert_eq!(
        infer_try_type(
            &Operand::Place("result".to_string()),
            &variable_types,
            &classes
        ),
        Some(DirectType::Scalar(ScalarKind::Int32))
    );
    assert_eq!(
        infer_operand_type(&Operand::Unit, &variable_types, &classes),
        Some(DirectType::Scalar(ScalarKind::Unit))
    );
    assert_eq!(
        infer_operand_type(
            &Operand::String("aurora".to_string()),
            &variable_types,
            &classes,
        ),
        Some(DirectType::Opaque(Type::named("String")))
    );
    assert_eq!(
        infer_operand_type(&Operand::Duration(5), &variable_types, &classes),
        Some(DirectType::Opaque(Type::named("Duration")))
    );
    assert_eq!(
        infer_operand_type(
            &Operand::Int((i64::MAX as u128) + 1),
            &variable_types,
            &classes,
        ),
        Some(DirectType::Opaque(Type::named("Unknown")))
    );
    assert_eq!(
        infer_rvalue_type(
            &Rvalue::VariantPayload {
                scrutinee: Operand::Place("maybe".to_string()),
                index: 0,
            },
            &variable_types,
            &HashMap::new(),
            &classes,
        ),
        Some(DirectType::Opaque(Type::named("Entry")))
    );
    assert_eq!(
        collect_spawn_targets(&crate::mir::MirModule {
            functions: vec![MirFunction {
                name: "main".to_string(),
                module_name: "<test>".to_string(),
                span: Span::new(1, 1),
                receiver: None,
                params: Vec::new(),
                local_types: Vec::new(),
                return_type: Type::named("int32"),
                entry: "entry".to_string(),
                blocks: vec![BasicBlock {
                    label: "entry".to_string(),
                    instructions: vec![Instruction::Assign {
                        target: "%t0".to_string(),
                        value: Rvalue::Spawn {
                            detached: false,
                            task_group: None,
                            function: "worker".to_string(),
                            args: Vec::new(),
                        },
                    }],
                    terminator: Terminator::Return(Operand::Int(0)),
                }],
            }],
            classes: Vec::new(),
            trait_impls: Vec::new(),
            top_level: None,
        }),
        BTreeSet::from(["worker".to_string()])
    );
}

#[test]
fn native_codegen_helper_utilities_cover_signatures_wildcards_and_metadata() {
    let call_conv = cranelift_codegen::isa::CallConv::SystemV;
    let main_sig = main_signature(call_conv);
    assert_eq!(main_sig.params.len(), 0);
    assert_eq!(main_sig.returns.len(), 1);

    let thunk_sig = thunk_signature(call_conv);
    assert_eq!(thunk_sig.params.len(), 2);
    assert_eq!(thunk_sig.returns.len(), 1);

    assert_eq!(
        mangle_thunk_symbol("pkg.main::worker"),
        "aurora_thunk_pkg_main__worker"
    );

    assert_eq!(
        direct_type_to_type(&DirectType::Scalar(ScalarKind::Unit)),
        Type::Unit
    );
    assert_eq!(
        direct_type_to_type(&DirectType::Scalar(ScalarKind::Float32)),
        Type::named("float32")
    );
    assert_eq!(
        direct_type_to_type(&DirectType::Scalar(ScalarKind::Float64)),
        Type::named("float64")
    );
    assert_eq!(
        direct_type_to_type(&DirectType::Scalar(ScalarKind::Bool)),
        Type::named("bool")
    );
    assert_eq!(
        direct_type_to_type(&DirectType::PlainClass(PlainClassType {
            class_name: "Point".to_string(),
            fields: vec![PlainClassField {
                name: "x".to_string(),
                ty: DirectType::Scalar(ScalarKind::Int32),
            }],
        })),
        Type::named("Point")
    );
    assert_eq!(
        direct_type_to_type(&DirectType::Opaque(Type::Named(
            "Vec".to_string(),
            vec![Type::named("int32")],
        ))),
        Type::Named("Vec".to_string(), vec![Type::named("int32")])
    );
    assert_eq!(
        DirectType::Scalar(ScalarKind::Bool).scalar_kind(),
        Some(ScalarKind::Bool)
    );
    assert_eq!(
        DirectType::Opaque(Type::named("String")).scalar_kind(),
        None
    );
    assert_eq!(
        render_direct_type(&DirectType::Scalar(ScalarKind::Float32)),
        "float32"
    );
    assert_eq!(
        render_direct_type(&DirectType::Scalar(ScalarKind::Float64)),
        "float64"
    );
    assert_eq!(
        render_direct_type(&DirectType::Scalar(ScalarKind::Bool)),
        "bool"
    );
    assert!(!ScalarKind::Bool.is_float());
    assert!(ScalarKind::Float32.is_float());
    assert_eq!(
        DirectType::Scalar(ScalarKind::Float64).abi_types(),
        vec![cranelift_codegen::ir::types::F64]
    );
    assert_eq!(
        DirectType::Opaque(Type::named("String")).abi_types(),
        vec![cranelift_codegen::ir::types::I64]
    );
    let flat_class = DirectType::PlainClass(PlainClassType {
        class_name: "Pair".to_string(),
        fields: vec![
            PlainClassField {
                name: "left".to_string(),
                ty: DirectType::Scalar(ScalarKind::Int32),
            },
            PlainClassField {
                name: "right".to_string(),
                ty: DirectType::Scalar(ScalarKind::Float64),
            },
        ],
    });
    assert_eq!(
        flat_class.abi_types(),
        vec![
            cranelift_codegen::ir::types::I64,
            cranelift_codegen::ir::types::F64,
        ]
    );
    assert_eq!(flat_class.value_count(), 2);
    assert_eq!(
        flat_class.field_slice("right"),
        Some((1, 2, DirectType::Scalar(ScalarKind::Float64)))
    );
    assert_eq!(flat_class.field_slice("missing"), None);

    let mut ctx = Context::new();
    ctx.func.signature = cranelift_codegen::ir::Signature::new(call_conv);
    let mut builder_ctx = FunctionBuilderContext::new();
    let mut builder = cranelift_frontend::FunctionBuilder::new(&mut ctx.func, &mut builder_ctx);
    let block = builder.create_block();
    builder.switch_to_block(block);
    builder.seal_block(block);

    let scalar_zero = DirectType::Scalar(ScalarKind::Int32).zero_values(&mut builder);
    let opaque_zero = DirectType::Opaque(Type::named("String")).zero_values(&mut builder);
    let class_zero = flat_class.zero_values(&mut builder);
    assert_eq!(scalar_zero.len(), 1);
    assert_eq!(opaque_zero.len(), 1);
    assert_eq!(class_zero.len(), 2);
    builder.ins().return_(&[]);
    builder.finalize();

    assert!(is_numeric_type_name(&Type::named("uint64")));
    assert!(is_numeric_type_name(&Type::named("float32")));
    assert!(!is_numeric_type_name(&Type::named("String")));
    assert!(!is_numeric_type_name(&Type::Named(
        "Vec".to_string(),
        vec![Type::named("int32")],
    )));

    assert!(runtime_type_is_wildcard(&Type::TypeParam("T".to_string())));
    assert!(runtime_type_is_wildcard(&Type::named("Unknown")));
    assert!(runtime_type_is_wildcard(&Type::Named(
        "Map".to_string(),
        vec![Type::named("String"), Type::named("Unknown")],
    )));
    assert!(!runtime_type_is_wildcard(&Type::Named(
        "Vec".to_string(),
        vec![Type::named("int32")],
    )));
    assert!(!runtime_type_is_wildcard(&Type::Unit));

    let source = "def main() -> int32:\n    return 0\n";
    let mir = lower_source_to_mir(source).expect("simple source should lower");
    let object = emit_host_object_with_metadata(&mir, "/tmp/demo.au", source)
        .expect("metadata-backed object emission should succeed");
    assert!(!object.is_empty());

    let invalid_module = crate::mir::MirModule {
        functions: vec![MirFunction {
            name: "main".to_string(),
            module_name: "<test>".to_string(),
            span: Span::new(1, 1),
            receiver: None,
            params: Vec::new(),
            local_types: Vec::new(),
            return_type: Type::named("int32"),
            entry: "entry".to_string(),
            blocks: vec![BasicBlock {
                label: "entry".to_string(),
                instructions: Vec::new(),
                terminator: Terminator::Unreachable,
            }],
        }],
        classes: Vec::new(),
        trait_impls: Vec::new(),
        top_level: None,
    };
    let error = emit_host_object(&invalid_module)
        .expect_err("invalid modules should be rejected before codegen");
    assert!(error.contains("does not yet support MIR terminator"));
}

#[test]
fn native_codegen_builtin_member_tables_and_trait_lookup_cover_additional_paths() {
    let classes = HashMap::from([
        (
            "Node".to_string(),
            crate::mir::MirClass {
                name: "Node".to_string(),
                type_params: Vec::new(),
                fields: vec![crate::mir::MirClassField {
                    name: "next".to_string(),
                    ty: Type::named("Node"),
                }],
                methods: Vec::new(),
            },
        ),
        (
            "Box".to_string(),
            crate::mir::MirClass {
                name: "Box".to_string(),
                type_params: vec!["T".to_string()],
                fields: vec![crate::mir::MirClassField {
                    name: "value".to_string(),
                    ty: Type::TypeParam("T".to_string()),
                }],
                methods: Vec::new(),
            },
        ),
    ]);

    for (object_ty, field, expected) in [
        (
            Type::named("int32"),
            "to_string",
            Some(DirectType::Opaque(Type::named("String"))),
        ),
        (
            Type::named("float64"),
            "to_string",
            Some(DirectType::Opaque(Type::named("String"))),
        ),
        (
            Type::named("bool"),
            "to_string",
            Some(DirectType::Opaque(Type::named("String"))),
        ),
        (
            Type::named("String"),
            "split",
            Some(DirectType::Opaque(Type::Named(
                "Vec".to_string(),
                vec![Type::named("String")],
            ))),
        ),
        (
            Type::named("String"),
            "to_lower",
            Some(DirectType::Opaque(Type::named("String"))),
        ),
        (
            Type::named("String"),
            "to_upper",
            Some(DirectType::Opaque(Type::named("String"))),
        ),
        (
            Type::named("String"),
            "strip_suffix",
            Some(DirectType::Opaque(Type::Named(
                "Option".to_string(),
                vec![Type::named("String")],
            ))),
        ),
        (
            Type::Named("Vec".to_string(), vec![Type::named("int32")]),
            "is_empty",
            Some(DirectType::Scalar(ScalarKind::Bool)),
        ),
        (
            Type::Named("Vec".to_string(), vec![Type::named("int32")]),
            "pop",
            Some(DirectType::Opaque(Type::Named(
                "Option".to_string(),
                vec![Type::named("int32")],
            ))),
        ),
        (
            Type::Named(
                "Map".to_string(),
                vec![Type::named("String"), Type::named("int32")],
            ),
            "keys",
            Some(DirectType::Opaque(Type::Named(
                "Vec".to_string(),
                vec![Type::named("String")],
            ))),
        ),
        (
            Type::Named("Set".to_string(), vec![Type::named("String")]),
            "contains",
            Some(DirectType::Scalar(ScalarKind::Bool)),
        ),
        (
            Type::Named("Queue".to_string(), vec![Type::named("int32")]),
            "get",
            Some(DirectType::Opaque(Type::Named(
                "Option".to_string(),
                vec![Type::named("int32")],
            ))),
        ),
        (
            Type::Named("Task".to_string(), vec![Type::named("int32")]),
            "result",
            Some(DirectType::Scalar(ScalarKind::Int32)),
        ),
        (
            Type::Named("TaskGroup".to_string(), vec![]),
            "cancel",
            Some(DirectType::Scalar(ScalarKind::Unit)),
        ),
        (Type::named("String"), "missing", None),
    ] {
        assert_eq!(
            builtin_opaque_member_return_type(&object_ty, field, &classes),
            expected,
            "unexpected direct member type for `{object_ty}.{field}`"
        );
    }

    assert_eq!(
        direct_type(&Type::named("Node"), &classes),
        Some(DirectType::Opaque(Type::named("Node"))),
        "recursive plain classes should fall back to opaque values"
    );
    assert_eq!(
        direct_type(
            &Type::Named("Box".to_string(), vec![Type::named("int32")]),
            &classes
        ),
        Some(DirectType::Opaque(Type::Named(
            "Box".to_string(),
            vec![Type::named("int32")],
        ))),
        "generic classes should stay opaque in direct type inference"
    );
}

#[test]
fn native_codegen_type_helpers_cover_nested_type_params_and_opaque_fallbacks() {
    let classes = HashMap::from([
        (
            "Pair".to_string(),
            crate::mir::MirClass {
                name: "Pair".to_string(),
                type_params: Vec::new(),
                fields: vec![
                    crate::mir::MirClassField {
                        name: "left".to_string(),
                        ty: Type::named("int32"),
                    },
                    crate::mir::MirClassField {
                        name: "right".to_string(),
                        ty: Type::named("bool"),
                    },
                ],
                methods: Vec::new(),
            },
        ),
        (
            "Wrapper".to_string(),
            crate::mir::MirClass {
                name: "Wrapper".to_string(),
                type_params: Vec::new(),
                fields: vec![crate::mir::MirClassField {
                    name: "value".to_string(),
                    ty: Type::named("String"),
                }],
                methods: Vec::new(),
            },
        ),
    ]);

    let mut collected = BTreeSet::new();
    collect_type_params_from_type(
        &Type::Named(
            "Map".to_string(),
            vec![
                Type::TypeParam("K".to_string()),
                Type::Named("Vec".to_string(), vec![Type::TypeParam("V".to_string())]),
            ],
        ),
        &mut collected,
    );
    assert_eq!(
        collected,
        BTreeSet::from(["K".to_string(), "V".to_string()])
    );

    assert_eq!(
        direct_type(&Type::TypeParam("T".to_string()), &classes),
        Some(DirectType::Opaque(Type::TypeParam("T".to_string())))
    );
    assert_eq!(
        direct_type(&Type::Module("pkg.tools".to_string()), &classes),
        Some(DirectType::Opaque(Type::Module("pkg.tools".to_string())))
    );
    assert_eq!(
        direct_type(&Type::named("float32"), &classes),
        Some(DirectType::Scalar(ScalarKind::Float32))
    );
    assert_eq!(
        direct_type(&Type::named("External"), &classes),
        Some(DirectType::Opaque(Type::named("External")))
    );

    let pair = direct_type(&Type::named("Pair"), &classes).expect("Pair should stay plain");
    assert_eq!(render_direct_type(&pair), "Pair");
    assert_eq!(pair.value_count(), 2);

    assert_eq!(
        direct_type(&Type::named("Wrapper"), &classes),
        Some(DirectType::Opaque(Type::named("Wrapper"))),
        "classes with opaque fields should fall back to opaque direct values"
    );
}

#[test]
fn validate_function_rejects_unreachable_terminators_for_direct_backend() {
    let function = MirFunction {
        name: "main".to_string(),
        module_name: "<test>".to_string(),
        span: Span::new(1, 1),
        receiver: None,
        params: vec![MirParam {
            name: "value".to_string(),
            passing: MirReceiverKind::Value,
            ty: Type::named("int32"),
        }],
        local_types: vec![MirLocalType {
            name: "value".to_string(),
            ty: Type::named("int32"),
        }],
        return_type: Type::named("int32"),
        entry: "entry".to_string(),
        blocks: vec![BasicBlock {
            label: "entry".to_string(),
            instructions: vec![
                Instruction::Assign {
                    target: "%t0".to_string(),
                    value: Rvalue::FormatString {
                        parts: vec![
                            crate::mir::MirFormatPart::Literal("value=".to_string()),
                            crate::mir::MirFormatPart::Value(Operand::Place("value".to_string())),
                        ],
                    },
                },
                Instruction::Assign {
                    target: "%t1".to_string(),
                    value: Rvalue::VecLiteral {
                        elements: vec![Operand::Int(1)],
                        element_type: Type::named("int32"),
                    },
                },
                Instruction::Assign {
                    target: "%t2".to_string(),
                    value: Rvalue::MapLiteral {
                        entries: vec![MirMapEntry {
                            key: Operand::String("a".to_string()),
                            value: Operand::Int(1),
                        }],
                        key_type: Type::named("String"),
                        value_type: Type::named("int32"),
                    },
                },
            ],
            terminator: Terminator::Unreachable,
        }],
    };

    let error = validate_function(&function, &HashMap::new()).expect_err("unreachable should fail");
    assert!(
        error.contains("does not yet support MIR terminator"),
        "unexpected error: {error}"
    );
}

#[test]
fn native_codegen_constructor_initializes_runtime_function_surface() {
    let module = crate::mir::MirModule {
        functions: vec![MirFunction {
            name: "main".to_string(),
            module_name: "<test>".to_string(),
            span: Span::new(1, 1),
            receiver: None,
            params: Vec::new(),
            local_types: Vec::new(),
            return_type: Type::named("int32"),
            entry: "entry".to_string(),
            blocks: vec![BasicBlock {
                label: "entry".to_string(),
                instructions: Vec::new(),
                terminator: Terminator::Return(Operand::Int(0)),
            }],
        }],
        classes: Vec::new(),
        trait_impls: Vec::new(),
        top_level: None,
    };

    let codegen = super::NativeCodegen::new(
        &module,
        "/tmp/direct_constructor.au",
        "def main() -> int32:\n    return 0\n",
    )
    .expect("direct codegen constructor should initialize runtime symbols");

    assert_eq!(codegen.program_path, "/tmp/direct_constructor.au");
    assert!(codegen.program_source.contains("return 0"));
    assert!(codegen.classes.is_empty());
    assert!(codegen.trait_impls.is_empty());
    assert!(codegen.string_data.is_empty());
    assert!(codegen.functions.contains_key("main"));
    assert!(codegen.function_return_types.contains_key("main"));
    assert!(codegen.function_param_types.contains_key("main"));
    assert!(codegen.function_writeback_types.contains_key("main"));
}

#[test]
fn native_codegen_constructor_tracks_receiver_and_writeback_types_for_methods_and_top_level() {
    let source = r#"
class Counter:
    value: int32

    def sync_into(borrow mut self, other: borrow mut Counter, amount: int32):
        self.value += amount
        other.value = self.value

mut left: Counter = Counter(value=1)
mut right: Counter = Counter(value=0)
left.sync_into(other=right, amount=2)
"#;
    let mir = lower_source_to_mir(source).expect("source should lower to MIR");
    let method = mir
        .functions
        .iter()
        .find(|function| function.receiver == Some(MirReceiverKind::BorrowMut))
        .expect("borrow-mut method should lower into a function");
    let top_level = mir
        .top_level
        .as_ref()
        .expect("top-level script should lower into a top-level entry function");

    let codegen = super::NativeCodegen::new(&mir, "/tmp/direct_constructor_writebacks.au", source)
        .expect("direct codegen constructor should initialize runtime symbols");

    let method_params = codegen
        .function_param_types
        .get(&method.name)
        .expect("method param metadata should be registered");
    assert_eq!(method_params.len(), 3);
    assert!(matches!(method_params[0], DirectType::PlainClass(_)));
    assert!(matches!(method_params[1], DirectType::PlainClass(_)));
    assert_eq!(method_params[2], DirectType::Scalar(ScalarKind::Int32));

    let method_writebacks = codegen
        .function_writeback_types
        .get(&method.name)
        .expect("method writeback metadata should be registered");
    assert_eq!(method_writebacks.len(), 2);
    assert!(matches!(method_writebacks[0], DirectType::PlainClass(_)));
    assert!(matches!(method_writebacks[1], DirectType::PlainClass(_)));

    assert!(codegen.functions.contains_key(&top_level.name));
    assert!(codegen.function_thunks.contains_key(&top_level.name));
}

#[test]
fn native_codegen_replace_nested_field_rejects_empty_paths_without_panicking() {
    let error = super::split_field_path_segments(&[])
        .expect_err("empty field paths should surface an internal diagnostic");
    assert!(error.contains("empty field path"));
}

#[test]
fn native_codegen_thunks_cover_float_bool_plain_class_params_and_unit_main_wrapper() {
    let thunk_source = r#"
class Pair:
    left: int32
    right: bool

def helper(value: float64, flag: bool, pair: Pair) -> float64:
    if flag:
        return value
    return 0.0

def main() -> int32:
    return 0
"#;
    let thunk_mir = lower_source_to_mir(thunk_source).expect("source should lower to MIR");
    let helper = thunk_mir
        .functions
        .iter()
        .find(|function| function.name == "helper")
        .expect("helper function should be lowered");
    let mut thunk_codegen = super::NativeCodegen::new(
        &thunk_mir,
        "/tmp/direct_thunk_float_bool_pair.au",
        thunk_source,
    )
    .expect("codegen should initialize");
    thunk_codegen
        .define_function_thunk(helper)
        .expect("thunk generation should support float, bool, and plain-class parameters");

    let wrapper_source = "def main():\n    pass\n";
    let wrapper_mir = lower_source_to_mir(wrapper_source).expect("unit main should lower");
    let mut wrapper_codegen = super::NativeCodegen::new(
        &wrapper_mir,
        "/tmp/direct_unit_main_wrapper.au",
        wrapper_source,
    )
    .expect("codegen should initialize");
    wrapper_codegen
        .define_main_wrapper()
        .expect("main wrapper should support unit-return entrypoints");
}
