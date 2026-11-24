use std::collections::HashMap;

use crate::lower_source_to_mir;
use crate::mir::{MirModule, MirOwnership};
use crate::sema::Type;

use super::super::{types, validated_for_emission, MirClass};
use super::{called_function, plan, slot_parts, twin_results, ResultSlotPlan};

const SOURCE: &str = r#"
enum Fault:
    Missing(name: str)
    Plain

def scalar(value: int64) -> Result[int64, int64]:
    if value < 0:
        return Result.Err(-value)
    return Result.Ok(value * 2)

def nothing(value: int64) -> Result[None, Fault]:
    if value == 7:
        return Result.Err(Fault.Plain)
    return Result.Ok(None)

def halves(value: float64) -> Result[float64, str]:
    if value < 0.0:
        return Result.Err("negative")
    return Result.Ok(value / 2.0)

def chain(value: int64) -> Result[int64, int64]:
    doubled = try scalar(value)
    return Result.Ok(doubled + 1)

def stored(values: list[Result[int64, int64]], index: int64) -> Result[int64, int64]:
    return values[index]

def printed(value: int64) -> Result[int64, int64]:
    if value == 0:
        return Result.Err(1)
    return Result.Ok(value)

def matched(value: int64) -> int64:
    match scalar(value):
        case Result.Ok(found):
            return found
        case Result.Err(code):
            return code

def unwrapped(value: int64) -> int64:
    match chain(value):
        case Result.Ok(found):
            return found
        case Result.Err(code):
            return -code

def looked(values: list[Result[int64, int64]]) -> int64:
    match stored(values, 0):
        case Result.Ok(found):
            return found
        case Result.Err(code):
            return code

def kept(value: int64) -> list[Result[int64, int64]]:
    return [scalar(value)]

def shown(value: int64) -> str:
    return f"{nothing(value)}"

def called(make: def(value: int64) -> Result[int64, int64], value: int64) -> int64:
    match make(value):
        case Result.Ok(found):
            return found
        case Result.Err(code):
            return code

def main():
    print(matched(1))
    print(unwrapped(2))
    print(looked([scalar(3)]))
    print(kept(4))
    print(shown(5))
    print(called(scalar, 6))
    print(printed(7))
    match halves(8.0):
        case Result.Ok(half):
            print(half)
        case Result.Err(message):
            print(message)
"#;

fn module() -> MirModule {
    let mir = lower_source_to_mir(SOURCE).expect("the source should lower");
    validated_for_emission(&mir).expect("the source should validate")
}

fn classes(module: &MirModule) -> HashMap<String, MirClass> {
    module
        .classes
        .iter()
        .map(|class| (class.name.clone(), class.clone()))
        .collect()
}

fn planned(module: &MirModule) -> ResultSlotPlan {
    let locals = crate::mir::ownership::explicit_locals(module);
    plan(module, &classes(module), &locals)
}

fn slots(plan: &ResultSlotPlan, function: &str) -> usize {
    plan.slots.get(function).map_or(0, Vec::len)
}

#[test]
fn a_function_whose_callers_match_or_unwrap_its_result_gets_a_twin() {
    let plan = planned(&module());
    let mut twins = plan.twins.iter().map(String::as_str).collect::<Vec<_>>();
    twins.sort_unstable();
    assert_eq!(twins, ["chain", "halves", "scalar"]);
    for function in ["scalar", "halves", "chain", "matched", "unwrapped", "main"] {
        assert!(slots(&plan, function) > 0, "`{function}` keeps a slot");
    }
}

#[test]
fn a_result_that_escapes_stays_a_box() {
    let plan = planned(&module());
    // `stored` returns a box it read; `printed` and `nothing` are entered
    // only where the result escapes.
    for function in ["stored", "printed", "nothing"] {
        assert!(!plan.twins.contains(function), "`{function}` has no twin");
    }
    // A result that goes into a list, a format, or comes from a callable
    // is no slot.
    for function in ["kept", "shown", "called", "stored"] {
        assert_eq!(slots(&plan, function), 0, "`{function}` keeps no slot");
    }
    // The caller of a function without a twin still keeps the result in a
    // slot: it unboxes the result.
    assert_eq!(slots(&plan, "looked"), 1);
}

#[test]
fn a_module_that_states_no_ownership_has_no_slots() {
    let mut module = module();
    module.ownership = MirOwnership::Implicit;
    let plan = planned(&module);
    assert!(plan.slots.is_empty() && plan.twins.is_empty());
}

fn named(name: &str) -> Type {
    Type::Named(name.to_string(), Vec::new())
}

fn result(ok: Type, err: Type) -> Type {
    Type::Named("Result".to_string(), vec![ok, err])
}

#[test]
fn a_slot_holds_scalars_and_single_boxes_only() {
    let classes = HashMap::new();
    let int64 = || named("int64");
    assert!(slot_parts(&result(int64(), named("str")), &classes).is_some());
    assert!(slot_parts(&result(Type::Unit, named("float64")), &classes).is_some());
    let union = crate::sema::optional_type(int64());
    assert!(slot_parts(&result(union, named("str")), &classes).is_none());
    assert!(slot_parts(&result(int64(), Type::TypeParam("T".to_string())), &classes).is_none());
    assert!(slot_parts(&int64(), &classes).is_none());
    assert!(slot_parts(&Type::Unit, &classes).is_none());
    assert!(slot_parts(&Type::Named("Lookup".to_string(), vec![int64()]), &classes).is_none());
    assert!(slot_parts(
        &Type::Named("Other".to_string(), vec![int64(), int64()]),
        &classes
    )
    .is_none());
}

#[test]
fn a_twin_returns_the_tag_and_the_payload_words() {
    let classes = HashMap::new();
    let words = |ok: Type, err: Type| {
        let (ok, err) = slot_parts(&result(ok, err), &classes).expect("a slot holds the result");
        twin_results(&ok, &err)
    };
    // Two payloads of one 64-bit word share it.
    assert_eq!(
        words(named("int64"), named("int64")),
        [types::I64, types::I64]
    );
    assert_eq!(
        words(named("int64"), named("str")),
        [types::I64, types::I64]
    );
    // `None` takes no word.
    assert_eq!(words(Type::Unit, named("str")), [types::I64, types::I64]);
    assert_eq!(words(Type::Unit, Type::Unit), [types::I64]);
    assert_eq!(
        words(named("float64"), named("int64")),
        [types::I64, types::F64, types::I64]
    );
}

#[test]
fn only_a_call_that_names_a_function_runs_a_known_function() {
    use crate::mir::{CallTarget, Operand, Rvalue};
    let call = Rvalue::Call {
        callee: CallTarget::Name("scalar".to_string()),
        args: Vec::new(),
        type_args: Default::default(),
    };
    assert_eq!(called_function(&call), Some("scalar"));
    assert_eq!(called_function(&Rvalue::Use(Operand::Unit)), None);
}
