//! The layout of a C struct or union, and how the C calling convention
//! passes one by value.
//!
//! An `extern "C" class` is a struct of fixed-width scalars, other such
//! structs, unions, and fixed-size arrays of them. An `extern "C" union`
//! holds the same kinds of members, all at offset 0. The native backend
//! and the interpreter both lay them out with this module, so they agree
//! on every offset.

use serde::{Deserialize, Serialize};

use crate::ffi::FfiScalar;

/// A C struct or union: its fields in declaration order.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct CStruct {
    /// The Aura name of the class.
    pub name: String,
    pub fields: Vec<CField>,
    /// The fields are the members of a union: each starts at offset 0.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub union: bool,
}

/// The name of the one field an `extern "C" union` has as an Aura class:
/// its bytes.
pub const UNION_BYTES_FIELD: &str = "bytes";

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
    /// A struct or union held by value.
    Struct(CStruct),
    /// A fixed-size array held in place: this many elements of one kind.
    Array(Box<CFieldKind>, usize),
}

impl CFieldKind {
    /// The size of a value of the kind in bytes.
    pub fn size(&self) -> usize {
        match self {
            Self::Scalar(scalar) => scalar.size(),
            Self::Struct(inner) => inner.size(),
            Self::Array(element, count) => element.size() * count,
        }
    }

    /// The alignment of a value of the kind in bytes.
    pub fn align(&self) -> usize {
        match self {
            Self::Scalar(scalar) => scalar.size(),
            Self::Struct(inner) => inner.align(),
            Self::Array(element, _) => element.align(),
        }
    }

    /// Whether a value of the kind is scalars only, each at one offset:
    /// no array and no union at any depth.
    fn is_plain(&self) -> bool {
        match self {
            Self::Scalar(_) => true,
            Self::Struct(inner) => inner.is_plain(),
            Self::Array(..) => false,
        }
    }

    fn holds_union(&self) -> bool {
        match self {
            Self::Scalar(_) => false,
            Self::Struct(inner) => inner.holds_union(),
            Self::Array(element, _) => element.holds_union(),
        }
    }

    fn push_leaves(&self, base: usize, leaves: &mut Vec<CLeaf>) {
        match self {
            Self::Scalar(scalar) => leaves.push(CLeaf {
                offset: base,
                scalar: *scalar,
            }),
            Self::Struct(inner) => inner.push_leaves(base, leaves),
            Self::Array(element, count) => {
                let size = element.size();
                for index in 0..*count {
                    element.push_leaves(base + index * size, leaves);
                }
            }
        }
    }
}

/// One scalar of a struct at its byte offset, with every nested struct,
/// union, and array flattened. The scalars of a union's members overlap.
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
    /// System V and AArch64 pass a struct on the stack when its pieces do
    /// not all fit the registers that are left. On AArch64 no later
    /// argument then takes a register of the kind the struct needed.
    /// Windows gives every argument one slot, so its one piece always
    /// travels as an argument of its own.
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
        // The arguments before may have used up one kind of register and
        // gone on to the stack. That matters only to a struct that needs
        // a register of that kind.
        let general_fit = general == 0 || general_used + general <= general_registers;
        let float_fit = float == 0 || float_used + float <= 8;
        if general_fit && float_fit {
            return Ok(CArgument::Registers(pieces));
        }
        Ok(CArgument::Stack)
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

    /// Places one field at its aligned offset and advances the struct's end.
    fn place_field(&self, field: &CField, end: &mut usize) -> usize {
        let offset = if self.union {
            0
        } else {
            end.next_multiple_of(field.kind.align())
        };
        *end = (*end).max(offset + field.kind.size());
        offset
    }

    /// Fields and their byte offsets in declaration order. All members of
    /// a union start at byte zero.
    pub(crate) fn fields_with_offsets(&self) -> impl Iterator<Item = (&CField, usize)> {
        let mut end = 0;
        self.fields
            .iter()
            .map(move |field| (field, self.place_field(field, &mut end)))
    }

    /// Whether the struct is scalars only, each at one offset: it is not a
    /// union and holds no array and no union at any depth. Generated code
    /// keeps such a class as one word per scalar.
    pub fn is_plain(&self) -> bool {
        !self.union && self.fields.iter().all(|field| field.kind.is_plain())
    }

    /// The size of the struct in bytes, as `sizeof` gives it.
    pub fn size(&self) -> usize {
        let mut end = 0;
        for field in &self.fields {
            self.place_field(field, &mut end);
        }
        end.next_multiple_of(self.align())
    }

    /// The byte offset of each field, in order.
    pub fn field_offsets(&self) -> Vec<usize> {
        self.fields_with_offsets()
            .map(|(_, offset)| offset)
            .collect()
    }

    /// Whether the struct is a union or holds one at any depth.
    pub fn holds_union(&self) -> bool {
        self.union || self.fields.iter().any(|field| field.kind.holds_union())
    }

    /// Every scalar of the struct in order, with nested structs, unions,
    /// and arrays flattened. The scalars of a union's members overlap.
    pub fn leaves(&self) -> Vec<CLeaf> {
        let mut leaves = Vec::new();
        self.push_leaves(0, &mut leaves);
        leaves
    }

    fn push_leaves(&self, base: usize, leaves: &mut Vec<CLeaf>) {
        for (field, offset) in self.fields_with_offsets() {
            field.kind.push_leaves(base + offset, leaves);
        }
    }

    /// The float type and count of the struct when it is a homogeneous
    /// floating-point aggregate of AArch64: every scalar in it is the same
    /// float type, and it holds one to four of them, counted by size so
    /// that the overlapping members of a union count once.
    fn homogeneous_floats(&self) -> Option<(FfiScalar, usize)> {
        let leaves = self.leaves();
        let first = leaves.first()?.scalar;
        if !matches!(first, FfiScalar::F32 | FfiScalar::F64)
            || leaves.iter().any(|leaf| leaf.scalar != first)
        {
            return None;
        }
        let count = self.size() / first.size();
        (1..=4).contains(&count).then_some((first, count))
    }

    /// How `abi` passes the struct by value.
    pub fn passing(&self, abi: CAbi) -> CPassing {
        let size = self.size();
        let leaves = self.leaves();
        let is_float = |leaf: &CLeaf| matches!(leaf.scalar, FfiScalar::F32 | FfiScalar::F64);
        if let (CAbi::Aapcs64, Some((scalar, count))) = (abi, self.homogeneous_floats()) {
            return CPassing::Registers(
                (0..count)
                    .map(|index| CPiece::Float {
                        offset: index * scalar.size(),
                        scalar,
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
