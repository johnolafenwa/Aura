//! Plain and volatile access through the signed or floating representation.
use super::*;

pub(super) fn access(name: &str, args: &[Value]) -> Option<Result<Value>> {
    if name == "mem::__pointer_offset" {
        return Some(match args {
            [Value::Int(base), Value::Int(count), Value::Int(stride)] => {
                match (base.as_i128(), count.as_i128(), stride.as_i128()) {
                    (Some(base), Some(count), Some(stride)) => {
                        Ok(Value::Int(IntegerValue::from_u64(
                            (base as u64).wrapping_add((count as u64).wrapping_mul(stride as u64)),
                        )))
                    }
                    _ => Err(mem_argument_error(name)),
                }
            }
            _ => Err(mem_argument_error(name)),
        });
    }
    let (operation, kind) = name.strip_prefix("mem::")?.rsplit_once('_')?;
    if !matches!(
        kind,
        "bool" | "int8" | "int16" | "int32" | "int64" | "float32" | "float64"
    ) {
        return None;
    }
    if !matches!(
        operation,
        "read" | "write" | "volatile_read" | "volatile_write"
    ) {
        return None;
    }
    let volatile = operation.starts_with("volatile_");
    let write = operation.ends_with("write");
    Some((|| {
        host_expect_arity(name, args, if write { 2 } else { 1 })?;
        let Value::Int(address) = args[0] else {
            return Err(mem_argument_error(name));
        };
        let address = address
            .as_i128()
            .and_then(|n| usize::try_from(n).ok())
            .ok_or_else(|| mem_argument_error(name))?;
        // SAFETY: the FFI caller supplies a live address for the requested
        // width. Volatile operations also require natural alignment.
        macro_rules! read {
            ($ty:ty) => {{
                let p = address as *const $ty;
                unsafe {
                    if volatile {
                        p.read_volatile()
                    } else {
                        p.read_unaligned()
                    }
                }
            }};
        }
        macro_rules! store {
            ($value:expr, $ty:ty) => {{
                let p = address as *mut $ty;
                unsafe {
                    if volatile {
                        p.write_volatile($value)
                    } else {
                        p.write_unaligned($value)
                    }
                }
            }};
        }
        if write {
            match (kind, &args[1]) {
                ("bool", Value::Bool(v)) => store!(u8::from(*v), u8),
                ("float32", Value::Float(v)) => store!(*v as f32, f32),
                ("float64", Value::Float(v)) => store!(*v, f64),
                (_, Value::Int(v)) => {
                    let v = v.as_i128().ok_or_else(|| mem_argument_error(name))?;
                    match kind {
                        "int8" => store!(v as i8, i8),
                        "int16" => store!(v as i16, i16),
                        "int32" => store!(v as i32, i32),
                        "int64" => store!(v as i64, i64),
                        _ => return Err(mem_argument_error(name)),
                    }
                }
                _ => return Err(mem_argument_error(name)),
            }
            return Ok(Value::Unit);
        }
        let number = match kind {
            "bool" => {
                return match read!(u8) {
                    0 => Ok(Value::Bool(false)),
                    1 => Ok(Value::Bool(true)),
                    _ => Err(Diagnostic::coded(
                        "AU4001",
                        "raw pointer read an invalid bool representation",
                    )),
                }
            }
            "float32" => return Ok(Value::Float(f64::from(read!(f32)))),
            "float64" => return Ok(Value::Float(read!(f64))),
            "int8" => i128::from(read!(i8)),
            "int16" => i128::from(read!(i16)),
            "int32" => i128::from(read!(i32)),
            _ => i128::from(read!(i64)),
        };
        let kind = IntegerKind::from_runtime_type_name(kind).expect("integer representation");
        Ok(Value::Int(
            IntegerValue::from_typed_signed(number, kind)
                .expect("value loaded at its declared width"),
        ))
    })())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn typed_memory_rejects_wrong_addresses_and_values() {
        let mut cell = 3_u64;
        let address = Value::Int(IntegerValue::from_u64((&mut cell as *mut u64) as u64));
        for (name, args) in [
            ("mem::write_int16", vec![address.clone()]),
            (
                "mem::write_float64",
                vec![address.clone(), Value::Bool(true)],
            ),
            ("mem::read_bool", vec![address.clone()]),
            ("mem::read_int16", vec![Value::Bool(false)]),
            (
                "mem::read_int64",
                vec![Value::Int(IntegerValue::from_i64(-1))],
            ),
            ("mem::__pointer_offset", vec![address.clone()]),
        ] {
            assert!(access(name, &args).unwrap().is_err(), "{name}");
        }
        let too_wide = Value::Int(IntegerValue::from_literal(u128::MAX));
        for (name, args) in [
            ("mem::write_int16", vec![address.clone(), too_wide.clone()]),
            (
                "mem::write_float32",
                vec![address.clone(), Value::Int(IntegerValue::from_i64(1))],
            ),
            (
                "mem::__pointer_offset",
                vec![
                    address.clone(),
                    too_wide,
                    Value::Int(IntegerValue::from_u64(1)),
                ],
            ),
        ] {
            assert!(access(name, &args).unwrap().is_err(), "{name}");
        }
        assert_eq!(cell, 3);
        assert!(access("mem::noise_int32", std::slice::from_ref(&address)).is_none());
        let offset = access(
            "mem::__pointer_offset",
            &[
                address,
                Value::Int(IntegerValue::from_i64(-2)),
                Value::Int(IntegerValue::from_u64(2)),
            ],
        )
        .unwrap()
        .unwrap();
        assert!(
            matches!(offset, Value::Int(value) if value.as_i128() == Some((&mut cell as *mut u64 as usize) as i128 - 4))
        );
    }
}
