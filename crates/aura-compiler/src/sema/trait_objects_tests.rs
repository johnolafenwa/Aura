//! Trait objects in the checker and the interpreter. The fixtures also
//! run through both backends in the fixture and parity suites; these tests
//! keep the same programs under the library's own coverage.
use std::path::PathBuf;

use crate::{check_source, run_path, run_source};

macro_rules! run_pass {
    ($name:literal) => {
        (
            $name,
            include_str!(concat!("../../tests/fixtures/run-pass/", $name, ".au")),
            include_str!(concat!("../../tests/fixtures/run-pass/", $name, ".stdout")),
        )
    };
}

macro_rules! check_fail {
    ($name:literal, $code:literal, $message:literal) => {
        (
            $name,
            include_str!(concat!("../../tests/fixtures/check-fail/", $name, ".au")),
            $code,
            $message,
        )
    };
}

fn normalized(text: &str) -> String {
    text.replace("\r\n", "\n").trim_end().to_string()
}

#[test]
fn trait_object_programs_run() {
    let fixtures = [
        run_pass!("trait_object_assignment"),
        run_pass!("trait_object_builtin_impls"),
        run_pass!("trait_object_callables"),
        run_pass!("trait_object_containers"),
        run_pass!("trait_object_display"),
        run_pass!("trait_object_element_borrows"),
        run_pass!("trait_object_expressions"),
        run_pass!("trait_object_fields"),
        run_pass!("trait_object_generic_arguments"),
        run_pass!("trait_object_generic_bounds"),
        run_pass!("trait_object_mut_methods"),
        run_pass!("trait_object_own_receivers"),
        run_pass!("trait_object_returns"),
    ];
    for (name, source, expected) in fixtures {
        let output = run_source(source).unwrap_or_else(|error| panic!("{name}: {error:?}"));
        assert_eq!(normalized(&output.stdout), normalized(expected), "{name}");
    }
}

#[test]
fn trait_objects_cross_module_boundaries() {
    let fixtures = [
        run_pass!("trait_object_imported"),
        run_pass!("trait_object_imported_namespace"),
        run_pass!("trait_object_imported_renamed"),
    ];
    for (name, _, expected) in fixtures {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/run-pass")
            .join(format!("{name}.au"));
        let output = run_path(&path).unwrap_or_else(|error| panic!("{name}: {error:?}"));
        assert_eq!(normalized(&output.stdout), normalized(expected), "{name}");
    }
}

#[test]
fn trait_object_misuse_is_rejected() {
    let fixtures = [
        check_fail!(
            "trait_object_element_append",
            "AU3005",
            "cannot implicitly copy `dyn Sink` out of a list index; trait object `dyn Sink` cannot be c"
        ),
        check_fail!(
            "trait_object_element_bind",
            "AU3005",
            "cannot implicitly copy `dyn Sink` out of a list index; trait object `dyn Sink` cannot be c"
        ),
        check_fail!(
            "trait_object_element_into_field",
            "AU3005",
            "cannot implicitly copy `dyn Sink` out of a list index; trait object `dyn Sink` cannot be c"
        ),
        check_fail!(
            "trait_object_element_into_union",
            "AU3005",
            "cannot implicitly copy `dyn Sink` out of a list index; trait object `dyn Sink` cannot be c"
        ),
        check_fail!(
            "trait_object_element_own_argument",
            "AU3005",
            "cannot implicitly copy `dyn Sink` out of a list index; trait object `dyn Sink` cannot be c"
        ),
        check_fail!(
            "trait_object_element_own_receiver",
            "AU3005",
            "cannot implicitly copy `dyn Sink` out of a list index; trait object `dyn Sink` cannot be c"
        ),
        check_fail!(
            "trait_object_entry_bind",
            "AU3005",
            "cannot implicitly copy `dyn Sink` out of a dict index; trait object `dyn Sink` cannot be c"
        ),
        check_fail!(
            "trait_object_impl_target",
            "AU2050",
            "trait `Extra` cannot be implemented for the trait object type `dyn Shape`"
        ),
        check_fail!(
            "trait_object_list_not_converted",
            "AU2002",
            "argument type mismatch for function `total`: expected `dyn Shape`, found `Square`"
        ),
        check_fail!(
            "trait_object_method_not_callable",
            "AU2051",
            "method `map` of trait `Merge` cannot be called on `dyn Merge`: it has type parameters of i"
        ),
        check_fail!(
            "trait_object_moves",
            "AU3001",
            "use of moved value `a`"
        ),
        check_fail!(
            "trait_object_mut_parameter_needs_object",
            "AU2051",
            "a `mut` parameter of type `dyn Shape` takes a `dyn Shape` variable, found `Square`"
        ),
        check_fail!(
            "trait_object_needs_trait",
            "AU2050",
            "`dyn` needs a trait, and `Square` is not a trait"
        ),
        check_fail!(
            "trait_object_no_clone",
            "AU3007",
            "cannot clone `dyn Shape` because a trait object cannot be cloned"
        ),
        check_fail!(
            "trait_object_no_dict_key",
            "AU2008",
            "cannot use `dyn Shape` as a dict key because trait object `dyn Shape` does not define equa"
        ),
        check_fail!(
            "trait_object_no_downcast",
            "AU2002",
            "binding `b` has annotated type `Square`, but value has type `dyn Shape`"
        ),
        check_fail!(
            "trait_object_no_equality",
            "AU2008",
            "cannot compare `dyn Shape` because trait object `dyn Shape` does not define equality"
        ),
        check_fail!(
            "trait_object_no_fields",
            "AU2002",
            "`dyn Shape` has no field `side`"
        ),
        check_fail!(
            "trait_object_no_list_copy",
            "AU3007",
            "cannot use `list.copy` because trait object `dyn Shape` cannot be cloned"
        ),
        check_fail!(
            "trait_object_no_set_element",
            "AU2008",
            "cannot use `dyn Shape` as a set element because trait object `dyn Shape` does not define e"
        ),
        check_fail!(
            "trait_object_not_transfer",
            "AU3008",
            "task argument `shape` cannot cross a task boundary because `dyn Shape` is a trait object, "
        ),
        check_fail!(
            "trait_object_reserved_name",
            "AU2002",
            "`dyn` is a reserved built-in type name"
        ),
        check_fail!(
            "trait_object_source_lacks_impl",
            "AU2002",
            "binding `a` has annotated type `dyn Shape`, but value has type `Other`"
        ),
        check_fail!(
            "trait_object_unknown_method",
            "AU2001",
            "`dyn Shape` has no method `volume`"
        ),
    ];
    for (name, source, code, message) in fixtures {
        let error = match check_source(source) {
            Ok(_) => panic!("{name} should be rejected"),
            Err(error) => error,
        };
        assert_eq!(error.code, code, "{name}: {error:?}");
        assert!(error.message.contains(message), "{name}: {error:?}");
    }
}

const SQUARE: &str = "trait Shape:\n    def area(self) -> int64\n\nclass Square:\n    side: int64\n\nimpl Shape for Square:\n    def area(self) -> int64:\n        return self.side * self.side\n\n";

fn rejected(body: &str) -> crate::diag::Diagnostic {
    match check_source(&format!("{SQUARE}{body}")) {
        Ok(_) => panic!("should be rejected:\n{body}"),
        Err(error) => error,
    }
}

#[test]
fn a_trait_object_type_names_one_trait_without_associated_types() {
    let cases = [
        (
            "def main() -> None:\n    a: Shape = Square(side=1)\n    print(1)\n",
            "AU2050",
            "`Shape` is a trait, not a type",
        ),
        (
            "def main() -> None:\n    a: dyn Square = Square(side=1)\n    print(1)\n",
            "AU2050",
            "`dyn` needs a trait, and `Square` is not a trait",
        ),
        (
            "trait Source:\n    type Item\n    def next(mut self) -> Self.Item\n\ndef main() -> None:\n    items: list[dyn Source] = []\n    print(len(items))\n",
            "AU2050",
            "trait `Source` has associated types, so it cannot be a trait object",
        ),
        (
            "trait Reader[T]:\n    def read(self) -> T\n\ndef main() -> None:\n    items: list[dyn Reader] = []\n    print(len(items))\n",
            "AU2050",
            "trait `Reader` expects 1 type argument, found 0",
        ),
        (
            "def main() -> None:\n    items: list[dyn Shape[int64]] = []\n    print(len(items))\n",
            "AU2050",
            "trait `Shape` expects 0 type arguments, found 1",
        ),
        (
            "def main() -> None:\n    a: dyn = 1\n    print(a)\n",
            "AU2001",
            "unknown type `dyn`",
        ),
        (
            "class Node:\n    next: indirect dyn Shape\n\ndef main() -> None:\n    print(1)\n",
            "AU1101",
            "`indirect` is not valid on a trait object type",
        ),
        (
            "def main() -> None:\n    a: dyn[Shape] = Square(side=1)\n    print(1)\n",
            "AU1101",
            "a trait object type is written `dyn Trait`, without brackets",
        ),
    ];
    for (body, code, message) in cases {
        let error = rejected(body);
        assert_eq!(error.code, code, "{error:?}");
        assert!(error.message.contains(message), "{error:?}");
    }
}

#[test]
fn only_methods_one_call_can_serve_are_callable_on_a_trait_object() {
    let same = "trait Same:\n    def same(self, other: Self) -> bool\n    def size(self) -> int64\n\nimpl Same for Square:\n    def same(self, other: Self) -> bool:\n        return self.side == other.side\n    def size(self) -> int64:\n        return self.side\n\n";
    // The other methods of the trait stay callable.
    check_source(&format!(
        "{SQUARE}{same}def main() -> None:\n    a: dyn Same = Square(side=1)\n    print(a.size())\n"
    ))
    .expect("a trait object of a trait with one uncallable method");
    let error = rejected(&format!(
        "{same}def main() -> None:\n    a: dyn Same = Square(side=1)\n    print(a.same(Square(side=1)))\n"
    ));
    assert_eq!(error.code, "AU2051", "{error:?}");
    assert!(
        error.message.contains("its signature names `Self`"),
        "{error:?}"
    );

    let error = rejected(
        "trait Named:\n    def kind() -> str\n    def size(self) -> int64\n\nimpl Named for Square:\n    def kind() -> str:\n        return \"square\"\n    def size(self) -> int64:\n        return self.side\n\ndef main() -> None:\n    a: dyn Named = Square(side=1)\n    print(a.kind())\n",
    );
    assert_eq!(error.code, "AU2051", "{error:?}");
    assert!(error.message.contains("it takes no `self`"), "{error:?}");

    // A type parameter bounded by the trait promises every method, so a
    // trait object that cannot answer one of them cannot stand for it.
    let error = rejected(&format!(
        "{same}def twice[T: Same](item: T) -> bool:\n    return item.same(item)\n\ndef main() -> None:\n    a: dyn Same = Square(side=1)\n    print(twice(a))\n"
    ));
    assert_eq!(error.code, "AU2051", "{error:?}");
    assert!(
        error
            .message
            .contains("`dyn Same` cannot stand for a type parameter bounded by `Same`"),
        "{error:?}"
    );
}

#[test]
fn a_value_converts_to_a_trait_object_only_where_its_type_implements_the_trait() {
    let error = rejected(
        "def one() -> int64:\n    return 1\n\ndef main() -> None:\n    a: dyn Shape = one\n    print(1)\n",
    );
    assert_eq!(error.code, "AU2002", "{error:?}");
    assert!(
        error
            .help
            .iter()
            .any(|help| help.contains("a function value does not implement a trait")),
        "{error:?}"
    );

    let error = rejected(
        "class Other:\n    x: int64\n\ndef make() -> dyn Shape:\n    return Other(x=1)\n\ndef main() -> None:\n    print(make().area())\n",
    );
    assert!(
        error
            .message
            .contains("return type mismatch: expected `dyn Shape`, found `Other`"),
        "{error:?}"
    );
    assert!(
        error
            .help
            .iter()
            .any(|help| help.contains("`Other` does not implement trait `Shape`")),
        "{error:?}"
    );

    // A value that fits two trait object members of a union has no one
    // member to become.
    let error = rejected(
        "trait Other:\n    def other(self) -> int64\n\nimpl Other for Square:\n    def other(self) -> int64:\n        return 2\n\ndef main() -> None:\n    a: dyn Shape | dyn Other = Square(side=1)\n    print(1)\n",
    );
    assert_eq!(error.code, "AU2011", "{error:?}");
    assert!(
        error.message.contains("converts to multiple members"),
        "{error:?}"
    );
}

#[test]
fn a_trait_object_is_an_ordinary_value_elsewhere() {
    let programs = [
        // `dyn` stays an ordinary name outside a type.
        ("def main() -> None:\n    dyn = 3\n    print(dyn)\n", "3\n"),
        (
            "def area_or(shape: dyn Shape = Square(side=3)) -> int64:\n    return shape.area()\n\ndef main() -> None:\n    print(area_or())\n",
            "9\n",
        ),
        (
            "UNIT: dyn Shape = Square(side=1)\n\ndef main() -> None:\n    print(UNIT.area())\n",
            "1\n",
        ),
        (
            "trait Grow:\n    def grow(mut self) -> None\n    def size(self) -> int64\n\nimpl Grow for Square:\n    def grow(mut self) -> None:\n        self.side += 1\n    def size(self) -> int64:\n        return self.side\n\ndef bump(item: mut dyn Grow) -> None:\n    item.grow()\n\ndef main() -> None:\n    mut a: dyn Grow = Square(side=1)\n    bump(a)\n    print(a.size())\n",
            "2\n",
        ),
        (
            "def main() -> None:\n    maybe: dyn Shape | None = Square(side=4)\n    if maybe is not None:\n        print(maybe.area())\n",
            "16\n",
        ),
    ];
    for (body, expected) in programs {
        let output = run_source(&format!("{SQUARE}{body}"))
            .unwrap_or_else(|error| panic!("{body}: {error:?}"));
        assert_eq!(output.stdout, expected, "{body}");
    }
}
