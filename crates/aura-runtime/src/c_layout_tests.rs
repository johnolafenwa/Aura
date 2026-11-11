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
    }
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
                offset: 0,
                scalar: I8
            },
            CLeaf {
                offset: 2,
                scalar: I16
            },
            CLeaf {
                offset: 4,
                scalar: I8
            },
            CLeaf {
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
    let error = pair.argument(CAbi::Aapcs64, 7, 0).unwrap_err();
    assert!(error.contains("struct `Pair` does not fit"), "{error}");
    assert!(error.contains("`mut Pair`"), "{error}");
    assert!(vec3.argument(CAbi::Aapcs64, 0, 6).is_err());
}
