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

#[test]
fn frozen_projection_fixture_emits_without_hosted_symbols() {
    let source = include_str!("../tests/fixtures/check-pass/compile_time_frozen_projections.au");
    let mir = crate::lower_source_to_mir(source).unwrap();
    let bytes = crate::native_codegen::emit_freestanding_object(
        &mir,
        "table.au",
        source,
        FreestandingTarget::Aarch64,
    )
    .expect("all of the table reads use freestanding data");
    use object::{Object, ObjectSymbol};
    let object = object::File::parse(bytes.as_slice()).unwrap();
    let imports: Vec<_> = object
        .symbols()
        .filter(|s| s.is_undefined())
        .map(|s| s.name().unwrap())
        .collect();
    assert_eq!(imports, ["aura_trap"]);
}

#[test]
fn unsupported_frozen_operations_keep_the_freestanding_subset_diagnostic() {
    let source = "const table = [11, 13]\nexport \"C\" def copy_size() -> int64:\n    return table.count(11)\n";
    let mir = crate::lower_source_to_mir(source).unwrap();
    let errors = crate::native_codegen::check_freestanding(&mir);
    assert!(
        errors.iter().any(|e| e.code == UNSUPPORTED
            && e.message
                .contains("a frozen aggregate in an operation that needs the hosted runtime")),
        "{errors:?}"
    );
}

#[test]
fn dotted_tuple_reads_expand_atomically_in_instructions_and_terminators() {
    let mut module = crate::lower_source_to_mir("def size() -> int64:\n    return 0\n").unwrap();
    let function = module
        .functions
        .iter_mut()
        .find(|f| f.name == "size")
        .unwrap();
    let word = Type::named("int64");
    let nested = Type::Tuple(vec![word.clone()]);
    let tuple = Type::Tuple(vec![nested.clone(), Type::named("Range")]);
    let mut types = HashMap::from([("table".into(), tuple)]);
    let addresses = HashSet::from(["table".into()]);
    let block = &mut function.blocks[0];
    block.instructions = vec![
        Instruction::Eval {
            value: Operand::MovePlace("table.0.0".into()),
        },
        Instruction::Eval {
            value: Operand::Place("table.0.0.extra".into()),
        },
    ];
    block.terminator = Terminator::ForRange {
        binding: "index".into(),
        iterable: Operand::Place("table.1".into()),
        body_label: "body".into(),
        exit_label: "end".into(),
    };

    assert!(places::expand(
        function,
        &addresses,
        &mut types,
        &HashMap::new()
    ));
    let block = &function.blocks[0];
    let [Instruction::Assign {
        target: outer,
        value: Rvalue::TupleElement { index: 0, .. },
    }, Instruction::Assign {
        target: inner,
        value:
            Rvalue::TupleElement {
                index: 0,
                tuple: Operand::Place(parent),
                ..
            },
    }, Instruction::Eval {
        value: Operand::MovePlace(moved),
    }, Instruction::Eval {
        value: Operand::Place(unchanged),
    }, Instruction::Assign {
        target: last,
        value: Rvalue::TupleElement { index: 1, .. },
    }] = block.instructions.as_slice()
    else {
        panic!("{:?}", block.instructions);
    };
    assert_eq!(outer, parent);
    assert_eq!(inner, moved);
    assert_eq!(types[outer], nested);
    assert_eq!(types[inner], word);
    assert_eq!(unchanged, "table.0.0.extra");
    assert!(
        matches!(&block.terminator, Terminator::ForRange { iterable: Operand::Place(name), .. } if name == last)
    );
    assert!(!places::expand(
        function,
        &addresses,
        &mut types,
        &HashMap::new()
    ));
}

#[test]
fn freestanding_emission_reports_a_missing_compiled_payload() {
    let source = "const table = [5, 9]\nexport \"C\" def at(index: int64) -> int64:\n    return table[index]\n";
    let module = crate::lower_source_to_mir(source).unwrap();
    let mut prepared = crate::native_codegen::freestanding::prepare(&module).unwrap();
    let constant = prepared.constants.pop().unwrap();
    // A reference to static storage still exists in the MIR. Removing its
    // payload must report the missing binding before the object can be linked.
    let error = crate::native_codegen::NativeCodegen::for_target(
        &prepared,
        "table.au",
        source,
        Some(FreestandingTarget::Aarch64),
    )
    .and_then(crate::native_codegen::NativeCodegen::emit)
    .unwrap_err();
    assert!(
        error.contains(&format!("missing frozen value for {}", constant.key)),
        "{error}"
    );
}
