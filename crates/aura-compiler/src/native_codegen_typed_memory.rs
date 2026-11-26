//! Convert signed integers and floats between scalar ABI words and memory.
use super::*;
use crate::native_backend::MemoryAccess;

pub(super) fn operation(name: &str) -> Option<(MemoryAccess, ScalarKind)> {
    if name == "__pointer_offset" {
        return Some((MemoryAccess::PointerOffset, ScalarKind::Uint64));
    }
    let (op, kind) = name.rsplit_once('_')?;
    let kind = ScalarKind::of_name(kind)?;
    if !matches!(
        kind,
        ScalarKind::Bool
            | ScalarKind::Int8
            | ScalarKind::Int16
            | ScalarKind::Int32
            | ScalarKind::Int64
            | ScalarKind::Float32
            | ScalarKind::Float64
    ) {
        return None;
    }
    let access = match op {
        "read" => MemoryAccess::Read,
        "write" => MemoryAccess::Write,
        "volatile_read" => MemoryAccess::VolatileRead,
        "volatile_write" => MemoryAccess::VolatileWrite,
        _ => return None,
    };
    Some((access, kind))
}

impl FunctionCompiler<'_> {
    pub(super) fn compile_typed_memory(
        &mut self,
        access: MemoryAccess,
        kind: ScalarKind,
        words: &[Val],
    ) -> Option<ValueRef> {
        let (width, shift) = match kind {
            ScalarKind::Int8 => (types::I8, 56),
            ScalarKind::Int16 => (types::I16, 48),
            ScalarKind::Int32 | ScalarKind::Float32 => (types::I32, 32),
            ScalarKind::Int64 | ScalarKind::Float64 => (types::I64, 0),
            _ => return None,
        };
        if matches!(access, MemoryAccess::Write | MemoryAccess::VolatileWrite) {
            let value = match kind {
                ScalarKind::Float32 => {
                    let single = self.builder.fdemote(types::F32, words[1]);
                    let bits = self.builder.bitcast(types::I32, single);
                    self.builder.uextend(types::I64, bits)
                }
                ScalarKind::Float64 => self.builder.bitcast(types::I64, words[1]),
                _ => words[1],
            };
            self.builder
                .memory_access(access, width, &[words[0], value]);
            return Some(unit_value(self.builder));
        }
        let raw = self.builder.memory_access(access, width, words)?;
        let result = match kind {
            ScalarKind::Float32 => {
                let bits = self.builder.ireduce(types::I32, raw);
                let float = self.builder.bitcast(types::F32, bits);
                self.builder.fpromote(types::F64, float)
            }
            ScalarKind::Float64 => self.builder.bitcast(types::F64, raw),
            _ if shift != 0 => {
                let count = self.builder.iconst(types::I64, shift);
                let high = self.builder.ishl(raw, count);
                self.builder.sshr(high, count)
            }
            _ => raw,
        };
        Some(ValueRef {
            values: vec![result],
            ty: DirectType::Scalar(kind),
        })
    }
}
