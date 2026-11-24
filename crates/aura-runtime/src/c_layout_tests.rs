use super::*;
use crate::ffi::FfiScalar::{self, *};

fn scalars(name: &str, fields: &[FfiScalar]) -> CStruct {
    CStruct {
        name: name.to_string(),
        fields: fields
            .iter()
            .enumerate()
            .map(|(index, scalar)| CField {
                name: format!("f{index}"),
                kind: CFieldKind::Scalar(*scalar),
            })
            .collect(),
        layout_controls: Default::default(),
        union: false,
    }
}

/// `fields` of the given kinds, as a struct or as the members of a union.
fn of_kinds(name: &str, union: bool, kinds: Vec<CFieldKind>) -> CStruct {
    CStruct {
        name: name.to_string(),
        fields: kinds
            .into_iter()
            .enumerate()
            .map(|(index, kind)| CField {
                name: format!("f{index}"),
                kind,
            })
            .collect(),
        layout_controls: Default::default(),
        union,
    }
}

fn array(kind: CFieldKind, count: usize) -> CFieldKind {
    CFieldKind::Array(Box::new(kind), count)
}

fn with_struct(name: &str, before: &[FfiScalar], inner: CStruct, after: &[FfiScalar]) -> CStruct {
    let mut outer = scalars(name, before);
    outer.fields.push(CField {
        name: "inner".to_string(),
        kind: CFieldKind::Struct(inner),
    });
    outer.fields.extend(scalars(name, after).fields);
    outer
}

fn int(offset: usize, size: usize) -> CPiece {
    CPiece::Int { offset, size }
}

fn sse(offset: usize, size: usize) -> CPiece {
    CPiece::Sse { offset, size }
}

fn float(offset: usize, scalar: FfiScalar) -> CPiece {
    CPiece::Float { offset, scalar }
}

#[test]
fn a_struct_is_laid_out_as_c_lays_it_out() {
    // struct { int8_t a; int64_t b; int16_t c; }
    let padded = scalars("Padded", &[I8, I64, I16]);
    assert_eq!(padded.field_offsets(), [0, 8, 16]);
    assert_eq!((padded.size(), padded.align()), (24, 8));

    // struct { int32_t a; int8_t b; }
    let tail = scalars("Tail", &[I32, I8]);
    assert_eq!(tail.field_offsets(), [0, 4]);
    assert_eq!((tail.size(), tail.align()), (8, 4));

    // struct { bool a; uint8_t b; uint16_t c; uint32_t d; uint64_t e; float f; double g; }
    let every = scalars("Every", &[Bool, U8, U16, U32, U64, F32, F64]);
    assert_eq!(every.field_offsets(), [0, 1, 2, 4, 8, 16, 24]);
    assert_eq!((every.size(), every.align()), (32, 8));

    // struct { int8_t a; struct { int16_t x; int8_t y; } inner; int8_t b; }
    let inner = scalars("Inner", &[I16, I8]);
    assert_eq!((inner.size(), inner.align()), (4, 2));
    let outer = with_struct("Outer", &[I8], inner, &[I8]);
    assert_eq!(outer.field_offsets(), [0, 2, 6]);
    assert_eq!((outer.size(), outer.align()), (8, 2));
    assert_eq!(
        outer.leaves(),
        [
            CLeaf {
                bits: None,
                offset: 0,
                scalar: I8
            },
            CLeaf {
                bits: None,
                offset: 2,
                scalar: I16
            },
            CLeaf {
                bits: None,
                offset: 4,
                scalar: I8
            },
            CLeaf {
                bits: None,
                offset: 6,
                scalar: I8
            },
        ]
    );

    let empty = scalars("Empty", &[]);
    assert_eq!((empty.size(), empty.align()), (0, 1));
}

#[test]
fn every_host_architecture_aura_builds_for_has_a_convention() {
    assert_eq!(CAbi::of_arch("x86_64"), Some(CAbi::SysV64));
    assert_eq!(CAbi::of_arch("aarch64"), Some(CAbi::Aapcs64));
    assert_eq!(CAbi::of_arch("arm64"), Some(CAbi::Aapcs64));
    assert_eq!(CAbi::of_arch("riscv64"), None);
    assert!(CAbi::host().is_some());
}

#[test]
fn system_v_passes_small_structs_by_eightbyte_class() {
    let abi = CAbi::SysV64;
    let registers = |fields: &[FfiScalar]| match scalars("S", fields).passing(abi) {
        CPassing::Registers(pieces) => pieces,
        CPassing::Memory => panic!("{fields:?} should pass in registers"),
    };
    assert_eq!(registers(&[F64, F64]), [sse(0, 8), sse(8, 8)]);
    assert_eq!(registers(&[I32, I32]), [int(0, 8)]);
    assert_eq!(registers(&[I64, F64]), [int(0, 8), sse(8, 8)]);
    assert_eq!(registers(&[F64, I32]), [sse(0, 8), int(8, 8)]);
    assert_eq!(registers(&[F32]), [sse(0, 4)]);
    assert_eq!(registers(&[F32, F32]), [sse(0, 8)]);
    assert_eq!(registers(&[F32, F32, F32]), [sse(0, 8), sse(8, 4)]);
    assert_eq!(registers(&[F32, F32, F32, F32]), [sse(0, 8), sse(8, 8)]);
    // A float that shares an eightbyte with an integer goes in a general
    // register.
    assert_eq!(registers(&[F32, I32]), [int(0, 8)]);
    assert_eq!(registers(&[F32, I32, F32]), [int(0, 8), sse(8, 4)]);
    assert_eq!(registers(&[I8, I64]), [int(0, 8), int(8, 8)]);
    assert_eq!(registers(&[I8]), [int(0, 1)]);
    assert_eq!(registers(&[I64, I8]), [int(0, 8), int(8, 8)]);
    assert_eq!(registers(&[I32, I32, I32]), [int(0, 8), int(8, 4)]);

    assert_eq!(
        scalars("S", &[I64, I64, I64]).passing(abi),
        CPassing::Memory
    );
    assert_eq!(
        scalars("S", &[F64, F64, F64]).passing(abi),
        CPassing::Memory
    );
    assert_eq!(scalars("S", &[I8, I64, I8]).passing(abi), CPassing::Memory);

    let nested = with_struct("Outer", &[F64], scalars("Inner", &[F32, I32]), &[]);
    assert_eq!(
        nested.passing(abi),
        CPassing::Registers(vec![sse(0, 8), int(8, 8)])
    );
}

#[test]
fn aarch64_passes_float_aggregates_in_float_registers() {
    let abi = CAbi::Aapcs64;
    let registers = |fields: &[FfiScalar]| match scalars("S", fields).passing(abi) {
        CPassing::Registers(pieces) => pieces,
        CPassing::Memory => panic!("{fields:?} should pass in registers"),
    };
    assert_eq!(registers(&[F64, F64]), [float(0, F64), float(8, F64)]);
    assert_eq!(
        registers(&[F64, F64, F64, F64]),
        [float(0, F64), float(8, F64), float(16, F64), float(24, F64)]
    );
    assert_eq!(
        registers(&[F32, F32, F32]),
        [float(0, F32), float(4, F32), float(8, F32)]
    );
    assert_eq!(registers(&[F32]), [float(0, F32)]);
    // Mixed float widths are no homogeneous aggregate.
    assert_eq!(registers(&[F32, F64]), [int(0, 8), int(8, 8)]);
    assert_eq!(registers(&[F64, I32]), [int(0, 8), int(8, 8)]);
    assert_eq!(registers(&[I32, I32]), [int(0, 8)]);
    assert_eq!(registers(&[I8]), [int(0, 1)]);
    assert_eq!(registers(&[I32, I32, I32]), [int(0, 8), int(8, 4)]);

    assert_eq!(
        scalars("S", &[F64, F64, F64, F64, F64]).passing(abi),
        CPassing::Memory
    );
    assert_eq!(
        scalars("S", &[I64, I64, I64]).passing(abi),
        CPassing::Memory
    );
    assert_eq!(scalars("S", &[F64, F64, I8]).passing(abi), CPassing::Memory);

    let nested = with_struct("Outer", &[F32], scalars("Inner", &[F32, F32]), &[F32]);
    assert_eq!(
        nested.passing(abi),
        CPassing::Registers(vec![
            float(0, F32),
            float(4, F32),
            float(8, F32),
            float(12, F32)
        ])
    );
}

#[test]
fn windows_passes_a_struct_as_one_integer_or_in_memory() {
    assert_eq!(CAbi::of_target("x86_64", "windows"), Some(CAbi::Win64));
    assert_eq!(CAbi::of_target("aarch64", "windows"), None);
    assert_eq!(CAbi::of_target("x86_64", "linux"), Some(CAbi::SysV64));
    assert_eq!(CAbi::of_target("aarch64", "macos"), Some(CAbi::Aapcs64));

    // Eight bytes of floats still travel in a general register.
    let pair = scalars("Pair", &[FfiScalar::F32, FfiScalar::F32]);
    assert_eq!(
        pair.passing(CAbi::Win64),
        CPassing::Registers(vec![int(0, 8)])
    );
    assert_eq!(
        pair.argument(CAbi::Win64, 4, 4),
        Ok(CArgument::Registers(vec![int(0, 8)])),
        "an argument past the fourth takes its own stack slot"
    );
    let small = scalars("Small", &[FfiScalar::I32]);
    assert_eq!(
        small.passing(CAbi::Win64),
        CPassing::Registers(vec![int(0, 4)])
    );
    // Sizes that are not 1, 2, 4 or 8 go in memory, by address.
    for fields in [
        &[FfiScalar::I32, FfiScalar::I32, FfiScalar::I32][..],
        &[FfiScalar::F64, FfiScalar::F64][..],
        &[FfiScalar::I64, FfiScalar::I64, FfiScalar::I64][..],
    ] {
        let layout = scalars("Wide", fields);
        assert_eq!(layout.passing(CAbi::Win64), CPassing::Memory);
        assert_eq!(layout.argument(CAbi::Win64, 0, 0), Ok(CArgument::Address));
    }
}

#[test]
fn a_struct_description_round_trips_through_serde() {
    let outer = with_struct("Outer", &[I8], scalars("Inner", &[F32, Bool]), &[U64]);
    let text = serde_json::to_string(&outer).expect("a struct serializes");
    let back: CStruct = serde_json::from_str(&text).expect("and reads back");
    assert_eq!(back, outer);
}

#[test]
fn an_argument_takes_registers_only_when_all_of_it_fits() {
    let pair = scalars("Pair", &[I64, I64]);
    let mixed = scalars("Mixed", &[I64, F64]);
    let big = scalars("Big", &[I64, I64, I64]);
    let vec3 = scalars("Vec3", &[F32, F32, F32]);

    // System V: registers while they last, then the stack.
    assert_eq!(
        pair.argument(CAbi::SysV64, 4, 0),
        Ok(CArgument::Registers(vec![int(0, 8), int(8, 8)]))
    );
    assert_eq!(pair.argument(CAbi::SysV64, 5, 0), Ok(CArgument::Stack));
    assert_eq!(
        mixed.argument(CAbi::SysV64, 5, 7),
        Ok(CArgument::Registers(vec![int(0, 8), sse(8, 8)]))
    );
    assert_eq!(mixed.argument(CAbi::SysV64, 5, 8), Ok(CArgument::Stack));
    assert_eq!(big.argument(CAbi::SysV64, 0, 0), Ok(CArgument::Stack));

    // AArch64: registers, or the address of a copy for a large struct.
    assert_eq!(
        pair.argument(CAbi::Aapcs64, 6, 0),
        Ok(CArgument::Registers(vec![int(0, 8), int(8, 8)]))
    );
    assert_eq!(big.argument(CAbi::Aapcs64, 8, 8), Ok(CArgument::Address));
    assert_eq!(
        vec3.argument(CAbi::Aapcs64, 8, 5),
        Ok(CArgument::Registers(vec![
            float(0, F32),
            float(4, F32),
            float(8, F32)
        ]))
    );
    // Scalars that went to the stack for want of one kind of register do
    // not move a struct that needs only the other kind.
    assert_eq!(
        pair.argument(CAbi::Aapcs64, 0, 11),
        Ok(CArgument::Registers(vec![int(0, 8), int(8, 8)]))
    );
    assert_eq!(
        pair.argument(CAbi::SysV64, 0, 9),
        Ok(CArgument::Registers(vec![int(0, 8), int(8, 8)]))
    );
    assert_eq!(
        vec3.argument(CAbi::Aapcs64, 12, 0),
        Ok(CArgument::Registers(vec![
            float(0, F32),
            float(4, F32),
            float(8, F32)
        ]))
    );
    // A struct that does not fit the registers that are left goes to the
    // stack whole.
    assert_eq!(pair.argument(CAbi::Aapcs64, 7, 0), Ok(CArgument::Stack));
    assert_eq!(vec3.argument(CAbi::Aapcs64, 0, 6), Ok(CArgument::Stack));
}

#[test]
fn an_array_field_is_laid_out_as_its_elements_in_a_row() {
    use CFieldKind::{Scalar, Struct};
    // struct { uint8_t tag; float params[4]; int16_t tail; }
    let material = of_kinds(
        "Material",
        false,
        vec![Scalar(U8), array(Scalar(F32), 4), Scalar(I16)],
    );
    assert_eq!(material.field_offsets(), [0, 4, 20]);
    assert_eq!((material.size(), material.align()), (24, 4));
    assert!(!material.is_plain());
    assert_eq!(
        material
            .leaves()
            .iter()
            .map(|leaf| leaf.offset)
            .collect::<Vec<_>>(),
        [0, 4, 8, 12, 16, 20]
    );

    // struct { char name[32]; int parent; }
    let bone = of_kinds("BoneInfo", false, vec![array(Scalar(I8), 32), Scalar(I32)]);
    assert_eq!(bone.field_offsets(), [0, 32]);
    assert_eq!((bone.size(), bone.align()), (36, 4));

    // struct { struct { int16_t x; int8_t y; } pairs[3]; int8_t end; }
    let inner = scalars("Inner", &[I16, I8]);
    let rows = of_kinds("Rows", false, vec![array(Struct(inner), 3), Scalar(I8)]);
    assert_eq!(rows.field_offsets(), [0, 12]);
    assert_eq!((rows.size(), rows.align()), (14, 2));
    assert_eq!(
        rows.leaves()
            .iter()
            .map(|leaf| (leaf.offset, leaf.scalar))
            .collect::<Vec<_>>(),
        [
            (0, I16),
            (2, I8),
            (4, I16),
            (6, I8),
            (8, I16),
            (10, I8),
            (12, I8)
        ]
    );

    // An array of arrays: float m[2][2].
    let grid = of_kinds("Grid", false, vec![array(array(Scalar(F32), 2), 2)]);
    assert_eq!((grid.size(), grid.align()), (16, 4));
    assert!(scalars("Plain", &[I32, F32]).is_plain());
    assert!(with_struct("Nested", &[I8], scalars("In", &[I8]), &[]).is_plain());
}

#[test]
fn a_union_puts_every_member_at_offset_zero() {
    use CFieldKind::{Scalar, Struct};
    // union { uint32_t type; struct { uint32_t type; int32_t x; int32_t y; } motion;
    //         uint8_t padding[24]; }
    let motion = scalars("Motion", &[U32, I32, I32]);
    let event = of_kinds(
        "Event",
        true,
        vec![Scalar(U32), Struct(motion), array(Scalar(U8), 24)],
    );
    assert_eq!(event.field_offsets(), [0, 0, 0]);
    assert_eq!((event.size(), event.align()), (24, 4));
    assert!(!event.is_plain());

    // union { uint8_t small; double wide; } rounds its size to the alignment.
    let mixed = of_kinds("Mixed", true, vec![array(Scalar(U8), 9), Scalar(F64)]);
    assert_eq!((mixed.size(), mixed.align()), (16, 8));

    // A struct that holds a union is not plain, and lays the union out
    // by its size and alignment.
    let holder = of_kinds("Holder", false, vec![Scalar(I8), Struct(mixed), Scalar(I8)]);
    assert_eq!(holder.field_offsets(), [0, 8, 24]);
    assert_eq!((holder.size(), holder.align()), (32, 8));
    assert!(!holder.is_plain());
}

#[test]
fn arrays_and_unions_are_classified_by_every_scalar_that_overlaps() {
    use CFieldKind::{Scalar, Struct};
    // struct { float v[2]; }: one SSE eightbyte, or two floats on AArch64.
    let pair = of_kinds("Pair", false, vec![array(Scalar(F32), 2)]);
    assert_eq!(
        pair.passing(CAbi::SysV64),
        CPassing::Registers(vec![sse(0, 8)])
    );
    assert_eq!(
        pair.passing(CAbi::Aapcs64),
        CPassing::Registers(vec![float(0, F32), float(4, F32)])
    );
    assert_eq!(
        pair.passing(CAbi::Win64),
        CPassing::Registers(vec![int(0, 8)])
    );

    // struct { double v[4]; }: memory on x86-64, four doubles on AArch64.
    let quad = of_kinds("Quad", false, vec![array(Scalar(F64), 4)]);
    assert_eq!(quad.passing(CAbi::SysV64), CPassing::Memory);
    assert_eq!(
        quad.passing(CAbi::Aapcs64),
        CPassing::Registers(vec![
            float(0, F64),
            float(8, F64),
            float(16, F64),
            float(24, F64)
        ])
    );
    // Five floats are no longer homogeneous registers.
    let five = of_kinds("Five", false, vec![array(Scalar(F32), 5)]);
    assert_eq!(five.passing(CAbi::Aapcs64), CPassing::Memory);
    assert_eq!(five.passing(CAbi::SysV64), CPassing::Memory);

    // union { float f; uint32_t bits; }: an integer where the two overlap.
    let bits = of_kinds("Bits", true, vec![Scalar(F32), Scalar(U32)]);
    assert_eq!(
        bits.passing(CAbi::SysV64),
        CPassing::Registers(vec![int(0, 4)])
    );
    assert_eq!(
        bits.passing(CAbi::Aapcs64),
        CPassing::Registers(vec![int(0, 4)])
    );

    // union { float f; float pair[2]; }: two floats, counted by size.
    let floats = of_kinds("Floats", true, vec![Scalar(F32), array(Scalar(F32), 2)]);
    assert_eq!(
        floats.passing(CAbi::SysV64),
        CPassing::Registers(vec![sse(0, 8)])
    );
    assert_eq!(
        floats.passing(CAbi::Aapcs64),
        CPassing::Registers(vec![float(0, F32), float(4, F32)])
    );

    // union { float f; double d; } is not homogeneous.
    let two = of_kinds("Two", true, vec![Scalar(F32), Scalar(F64)]);
    assert_eq!(
        two.passing(CAbi::SysV64),
        CPassing::Registers(vec![sse(0, 8)])
    );
    assert_eq!(
        two.passing(CAbi::Aapcs64),
        CPassing::Registers(vec![int(0, 8)])
    );

    // struct { int32_t tag; union { float f; int32_t i; } value; double d; }
    let value = of_kinds("Value", true, vec![Scalar(F32), Scalar(I32)]);
    let tagged = of_kinds(
        "Tagged",
        false,
        vec![Scalar(I32), Struct(value), Scalar(F64)],
    );
    assert_eq!(
        tagged.passing(CAbi::SysV64),
        CPassing::Registers(vec![int(0, 8), sse(8, 8)])
    );
    assert_eq!(
        tagged.passing(CAbi::Aapcs64),
        CPassing::Registers(vec![int(0, 8), int(8, 8)])
    );

    // A union larger than 16 bytes goes in memory everywhere.
    let big = of_kinds("Big", true, vec![Scalar(U32), array(Scalar(U8), 56)]);
    assert_eq!(big.size(), 56);
    for abi in [CAbi::SysV64, CAbi::Aapcs64, CAbi::Win64] {
        assert_eq!(big.passing(abi), CPassing::Memory);
    }
}

#[test]
fn bit_field_storage_and_calling_classes_follow_the_record_abi() {
    let mut s = scalars("Bits", &[U8, U16, U32, U8]);
    s.layout_controls
        .bit_widths
        .extend([("f1".into(), 3), ("f2".into(), 6)]);
    assert_eq!(
        (s.size(), s.align(), s.field_offsets()),
        (4, 4, vec![0, 1, 1, 3])
    );
    assert_eq!(s.leaves()[2].bits, Some((3, 6)));
    s.layout_controls.microsoft_bit_fields = true;
    assert_eq!(
        (s.size(), s.align(), s.field_offsets()),
        (12, 4, vec![0, 2, 4, 8])
    );
    s.layout_controls.packed = true;
    assert_eq!(
        (s.size(), s.align(), s.field_offsets()),
        (8, 1, vec![0, 1, 3, 7])
    );
    s.layout_controls.alignment = Some(16);
    assert_eq!((s.size(), s.align()), (16, 16));
    let mut small = scalars("Small", &[U8, U32]);
    small.layout_controls.packed = true;
    assert_eq!(small.passing(CAbi::SysV64), CPassing::Memory);
    assert_eq!(
        small.passing(CAbi::Aapcs64),
        CPassing::Registers(vec![int(0, 5)])
    );
    assert_eq!(small.passing(CAbi::Win64), CPassing::Memory);
    assert_eq!(
        bit_chunks(7, 3).collect::<Vec<_>>(),
        [(0, 7, 128), (1, 0, 3)]
    );
    assert_eq!(bit_chunks(1, 64).count(), 9);
}

#[test]
fn packed_bit_fields_classify_every_integer_register_they_overlap() {
    let mut record = scalars("Crossing", &[U64, U64]);
    record.layout_controls.bit_widths = [("f0".into(), 60), ("f1".into(), 12)].into();

    assert_eq!(record.field_offsets(), [0, 8]);
    assert_eq!((record.size(), record.align()), (16, 8));
    record.layout_controls.packed = true;
    assert_eq!(record.field_offsets(), [0, 7]);
    assert_eq!((record.size(), record.align()), (9, 1));
    assert_eq!(
        record.passing(CAbi::SysV64),
        CPassing::Registers(vec![int(0, 8), int(8, 1)])
    );
}

#[test]
fn over_aligned_float_record_padding_is_not_an_aarch64_float_member() {
    let mut record = scalars("WideFloat", &[F32]);
    record.layout_controls.alignment = Some(16);
    assert_eq!(
        record.passing(CAbi::Aapcs64),
        CPassing::Registers(vec![int(0, 8), int(8, 8)])
    );
    assert_eq!(
        record.passing(CAbi::SysV64),
        CPassing::Registers(vec![sse(0, 8)])
    );
}
