use std::sync::{Arc, Mutex};

use crate::mir::{CallTarget, Instruction, MethodDispatch, MirFunction, MirModule, Rvalue};
use crate::sema::Type;

const GENERICS: &str = r#"
class Box[T]:
    value: T

    def get(own self) -> T:
        return self.value

def identity[T](value: own T) -> T:
    return value

def first_or[T](values: own list[T], fallback: own T) -> T:
    mut items = values
    if items.len() == 0:
        return fallback
    return items.pop(0)

def describe[T](value: T, label: own str = "value") -> str:
    show: def(T) -> str = lambda item: f"{label}={item}"
    return show(value)

def twice[T](value: T) -> str:
    plain = describe(value)
    named = describe(value, label="again")
    return f"{plain} {named}"

def present[T](value: T | None) -> bool:
    return value is not None

def wrap[T](value: own T) -> T | int64:
    return value

def nest[T](value: own T, depth: int64) -> int64:
    if depth == 0:
        return 0
    return 1 + nest([value], depth - 1)

def main():
    print(identity(3))
    print(identity("x"))
    print(first_or([1, 2], 0))
    box = Box[str](value="boxed")
    print(box.get())
    print(twice(2.5))
    print(present("x"))
    missing: str | None = None
    print(present(missing))
    print(wrap("s"))
    print(wrap(5))
    print(nest(1, 40))
"#;

const OUTPUT: &str = "3\nx\n1\nboxed\nvalue=2.5 again=2.5\ntrue\nfalse\ns\n5\n40\n";

fn lowered(source: &str) -> MirModule {
    crate::lower_source_to_mir(source).expect("the source lowers")
}

fn function<'a>(module: &'a MirModule, name: &str) -> &'a MirFunction {
    module
        .functions
        .iter()
        .find(|function| function.name == name)
        .unwrap_or_else(|| panic!("the module has no `{name}`"))
}

fn has_function(module: &MirModule, name: &str) -> bool {
    module
        .functions
        .iter()
        .any(|function| function.name == name)
}

/// The functions `caller` calls by name or runs as a static method.
fn callees(module: &MirModule, caller: &str) -> Vec<String> {
    function(module, caller)
        .blocks
        .iter()
        .flat_map(|block| &block.instructions)
        .filter_map(|instruction| match instruction {
            Instruction::Assign {
                value: Rvalue::Call { callee, .. },
                ..
            } => match callee {
                CallTarget::Name(name) => Some(name.clone()),
                CallTarget::Member {
                    dispatch: Some(MethodDispatch::Static { function }),
                    ..
                } => Some(function.clone()),
                _ => None,
            },
            _ => None,
        })
        .collect()
}

fn param_types(function: &MirFunction) -> Vec<String> {
    function
        .params
        .iter()
        .map(|param| param.ty.to_string())
        .collect()
}

fn interpreted_output(module: &MirModule) -> String {
    let captured = Arc::new(Mutex::new(String::new()));
    let sink_capture = captured.clone();
    let sink: crate::StdoutSink = Arc::new(move |chunk| {
        sink_capture
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .push_str(chunk);
    });
    crate::run_mir_with_stdout_sink(module, Some(sink)).expect("the module runs");
    let output = captured
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .clone();
    output
}

#[test]
fn concrete_calls_run_copies_with_concrete_types() {
    let module = lowered(GENERICS);
    let main = callees(&module, "main");
    for copy in [
        "identity[int64]",
        "identity[str]",
        "first_or[int64]",
        "twice[float64]",
    ] {
        assert!(main.contains(&copy.to_string()), "main calls {main:?}");
    }
    assert_eq!(param_types(function(&module, "identity[str]")), ["str"]);
    assert_eq!(
        function(&module, "identity[str]").return_type,
        Type::named("str")
    );
    assert_eq!(
        param_types(function(&module, "first_or[int64]")),
        ["list[int64]", "int64"]
    );
    // A copy's calls run copies too, keyword arguments included.
    assert_eq!(
        callees(&module, "twice[float64]"),
        ["describe[float64]", "describe[float64]"]
    );
    // The generic originals stay for calls that cannot be specialized.
    assert!(has_function(&module, "identity"));
    assert_eq!(callees(&module, "twice"), ["describe", "describe"]);
}

#[test]
fn copies_bring_their_lambdas_defaults_and_methods() {
    let module = lowered(GENERICS);
    let describe = function(&module, "describe[float64]");
    assert_eq!(param_types(describe), ["float64", "str"]);
    let default = describe.params[1]
        .default_function
        .as_deref()
        .expect("`label` has a default");
    assert!(
        default.starts_with("describe[float64]::__default_"),
        "{default}"
    );
    assert!(has_function(&module, default));
    let lambda = module
        .functions
        .iter()
        .find(|function| function.name.starts_with("describe[float64]::__lambda"))
        .expect("the copy brings its lambda");
    // The lambda takes its captured `label`, then its item.
    assert_eq!(param_types(lambda), ["str", "float64"]);

    // `box.get()` runs the copy of `get` for `Box[str]`.
    let get = callees(&module, "main")
        .into_iter()
        .find(|name| name.ends_with("[str]") && name.contains("get"))
        .expect("main runs a copy of `get`");
    assert_eq!(function(&module, &get).return_type, Type::named("str"));
}

#[test]
fn a_type_parameter_beside_none_binds_the_other_member() {
    let module = lowered(GENERICS);
    let present = function(&module, "present[str]");
    assert_eq!(param_types(present), ["str | None"]);
    // The copy tests the union's tag instead of the runtime value.
    let tests = present
        .blocks
        .iter()
        .flat_map(|block| &block.instructions)
        .filter_map(|instruction| match instruction {
            Instruction::Assign { value, .. } => Some(value),
            _ => None,
        })
        .filter(|value| matches!(value, Rvalue::NoneTest { .. } | Rvalue::UnionTagTest { .. }))
        .collect::<Vec<_>>();
    assert!(
        matches!(tests.as_slice(), [Rvalue::UnionTagTest { .. }]),
        "{tests:?}"
    );
    assert_eq!(
        callees(&module, "main")
            .iter()
            .filter(|name| *name == "present[str]")
            .count(),
        2
    );
}

#[test]
fn a_copy_that_would_merge_union_members_is_not_made() {
    let module = lowered(GENERICS);
    assert!(has_function(&module, "wrap[str]"));
    // `int64 | int64` is one type, so the union's tags would change.
    assert!(!has_function(&module, "wrap[int64]"));
    let main = callees(&module, "main");
    assert!(main.contains(&"wrap[str]".to_string()));
    assert!(main.contains(&"wrap".to_string()));
}

#[test]
fn deeply_nested_type_arguments_stop_specializing() {
    let module = lowered(GENERICS);
    let copies = module
        .functions
        .iter()
        .filter(|function| function.name.starts_with("nest["))
        .count();
    assert!((1..=32).contains(&copies), "{copies} copies of `nest`");
}

#[test]
fn specialized_modules_validate_and_run_as_before() {
    let mut module = lowered(GENERICS);
    assert_eq!(interpreted_output(&module), OUTPUT);
    crate::mir::validate_at_boundary(&mut module)
        .unwrap_or_else(|error| panic!("the module validates: {error:?}"));
    crate::native_codegen::emit_host_object(&module).expect("the module compiles");
    // `aura run` lowers without specializing.
    let program = crate::check_source(GENERICS).expect("the source checks");
    let implicit = crate::mir::lower_implicit(&program);
    assert!(!implicit
        .functions
        .iter()
        .any(|function| function.name.contains('[')));
}

#[test]
fn backtraces_name_the_function_a_copy_was_made_from() {
    for (name, public) in [
        ("identity[int64]", "identity"),
        ("pair[int64, list[str]]", "pair"),
        ("describe[float64]::__default_label", "describe"),
        ("describe[float64]::__lambda_3_9", "describe::__lambda_3_9"),
        ("json::decode[Point][Point]", "json::decode"),
        ("main", "main"),
    ] {
        assert_eq!(crate::mir::public_function_name(name), public);
    }
}
