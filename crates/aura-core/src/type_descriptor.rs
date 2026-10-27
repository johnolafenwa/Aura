//! A compact text descriptor for semantic types.
//!
//! Generated code hands types to the native runtime as constant data. The
//! descriptor replaces the JSON encoding those crossings used, so the
//! runtime reads types without linking a JSON parser.
//!
//! Grammar, one tag byte per node:
//!
//! | Tag | Node | Body |
//! | --- | --- | --- |
//! | `u` | unit | none |
//! | `n` | named | string, count, arguments |
//! | `t` | tuple | count, elements |
//! | `U` | union | module string, count, then key string and member per entry |
//! | `f` | function | count, parameters, return type |
//! | `c` | closure | call kind, count, parameters, return type, count, captures |
//! | `C` | callable | task flag, call kind, count, parameters, return type |
//! | `v` | returned view | mutable flag, origin, pointee |
//! | `p` | type parameter | string |
//! | `m` | module | string |
//!
//! A string is its UTF-8 byte length, `:`, then the bytes. A count or number
//! is decimal digits ended by `;`. A flag is `0` or `1`.

use std::collections::BTreeMap;
use std::fmt::Write as _;

use crate::diag::Span;
use crate::ops::ReceiverKind;
use crate::type_budget::ExpansionBudget;
use crate::types::{
    CallableType, ClosureCallKind, ClosureCapture, ClosureCaptureMode, FunctionParamContract,
    ReturnedViewType, Type, UnionType,
};

/// The prefix every descriptor starts with. The version changes whenever the
/// grammar does.
pub const DESCRIPTOR_PREFIX: &str = "aura-type-v2:";

/// Nesting deeper than this is rejected rather than recursed into.
const MAX_DESCRIPTOR_DEPTH: usize = 256;

/// Encodes a type as a descriptor.
pub fn encode(ty: &Type) -> String {
    let mut out = String::from(DESCRIPTOR_PREFIX);
    push_type(&mut out, ty);
    out
}

/// Decodes a descriptor that `encode` wrote.
pub fn decode(descriptor: &str) -> Result<Type, String> {
    let body = descriptor
        .strip_prefix(DESCRIPTOR_PREFIX)
        .ok_or_else(|| "type descriptor has an unknown version".to_string())?;
    let mut reader = Reader {
        bytes: body.as_bytes(),
        position: 0,
    };
    let ty = reader.read_type(0)?;
    if reader.position != reader.bytes.len() {
        return Err("type descriptor has trailing bytes".to_string());
    }
    Ok(ty)
}

pub(crate) fn push_string(out: &mut String, text: &str) {
    let _ = write!(out, "{}:", text.len());
    out.push_str(text);
}

pub(crate) fn push_count(out: &mut String, value: usize) {
    let _ = write!(out, "{value};");
}

fn push_flag(out: &mut String, value: bool) {
    out.push(if value { '1' } else { '0' });
}

fn passing_tag(passing: ReceiverKind) -> char {
    match passing {
        ReceiverKind::Value => 'v',
        ReceiverKind::Borrow => 'b',
        ReceiverKind::BorrowMut => 'B',
    }
}

fn call_kind_tag(kind: ClosureCallKind) -> char {
    match kind {
        ClosureCallKind::Repeatable => 'r',
        ClosureCallKind::MutableRepeatable => 'm',
        ClosureCallKind::Consuming => 'o',
    }
}

fn capture_mode_tag(mode: ClosureCaptureMode) -> char {
    match mode {
        ClosureCaptureMode::Copy => 'c',
        ClosureCaptureMode::Move => 'm',
        ClosureCaptureMode::SharedView => 's',
        ClosureCaptureMode::MutableView => 'M',
    }
}

fn push_params(out: &mut String, params: &[FunctionParamContract]) {
    push_count(out, params.len());
    for param in params {
        push_flag(out, param.keyword_only);
        push_string(out, &param.name);
        out.push(passing_tag(param.passing));
        push_flag(out, param.has_default);
        push_type(out, &param.ty);
    }
}

pub(crate) fn push_type(out: &mut String, ty: &Type) {
    match ty {
        Type::Unit => out.push('u'),
        Type::Named(name, args) => {
            out.push('n');
            push_string(out, name);
            push_count(out, args.len());
            for arg in args {
                push_type(out, arg);
            }
        }
        Type::Tuple(elements) => {
            out.push('t');
            push_count(out, elements.len());
            for element in elements {
                push_type(out, element);
            }
        }
        Type::Union(union) => {
            out.push('U');
            push_string(out, &union.module_name);
            push_count(out, union.members.len());
            for (key, member) in union.keys.iter().zip(&union.members) {
                push_string(out, key);
                push_type(out, member);
            }
        }
        Type::Function {
            params,
            return_type,
        } => {
            out.push('f');
            push_params(out, params);
            push_type(out, return_type);
        }
        Type::Closure {
            params,
            return_type,
            captures,
            call_kind,
        } => {
            out.push('c');
            out.push(call_kind_tag(*call_kind));
            push_params(out, params);
            push_type(out, return_type);
            push_count(out, captures.len());
            for capture in captures.iter() {
                push_string(out, &capture.name);
                out.push(capture_mode_tag(capture.mode));
                push_count(out, capture.span.line);
                push_count(out, capture.span.column);
                push_flag(out, capture.mutated);
                push_type(out, &capture.ty);
            }
        }
        Type::Callable(callable) => {
            out.push('C');
            push_flag(out, callable.task);
            out.push(call_kind_tag(callable.call_kind));
            push_params(out, &callable.params);
            push_type(out, &callable.return_type);
        }
        Type::ReturnedView(view) => {
            out.push('v');
            push_flag(out, view.mutable);
            push_count(out, view.origin);
            push_type(out, &view.pointee);
        }
        Type::TypeParam(name) => {
            out.push('p');
            push_string(out, name);
        }
        Type::Module(name) => {
            out.push('m');
            push_string(out, name);
        }
    }
}

pub(crate) struct Reader<'a> {
    pub(crate) bytes: &'a [u8],
    pub(crate) position: usize,
}

impl Reader<'_> {
    pub(crate) fn byte(&mut self) -> Result<u8, String> {
        let byte = *self
            .bytes
            .get(self.position)
            .ok_or_else(|| "type descriptor ends early".to_string())?;
        self.position += 1;
        Ok(byte)
    }

    fn digits_until(&mut self, end: u8) -> Result<usize, String> {
        let start = self.position;
        let mut value: usize = 0;
        loop {
            let byte = self.byte()?;
            if byte == end && self.position - 1 > start {
                return Ok(value);
            }
            if !byte.is_ascii_digit() {
                return Err("type descriptor has a malformed number".to_string());
            }
            value = value
                .checked_mul(10)
                .and_then(|value| value.checked_add(usize::from(byte - b'0')))
                .ok_or_else(|| "type descriptor number overflows".to_string())?;
        }
    }

    pub(crate) fn count(&mut self) -> Result<usize, String> {
        let count = self.digits_until(b';')?;
        // Every counted item takes at least one byte, so a count larger than
        // the rest of the input is malformed and must not size an allocation.
        if count > self.bytes.len() - self.position {
            return Err("type descriptor count exceeds its input".to_string());
        }
        Ok(count)
    }

    pub(crate) fn string(&mut self) -> Result<String, String> {
        let len = self.digits_until(b':')?;
        let end = self
            .position
            .checked_add(len)
            .filter(|end| *end <= self.bytes.len())
            .ok_or_else(|| "type descriptor string ends early".to_string())?;
        let text = std::str::from_utf8(&self.bytes[self.position..end])
            .map_err(|_| "type descriptor string is not UTF-8".to_string())?;
        self.position = end;
        Ok(text.to_string())
    }

    fn flag(&mut self) -> Result<bool, String> {
        match self.byte()? {
            b'0' => Ok(false),
            b'1' => Ok(true),
            _ => Err("type descriptor has a malformed flag".to_string()),
        }
    }

    fn passing(&mut self) -> Result<ReceiverKind, String> {
        match self.byte()? {
            b'v' => Ok(ReceiverKind::Value),
            b'b' => Ok(ReceiverKind::Borrow),
            b'B' => Ok(ReceiverKind::BorrowMut),
            _ => Err("type descriptor has an unknown passing mode".to_string()),
        }
    }

    fn call_kind(&mut self) -> Result<ClosureCallKind, String> {
        match self.byte()? {
            b'r' => Ok(ClosureCallKind::Repeatable),
            b'm' => Ok(ClosureCallKind::MutableRepeatable),
            b'o' => Ok(ClosureCallKind::Consuming),
            _ => Err("type descriptor has an unknown call kind".to_string()),
        }
    }

    fn capture_mode(&mut self) -> Result<ClosureCaptureMode, String> {
        match self.byte()? {
            b'c' => Ok(ClosureCaptureMode::Copy),
            b'm' => Ok(ClosureCaptureMode::Move),
            b's' => Ok(ClosureCaptureMode::SharedView),
            b'M' => Ok(ClosureCaptureMode::MutableView),
            _ => Err("type descriptor has an unknown capture mode".to_string()),
        }
    }

    fn params(&mut self, depth: usize) -> Result<Vec<FunctionParamContract>, String> {
        let count = self.count()?;
        let mut params = Vec::with_capacity(count);
        for _ in 0..count {
            let keyword_only = self.flag()?;
            let name = self.string()?;
            let passing = self.passing()?;
            let has_default = self.flag()?;
            let ty = self.read_type(depth + 1)?;
            params.push(FunctionParamContract {
                keyword_only,
                name,
                ty,
                passing,
                has_default,
            });
        }
        Ok(params)
    }

    fn types(&mut self, depth: usize) -> Result<Vec<Type>, String> {
        let count = self.count()?;
        let mut types = Vec::with_capacity(count);
        for _ in 0..count {
            types.push(self.read_type(depth + 1)?);
        }
        Ok(types)
    }

    pub(crate) fn read_type(&mut self, depth: usize) -> Result<Type, String> {
        if depth > MAX_DESCRIPTOR_DEPTH {
            return Err("type descriptor nests too deeply".to_string());
        }
        Ok(match self.byte()? {
            b'u' => Type::Unit,
            b'n' => {
                let name = self.string()?;
                Type::Named(name, self.types(depth)?)
            }
            b't' => Type::Tuple(self.types(depth)?),
            b'U' => {
                let module_name = self.string()?;
                let count = self.count()?;
                let mut keys = Vec::with_capacity(count);
                let mut members = Vec::with_capacity(count);
                for _ in 0..count {
                    keys.push(self.string()?);
                    members.push(self.read_type(depth + 1)?);
                }
                validate_union(&members, &keys, &module_name)?;
                Type::Union(Box::new(UnionType {
                    members,
                    keys,
                    module_name,
                }))
            }
            b'f' => {
                let params = self.params(depth)?;
                let return_type = Box::new(self.read_type(depth + 1)?);
                Type::Function {
                    params,
                    return_type,
                }
            }
            b'c' => {
                let call_kind = self.call_kind()?;
                let params = Box::new(self.params(depth)?);
                let return_type = Box::new(self.read_type(depth + 1)?);
                let count = self.count()?;
                let mut captures = Vec::with_capacity(count);
                for _ in 0..count {
                    let name = self.string()?;
                    let mode = self.capture_mode()?;
                    let line = self.digits_until(b';')?;
                    let column = self.digits_until(b';')?;
                    let mutated = self.flag()?;
                    let ty = self.read_type(depth + 1)?;
                    captures.push(ClosureCapture {
                        name,
                        ty,
                        mode,
                        span: Span::new(line, column),
                        mutated,
                    });
                }
                Type::Closure {
                    params,
                    return_type,
                    captures: Box::new(captures),
                    call_kind,
                }
            }
            b'C' => {
                let task = self.flag()?;
                let call_kind = self.call_kind()?;
                let params = self.params(depth)?;
                let return_type = self.read_type(depth + 1)?;
                Type::Callable(Box::new(CallableType {
                    task,
                    call_kind,
                    params,
                    return_type,
                }))
            }
            b'v' => {
                let mutable = self.flag()?;
                let origin = self.digits_until(b';')?;
                let pointee = self.read_type(depth + 1)?;
                Type::ReturnedView(Box::new(ReturnedViewType {
                    mutable,
                    pointee,
                    origin,
                }))
            }
            b'p' => Type::TypeParam(self.string()?),
            b'm' => Type::Module(self.string()?),
            _ => return Err("type descriptor has an unknown type tag".to_string()),
        })
    }
}

/// Applies the checks a stored union must pass, as its deserializer does.
fn validate_union(members: &[Type], keys: &[String], module_name: &str) -> Result<(), String> {
    let invalid = |reason: &str| format!("invalid union metadata: {reason}");
    if members.len() < 2 {
        return Err(invalid("a stored union requires at least two members"));
    }
    if keys.windows(2).any(|pair| pair[0] >= pair[1]) {
        return Err(invalid("keys must be strictly ordered and unique"));
    }
    for (member, key) in members.iter().zip(keys) {
        if matches!(member, Type::Union(_)) {
            return Err(invalid("nested members must be flattened"));
        }
        ExpansionBudget::default()
            .check_canonical_key(member, module_name, &BTreeMap::new(), Span::new(1, 1))
            .map_err(|error| invalid(&error.message))?;
        if member.canonical_key(module_name, &BTreeMap::new()) != *key {
            return Err(invalid("member key does not match its resolved type"));
        }
    }
    Ok(())
}

#[cfg(test)]
#[path = "type_descriptor_tests.rs"]
mod tests;
