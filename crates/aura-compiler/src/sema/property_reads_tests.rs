use std::collections::BTreeSet;

use super::property_reads::rewrite_module;
use crate::ast::{ExprKind, Item, Stmt};

#[test]
fn rewriting_skips_extern_items_and_turns_recorded_reads_into_getter_calls() {
    let source = "\
extern \"C\" def current_value() -> int64

class Gauge:
    level: int64

    @property
    def doubled(self) -> int64:
        return self.level * 2

def main():
    gauge = Gauge(level=current_value())
    print(gauge.doubled)
";
    let mut module = crate::parser::parse(source).expect("module should parse");
    assert!(module
        .items
        .iter()
        .any(|item| matches!(item, Item::ExternFunction(_))));
    let read_site = {
        let Some(Item::Function(main)) = module
            .items
            .iter()
            .find(|item| matches!(item, Item::Function(function) if function.name == "main"))
        else {
            panic!("main should be declared");
        };
        let Stmt::Expr(print) = &main.body[1] else {
            panic!("the second statement should be the print call");
        };
        let ExprKind::Call { args, .. } = &print.expr.kind else {
            panic!("print should be a call");
        };
        assert!(
            matches!(&args[0].value.kind, ExprKind::Member { field, .. } if field == "doubled")
        );
        (args[0].value.span.line, args[0].value.span.column)
    };
    let rewritten = rewrite_module(&mut module, &BTreeSet::from([read_site]), &BTreeSet::new());
    assert_eq!(rewritten, BTreeSet::from([read_site]));

    let Some(Item::Function(main)) = module
        .items
        .iter()
        .find(|item| matches!(item, Item::Function(function) if function.name == "main"))
    else {
        panic!("main should be declared");
    };
    let Stmt::Expr(print) = &main.body[1] else {
        panic!("the second statement should be the print call");
    };
    let ExprKind::Call { args, .. } = &print.expr.kind else {
        panic!("print should stay a call");
    };
    let ExprKind::Call {
        callee,
        args: getter_args,
    } = &args[0].value.kind
    else {
        panic!("the property read should become a getter call");
    };
    assert!(getter_args.is_empty());
    assert!(matches!(&callee.kind, ExprKind::Member { field, .. } if field == "doubled"));

    let unchanged = rewrite_module(&mut module, &BTreeSet::from([(99, 1)]), &BTreeSet::new());
    assert!(unchanged.is_empty());
}

#[test]
fn property_setter_reads_and_writes_evaluate_in_source_order() {
    let source = include_str!("../../tests/fixtures/run-pass/property_setter.au");
    let expected = include_str!("../../tests/fixtures/run-pass/property_setter.stdout");
    assert_eq!(crate::run_source(source).unwrap().stdout, expected);
}

#[test]
fn property_setter_signature_matches_the_getter() {
    let source = include_str!("../../tests/fixtures/check-fail/property_setter_signature.au");
    assert_eq!(crate::check_source(source).unwrap_err().code, "AU2064");
}

#[test]
fn property_setter_rejects_invalid_declarations() {
    let declaration = include_str!("../../tests/fixtures/parse-pass/property_setter.au");
    for (before, after, message) in [
        (
            "def count(mut self,",
            "def other(mut self,",
            "method named `count`",
        ),
        ("    @property\n", "", "matching @property getter"),
        (
            "def count(mut self,",
            "def count(self,",
            "`mut self` receiver",
        ),
        (
            "def count(mut self,",
            "def count[T](mut self,",
            "cannot be generic",
        ),
        (
            "value: int64):",
            "value: int64) -> int64:",
            "return a value",
        ),
        ("value: int64", "value: mut int64", "take one value"),
        ("value: int64", "value: int64 = 1", "take one value"),
        ("value: int64", "*, value: int64", "take one value"),
        (
            "value: int64",
            "value: int64, other: int64",
            "take one value",
        ),
        (
            "    def count(mut self, value: int64):",
            "    next: int64",
            "precede a method",
        ),
    ] {
        let source = declaration.replace(before, after);
        let diagnostic = crate::check_source(&source).unwrap_err();
        assert_eq!(diagnostic.code, "AU2064", "{source}: {diagnostic}");
        assert!(diagnostic.message.contains(message), "{diagnostic}");
    }
    let duplicate = format!(
        "{declaration}\n    @count.setter\n    def count(mut self, value: int64):\n        pass\n"
    );
    let diagnostic = crate::check_source(&duplicate).unwrap_err();
    assert_eq!(diagnostic.code, "AU2064");
    assert!(diagnostic.message.contains("duplicate setter"));
}

#[test]
fn property_setter_requires_mutable_place_and_checks_rhs() {
    let declaration = include_str!("../../tests/fixtures/parse-pass/property_setter.au");
    for (binding, assignment) in [
        ("counter", "counter.count = 1"),
        ("counter", "counter.count += 1"),
        ("mut counter", "counter.count = \"invalid\""),
    ] {
        let source = format!(
            "{declaration}\ndef main():\n    {binding} = Counter(stored=0)\n    {assignment}\n"
        );
        crate::check_source(&source).expect_err(&source);
    }
}

#[test]
fn property_setter_accepts_generic_getters_and_reborrows() {
    let source = "
class Slot[T]:
    tag: T
    stored: int64
    @value.setter
    def value(mut self, replacement: own int64):
        self.stored = replacement
    @property
    def value(self) -> int64:
        return self.stored

class Container:
    slot: Slot[str]

def main():
    mut box = Container(slot=Slot(tag=\"tag\", stored=3))
    box.slot.value += box.slot.value
    print(box.slot.value)
";
    assert_eq!(crate::run_source(source).unwrap().stdout, "6\n");
}
