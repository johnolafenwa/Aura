use std::collections::{BTreeMap, HashMap};

use super::{bind_type_params, validate_call_type_args};
use crate::diag::Span;
use crate::mir::{CallTarget, Instruction, MirModule, Rvalue};
use crate::sema::Type;

const GENERIC_CALLS: &str = r#"
def identity[T](value: own T) -> T:
    return value

def count_some[T](value: T | None) -> int64:
    return 1

def plain(value: int64) -> int64:
    return value

def main():
    a = identity(3)
    b = identity("x")
    c = count_some[str](None)
    print(a + plain(c))
    print(b)
"#;

/// The type arguments of each call `main` makes to `callee`, in order.
fn call_type_args<'a>(
    module: &'a mut MirModule,
    callee: &str,
) -> Vec<&'a mut BTreeMap<String, Type>> {
    module
        .functions
        .iter_mut()
        .filter(|function| function.name == "main")
        .flat_map(|function| &mut function.blocks)
        .flat_map(|block| &mut block.instructions)
        .filter_map(|instruction| match instruction {
            Instruction::Assign {
                value:
                    Rvalue::Call {
                        callee: CallTarget::Name(name),
                        type_args,
                        ..
                    },
                ..
            } if name == callee => Some(type_args),
            _ => None,
        })
        .collect()
}

/// `GENERIC_CALLS` lowered with its generic calls left erased, as calls
/// the specialization pass makes no copy for stay.
fn lower_erased() -> MirModule {
    let program = crate::check_source(GENERIC_CALLS).expect("source should check");
    crate::mir::lower_unspecialized(&program)
}

fn assert_boundaries_reject(module: &MirModule, expected: &str) {
    let runtime = crate::run_mir(module).expect_err("the interpreter must reject the call");
    assert!(runtime.message.contains(expected), "{}", runtime.message);
    let direct = crate::native_codegen::emit_host_object(module)
        .expect_err("the direct backend must reject the call");
    assert!(direct.contains(expected), "{direct}");
}

#[test]
fn lowering_records_the_type_each_generic_call_binds() {
    let mut module = lower_erased();
    let identity = call_type_args(&mut module, "identity");
    assert_eq!(identity.len(), 2);
    assert_eq!(
        *identity[0],
        BTreeMap::from([("T".to_string(), Type::named("int64"))])
    );
    assert_eq!(
        *identity[1],
        BTreeMap::from([("T".to_string(), Type::named("str"))])
    );
    // A union binds nothing, so `T` stands for itself.
    let count_some = call_type_args(&mut module, "count_some");
    assert_eq!(
        *count_some[0],
        BTreeMap::from([("T".to_string(), Type::TypeParam("T".to_string()))])
    );
    let plain = call_type_args(&mut module, "plain");
    assert!(plain[0].is_empty());
    assert_eq!(validate_call_type_args(&module), Ok(()));

    // `lower_implicit` leaves the calls bare; the native boundary records
    // the same arguments.
    let program = crate::check_source(GENERIC_CALLS).expect("source should check");
    let mut implicit = crate::mir::lower_implicit(&program);
    assert!(call_type_args(&mut implicit, "identity")
        .iter()
        .all(|type_args| type_args.is_empty()));
    crate::mir::validate_at_boundary(&mut implicit).expect("the module should validate");
    assert_eq!(
        *call_type_args(&mut implicit, "identity")[1],
        BTreeMap::from([("T".to_string(), Type::named("str"))])
    );
}

#[test]
fn boundaries_reject_calls_with_the_wrong_type_arguments() {
    let lowered = lower_erased();

    let mut missing = lowered.clone();
    call_type_args(&mut missing, "identity")[0].clear();
    assert_boundaries_reject(
        &missing,
        "invalid MIR call in `main` passes 0 type argument(s) to a callee with 1 type parameter(s)",
    );

    let mut renamed = lowered.clone();
    let type_args = &mut *call_type_args(&mut renamed, "identity")[0];
    type_args.clear();
    type_args.insert("U".to_string(), Type::named("int64"));
    assert_boundaries_reject(
        &renamed,
        "invalid MIR call in `main` passes 1 type argument(s) to a callee with 1 type parameter(s)",
    );

    let mut extra = lowered;
    call_type_args(&mut extra, "plain")[0].insert("T".to_string(), Type::named("int64"));
    assert_boundaries_reject(
        &extra,
        "invalid MIR call in `main` passes 1 type argument(s) to a callee with 0 type parameter(s)",
    );
}

#[test]
fn type_params_bind_through_callable_structure_and_capabilities() {
    use crate::ast::ReceiverKind;
    use crate::sema::{ClosureCallKind, ClosureCapture, ClosureCaptureMode, FunctionParamContract};

    let param = |name: &str, ty: Type, passing| FunctionParamContract {
        keyword_only: false,
        name: name.to_string(),
        ty,
        passing,
        has_default: false,
    };
    let pattern = Type::Closure {
        params: Box::new(vec![param(
            "value",
            Type::TypeParam("T".to_string()),
            ReceiverKind::Value,
        )]),
        return_type: Box::new(Type::Tuple(vec![
            Type::TypeParam("T".to_string()),
            Type::TypeParam("U".to_string()),
        ])),
        captures: Box::new(vec![ClosureCapture {
            name: "state".to_string(),
            ty: Type::TypeParam("U".to_string()),
            mode: ClosureCaptureMode::MutableView,
            span: Span::new(1, 1),
            mutated: false,
        }]),
        call_kind: ClosureCallKind::MutableRepeatable,
    };
    let actual = Type::Closure {
        params: Box::new(vec![param(
            "value",
            Type::named("int64"),
            ReceiverKind::Value,
        )]),
        return_type: Box::new(Type::Tuple(vec![Type::named("int64"), Type::named("str")])),
        captures: Box::new(vec![ClosureCapture {
            name: "state".to_string(),
            ty: Type::named("str"),
            mode: ClosureCaptureMode::MutableView,
            span: Span::new(1, 1),
            mutated: false,
        }]),
        call_kind: ClosureCallKind::MutableRepeatable,
    };
    let mut substitutions = HashMap::new();
    bind_type_params(&pattern, &actual, &mut substitutions);
    assert_eq!(substitutions.get("T"), Some(&Type::named("int64")));
    assert_eq!(substitutions.get("U"), Some(&Type::named("str")));

    let function_pattern = Type::Function {
        params: vec![param(
            "value",
            Type::TypeParam("V".to_string()),
            ReceiverKind::Borrow,
        )],
        return_type: Box::new(Type::TypeParam("R".to_string())),
    };
    let wrong_capability = Type::Function {
        params: vec![param("value", Type::named("bool"), ReceiverKind::BorrowMut)],
        return_type: Box::new(Type::named("int32")),
    };
    bind_type_params(&function_pattern, &wrong_capability, &mut substitutions);
    assert!(!substitutions.contains_key("V"));
    assert!(!substitutions.contains_key("R"));

    for (pattern, actual) in [
        (
            Type::Named("list".to_string(), vec![Type::TypeParam("N".to_string())]),
            Type::named("str"),
        ),
        (
            Type::Tuple(vec![Type::TypeParam("Q".to_string())]),
            Type::Tuple(Vec::new()),
        ),
        (function_pattern, Type::Unit),
    ] {
        bind_type_params(&pattern, &actual, &mut substitutions);
    }
    assert!(!substitutions.contains_key("N"));
    assert!(!substitutions.contains_key("Q"));
}
