use super::*;
use crate::ast::{Expr, ExprKind, TypeRef};
use crate::diag::Span;
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

enum Status:
    Ok

def helper() -> int32:
    return 7
"#;

    let mut program = checked_program(main_source);
    let imported = checked_program(imported_source);
    let helpers = namespace_from_program("helpers", "pkg.helpers", &imported);
    let mut pkg = ModuleNamespace {
        name: "pkg".to_string(),
        path: "pkg".to_string(),
        source_path: None,
        modules: BTreeMap::from([("helpers".to_string(), helpers.clone())]),
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

class User:
    label: String

impl Named for User:
    def name(borrow self) -> String:
        return self.label
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

    let named_bound = TraitBound {
        trait_name: "Named".to_string(),
        trait_args: Vec::new(),
    };
    assert!(lowerer.type_implements_trait_bound(&Type::named("User"), &named_bound));
    assert!(!lowerer.type_implements_trait_bound(&Type::named("String"), &named_bound));
    assert!(lowerer
        .trait_method_for_receiver(&Type::named("User"), "name")
        .is_some());
    assert!(lowerer
        .trait_impl_method_for_class_name("User", "name")
        .is_some());

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
        lowerer.builtin_runtime_member_return_type(&Type::named("String"), "split"),
        Some(Type::Named("Vec".to_string(), vec![Type::named("String")]))
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
            &Type::Named("Queue".to_string(), vec![Type::named("bool")]),
            "get"
        ),
        Some(Type::Named(
            "QueueReceive".to_string(),
            vec![Type::named("bool")]
        ))
    );
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
        return self.label

class Resource:
    closed: bool = false
    def close(borrow mut self):
        self.closed = true

class Counter:
    value: int32

def worker(value: int32) -> int32:
    return value + 1

def consume[T: Named](value: T) -> String:
    return value.name()

def main() -> int32:
    mut counter = Counter(value=0)
    mut values = [1, 2]
    values[0] = 3
    mut counts = {"a": 1}
    counts["b"] = 2
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
