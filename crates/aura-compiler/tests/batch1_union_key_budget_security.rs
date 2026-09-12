//! Bounded regression for canonical union-key representation growth.

use aura_compiler::{check_source, sema::Type};

const NESTING_LEVELS: usize = 8;

fn nested_union_key_lengths() -> Vec<usize> {
    let mut source = "type U0 = int64 | str\n".to_string();
    for level in 1..=NESTING_LEVELS {
        source.push_str(&format!("type U{level} = list[U{}] | None\n", level - 1));
    }
    for level in 0..=NESTING_LEVELS {
        source.push_str(&format!("def accept{level}(value: U{level}):\n    pass\n"));
    }

    let program = check_source(&source).expect("the bounded alias family must check");
    (0..=NESTING_LEVELS)
        .map(|level| {
            let ty = &program.functions[&format!("accept{level}")]
                .signature
                .params[0];
            let encoded = serde_json::to_value(ty).unwrap();
            encoded["Union"]["keys"]
                .as_array()
                .expect("every level remains a two-member union")
                .iter()
                .map(|key| key.as_str().unwrap().len())
                .max()
                .unwrap()
        })
        .collect()
}

#[test]
fn nested_union_canonical_keys_grow_with_structure_not_repeated_string_escaping() {
    let lengths = nested_union_key_lengths();
    let base = lengths[0];
    let linear_allowance_per_level = 256;

    for (level, &length) in lengths.iter().enumerate() {
        assert!(
            length <= base + level * linear_allowance_per_level,
            "canonical key length must remain linear in structural depth; level {level} grew from {base} to {length} bytes; observed {lengths:?}"
        );
    }
}

#[test]
fn bounded_control_still_resolves_to_a_union_type() {
    let mut source = "type U0 = int64 | str\n".to_string();
    for level in 1..=2 {
        source.push_str(&format!("type U{level} = list[U{}] | None\n", level - 1));
    }
    source.push_str("def accept(value: U2):\n    pass\n");

    let ty = &check_source(&source).unwrap().functions["accept"]
        .signature
        .params[0];
    assert!(matches!(ty, Type::Union(_)));
}
