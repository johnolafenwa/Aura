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
    let read_site = (11, 17);
    let rewritten = rewrite_module(&mut module, &BTreeSet::from([read_site]));
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

    let unchanged = rewrite_module(&mut module, &BTreeSet::from([(99, 1)]));
    assert!(unchanged.is_empty());
}
