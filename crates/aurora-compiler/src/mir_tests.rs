use super::*;
use crate::ast::{
    Argument, AssignTarget, BindingPattern, Expr, ExprKind, ForStmt, LiteralPattern,
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
    TypeRef {
        name: name.to_string(),
        args: Vec::new(),
        indirect: false,
        span: Span::new(1, 1),
    }
}

fn arg(value: Expr) -> Argument {
    Argument {
        name: None,
        span: value.span,
        value,
    }
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
    let imported = checked_program(imported_source);
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
    def add(borrow self, rhs: Rhs) -> Out

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
    def add(borrow self, rhs: int32) -> bool:
        return rhs > 0

impl Neg[String] for User:
    def neg(borrow self) -> String:
        return self.label.clone()

impl[T: Named] Add[Box[T], Box[T]] for Box[T]:
    def add(borrow self, rhs: Box[T]) -> Box[T]:
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
        lower_type_ref(&TypeRef {
            name: "Vec".to_string(),
            args: vec![type_ref("int32")],
            indirect: false,
            span: Span::new(1, 1),
        }),
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
        "Status"
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
        Some(Type::named("Thing"))
    );
    assert_eq!(
        lowerer.infer_expr_type(&expr(ExprKind::Call {
            callee: Box::new(member_expr(
                member_expr(name_expr("pkg"), "helpers"),
                "Status",
            )),
            args: Vec::new(),
        })),
        Some(Type::named("Status"))
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
        Some(Type::named("int32"))
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
    assert_eq!(local_static_target.0, "Thing.zero");
    let module_static_target = lowerer
        .resolve_task_start_target(&member_expr(
            member_expr(member_expr(name_expr("pkg"), "helpers"), "Thing"),
            "zero",
        ))
        .expect("module-qualified imported class static methods should resolve");
    assert_eq!(module_static_target.0, "pkg.helpers::Thing.zero");
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
    assert_eq!(module_function_target.0, "pkg.helpers::helper");
    let reexport_function_target = lowerer
        .resolve_task_start_target(&member_expr(
            member_expr(name_expr("pkg"), "reexport"),
            "helper",
        ))
        .expect("all-functions-only imported functions should resolve");
    assert_eq!(reexport_function_target.0, "pkg.reexport::helper");
    let specialized_local_function = expr(ExprKind::Specialize {
        expr: Box::new(name_expr("local_helper")),
        type_args: Vec::new(),
    });
    assert_eq!(
        lowerer
            .resolve_task_start_target(&specialized_local_function)
            .expect("specialized local functions should resolve as task targets")
            .0,
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
            .0,
        "Thing.zero"
    );
    let specialized_class_object = expr(ExprKind::Specialize {
        expr: Box::new(name_expr("Thing")),
        type_args: Vec::new(),
    });
    assert_eq!(
        lowerer
            .resolve_task_start_target(&member_expr(specialized_class_object, "zero"))
            .expect("static methods on specialized class objects should resolve")
            .0,
        "Thing.zero"
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
        } if class_name == "Thing"
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
        } if name == "Thing.zero"
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
        Some(Type::named("int32"))
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

def first_mut(values: Vec[int32]) -> int32:
    mut local = values
    for item in borrow mut local:
        return item
    return 0

def main() -> int32:
    mut counter = Counter(value=0)
    positional = Counter(2)
    counter.value += positional.value
    mut values = [1, 2]
    values[0] = 3
    values[0] += 4
    mut counts = {"a": 1}
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
            vec![Type::named("String"), Type::named("int32")]
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
        binding: "item".to_string(),
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
        binding: "item".to_string(),
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
                    value: Rvalue::Use(Operand::Place(place)),
                } if target == &parent_return_place && place == "item"
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
            && value_type == &Type::named("int32")
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
        true,
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
        true,
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
        true,
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
        true,
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
        &LiteralPatternKind::Int(IntegerValue::Signed(5)),
        Span::new(1, 1),
    );
    assert_eq!(positive, Operand::Int(5));

    let negative = lowerer.lower_literal_pattern_operand(
        Some(&Type::named("int32")),
        &LiteralPatternKind::Int(IntegerValue::Signed(-5)),
        Span::new(1, 1),
    );
    assert!(matches!(negative, Operand::Place(_)));
    let negative_unknown = lowerer.lower_literal_pattern_operand(
        None,
        &LiteralPatternKind::Int(IntegerValue::Signed(-7)),
        Span::new(1, 1),
    );
    assert!(matches!(negative_unknown, Operand::Place(_)));

    let literal_entry = lowerer.new_block("literal_entry");
    let literal_success = lowerer.new_block("literal_success");
    let literal_failure = lowerer.new_block("literal_failure");
    lowerer.switch_to(literal_entry);
    let literal_writeback = lowerer.lower_pattern(
        &Pattern::Literal(LiteralPattern {
            kind: LiteralPatternKind::Int(IntegerValue::Signed(2)),
            span: Span::new(1, 1),
        }),
        Operand::Int(2),
        Some(&Type::named("int32")),
        literal_success,
        literal_failure,
        true,
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
    assert!(module.classes.iter().any(|class| class.name == "User"));
    assert!(module
        .trait_impls
        .iter()
        .any(|impl_info| impl_info.trait_name == "Named"));
    assert!(module.top_level.is_none());
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
