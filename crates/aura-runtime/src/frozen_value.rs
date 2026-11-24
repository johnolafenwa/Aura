//! Target-independent, typed data for an evaluated module binding.

use aura_core::{integer::IntegerValue, types::Type};
use serde::{Deserialize, Serialize};

use crate::list_elements::ListElements;
use crate::runtime_value::{
    EnumVariantValue, InstanceValue, MapValue, SetValue, TupleValue, UnionValue, Value, VecValue,
};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum FrozenValue {
    None,
    Bool(bool),
    Int(IntegerValue),
    Duration(i128),
    /// IEEE binary64 bits. An Aura float32 has already rounded to its width.
    Float(u64),
    Str(String),
    Bytes(Vec<u8>),
    Tuple(Vec<Type>, Vec<Self>),
    List(Type, Vec<Self>),
    Dict(Type, Type, Vec<(Self, Self)>),
    Set(Type, Vec<Self>),
    Class(String, Vec<(String, Self)>),
    Enum(String, String, Vec<Self>),
    Union(Type, usize, Box<Self>),
}

impl FrozenValue {
    /// Captures owned data, preserving iteration order and runtime numeric types.
    pub fn freeze(value: &Value) -> Option<Self> {
        let result = match value {
            Value::Unit => Self::None,
            Value::Bool(value) => Self::Bool(*value),
            Value::Int(value) => Self::Int(*value),
            Value::Duration(value) => Self::Duration(*value),
            Value::Float(value) => Self::Float(if value.is_nan() {
                f64::NAN.to_bits()
            } else {
                value.to_bits()
            }),
            Value::String(value) => Self::Str(value.as_str().to_string()),
            Value::Tuple(tuple) => Self::Tuple(
                tuple.element_types.clone(),
                tuple
                    .elements
                    .iter()
                    .map(Self::freeze)
                    .collect::<Option<_>>()?,
            ),
            Value::Vec(list) => {
                if let Some(bytes) = list.elements.as_uint8() {
                    Self::Bytes(bytes.to_vec())
                } else {
                    Self::List(
                        (*list.element_type).clone(),
                        list.elements
                            .iter()
                            .map(|v| Self::freeze(&v))
                            .collect::<Option<_>>()?,
                    )
                }
            }
            Value::Map(map) => Self::Dict(
                (*map.key_type).clone(),
                (*map.value_type).clone(),
                map.entries
                    .iter()
                    .map(|(k, v)| Some((Self::freeze(k)?, Self::freeze(v)?)))
                    .collect::<Option<_>>()?,
            ),
            Value::Set(set) => Self::Set(
                (*set.element_type).clone(),
                set.elements
                    .iter()
                    .map(Self::freeze)
                    .collect::<Option<_>>()?,
            ),
            // These built-in wrappers store an index in the local pattern
            // registry. Transport only classes whose fields stand alone.
            Value::Instance(object)
                if matches!(object.class_name.as_ref(), "regex.Regex" | "regex.Captures") =>
            {
                return None
            }
            Value::Instance(object) => Self::Class(
                object.class_name.to_string(),
                object
                    .fields
                    .iter()
                    .map(|(k, v)| Some((k.to_string(), Self::freeze(&v)?)))
                    .collect::<Option<_>>()?,
            ),
            Value::EnumVariant(variant) => Self::Enum(
                variant.enum_name.to_string(),
                variant.variant_name.to_string(),
                variant
                    .payloads
                    .iter()
                    .map(|v| Self::freeze(&v))
                    .collect::<Option<_>>()?,
            ),
            Value::Union(value) => Self::Union(
                (*value.union_type).clone(),
                value.member_index,
                Box::new(Self::freeze(&value.payload)?),
            ),
            _ => return None,
        };
        Some(result)
    }

    /// Reconstitutes a runtime view of an immutable value without running Aura code.
    pub fn thaw(&self) -> Value {
        let values = |items: &[Self]| items.iter().map(Self::thaw).collect::<Vec<_>>();
        match self {
            Self::None => Value::Unit,
            Self::Bool(v) => Value::Bool(*v),
            Self::Int(v) => Value::Int(*v),
            Self::Duration(v) => Value::Duration(*v),
            Self::Float(bits) => Value::Float(f64::from_bits(*bits)),
            Self::Str(s) => Value::string(s.as_str()),
            Self::Bytes(bytes) => Value::Vec(VecValue::new(
                Type::named("uint8"),
                ListElements::from_uint8(bytes.clone()),
            )),
            Self::Tuple(types, items) => Value::Tuple(TupleValue {
                element_types: types.clone(),
                elements: values(items),
            }),
            Self::List(ty, items) => Value::Vec(VecValue::new(ty.clone(), values(items).into())),
            Self::Dict(k, v, items) => Value::Map(MapValue {
                key_type: k.clone().into(),
                value_type: v.clone().into(),
                entries: items.iter().map(|(k, v)| (k.thaw(), v.thaw())).collect(),
            }),
            Self::Set(ty, items) => Value::Set(SetValue {
                element_type: ty.clone().into(),
                elements: items.iter().map(Self::thaw).collect(),
            }),
            Self::Class(name, fields) => Value::Instance(InstanceValue {
                class_name: name.into(),
                fields: fields.iter().map(|(k, v)| (k.clone(), v.thaw())).collect(),
            }),
            Self::Enum(name, variant, payloads) => Value::EnumVariant(EnumVariantValue {
                enum_name: name.into(),
                variant_name: variant.into(),
                payloads: values(payloads).into(),
            }),
            Self::Union(ty, index, payload) => Value::Union(Box::new(UnionValue {
                union_type: ty.clone().into(),
                member_index: *index,
                payload: payload.thaw(),
            })),
        }
    }
}
