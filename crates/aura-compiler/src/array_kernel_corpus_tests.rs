//! Frozen pre-vectorization corpus shared by the MIR evaluator and direct ABI tests.
use crate::diag::Diagnostic;
use crate::integer::IntegerValue;
use crate::runtime_value::{ArrayStorage, ArrayValue, Value};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug)]
pub(crate) enum Kernel {
    Binary {
        operation: i64,
        mode: i64,
        scalar_left: bool,
    },
    Reduce(i64),
}

fn array(dtype: usize, len: usize, right: bool, special: bool) -> Value {
    let mut seed = if right {
        0x92d6_8ca2_13f4_751b_u64
    } else {
        0x5a17_00c3_fed9_8241_u64
    };
    let mut next = || {
        seed = seed
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        if right {
            ((seed >> 32) % 31 + 1) as i64
        } else {
            ((seed >> 32) % 1024) as i64 - 512
        }
    };
    let storage = match dtype {
        0 => ArrayStorage::Int32(
            (0..len)
                .map(|i| {
                    let value = next() as i32;
                    if special && i % 8 == 7 {
                        i32::MAX
                    } else if special && i == 0 {
                        i32::MIN
                    } else {
                        value
                    }
                })
                .collect(),
        ),
        1 => ArrayStorage::Int64(
            (0..len)
                .map(|i| {
                    let value = next();
                    if special && i % 8 == 7 {
                        i64::MAX
                    } else if special && i == 0 {
                        i64::MIN
                    } else {
                        value
                    }
                })
                .collect(),
        ),
        2 => ArrayStorage::Float32(
            (0..len)
                .map(|i| {
                    let value = next() as f32 / 16.0 + 0.03125;
                    if special {
                        match i % 9 {
                            0 => f32::from_bits(0x7fc0_1234),
                            1 => f32::INFINITY,
                            2 => f32::NEG_INFINITY,
                            7 => -0.0,
                            _ => value,
                        }
                    } else {
                        value
                    }
                })
                .collect(),
        ),
        _ => ArrayStorage::Float64(
            (0..len)
                .map(|i| {
                    let value = next() as f64 / 16.0 + 0.03125;
                    if special {
                        match i % 9 {
                            0 => f64::from_bits(0x7ff8_0000_0000_1234),
                            1 => f64::INFINITY,
                            2 => f64::NEG_INFINITY,
                            7 => -0.0,
                            _ => value,
                        }
                    } else {
                        value
                    }
                })
                .collect(),
        ),
    };
    Value::Array(ArrayValue::new(vec![len].into_boxed_slice(), storage).unwrap())
}

fn scalar(dtype: usize) -> Value {
    match dtype {
        0 => Value::Int(IntegerValue::from_i32(3)),
        1 => Value::Int(IntegerValue::from_i64(3)),
        _ => Value::Float(3.25),
    }
}

fn fingerprint(result: Result<Value, Diagnostic>) -> String {
    let mut digest = Sha256::new();
    match result {
        Err(error) => return format!("{}: {}", error.code, error.message),
        Ok(Value::Int(value)) => return format!("int: {value:?}"),
        Ok(Value::Float(value)) => return format!("float64: {:016x}", value.to_bits()),
        Ok(Value::Array(array)) => {
            for dim in &array.shape {
                digest.update((*dim as u64).to_le_bytes());
            }
            let bytes: Vec<u8> = match array.storage {
                ArrayStorage::Int32(values) => {
                    values.iter().flat_map(|v| v.to_le_bytes()).collect()
                }
                ArrayStorage::Int64(values) => {
                    values.iter().flat_map(|v| v.to_le_bytes()).collect()
                }
                ArrayStorage::Float32(values) => values
                    .iter()
                    .flat_map(|v| v.to_bits().to_le_bytes())
                    .collect(),
                ArrayStorage::Float64(values) => values
                    .iter()
                    .flat_map(|v| v.to_bits().to_le_bytes())
                    .collect(),
            };
            digest.update(bytes);
        }
        other => panic!("unexpected Array kernel result: {other:?}"),
    }
    format!("sha256: {:x}", digest.finalize())
}

pub(crate) fn verify(
    mut evaluate: impl FnMut(Kernel, Value, Option<Value>) -> Result<Value, Diagnostic>,
) {
    let expected: BTreeMap<String, String> =
        serde_json::from_str(include_str!("array_kernel_expected.json")).unwrap();
    let mut actual = BTreeMap::new();
    for dtype in 0..4 {
        for len in [0, 1, 7, 8, 9, 1_000_001] {
            for special in [false, true] {
                let left = array(dtype, len, false, special);
                let right = array(dtype, len, true, false);
                let prefix = format!("dtype={dtype}/len={len}/special={special}");
                for operation in 0..if dtype < 2 { 3 } else { 4 } {
                    for mode in 0..if dtype < 2 { 3 } else { 1 } {
                        for lane in 0..if mode == 0 { 3 } else { 2 } {
                            let (a, b) = match lane {
                                0 => (left.clone(), right.clone()),
                                1 => (left.clone(), scalar(dtype)),
                                _ => (scalar(dtype), left.clone()),
                            };
                            let key = format!("{prefix}/op={operation}/mode={mode}/lane={lane}");
                            actual.insert(
                                key,
                                fingerprint(evaluate(
                                    Kernel::Binary {
                                        operation,
                                        mode,
                                        scalar_left: lane == 2,
                                    },
                                    a,
                                    Some(b),
                                )),
                            );
                        }
                    }
                }
                for reduction in 0..4 {
                    actual.insert(
                        format!("{prefix}/reduce={reduction}"),
                        fingerprint(evaluate(Kernel::Reduce(reduction), left.clone(), None)),
                    );
                }
            }
        }
    }
    // Two failing positions straddle vector boundaries: retain the first index,
    // including when it occurs in the final scalar tail.
    for dtype in 0..4 {
        for first in [0, 7, 8] {
            for operation in 0..if dtype < 2 { 3 } else { 1 } {
                let mut a32 = vec![1_i32; 9];
                let mut a64 = vec![1_i64; 9];
                let mut b32 = vec![1_i32; 9];
                let mut b64 = vec![1_i64; 9];
                let mut bf32 = vec![1.0_f32; 9];
                let mut bf64 = vec![1.0_f64; 9];
                for i in [first, 8] {
                    a32[i] = if operation == 1 { i32::MIN } else { i32::MAX };
                    a64[i] = if operation == 1 { i64::MIN } else { i64::MAX };
                    b32[i] = 2;
                    b64[i] = 2;
                    bf32[i] = -0.0;
                    bf64[i] = -0.0;
                }
                let (left, right) = match dtype {
                    0 => (
                        ArrayStorage::Int32(a32.into()),
                        ArrayStorage::Int32(b32.into()),
                    ),
                    1 => (
                        ArrayStorage::Int64(a64.into()),
                        ArrayStorage::Int64(b64.into()),
                    ),
                    2 => (
                        ArrayStorage::Float32(vec![1.0; 9].into()),
                        ArrayStorage::Float32(bf32.into()),
                    ),
                    _ => (
                        ArrayStorage::Float64(vec![1.0; 9].into()),
                        ArrayStorage::Float64(bf64.into()),
                    ),
                };
                let left = Value::Array(ArrayValue::new(vec![9].into(), left).unwrap());
                let right = Value::Array(ArrayValue::new(vec![9].into(), right).unwrap());
                let operation = if dtype < 2 { operation } else { 3 };
                actual.insert(
                    format!("first-trap/dtype={dtype}/op={operation}/first={first}"),
                    fingerprint(evaluate(
                        Kernel::Binary {
                            operation,
                            mode: 0,
                            scalar_left: false,
                        },
                        left,
                        Some(right),
                    )),
                );
            }
        }
    }
    assert_eq!(
        actual.len(),
        expected.len(),
        "the frozen corpus must cover every case"
    );
    for (key, value) in actual {
        assert_eq!(
            Some(&value),
            expected.get(&key),
            "pre-change bits/diagnostic changed for {key}"
        );
    }
}
