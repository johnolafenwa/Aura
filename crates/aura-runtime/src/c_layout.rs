//! The layout of a C struct, and how the C calling convention passes one
//! by value.
//!
//! An `extern "C" class` is a struct of fixed-width scalars and other such
//! structs. The native backend and the interpreter both lay it out with
//! this module, so they agree on every offset.

use serde::{Deserialize, Serialize};

use crate::ffi::FfiScalar;

/// A C struct: its fields in declaration order.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct CStruct {
    /// The Aura name of the class.
    pub name: String,
    pub fields: Vec<CField>,
}

/// One field of a [`CStruct`].
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct CField {
    pub name: String,
    pub kind: CFieldKind,
}

/// What a field of a [`CStruct`] holds.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum CFieldKind {
    Scalar(FfiScalar),
    /// A struct held by value.
    Struct(CStruct),
}

impl CFieldKind {
    fn size(&self) -> usize {
        match self {
            Self::Scalar(scalar) => scalar.size(),
            Self::Struct(inner) => inner.size(),
        }
    }

    fn align(&self) -> usize {
        match self {
            Self::Scalar(scalar) => scalar.size(),
            Self::Struct(inner) => inner.align(),
        }
    }
}

/// One scalar of a struct at its byte offset, with every nested struct
/// flattened.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CLeaf {
    pub offset: usize,
    pub scalar: FfiScalar,
}

/// A C calling convention for 64-bit targets.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CAbi {
    /// The System V convention of x86-64 Linux and macOS.
    SysV64,
    /// The procedure call standard of AArch64 Linux and macOS.
    Aapcs64,
    /// The Microsoft convention of x86-64 Windows.
    Win64,
}

impl CAbi {
    /// The convention of the architecture `arch`, as a target triple names
    /// it, when Aura passes structs on it.
    pub fn of_arch(arch: &str) -> Option<Self> {
        match arch {
            "x86_64" => Some(Self::SysV64),
            "aarch64" | "arm64" => Some(Self::Aapcs64),
            _ => None,
        }
    }

    /// The convention of the architecture `arch` on the system `os`, as
    /// `std::env::consts` names them.
    pub fn of_target(arch: &str, os: &str) -> Option<Self> {
        match (arch, os) {
            ("x86_64", "windows") => Some(Self::Win64),
            (_, "windows") => None,
            _ => Self::of_arch(arch),
        }
    }

    /// The convention of the machine this code was built for.
    pub fn host() -> Option<Self> {
        Self::of_target(std::env::consts::ARCH, std::env::consts::OS)
    }
}

/// One register's share of a struct that is passed in registers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CPiece {
    /// The `size` bytes at `offset`, at most 8, in a general register.
    Int { offset: usize, size: usize },
    /// The `size` bytes at `offset`, at most 8, in the low bytes of a
    /// vector register: the SSE class of System V.
    Sse { offset: usize, size: usize },
    /// One member of a homogeneous floating-point aggregate, in a
    /// floating-point register of its own.
    Float { offset: usize, scalar: FfiScalar },
}

/// How a struct crosses a call by value, as an argument or as a result.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CPassing {
    /// In registers, one piece each, in order.
    Registers(Vec<CPiece>),
    /// In memory. As an argument, System V copies it to the stack, and
    /// AArch64 and Windows pass the address of a copy. As a result, the
    /// caller passes the address of the storage.
    Memory,
}

/// How a struct travels as one argument of a call.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CArgument {
    /// In registers, one piece each, in order.
    Registers(Vec<CPiece>),
    /// Copied to the argument area of the stack.
    Stack,
    /// As the address of a copy the caller makes.
    Address,
}

impl CStruct {
    /// How `abi` passes the struct as an argument that follows arguments
    /// which took `general_used` general registers and `float_used`
    /// floating-point registers.
    ///
    /// System V passes a struct on the stack when its pieces do not all
    /// fit the registers that are left. AArch64 does the same, but in a
    /// stack layout that separate arguments cannot spell, so that case is
    /// an error here. Windows gives every argument one slot, so its one
    /// piece always travels as an argument of its own.
    pub fn argument(
        &self,
        abi: CAbi,
        general_used: usize,
        float_used: usize,
    ) -> Result<CArgument, String> {
        let pieces = match (self.passing(abi), abi) {
            (CPassing::Registers(pieces), CAbi::SysV64 | CAbi::Aapcs64) => pieces,
            (CPassing::Memory, CAbi::SysV64) => return Ok(CArgument::Stack),
            (CPassing::Memory, CAbi::Aapcs64 | CAbi::Win64) => return Ok(CArgument::Address),
            (CPassing::Registers(pieces), CAbi::Win64) => return Ok(CArgument::Registers(pieces)),
        };
        let general = pieces
            .iter()
            .filter(|piece| matches!(piece, CPiece::Int { .. }))
            .count();
        let float = pieces.len() - general;
        let general_registers = match abi {
            CAbi::SysV64 => 6,
            CAbi::Aapcs64 | CAbi::Win64 => 8,
        };
        if general_used + general <= general_registers && float_used + float <= 8 {
            return Ok(CArgument::Registers(pieces));
        }
        match abi {
            CAbi::SysV64 => Ok(CArgument::Stack),
            CAbi::Aapcs64 | CAbi::Win64 => Err(format!(
                "struct `{}` does not fit the argument registers that are left on this target; \
                 declare it earlier in the parameter list or pass it as `mut {}`",
                self.name, self.name
            )),
        }
    }

    /// A copy of the description that lives as long as the process, shared
    /// by every caller that asks for an equal one. A boundary type names a
    /// struct by this reference.
    pub fn interned(&self) -> &'static CStruct {
        static TABLE: std::sync::Mutex<Vec<&'static CStruct>> = std::sync::Mutex::new(Vec::new());
        let mut table = TABLE.lock().unwrap_or_else(|poison| poison.into_inner());
        if let Some(found) = table.iter().find(|known| **known == self) {
            return found;
        }
        let leaked: &'static CStruct = Box::leak(Box::new(self.clone()));
        table.push(leaked);
        leaked
    }

    /// The alignment of the struct in bytes: that of its strictest field.
    pub fn align(&self) -> usize {
        self.fields
            .iter()
            .map(|field| field.kind.align())
            .max()
            .unwrap_or(1)
    }

    /// The byte offset of each field, in order, and the size of the struct
    /// with its tail padding.
    fn offsets(&self) -> (Vec<usize>, usize) {
        let mut end = 0_usize;
        let offsets = self
            .fields
            .iter()
            .map(|field| {
                let offset = end.next_multiple_of(field.kind.align());
                end = offset + field.kind.size();
                offset
            })
            .collect();
        (offsets, end.next_multiple_of(self.align()))
    }

    /// The size of the struct in bytes, as `sizeof` gives it.
    pub fn size(&self) -> usize {
        self.offsets().1
    }

    /// The byte offset of each field, in order.
    pub fn field_offsets(&self) -> Vec<usize> {
        self.offsets().0
    }

    /// Every scalar of the struct in order, with nested structs flattened.
    pub fn leaves(&self) -> Vec<CLeaf> {
        let mut leaves = Vec::new();
        self.push_leaves(0, &mut leaves);
        leaves
    }

    fn push_leaves(&self, base: usize, leaves: &mut Vec<CLeaf>) {
        for (field, offset) in self.fields.iter().zip(self.field_offsets()) {
            match &field.kind {
                CFieldKind::Scalar(scalar) => leaves.push(CLeaf {
                    offset: base + offset,
                    scalar: *scalar,
                }),
                CFieldKind::Struct(inner) => inner.push_leaves(base + offset, leaves),
            }
        }
    }

    /// How `abi` passes the struct by value.
    pub fn passing(&self, abi: CAbi) -> CPassing {
        let size = self.size();
        let leaves = self.leaves();
        let is_float = |leaf: &CLeaf| matches!(leaf.scalar, FfiScalar::F32 | FfiScalar::F64);
        if abi == CAbi::Aapcs64
            && leaves.len() <= 4
            && leaves.iter().all(is_float)
            && leaves.iter().all(|leaf| leaf.scalar == leaves[0].scalar)
        {
            return CPassing::Registers(
                leaves
                    .iter()
                    .map(|leaf| CPiece::Float {
                        offset: leaf.offset,
                        scalar: leaf.scalar,
                    })
                    .collect(),
            );
        }
        if abi == CAbi::Win64 {
            // Windows passes a struct of 1, 2, 4 or 8 bytes as an integer of
            // that size, whatever its fields, and every other struct in
            // memory.
            return if matches!(size, 1 | 2 | 4 | 8) {
                CPassing::Registers(vec![CPiece::Int { offset: 0, size }])
            } else {
                CPassing::Memory
            };
        }
        if size > 16 {
            return CPassing::Memory;
        }
        let pieces = (0..size)
            .step_by(8)
            .map(|offset| {
                let piece = (size - offset).min(8);
                // Natural alignment keeps a scalar inside one eightbyte.
                let all_float = leaves
                    .iter()
                    .filter(|leaf| leaf.offset / 8 == offset / 8)
                    .all(is_float);
                match (abi, all_float) {
                    (CAbi::SysV64, true) => CPiece::Sse {
                        offset,
                        size: piece,
                    },
                    _ => CPiece::Int {
                        offset,
                        size: piece,
                    },
                }
            })
            .collect();
        CPassing::Registers(pieces)
    }
}

#[cfg(test)]
#[path = "c_layout_tests.rs"]
mod tests;
