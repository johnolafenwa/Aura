use super::*;
use std::collections::BTreeMap;

fn union(members: Vec<Type>) -> UnionType {
    match Type::normalize_union(members, "main", &BTreeMap::new()).expect("normalized union") {
        Type::Union(union) => *union,
        other => panic!("expected a union, found {other}"),
    }
}

#[test]
fn scalar_and_unit_members_take_a_one_byte_tag_and_the_widest_payload() {
    let plan = plan_union_layout(&union(vec![Type::named("int64"), Type::Unit]), |ty| {
        ty == &Type::named("int64")
    });
    assert_eq!(plan.tag_width, 1);
    assert_eq!(plan.payload_size, 8);
    assert_eq!(plan.payload_align, 8);
    assert_eq!(plan.payload_offset, 8);
    assert_eq!(plan.size, 16);
    assert_eq!(plan.align, 8);
    assert!(plan.copy);
    assert_eq!(
        plan.members
            .iter()
            .map(|member| member.tag)
            .collect::<Vec<_>>(),
        vec![0, 1]
    );
    let unit = plan
        .members
        .iter()
        .find(|member| member.ty == Type::Unit)
        .expect("unit member");
    assert_eq!(
        (unit.size, unit.align, unit.copy, unit.needs_drop),
        (0, 1, true, false)
    );
    assert_eq!(plan.layout_version, UNION_LAYOUT_VERSION);
}

#[test]
fn boxed_members_are_one_pointer_and_own_cleanup() {
    let plan = plan_union_layout(
        &union(vec![Type::named("str"), Type::named("int32")]),
        |ty| ty == &Type::named("int32"),
    );
    let text = plan
        .members
        .iter()
        .find(|member| member.ty == Type::named("str"))
        .expect("str member");
    assert_eq!(text.size, std::mem::size_of::<usize>() as u64);
    assert!(text.needs_drop && !text.copy);
    assert!(!plan.copy);
    assert_eq!(plan.payload_align, std::mem::size_of::<usize>() as u64);
}

#[test]
fn wide_members_align_the_payload_and_round_the_size() {
    let plan = plan_union_layout(
        &union(vec![Type::named("Duration"), Type::named("bool")]),
        |_| true,
    );
    assert_eq!(plan.payload_align, 16);
    assert_eq!(plan.payload_offset, 16);
    assert_eq!(plan.size, 32);
    assert_eq!(plan.align, 16);
}

#[test]
fn tag_widths_follow_the_member_count() {
    assert_eq!(tag_width_for(2), 1);
    assert_eq!(tag_width_for(256), 1);
    assert_eq!(tag_width_for(257), 2);
    assert_eq!(tag_width_for(65_536), 2);
    assert_eq!(tag_width_for(65_537), 4);
}

#[test]
fn a_computed_plan_validates_and_a_forged_plan_fails_closed() {
    let plan = plan_union_layout(
        &union(vec![Type::named("int64"), Type::named("str"), Type::Unit]),
        |ty| ty == &Type::named("int64"),
    );
    validate_union_layout(&plan).expect("computed plans validate");

    let mut wrong_tag = plan.clone();
    wrong_tag.members.swap(0, 1);
    assert!(validate_union_layout(&wrong_tag)
        .unwrap_err()
        .contains("canonical member"));

    let mut wrong_width = plan.clone();
    wrong_width.tag_width = 2;
    assert!(validate_union_layout(&wrong_width)
        .unwrap_err()
        .contains("layout rule"));

    let mut wrong_drop = plan.clone();
    wrong_drop.members[1].needs_drop = false;
    assert!(validate_union_layout(&wrong_drop).is_err());

    let mut stale = plan.clone();
    stale.layout_version = UNION_LAYOUT_VERSION + 1;
    assert!(validate_union_layout(&stale)
        .unwrap_err()
        .contains("layout version"));

    let mut other_target = plan;
    other_target.pointer_width = 2;
    assert!(validate_union_layout(&other_target)
        .unwrap_err()
        .contains("pointer width"));
}
