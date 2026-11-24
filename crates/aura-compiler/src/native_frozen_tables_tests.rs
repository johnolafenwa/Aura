use super::*;
use crate::integer::IntegerValue;

fn word(bytes: &[u8], offset: usize) -> u64 {
    u64::from_le_bytes(bytes[offset..offset + 8].try_into().unwrap())
}

#[test]
fn nested_frozen_nodes_encode_distances_relative_to_the_containing_slot() {
    let word_type = Type::named("int64");
    let value = FrozenValue::Tuple(
        vec![],
        vec![
            FrozenValue::Set(
                word_type.clone(),
                vec![FrozenValue::Int(IntegerValue::from_i64(-9))],
            ),
            FrozenValue::Dict(
                word_type.clone(),
                word_type.clone(),
                vec![(
                    FrozenValue::Int(IntegerValue::from_u64(u64::MAX)),
                    FrozenValue::Bool(true),
                )],
            ),
            FrozenValue::Class(
                "P".into(),
                vec![("x".into(), FrozenValue::Float(1.25f64.to_bits()))],
            ),
            FrozenValue::Enum("E".into(), "Some".into(), vec![FrozenValue::Duration(7)]),
            FrozenValue::Union(word_type, 0, Box::new(FrozenValue::None)),
            FrozenValue::Bytes(vec![0x80, 0xff]),
            FrozenValue::Str("λa".into()),
        ],
    );
    let mut bytes = Vec::new();
    let base = append_node(&mut bytes, &value, &HashMap::new(), &HashMap::new());
    assert_eq!(word(&bytes, base), 7);
    let child = |i: usize| {
        let slot = base + (i + 1) * 8;
        slot + word(&bytes, slot) as usize
    };
    assert_eq!(word(&bytes, child(0) + 8), (-9i64) as u64);
    assert_eq!(word(&bytes, child(1) + 8), u64::MAX);
    assert_eq!(word(&bytes, child(1) + 16), 1);
    assert_eq!(word(&bytes, child(2) + 8), 1.25f64.to_bits());
    let duration_slot = child(3) + 8;
    let duration = duration_slot + word(&bytes, duration_slot) as usize;
    assert_eq!(&bytes[duration..duration + 16], &7i128.to_le_bytes());
    assert_eq!(word(&bytes, child(4) + 8), 0);
    assert_eq!(word(&bytes, child(5) + 16), 255);
    assert_eq!(word(&bytes, child(6)), 2);
    assert_eq!(word(&bytes, child(6) + 8), 3);
}

#[test]
fn signed_and_unsigned_wide_values_keep_their_high_words() {
    use crate::integer::IntegerKind;
    let signed = FrozenValue::Int(
        IntegerValue::from_typed_signed(-1_i128 << 122, IntegerKind::Int128).unwrap(),
    );
    let unsigned = FrozenValue::Int(
        IntegerValue::from_typed_unsigned(1_u128 << 124, IntegerKind::Uint128).unwrap(),
    );
    assert_eq!(wide_payload(&signed), Some((-1_i128 << 122).to_le_bytes()));
    assert_eq!(wide_payload(&unsigned), Some((1_u128 << 124).to_le_bytes()));
    assert_eq!(
        wide_payload(&FrozenValue::Int(IntegerValue::from_i64(-1))),
        None
    );
    assert_eq!(
        wide_payload(&FrozenValue::Int(IntegerValue::from_u64(1))),
        None
    );
}
