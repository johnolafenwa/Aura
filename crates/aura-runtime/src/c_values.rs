//! Aura values as the bytes of C structs, unions, and arrays.
//!
//! An `extern "C"` class crosses as the bytes [`CStruct`] lays out. A
//! class of scalars is converted by the code that makes the call. Every
//! other shape, a class with an array field or a union, a union itself,
//! and the elements of a struct array, goes through [`pack`] and
//! [`unpack`], which the interpreter and the native runtime both call, so
//! the two agree.
//!
//! - A class is an instance with one field per C field.
//! - An array field of `N` elements is a list of exactly `N` values.
//! - A union is an instance with the one field [`UNION_BYTES_FIELD`], a
//!   `list[uint8]` of the union's size.

use std::collections::BTreeMap;
use std::fmt;

use aura_core::types::Type;

use crate::c_layout::{CFieldKind, CStruct, UNION_BYTES_FIELD};
use crate::ffi::FfiScalar;
use crate::integer::{IntegerKind, IntegerRepresentation, IntegerValue};
use crate::list_elements::ListElements;
use crate::runtime_value::{InstanceValue, RuntimeName, Value, VecValue};

/// Why a value did not cross.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CValueError {
    /// The value is not the shape the C type needs.
    Shape {
        expected: String,
        found: String,
    },
    /// An array field holds a list of the wrong length.
    Length {
        field: String,
        expected: usize,
        found: usize,
    },
    /// The `bytes` list of a union is not as long as the C union.
    UnionLength {
        name: String,
        expected: usize,
        found: usize,
    },
    /// C left a byte other than 0 or 1 in a `bool`.
    BadBool(u8),
    BitWidth {
        field: String,
        width: u8,
    },
}

impl CValueError {
    /// The diagnostic code the failure reports under.
    pub fn code(&self) -> &'static str {
        match self {
            Self::Shape { .. } | Self::Length { .. } | Self::UnionLength { .. } => "AU4005",
            Self::BadBool(_) => "AU4001",
            Self::BitWidth { .. } => "AU4033",
        }
    }
}

impl fmt::Display for CValueError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Shape { expected, found } => write!(
                formatter,
                "FFI value for C type `{expected}` has incompatible runtime shape `{found}`"
            ),
            Self::Length {
                field,
                expected,
                found,
            } => write!(
                formatter,
                "C array field `{field}` holds {expected} element(s), but the list has {found}"
            ),
            Self::UnionLength {
                name,
                expected,
                found,
            } => write!(
                formatter,
                "C union `{name}` is {expected} byte(s), but its `{UNION_BYTES_FIELD}` list has {found}"
            ),
            Self::BitWidth { field, width } => write!(formatter, "value does not fit the {width}-bit C field `{field}`"),
            Self::BadBool(byte) => write!(
                formatter,
                "C left the byte {byte} in a `bool`, which holds only 0 or 1"
            ),
        }
    }
}

/// The Aura name of the type of a scalar.
fn scalar_name(scalar: FfiScalar) -> &'static str {
    match scalar {
        FfiScalar::Bool => "bool",
        FfiScalar::I8 => "int8",
        FfiScalar::I16 => "int16",
        FfiScalar::I32 => "int32",
        FfiScalar::I64 => "int64",
        FfiScalar::U8 => "uint8",
        FfiScalar::U16 => "uint16",
        FfiScalar::U32 => "uint32",
        FfiScalar::U64 => "uint64",
        FfiScalar::F32 => "float32",
        FfiScalar::F64 => "float64",
    }
}

/// The Aura type of a value of kind `kind`.
pub fn kind_type(kind: &CFieldKind) -> Type {
    match kind {
        CFieldKind::Scalar(scalar) => Type::named(scalar_name(*scalar)),
        CFieldKind::Struct(layout) => Type::named(layout.name.clone()),
        CFieldKind::Array(element, _) => Type::Named("list".to_string(), vec![kind_type(element)]),
    }
}

fn shape(expected: impl Into<String>, found: &Value) -> CValueError {
    CValueError::Shape {
        expected: expected.into(),
        found: found.render(),
    }
}

/// The bits of the scalar `value` in the low bytes of a word.
fn scalar_word(scalar: FfiScalar, value: &Value) -> Result<u64, CValueError> {
    let mismatch = || shape(scalar_name(scalar), value);
    let signed = |min: i128, max: i128| match value {
        Value::Int(integer) => integer
            .as_i128()
            .filter(|held| (min..=max).contains(held))
            .map(|held| held as u64)
            .ok_or_else(mismatch),
        _ => Err(mismatch()),
    };
    let unsigned = |max: u128| match value {
        Value::Int(integer) => match integer.representation() {
            IntegerRepresentation::Signed(held) => u128::try_from(held).ok(),
            IntegerRepresentation::Unsigned(held) => Some(held),
        }
        .filter(|held| *held <= max)
        .map(|held| held as u64)
        .ok_or_else(mismatch),
        _ => Err(mismatch()),
    };
    match scalar {
        FfiScalar::Bool => match value {
            Value::Bool(flag) => Ok(u64::from(*flag)),
            _ => Err(mismatch()),
        },
        FfiScalar::I8 => signed(i8::MIN.into(), i8::MAX.into()).map(|word| word & 0xff),
        FfiScalar::I16 => signed(i16::MIN.into(), i16::MAX.into()).map(|word| word & 0xffff),
        FfiScalar::I32 => signed(i32::MIN.into(), i32::MAX.into()).map(|word| word & 0xffff_ffff),
        FfiScalar::I64 => signed(i64::MIN.into(), i64::MAX.into()),
        FfiScalar::U8 => unsigned(u8::MAX.into()),
        FfiScalar::U16 => unsigned(u16::MAX.into()),
        FfiScalar::U32 => unsigned(u32::MAX.into()),
        FfiScalar::U64 => unsigned(u64::MAX.into()),
        FfiScalar::F32 => match value {
            Value::Float(float) => Ok(u64::from((*float as f32).to_bits())),
            _ => Err(mismatch()),
        },
        FfiScalar::F64 => match value {
            Value::Float(float) => Ok(float.to_bits()),
            _ => Err(mismatch()),
        },
    }
}

/// The Aura value of the scalar in the low bytes of `word`.
fn scalar_value(scalar: FfiScalar, word: u64) -> Result<Value, CValueError> {
    let signed = |value: i64, kind| {
        Value::Int(
            IntegerValue::from_typed_signed(value.into(), kind)
                .expect("the value was read at the width of its kind"),
        )
    };
    let unsigned = |value: u64, kind| {
        Value::Int(
            IntegerValue::from_typed_unsigned(value.into(), kind)
                .expect("the value was read at the width of its kind"),
        )
    };
    Ok(match scalar {
        FfiScalar::Bool => match word as u8 {
            0 => Value::Bool(false),
            1 => Value::Bool(true),
            byte => return Err(CValueError::BadBool(byte)),
        },
        FfiScalar::I8 => signed((word as i8).into(), IntegerKind::Int8),
        FfiScalar::I16 => signed((word as i16).into(), IntegerKind::Int16),
        FfiScalar::I32 => signed((word as i32).into(), IntegerKind::Int32),
        FfiScalar::I64 => signed(word as i64, IntegerKind::Int64),
        FfiScalar::U8 => unsigned((word as u8).into(), IntegerKind::Uint8),
        FfiScalar::U16 => unsigned((word as u16).into(), IntegerKind::Uint16),
        FfiScalar::U32 => unsigned((word as u32).into(), IntegerKind::Uint32),
        FfiScalar::U64 => unsigned(word, IntegerKind::Uint64),
        FfiScalar::F32 => Value::Float(f64::from(f32::from_bits(word as u32))),
        FfiScalar::F64 => Value::Float(f64::from_bits(word)),
    })
}

/// Writes `value`, of kind `kind`, at the start of `bytes`. `field` names
/// the field for a message.
pub(crate) fn pack_kind(
    value: &Value,
    kind: &CFieldKind,
    field: &str,
    bytes: &mut [u8],
) -> Result<(), CValueError> {
    match kind {
        CFieldKind::Scalar(scalar) => {
            let word = scalar_word(*scalar, value)?;
            let size = scalar.size();
            bytes[..size].copy_from_slice(&word.to_le_bytes()[..size]);
            Ok(())
        }
        CFieldKind::Struct(layout) => pack(value, layout, &mut bytes[..layout.size()]),
        CFieldKind::Array(element, count) => {
            let Value::Vec(list) = value else {
                return Err(shape(kind_type(kind).to_string(), value));
            };
            if list.elements.len() != *count {
                return Err(CValueError::Length {
                    field: field.to_string(),
                    expected: *count,
                    found: list.elements.len(),
                });
            }
            let size = element.size();
            // A packed scalar list already has the C element widths. Read
            // through its byte representation only while the list is borrowed.
            if cfg!(target_endian = "little") {
                if let CFieldKind::Scalar(scalar) = &**element {
                    if let Some(source) = list.elements.scalars(scalar.list_form()) {
                        let length = size * count;
                        if length != 0 {
                            // SAFETY: the form and length were checked above.
                            let source = unsafe { std::slice::from_raw_parts(source, length) };
                            bytes[..length].copy_from_slice(source);
                        }
                        return Ok(());
                    }
                }
            }
            for (index, held) in list.elements.iter().enumerate() {
                pack_kind(&held, element, field, &mut bytes[index * size..])?;
            }
            Ok(())
        }
    }
}

/// The value of kind `kind` at the start of `bytes`.
pub(crate) fn unpack_kind(kind: &CFieldKind, bytes: &[u8]) -> Result<Value, CValueError> {
    match kind {
        CFieldKind::Scalar(scalar) => {
            let size = scalar.size();
            let mut word = [0_u8; 8];
            word[..size].copy_from_slice(&bytes[..size]);
            scalar_value(*scalar, u64::from_le_bytes(word))
        }
        CFieldKind::Struct(layout) => unpack(layout, &bytes[..layout.size()]),
        CFieldKind::Array(element, count) => {
            let size = element.size();
            let elements = if matches!(&**element, CFieldKind::Scalar(FfiScalar::U8)) {
                ListElements::from_uint8(bytes[..*count].to_vec())
            } else {
                let mut elements = (0..*count)
                    .map(|index| unpack_kind(element, &bytes[index * size..]))
                    .collect::<Result<Vec<_>, _>>()
                    .map(ListElements::from)?;
                if matches!(&**element, CFieldKind::Scalar(FfiScalar::F32)) {
                    elements.narrow_to_float32();
                }
                elements
            };
            Ok(Value::Vec(VecValue::new(kind_type(element), elements)))
        }
    }
}

/// Writes the instance `value` into `bytes`, which holds one `layout`.
/// Padding keeps the bytes it had.
pub fn pack(value: &Value, layout: &CStruct, bytes: &mut [u8]) -> Result<(), CValueError> {
    let mismatch = || shape(layout.name.clone(), value);
    let Value::Instance(instance) = value else {
        return Err(mismatch());
    };
    if layout.union {
        let held = instance
            .fields
            .get(UNION_BYTES_FIELD)
            .ok_or_else(mismatch)?;
        let size = layout.size();
        if let Value::Vec(list) = &*held {
            if let Some(source) = list.elements.as_uint8() {
                if source.len() != size {
                    return Err(CValueError::UnionLength {
                        name: layout.name.clone(),
                        expected: size,
                        found: source.len(),
                    });
                }
                bytes[..size].copy_from_slice(source);
                return Ok(());
            }
        }
        let kind = CFieldKind::Array(Box::new(CFieldKind::Scalar(FfiScalar::U8)), size);
        return pack_kind(&held, &kind, UNION_BYTES_FIELD, bytes).map_err(|error| match error {
            CValueError::Length {
                expected, found, ..
            } => CValueError::UnionLength {
                name: layout.name.clone(),
                expected,
                found,
            },
            other => other,
        });
    }
    for (field, position) in layout.positions() {
        let offset = position.byte;
        let held = instance.fields.get(&field.name).ok_or_else(mismatch)?;
        if let (Some((start, width)), CFieldKind::Scalar(scalar)) = (position.bits, &field.kind) {
            let value = scalar_word(*scalar, &held)?;
            if width < 64 && value >> width != 0 {
                return Err(CValueError::BitWidth {
                    field: field.name.clone(),
                    width,
                });
            }
            for (byte, (src, dst, mask)) in bytes[offset..]
                .iter_mut()
                .zip(crate::c_layout::bit_chunks(start, width))
            {
                *byte = (*byte & !mask) | (((value >> src) as u8) << dst & mask);
            }
            continue;
        }
        pack_kind(&held, &field.kind, &field.name, &mut bytes[offset..])?;
    }
    Ok(())
}

/// The instance the bytes of one `layout` describe.
pub fn unpack(layout: &CStruct, bytes: &[u8]) -> Result<Value, CValueError> {
    let mut fields = BTreeMap::new();
    if layout.union {
        let elements = ListElements::from_uint8(bytes[..layout.size()].to_vec());
        fields.insert(
            UNION_BYTES_FIELD.to_string(),
            Value::Vec(VecValue::new(Type::named("uint8"), elements)),
        );
    } else {
        for (field, position) in layout.positions() {
            let bytes = &bytes[position.byte..];
            let value = if let (Some((start, width)), CFieldKind::Scalar(scalar)) =
                (position.bits, &field.kind)
            {
                let value = bytes
                    .iter()
                    .zip(crate::c_layout::bit_chunks(start, width))
                    .fold(0, |word, (byte, (dst, src, mask))| {
                        word | (u64::from((byte & mask) >> src) << dst)
                    });
                scalar_value(*scalar, value)?
            } else {
                unpack_kind(&field.kind, bytes)?
            };
            fields.insert(field.name.clone(), value);
        }
    }
    Ok(Value::Instance(InstanceValue {
        class_name: RuntimeName::from(layout.name.clone()),
        fields: fields.into(),
    }))
}

/// Writes the list `value` into a new C array of `layout`, one element
/// after another.
pub fn pack_elements(value: &Value, layout: &CStruct) -> Result<Vec<u8>, CValueError> {
    let Value::Vec(list) = value else {
        return Err(shape(format!("list[{}]", layout.name), value));
    };
    let size = layout.size();
    let mut bytes = vec![0_u8; size * list.elements.len()];
    for (index, held) in list.elements.iter().enumerate() {
        pack(&held, layout, &mut bytes[index * size..(index + 1) * size])?;
    }
    Ok(bytes)
}

/// The list of `layout` values in `bytes`, a C array a function wrote.
pub fn unpack_elements(layout: &CStruct, bytes: &[u8]) -> Result<Value, CValueError> {
    let elements = bytes
        .chunks_exact(layout.size().max(1))
        .map(|chunk| unpack(layout, chunk))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(Value::Vec(VecValue::new(
        Type::named(layout.name.clone()),
        ListElements::from(elements),
    )))
}

/// Reads member `kind` of a union whose bytes are `bytes`.
pub fn union_member(kind: &CFieldKind, bytes: &[u8]) -> Result<Value, CValueError> {
    unpack_kind(kind, bytes)
}

/// The bytes of a union of `size` bytes that holds `value` in a member of
/// kind `kind`, and zero elsewhere.
pub fn union_with_member(
    value: &Value,
    kind: &CFieldKind,
    field: &str,
    size: usize,
) -> Result<Vec<u8>, CValueError> {
    let mut bytes = vec![0_u8; size];
    pack_kind(value, kind, field, &mut bytes)?;
    Ok(bytes)
}

#[cfg(test)]
#[path = "c_values_tests.rs"]
mod tests;
