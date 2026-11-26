//! Raw loads and stores of 128-bit integer representations.
use crate::integer::{IntegerKind, IntegerRepresentation, IntegerValue};
use crate::runtime_value::Value;

/// # Safety
/// The address covers 16 readable bytes and is naturally aligned for a volatile read.
pub unsafe fn read(address: *const u128, signed: bool, volatile: bool) -> Value {
    let bits = unsafe {
        if volatile {
            address.read_volatile()
        } else {
            address.read_unaligned()
        }
    };
    let value = if signed {
        IntegerValue::from_typed_signed(bits as i128, IntegerKind::Int128)
    } else {
        IntegerValue::from_typed_unsigned(bits, IntegerKind::Uint128)
    };
    Value::Int(value.expect("every bit pattern represents a wide integer"))
}

/// # Safety
/// The address covers 16 writable bytes and is naturally aligned for a volatile write.
pub unsafe fn write(address: *mut u128, value: &Value, volatile: bool) -> bool {
    let Value::Int(held) = value else {
        return false;
    };
    let bits = match held.representation() {
        IntegerRepresentation::Unsigned(bits) => bits,
        IntegerRepresentation::Signed(value) => value as u128,
    };
    unsafe {
        if volatile {
            address.write_volatile(bits);
        } else {
            address.write_unaligned(bits);
        }
    }
    true
}
