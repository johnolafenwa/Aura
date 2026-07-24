use super::*;
use crate::ast::{
    Argument, AssignTarget, BindingPattern, BindingTarget, Expr, ExprKind, ForStmt, LiteralPattern,
    LiteralPatternKind, MapEntryExpr, PassStmt, Pattern, Stmt, TypeRef, VariantPattern,
};
use crate::diag::Span;
use crate::integer::IntegerValue;
use crate::sema::{binary_operator_trait, unary_operator_trait, ModuleNamespace, TraitBound};
use std::path::PathBuf;

fn checked_program(source: &str) -> Program {
    crate::check_source(source).expect("source should type check")
}

fn expr(kind: ExprKind) -> Expr {
    Expr {
        kind,
        span: Span::new(1, 1),
    }
}

fn name_expr(name: &str) -> Expr {
    expr(ExprKind::Name(name.to_string()))
}

fn member_expr(object: Expr, field: &str) -> Expr {
    expr(ExprKind::Member {
        object: Box::new(object),
        field: field.to_string(),
    })
}

fn type_ref(name: &str) -> TypeRef {
    TypeRef::named(name, Vec::new(), false, Span::new(1, 1))
}

#[test]
fn json_runtime_enums_keep_their_module_qualified_identity() {
    let program = checked_program("def main():\n    pass\n");
    for enum_name in ["Value", "Error"] {
        let enum_info = crate::sema::EnumInfo {
            module_name: "json".to_string(),
            decl: crate::ast::EnumDecl {
                public: true,
                name: enum_name.to_string(),
                type_params: Vec::new(),
                type_param_bounds: std::collections::BTreeMap::new(),
                variants: Vec::new(),
                span: Span::new(1, 1),
            },
            type_param_bounds: std::collections::BTreeMap::new(),
            variants: std::collections::BTreeMap::new(),
        };
        assert_eq!(
            mir_runtime_enum_name(&program, &enum_info),
            format!("json.{enum_name}")
        );
    }
}

fn arg(value: Expr) -> Argument {
    Argument {
        name: None,
        span: value.span,
        value,
    }
}

#[test]
fn tuple_literals_capture_each_element_left_to_right_before_construction() {
    let module = crate::lower_source_to_mir(
        r#"
def first() -> String:
    return "first"

def second() -> String:
    return "second"

def main():
    pair = (first(), second())
"#,
    )
    .expect("tuple literal should lower");
    let main = module
        .functions
        .iter()
        .find(|function| function.name == "main")
        .expect("main should lower");
    let instructions = main
        .blocks
        .iter()
        .flat_map(|block| &block.instructions)
        .collect::<Vec<_>>();

    let first_call = instructions
        .iter()
        .position(|instruction| {
            matches!(
                instruction,
                Instruction::Assign {
                    value: Rvalue::Call {
                        callee: CallTarget::Name(name),
                        ..
                    },
                    ..
                } if name == "first"
            )
        })
        .expect("first call should lower");
    let second_call = instructions
        .iter()
        .position(|instruction| {
            matches!(
                instruction,
                Instruction::Assign {
                    value: Rvalue::Call {
                        callee: CallTarget::Name(name),
                        ..
                    },
                    ..
                } if name == "second"
            )
        })
        .expect("second call should lower");
    let tuple_construct = instructions
        .iter()
        .position(|instruction| {
            matches!(
                instruction,
                Instruction::Assign {
                    value: Rvalue::TupleLiteral { .. },
                    ..
                }
            )
        })
        .expect("tuple construction should lower");
    assert!(first_call < second_call && second_call < tuple_construct);
    assert!(
        instructions[first_call + 1..second_call]
            .iter()
            .any(|instruction| {
                matches!(
                    instruction,
                    Instruction::Assign {
                        value: Rvalue::Use(Operand::MovePlace(_)),
                        ..
                    }
                )
            }),
        "the first element must be captured before the second expression runs"
    );
}

#[test]
fn tuple_literal_elements_use_the_explicit_tuple_annotation_context() {
    let module = crate::lower_source_to_mir(
        r#"
def main():
    pair: (int8, int16) = (1, 2)
"#,
    )
    .expect("annotated tuple literal should lower");
    let main = module
        .functions
        .iter()
        .find(|function| function.name == "main")
        .expect("main should lower");
    let element_types = main
        .blocks
        .iter()
        .flat_map(|block| &block.instructions)
        .find_map(|instruction| match instruction {
            Instruction::Assign {
                value: Rvalue::TupleLiteral { element_types, .. },
                ..
            } => Some(element_types),
            _ => None,
        })
        .expect("tuple literal should be explicit in MIR");
    assert_eq!(
        element_types,
        &vec![Type::named("int8"), Type::named("int16")]
    );
}

#[test]
fn non_copy_destructure_consumes_the_whole_source_then_takes_captured_elements() {
    let module = crate::lower_source_to_mir(
        r#"
def main():
    pair = ("left", "right")
    left, right = pair
    print(left)
    print(right)
"#,
    )
    .expect("non-Copy tuple destructure should lower");
    let main = module
        .functions
        .iter()
        .find(|function| function.name == "main")
        .expect("main should lower");
    let instructions = main
        .blocks
        .iter()
        .flat_map(|block| &block.instructions)
        .collect::<Vec<_>>();

    assert_eq!(
        instructions
            .iter()
            .filter(|instruction| {
                matches!(
                    instruction,
                    Instruction::Assign {
                        value: Rvalue::Use(Operand::MovePlace(place)),
                        ..
                    } if place == "pair"
                )
            })
            .count(),
        1,
        "the original tuple binding must be consumed exactly once"
    );
    let takes = instructions
        .iter()
        .filter_map(|instruction| match instruction {
            Instruction::Assign {
                value: Rvalue::TupleTakeElement { place, index, .. },
                ..
            } => Some((place, index)),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(takes.len(), 2);
    assert!(takes.iter().all(|(place, _)| place.starts_with("%t")));
    assert_eq!(
        takes.iter().map(|(_, index)| **index).collect::<Vec<_>>(),
        vec![0, 1]
    );
}

#[test]
fn copy_tuple_indexing_projects_without_partial_move_mir() {
    let module = crate::lower_source_to_mir(
        r#"
def main():
    pair = (10, 20)
    first = pair[0]
    print(first)
"#,
    )
    .expect("Copy tuple index should lower");
    let main = module
        .functions
        .iter()
        .find(|function| function.name == "main")
        .expect("main should lower");
    let rvalues = main
        .blocks
        .iter()
        .flat_map(|block| &block.instructions)
        .filter_map(|instruction| match instruction {
            Instruction::Assign { value, .. } => Some(value),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert!(rvalues.iter().any(|value| {
        matches!(
            value,
            Rvalue::TupleElement {
                index: 0,
                element_type,
                ..
            } if *element_type == Type::named("int64")
        )
    }));
    assert!(!rvalues
        .iter()
        .any(|value| matches!(value, Rvalue::TupleTakeElement { .. })));
}

#[test]
fn tuple_patterns_lower_through_element_cfg_not_enum_match() {
    let module = crate::lower_source_to_mir(
        r#"
def main():
    match (1, 2):
        case (1, value):
            print(value)
        case _:
            pass
"#,
    )
    .expect("tuple pattern should lower");
    let main = module
        .functions
        .iter()
        .find(|function| function.name == "main")
        .expect("main should lower");

    assert!(main.blocks.iter().any(|block| {
        block.instructions.iter().any(|instruction| {
            matches!(
                instruction,
                Instruction::Assign {
                    value: Rvalue::TupleElement { .. },
                    ..
                }
            )
        })
    }));
    assert!(main
        .blocks
        .iter()
        .any(|block| matches!(block.terminator, Terminator::Branch { .. })));
    assert!(
        !main
            .blocks
            .iter()
            .any(|block| matches!(block.terminator, Terminator::Match { .. })),
        "tuple patterns must not use the enum-only Match terminator"
    );
}

#[test]
fn consuming_bind_only_tuple_patterns_do_not_clone_elements_during_matching() {
    let module = crate::lower_source_to_mir(
        r#"
def main():
    match ("left", "right"):
        case (left, right):
            print(left)
            print(right)
"#,
    )
    .expect("consuming bind-only tuple pattern should lower");
    let main = module
        .functions
        .iter()
        .find(|function| function.name == "main")
        .expect("main should lower");
    let rvalues = main
        .blocks
        .iter()
        .flat_map(|block| &block.instructions)
        .filter_map(|instruction| match instruction {
            Instruction::Assign { value, .. } => Some(value),
            _ => None,
        })
        .collect::<Vec<_>>();

    assert_eq!(
        rvalues
            .iter()
            .filter(|value| matches!(value, Rvalue::TupleTakeElement { .. }))
            .count(),
        2
    );
    assert!(
        !rvalues
            .iter()
            .any(|value| matches!(value, Rvalue::TupleElement { .. })),
        "binding registration must not clone non-Copy tuple elements"
    );
}

#[test]
fn consuming_mixed_tuple_patterns_take_owned_elements_and_copy_scalar_bindings() {
    let source = r#"
def main():
    match ("owned", 7, true):
        case (text, number, true):
            print(f"{text}:{number}")
        case _:
            pass
"#;
    let module = crate::lower_source_to_mir(source)
        .expect("a consuming tuple pattern may mix owned and Copy bindings");
    let output = crate::run_mir(&module).expect("the mixed tuple pattern should execute");
    assert_eq!(output.stdout, "owned:7\n");

    let main = module
        .functions
        .iter()
        .find(|function| function.name == "main")
        .expect("main should lower");
    let rvalues = main
        .blocks
        .iter()
        .flat_map(|block| &block.instructions)
        .filter_map(|instruction| match instruction {
            Instruction::Assign { value, .. } => Some(value),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert!(rvalues.iter().any(|value| {
        matches!(
            value,
            Rvalue::TupleTakeElement {
                index: 0,
                element_type,
                ..
            } if element_type == &Type::named("String")
        )
    }));
    assert!(rvalues.iter().any(|value| {
        matches!(
            value,
            Rvalue::TupleElement {
                index: 1,
                element_type,
                ..
            } if element_type == &Type::named("int64")
        )
    }));
}

#[test]
fn tuple_for_targets_project_the_iteration_value_before_the_body() {
    let module = crate::lower_source_to_mir(
        r#"
def main():
    rows = [(1, 2), (3, 4)]
    for left, right in rows:
        print(left + right)
"#,
    )
    .expect("tuple for-target should lower");
    let main = module
        .functions
        .iter()
        .find(|function| function.name == "main")
        .expect("main should lower");
    let body = main
        .blocks
        .iter()
        .find(|block| block.label.contains("for_body"))
        .expect("for body should lower");
    let projection_indices = body
        .instructions
        .iter()
        .filter_map(|instruction| match instruction {
            Instruction::Assign {
                value: Rvalue::TupleElement { index, .. },
                ..
            } => Some(*index),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(projection_indices, vec![0, 1]);
}

#[test]
fn nested_and_copy_tuple_patterns_preserve_binding_ownership() {
    let module = crate::lower_source_to_mir(
        r#"
def nested():
    match (("left", "right"), "tail"):
        case ((left, right), tail):
            print(left)
            print(right)
            print(tail)

def copied():
    pair = (10, 20)
    match pair:
        case (left, right):
            print(left + right)
    print(pair[0])

def main():
    nested()
    copied()
"#,
    )
    .expect("nested and Copy tuple patterns should lower");

    let nested = module
        .functions
        .iter()
        .find(|function| function.name == "nested")
        .expect("nested pattern function should lower");
    assert!(
        nested.blocks.iter().any(|block| {
            block.instructions.iter().any(|instruction| {
                matches!(
                    instruction,
                    Instruction::Assign {
                        value: Rvalue::TupleTakeElement { .. },
                        ..
                    }
                )
            })
        }),
        "nested non-Copy bindings must transfer their tuple elements"
    );

    let copied = module
        .functions
        .iter()
        .find(|function| function.name == "copied")
        .expect("Copy pattern function should lower");
    let copied_rvalues = copied
        .blocks
        .iter()
        .flat_map(|block| &block.instructions)
        .filter_map(|instruction| match instruction {
            Instruction::Assign { value, .. } => Some(value),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert!(
        copied_rvalues
            .iter()
            .filter(|value| matches!(value, Rvalue::TupleElement { .. }))
            .count()
            >= 3,
        "Copy pattern bindings and the later index must project without consuming the tuple"
    );
    assert!(
        !copied_rvalues
            .iter()
            .any(|value| matches!(value, Rvalue::TupleTakeElement { .. })),
        "matching Copy elements must leave the original tuple readable"
    );
}

#[test]
fn grouped_tuple_index_and_set_destructure_execute_through_mir() {
    let output = crate::run_source(
        r#"
def main():
    pair = (10, 20)
    print(pair[(0)])

    rows: Set[(int64, int64)] = Set{(1, 2)}
    for left, right in rows:
        print(left + right)
"#,
    )
    .expect("grouped tuple indexing and Set tuple targets should run");

    assert_eq!(output.stdout, "10\n3\n");
}

#[test]
fn generic_tuple_type_helpers_preserve_nested_parameters_and_structure() {
    let tuple = Type::Tuple(vec![
        Type::TypeParam("Left".to_string()),
        Type::Named(
            "Vec".to_string(),
            vec![Type::TypeParam("Right".to_string())],
        ),
    ]);
    let mut collected = BTreeSet::new();
    collect_type_params_from_type(&tuple, &mut collected);
    assert_eq!(
        collected,
        BTreeSet::from(["Left".to_string(), "Right".to_string()]),
        "tuple elements must participate in generic trait-impl specialization"
    );

    let tuple_ref = TypeRef::tuple(
        vec![
            type_ref("int"),
            TypeRef::named("Vec", vec![type_ref("str")], false, Span::new(1, 1)),
        ],
        false,
        Span::new(1, 1),
    );
    assert_eq!(
        lower_type_ref(&tuple_ref),
        Type::Tuple(vec![
            Type::named("int64"),
            Type::Named("Vec".to_string(), vec![Type::named("String")]),
        ])
    );
}

#[test]
fn d3_mir_canonicalizes_int_and_defaults_unhinted_integer_values_to_int64() {
    let lowerer = trait_lowerer();

    assert_eq!(lower_type_ref(&type_ref("int")), Type::named("int64"));
    assert_eq!(
        lowerer.infer_expr_type(&expr(ExprKind::Int(7))),
        Some(Type::named("int64"))
    );
    assert_eq!(
        lowerer.infer_expr_type(&expr(ExprKind::Unary {
            op: UnaryOp::Neg,
            expr: Box::new(expr(ExprKind::Int(7))),
        })),
        Some(Type::named("int64"))
    );
    assert_eq!(
        lowerer.infer_option_some_call_type(&expr(ExprKind::Int(7))),
        Some(Type::Named(
            "Option".to_string(),
            vec![Type::named("int64")]
        ))
    );
    assert_eq!(
        lowerer.infer_operand_type(&Operand::Int(7)),
        Some(Type::named("int64"))
    );
}

#[test]
fn duration_nanoseconds_lower_to_an_exact_i128_operand() {
    let exact = i128::MAX - 123;
    let mut lowerer = trait_lowerer();

    assert_eq!(
        lowerer.lower_expr(&expr(ExprKind::DurationNanos(exact))),
        Operand::Duration(exact)
    );

    let largest_millisecond_count = i128::MAX as u128 / 1_000_000;
    let expected_nanos = i128::try_from(largest_millisecond_count * 1_000_000)
        .expect("largest millisecond literal should fit signed i128 nanoseconds");
    let module = crate::lower_source_to_mir(&format!(
        "def exact() -> Duration:\n    return {largest_millisecond_count}ms\n\ndef main() -> int32:\n    return 0\n"
    ))
    .expect("largest millisecond literal should lower to MIR");
    let exact = module
        .functions
        .iter()
        .find(|function| function.name == "exact")
        .expect("exact should lower");
    assert!(exact.blocks.iter().any(|block| {
        matches!(block.terminator, Terminator::Return(Operand::Duration(value)) if value == expected_nanos)
    }));
}

#[test]
fn duration_constructors_and_conversions_lower_to_canonical_call_targets() {
    let module = crate::lower_source_to_mir(
        r#"
def milliseconds(value: int64) -> float64:
    duration = Duration.seconds(value)
    return duration.to_ms()

def main() -> int32:
    return 0
"#,
    )
    .expect("Duration constructors and conversion methods should lower to MIR");
    let function = module
        .functions
        .iter()
        .find(|function| function.name == "milliseconds")
        .expect("milliseconds helper should lower");
    assert!(function
        .blocks
        .iter()
        .any(
            |block| block.instructions.iter().any(|instruction| matches!(
                instruction,
                Instruction::Assign {
                    value: Rvalue::Call {
                        callee: CallTarget::Name(name),
                        ..
                    },
                    ..
                } if name == "Duration.seconds"
            ))
        ));
    assert!(function
        .blocks
        .iter()
        .any(
            |block| block.instructions.iter().any(|instruction| matches!(
                instruction,
                Instruction::Assign {
                    value: Rvalue::Call {
                        callee: CallTarget::Member { field, .. },
                        ..
                    },
                    ..
                } if field == "to_ms"
            ))
        ));
}

#[test]
fn json_dumps_omitted_indent_materializes_option_none_in_checked_mir() {
    let module = crate::lower_source_to_mir(
        r#"
import json

def render(value: json.Value) -> String:
    return json.dumps(value)
"#,
    )
    .expect("json.dumps should lower with its omitted indent default");
    let render = module
        .functions
        .iter()
        .find(|function| function.name == "render")
        .expect("render should lower");

    assert!(render.blocks.iter().any(|block| {
        block.instructions.iter().any(|instruction| {
            matches!(
                instruction,
                Instruction::Assign {
                    value: Rvalue::EnumVariant { enum_name, variant_name, .. },
                    ..
                } if enum_name == "Option" && variant_name == "None"
            )
        })
    }));
    assert!(render.blocks.iter().any(|block| {
        block.instructions.iter().any(|instruction| {
            matches!(
                instruction,
                Instruction::Assign {
                    value: Rvalue::Call {
                        callee: CallTarget::Name(name),
                        args,
                    },
                    ..
                } if name == "json::dumps" && args.len() == 2
            )
        })
    }));
}

#[test]
fn json_parse_dump_and_accessors_execute_through_the_mir_runtime() {
    let module = crate::lower_source_to_mir(
        r#"
import json

def main():
    match json.parse("{\"z\":1.0,\"f\":1.5,\"items\":[true,null,\"x\"]}"):
        case Result.Ok(value):
            print(json.dumps(value))
            print(json.dumps(value, indent=Option.Some(2)))
            print(json.as_int(json.Value.Int(7)))
            print(json.as_float(json.Value.Int(7)))
        case Result.Err(error):
            print(error)

    match json.parse("1e400"):
        case Result.Ok(value):
            print(value)
        case Result.Err(json.Error.NumberOutOfRange(line, column)):
            print(line)
            print(column)
        case Result.Err(error):
            print(error)
"#,
    )
    .expect("dynamic JSON should lower to MIR");
    let output = crate::run_mir(&module).expect("dynamic JSON should execute through MIR");
    assert_eq!(
        output.stdout,
        "{\"f\":1.5,\"items\":[true,null,\"x\"],\"z\":1}\n{\n  \"f\": 1.5,\n  \"items\": [\n    true,\n    null,\n    \"x\"\n  ],\n  \"z\": 1\n}\nOption.Some(7)\nOption.None\n1\n1\n"
    );
}

#[test]
fn json_named_and_default_arguments_preserve_source_evaluation_order() {
    let module = crate::lower_source_to_mir(
        r#"
import json

def value_arg() -> json.Value:
    print("value")
    return json.Value.Null

def indent_arg() -> Option[int64]:
    print("indent")
    return Option.None

def main():
    indent = Option.Some(2)
    print(json.dumps(indent=indent_arg(), value=value_arg()))
    print(json.dumps(json.Value.Null, indent=indent))
    print(indent)
"#,
    )
    .expect("named JSON arguments should lower");
    let output = crate::run_mir(&module).expect("named JSON arguments should execute");
    assert_eq!(output.stdout, "indent\nvalue\nnull\nnull\nOption.Some(2)\n");
}

#[test]
fn json_owned_accessors_accept_rvalue_temporaries() {
    let module = crate::lower_source_to_mir(
        r#"
import json

def main():
    print(json.into_string(json.Value.String("temporary")))
    print(json.into_array(json.Value.Array([json.Value.Null])))
    print(json.into_object(json.Value.Object({"k": json.Value.Bool(true)})))
"#,
    )
    .expect("owned JSON accessors should accept rvalue temporaries");
    let output = crate::run_mir(&module).expect("owned JSON temporaries should execute");
    assert_eq!(
        output.stdout,
        "Option.Some(temporary)\nOption.Some([json.Value.Null])\nOption.Some({k: json.Value.Bool(true)})\n"
    );
}

#[test]
fn json_owned_accessors_lower_noncopy_places_without_snapshot_clones() {
    let module = crate::lower_source_to_mir(
        r#"
import json

def extract(value: own json.Value) -> Option[String]:
    return json.into_string(value)
"#,
    )
    .expect("owned JSON accessors should lower");
    let extract = module
        .functions
        .iter()
        .find(|function| function.name == "extract")
        .expect("extract should lower");

    let argument = extract
        .blocks
        .iter()
        .flat_map(|block| &block.instructions)
        .find_map(|instruction| match instruction {
            Instruction::Assign {
                value:
                    Rvalue::Call {
                        callee: CallTarget::Name(name),
                        args,
                    },
                ..
            } if name == "json::into_string" => args.first(),
            _ => None,
        })
        .expect("json.into_string call should be present");
    assert_eq!(
        argument.value,
        Operand::MovePlace("value".to_string()),
        "an own non-copy place must reach the consuming adapter as an explicit move"
    );
    assert!(
        extract.blocks.iter().all(|block| {
            block.instructions.iter().all(|instruction| {
                !matches!(
                    instruction,
                    Instruction::Assign {
                        value: Rvalue::Use(Operand::Place(place)),
                        ..
                    } if place == "value"
                )
            })
        }),
        "MIR must not snapshot-clone an own json.Value before extraction"
    );
}

#[test]
fn noncopy_value_flow_lowers_to_explicit_moves_while_copy_payloads_stay_reads() {
    let module = crate::lower_source_to_mir(
        r#"
import json

class Holder:
    value: json.Value

def relay(value: own json.Value) -> json.Value:
    assigned = value
    return assigned

def main():
    text = "payload"
    encoded = json.Value.String(text)
    holder = Holder(encoded)
    relayed = relay(holder.value)
    values = [relayed]
    timeout = 2s
    wrapped = Option.Some(timeout)
    print(timeout)
    print(wrapped)
    print(values)
"#,
    )
    .expect("owned and copy value flow should lower");

    let relay = module
        .functions
        .iter()
        .find(|function| function.name == "relay")
        .expect("relay should lower");
    assert!(relay.blocks.iter().any(|block| {
        block.instructions.iter().any(|instruction| {
            matches!(
                instruction,
                Instruction::Assign {
                    target,
                    value: Rvalue::Use(Operand::MovePlace(place)),
                } if target == "assigned" && place == "value"
            )
        })
    }));
    assert!(relay.blocks.iter().any(|block| {
        matches!(
            &block.terminator,
            Terminator::Return(Operand::MovePlace(place)) if place == "assigned"
        )
    }));

    let main = module
        .functions
        .iter()
        .find(|function| function.name == "main")
        .expect("main should lower");
    let rvalues = main
        .blocks
        .iter()
        .flat_map(|block| &block.instructions)
        .filter_map(|instruction| match instruction {
            Instruction::Assign { value, .. } => Some(value),
            _ => None,
        })
        .collect::<Vec<_>>();

    assert!(rvalues.iter().any(|rvalue| {
        matches!(
            rvalue,
            Rvalue::EnumVariant {
                enum_name,
                variant_name,
                payloads,
            } if enum_name == "json.Value"
                && variant_name == "String"
                && payloads == &vec![Operand::MovePlace("text".to_string())]
        )
    }));
    assert!(rvalues.iter().any(|rvalue| {
        matches!(
            rvalue,
            Rvalue::Construct { class_name, fields }
                if class_name == "Holder"
                    && fields.iter().any(|field| {
                        field.name == "value"
                            && field.value == Operand::MovePlace("encoded".to_string())
                    })
        )
    }));
    assert!(rvalues.iter().any(|rvalue| {
        matches!(
            rvalue,
            Rvalue::Call {
                callee: CallTarget::Name(name),
                args,
            } if name == "relay"
                && args.first().is_some_and(|argument| {
                    argument.value == Operand::MovePlace("holder.value".to_string())
                })
        )
    }));
    assert!(rvalues.iter().any(|rvalue| {
        matches!(
            rvalue,
            Rvalue::VecLiteral { elements, .. }
                if elements == &vec![Operand::MovePlace("relayed".to_string())]
        )
    }));
    assert!(rvalues.iter().any(|rvalue| {
        matches!(
            rvalue,
            Rvalue::EnumVariant {
                enum_name,
                variant_name,
                payloads,
            } if enum_name == "Option"
                && variant_name == "Some"
                && payloads
                    .first()
                    .is_some_and(|payload| matches!(payload, Operand::Place(_)))
                && payloads
                    .iter()
                    .all(|payload| !matches!(payload, Operand::MovePlace(_)))
        )
    }));
}

#[test]
fn consuming_match_uses_a_private_owner_and_destructive_payload_operands() {
    let module = crate::lower_source_to_mir(
        r#"
enum Packet:
    Text(String)

def unwrap(packet: own Packet) -> String:
    match packet:
        case Packet.Text(text):
            return text
"#,
    )
    .expect("consuming match should lower");
    let unwrap = module
        .functions
        .iter()
        .find(|function| function.name == "unwrap")
        .expect("unwrap should lower");

    assert!(unwrap.blocks.iter().any(|block| {
        block.instructions.iter().any(|instruction| {
            matches!(
                instruction,
                Instruction::Assign {
                    value: Rvalue::Use(Operand::MovePlace(place)),
                    ..
                } if place == "packet"
            )
        })
    }));
    assert!(unwrap.blocks.iter().any(|block| {
        block.instructions.iter().any(|instruction| {
            matches!(
                instruction,
                Instruction::Assign {
                    value: Rvalue::VariantPayload {
                        scrutinee: Operand::MovePlace(_),
                        index: 0,
                        ..
                    },
                    ..
                }
            )
        })
    }));
}

#[test]
fn own_user_and_trait_receivers_lower_as_explicit_moves() {
    let module = crate::lower_source_to_mir(
        r#"
class DirectBox:
    value: String

    def take(own self) -> String:
        return self.value

trait Take:
    def take(own self) -> String

class TraitBox:
    value: String

impl Take for TraitBox:
    def take(own self) -> String:
        return self.value

def main():
    direct = DirectBox("direct")
    print(direct.take())
    trait_value = TraitBox("trait")
    print(trait_value.take())
"#,
    )
    .expect("own receiver calls should lower");
    let main = module
        .functions
        .iter()
        .find(|function| function.name == "main")
        .expect("main should lower");
    let moved_receivers = main
        .blocks
        .iter()
        .flat_map(|block| &block.instructions)
        .filter_map(|instruction| match instruction {
            Instruction::Assign {
                value:
                    Rvalue::Call {
                        callee:
                            CallTarget::Member {
                                object: Operand::MovePlace(place),
                                field,
                                ..
                            },
                        ..
                    },
                ..
            } if field == "take" => Some(place.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(moved_receivers, vec!["direct", "trait_value"]);
}

#[test]
fn queue_and_owned_collection_iteration_lower_destructive_yields() {
    let module = crate::lower_source_to_mir(
        r#"
def consume_vector(values: own Vec[String]):
    for value in own values:
        print(value)

def consume_set(values: own Set[String]):
    for value in own values:
        print(value)

def consume_queue(values: Queue[String]):
    for value in values:
        print(value)
"#,
    )
    .expect("owned collection and queue iteration should lower");

    for function_name in ["consume_vector", "consume_set"] {
        let function = module
            .functions
            .iter()
            .find(|function| function.name == function_name)
            .unwrap_or_else(|| panic!("{function_name} should lower"));
        assert!(
            function.blocks.iter().any(|block| {
                block.instructions.iter().any(|instruction| {
                    matches!(
                        instruction,
                        Instruction::Assign {
                            value:
                                Rvalue::Call {
                                    callee:
                                        CallTarget::Member {
                                            object: Operand::MovePlace(_),
                                            field,
                                            receiver_place: Some(_),
                                        },
                                    ..
                                },
                            ..
                        } if field == "__take_index_option"
                    )
                })
            }),
            "{function_name} must destructively take from its private collection owner"
        );
        assert!(function.blocks.iter().any(|block| {
            block.instructions.iter().any(|instruction| {
                matches!(
                    instruction,
                    Instruction::Assign {
                        value: Rvalue::VariantPayload {
                            scrutinee: Operand::MovePlace(_),
                            index: 0,
                            ..
                        },
                        ..
                    }
                )
            })
        }));
    }

    let queue = module
        .functions
        .iter()
        .find(|function| function.name == "consume_queue")
        .expect("consume_queue should lower");
    assert!(queue.blocks.iter().any(|block| {
        block.instructions.iter().any(|instruction| {
            matches!(
                instruction,
                Instruction::Assign {
                    value: Rvalue::VariantPayload {
                        scrutinee: Operand::MovePlace(_),
                        index: 0,
                        ..
                    },
                    ..
                }
            )
        })
    }));
}

#[test]
fn task_group_captures_use_owned_operands_for_bare_and_own_target_params() {
    let module = crate::lower_source_to_mir(
        r#"
def shared_worker(value: String):
    print(value)

def own_worker(value: own String):
    print(value)

def main():
    shared_value = "shared-capture"
    own_value = "own-capture"
    with TaskGroup() as group:
        group.start_soon(shared_worker, shared_value)
        group.start_soon(own_worker, own_value)
"#,
    )
    .expect("TaskGroup captures should lower");
    let main = module
        .functions
        .iter()
        .find(|function| function.name == "main")
        .expect("main should lower");
    let captures = main
        .blocks
        .iter()
        .flat_map(|block| &block.instructions)
        .filter_map(|instruction| match instruction {
            Instruction::Assign {
                value: Rvalue::StartTask { args, .. },
                ..
            } => Some(args),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(captures.len(), 2);
    assert_eq!(
        captures[0][0].value,
        Operand::MovePlace("shared_value".to_string())
    );
    assert_eq!(
        captures[1][0].value,
        Operand::MovePlace("own_value".to_string())
    );
    assert!(captures.iter().all(|args| args.len() == 1
        && args[0].name.is_none()
        && args[0].writeback_place.is_none()));
}

#[test]
fn task_group_start_records_copyability_of_the_result_type() {
    let module = crate::lower_source_to_mir(
        r#"
def duration_worker() -> Duration:
    return Duration.ms(1)

def queue_worker() -> Queue[int32]:
    return Queue[int32]()

def string_worker() -> String:
    return "value"

def vector_worker() -> Vec[int32]:
    return [1]

def main():
    with TaskGroup() as group:
        group.start(duration_worker)
        group.start(queue_worker)
        group.start(string_worker)
        group.start(vector_worker)
"#,
    )
    .expect("task result copyability should lower");
    let main = module
        .functions
        .iter()
        .find(|function| function.name == "main")
        .expect("main should lower");
    let starts = main
        .blocks
        .iter()
        .flat_map(|block| &block.instructions)
        .filter_map(|instruction| match instruction {
            Instruction::Assign {
                value:
                    Rvalue::StartTask {
                        function,
                        result_is_copy,
                        ..
                    },
                ..
            } => Some((function.as_str(), *result_is_copy)),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(
        starts,
        vec![
            ("duration_worker", true),
            ("queue_worker", true),
            ("string_worker", false),
            ("vector_worker", false),
        ]
    );
}

#[test]
fn retained_process_and_http_builtin_arguments_lower_with_owned_operands() {
    let module = crate::lower_source_to_mir(
        r#"
import process
import net

def supervise(supervisor: process.Supervisor, name: own String, command: own Vec[String], cwd: own Option[String], environment: own Map[String, String], stdin: own process.Stdio, stdout: own process.Stdio, stderr: own process.Stdio, restart: own process.RestartPolicy, backoff: own Duration, max_restarts: own int32, group: own bool):
    supervisor.start(name=name, command=command, cwd=cwd, env=environment, stdin=stdin, stdout=stdout, stderr=stderr, restart=restart, backoff=backoff, max_restarts=max_restarts, group=group)

def respond_text(exchange: net.HttpExchange, status: int32, text: own String, headers: own Map[String, String]):
    exchange.respond_text(status=status, text=text, headers=headers)

def respond_bytes(exchange: net.HttpExchange, status: int32, bytes: own Vec[uint8], headers: own Map[String, String]):
    exchange.respond_bytes(status=status, bytes=bytes, headers=headers)
"#,
    )
    .expect("retained process and HTTP builtin arguments should lower");

    let member_args = |function_name: &str, member_name: &str| {
        module
            .functions
            .iter()
            .find(|function| function.name == function_name)
            .unwrap_or_else(|| panic!("{function_name} should lower"))
            .blocks
            .iter()
            .flat_map(|block| &block.instructions)
            .find_map(|instruction| match instruction {
                Instruction::Assign {
                    value:
                        Rvalue::Call {
                            callee: CallTarget::Member { field, .. },
                            args,
                        },
                    ..
                } if field == member_name => Some(args),
                _ => None,
            })
            .unwrap_or_else(|| panic!("{function_name} should call {member_name}"))
    };

    let supervisor_args = member_args("supervise", "start");
    for (name, place) in [
        ("name", "name"),
        ("command", "command"),
        ("cwd", "cwd"),
        ("env", "environment"),
    ] {
        let argument = supervisor_args
            .iter()
            .find(|argument| argument.name.as_deref() == Some(name))
            .unwrap_or_else(|| panic!("process.Supervisor.start should retain {name}"));
        assert_eq!(
            argument.value,
            Operand::MovePlace(place.to_string()),
            "non-copy process.Supervisor.start argument {name} must be transferred"
        );
    }
    for name in [
        "stdin",
        "stdout",
        "stderr",
        "restart",
        "backoff",
        "max_restarts",
        "group",
    ] {
        let argument = supervisor_args
            .iter()
            .find(|argument| argument.name.as_deref() == Some(name))
            .unwrap_or_else(|| panic!("process.Supervisor.start should retain {name}"));
        assert!(
            matches!(argument.value, Operand::Place(_)),
            "copy process.Supervisor.start argument {name} must use a value snapshot"
        );
    }

    for (function_name, member_name, owned) in [
        ("respond_text", "respond_text", ["text", "headers"]),
        ("respond_bytes", "respond_bytes", ["bytes", "headers"]),
    ] {
        let args = member_args(function_name, member_name);
        for place in owned {
            let argument = args
                .iter()
                .find(|argument| argument.name.as_deref() == Some(place))
                .unwrap_or_else(|| panic!("{member_name} should bind {place}"));
            assert_eq!(argument.value, Operand::MovePlace(place.to_string()));
        }
        assert!(matches!(
            args.iter()
                .find(|argument| argument.name.as_deref() == Some("status"))
                .expect("status should bind")
                .value,
            Operand::Place(_)
        ));
    }
}

#[test]
fn json_dump_failures_keep_their_documented_mir_trap_codes() {
    let module = crate::lower_source_to_mir(
        r#"
import json

def main():
    print(json.dumps(json.Value.Null, indent=Option.Some(17)))
"#,
    )
    .expect("invalid runtime indent should still lower");
    let error = crate::run_mir(&module).expect_err("invalid JSON indent should trap");
    assert_eq!(error.code, "AU4003");
    assert!(error.message.contains("between 0 and 16"));
}

#[test]
fn random_rng_constructor_and_projected_shuffle_lower_with_mutable_writeback() {
    let module = crate::lower_source_to_mir(
        r#"
import random

class Item:
    label: String

class Holder:
    values: Vec[Item]

def main() -> int32:
    mut rng = random.Rng(seed=42)
    mut holder = Holder([Item("a"), Item("b"), Item("c")])
    rng.shuffle(values=holder.values)
    return 0
"#,
    )
    .expect("Randomness constructor and projected shuffle should lower to MIR");
    let main = module
        .functions
        .iter()
        .find(|function| function.name == "main")
        .expect("main should lower");

    assert!(
        main.local_types
            .iter()
            .any(|local| local.ty == Type::named("random.Rng")),
        "random.Rng constructor temporaries must retain canonical module provenance"
    );
    assert!(
        main.local_types
            .iter()
            .all(|local| local.ty != Type::named("Rng")),
        "random.Rng must never be lowered as a bare-name builtin type"
    );

    assert!(main.blocks.iter().any(|block| {
        block.instructions.iter().any(|instruction| {
            matches!(
                instruction,
                Instruction::Assign {
                    value: Rvalue::Call {
                        callee: CallTarget::Name(name),
                        ..
                    },
                    ..
                } if name == "random::Rng"
            )
        })
    }));

    let shuffle = main
        .blocks
        .iter()
        .flat_map(|block| block.instructions.iter())
        .find_map(|instruction| match instruction {
            Instruction::Assign {
                value:
                    Rvalue::Call {
                        callee:
                            CallTarget::Member {
                                field,
                                receiver_place,
                                ..
                            },
                        args,
                    },
                ..
            } if field == "shuffle" => Some((receiver_place, args)),
            _ => None,
        })
        .expect("shuffle call should lower");
    assert_eq!(shuffle.0.as_deref(), Some("rng"));
    assert_eq!(shuffle.1.len(), 1);
    assert_eq!(shuffle.1[0].name.as_deref(), Some("values"));
    assert_eq!(
        shuffle.1[0].writeback_place.as_deref(),
        Some("holder.values")
    );
}

#[test]
fn path_named_random_keeps_local_and_imported_user_rng_classes_out_of_builtin_lowering() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/run-pass/random.au");
    let program = crate::check_path(&path)
        .expect("a user Rng in an entry module named random should type check normally");
    assert!(!program.classes["Rng"].is_builtin);

    let module = crate::lower_path_to_mir(&path)
        .expect("local and imported user Rng classes should lower as ordinary classes");
    assert!(module.classes.iter().any(|class| class.name == "Rng"));
    assert!(module
        .classes
        .iter()
        .any(|class| class.name == "user_rng_origin_support.random.Rng"));

    let main = module
        .functions
        .iter()
        .find(|function| function.name == "main")
        .expect("fixture main should lower");
    let instructions = main
        .blocks
        .iter()
        .flat_map(|block| block.instructions.iter())
        .collect::<Vec<_>>();
    assert!(instructions.iter().any(|instruction| matches!(
        instruction,
        Instruction::Assign {
            value: Rvalue::Construct { class_name, .. },
            ..
        } if class_name == "Rng"
    )));
    assert!(instructions.iter().any(|instruction| matches!(
        instruction,
        Instruction::Assign {
            value: Rvalue::Construct { class_name, .. },
            ..
        } if class_name == "user_rng_origin_support.random.Rng"
    )));
    assert!(!instructions.iter().any(|instruction| matches!(
        instruction,
        Instruction::Assign {
            value: Rvalue::Call {
                callee: CallTarget::Name(name),
                ..
            },
            ..
        } if name == "random::Rng"
    )));
}

#[test]
fn duration_builtin_operator_matrix_takes_precedence_over_traits() {
    let duration = Type::named("Duration");
    let int64 = Type::named("int64");
    let int32 = Type::named("int32");

    for op in [
        BinaryOp::Add,
        BinaryOp::Sub,
        BinaryOp::Eq,
        BinaryOp::NotEq,
        BinaryOp::Less,
        BinaryOp::LessEq,
        BinaryOp::Greater,
        BinaryOp::GreaterEq,
    ] {
        assert!(is_builtin_binary_operator(op, &duration, &duration));
    }
    assert!(is_builtin_binary_operator(BinaryOp::Mul, &duration, &int64));
    assert!(is_builtin_binary_operator(BinaryOp::Mul, &int64, &duration));
    assert!(is_builtin_binary_operator(
        BinaryOp::FloorDiv,
        &duration,
        &int64
    ));

    for (op, left, right) in [
        (BinaryOp::Div, &duration, &duration),
        (BinaryOp::Mod, &duration, &duration),
        (BinaryOp::FloorDiv, &int64, &duration),
        (BinaryOp::Mul, &duration, &int32),
    ] {
        assert!(!is_builtin_binary_operator(op, left, right));
    }
}

#[test]
fn duration_operators_with_integer_literals_keep_heterogeneous_builtin_types() {
    let source = r#"
def main() -> int32:
    print(1ms // 0)
    print(3 * 1ms)
    print((3 * 1ms).to_ms())
    return 0
"#;
    let program = crate::check_source(source).expect("Duration literal operators should check");
    let module = lower(&program);
    let main = module
        .functions
        .iter()
        .find(|function| function.name == "main")
        .expect("main should lower");

    assert!(main.blocks.iter().any(|block| {
        block.instructions.iter().any(|instruction| {
            matches!(
                instruction,
                Instruction::Assign {
                    value: Rvalue::Binary {
                        op: BinaryOp::FloorDiv,
                        ..
                    },
                    ..
                }
            )
        })
    }));
    assert!(!main.blocks.iter().any(|block| {
        block.instructions.iter().any(|instruction| {
            matches!(
                instruction,
                Instruction::Assign {
                    value: Rvalue::Call {
                        callee: CallTarget::Member { field, .. },
                        ..
                    },
                    ..
                } if field == "floor_div"
            )
        })
    }));
    assert!(main.blocks.iter().any(|block| {
        block.instructions.iter().any(|instruction| {
            matches!(
                instruction,
                Instruction::Assign {
                    value: Rvalue::Call {
                        callee: CallTarget::Member { field, .. },
                        ..
                    },
                    ..
                } if field == "to_ms"
            )
        })
    }));
}

#[test]
fn d6_mir_uses_declaration_resolved_parameter_conventions() {
    let module = crate::lower_source_to_mir(
        r#"
def modes(copy_value: int32, inferred: String, owned: own String, shared: borrow String, mutable: borrow mut String):
    pass

def generic[T](value: T):
    pass

def main() -> int32:
    generic[int32](1)
    return 0
"#,
    )
    .expect("D6 parameter modes should lower to MIR");

    let modes = module
        .functions
        .iter()
        .find(|function| function.name == "modes")
        .expect("modes should lower");
    assert_eq!(
        modes
            .params
            .iter()
            .map(|param| param.passing)
            .collect::<Vec<_>>(),
        vec![
            MirReceiverKind::Value,
            MirReceiverKind::Borrow,
            MirReceiverKind::Value,
            MirReceiverKind::Borrow,
            MirReceiverKind::BorrowMut,
        ]
    );

    let generic = module
        .functions
        .iter()
        .find(|function| function.name == "generic")
        .expect("generic should lower");
    assert_eq!(generic.params[0].passing, MirReceiverKind::Borrow);
}

#[test]
fn d6_shared_default_temporary_lives_through_the_call() {
    let module = crate::lower_source_to_mir(
        r#"
def shared(value: borrow String = "shared") -> String:
    return value.clone()

def owned(value: own String = "owned") -> String:
    return value

def main() -> int32:
    print(shared())
    print(owned())
    return 0
"#,
    )
    .expect("shared and owned defaults should lower");
    let output = crate::run_mir(&module).expect("default temporaries should remain live in calls");
    assert_eq!(output.stdout, "shared\nowned\n");
}

fn named_arg(name: &str, value: Expr) -> Argument {
    Argument {
        name: Some(name.to_string()),
        span: value.span,
        value,
    }
}

fn binding_pattern(name: &str) -> Pattern {
    Pattern::Binding(BindingPattern {
        name: name.to_string(),
        span: Span::new(1, 1),
    })
}

fn variant_pattern(
    enum_name: Option<&str>,
    variant_name: &str,
    subpatterns: Vec<Pattern>,
) -> Pattern {
    Pattern::Variant(VariantPattern {
        enum_name: enum_name.map(str::to_string),
        variant_name: variant_name.to_string(),
        subpatterns,
        span: Span::new(1, 1),
    })
}

fn namespace_from_program(name: &str, path: &str, program: &Program) -> ModuleNamespace {
    ModuleNamespace {
        name: name.to_string(),
        path: path.to_string(),
        source_path: None,
        modules: BTreeMap::new(),
        functions: program.functions.clone(),
        classes: program.classes.clone(),
        enums: program.enums.clone(),
        traits: program.traits.clone(),
        trait_impls: program.trait_impls.clone(),
        all_functions: program.functions.clone(),
        all_classes: program.classes.clone(),
        all_enums: program.enums.clone(),
        all_traits: program.traits.clone(),
        imported_modules: program.imported_modules.clone(),
    }
}

fn lowerer_with_imported_modules() -> Lowerer<'static> {
    let main_source = r#"
def local_helper() -> int32:
    return 1

def main() -> int32:
    return local_helper()
"#;
    let imported_source = r#"
class Thing:
    value: int32
    flag: bool = true

    def zero() -> Thing:
        return Thing(value=0)

    def get(borrow self) -> int32:
        return self.value

enum Status:
    Ok
    Value(int32)

trait RemoteTrait:
    def label(borrow self) -> String

def helper() -> int32:
    return 7
"#;

    let mut program = checked_program(main_source);
    let imported = crate::sema::check_with_context(
        crate::parse_source(imported_source).expect("imported helper source should parse"),
        crate::sema::ModuleContext {
            module_name: "pkg.helpers".to_string(),
            ..crate::sema::ModuleContext::default()
        },
    )
    .expect("imported helper source should type check in its owning module");
    let helpers = namespace_from_program("helpers", "pkg.helpers", &imported);
    let mut reexport = helpers.clone();
    reexport.name = "reexport".to_string();
    reexport.path = "pkg.reexport".to_string();
    reexport.functions.clear();
    reexport.classes.clear();
    reexport.enums.clear();
    let mut pkg = ModuleNamespace {
        name: "pkg".to_string(),
        path: "pkg".to_string(),
        source_path: None,
        modules: BTreeMap::from([
            ("helpers".to_string(), helpers.clone()),
            ("reexport".to_string(), reexport.clone()),
        ]),
        functions: BTreeMap::new(),
        classes: BTreeMap::new(),
        enums: BTreeMap::new(),
        traits: BTreeMap::new(),
        trait_impls: Vec::new(),
        all_functions: BTreeMap::new(),
        all_classes: BTreeMap::new(),
        all_enums: BTreeMap::new(),
        all_traits: BTreeMap::new(),
        imported_modules: BTreeMap::new(),
    };
    pkg.imported_modules
        .insert("helpers".to_string(), helpers.clone());
    pkg.imported_modules
        .insert("reexport".to_string(), reexport.clone());

    let mut current = ModuleNamespace {
        name: "main".to_string(),
        path: "pkg.main".to_string(),
        source_path: None,
        modules: BTreeMap::new(),
        functions: program.functions.clone(),
        classes: program.classes.clone(),
        enums: program.enums.clone(),
        traits: program.traits.clone(),
        trait_impls: program.trait_impls.clone(),
        all_functions: program.functions.clone(),
        all_classes: program.classes.clone(),
        all_enums: program.enums.clone(),
        all_traits: program.traits.clone(),
        imported_modules: BTreeMap::from([("pkg".to_string(), pkg.clone())]),
    };
    current
        .all_classes
        .extend(imported.classes.iter().map(|(k, v)| (k.clone(), v.clone())));
    current
        .all_enums
        .extend(imported.enums.iter().map(|(k, v)| (k.clone(), v.clone())));
    current
        .all_traits
        .extend(imported.traits.iter().map(|(k, v)| (k.clone(), v.clone())));
    current.all_functions.extend(
        imported
            .functions
            .iter()
            .map(|(k, v)| (k.clone(), v.clone())),
    );

    program.module_name = "<root>".to_string();
    program.imported_modules = BTreeMap::from([("pkg".to_string(), pkg.clone())]);
    program.module_registry = BTreeMap::from([
        ("pkg".to_string(), pkg),
        ("pkg.helpers".to_string(), helpers),
        ("pkg.reexport".to_string(), reexport),
        ("pkg.main".to_string(), current),
    ]);

    let program = Box::leak(Box::new(program));
    Lowerer::new(
        program,
        "main",
        "pkg.main",
        Type::named("int32"),
        BTreeMap::new(),
    )
}

fn trait_lowerer() -> Lowerer<'static> {
    let source = r#"
trait Add[Rhs, Out]:
    def add(borrow self, rhs: own Rhs) -> Out

trait Neg[Out]:
    def neg(borrow self) -> Out

trait Named:
    def name(borrow self) -> String

trait Reset:
    def reset(borrow mut self)

class User:
    label: String

class Counter:
    value: int32

    def bump(borrow mut self):
        self.value += 1

class Box[T]:
    value: T

enum Status:
    Value(int32)

def make_flag() -> bool:
    return true

impl Named for User:
    def name(borrow self) -> String:
        return self.label.clone()

impl Reset for User:
    def reset(borrow mut self):
        self.label = ""

impl Add[int32, bool] for User:
    def add(borrow self, rhs: own int32) -> bool:
        return rhs > 0

impl Neg[String] for User:
    def neg(borrow self) -> String:
        return self.label.clone()

impl[T: Named] Add[Box[T], Box[T]] for Box[T]:
    def add(borrow self, rhs: own Box[T]) -> Box[T]:
        return rhs
"#;
    let program = Box::leak(Box::new(checked_program(source)));
    Lowerer::new(
        program,
        "main",
        &program.module_name,
        Type::named("int32"),
        BTreeMap::from([
            (
                "T".to_string(),
                vec![TraitBound {
                    trait_name: binary_operator_trait(BinaryOp::Add)
                        .expect("add trait should exist")
                        .0
                        .to_string(),
                    trait_args: vec![Type::named("int32"), Type::named("bool")],
                }],
            ),
            (
                "U".to_string(),
                vec![TraitBound {
                    trait_name: unary_operator_trait(UnaryOp::Neg)
                        .expect("neg trait should exist")
                        .0
                        .to_string(),
                    trait_args: vec![Type::named("String")],
                }],
            ),
        ]),
    )
}

fn function_names(module: &MirModule) -> BTreeSet<String> {
    module
        .functions
        .iter()
        .map(|function| function.name.clone())
        .collect()
}

#[test]
fn mir_helper_functions_cover_builtin_ops_and_type_lowering() {
    let enum_program =
        checked_program("enum Flag:\n    Ok\n\ndef main() -> int32:\n    return 0\n");
    assert!(is_known_enum_name(&enum_program, "Flag"));
    assert!(is_known_enum_name(
        &checked_program("def main() -> int32:\n    return 0\n"),
        "Option"
    ));
    assert!(!is_known_enum_name(
        &checked_program("def main() -> int32:\n    return 0\n"),
        "Missing"
    ));

    assert!(is_builtin_unary_operator(
        UnaryOp::Not,
        &Type::named("bool")
    ));
    assert!(is_builtin_unary_operator(
        UnaryOp::Neg,
        &Type::named("float64")
    ));
    assert!(!is_builtin_unary_operator(
        UnaryOp::Not,
        &Type::named("int32")
    ));

    assert!(is_builtin_binary_operator(
        BinaryOp::Add,
        &Type::named("int32"),
        &Type::named("int32")
    ));
    assert!(is_builtin_binary_operator(
        BinaryOp::Add,
        &Type::named("String"),
        &Type::named("String")
    ));
    assert!(is_builtin_binary_operator(
        BinaryOp::And,
        &Type::named("bool"),
        &Type::named("bool")
    ));
    assert!(!is_builtin_binary_operator(
        BinaryOp::Add,
        &Type::named("int32"),
        &Type::named("float64")
    ));

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
    collect_type_params_from_type(&Type::Unit, &mut collected);
    collect_type_params_from_type(&Type::Module("pkg".to_string()), &mut collected);
    assert_eq!(
        collected,
        BTreeSet::from(["K".to_string(), "V".to_string()])
    );

    let (left_ty, right_ty) = adjusted_binary_operand_types(
        &expr(ExprKind::Int(1)),
        Type::named("int32"),
        &expr(ExprKind::Float(1.0)),
        Type::named("float64"),
    );
    assert_eq!(left_ty, Type::named("float64"));
    assert_eq!(right_ty, Type::named("float64"));

    assert_eq!(default_return_operand(&Type::Unit), Operand::Unit);
    assert_eq!(
        default_return_operand(&Type::named("bool")),
        Operand::Bool(false)
    );
    assert_eq!(
        default_return_operand(&Type::named("float64")),
        Operand::Float(0.0)
    );
    assert_eq!(
        default_return_operand(&Type::named("String")),
        Operand::String(String::new())
    );
    assert_eq!(
        default_return_operand(&Type::named("Duration")),
        Operand::Duration(0)
    );
    assert_eq!(
        default_return_operand(&Type::named("int32")),
        Operand::Int(0)
    );
    assert_eq!(default_return_operand(&Type::named("Thing")), Operand::Unit);

    assert_eq!(
        lower_receiver_kind(ReceiverKind::BorrowMut),
        MirReceiverKind::BorrowMut
    );
    assert_eq!(
        lower_receiver_kind(ReceiverKind::Value),
        MirReceiverKind::Value
    );
    assert_eq!(
        lower_receiver_kind(ReceiverKind::Borrow),
        MirReceiverKind::Borrow
    );
    assert_eq!(
        imported_module_function_name("pkg.tools", "work"),
        "pkg.tools::work"
    );
    assert_eq!(
        format_trait_args(&[Type::named("int32"), Type::named("String")]),
        "[int32, String]"
    );
    assert_eq!(format_trait_args(&[]), "");

    assert_eq!(lower_type_ref(&type_ref("None")), Type::Unit);
    assert_eq!(lower_type_ref(&type_ref("str")), Type::named("String"));
    assert_eq!(
        lower_type_ref(&TypeRef::named(
            "Vec",
            vec![type_ref("int32")],
            false,
            Span::new(1, 1),
        )),
        Type::Named("Vec".to_string(), vec![Type::named("int32")])
    );
}

#[test]
fn lowerer_module_resolution_and_rendering_helpers_cover_imported_paths() {
    let mut lowerer = lowerer_with_imported_modules();
    lowerer
        .local_types
        .insert("pkg".to_string(), Type::Module("pkg".to_string()));

    assert_eq!(
        lowerer
            .current_module_namespace()
            .map(|namespace| namespace.path.as_str()),
        Some("pkg.main")
    );
    assert_eq!(
        lowerer
            .module_namespace("pkg.helpers")
            .map(|namespace| namespace.path.as_str()),
        Some("pkg.helpers")
    );
    assert_eq!(
        lowerer
            .trait_info_in_scope("RemoteTrait")
            .map(|info| info.decl.name.as_str()),
        Some("RemoteTrait")
    );
    let mut imported_only_root = ModuleNamespace {
        name: "pkg".to_string(),
        path: "pkg".to_string(),
        source_path: None,
        modules: BTreeMap::new(),
        functions: BTreeMap::new(),
        classes: BTreeMap::new(),
        enums: BTreeMap::new(),
        traits: BTreeMap::new(),
        trait_impls: Vec::new(),
        all_functions: BTreeMap::new(),
        all_classes: BTreeMap::new(),
        all_enums: BTreeMap::new(),
        all_traits: BTreeMap::new(),
        imported_modules: BTreeMap::new(),
    };
    imported_only_root.imported_modules.insert(
        "helpers".to_string(),
        lowerer.program.module_registry["pkg.helpers"].clone(),
    );
    assert_eq!(
        Lowerer::find_namespace_in_modules(
            &BTreeMap::from([("pkg".to_string(), imported_only_root)]),
            "pkg.helpers",
        )
        .map(|namespace| namespace.path.as_str()),
        Some("pkg.helpers")
    );
    assert_eq!(
        lowerer
            .infer_module_path(&member_expr(name_expr("pkg"), "helpers"))
            .as_deref(),
        Some("pkg.helpers")
    );
    assert_eq!(
        lowerer.qualified_module_item(&member_expr(
            member_expr(name_expr("pkg"), "helpers"),
            "Thing"
        )),
        Some(("pkg.helpers".to_string(), "Thing".to_string()))
    );
    assert_eq!(
        lowerer
            .resolve_function_info("local_helper")
            .map(|info| info.decl.name.as_str()),
        Some("local_helper")
    );
    assert_eq!(
        lowerer
            .resolve_class_info("pkg.helpers.Thing")
            .map(|info| info.decl.name.as_str()),
        Some("Thing")
    );
    assert_eq!(
        lowerer
            .resolve_class_info("pkg.reexport.Thing")
            .map(|info| info.decl.name.as_str()),
        Some("Thing")
    );
    assert_eq!(
        lowerer
            .resolve_class_info("Thing")
            .map(|info| info.decl.name.as_str()),
        Some("Thing")
    );
    assert_eq!(
        lowerer
            .resolve_enum_info("pkg.helpers.Status")
            .map(|info| info.decl.name.as_str()),
        Some("Status")
    );
    assert_eq!(
        lowerer
            .resolve_enum_info("pkg.reexport.Status")
            .map(|info| info.decl.name.as_str()),
        Some("Status")
    );
    assert_eq!(
        lowerer.resolve_pattern_enum_name(
            &VariantPattern {
                enum_name: Some("pkg.reexport.Status".to_string()),
                variant_name: "Ok".to_string(),
                subpatterns: Vec::new(),
                span: Span::new(1, 1),
            },
            None,
        ),
        "pkg.helpers.Status"
    );
    assert_eq!(
        lowerer.render_assign_target(&crate::ast::AssignTarget::Name("value".to_string())),
        "value".to_string()
    );
    assert_eq!(
        lowerer.render_expr_place(&member_expr(name_expr("pkg"), "helpers")),
        "pkg.helpers".to_string()
    );
    assert_eq!(
        lowerer.render_place_expr_option(&name_expr("value")),
        Some("value".to_string())
    );
    assert_eq!(
        lowerer.render_place_expr_option(&expr(ExprKind::Int(1))),
        None
    );
    assert_eq!(
        lowerer.infer_expr_type(&expr(ExprKind::Call {
            callee: Box::new(member_expr(name_expr("pkg"), "helpers")),
            args: Vec::new(),
        })),
        Some(Type::Module("pkg.helpers".to_string()))
    );
    assert_eq!(
        lowerer.infer_expr_type(&expr(ExprKind::Call {
            callee: Box::new(member_expr(
                member_expr(name_expr("pkg"), "helpers"),
                "helper",
            )),
            args: Vec::new(),
        })),
        Some(Type::named("int32"))
    );
    assert_eq!(
        lowerer.infer_expr_type(&expr(ExprKind::Call {
            callee: Box::new(member_expr(
                member_expr(name_expr("pkg"), "helpers"),
                "Thing",
            )),
            args: Vec::new(),
        })),
        Some(Type::named("pkg.helpers.Thing"))
    );
    assert_eq!(
        lowerer.infer_expr_type(&expr(ExprKind::Call {
            callee: Box::new(member_expr(
                member_expr(name_expr("pkg"), "helpers"),
                "Status",
            )),
            args: Vec::new(),
        })),
        Some(Type::named("pkg.helpers.Status"))
    );
    for (builtin_name, args) in [
        ("Option", vec![type_ref("int32")]),
        ("Result", vec![type_ref("int32"), type_ref("String")]),
        ("SendError", vec![type_ref("int32")]),
        ("Queue", vec![type_ref("String")]),
        ("Vec", vec![type_ref("int32")]),
        ("Set", vec![type_ref("String")]),
        ("Map", vec![type_ref("String"), type_ref("int32")]),
    ] {
        assert_eq!(
            lowerer.infer_expr_type(&expr(ExprKind::Specialize {
                expr: Box::new(name_expr(builtin_name)),
                type_args: args.clone(),
            })),
            Some(Type::Named(
                builtin_name.to_string(),
                args.into_iter().map(|arg| lower_type_ref(&arg)).collect(),
            )),
            "{builtin_name} specialization should infer a builtin generic type"
        );
    }
    assert_eq!(
        lowerer.infer_expr_type(&expr(ExprKind::Specialize {
            expr: Box::new(expr(ExprKind::Int(7))),
            type_args: Vec::new(),
        })),
        Some(Type::named("int64"))
    );
    assert_eq!(
        lowerer.infer_expr_type(&expr(ExprKind::Try(Box::new(expr(ExprKind::Int(1)))))),
        None
    );
    assert_eq!(
        lowerer.infer_expr_type(&expr(ExprKind::Unary {
            op: UnaryOp::Neg,
            expr: Box::new(expr(ExprKind::Bool(true))),
        })),
        None
    );
    assert_eq!(
        lowerer.infer_expr_type(&expr(ExprKind::Call {
            callee: Box::new(name_expr("wait_any")),
            args: vec![arg(expr(ExprKind::List(vec![expr(ExprKind::Int(1))])))],
        })),
        None
    );
    assert_eq!(
        lowerer.infer_expr_type(&expr(ExprKind::Call {
            callee: Box::new(name_expr("wait_any")),
            args: vec![arg(expr(ExprKind::Bool(true)))],
        })),
        None
    );
    assert_eq!(
        lowerer.infer_expr_type(&expr(ExprKind::Call {
            callee: Box::new(name_expr("wait_all")),
            args: vec![arg(expr(ExprKind::List(vec![expr(ExprKind::Int(1))])))],
        })),
        None
    );
    assert_eq!(
        lowerer.infer_expr_type(&expr(ExprKind::Call {
            callee: Box::new(name_expr("wait_all")),
            args: vec![arg(expr(ExprKind::Bool(true)))],
        })),
        None
    );
    let local_static_target = lowerer
        .resolve_task_start_target(&member_expr(name_expr("Thing"), "zero"))
        .expect("unqualified imported class static methods should resolve");
    assert_eq!(local_static_target.function, "pkg.helpers::Thing.zero");
    let module_static_target = lowerer
        .resolve_task_start_target(&member_expr(
            member_expr(member_expr(name_expr("pkg"), "helpers"), "Thing"),
            "zero",
        ))
        .expect("module-qualified imported class static methods should resolve");
    assert_eq!(module_static_target.function, "pkg.helpers::Thing.zero");
    assert!(
        lowerer
            .resolve_task_start_target(&member_expr(
                member_expr(member_expr(name_expr("pkg"), "helpers"), "Thing"),
                "get",
            ))
            .is_none(),
        "receiver methods are not valid task start targets"
    );
    let module_function_target = lowerer
        .resolve_task_start_target(&member_expr(
            member_expr(name_expr("pkg"), "helpers"),
            "helper",
        ))
        .expect("module-qualified imported functions should resolve");
    assert_eq!(module_function_target.function, "pkg.helpers::helper");
    let reexport_function_target = lowerer
        .resolve_task_start_target(&member_expr(
            member_expr(name_expr("pkg"), "reexport"),
            "helper",
        ))
        .expect("all-functions-only imported functions should resolve");
    assert_eq!(reexport_function_target.function, "pkg.reexport::helper");
    let specialized_local_function = expr(ExprKind::Specialize {
        expr: Box::new(name_expr("local_helper")),
        type_args: Vec::new(),
    });
    assert_eq!(
        lowerer
            .resolve_task_start_target(&specialized_local_function)
            .expect("specialized local functions should resolve as task targets")
            .function,
        "local_helper"
    );
    let specialized_static_target = expr(ExprKind::Specialize {
        expr: Box::new(member_expr(name_expr("Thing"), "zero")),
        type_args: Vec::new(),
    });
    assert_eq!(
        lowerer
            .resolve_task_start_target(&specialized_static_target)
            .expect("specialized static methods should resolve as task targets")
            .function,
        "pkg.helpers::Thing.zero"
    );
    let specialized_class_object = expr(ExprKind::Specialize {
        expr: Box::new(name_expr("Thing")),
        type_args: Vec::new(),
    });
    assert_eq!(
        lowerer
            .resolve_task_start_target(&member_expr(specialized_class_object, "zero"))
            .expect("static methods on specialized class objects should resolve")
            .function,
        "pkg.helpers::Thing.zero"
    );

    let static_call = expr(ExprKind::Call {
        callee: Box::new(member_expr(
            member_expr(member_expr(name_expr("pkg"), "helpers"), "Thing"),
            "zero",
        )),
        args: Vec::new(),
    });
    assert!(matches!(
        lowerer.lower_expr(&static_call),
        Operand::Place(_)
    ));
    assert!(matches!(
        lowerer.lower_expr(&member_expr(
            member_expr(member_expr(name_expr("pkg"), "helpers"), "Status"),
            "Ok",
        )),
        Operand::Place(_)
    ));
    let module_function_call = expr(ExprKind::Call {
        callee: Box::new(member_expr(
            member_expr(name_expr("pkg"), "helpers"),
            "helper",
        )),
        args: Vec::new(),
    });
    assert!(matches!(
        lowerer.lower_expr(&module_function_call),
        Operand::Place(_)
    ));
    let module_enum_variant_call = expr(ExprKind::Call {
        callee: Box::new(member_expr(
            member_expr(member_expr(name_expr("pkg"), "helpers"), "Status"),
            "Value",
        )),
        args: vec![arg(expr(ExprKind::Int(9)))],
    });
    assert!(matches!(
        lowerer.lower_expr(&module_enum_variant_call),
        Operand::Place(_)
    ));
    for builtin_variant in ["TimedOut", "Full", "Item", "Ready"] {
        let builtin_variant_call = expr(ExprKind::Call {
            callee: Box::new(name_expr(builtin_variant)),
            args: Vec::new(),
        });
        assert!(
            matches!(lowerer.lower_expr(&builtin_variant_call), Operand::Place(_)),
            "{builtin_variant} should lower through the builtin enum fallback"
        );
    }
    let constructor_call = expr(ExprKind::Call {
        callee: Box::new(member_expr(
            member_expr(name_expr("pkg"), "helpers"),
            "Thing",
        )),
        args: vec![arg(expr(ExprKind::Int(5)))],
    });
    assert!(matches!(
        lowerer.lower_expr(&constructor_call),
        Operand::Place(_)
    ));
    let unsupported_call = expr(ExprKind::Call {
        callee: Box::new(expr(ExprKind::Int(1))),
        args: Vec::new(),
    });
    assert!(matches!(
        lowerer.lower_expr(&unsupported_call),
        Operand::Place(_)
    ));
    let specialized_value = expr(ExprKind::Specialize {
        expr: Box::new(expr(ExprKind::Int(7))),
        type_args: vec![type_ref("int32")],
    });
    assert_eq!(lowerer.lower_expr(&specialized_value), Operand::Int(7));
    let current_instructions = &lowerer.blocks[lowerer.current_block].instructions;
    assert!(current_instructions.iter().any(|instruction| matches!(
        instruction,
        Instruction::Assign {
            value: Rvalue::Construct {
                class_name,
                fields,
            },
            ..
        } if class_name == "pkg.helpers.Thing"
            && fields.iter().any(|field| field.name == "value")
            && fields.iter().any(|field| field.name == "flag")
    )));
    assert!(current_instructions.iter().any(|instruction| matches!(
        instruction,
        Instruction::Assign {
            value: Rvalue::Call {
                callee: CallTarget::Name(name),
                ..
            },
            ..
        } if name == "pkg.helpers::Thing.zero"
    )));
    assert!(current_instructions.iter().any(|instruction| matches!(
        instruction,
        Instruction::Assign {
            value: Rvalue::Call {
                callee: CallTarget::Name(name),
                ..
            },
            ..
        } if name.starts_with("unsupported<")
    )));

    let first_temp = lowerer.new_temp();
    let typed_temp = lowerer.new_typed_temp(Type::named("String"));
    let temp_for_expr = lowerer.new_temp_for_expr(&expr(ExprKind::String("aurora".to_string())));
    assert!(first_temp.starts_with("%t"));
    assert!(typed_temp.starts_with("%t"));
    assert!(temp_for_expr.starts_with("%t"));
    assert_eq!(
        lowerer.local_types.get(&typed_temp),
        Some(&Type::named("String"))
    );
    assert_eq!(
        lowerer.local_types.get(&temp_for_expr),
        Some(&Type::named("String"))
    );

    let block = lowerer.new_block("branch");
    let label = lowerer.label(block);
    assert!(label.starts_with("main_branch_"));
    assert!(!lowerer.current_terminated());
    lowerer.emit(Instruction::Eval {
        value: Operand::Int(1),
    });
    lowerer.with_stack.push("resource".to_string());
    lowerer.emit_cleanup_range(0, true);
    lowerer.terminate(Terminator::Return(Operand::Int(0)));
    assert!(lowerer.current_terminated());
    lowerer.switch_to(block);
    let function = lowerer.finish(MirFunctionSpec {
        name: "main".to_string(),
        span: Span::new(1, 1),
        receiver: None,
        params: Vec::new(),
        return_type: Type::named("int32"),
        default_return: Operand::Int(0),
    });
    assert_eq!(function.name, "main");
    assert!(function
        .blocks
        .iter()
        .flat_map(|block| block.instructions.iter())
        .any(|instruction| matches!(
            instruction,
            Instruction::PopCleanup {
                place,
                cancel_before_cleanup: true
            } if place == "resource"
        )));
}

#[test]
fn imported_module_class_collection_walks_nested_namespaces() {
    let program = checked_program("def main() -> int32:\n    return 0\n");
    let helpers_program = checked_program(
        "\
class Thing:
    value: int32

    def zero() -> Thing:
        return Thing(value=0)
",
    );
    let nested_program = checked_program(
        "\
class Leaf:
    value: int32

def leaf_helper() -> int32:
    return 5
",
    );
    let nested = namespace_from_program("nested", "pkg.helpers.nested", &nested_program);
    let mut helpers = namespace_from_program("helpers", "pkg.helpers", &helpers_program);
    helpers.modules.insert("nested".to_string(), nested.clone());

    let mut classes = Vec::new();
    let mut functions = Vec::new();
    let mut seen_functions = BTreeSet::new();
    let mut seen_classes = BTreeSet::new();
    push_imported_module_classes_from_namespace(
        &program,
        &helpers,
        &mut classes,
        &mut functions,
        &mut seen_functions,
        &mut seen_classes,
    );

    assert!(classes.iter().any(|class| class.name == "Thing"));
    assert!(classes.iter().any(|class| class.name == "Leaf"));
    assert!(functions
        .iter()
        .any(|function| function.name == "pkg.helpers::Thing.zero"));

    let mut imported_functions = Vec::new();
    let mut seen_imported_functions = BTreeSet::new();
    push_imported_module_functions_from_namespace(
        &program,
        &helpers,
        &mut imported_functions,
        &mut seen_imported_functions,
    );
    assert!(imported_functions
        .iter()
        .any(|function| function.name == "pkg.helpers.nested::leaf_helper"));
}

#[test]
fn imported_trait_impl_collection_deduplicates_equivalent_impls() {
    let program = checked_program("def main() -> int32:\n    return 0\n");
    let trait_program = checked_program(
        r#"
trait Named:
    def name(borrow self) -> String

class User:
    label: String

impl Named for User:
    def name(borrow self) -> String:
        return self.label.clone()
"#,
    );
    let first = namespace_from_program("first", "pkg.first", &trait_program);
    let second = namespace_from_program("second", "pkg.second", &trait_program);
    let mut program = program;
    program.module_registry = BTreeMap::from([
        ("pkg.first".to_string(), first),
        ("pkg.second".to_string(), second),
    ]);

    let mut functions = Vec::new();
    let mut trait_impls = Vec::new();
    let mut seen_functions = BTreeSet::new();
    let mut seen_trait_impls = BTreeSet::new();
    push_imported_module_trait_impls(
        &program,
        &mut functions,
        &mut trait_impls,
        &mut seen_functions,
        &mut seen_trait_impls,
    );

    assert_eq!(trait_impls.len(), 1);
    assert_eq!(functions.len(), 1);
}

#[test]
fn imported_class_and_enum_lookup_rejects_ambiguous_unqualified_names() {
    let mut program = checked_program("def main() -> int32:\n    return 0\n");
    let first_program = checked_program(
        r#"
class Thing:
    value: int32

enum Status:
    Ready
"#,
    );
    let second_program = checked_program(
        r#"
class Thing:
    value: int32

enum Status:
    Ready
"#,
    );
    program.imported_modules = BTreeMap::from([
        (
            "first".to_string(),
            namespace_from_program("first", "first", &first_program),
        ),
        (
            "second".to_string(),
            namespace_from_program("second", "second", &second_program),
        ),
    ]);
    let program = Box::leak(Box::new(program));
    let lowerer = Lowerer::new(
        program,
        "main",
        &program.module_name,
        Type::named("int32"),
        BTreeMap::new(),
    );

    assert!(lowerer.resolve_class_info("first.Thing").is_some());
    assert!(lowerer.resolve_enum_info("first.Status").is_some());
    assert!(lowerer.resolve_class_info("Thing").is_none());
    assert!(lowerer.resolve_enum_info("Status").is_none());
}

#[test]
fn lowerer_trait_and_member_type_helpers_cover_trait_bounds_and_variants() {
    let mut lowerer = trait_lowerer();
    lowerer
        .local_types
        .insert("left".to_string(), Type::TypeParam("T".to_string()));
    lowerer
        .local_types
        .insert("right".to_string(), Type::named("int32"));
    lowerer
        .local_types
        .insert("value".to_string(), Type::TypeParam("U".to_string()));

    assert_eq!(
        lowerer.operator_field_for_binary(BinaryOp::Add, &name_expr("left"), &name_expr("right")),
        binary_operator_trait(BinaryOp::Add).map(|(_, field)| field.to_string())
    );
    assert_eq!(
        lowerer.operator_field_for_unary(UnaryOp::Neg, &name_expr("value")),
        unary_operator_trait(UnaryOp::Neg).map(|(_, field)| field.to_string())
    );
    assert_eq!(
        lowerer.operator_return_type_for_binary(
            &Type::TypeParam("T".to_string()),
            &Type::named("int32"),
            BinaryOp::Add
        ),
        Some(Type::named("bool"))
    );
    assert_eq!(
        lowerer.operator_return_type_for_unary(&Type::TypeParam("U".to_string()), UnaryOp::Neg),
        Some(Type::named("String"))
    );
    assert_eq!(
        lowerer.operator_return_type_for_binary(
            &Type::named("User"),
            &Type::named("int32"),
            BinaryOp::Add
        ),
        Some(Type::named("bool"))
    );
    assert_eq!(
        lowerer.operator_return_type_for_unary(&Type::named("User"), UnaryOp::Neg),
        Some(Type::named("String"))
    );
    assert_eq!(
        lowerer.operator_return_type_for_unary(&Type::named("User"), UnaryOp::Not),
        None
    );

    let named_bound = TraitBound {
        trait_name: "Named".to_string(),
        trait_args: Vec::new(),
    };
    assert!(lowerer.type_implements_trait_bound(&Type::named("User"), &named_bound));
    assert!(!lowerer.type_implements_trait_bound(&Type::named("String"), &named_bound));
    let bounded_box_add_impl = lowerer
        .program
        .trait_impls
        .iter()
        .find(|trait_impl| {
            trait_impl.trait_name == "Add"
                && matches!(&trait_impl.for_type, Type::Named(name, _) if name == "Box")
        })
        .expect("bounded Box Add impl should be present");
    let box_user = Type::Named("Box".to_string(), vec![Type::named("User")]);
    let box_user_add_bound = TraitBound {
        trait_name: "Add".to_string(),
        trait_args: vec![box_user.clone(), box_user.clone()],
    };
    assert!(lowerer
        .trait_impl_substitutions_for_bound(bounded_box_add_impl, &box_user, &box_user_add_bound)
        .is_some());
    let box_string = Type::Named("Box".to_string(), vec![Type::named("String")]);
    let box_string_add_bound = TraitBound {
        trait_name: "Add".to_string(),
        trait_args: vec![box_string.clone(), box_string.clone()],
    };
    assert!(lowerer
        .trait_impl_substitutions_for_bound(
            bounded_box_add_impl,
            &box_string,
            &box_string_add_bound,
        )
        .is_none());
    assert!(lowerer
        .trait_method_for_receiver(&Type::named("User"), "name")
        .is_some());
    assert!(lowerer
        .trait_impl_method_for_class_name("User", "name")
        .is_some());

    let option_string = Type::Named("Option".to_string(), vec![Type::named("String")]);
    assert_eq!(
        lowerer.builtin_enum_variant_type(&option_string, "Some"),
        Some(option_string.clone())
    );
    assert_eq!(
        lowerer.builtin_enum_variant_type(&option_string, "Missing"),
        None
    );
    let send_error_string = Type::Named("SendError".to_string(), vec![Type::named("String")]);
    assert_eq!(
        lowerer.builtin_enum_variant_type(&send_error_string, "Closed"),
        Some(send_error_string.clone())
    );

    assert_eq!(
        lowerer.variant_payload_types(
            Some(&Type::Named(
                "Option".to_string(),
                vec![Type::named("String")]
            )),
            "Option",
            "Some"
        ),
        Some(vec![Type::named("String")])
    );
    assert_eq!(
        lowerer.variant_payload_types(
            Some(&Type::Named(
                "Result".to_string(),
                vec![Type::named("int32"), Type::named("String")]
            )),
            "Result",
            "Err"
        ),
        Some(vec![Type::named("String")])
    );
    assert_eq!(
        lowerer.variant_payload_types(
            Some(&Type::Named(
                "SendError".to_string(),
                vec![Type::named("int32")]
            )),
            "SendError",
            "Closed"
        ),
        Some(vec![Type::named("int32")])
    );
    assert_eq!(
        lowerer.variant_payload_types(
            Some(&Type::Named(
                "TaskResult".to_string(),
                vec![Type::named("bool")]
            )),
            "TaskResult",
            "Ready"
        ),
        Some(vec![Type::named("bool")])
    );
    assert_eq!(
        lowerer.variant_payload_types(
            Some(&Type::Named(
                "WaitAny".to_string(),
                vec![Type::named("String")]
            )),
            "WaitAny",
            "Ready"
        ),
        Some(vec![Type::named("int32"), Type::named("String")])
    );
    assert_eq!(
        lowerer.variant_payload_types(
            Some(&Type::Named(
                "QueueReceive".to_string(),
                vec![Type::named("String")]
            )),
            "QueueReceive",
            "TimedOut"
        ),
        Some(Vec::new())
    );
    assert_eq!(
        lowerer.variant_payload_types(
            Some(&Type::Named(
                "WaitAll".to_string(),
                vec![Type::named("String")]
            )),
            "WaitAll",
            "Error"
        ),
        Some(vec![Type::named("int32"), Type::named("String")])
    );
    for (ty, enum_name) in [
        (
            Type::Named("Option".to_string(), vec![Type::named("String")]),
            "Option",
        ),
        (
            Type::Named(
                "Result".to_string(),
                vec![Type::named("int32"), Type::named("String")],
            ),
            "Result",
        ),
        (
            Type::Named("SendError".to_string(), vec![Type::named("int32")]),
            "SendError",
        ),
        (
            Type::Named("QueueReceive".to_string(), vec![Type::named("String")]),
            "QueueReceive",
        ),
        (
            Type::Named("TaskResult".to_string(), vec![Type::named("String")]),
            "TaskResult",
        ),
        (
            Type::Named("WaitAny".to_string(), vec![Type::named("String")]),
            "WaitAny",
        ),
        (
            Type::Named("WaitAll".to_string(), vec![Type::named("String")]),
            "WaitAll",
        ),
    ] {
        assert_eq!(
            lowerer.variant_payload_types(Some(&ty), enum_name, "Missing"),
            None,
            "{enum_name} should reject unknown builtin variants"
        );
    }
    assert_eq!(
        lowerer.variant_payload_types(None, "Status", "Value"),
        Some(vec![Type::named("int32")])
    );
    assert_eq!(
        lowerer.builtin_runtime_member_return_type(&Type::named("String"), "len"),
        Some(Type::named("int32"))
    );
    assert_eq!(
        lowerer.builtin_runtime_member_return_type(&Type::named("String"), "byte_len"),
        Some(Type::named("int32"))
    );
    assert_eq!(
        lowerer.builtin_runtime_member_return_type(&Type::named("String"), "split"),
        Some(Type::Named("Vec".to_string(), vec![Type::named("String")]))
    );
    assert_eq!(
        lowerer.builtin_runtime_member_return_type(&Type::Unit, "to_string"),
        None
    );
    assert_eq!(
        lowerer.builtin_runtime_member_return_type(&Type::named("int32"), "to_string"),
        Some(Type::named("String"))
    );
    assert_eq!(
        lowerer.builtin_runtime_member_return_type(
            &Type::Named("Vec".to_string(), vec![Type::named("String")]),
            "pop"
        ),
        Some(Type::Named(
            "Option".to_string(),
            vec![Type::named("String")]
        ))
    );
    assert_eq!(
        lowerer.builtin_runtime_member_return_type(
            &Type::Named("Set".to_string(), vec![Type::named("String")]),
            "insert"
        ),
        Some(Type::named("bool"))
    );
    assert_eq!(
        lowerer.builtin_runtime_member_return_type(
            &Type::Named(
                "Map".to_string(),
                vec![Type::named("String"), Type::named("int32")]
            ),
            "items"
        ),
        Some(Type::Named(
            "Vec".to_string(),
            vec![Type::Named(
                "MapEntry".to_string(),
                vec![Type::named("String"), Type::named("int32")]
            )]
        ))
    );
    assert_eq!(
        lowerer.builtin_runtime_member_return_type(
            &Type::Named(
                "Map".to_string(),
                vec![Type::named("String"), Type::named("int32")]
            ),
            "keys"
        ),
        Some(Type::Named("Vec".to_string(), vec![Type::named("String")]))
    );
    assert_eq!(
        lowerer.builtin_runtime_member_return_type(
            &Type::Named(
                "Map".to_string(),
                vec![Type::named("String"), Type::named("int32")]
            ),
            "get"
        ),
        Some(Type::Named(
            "Option".to_string(),
            vec![Type::named("int32")]
        ))
    );
    assert_eq!(
        lowerer.builtin_runtime_member_return_type(
            &Type::Named("Task".to_string(), vec![Type::named("bool")]),
            "result"
        ),
        Some(Type::Named(
            "TaskResult".to_string(),
            vec![Type::named("bool")]
        ))
    );
    assert_eq!(
        lowerer.builtin_runtime_member_return_type(
            &Type::Named("Task".to_string(), vec![Type::named("bool")]),
            "result_or_none"
        ),
        Some(Type::Named("Option".to_string(), vec![Type::named("bool")]))
    );
    assert_eq!(
        lowerer.builtin_runtime_member_return_type(
            &Type::Named("Queue".to_string(), vec![Type::named("bool")]),
            "get"
        ),
        Some(Type::Named(
            "QueueReceive".to_string(),
            vec![Type::named("bool")]
        ))
    );
    assert_eq!(
        lowerer.builtin_runtime_member_return_type(
            &Type::Named("Queue".to_string(), vec![Type::named("bool")]),
            "put"
        ),
        Some(Type::Named(
            "Result".to_string(),
            vec![
                Type::Unit,
                Type::Named("SendError".to_string(), vec![Type::named("bool")])
            ]
        ))
    );
    assert_eq!(
        lowerer.builtin_runtime_member_return_type(&Type::named("Vec"), "get"),
        Some(Type::Named(
            "Option".to_string(),
            vec![Type::named("Unknown")]
        ))
    );
    assert_eq!(
        lowerer.builtin_runtime_member_return_type(&Type::named("Map"), "keys"),
        Some(Type::Named("Vec".to_string(), vec![Type::named("Unknown")]))
    );
    assert_eq!(
        lowerer.builtin_runtime_member_return_type(&Type::named("Map"), "values"),
        Some(Type::Named("Vec".to_string(), vec![Type::named("Unknown")]))
    );
    assert_eq!(
        lowerer.builtin_runtime_member_return_type(&Type::named("Map"), "items"),
        Some(Type::Named(
            "Vec".to_string(),
            vec![Type::Named(
                "MapEntry".to_string(),
                vec![Type::named("Unknown"), Type::named("Unknown")]
            )]
        ))
    );
    assert_eq!(
        lowerer.builtin_runtime_member_return_type(&Type::named("Map"), "get"),
        Some(Type::Named(
            "Option".to_string(),
            vec![Type::named("Unknown")]
        ))
    );
    assert_eq!(
        lowerer.builtin_runtime_member_return_type(&Type::named("Queue"), "get"),
        Some(Type::Named(
            "QueueReceive".to_string(),
            vec![Type::named("Unknown")]
        ))
    );
    assert_eq!(
        lowerer.builtin_runtime_member_return_type(&Type::named("Queue"), "get_or_none"),
        Some(Type::Named(
            "Option".to_string(),
            vec![Type::named("Unknown")]
        ))
    );
    assert_eq!(
        lowerer.builtin_runtime_member_return_type(&Type::named("Queue"), "put"),
        Some(Type::Named(
            "Result".to_string(),
            vec![
                Type::Unit,
                Type::Named("SendError".to_string(), vec![Type::named("Unknown")])
            ]
        ))
    );
    assert_eq!(
        lowerer.builtin_runtime_member_return_type(&Type::named("Queue"), "try_put"),
        Some(Type::Named(
            "Result".to_string(),
            vec![
                Type::Unit,
                Type::Named("SendError".to_string(), vec![Type::named("Unknown")])
            ]
        ))
    );
    assert_eq!(
        lowerer.builtin_runtime_member_return_type(&Type::named("TaskGroup"), "start"),
        Some(Type::Named(
            "Task".to_string(),
            vec![Type::named("Unknown")]
        ))
    );
    assert_eq!(
        lowerer.builtin_runtime_member_return_type(&Type::named("TaskGroup"), "start_soon"),
        Some(Type::Unit)
    );
    assert_eq!(
        lowerer.builtin_runtime_member_return_type(&Type::named("fs.File"), "read_all"),
        Some(Type::Named(
            "Result".to_string(),
            vec![Type::named("String"), Type::named("io.Error")]
        ))
    );
    assert_eq!(
        lowerer.builtin_runtime_member_return_type(&Type::named("fs.File"), "read_bytes"),
        Some(Type::Named(
            "Result".to_string(),
            vec![
                Type::Named("Vec".to_string(), vec![Type::named("uint8")]),
                Type::named("io.Error")
            ]
        ))
    );
    assert_eq!(
        lowerer.builtin_runtime_member_return_type(&Type::named("fs.File"), "flush"),
        Some(Type::Named(
            "Result".to_string(),
            vec![Type::Unit, Type::named("io.Error")]
        ))
    );
    assert_eq!(
        lowerer.builtin_runtime_member_return_type(&Type::named("fs.File"), "close"),
        Some(Type::Unit)
    );
    assert_eq!(
        lowerer.builtin_runtime_member_return_type(&Type::named("process.Completed"), "stdout"),
        Some(Type::named("String"))
    );
    assert_eq!(
        lowerer.builtin_runtime_member_return_type(&Type::named("net.TcpListener"), "close"),
        Some(Type::Unit)
    );
    assert_eq!(
        lowerer.builtin_runtime_member_return_type(&Type::named("net.TcpStream"), "shutdown_read"),
        Some(Type::Named(
            "Result".to_string(),
            vec![Type::Unit, Type::named("io.Error")]
        ))
    );
    assert_eq!(
        lowerer.builtin_runtime_member_return_type(&Type::named("net.TcpStream"), "close"),
        Some(Type::Unit)
    );
    assert_eq!(
        lowerer.builtin_runtime_member_return_type(&Type::named("net.UdpSocket"), "recv"),
        Some(Type::Named(
            "Result".to_string(),
            vec![
                Type::Named(
                    "Option".to_string(),
                    vec![Type::Named("Vec".to_string(), vec![Type::named("uint8")])]
                ),
                Type::named("io.Error")
            ]
        ))
    );
    assert_eq!(
        lowerer.builtin_runtime_member_return_type(&Type::named("net.UdpSocket"), "close"),
        Some(Type::Unit)
    );
    assert_eq!(
        lowerer.builtin_runtime_member_return_type(&Type::named("net.HttpListener"), "close"),
        Some(Type::Unit)
    );
    assert_eq!(
        lowerer.builtin_runtime_member_return_type(&Type::named("net.HttpExchange"), "close"),
        Some(Type::Unit)
    );
    assert_eq!(
        lowerer.builtin_runtime_member_return_type(&Type::named("net.HttpResponse"), "reason"),
        Some(Type::named("String"))
    );
    assert_eq!(
        lowerer.builtin_runtime_member_return_type(&Type::named("net.HttpResponse"), "close"),
        Some(Type::Unit)
    );
    assert_eq!(
        lowerer.builtin_runtime_member_return_type(&Type::named("net.WebSocketListener"), "close"),
        Some(Type::Unit)
    );
    assert_eq!(
        lowerer.builtin_runtime_member_return_type(&Type::named("net.WebSocket"), "recv_bytes"),
        Some(Type::Named(
            "Result".to_string(),
            vec![
                Type::Named(
                    "Option".to_string(),
                    vec![Type::Named("Vec".to_string(), vec![Type::named("uint8")])]
                ),
                Type::named("io.Error")
            ]
        ))
    );
    assert_eq!(
        lowerer.builtin_runtime_member_return_type(&Type::named("net.WebSocket"), "close"),
        Some(Type::Unit)
    );
    assert_eq!(
        lowerer.builtin_runtime_member_return_type(&Type::named("net.UnixListener"), "close"),
        Some(Type::Unit)
    );
    assert_eq!(
        lowerer.builtin_runtime_member_return_type(&Type::named("net.UnixStream"), "read_exact"),
        Some(Type::Named(
            "Result".to_string(),
            vec![
                Type::Named("Vec".to_string(), vec![Type::named("uint8")]),
                Type::named("io.Error")
            ]
        ))
    );
    assert_eq!(
        lowerer.builtin_runtime_member_return_type(&Type::named("net.UnixStream"), "close"),
        Some(Type::Unit)
    );
    assert_eq!(
        lowerer.builtin_runtime_member_return_type(&Type::named("net.TlsListener"), "close"),
        Some(Type::Unit)
    );
    assert_eq!(
        lowerer.builtin_runtime_member_return_type(&Type::named("net.TlsStream"), "read_line"),
        Some(Type::Named(
            "Result".to_string(),
            vec![
                Type::Named("Option".to_string(), vec![Type::named("String")]),
                Type::named("io.Error")
            ]
        ))
    );
    assert_eq!(
        lowerer.builtin_runtime_member_return_type(&Type::named("net.TlsStream"), "close"),
        Some(Type::Unit)
    );

    let int_vec = Type::Named("Vec".to_string(), vec![Type::named("int32")]);
    lowerer
        .local_types
        .insert("items".to_string(), int_vec.clone());
    lowerer
        .local_types
        .insert("label".to_string(), Type::named("String"));
    lowerer
        .local_types
        .insert("user".to_string(), Type::named("User"));
    lowerer
        .local_types
        .insert("counter".to_string(), Type::named("Counter"));
    lowerer
        .local_types
        .insert("count".to_string(), Type::named("int32"));
    assert_eq!(
        lowerer.infer_operand_type(&Operand::Place("items".to_string())),
        Some(int_vec)
    );
    assert_eq!(
        lowerer.infer_operand_type(&Operand::Int(7)),
        Some(Type::named("int64"))
    );
    assert_eq!(
        lowerer.infer_operand_type(&Operand::Duration(10)),
        Some(Type::named("Duration"))
    );
    assert_eq!(
        lowerer.infer_operand_type(&Operand::Float(1.5)),
        Some(Type::named("float64"))
    );
    assert_eq!(
        lowerer.infer_operand_type(&Operand::Bool(true)),
        Some(Type::named("bool"))
    );
    assert_eq!(
        lowerer.infer_operand_type(&Operand::String("label".to_string())),
        Some(Type::named("String"))
    );
    assert_eq!(lowerer.infer_operand_type(&Operand::Unit), Some(Type::Unit));
    assert_eq!(
        lowerer.infer_expr_type(&name_expr("make_flag")),
        Some(Type::named("bool"))
    );
    assert!(lowerer.member_call_mutates_receiver(&Operand::Place("items".to_string()), "push"));
    assert!(!lowerer.member_call_mutates_receiver(&Operand::Place("label".to_string()), "len"));
    assert!(!lowerer.member_call_mutates_receiver(&Operand::Place("items".to_string()), "contains"));
    assert!(lowerer.member_call_mutates_receiver(&Operand::Place("user".to_string()), "reset"));
    assert!(lowerer.member_call_mutates_receiver(&Operand::Place("counter".to_string()), "bump"));
    assert!(!lowerer.member_call_mutates_receiver(&Operand::Place("missing".to_string()), "push"));
    assert!(!lowerer.member_call_mutates_receiver(&Operand::Place("count".to_string()), "missing"));
    assert!(lowerer.rvalue_writes_place(
        &Rvalue::Call {
            callee: CallTarget::Member {
                object: Operand::Place("items".to_string()),
                field: "push".to_string(),
                receiver_place: Some("items".to_string()),
            },
            args: Vec::new(),
        },
        "items"
    ));
    assert!(lowerer.rvalue_writes_place(
        &Rvalue::Call {
            callee: CallTarget::Name("borrow_items".to_string()),
            args: vec![MirArg {
                name: None,
                value: Operand::Place("items".to_string()),
                writeback_place: Some("items.length".to_string()),
            }],
        },
        "items"
    ));
    assert!(!lowerer.rvalue_writes_place(&Rvalue::Use(Operand::Unit), "items"));
}

#[test]
fn lower_source_to_mir_covers_broad_control_flow_and_collection_surface() {
    let source = r#"
trait Named:
    def name(borrow self) -> String

class User:
    label: String

impl Named for User:
    def name(borrow self) -> String:
        return self.label.clone()

class Resource:
    closed: bool = false
    def close(borrow mut self):
        self.closed = true

class Counter:
    value: int32

enum Boxed:
    Filled(int32)
    Empty

def worker(value: int32) -> int32:
    return value + 1

def consume[T: Named](value: T) -> String:
    return value.name()

def first_mut(values: own Vec[int32]) -> int32:
    mut local = values
    for item in borrow mut local:
        return item
    return 0

def main() -> int32:
    mut counter = Counter(value=0)
    positional = Counter(2)
    counter.value += positional.value
    mut values: Vec[int32] = [1, 2]
    values[0] = 3
    values[0] += 4
    mut counts: Map[String, int32] = {"a": 1}
    counts["b"] = 2
    counts["a"] += 5
    seen = Set{"a", "b"}
    jobs = Queue[int32]()
    jobs.put(1)
    if true and not false:
        counter.value += values[0]
    match "ok":
        case "ok":
            counter.value += 1
        case _:
            pass
    for i in range(2):
        counter.value += i
    for item in values:
        counter.value += item
    while counter.value < 10:
        break
    match jobs.get(timeout=0ms):
        case QueueReceive.Item(value):
            print(value)
        case QueueReceive.TimedOut:
            counter.value += 10
        case QueueReceive.Closed:
            pass
        case QueueReceive.Cancelled:
            pass
    jobs.close()
    with Resource() as resource:
        print(resource.closed)
    print(consume(value=User(label="aurora")))
    with TaskGroup() as group:
        task = group.start(worker, counter.value)
        print(task.result())
    print(seen.contains("a"))
    print(counts.get("a"))
    mut boxed = Boxed.Filled(3)
    counter.value += match borrow mut boxed:
        case Filled(v): v + 1
        case Empty: 0
    counter.value += first_mut([4, 5])
    return counter.value
"#;

    let module = crate::lower_source_to_mir(source).expect("source should lower");
    assert!(function_names(&module).contains("main"));
    assert!(module
        .trait_impls
        .iter()
        .any(|impl_info| impl_info.trait_name == "Named"));
    let mut saw_task_start = false;
    let mut saw_vec_literal = false;
    let mut saw_set_literal = false;
    let mut saw_map_literal = false;
    for function in module.functions.iter().chain(module.top_level.iter()) {
        for block in &function.blocks {
            for instruction in &block.instructions {
                if let Instruction::Assign { value, .. } = instruction {
                    match value {
                        Rvalue::StartTask { .. } => saw_task_start = true,
                        Rvalue::VecLiteral { .. } => saw_vec_literal = true,
                        Rvalue::SetLiteral { .. } => saw_set_literal = true,
                        Rvalue::MapLiteral { .. } => saw_map_literal = true,
                        _ => {}
                    }
                }
            }
        }
    }
    assert!(saw_task_start);
    assert!(saw_vec_literal);
    assert!(saw_set_literal);
    assert!(saw_map_literal);
}

#[test]
fn indexed_compound_assignment_results_keep_the_collection_element_type() {
    let source = r#"
def main() -> int32:
    mut values: Vec[int32] = [-7]
    values[0] //= 3
    values[0] %= -3
    mut counts: Map[String, int32] = {"left": -7}
    counts["left"] //= 3
    counts["left"] %= -3
    return 0
"#;

    let module = crate::lower_source_to_mir(source).expect("source should lower");
    let main = module
        .functions
        .iter()
        .find(|function| function.name == "main")
        .expect("main should be present in MIR");
    let local_types = main
        .local_types
        .iter()
        .map(|local| (local.name.as_str(), &local.ty))
        .collect::<BTreeMap<_, _>>();
    let result_types = main
        .blocks
        .iter()
        .flat_map(|block| &block.instructions)
        .filter_map(|instruction| match instruction {
            Instruction::Assign {
                target,
                value:
                    Rvalue::Binary {
                        op: BinaryOp::FloorDiv | BinaryOp::Mod,
                        ..
                    },
            } => local_types.get(target.as_str()).copied(),
            _ => None,
        })
        .collect::<Vec<_>>();

    assert_eq!(
        result_types,
        vec![
            &Type::named("int32"),
            &Type::named("int32"),
            &Type::named("int32"),
            &Type::named("int32"),
        ],
        "Vec and Map compound results must retain their indexed int32 element type",
    );

    let negative_rhs_types = main
        .blocks
        .iter()
        .flat_map(|block| &block.instructions)
        .filter_map(|instruction| match instruction {
            Instruction::Assign {
                value:
                    Rvalue::Binary {
                        op: BinaryOp::Mod,
                        right: Operand::Place(place),
                        ..
                    },
                ..
            } => local_types.get(place.as_str()).copied(),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(
        negative_rhs_types,
        vec![&Type::named("int32"), &Type::named("int32")],
        "contextual negative RHS values must retain the indexed int32 element type",
    );
}

#[test]
fn lowerer_constructor_inference_and_for_fallback_cover_unchecked_edges() {
    let program = Box::leak(Box::new(checked_program(
        "\
class Pair[A, B]:
    first: A
    second: B

def main() -> int32:
    return 0
",
    )));
    let mut lowerer = Lowerer::new(
        program,
        "main",
        &program.module_name,
        Type::named("int32"),
        BTreeMap::new(),
    );

    assert_eq!(
        lowerer.infer_class_constructor_type(
            "Pair",
            &[
                arg(expr(ExprKind::String("left".to_string()))),
                arg(expr(ExprKind::Int(1)))
            ],
            None,
        ),
        Some(Type::Named(
            "Pair".to_string(),
            vec![Type::named("String"), Type::named("int64")]
        ))
    );
    assert_eq!(
        lowerer.infer_class_constructor_type(
            "Pair",
            &[
                named_arg("first", expr(ExprKind::String("left".to_string()))),
                arg(expr(ExprKind::Int(1))),
            ],
            None,
        ),
        Some(Type::named("Pair"))
    );
    assert_eq!(
        lowerer.infer_class_constructor_type(
            "Pair",
            &[],
            Some(&[type_ref("String"), type_ref("int32")])
        ),
        Some(Type::Named(
            "Pair".to_string(),
            vec![Type::named("String"), Type::named("int32")]
        ))
    );
    assert_eq!(
        lowerer.infer_class_constructor_type("Pair", &[], None),
        Some(Type::named("Pair"))
    );

    lowerer.lower_for(&ForStmt {
        target: BindingTarget::Name {
            name: "item".to_string(),
            span: Span::new(1, 1),
        },
        iterable: expr(ExprKind::Bool(true)),
        borrow_mode: None,
        body: vec![Stmt::Pass(PassStmt {
            span: Span::new(1, 1),
        })],
        span: Span::new(1, 1),
    });
    assert!(lowerer.blocks.iter().any(|block| matches!(
        block.terminator,
        Some(Terminator::ForRange { ref binding, .. }) if binding == "item"
    )));

    let mut return_lowerer = Lowerer::new(
        program,
        "main",
        &program.module_name,
        Type::named("int32"),
        BTreeMap::new(),
    );
    return_lowerer.local_types.insert(
        "items".to_string(),
        Type::Named("Vec".to_string(), vec![Type::named("int32")]),
    );
    let parent_return_block = return_lowerer.new_block("parent_return");
    let parent_return_label = return_lowerer.label(parent_return_block);
    let parent_return_place = return_lowerer.new_typed_temp(Type::named("int32"));
    return_lowerer.return_redirects.push(ReturnRedirect {
        label: parent_return_label.clone(),
        return_place: parent_return_place.clone(),
        cleanup_depth: 0,
    });
    return_lowerer.lower_for(&ForStmt {
        target: BindingTarget::Name {
            name: "item".to_string(),
            span: Span::new(1, 1),
        },
        iterable: name_expr("items"),
        borrow_mode: Some(ReceiverKind::BorrowMut),
        body: vec![Stmt::Return(crate::ast::ReturnStmt {
            value: Some(name_expr("item")),
            span: Span::new(1, 1),
        })],
        span: Span::new(1, 1),
    });
    return_lowerer.return_redirects.pop();
    assert!(return_lowerer.blocks.iter().any(|block| matches!(
        block.terminator,
        Some(Terminator::Goto(ref label)) if label == &parent_return_label
    )));
    assert!(return_lowerer.blocks.iter().any(|block| {
        block.instructions.iter().any(|instruction| {
            matches!(
                instruction,
                Instruction::Assign {
                    target,
                    value: Rvalue::Use(Operand::Place(_)),
                } if target == &parent_return_place
            )
        })
    }));

    let indexed_target = AssignTarget::Index {
        object: Box::new(name_expr("items")),
        index: Box::new(expr(ExprKind::Int(0))),
    };
    let previous_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(|_| {}));
    let indexed_panic = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        lowerer.render_assign_target(&indexed_target);
    }));
    std::panic::set_hook(previous_hook);
    assert!(
        indexed_panic.is_err(),
        "indexed assignments should lower through helper calls before rendering"
    );
}

#[test]
fn lowerer_direct_collection_literals_cover_uninferred_set_and_map_exprs() {
    let program = Box::leak(Box::new(checked_program(
        "def main() -> int32:\n    return 0\n",
    )));
    let mut lowerer = Lowerer::new(
        program,
        "main",
        &program.module_name,
        Type::named("int32"),
        BTreeMap::new(),
    );

    let set_operand = lowerer.lower_expr(&expr(ExprKind::Set(vec![
        expr(ExprKind::String("a".to_string())),
        expr(ExprKind::String("b".to_string())),
    ])));
    let map_operand = lowerer.lower_expr(&expr(ExprKind::Map(vec![MapEntryExpr {
        key: expr(ExprKind::String("a".to_string())),
        value: expr(ExprKind::Int(1)),
    }])));
    let empty_list_operand = lowerer.lower_expr(&expr(ExprKind::List(Vec::new())));
    let empty_set_operand = lowerer.lower_expr(&expr(ExprKind::Set(Vec::new())));
    let empty_map_operand = lowerer.lower_expr(&expr(ExprKind::Map(Vec::new())));
    let malformed_vec_constructor = lowerer.lower_expr(&expr(ExprKind::Call {
        callee: Box::new(expr(ExprKind::Specialize {
            expr: Box::new(name_expr("Vec")),
            type_args: Vec::new(),
        })),
        args: Vec::new(),
    }));
    let malformed_map_constructor = lowerer.lower_expr(&expr(ExprKind::Call {
        callee: Box::new(expr(ExprKind::Specialize {
            expr: Box::new(name_expr("Map")),
            type_args: Vec::new(),
        })),
        args: Vec::new(),
    }));

    assert!(matches!(set_operand, Operand::Place(_)));
    assert!(matches!(map_operand, Operand::Place(_)));
    assert!(matches!(empty_list_operand, Operand::Place(_)));
    assert!(matches!(empty_set_operand, Operand::Place(_)));
    assert!(matches!(empty_map_operand, Operand::Place(_)));
    assert!(matches!(malformed_vec_constructor, Operand::Place(_)));
    assert!(matches!(malformed_map_constructor, Operand::Place(_)));
    let instructions = lowerer.blocks[lowerer.current_block]
        .instructions
        .iter()
        .collect::<Vec<_>>();
    assert!(instructions.iter().any(|instruction| matches!(
        instruction,
        Instruction::Assign {
            value: Rvalue::SetLiteral {
                element_type,
                elements,
            },
            ..
        } if element_type == &Type::named("String") && elements.len() == 2
    )));
    assert!(instructions.iter().any(|instruction| matches!(
        instruction,
        Instruction::Assign {
            value: Rvalue::MapLiteral {
                key_type,
                value_type,
                entries,
            },
            ..
        } if key_type == &Type::named("String")
            && value_type == &Type::named("int64")
            && entries.len() == 1
    )));
    assert!(instructions.iter().any(|instruction| matches!(
        instruction,
        Instruction::Assign {
            value: Rvalue::VecLiteral {
                element_type,
                elements,
            },
            ..
        } if element_type == &Type::named("Unknown") && elements.is_empty()
    )));
    assert!(instructions.iter().any(|instruction| matches!(
        instruction,
        Instruction::Assign {
            value: Rvalue::SetLiteral {
                element_type,
                elements,
            },
            ..
        } if element_type == &Type::named("Unknown") && elements.is_empty()
    )));
    assert!(instructions.iter().any(|instruction| matches!(
        instruction,
        Instruction::Assign {
            value: Rvalue::MapLiteral {
                key_type,
                value_type,
                entries,
            },
            ..
        } if key_type == &Type::named("Unknown")
            && value_type == &Type::named("Unknown")
            && entries.is_empty()
    )));
}

#[test]
fn lowerer_direct_pattern_helpers_cover_defensive_variant_and_literal_edges() {
    let program = Box::leak(Box::new(checked_program(
        "\
enum Maybe:
    Some(int32)
    Empty

def main() -> int32:
    return 0
",
    )));
    let mut lowerer = Lowerer::new(
        program,
        "main",
        &program.module_name,
        Type::named("int32"),
        BTreeMap::new(),
    );
    lowerer.scoped_names.push(std::collections::HashMap::new());

    let binding_success = lowerer.new_block("binding_success");
    let binding_failure = lowerer.new_block("binding_failure");
    let binding_writeback = lowerer.lower_pattern(
        &binding_pattern("item"),
        Operand::Int(7),
        None,
        binding_success,
        binding_failure,
        PatternLoweringOptions {
            collect_writeback: true,
            consume_payloads: false,
        },
    );
    let PatternWriteback::Use(Operand::Place(binding_place)) =
        binding_writeback.expect("binding patterns should produce writeback")
    else {
        panic!("binding pattern should write back the generated place");
    };
    assert_eq!(
        lowerer
            .scoped_names
            .last()
            .and_then(|scope| scope.get("item")),
        Some(&binding_place)
    );
    assert!(
        !lowerer.local_types.contains_key(&binding_place),
        "untyped defensive pattern lowering should allocate an untyped temp"
    );

    let mismatch_entry = lowerer.new_block("mismatch_entry");
    let mismatch_success = lowerer.new_block("mismatch_success");
    let mismatch_failure = lowerer.new_block("mismatch_failure");
    lowerer.switch_to(mismatch_entry);
    let mismatched = lowerer.lower_pattern(
        &variant_pattern(Some("Maybe"), "Some", Vec::new()),
        Operand::Place("candidate".to_string()),
        Some(&Type::named("Maybe")),
        mismatch_success,
        mismatch_failure,
        PatternLoweringOptions {
            collect_writeback: true,
            consume_payloads: false,
        },
    );
    assert!(mismatched.is_none());
    assert!(matches!(
        lowerer.blocks[lowerer.current_block].terminator,
        Some(Terminator::Goto(ref label)) if label == &lowerer.label(mismatch_failure)
    ));

    let unknown_entry = lowerer.new_block("unknown_entry");
    let unknown_success = lowerer.new_block("unknown_success");
    let unknown_failure = lowerer.new_block("unknown_failure");
    lowerer.switch_to(unknown_entry);
    let unknown_writeback = lowerer.lower_pattern(
        &variant_pattern(None, "Some", vec![Pattern::Wildcard(Span::new(1, 1))]),
        Operand::Place("unknown".to_string()),
        None,
        unknown_success,
        unknown_failure,
        PatternLoweringOptions {
            collect_writeback: true,
            consume_payloads: false,
        },
    );
    let PatternWriteback::Variant { ty, payloads, .. } =
        unknown_writeback.expect("unknown variant lowering should produce a writeback")
    else {
        panic!("variant pattern should write back a reconstructed variant");
    };
    assert_eq!(ty, Type::named("Unknown"));
    assert!(matches!(
        payloads.as_slice(),
        [PatternWriteback::Use(Operand::Place(_))]
    ));

    let unit_variant_entry = lowerer.new_block("unit_variant_entry");
    let unit_variant_success = lowerer.new_block("unit_variant_success");
    let unit_variant_failure = lowerer.new_block("unit_variant_failure");
    lowerer.switch_to(unit_variant_entry);
    let unit_variant_writeback = lowerer.lower_pattern(
        &variant_pattern(Some("Maybe"), "Empty", Vec::new()),
        Operand::Place("unknown".to_string()),
        None,
        unit_variant_success,
        unit_variant_failure,
        PatternLoweringOptions {
            collect_writeback: true,
            consume_payloads: false,
        },
    );
    let PatternWriteback::Variant { ty, payloads, .. } =
        unit_variant_writeback.expect("unit variant pattern should produce a writeback")
    else {
        panic!("unit variant pattern should write back a reconstructed variant");
    };
    assert_eq!(ty, Type::named("Unknown"));
    assert!(payloads.is_empty());

    let positive = lowerer.lower_literal_pattern_operand(
        None,
        &LiteralPatternKind::Int(IntegerValue::from_signed(5)),
        Span::new(1, 1),
    );
    assert_eq!(positive, Operand::Int(5));

    let negative = lowerer.lower_literal_pattern_operand(
        Some(&Type::named("int32")),
        &LiteralPatternKind::Int(IntegerValue::from_signed(-5)),
        Span::new(1, 1),
    );
    assert!(matches!(negative, Operand::Place(_)));
    let negative_unknown = lowerer.lower_literal_pattern_operand(
        None,
        &LiteralPatternKind::Int(IntegerValue::from_signed(-7)),
        Span::new(1, 1),
    );
    assert!(matches!(negative_unknown, Operand::Place(_)));

    let literal_entry = lowerer.new_block("literal_entry");
    let literal_success = lowerer.new_block("literal_success");
    let literal_failure = lowerer.new_block("literal_failure");
    lowerer.switch_to(literal_entry);
    let literal_writeback = lowerer.lower_pattern(
        &Pattern::Literal(LiteralPattern {
            kind: LiteralPatternKind::Int(IntegerValue::from_signed(2)),
            span: Span::new(1, 1),
        }),
        Operand::Int(2),
        Some(&Type::named("int32")),
        literal_success,
        literal_failure,
        PatternLoweringOptions {
            collect_writeback: true,
            consume_payloads: false,
        },
    );
    assert!(matches!(
        literal_writeback,
        Some(PatternWriteback::Use(Operand::Int(2)))
    ));
}

#[test]
fn lower_path_to_mir_covers_imported_module_surface() {
    let repo_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("compiler crate should live under repo root")
        .parent()
        .expect("compiler crate should live under repo root")
        .to_path_buf();
    let path = repo_root.join("examples/modules/trait_impl_imports.au");
    let module = crate::lower_path_to_mir(&path).expect("example should lower");

    assert!(module
        .functions
        .iter()
        .any(|function| function.name == "show"));
    assert!(module
        .classes
        .iter()
        .any(|class| class.name == "pkg.user.User"));
    assert!(module
        .trait_impls
        .iter()
        .any(|impl_info| impl_info.trait_name == "Named"));
    assert!(module.top_level.is_none());
}

#[test]
fn imported_generic_rng_holders_keep_distinct_canonical_mir_identities() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/run-pass/imported_same_leaf_class_identity.au");
    let module = crate::lower_path_to_mir(&path)
        .expect("same-leaf generic Rng holders should lower with module provenance");

    let rng = Type::named("random.Rng");
    let local_holder = Type::Named("Holder".to_string(), vec![rng.clone()]);
    let remote_holder = Type::Named(
        "same_leaf_support.remote.Holder".to_string(),
        vec![rng.clone()],
    );
    let local_envelope = Type::Named("Envelope".to_string(), vec![Type::named("random.Rng")]);
    let remote_envelope = Type::Named(
        "same_leaf_support.remote.Envelope".to_string(),
        vec![Type::named("random.Rng")],
    );
    let main = module
        .functions
        .iter()
        .find(|function| function.name == "main")
        .expect("fixture main should lower");

    assert!(
        main.local_types
            .iter()
            .any(|local| local.ty == local_holder),
        "local types: {:#?}",
        main.local_types
    );
    assert!(
        main.local_types
            .iter()
            .any(|local| local.ty == remote_holder),
        "local types: {:#?}",
        main.local_types
    );
    assert!(main
        .local_types
        .iter()
        .any(|local| local.ty == local_envelope));
    assert!(main
        .local_types
        .iter()
        .any(|local| local.ty == remote_envelope));
    assert_eq!(
        main.local_types
            .iter()
            .find(|local| local.name == "bridge_holder")
            .map(|local| &local.ty),
        Some(&remote_holder)
    );
    assert_eq!(
        main.local_types
            .iter()
            .find(|local| local.name == "bridge_envelope")
            .map(|local| &local.ty),
        Some(&remote_envelope)
    );
    assert_eq!(
        main.local_types
            .iter()
            .find(|local| local.name == "bridge_empty_envelope")
            .map(|local| &local.ty),
        Some(&remote_envelope)
    );
    assert!(module.classes.iter().any(|class| class.name == "Holder"));
    assert!(module
        .classes
        .iter()
        .any(|class| class.name == "same_leaf_support.remote.Holder"));
    assert!(module
        .trait_impls
        .iter()
        .any(|trait_impl| trait_impl.for_type
            == Type::Named(
                "same_leaf_support.remote.Holder".to_string(),
                vec![Type::TypeParam("T".to_string())],
            )));
    assert!(module
        .functions
        .iter()
        .any(|function| function.name == "same_leaf_support.remote::Holder.source"));

    let bridge_holder = module
        .functions
        .iter()
        .find(|function| function.name == "same_leaf_support.bridge::make_holder")
        .expect("transitive holder factory should lower");
    assert_eq!(bridge_holder.return_type, remote_holder);
    assert!(bridge_holder.blocks.iter().any(|block| {
        block.instructions.iter().any(|instruction| {
            matches!(
                instruction,
                Instruction::Assign {
                    value: Rvalue::Construct { class_name, .. },
                    ..
                } if class_name == "same_leaf_support.remote.Holder"
            )
        })
    }));

    for function_name in ["make_envelope", "make_empty_envelope"] {
        let function = module
            .functions
            .iter()
            .find(|function| function.name == format!("same_leaf_support.bridge::{function_name}"))
            .expect("transitive enum factory should lower");
        assert_eq!(function.return_type, remote_envelope);
        assert!(function.blocks.iter().any(|block| {
            block.instructions.iter().any(|instruction| {
                matches!(
                    instruction,
                    Instruction::Assign {
                        value: Rvalue::EnumVariant { enum_name, .. },
                        ..
                    } if enum_name == "same_leaf_support.remote.Envelope"
                )
            })
        }));
    }

    let enum_names = main
        .blocks
        .iter()
        .flat_map(|block| block.instructions.iter())
        .filter_map(|instruction| match instruction {
            Instruction::Assign {
                value: Rvalue::EnumVariant { enum_name, .. },
                ..
            } => Some(enum_name.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert!(
        enum_names.contains(&"Envelope"),
        "enum names: {enum_names:?}"
    );
    assert!(
        enum_names.contains(&"same_leaf_support.remote.Envelope"),
        "enum names: {enum_names:?}"
    );
}

#[test]
fn contextual_none_equality_lowers_none_as_option_variants() {
    let source = include_str!("../tests/fixtures/run-pass/contextual_none_equality.au");
    let module = crate::lower_source_to_mir(source).expect("contextual None source should lower");
    let main = module
        .functions
        .iter()
        .find(|function| function.name == "main")
        .expect("main should be lowered");
    let contextual_none_count = main
        .blocks
        .iter()
        .flat_map(|block| block.instructions.iter())
        .filter(|instruction| {
            matches!(
                instruction,
                Instruction::Assign {
                    value: Rvalue::EnumVariant {
                        enum_name,
                        variant_name,
                        payloads,
                    },
                    ..
                } if enum_name == "Option" && variant_name == "None" && payloads.is_empty()
            )
        })
        .count();

    assert_eq!(contextual_none_count, 12);
}

#[test]
fn assertions_lower_to_lazy_failure_blocks_with_keyword_spans() {
    let source = r#"def main() -> int32:
    assert true
    assert false, "  exact message  "
    return 0
"#;
    let module = crate::lower_source_to_mir(source).expect("assertions should lower to MIR");
    let main = module
        .functions
        .iter()
        .find(|function| function.name == "main")
        .expect("main should be lowered");

    let branches = main
        .blocks
        .iter()
        .filter(|block| matches!(block.terminator, Terminator::Branch { .. }))
        .count();
    assert_eq!(
        branches, 2,
        "each assertion must branch before its lazy failure message"
    );

    let failures = main
        .blocks
        .iter()
        .filter_map(|block| match &block.terminator {
            Terminator::AssertFail { message, span } => Some((message, span)),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(failures.len(), 2);
    assert_eq!(failures[0], (&None, &Span::new(2, 5)));
    assert_eq!(
        failures[1],
        (
            &Some(Operand::String("  exact message  ".to_string())),
            &Span::new(3, 5)
        )
    );
}
