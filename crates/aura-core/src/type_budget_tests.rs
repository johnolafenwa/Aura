use std::collections::HashMap;

use super::*;

const SPAN: Span = Span { line: 3, column: 7 };

#[test]
fn a_depth_counter_at_its_ceiling_reports_a_capacity_error() {
    let budget = ExpansionBudget::default();
    budget.state.borrow_mut().depth = usize::MAX;
    let Err(error) = budget.enter(SPAN) else {
        panic!("the depth cannot grow");
    };
    assert!(error.message.contains("depth"), "{error:?}");
    assert_eq!(budget.state.borrow().depth, usize::MAX);
}

#[test]
fn an_aggregate_counter_at_its_ceiling_reports_the_aggregate_limit() {
    let budget = ExpansionBudget::default();
    budget.state.borrow_mut().aggregate_nodes = usize::MAX;
    let template = Type::Tuple(Vec::new());
    let error = budget
        .check_substitution(&template, &HashMap::new(), SPAN)
        .expect_err("the aggregate count cannot grow");
    assert_eq!(
        error.message,
        aggregate_error(SPAN, budget.limits.aggregate_nodes).message
    );
}

#[test]
fn key_bytes_that_overflow_report_the_byte_limit() {
    let mut bytes = usize::MAX;
    let error = add_key_bytes(&mut bytes, 1, 64, SPAN).expect_err("the byte count overflows");
    assert_eq!(error.message, key_limit_error(SPAN, "byte", 64).message);

    let mut bytes = 60;
    add_key_bytes(&mut bytes, 4, 64, SPAN).expect("the count reaches the limit exactly");
    assert_eq!(bytes, 64);
    let error = add_key_bytes(&mut bytes, 1, 64, SPAN).expect_err("the count passes the limit");
    assert_eq!(error.message, key_limit_error(SPAN, "byte", 64).message);
}

#[test]
fn stacks_that_cannot_grow_report_a_capacity_error() {
    let mut stack = Vec::new();
    let error = reserve_stack(&mut stack, usize::MAX, SPAN).expect_err("the request is too large");
    assert!(error.message.contains("type-expansion"), "{error:?}");
    reserve_stack(&mut stack, 4, SPAN).expect("a small request fits");
    assert!(stack.capacity() >= 4);

    let mut keys = Vec::new();
    let error =
        reserve_key_stack(&mut keys, usize::MAX, SPAN).expect_err("the request is too large");
    assert!(error.message.contains("type-key"), "{error:?}");
    reserve_key_stack(&mut keys, 4, SPAN).expect("a small request fits");
    assert!(keys.capacity() >= 4);
}
