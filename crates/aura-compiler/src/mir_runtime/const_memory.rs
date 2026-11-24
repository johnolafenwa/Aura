//! Logical storage charges for the build-time interpreter. A scalar uses
//! its Aura width. Collections own their elements, strings own UTF-8 bytes,
//! and stored values that need an indirection own one target pointer.
use super::*;

use aura_core::layout::{ScalarLayout, POINTER_BYTES};

pub(super) const WORD: usize = POINTER_BYTES as usize;

/// Sum only up to the language limit: a shared runtime collection can
/// describe substantially more logical storage than the interpreter owns.
fn sum(sizes: impl IntoIterator<Item = usize>) -> usize {
    let mut total = 0usize;
    for size in sizes {
        total = total.saturating_add(size);
        if total > crate::const_eval::MEMORY_LIMIT {
            break;
        }
    }
    total
}

pub(super) fn value_bytes(value: &Value, depth: usize) -> usize {
    // A nested owned payload must fit the same traversal stack allowance
    // as nested Aura calls. Check before descending further.
    if depth >= 64 {
        return usize::MAX;
    }
    let nested = |v: &Value| value_bytes(v, depth + 1);
    match value {
        Value::Unit => 0,
        Value::Bool(_) => 1,
        Value::Int(n) => n.runtime_kind().map_or(16, |k| k.bit_width() as usize / 8),
        Value::Float(_) => 8,
        Value::Duration(_) => 16,
        Value::String(s) => s.len(),
        Value::Vec(list) => match &*list.element_type {
            Type::Named(name, _) if ScalarLayout::of_name(name).is_some() => list
                .elements
                .len()
                .saturating_mul(ScalarLayout::of_name(name).unwrap().size() as usize),
            _ => sum(list
                .elements
                .iter()
                .map(|v| WORD.saturating_add(nested(&v)))),
        },
        Value::Array(array) => array.len().saturating_mul(
            ScalarLayout::of_name(array.dtype().runtime_type_name())
                .unwrap()
                .size() as usize,
        ),
        Value::Set(set) => sum(set.elements.iter().map(|v| WORD.saturating_add(nested(v)))),
        Value::Map(map) => sum(map.entries.iter().map(|(k, v)| {
            (WORD * 2)
                .saturating_add(nested(k))
                .saturating_add(nested(v))
        })),
        Value::Tuple(t) => WORD.saturating_add(sum(t.elements.iter().map(nested))),
        Value::Instance(i) => WORD.saturating_add(sum(i.fields.values().map(|v| nested(&v)))),
        Value::EnumVariant(v) => WORD.saturating_add(sum(v.payloads.iter().map(|v| nested(&v)))),
        Value::Union(u) => WORD.saturating_add(nested(&u.payload)),
        Value::TraitObject(o) => WORD.saturating_add(nested(&o.value)),
        _ => WORD,
    }
}

pub(super) fn operand_bytes(op: &Operand, env: &Env) -> usize {
    match op {
        Operand::String(s) => s.len(),
        Operand::Place(p) | Operand::MovePlace(p) => {
            env.place_ref(p).map_or(0, |v| value_bytes(&v, 0))
        }
        Operand::Int(_) | Operand::Duration(_) => 16,
        Operand::Float(_) => 8,
        Operand::Bool(_) => 1,
        _ => 0,
    }
}

pub(super) fn count(arg: Option<&MirArg>, env: &Env) -> usize {
    match arg.map(|a| &a.value) {
        Some(Operand::Int(n)) => usize::try_from(*n).unwrap_or(usize::MAX),
        Some(Operand::MovePlace(p) | Operand::Place(p)) => env
            .place_ref(p)
            .ok()
            .and_then(|v| match &*v {
                Value::Int(n) => Some(match n.representation() {
                    IntegerRepresentation::Signed(i) => {
                        usize::try_from(i.max(0)).unwrap_or(usize::MAX)
                    }
                    IntegerRepresentation::Unsigned(u) => usize::try_from(u).unwrap_or(usize::MAX),
                }),
                _ => None,
            })
            .unwrap_or(0),
        _ => 0,
    }
}

fn text<'a>(op: &'a Operand, env: &'a Env) -> Option<&'a str> {
    match op {
        Operand::String(s) => Some(s),
        Operand::MovePlace(p) | Operand::Place(p) => match env.place_borrow(p).ok()? {
            Value::String(s) => Some(s),
            _ => None,
        },
        _ => None,
    }
}

pub(super) fn text_output(op: StrOp, receiver: &Operand, args: &[MirArg], env: &Env) -> usize {
    let Some(s) = text(receiver, env) else {
        return 0;
    };
    match op {
        StrOp::Repeat => s.len().saturating_mul(count(args.first(), env)),
        StrOp::Replace => {
            let Some(pattern) = args.first().and_then(|a| text(&a.value, env)) else {
                return 0;
            };
            let Some(replacement) = args.get(1).and_then(|a| text(&a.value, env)) else {
                return 0;
            };
            let matches = s.matches(pattern).count();
            s.len()
                .saturating_sub(matches.saturating_mul(pattern.len()))
                .saturating_add(matches.saturating_mul(replacement.len()))
        }
        StrOp::Join => {
            let Some(Operand::Place(p) | Operand::MovePlace(p)) = args.first().map(|a| &a.value)
            else {
                return 0;
            };
            let Ok(list) = env.place_ref(p) else {
                return 0;
            };
            let Value::Vec(list) = &*list else {
                return 0;
            };
            let separator_bytes = s
                .len()
                .saturating_mul(list.elements.len().saturating_sub(1));
            separator_bytes.saturating_add(sum(list.elements.iter().map(|v| match &*v {
                Value::String(text) => text.len(),
                _ => 0,
            })))
        }
        StrOp::Len
        | StrOp::ByteLen
        | StrOp::Contains
        | StrOp::StartsWith
        | StrOp::EndsWith
        | StrOp::Find
        | StrOp::Count => 0,
        StrOp::Chars | StrOp::Lines | StrOp::SplitWhitespace | StrOp::Split => {
            s.len().saturating_mul(WORD + 1)
        }
        StrOp::ToUpper | StrOp::ToLower => s.len().saturating_mul(3),
        _ => s.len(),
    }
}

pub(super) fn format_bytes(parts: &[MirFormatPart], env: &Env) -> usize {
    sum(parts.iter().map(|part| match part {
        MirFormatPart::Literal(text) => text.len(),
        MirFormatPart::Formatted { value, spec, .. } => {
            let width = crate::runtime_value::parse_format_spec(spec).map_or(0, |spec| {
                spec.width
                    .unwrap_or(0)
                    .saturating_mul(spec.fill.len_utf8())
                    .max(spec.precision.unwrap_or(0))
            });
            width.max(operand_bytes(value, env))
        }
        MirFormatPart::Int64(value) | MirFormatPart::Value(value) => operand_bytes(value, env),
    }))
}

/// Bounds materialization by third-party map parsers and string serializers
/// that do not accept an output limit. Delimiters can introduce another
/// owned field, table or list slot. Escaping one input byte takes up to
/// six bytes of serialized text.
pub(super) fn host_output_bound(name: &str, args: &[MirArg], env: &Env) -> usize {
    match name {
        "json::stringify_map" | "toml::stringify_map" => args
            .first()
            .map_or(0, |arg| operand_bytes(&arg.value, env).saturating_mul(8)),
        "toml::parse_string_map" | "toml::is_valid" | "json::parse_string_map" => args
            .first()
            .and_then(|arg| text(&arg.value, env))
            .map_or(0, |source| {
                let fields = source
                    .bytes()
                    .filter(|b| matches!(b, b',' | b'.' | b'=' | b':' | b'[' | b'{'))
                    .count();
                source.len().saturating_add(fields.saturating_mul(WORD * 4))
            }),
        "path::join" => args.iter().fold(1usize, |size, arg| {
            size.saturating_add(operand_bytes(&arg.value, env))
        }),
        "path::file_name" | "path::extension" | "path::parent" => {
            args.first().map_or(0, |arg| operand_bytes(&arg.value, env))
        }
        _ => 0,
    }
}
