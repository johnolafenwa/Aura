//! Backend-neutral execution support for Aura's deliberately small C ABI.
//!
//! The public surface only represents the ABI forms admitted by FFI v0.
//! There is intentionally no representation for callbacks, variadic calls, or
//! raw-pointer arithmetic. String and byte-vector arguments are borrowed only
//! for the duration of a synchronous call and lower to `(pointer, length)`.
//!
//! A foreign declaration cannot be checked against a process symbol at
//! runtime. Consequently, the two call entry points are unsafe: the caller must
//! ensure that the symbol's true C signature exactly matches [`FfiSignature`],
//! that pointer arguments are not retained, and that foreign code neither
//! unwinds nor writes outside a mutable byte view. Marshal, lookup, and
//! representable return-value failures are still reported as [`FfiError`]
//! rather than panicking.

use libffi::middle::{Arg, Cif, CodePtr, Ret, Type};

use crate::c_layout::{CFieldKind, CStruct};
use std::error::Error;
use std::ffi::{c_void, CString};
use std::fmt;
use std::path::{Path, PathBuf};
use std::ptr::NonNull;

/// A source-level value's representation at Aura's C ABI boundary.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FfiType {
    Unit,
    Bool,
    I8,
    I16,
    I32,
    I64,
    U8,
    U16,
    U32,
    U64,
    F32,
    F64,
    /// UTF-8 bytes borrowed as `(const uint8_t *, size_t)`.
    StringView,
    /// Bytes borrowed as `(const uint8_t *, size_t)`.
    BytesView,
    /// Bytes borrowed as `(uint8_t *, size_t)` with fixed-length writeback.
    BytesViewMut,
    /// A non-null, foreign-owned opaque pointer.
    OpaqueHandle,
    /// An opaque pointer that may be null. Null is `None` (ADR-0052 A10).
    NullableOpaqueHandle,
    /// NUL-terminated UTF-8 text, `const char *`. An argument is copied
    /// with its terminator for the call. A result is copied into a string
    /// and stays the foreign code's to free.
    CString,
    /// A [`FfiType::CString`] that may be null. Null is `None`.
    NullableCString,
    /// A pointer to one scalar that the foreign function may read and
    /// write, such as `int32_t *`.
    Out(FfiScalar),
    /// A pointer to an opaque pointer that the foreign function may read
    /// and write, such as `sqlite3 **`. A null pointee is `None`.
    OpaqueHandleOut,
    /// A pointer to the first of a run of scalars the foreign function
    /// reads, such as `const double *`. No length crosses with it.
    Pointer(FfiScalar),
    /// A [`FfiType::Pointer`] the foreign function may also write through.
    PointerMut(FfiScalar),
    /// A C struct passed or returned by value.
    Struct(&'static CStruct),
    /// A pointer to a C struct the foreign function may read and write.
    StructPointer(&'static CStruct),
}

/// A scalar a foreign function reads and writes through a pointer.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum FfiScalar {
    Bool,
    I8,
    I16,
    I32,
    I64,
    U8,
    U16,
    U32,
    U64,
    F32,
    F64,
}

impl FfiScalar {
    /// The by-value boundary type of the scalar.
    pub fn ffi_type(self) -> FfiType {
        match self {
            Self::Bool => FfiType::Bool,
            Self::I8 => FfiType::I8,
            Self::I16 => FfiType::I16,
            Self::I32 => FfiType::I32,
            Self::I64 => FfiType::I64,
            Self::U8 => FfiType::U8,
            Self::U16 => FfiType::U16,
            Self::U32 => FfiType::U32,
            Self::U64 => FfiType::U64,
            Self::F32 => FfiType::F32,
            Self::F64 => FfiType::F64,
        }
    }

    /// The scalar a by-value boundary type passes, if it passes one.
    pub fn of(ffi_type: FfiType) -> Option<Self> {
        Some(match ffi_type {
            FfiType::Bool => Self::Bool,
            FfiType::I8 => Self::I8,
            FfiType::I16 => Self::I16,
            FfiType::I32 => Self::I32,
            FfiType::I64 => Self::I64,
            FfiType::U8 => Self::U8,
            FfiType::U16 => Self::U16,
            FfiType::U32 => Self::U32,
            FfiType::U64 => Self::U64,
            FfiType::F32 => Self::F32,
            FfiType::F64 => Self::F64,
            _ => return None,
        })
    }

    /// The size of the scalar in bytes.
    pub fn size(self) -> usize {
        match self {
            Self::Bool | Self::I8 | Self::U8 => 1,
            Self::I16 | Self::U16 => 2,
            Self::I32 | Self::U32 | Self::F32 => 4,
            Self::I64 | Self::U64 | Self::F64 => 8,
        }
    }

    /// The bits of `value`, a scalar of this type, in the low bytes of a
    /// word, or `None` when `value` is another type.
    pub fn word(self, value: &FfiValue) -> Option<u64> {
        Some(match (self, value) {
            (Self::Bool, FfiValue::Bool(value)) => u64::from(*value),
            (Self::I8, FfiValue::I8(value)) => u64::from(*value as u8),
            (Self::I16, FfiValue::I16(value)) => u64::from(*value as u16),
            (Self::I32, FfiValue::I32(value)) => u64::from(*value as u32),
            (Self::I64, FfiValue::I64(value)) => *value as u64,
            (Self::U8, FfiValue::U8(value)) => u64::from(*value),
            (Self::U16, FfiValue::U16(value)) => u64::from(*value),
            (Self::U32, FfiValue::U32(value)) => u64::from(*value),
            (Self::U64, FfiValue::U64(value)) => *value,
            (Self::F32, FfiValue::F32(value)) => u64::from(value.to_bits()),
            (Self::F64, FfiValue::F64(value)) => value.to_bits(),
            _ => return None,
        })
    }

    /// The scalar of this type in the low bytes of `word`.
    pub fn value(self, word: u64) -> Result<FfiValue, FfiError> {
        Ok(match self {
            Self::Bool => match word as u8 {
                0 => FfiValue::Bool(false),
                1 => FfiValue::Bool(true),
                byte => return Err(FfiError::NonCanonicalBoolReturn(byte)),
            },
            Self::I8 => FfiValue::I8(word as i8),
            Self::I16 => FfiValue::I16(word as i16),
            Self::I32 => FfiValue::I32(word as i32),
            Self::I64 => FfiValue::I64(word as i64),
            Self::U8 => FfiValue::U8(word as u8),
            Self::U16 => FfiValue::U16(word as u16),
            Self::U32 => FfiValue::U32(word as u32),
            Self::U64 => FfiValue::U64(word),
            Self::F32 => FfiValue::F32(f32::from_bits(word as u32)),
            Self::F64 => FfiValue::F64(f64::from_bits(word)),
        })
    }
}

impl fmt::Display for FfiType {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Out(scalar) => return write!(formatter, "mut {}", scalar.ffi_type()),
            Self::Pointer(scalar) => return write!(formatter, "ptr[{}]", scalar.ffi_type()),
            Self::PointerMut(scalar) => return write!(formatter, "mut ptr[{}]", scalar.ffi_type()),
            Self::Struct(layout) => return write!(formatter, "struct {}", layout.name),
            Self::StructPointer(layout) => return write!(formatter, "mut struct {}", layout.name),
            Self::Unit => "None",
            Self::Bool => "bool",
            Self::I8 => "int8",
            Self::I16 => "int16",
            Self::I32 => "int32",
            Self::I64 => "int64",
            Self::U8 => "uint8",
            Self::U16 => "uint16",
            Self::U32 => "uint32",
            Self::U64 => "uint64",
            Self::F32 => "float32",
            Self::F64 => "float64",
            Self::StringView => "str view",
            Self::BytesView => "list[uint8] view",
            Self::BytesViewMut => "mut list[uint8] view",
            Self::OpaqueHandle => "opaque handle",
            Self::NullableOpaqueHandle => "nullable opaque handle",
            Self::CString => "cstr",
            Self::NullableCString => "nullable cstr",
            Self::OpaqueHandleOut => "mut nullable opaque handle",
        })
    }
}

/// A validated, non-variadic C function signature.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FfiSignature {
    parameters: Vec<FfiType>,
    result: FfiType,
}

impl FfiSignature {
    pub fn new(parameters: Vec<FfiType>, result: FfiType) -> Self {
        Self { parameters, result }
    }

    pub fn parameters(&self) -> &[FfiType] {
        &self.parameters
    }

    pub fn result(&self) -> FfiType {
        self.result
    }
}

/// A non-null pointer whose pointee layout and lifetime are owned by foreign
/// code.
///
/// This wrapper deliberately does not implement `Clone`, `Copy`, or `Drop`.
/// Aura's semantic layer owns the consume/share rules and the foreign
/// destructor contract; this engine only transports the address.
#[derive(Debug, PartialEq, Eq)]
#[repr(transparent)]
pub struct OpaqueHandle(NonNull<c_void>);

impl OpaqueHandle {
    pub fn new(pointer: *mut c_void) -> Option<Self> {
        NonNull::new(pointer).map(Self)
    }

    pub fn as_ptr(&self) -> *mut c_void {
        self.0.as_ptr()
    }
}

/// A backend-independent argument or result at the host-call boundary.
#[derive(Debug, PartialEq)]
pub enum FfiValue {
    Unit,
    Bool(bool),
    I8(i8),
    I16(i16),
    I32(i32),
    I64(i64),
    U8(u8),
    U16(u16),
    U32(u32),
    U64(u64),
    F32(f32),
    F64(f64),
    String(String),
    Bytes(Vec<u8>),
    OpaqueHandle(OpaqueHandle),
    /// The scalars a pointer argument points to, each as
    /// [`FfiScalar::word`] gives it.
    Elements(FfiScalar, Vec<u64>),
    /// The bytes of a C struct, `size` of them, laid out as C lays it out.
    Struct(&'static CStruct, Vec<u8>),
}

impl FfiValue {
    pub fn ffi_type(&self) -> FfiType {
        match self {
            Self::Unit => FfiType::Unit,
            Self::Bool(_) => FfiType::Bool,
            Self::I8(_) => FfiType::I8,
            Self::I16(_) => FfiType::I16,
            Self::I32(_) => FfiType::I32,
            Self::I64(_) => FfiType::I64,
            Self::U8(_) => FfiType::U8,
            Self::U16(_) => FfiType::U16,
            Self::U32(_) => FfiType::U32,
            Self::U64(_) => FfiType::U64,
            Self::F32(_) => FfiType::F32,
            Self::F64(_) => FfiType::F64,
            Self::String(_) => FfiType::StringView,
            Self::Bytes(_) => FfiType::BytesView,
            Self::OpaqueHandle(_) => FfiType::OpaqueHandle,
            Self::Elements(scalar, _) => FfiType::Pointer(*scalar),
            Self::Struct(layout, _) => FfiType::Struct(layout),
        }
    }
}

/// A dynamically resolved host function address.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(transparent)]
pub struct HostFunction(NonNull<c_void>);

impl HostFunction {
    pub fn new(pointer: *mut c_void) -> Option<Self> {
        NonNull::new(pointer).map(Self)
    }

    pub fn as_ptr(self) -> *mut c_void {
        self.0.as_ptr()
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum FfiError {
    InvalidSymbolName,
    SymbolNotFound {
        symbol: String,
        detail: String,
    },
    UnsupportedProcessLookupPlatform,
    LibraryNotFound {
        library: String,
        detail: String,
    },
    ArityMismatch {
        expected: usize,
        actual: usize,
    },
    ArgumentTypeMismatch {
        index: usize,
        expected: FfiType,
        actual: FfiType,
    },
    UnsupportedArgumentType {
        index: usize,
        ffi_type: FfiType,
    },
    UnsupportedReturnType(FfiType),
    NonCanonicalBoolReturn(u8),
    NullOpaqueHandleReturn,
    /// A `cstr` argument holds a NUL byte, which would end the C string
    /// early.
    InteriorNul {
        index: usize,
    },
    NullCStringReturn,
    InvalidUtf8Return,
}

impl fmt::Display for FfiError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidSymbolName => {
                formatter.write_str("FFI symbol names cannot contain a NUL byte")
            }
            Self::SymbolNotFound { symbol, detail } => {
                write!(formatter, "FFI symbol `{symbol}` was not found: {detail}")
            }
            Self::UnsupportedProcessLookupPlatform => formatter
                .write_str("process-global FFI symbol lookup is unavailable on this platform"),
            Self::LibraryNotFound { library, detail } => {
                write!(
                    formatter,
                    "FFI library `{library}` could not be loaded: {detail}"
                )
            }
            Self::ArityMismatch { expected, actual } => write!(
                formatter,
                "FFI call expected {expected} argument(s), but received {actual}"
            ),
            Self::ArgumentTypeMismatch {
                index,
                expected,
                actual,
            } => write!(
                formatter,
                "FFI argument {} expected {expected}, but received {actual}",
                index + 1
            ),
            Self::UnsupportedArgumentType { index, ffi_type } => write!(
                formatter,
                "FFI argument {} cannot use {ffi_type} at the C boundary",
                index + 1
            ),
            Self::UnsupportedReturnType(ffi_type) => {
                write!(formatter, "FFI functions cannot return {ffi_type}")
            }
            Self::NonCanonicalBoolReturn(value) => write!(
                formatter,
                "FFI bool return must be encoded as 0 or 1, but received {value}"
            ),
            Self::NullOpaqueHandleReturn => {
                formatter.write_str("FFI function returned a null opaque handle")
            }
            Self::InteriorNul { index } => write!(
                formatter,
                "FFI argument {} is a cstr that contains a NUL byte",
                index + 1
            ),
            Self::NullCStringReturn => formatter.write_str("FFI function returned a null cstr"),
            Self::InvalidUtf8Return => {
                formatter.write_str("FFI function returned a cstr that is not UTF-8")
            }
        }
    }
}

impl Error for FfiError {}

enum AbiArgument {
    U8(u8),
    I8(i8),
    U16(u16),
    I16(i16),
    U32(u32),
    I32(i32),
    U64(u64),
    I64(i64),
    F32(f32),
    F64(f64),
    ConstPointer(*const u8),
    MutPointer(*mut u8),
    OpaquePointer(*mut c_void),
    Length(usize),
    /// A struct passed by value: the address of its bytes, and how many
    /// words hold them.
    Struct(*const u64, usize),
}

impl AbiArgument {
    fn as_arg(&self) -> Arg<'_> {
        match self {
            Self::U8(value) => Arg::new(value),
            Self::I8(value) => Arg::new(value),
            Self::U16(value) => Arg::new(value),
            Self::I16(value) => Arg::new(value),
            Self::U32(value) => Arg::new(value),
            Self::I32(value) => Arg::new(value),
            Self::U64(value) => Arg::new(value),
            Self::I64(value) => Arg::new(value),
            Self::F32(value) => Arg::new(value),
            Self::F64(value) => Arg::new(value),
            Self::ConstPointer(value) => Arg::new(value),
            Self::MutPointer(value) => Arg::new(value),
            Self::OpaquePointer(value) => Arg::new(value),
            Self::Length(value) => Arg::new(value),
            // SAFETY: the words are in the call's storage, which outlives
            // every `Arg`.
            Self::Struct(words, count) => {
                Arg::new(unsafe { std::slice::from_raw_parts(*words, *count) })
            }
        }
    }
}

/// What a call's pointer arguments point to, kept alive until the call
/// returns: the NUL-terminated copies of `cstr` arguments, and one word for
/// each out-parameter with the index of its argument.
#[derive(Default)]
struct CallStorage {
    strings: Vec<CString>,
    slots: Vec<(usize, Box<u64>)>,
    /// The packed elements of each pointer argument, in storage aligned for
    /// every scalar, with the index of the argument.
    elements: Vec<(usize, Vec<u64>)>,
    /// The bytes of each struct argument, in storage aligned for every
    /// scalar, with the index of the argument.
    structs: Vec<(usize, Vec<u64>)>,
}

/// `bytes` in storage aligned for every scalar, with room for at least
/// `least` words.
fn aligned_bytes(bytes: &[u8], least: usize) -> Vec<u64> {
    let mut words = vec![0_u64; bytes.len().div_ceil(8).max(least)];
    // SAFETY: the storage holds at least `bytes.len()` bytes.
    unsafe {
        std::ptr::copy_nonoverlapping(bytes.as_ptr(), words.as_mut_ptr().cast(), bytes.len());
    }
    words
}

/// The first `count` bytes of `words`.
fn leading_bytes(words: &[u64], count: usize) -> Vec<u8> {
    // SAFETY: every caller made `words` with room for `count` bytes.
    unsafe { std::slice::from_raw_parts(words.as_ptr().cast::<u8>(), count) }.to_vec()
}

/// The libffi description of a C struct.
fn struct_type(layout: &CStruct) -> Type {
    Type::structure(
        layout
            .fields
            .iter()
            .map(|field| match &field.kind {
                CFieldKind::Scalar(scalar) => scalar_type(*scalar),
                CFieldKind::Struct(inner) => struct_type(inner),
            })
            .collect::<Vec<_>>(),
    )
}

fn scalar_type(scalar: FfiScalar) -> Type {
    match scalar {
        FfiScalar::Bool | FfiScalar::U8 => Type::u8(),
        FfiScalar::I8 => Type::i8(),
        FfiScalar::I16 => Type::i16(),
        FfiScalar::U16 => Type::u16(),
        FfiScalar::I32 => Type::i32(),
        FfiScalar::U32 => Type::u32(),
        FfiScalar::I64 => Type::i64(),
        FfiScalar::U64 => Type::u64(),
        FfiScalar::F32 => Type::f32(),
        FfiScalar::F64 => Type::f64(),
    }
}

/// `words`, one scalar each, packed as C lays out an array of `scalar`.
fn pack_elements(scalar: FfiScalar, words: &[u64]) -> Vec<u64> {
    let size = scalar.size();
    let mut packed = vec![0_u64; (words.len() * size).div_ceil(8)];
    // SAFETY: the storage holds at least `words.len() * size` bytes, and a
    // byte has no alignment.
    let bytes = unsafe {
        std::slice::from_raw_parts_mut(packed.as_mut_ptr().cast::<u8>(), words.len() * size)
    };
    for (chunk, word) in bytes.chunks_exact_mut(size).zip(words) {
        chunk.copy_from_slice(&word.to_le_bytes()[..size]);
    }
    packed
}

/// The first `count` scalars of `packed`, one word each: the inverse of
/// [`pack_elements`].
fn unpack_elements(scalar: FfiScalar, packed: &[u64], count: usize) -> Vec<u64> {
    let size = scalar.size();
    // SAFETY: `packed` was made by `pack_elements` for `count` scalars.
    let bytes = unsafe { std::slice::from_raw_parts(packed.as_ptr().cast::<u8>(), count * size) };
    bytes
        .chunks_exact(size)
        .map(|chunk| {
            let mut word = [0_u8; 8];
            word[..size].copy_from_slice(chunk);
            u64::from_le_bytes(word)
        })
        .collect()
}

fn type_accepts_value(expected: FfiType, value: &FfiValue) -> bool {
    match (expected, value) {
        (FfiType::BytesView | FfiType::BytesViewMut, FfiValue::Bytes(_)) => true,
        (
            FfiType::Pointer(expected) | FfiType::PointerMut(expected),
            FfiValue::Elements(actual, _),
        ) => expected == *actual,
        (
            FfiType::Struct(expected) | FfiType::StructPointer(expected),
            FfiValue::Struct(actual, bytes),
        ) => expected == *actual && bytes.len() == expected.size(),
        (FfiType::CString | FfiType::NullableCString, FfiValue::String(_)) => true,
        (FfiType::NullableCString | FfiType::NullableOpaqueHandle, FfiValue::Unit) => true,
        (FfiType::NullableOpaqueHandle, FfiValue::OpaqueHandle(_)) => true,
        (FfiType::OpaqueHandleOut, FfiValue::OpaqueHandle(_) | FfiValue::Unit) => true,
        (FfiType::Out(scalar), value) => scalar.word(value).is_some(),
        (expected, value) => expected == value.ffi_type(),
    }
}

fn append_argument(
    index: usize,
    expected: FfiType,
    value: &mut FfiValue,
    mutable_bytes: Option<&mut Vec<u8>>,
    storage: &mut CallStorage,
    types: &mut Vec<Type>,
    values: &mut Vec<AbiArgument>,
) -> Result<(), FfiError> {
    if !type_accepts_value(expected, value) {
        return Err(FfiError::ArgumentTypeMismatch {
            index,
            expected,
            actual: value.ffi_type(),
        });
    }

    match (expected, value) {
        (FfiType::Bool, FfiValue::Bool(value)) => {
            types.push(Type::u8());
            values.push(AbiArgument::U8(u8::from(*value)));
        }
        (FfiType::I8, FfiValue::I8(value)) => {
            types.push(Type::i8());
            values.push(AbiArgument::I8(*value));
        }
        (FfiType::I16, FfiValue::I16(value)) => {
            types.push(Type::i16());
            values.push(AbiArgument::I16(*value));
        }
        (FfiType::I32, FfiValue::I32(value)) => {
            types.push(Type::i32());
            values.push(AbiArgument::I32(*value));
        }
        (FfiType::I64, FfiValue::I64(value)) => {
            types.push(Type::i64());
            values.push(AbiArgument::I64(*value));
        }
        (FfiType::U8, FfiValue::U8(value)) => {
            types.push(Type::u8());
            values.push(AbiArgument::U8(*value));
        }
        (FfiType::U16, FfiValue::U16(value)) => {
            types.push(Type::u16());
            values.push(AbiArgument::U16(*value));
        }
        (FfiType::U32, FfiValue::U32(value)) => {
            types.push(Type::u32());
            values.push(AbiArgument::U32(*value));
        }
        (FfiType::U64, FfiValue::U64(value)) => {
            types.push(Type::u64());
            values.push(AbiArgument::U64(*value));
        }
        (FfiType::F32, FfiValue::F32(value)) => {
            types.push(Type::f32());
            values.push(AbiArgument::F32(*value));
        }
        (FfiType::F64, FfiValue::F64(value)) => {
            types.push(Type::f64());
            values.push(AbiArgument::F64(*value));
        }
        (FfiType::StringView, FfiValue::String(value)) => {
            let pointer = if value.is_empty() {
                std::ptr::null()
            } else {
                value.as_ptr()
            };
            types.extend([Type::pointer(), Type::usize()]);
            values.extend([
                AbiArgument::ConstPointer(pointer),
                AbiArgument::Length(value.len()),
            ]);
        }
        (FfiType::BytesView, FfiValue::Bytes(value)) => {
            let pointer = if value.is_empty() {
                std::ptr::null()
            } else {
                value.as_ptr()
            };
            types.extend([Type::pointer(), Type::usize()]);
            values.extend([
                AbiArgument::ConstPointer(pointer),
                AbiArgument::Length(value.len()),
            ]);
        }
        (FfiType::BytesViewMut, FfiValue::Bytes(_)) => {
            let value = mutable_bytes.expect("mutable byte view scratch buffer exists");
            let pointer = if value.is_empty() {
                std::ptr::null_mut()
            } else {
                value.as_mut_ptr()
            };
            types.extend([Type::pointer(), Type::usize()]);
            values.extend([
                AbiArgument::MutPointer(pointer),
                AbiArgument::Length(value.len()),
            ]);
        }
        (FfiType::OpaqueHandle | FfiType::NullableOpaqueHandle, FfiValue::OpaqueHandle(value)) => {
            types.push(Type::pointer());
            values.push(AbiArgument::OpaquePointer(value.as_ptr()));
        }
        (FfiType::NullableOpaqueHandle | FfiType::NullableCString, FfiValue::Unit) => {
            types.push(Type::pointer());
            values.push(AbiArgument::OpaquePointer(std::ptr::null_mut()));
        }
        (FfiType::CString | FfiType::NullableCString, FfiValue::String(value)) => {
            let text =
                CString::new(value.as_bytes()).map_err(|_| FfiError::InteriorNul { index })?;
            types.push(Type::pointer());
            values.push(AbiArgument::ConstPointer(text.as_ptr().cast()));
            // Moving the `CString` keeps its heap buffer where it is.
            storage.strings.push(text);
        }
        (FfiType::Out(scalar), value) => {
            let word = scalar.word(value).expect("the scalar was checked above");
            let mut slot = Box::new(word.to_le());
            types.push(Type::pointer());
            values.push(AbiArgument::MutPointer((&raw mut *slot).cast()));
            storage.slots.push((index, slot));
        }
        (FfiType::Pointer(scalar) | FfiType::PointerMut(scalar), FfiValue::Elements(_, words)) => {
            let mut packed = pack_elements(scalar, words);
            // An empty run passes a null pointer, as an empty view does.
            let pointer = if words.is_empty() {
                std::ptr::null_mut()
            } else {
                packed.as_mut_ptr().cast()
            };
            types.push(Type::pointer());
            values.push(AbiArgument::MutPointer(pointer));
            // Moving the `Vec` keeps its heap buffer where it is.
            storage.elements.push((index, packed));
        }
        (FfiType::Struct(layout), FfiValue::Struct(_, bytes)) => {
            let words = aligned_bytes(bytes, 1);
            types.push(struct_type(layout));
            // Moving the `Vec` keeps its heap buffer where it is.
            values.push(AbiArgument::Struct(words.as_ptr(), words.len()));
            storage.structs.push((index, words));
        }
        (FfiType::StructPointer(_), FfiValue::Struct(_, bytes)) => {
            let mut words = aligned_bytes(bytes, 1);
            types.push(Type::pointer());
            values.push(AbiArgument::MutPointer(words.as_mut_ptr().cast()));
            storage.structs.push((index, words));
        }
        (FfiType::OpaqueHandleOut, value) => {
            let address = match value {
                FfiValue::OpaqueHandle(handle) => handle.as_ptr() as usize as u64,
                _ => 0,
            };
            let mut slot = Box::new(address);
            types.push(Type::pointer());
            values.push(AbiArgument::MutPointer((&raw mut *slot).cast()));
            storage.slots.push((index, slot));
        }
        (ffi_type, _) => {
            return Err(FfiError::UnsupportedArgumentType { index, ffi_type });
        }
    }
    Ok(())
}

fn result_type(ffi_type: FfiType) -> Result<Type, FfiError> {
    Ok(match ffi_type {
        FfiType::Unit => Type::void(),
        FfiType::Bool | FfiType::U8 => Type::u8(),
        FfiType::I8 => Type::i8(),
        FfiType::U16 => Type::u16(),
        FfiType::I16 => Type::i16(),
        FfiType::U32 => Type::u32(),
        FfiType::I32 => Type::i32(),
        FfiType::U64 => Type::u64(),
        FfiType::I64 => Type::i64(),
        FfiType::F32 => Type::f32(),
        FfiType::F64 => Type::f64(),
        FfiType::OpaqueHandle
        | FfiType::NullableOpaqueHandle
        | FfiType::CString
        | FfiType::NullableCString => Type::pointer(),
        FfiType::Struct(layout) => struct_type(layout),
        FfiType::StringView
        | FfiType::BytesView
        | FfiType::BytesViewMut
        | FfiType::Out(_)
        | FfiType::OpaqueHandleOut
        | FfiType::Pointer(_)
        | FfiType::PointerMut(_)
        | FfiType::StructPointer(_) => {
            return Err(FfiError::UnsupportedReturnType(ffi_type));
        }
    })
}

unsafe fn invoke(
    cif: &Cif,
    function: HostFunction,
    arguments: &[Arg<'_>],
    result: FfiType,
) -> Result<FfiValue, FfiError> {
    let function = CodePtr::from_ptr(function.as_ptr());
    // SAFETY: upheld by `call_host_function`: the actual C signature must
    // exactly match the CIF and each Arg points into `abi_values`, which stays
    // alive and immobile until the synchronous call returns.
    Ok(match result {
        FfiType::Unit => {
            unsafe { cif.call::<()>(function, arguments) };
            FfiValue::Unit
        }
        FfiType::Bool => {
            let value = unsafe { cif.call::<u8>(function, arguments) };
            match value {
                0 => FfiValue::Bool(false),
                1 => FfiValue::Bool(true),
                value => return Err(FfiError::NonCanonicalBoolReturn(value)),
            }
        }
        FfiType::I8 => FfiValue::I8(unsafe { cif.call(function, arguments) }),
        FfiType::I16 => FfiValue::I16(unsafe { cif.call(function, arguments) }),
        FfiType::I32 => FfiValue::I32(unsafe { cif.call(function, arguments) }),
        FfiType::I64 => FfiValue::I64(unsafe { cif.call(function, arguments) }),
        FfiType::U8 => FfiValue::U8(unsafe { cif.call(function, arguments) }),
        FfiType::U16 => FfiValue::U16(unsafe { cif.call(function, arguments) }),
        FfiType::U32 => FfiValue::U32(unsafe { cif.call(function, arguments) }),
        FfiType::U64 => FfiValue::U64(unsafe { cif.call(function, arguments) }),
        FfiType::F32 => FfiValue::F32(unsafe { cif.call(function, arguments) }),
        FfiType::F64 => FfiValue::F64(unsafe { cif.call(function, arguments) }),
        FfiType::OpaqueHandle => {
            let pointer: *mut c_void = unsafe { cif.call(function, arguments) };
            FfiValue::OpaqueHandle(
                OpaqueHandle::new(pointer).ok_or(FfiError::NullOpaqueHandleReturn)?,
            )
        }
        FfiType::NullableOpaqueHandle => {
            let pointer: *mut c_void = unsafe { cif.call(function, arguments) };
            match OpaqueHandle::new(pointer) {
                Some(handle) => FfiValue::OpaqueHandle(handle),
                None => FfiValue::Unit,
            }
        }
        FfiType::CString | FfiType::NullableCString => {
            let pointer: *const std::ffi::c_char = unsafe { cif.call(function, arguments) };
            if pointer.is_null() {
                return match result {
                    FfiType::NullableCString => Ok(FfiValue::Unit),
                    _ => Err(FfiError::NullCStringReturn),
                };
            }
            // SAFETY: the declaration promises NUL-terminated text that
            // stays valid at least until the caller copies it here.
            let text = unsafe { std::ffi::CStr::from_ptr(pointer) };
            let text = text.to_str().map_err(|_| FfiError::InvalidUtf8Return)?;
            FfiValue::String(text.to_owned())
        }
        FfiType::Struct(layout) => {
            // libffi widens a small result to a whole register.
            let mut words = aligned_bytes(&[], layout.size().div_ceil(8).max(2));
            unsafe {
                cif.call_return_into(function, arguments, Ret::new(words.as_mut_slice()));
            }
            FfiValue::Struct(layout, leading_bytes(&words, layout.size()))
        }
        // `result_type` refused these before the call was built.
        _ => return Err(FfiError::UnsupportedReturnType(result)),
    })
}

/// Calls a previously resolved non-variadic C function.
///
/// Mutable byte arguments use a same-length scratch allocation. Their contents
/// are copied in before the call and copied back into the original `Vec` after
/// the call, including when return-value validation fails. The length is passed
/// by value, so neither this layer nor the foreign function can change the
/// vector's length through the v0 ABI.
///
/// # Safety
///
/// `function` must remain callable for the duration of this function and its
/// true C signature must exactly match `signature`. Foreign code must not retain
/// a borrowed pointer, access past its supplied length, unwind across the ABI
/// boundary, or concurrently access any argument storage.
pub unsafe fn call_host_function(
    function: HostFunction,
    signature: &FfiSignature,
    arguments: &mut [FfiValue],
) -> Result<FfiValue, FfiError> {
    if signature.parameters.len() != arguments.len() {
        return Err(FfiError::ArityMismatch {
            expected: signature.parameters.len(),
            actual: arguments.len(),
        });
    }

    // Reject an unsupported return before resolving or invoking the symbol.
    let result_type = result_type(signature.result)?;
    let mut mutable_byte_buffers: Vec<Option<Vec<u8>>> = signature
        .parameters
        .iter()
        .copied()
        .zip(arguments.iter())
        .map(|(ffi_type, value)| match (ffi_type, value) {
            (FfiType::BytesViewMut, FfiValue::Bytes(bytes)) => Some(bytes.clone()),
            _ => None,
        })
        .collect();
    let mut storage = CallStorage::default();
    let mut abi_types = Vec::new();
    let mut abi_values = Vec::new();
    for (index, (expected, value)) in signature
        .parameters
        .iter()
        .copied()
        .zip(arguments.iter_mut())
        .enumerate()
    {
        append_argument(
            index,
            expected,
            value,
            mutable_byte_buffers[index].as_mut(),
            &mut storage,
            &mut abi_types,
            &mut abi_values,
        )?;
    }
    let cif = Cif::new(abi_types, result_type);
    let abi_arguments: Vec<_> = abi_values.iter().map(AbiArgument::as_arg).collect();

    // SAFETY: the remaining requirements are delegated to this function's
    // caller; the engine itself constructed matching libffi types/arguments.
    let result = unsafe { invoke(&cif, function, &abi_arguments, signature.result) };
    for (index, buffer) in mutable_byte_buffers.into_iter().enumerate() {
        if let Some(buffer) = buffer {
            let FfiValue::Bytes(destination) = &mut arguments[index] else {
                unreachable!("mutable byte view was validated before invocation");
            };
            destination.copy_from_slice(&buffer);
        }
    }
    // A struct passed by pointer takes the bytes the function left.
    for (index, words) in storage.structs {
        if let (FfiType::StructPointer(_), FfiValue::Struct(_, bytes)) =
            (signature.parameters[index], &mut arguments[index])
        {
            *bytes = leading_bytes(&words, bytes.len());
        }
    }
    // A `mut` pointer argument takes the scalars the function left.
    for (index, packed) in storage.elements {
        if let (FfiType::PointerMut(scalar), FfiValue::Elements(_, words)) =
            (signature.parameters[index], &mut arguments[index])
        {
            *words = unpack_elements(scalar, &packed, words.len());
        }
    }
    // Each out-parameter takes what the function left in its word, even
    // when the result is refused. A word that is not a value of its type
    // is reported unless the result already failed.
    let mut written = Ok(());
    for (index, slot) in storage.slots {
        let value = match signature.parameters[index] {
            FfiType::Out(scalar) => scalar.value(u64::from_le(*slot)),
            _ => Ok(match OpaqueHandle::new(*slot as usize as *mut c_void) {
                Some(handle) => FfiValue::OpaqueHandle(handle),
                None => FfiValue::Unit,
            }),
        };
        match value {
            Ok(value) => arguments[index] = value,
            Err(error) => written = written.and(Err(error)),
        }
    }
    let result = result?;
    written.map(|()| result)
}

#[cfg(unix)]
fn process_symbol(symbol: &str) -> Result<HostFunction, FfiError> {
    let c_symbol = CString::new(symbol).map_err(|_| FfiError::InvalidSymbolName)?;
    // SAFETY: `c_symbol` is NUL terminated. Clearing dlerror before dlsym lets
    // us distinguish a stale loader error from this lookup.
    let pointer = unsafe {
        libc::dlerror();
        libc::dlsym(libc::RTLD_DEFAULT, c_symbol.as_ptr())
    };
    HostFunction::new(pointer).ok_or_else(|| {
        // SAFETY: dlerror returns either null or a loader-owned NUL-terminated
        // string that remains valid until the next loader call on this thread.
        let detail = unsafe {
            let error = libc::dlerror();
            if error.is_null() {
                "dynamic loader returned a null address".to_owned()
            } else {
                std::ffi::CStr::from_ptr(error)
                    .to_string_lossy()
                    .into_owned()
            }
        };
        FfiError::SymbolNotFound {
            symbol: symbol.to_owned(),
            detail,
        }
    })
}

#[cfg(not(unix))]
fn process_symbol(symbol: &str) -> Result<HostFunction, FfiError> {
    if CString::new(symbol).is_err() {
        Err(FfiError::InvalidSymbolName)
    } else {
        Err(FfiError::UnsupportedProcessLookupPlatform)
    }
}

/// The file names a library called `name` may have on this host, most
/// specific first.
fn library_file_names(name: &str) -> Vec<String> {
    if cfg!(target_os = "macos") {
        vec![format!("lib{name}.dylib"), format!("lib{name}.so")]
    } else {
        vec![format!("lib{name}.so")]
    }
}

/// The directories the linker searches for a library by default, where a
/// library may sit only under a versioned name such as `libsqlite3.so.0`.
#[cfg(unix)]
fn system_library_directories() -> Vec<PathBuf> {
    let arch = std::env::consts::ARCH;
    let mut directories = std::env::var_os("LD_LIBRARY_PATH")
        .map(|paths| std::env::split_paths(&paths).collect::<Vec<_>>())
        .unwrap_or_default();
    directories.push(PathBuf::from("/usr/local/lib"));
    directories.push(PathBuf::from(format!("/usr/lib/{arch}-linux-gnu")));
    directories.push(PathBuf::from(format!("/lib/{arch}-linux-gnu")));
    directories.extend(["/usr/lib64", "/lib64", "/usr/lib", "/lib"].map(PathBuf::from));
    directories
}

/// The files in `directory` that are versions of `file_name`, such as
/// `libsqlite3.so.0` for `libsqlite3.so`, in name order.
#[cfg(unix)]
fn versioned_library_files(directory: &Path, file_name: &str) -> Vec<PathBuf> {
    let prefix = format!("{file_name}.");
    let mut files = std::fs::read_dir(directory)
        .map(|entries| {
            entries
                .filter_map(|entry| entry.ok())
                .filter(|entry| entry.file_name().to_string_lossy().starts_with(&prefix))
                .map(|entry| entry.path())
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    files.sort();
    files
}

#[cfg(unix)]
fn open_library(path: &Path) -> Result<(), String> {
    use std::os::unix::ffi::OsStrExt;
    let c_path = CString::new(path.as_os_str().as_bytes())
        .map_err(|_| "the path contains a NUL byte".to_string())?;
    // SAFETY: `c_path` is NUL terminated. The handle is never closed, so the
    // library stays loaded for the life of the process.
    let handle = unsafe {
        libc::dlerror();
        libc::dlopen(c_path.as_ptr(), libc::RTLD_NOW | libc::RTLD_GLOBAL)
    };
    if !handle.is_null() {
        return Ok(());
    }
    // SAFETY: dlerror returns null or a loader-owned NUL-terminated string
    // that stays valid until this thread's next loader call.
    let reason = unsafe {
        let error = libc::dlerror();
        (!error.is_null()).then(|| {
            std::ffi::CStr::from_ptr(error)
                .to_string_lossy()
                .into_owned()
        })
    };
    Err(reason.unwrap_or_else(|| "the dynamic loader gave no reason".to_owned()))
}

/// The files to try for the library `name`, in order: each of
/// `search_paths`, then the loader's own search, then a versioned file in
/// the system's library directories.
#[cfg(unix)]
fn library_candidates(name: &str, search_paths: &[PathBuf]) -> Vec<PathBuf> {
    let file_names = library_file_names(name);
    let mut candidates = Vec::new();
    for directory in search_paths {
        for file_name in &file_names {
            candidates.push(directory.join(file_name));
            candidates.extend(versioned_library_files(directory, file_name));
        }
    }
    candidates.extend(file_names.iter().map(PathBuf::from));
    for directory in system_library_directories() {
        for file_name in &file_names {
            candidates.extend(versioned_library_files(&directory, file_name));
        }
    }
    candidates
}

/// The files to try for the macOS framework `name`.
#[cfg(unix)]
fn framework_candidates(name: &str) -> Vec<PathBuf> {
    ["/System/Library/Frameworks", "/Library/Frameworks"]
        .map(|root| PathBuf::from(format!("{root}/{name}.framework/{name}")))
        .to_vec()
}

/// Loads the first of `candidates` the loader accepts into the process, so
/// [`call_process_symbol`] finds its symbols. The failure names `library`
/// and gives the loader's reason for the first candidate.
#[cfg(unix)]
fn load_first(library: &str, candidates: &[PathBuf]) -> Result<(), FfiError> {
    let mut first_failure = None;
    for candidate in candidates {
        match open_library(candidate) {
            Ok(()) => return Ok(()),
            Err(detail) => first_failure = first_failure.or(Some(detail)),
        }
    }
    Err(FfiError::LibraryNotFound {
        library: library.to_owned(),
        detail: first_failure.unwrap_or_else(|| "there is no file to try".to_owned()),
    })
}

/// Loads every library and framework a program links into this process,
/// once each, so the symbols resolve as they do in a built program. The
/// libraries stay loaded. Frameworks exist only on macOS and are skipped
/// elsewhere.
#[cfg(unix)]
pub fn load_libraries(
    libraries: &[String],
    search_paths: &[PathBuf],
    frameworks: &[String],
) -> Result<(), FfiError> {
    static LOADED: std::sync::Mutex<std::collections::BTreeSet<String>> =
        std::sync::Mutex::new(std::collections::BTreeSet::new());
    let frameworks = frameworks.iter().filter(|_| cfg!(target_os = "macos"));
    let wanted = libraries
        .iter()
        .map(|name| {
            (
                format!("l{name}"),
                name,
                library_candidates(name, search_paths),
            )
        })
        .chain(frameworks.map(|name| (format!("f{name}"), name, framework_candidates(name))));
    let mut loaded = LOADED
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    for (key, name, candidates) in wanted {
        if !loaded.contains(&key) {
            load_first(name, &candidates)?;
            loaded.insert(key);
        }
    }
    Ok(())
}

#[cfg(not(unix))]
pub fn load_libraries(
    libraries: &[String],
    _search_paths: &[PathBuf],
    frameworks: &[String],
) -> Result<(), FfiError> {
    match libraries.first().or(frameworks.first()) {
        Some(_) => Err(FfiError::UnsupportedProcessLookupPlatform),
        None => Ok(()),
    }
}

/// Resolves `symbol` from the process-global/system symbol namespace and calls
/// it synchronously.
///
/// # Safety
///
/// The resolved symbol's actual C signature must exactly match `signature`.
/// The foreign implementation must obey all pointer-lifetime, bounds, and
/// no-unwind requirements documented on [`call_host_function`].
pub unsafe fn call_process_symbol(
    symbol: &str,
    signature: &FfiSignature,
    arguments: &mut [FfiValue],
) -> Result<FfiValue, FfiError> {
    let function = process_symbol(symbol)?;
    // SAFETY: delegated to this function's caller.
    unsafe { call_host_function(function, signature, arguments) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::c_void;
    use std::sync::atomic::{AtomicUsize, Ordering};

    static LAST_MUTABLE_BYTES_ADDRESS: AtomicUsize = AtomicUsize::new(0);

    unsafe extern "C" fn scalar_arguments(
        boolean: bool,
        i8_value: i8,
        i16_value: i16,
        i32_value: i32,
        i64_value: i64,
        u8_value: u8,
        u16_value: u16,
        u32_value: u32,
        u64_value: u64,
        f32_value: f32,
        f64_value: f64,
    ) -> f64 {
        if boolean
            && i8_value == -8
            && i16_value == -1_600
            && i32_value == -32_000
            && i64_value == -64_000
            && u8_value == 8
            && u16_value == 1_600
            && u32_value == 32_000
            && u64_value == 64_000
            && f32_value == 3.5
            && f64_value == 7.25
        {
            42.5
        } else {
            -1.0
        }
    }

    unsafe extern "C" fn bool_not(value: bool) -> bool {
        !value
    }

    unsafe extern "C" fn invalid_bool_return() -> u8 {
        2
    }

    unsafe extern "C" fn i8_identity(value: i8) -> i8 {
        value
    }

    unsafe extern "C" fn i16_identity(value: i16) -> i16 {
        value
    }

    unsafe extern "C" fn i32_identity(value: i32) -> i32 {
        value
    }

    unsafe extern "C" fn i64_identity(value: i64) -> i64 {
        value
    }

    unsafe extern "C" fn u8_identity(value: u8) -> u8 {
        value
    }

    unsafe extern "C" fn u16_identity(value: u16) -> u16 {
        value
    }

    unsafe extern "C" fn u32_identity(value: u32) -> u32 {
        value
    }

    unsafe extern "C" fn u64_identity(value: u64) -> u64 {
        value
    }

    unsafe extern "C" fn f32_identity(value: f32) -> f32 {
        value
    }

    unsafe extern "C" fn f64_identity(value: f64) -> f64 {
        value
    }

    unsafe extern "C" fn observe_string(bytes: *const u8, len: usize) -> u64 {
        let bytes = unsafe { std::slice::from_raw_parts(bytes, len) };
        if bytes == "snowman ☃".as_bytes() {
            len as u64
        } else {
            0
        }
    }

    unsafe extern "C" fn sum_bytes(bytes: *const u8, len: usize) -> u64 {
        unsafe { std::slice::from_raw_parts(bytes, len) }
            .iter()
            .map(|byte| u64::from(*byte))
            .sum()
    }

    unsafe extern "C" fn increment_bytes(bytes: *mut u8, len: usize) {
        LAST_MUTABLE_BYTES_ADDRESS.store(bytes as usize, Ordering::Relaxed);
        for byte in unsafe { std::slice::from_raw_parts_mut(bytes, len) } {
            *byte = byte.wrapping_add(1);
        }
    }

    unsafe extern "C" fn observe_empty_views(
        string: *const u8,
        string_len: usize,
        bytes: *const u8,
        bytes_len: usize,
        mutable_bytes: *mut u8,
        mutable_bytes_len: usize,
    ) -> bool {
        string.is_null()
            && string_len == 0
            && bytes.is_null()
            && bytes_len == 0
            && mutable_bytes.is_null()
            && mutable_bytes_len == 0
    }

    unsafe extern "C" fn mutate_then_return_invalid_bool(bytes: *mut u8, len: usize) -> u8 {
        for byte in unsafe { std::slice::from_raw_parts_mut(bytes, len) } {
            *byte = byte.wrapping_add(1);
        }
        2
    }

    unsafe extern "C" fn round_trip_handle(handle: *mut c_void) -> *mut c_void {
        handle
    }

    unsafe extern "C" fn return_null_handle() -> *mut c_void {
        std::ptr::null_mut()
    }

    unsafe extern "C" fn text_length(text: *const std::ffi::c_char) -> i64 {
        if text.is_null() {
            return -1;
        }
        unsafe { std::ffi::CStr::from_ptr(text) }.to_bytes().len() as i64
    }

    unsafe extern "C" fn pick_text(which: i32) -> *const std::ffi::c_char {
        match which {
            0 => std::ptr::null(),
            1 => c"h\u{e9}llo".as_ptr(),
            _ => c"\xff".as_ptr(),
        }
    }

    #[repr(C)]
    #[derive(Clone, Copy)]
    struct Pair {
        x: f64,
        y: f64,
    }

    #[repr(C)]
    #[derive(Clone, Copy)]
    struct Small {
        count: i32,
        flag: i8,
    }

    #[repr(C)]
    #[derive(Clone, Copy)]
    struct Wide {
        tag: i8,
        total: i64,
        ratio: f32,
        inner: Small,
    }

    unsafe extern "C" fn swap_pair(pair: Pair, scale: f64) -> Pair {
        Pair {
            x: pair.y * scale,
            y: pair.x * scale,
        }
    }

    unsafe extern "C" fn small_sum(before: i64, small: Small, after: i64) -> i64 {
        before + i64::from(small.count) + i64::from(small.flag) + after
    }

    unsafe extern "C" fn make_small(count: i32) -> Small {
        Small {
            count,
            flag: -(count as i8),
        }
    }

    unsafe extern "C" fn widen(wide: Wide, more: i64) -> Wide {
        Wide {
            tag: wide.tag + 1,
            total: wide.total + more,
            ratio: wide.ratio * 2.0,
            inner: Small {
                count: wide.inner.count + 1,
                flag: wide.inner.flag + 1,
            },
        }
    }

    unsafe extern "C" fn widen_in_place(wide: *mut Wide) -> i32 {
        unsafe { *wide = widen(*wide, 100) };
        7
    }

    /// The bytes of `value`, with every padding byte zero.
    fn bytes_of<T: Copy>(layout: &CStruct, value: T) -> Vec<u8> {
        assert_eq!(layout.size(), std::mem::size_of::<T>());
        let mut bytes = vec![0_u8; layout.size()];
        for leaf in layout.leaves() {
            let size = leaf.scalar.size();
            // SAFETY: the leaf lies inside `value`.
            let field = unsafe {
                std::slice::from_raw_parts((&raw const value).cast::<u8>().add(leaf.offset), size)
            };
            bytes[leaf.offset..leaf.offset + size].copy_from_slice(field);
        }
        bytes
    }

    /// The struct bytes of `value` with every padding byte zero. C leaves
    /// padding unspecified.
    fn scrubbed(value: Result<FfiValue, FfiError>) -> Vec<u8> {
        let Ok(FfiValue::Struct(layout, bytes)) = value else {
            panic!("expected a struct, found {value:?}");
        };
        let mut scrubbed = vec![0_u8; bytes.len()];
        for leaf in layout.leaves() {
            let field = leaf.offset..leaf.offset + leaf.scalar.size();
            scrubbed[field.clone()].copy_from_slice(&bytes[field]);
        }
        scrubbed
    }

    fn layout_of(name: &str, fields: Vec<(&str, CFieldKind)>) -> &'static CStruct {
        CStruct {
            name: name.to_string(),
            fields: fields
                .into_iter()
                .map(|(name, kind)| crate::c_layout::CField {
                    name: name.to_string(),
                    kind,
                })
                .collect(),
        }
        .interned()
    }

    #[test]
    fn structs_cross_by_value_by_pointer_and_as_results() {
        use CFieldKind::Scalar;
        let pair = layout_of(
            "Pair",
            vec![("x", Scalar(FfiScalar::F64)), ("y", Scalar(FfiScalar::F64))],
        );
        let small = layout_of(
            "Small",
            vec![
                ("count", Scalar(FfiScalar::I32)),
                ("flag", Scalar(FfiScalar::I8)),
            ],
        );
        let wide = layout_of(
            "Wide",
            vec![
                ("tag", Scalar(FfiScalar::I8)),
                ("total", Scalar(FfiScalar::I64)),
                ("ratio", Scalar(FfiScalar::F32)),
                ("inner", CFieldKind::Struct(small.clone())),
            ],
        );
        assert!(
            std::ptr::eq(pair, pair.clone().interned()),
            "equal layouts share one copy"
        );
        assert_eq!(FfiType::Struct(pair).to_string(), "struct Pair");
        assert_eq!(FfiType::StructPointer(wide).to_string(), "mut struct Wide");

        let result = call(
            swap_pair as *const (),
            FfiSignature::new(
                vec![FfiType::Struct(pair), FfiType::F64],
                FfiType::Struct(pair),
            ),
            &mut [
                FfiValue::Struct(pair, bytes_of(pair, Pair { x: 1.5, y: -2.0 })),
                FfiValue::F64(2.0),
            ],
        );
        assert_eq!(
            result,
            Ok(FfiValue::Struct(
                pair,
                bytes_of(pair, Pair { x: -4.0, y: 3.0 })
            ))
        );

        let small_value = || {
            FfiValue::Struct(
                small,
                bytes_of(
                    small,
                    Small {
                        count: 40,
                        flag: -3,
                    },
                ),
            )
        };
        assert_eq!(small_value().ffi_type(), FfiType::Struct(small));
        let result = call(
            small_sum as *const (),
            FfiSignature::new(
                vec![FfiType::I64, FfiType::Struct(small), FfiType::I64],
                FfiType::I64,
            ),
            &mut [FfiValue::I64(1000), small_value(), FfiValue::I64(5)],
        );
        assert_eq!(result, Ok(FfiValue::I64(1042)));
        let result = call(
            make_small as *const (),
            FfiSignature::new(vec![FfiType::I32], FfiType::Struct(small)),
            &mut [FfiValue::I32(9)],
        );
        assert_eq!(
            result,
            Ok(FfiValue::Struct(
                small,
                bytes_of(small, Small { count: 9, flag: -9 })
            ))
        );

        let before = Wide {
            tag: 1,
            total: 1 << 40,
            ratio: 0.25,
            inner: Small { count: 6, flag: 7 },
        };
        let after = |more: i64| Wide {
            tag: 2,
            total: (1 << 40) + more,
            ratio: 0.5,
            inner: Small { count: 7, flag: 8 },
        };
        let result = call(
            widen as *const (),
            FfiSignature::new(
                vec![FfiType::Struct(wide), FfiType::I64],
                FfiType::Struct(wide),
            ),
            &mut [
                FfiValue::Struct(wide, bytes_of(wide, before)),
                FfiValue::I64(3),
            ],
        );
        assert_eq!(scrubbed(result), bytes_of(wide, after(3)));

        // A by-value argument keeps its bytes, and a pointer takes what
        // the function left.
        let mut arguments = [FfiValue::Struct(wide, bytes_of(wide, before))];
        let result = call(
            widen_in_place as *const (),
            FfiSignature::new(vec![FfiType::StructPointer(wide)], FfiType::I32),
            &mut arguments,
        );
        assert_eq!(result, Ok(FfiValue::I32(7)));
        let [written] = arguments;
        assert_eq!(scrubbed(Ok(written)), bytes_of(wide, after(100)));

        let error = call(
            small_sum as *const (),
            FfiSignature::new(
                vec![FfiType::I64, FfiType::Struct(small), FfiType::I64],
                FfiType::I64,
            ),
            &mut [
                FfiValue::I64(0),
                FfiValue::Struct(pair, bytes_of(pair, Pair { x: 0.0, y: 0.0 })),
                FfiValue::I64(0),
            ],
        )
        .expect_err("a struct parameter takes its own struct");
        assert_eq!(
            error,
            FfiError::ArgumentTypeMismatch {
                index: 1,
                expected: FfiType::Struct(small),
                actual: FfiType::Struct(pair),
            }
        );
        let error = call(
            small_sum as *const (),
            FfiSignature::new(
                vec![FfiType::I64, FfiType::Struct(small), FfiType::I64],
                FfiType::I64,
            ),
            &mut [
                FfiValue::I64(0),
                FfiValue::Struct(small, vec![0; 3]),
                FfiValue::I64(0),
            ],
        )
        .expect_err("a struct value holds exactly the struct's bytes");
        assert!(matches!(
            error,
            FfiError::ArgumentTypeMismatch { index: 1, .. }
        ));
        let error = call(
            make_small as *const (),
            FfiSignature::new(Vec::new(), FfiType::StructPointer(small)),
            &mut [],
        )
        .expect_err("a struct pointer is not a result");
        assert_eq!(
            error,
            FfiError::UnsupportedReturnType(FfiType::StructPointer(small))
        );
    }

    /// Reverses the order of `count` elements of `size` bytes each.
    unsafe extern "C" fn reverse_run(values: *mut u8, count: i64, size: i64) {
        let (count, size) = (count as usize, size as usize);
        let bytes = unsafe { std::slice::from_raw_parts_mut(values, count * size) };
        let reversed: Vec<u8> = bytes.chunks_exact(size).rev().flatten().copied().collect();
        bytes.copy_from_slice(&reversed);
    }

    unsafe extern "C" fn run_is_null(values: *const u8) -> bool {
        values.is_null()
    }

    #[allow(clippy::too_many_arguments)]
    unsafe extern "C" fn bump_scalars(
        flag: *mut bool,
        i8_value: *mut i8,
        i16_value: *mut i16,
        i32_value: *mut i32,
        i64_value: *mut i64,
        u8_value: *mut u8,
        u16_value: *mut u16,
        u32_value: *mut u32,
        u64_value: *mut u64,
        f32_value: *mut f32,
        f64_value: *mut f64,
    ) {
        unsafe {
            *flag = !*flag;
            *i8_value -= 1;
            *i16_value -= 1;
            *i32_value -= 1;
            *i64_value -= 1;
            *u8_value += 1;
            *u16_value += 1;
            *u32_value += 1;
            *u64_value += 1;
            *f32_value *= 2.0;
            *f64_value *= 2.0;
        }
    }

    unsafe extern "C" fn write_bad_bool(flag: *mut u8) -> u8 {
        unsafe { *flag = 7 };
        1
    }

    unsafe extern "C" fn write_bad_bool_and_return_one(flag: *mut u8) -> u8 {
        unsafe { *flag = 7 };
        9
    }

    /// Swaps the handle in `slot` for `next` and returns the one it held.
    unsafe extern "C" fn swap_handle(slot: *mut *mut c_void, next: *mut c_void) -> *mut c_void {
        unsafe { std::ptr::replace(slot, next) }
    }

    fn pointer(function: *const ()) -> HostFunction {
        HostFunction::new(function.cast_mut().cast()).expect("test function pointer is non-null")
    }

    fn call(
        function: *const (),
        signature: FfiSignature,
        arguments: &mut [FfiValue],
    ) -> Result<FfiValue, FfiError> {
        // SAFETY: every test-owned pointer has exactly the signature supplied at
        // its call site and remains live for the duration of the call.
        unsafe { call_host_function(pointer(function), &signature, arguments) }
    }

    #[test]
    fn rejected_argument_diagnostics_name_every_ffi_v0_boundary_type() {
        let cases = [
            (FfiType::Bool, "bool"),
            (FfiType::I8, "int8"),
            (FfiType::I16, "int16"),
            (FfiType::I32, "int32"),
            (FfiType::I64, "int64"),
            (FfiType::U8, "uint8"),
            (FfiType::U16, "uint16"),
            (FfiType::U32, "uint32"),
            (FfiType::U64, "uint64"),
            (FfiType::F32, "float32"),
            (FfiType::F64, "float64"),
            (FfiType::StringView, "str view"),
            (FfiType::BytesView, "list[uint8] view"),
            (FfiType::BytesViewMut, "mut list[uint8] view"),
            (FfiType::OpaqueHandle, "opaque handle"),
            (FfiType::CString, "cstr"),
            (FfiType::Out(FfiScalar::I32), "mut int32"),
        ];
        for (ffi_type, source_name) in cases {
            let error = call(
                i32_identity as *const (),
                FfiSignature::new(vec![ffi_type], FfiType::Unit),
                &mut [FfiValue::Unit],
            )
            .expect_err("an Aura None value must not cross a typed C parameter");
            assert_eq!(
                error,
                FfiError::ArgumentTypeMismatch {
                    index: 0,
                    expected: ffi_type,
                    actual: FfiType::Unit,
                }
            );
            assert_eq!(
                error.to_string(),
                format!("FFI argument 1 expected {source_name}, but received None")
            );
        }
    }

    #[test]
    fn marshals_every_fixed_width_scalar_argument() {
        let signature = FfiSignature::new(
            vec![
                FfiType::Bool,
                FfiType::I8,
                FfiType::I16,
                FfiType::I32,
                FfiType::I64,
                FfiType::U8,
                FfiType::U16,
                FfiType::U32,
                FfiType::U64,
                FfiType::F32,
                FfiType::F64,
            ],
            FfiType::F64,
        );
        let mut arguments = vec![
            FfiValue::Bool(true),
            FfiValue::I8(-8),
            FfiValue::I16(-1_600),
            FfiValue::I32(-32_000),
            FfiValue::I64(-64_000),
            FfiValue::U8(8),
            FfiValue::U16(1_600),
            FfiValue::U32(32_000),
            FfiValue::U64(64_000),
            FfiValue::F32(3.5),
            FfiValue::F64(7.25),
        ];

        assert_eq!(
            call(scalar_arguments as *const (), signature, &mut arguments),
            Ok(FfiValue::F64(42.5))
        );
    }

    #[test]
    fn unmarshals_every_fixed_width_scalar_return() {
        let cases: Vec<(*const (), FfiType, FfiValue, FfiValue)> = vec![
            (
                bool_not as *const (),
                FfiType::Bool,
                FfiValue::Bool(true),
                FfiValue::Bool(false),
            ),
            (
                i8_identity as *const (),
                FfiType::I8,
                FfiValue::I8(-8),
                FfiValue::I8(-8),
            ),
            (
                i16_identity as *const (),
                FfiType::I16,
                FfiValue::I16(-1_600),
                FfiValue::I16(-1_600),
            ),
            (
                i32_identity as *const (),
                FfiType::I32,
                FfiValue::I32(-32_000),
                FfiValue::I32(-32_000),
            ),
            (
                i64_identity as *const (),
                FfiType::I64,
                FfiValue::I64(-64_000),
                FfiValue::I64(-64_000),
            ),
            (
                u8_identity as *const (),
                FfiType::U8,
                FfiValue::U8(8),
                FfiValue::U8(8),
            ),
            (
                u16_identity as *const (),
                FfiType::U16,
                FfiValue::U16(1_600),
                FfiValue::U16(1_600),
            ),
            (
                u32_identity as *const (),
                FfiType::U32,
                FfiValue::U32(32_000),
                FfiValue::U32(32_000),
            ),
            (
                u64_identity as *const (),
                FfiType::U64,
                FfiValue::U64(64_000),
                FfiValue::U64(64_000),
            ),
            (
                f32_identity as *const (),
                FfiType::F32,
                FfiValue::F32(3.5),
                FfiValue::F32(3.5),
            ),
            (
                f64_identity as *const (),
                FfiType::F64,
                FfiValue::F64(7.25),
                FfiValue::F64(7.25),
            ),
        ];

        for (function, result_type, argument, expected) in cases {
            assert_eq!(
                call(
                    function,
                    FfiSignature::new(vec![result_type], result_type),
                    &mut [argument]
                ),
                Ok(expected)
            );
        }
    }

    #[test]
    fn passes_utf8_strings_and_byte_vectors_as_pointer_length_views() {
        let mut string = [FfiValue::String("snowman ☃".to_owned())];
        assert_eq!(
            call(
                observe_string as *const (),
                FfiSignature::new(vec![FfiType::StringView], FfiType::U64),
                &mut string
            ),
            Ok(FfiValue::U64("snowman ☃".len() as u64))
        );

        let mut bytes = [FfiValue::Bytes(vec![1, 2, 3, 4])];
        assert_eq!(
            call(
                sum_bytes as *const (),
                FfiSignature::new(vec![FfiType::BytesView], FfiType::U64),
                &mut bytes
            ),
            Ok(FfiValue::U64(10))
        );
    }

    #[test]
    fn mutable_byte_view_writes_back_without_allowing_a_length_change() {
        let mut arguments = [FfiValue::Bytes(vec![0, 1, 254])];
        let original_address = match &arguments[0] {
            FfiValue::Bytes(bytes) => bytes.as_ptr() as usize,
            _ => unreachable!(),
        };
        assert_eq!(
            call(
                increment_bytes as *const (),
                FfiSignature::new(vec![FfiType::BytesViewMut], FfiType::Unit),
                &mut arguments
            ),
            Ok(FfiValue::Unit)
        );
        assert_eq!(arguments, [FfiValue::Bytes(vec![1, 2, 255])]);
        assert_ne!(
            LAST_MUTABLE_BYTES_ADDRESS.load(Ordering::Relaxed),
            original_address,
            "mutable FFI views must use a copy-in/out scratch buffer"
        );
    }

    #[test]
    fn empty_views_use_null_pointers_with_zero_lengths() {
        let mut arguments = [
            FfiValue::String(String::new()),
            FfiValue::Bytes(Vec::new()),
            FfiValue::Bytes(Vec::new()),
        ];
        assert_eq!(
            call(
                observe_empty_views as *const (),
                FfiSignature::new(
                    vec![
                        FfiType::StringView,
                        FfiType::BytesView,
                        FfiType::BytesViewMut,
                    ],
                    FfiType::Bool,
                ),
                &mut arguments,
            ),
            Ok(FfiValue::Bool(true))
        );
        assert_eq!(arguments[2], FfiValue::Bytes(Vec::new()));
    }

    #[test]
    fn mutable_view_writes_back_even_when_return_validation_fails() {
        let mut arguments = [FfiValue::Bytes(vec![1, 2, 3])];
        assert_eq!(
            call(
                mutate_then_return_invalid_bool as *const (),
                FfiSignature::new(vec![FfiType::BytesViewMut], FfiType::Bool),
                &mut arguments,
            ),
            Err(FfiError::NonCanonicalBoolReturn(2))
        );
        assert_eq!(arguments, [FfiValue::Bytes(vec![2, 3, 4])]);
    }

    #[test]
    fn opaque_handle_round_trips_but_a_null_return_is_rejected() {
        let mut payload = Box::new(27_u64);
        let handle =
            OpaqueHandle::new((&mut *payload as *mut u64).cast()).expect("box pointer is non-null");
        let mut arguments = [FfiValue::OpaqueHandle(handle)];
        let result = call(
            round_trip_handle as *const (),
            FfiSignature::new(vec![FfiType::OpaqueHandle], FfiType::OpaqueHandle),
            &mut arguments,
        )
        .expect("non-null returned handle");
        let FfiValue::OpaqueHandle(result) = result else {
            panic!("expected an opaque handle");
        };
        assert_eq!(result.as_ptr(), (&mut *payload as *mut u64).cast());

        let null_error = call(
            return_null_handle as *const (),
            FfiSignature::new(vec![], FfiType::OpaqueHandle),
            &mut [],
        )
        .expect_err("null handles are invalid");
        assert_eq!(null_error, FfiError::NullOpaqueHandleReturn);
        assert_eq!(
            null_error.to_string(),
            "FFI function returned a null opaque handle"
        );
        assert!(OpaqueHandle::new(std::ptr::null_mut()).is_none());
        assert!(HostFunction::new(std::ptr::null_mut()).is_none());
    }

    #[test]
    fn c_text_arguments_are_nul_terminated_and_none_is_a_null_pointer() {
        let length = |ffi_type, value| {
            call(
                text_length as *const (),
                FfiSignature::new(vec![ffi_type], FfiType::I64),
                &mut [value],
            )
        };
        let text = || FfiValue::String("h\u{e9}llo".to_string());
        assert_eq!(length(FfiType::CString, text()), Ok(FfiValue::I64(6)));
        assert_eq!(
            length(FfiType::CString, FfiValue::String(String::new())),
            Ok(FfiValue::I64(0))
        );
        assert_eq!(
            length(FfiType::NullableCString, text()),
            Ok(FfiValue::I64(6))
        );
        assert_eq!(
            length(FfiType::NullableCString, FfiValue::Unit),
            Ok(FfiValue::I64(-1))
        );

        let error = length(FfiType::CString, FfiValue::String("a\0b".to_string()))
            .expect_err("text with a NUL inside cannot be a C string");
        assert_eq!(error, FfiError::InteriorNul { index: 0 });
        assert_eq!(
            error.to_string(),
            "FFI argument 1 is a cstr that contains a NUL byte"
        );
        assert_eq!(FfiType::NullableCString.to_string(), "nullable cstr");
    }

    #[test]
    fn c_text_results_are_copied_and_null_or_invalid_text_is_reported() {
        let pick = |result, which| {
            call(
                pick_text as *const (),
                FfiSignature::new(vec![FfiType::I32], result),
                &mut [FfiValue::I32(which)],
            )
        };
        let text = FfiValue::String("h\u{e9}llo".to_string());
        assert_eq!(pick(FfiType::CString, 1).as_ref(), Ok(&text));
        assert_eq!(pick(FfiType::NullableCString, 1).as_ref(), Ok(&text));
        assert_eq!(pick(FfiType::NullableCString, 0), Ok(FfiValue::Unit));

        let null = pick(FfiType::CString, 0).expect_err("a cstr result is not null");
        assert_eq!(null, FfiError::NullCStringReturn);
        assert_eq!(null.to_string(), "FFI function returned a null cstr");
        for result in [FfiType::CString, FfiType::NullableCString] {
            let invalid = pick(result, 2).expect_err("a cstr result is UTF-8");
            assert_eq!(invalid, FfiError::InvalidUtf8Return);
            assert_eq!(
                invalid.to_string(),
                "FFI function returned a cstr that is not UTF-8"
            );
        }
    }

    #[test]
    fn pointer_parameters_pass_a_c_array_and_read_a_mut_one_back() {
        let runs = [
            (FfiScalar::Bool, 1, vec![1, 0, 0]),
            (FfiScalar::I8, 1, vec![0x80, 2, 3]),
            (FfiScalar::U8, 1, vec![0xff, 2, 3]),
            (FfiScalar::I16, 2, vec![0x8000, 2, 3]),
            (FfiScalar::U16, 2, vec![0xffff, 2, 3]),
            (FfiScalar::I32, 4, vec![0x8000_0000, 2, 3]),
            (FfiScalar::U32, 4, vec![0xffff_ffff, 2, 3]),
            (FfiScalar::F32, 4, vec![u64::from(1.5_f32.to_bits()), 2, 3]),
            (FfiScalar::I64, 8, vec![1 << 63, 2, 3]),
            (FfiScalar::U64, 8, vec![u64::MAX, 2, 3]),
            (FfiScalar::F64, 8, vec![(-2.25_f64).to_bits(), 2, 3]),
        ];
        for (scalar, size, words) in runs {
            assert_eq!(scalar.size(), size);
            let packed = pack_elements(scalar, &words);
            assert_eq!(unpack_elements(scalar, &packed, words.len()), words);
            let value = || FfiValue::Elements(scalar, words.clone());
            assert_eq!(value().ffi_type(), FfiType::Pointer(scalar));

            let reversed: Vec<u64> = words.iter().rev().copied().collect();
            let count = || FfiValue::I64(words.len() as i64);
            let width = || FfiValue::I64(size as i64);
            // The function writes through a `mut` pointer to the caller.
            let mut arguments = [value(), count(), width()];
            let result = call(
                reverse_run as *const (),
                FfiSignature::new(
                    vec![FfiType::PointerMut(scalar), FfiType::I64, FfiType::I64],
                    FfiType::Unit,
                ),
                &mut arguments,
            );
            assert_eq!(result, Ok(FfiValue::Unit));
            assert_eq!(arguments[0], FfiValue::Elements(scalar, reversed));
            // What it writes through a read pointer stays in the copy.
            let mut arguments = [value(), count(), width()];
            let result = call(
                reverse_run as *const (),
                FfiSignature::new(
                    vec![FfiType::Pointer(scalar), FfiType::I64, FfiType::I64],
                    FfiType::Unit,
                ),
                &mut arguments,
            );
            assert_eq!(result, Ok(FfiValue::Unit));
            assert_eq!(arguments[0], value());
        }
        assert_eq!(FfiType::Pointer(FfiScalar::I16).to_string(), "ptr[int16]");
        assert_eq!(
            FfiType::PointerMut(FfiScalar::F64).to_string(),
            "mut ptr[float64]"
        );

        for pointer in [
            FfiType::Pointer(FfiScalar::F64),
            FfiType::PointerMut(FfiScalar::F64),
        ] {
            let mut arguments = [FfiValue::Elements(FfiScalar::F64, Vec::new())];
            let result = call(
                run_is_null as *const (),
                FfiSignature::new(vec![pointer], FfiType::Bool),
                &mut arguments,
            );
            assert_eq!(result, Ok(FfiValue::Bool(true)), "an empty run is null");
            assert_eq!(arguments[0], FfiValue::Elements(FfiScalar::F64, Vec::new()));

            let error = call(
                run_is_null as *const (),
                FfiSignature::new(vec![pointer], FfiType::Bool),
                &mut [FfiValue::Elements(FfiScalar::F32, vec![1])],
            )
            .expect_err("a pointer takes elements of its own scalar");
            assert_eq!(
                error,
                FfiError::ArgumentTypeMismatch {
                    index: 0,
                    expected: pointer,
                    actual: FfiType::Pointer(FfiScalar::F32),
                }
            );
            let error = call(
                run_is_null as *const (),
                FfiSignature::new(Vec::new(), pointer),
                &mut [],
            )
            .expect_err("a pointer is not a result");
            assert_eq!(error, FfiError::UnsupportedReturnType(pointer));
        }
    }

    #[test]
    fn out_parameters_pass_a_pointer_and_read_the_scalar_back() {
        let scalars = [
            FfiScalar::Bool,
            FfiScalar::I8,
            FfiScalar::I16,
            FfiScalar::I32,
            FfiScalar::I64,
            FfiScalar::U8,
            FfiScalar::U16,
            FfiScalar::U32,
            FfiScalar::U64,
            FfiScalar::F32,
            FfiScalar::F64,
        ];
        for scalar in scalars {
            assert_eq!(FfiScalar::of(scalar.ffi_type()), Some(scalar));
        }
        assert_eq!(FfiScalar::of(FfiType::StringView), None);
        assert_eq!(
            FfiType::Out(FfiScalar::U16).to_string(),
            "mut uint16",
            "an out-parameter is named as its declaration spells it"
        );

        let mut arguments = [
            FfiValue::Bool(false),
            FfiValue::I8(i8::MIN + 1),
            FfiValue::I16(i16::MIN + 1),
            FfiValue::I32(i32::MIN + 1),
            FfiValue::I64(i64::MIN + 1),
            FfiValue::U8(u8::MAX - 1),
            FfiValue::U16(u16::MAX - 1),
            FfiValue::U32(u32::MAX - 1),
            FfiValue::U64(u64::MAX - 1),
            FfiValue::F32(1.5),
            FfiValue::F64(-2.25),
        ];
        let result = call(
            bump_scalars as *const (),
            FfiSignature::new(scalars.map(FfiType::Out).to_vec(), FfiType::Unit),
            &mut arguments,
        );
        assert_eq!(result, Ok(FfiValue::Unit));
        assert_eq!(
            arguments,
            [
                FfiValue::Bool(true),
                FfiValue::I8(i8::MIN),
                FfiValue::I16(i16::MIN),
                FfiValue::I32(i32::MIN),
                FfiValue::I64(i64::MIN),
                FfiValue::U8(u8::MAX),
                FfiValue::U16(u16::MAX),
                FfiValue::U32(u32::MAX),
                FfiValue::U64(u64::MAX),
                FfiValue::F32(3.0),
                FfiValue::F64(-4.5),
            ]
        );

        let error = call(
            bump_scalars as *const (),
            FfiSignature::new(vec![FfiType::Out(FfiScalar::I32)], FfiType::Unit),
            &mut [FfiValue::I64(1)],
        )
        .expect_err("an out-parameter takes a scalar of its own type");
        assert_eq!(
            error,
            FfiError::ArgumentTypeMismatch {
                index: 0,
                expected: FfiType::Out(FfiScalar::I32),
                actual: FfiType::I64,
            }
        );
    }

    #[test]
    fn an_out_bool_that_is_not_a_bool_is_reported_after_the_result() {
        let signature = || FfiSignature::new(vec![FfiType::Out(FfiScalar::Bool)], FfiType::Bool);
        let mut arguments = [FfiValue::Bool(false)];
        let error = call(write_bad_bool as *const (), signature(), &mut arguments)
            .expect_err("7 is not a bool");
        assert_eq!(error, FfiError::NonCanonicalBoolReturn(7));
        assert_eq!(
            arguments,
            [FfiValue::Bool(false)],
            "the place keeps its value"
        );

        let error = call(
            write_bad_bool_and_return_one as *const (),
            signature(),
            &mut arguments,
        )
        .expect_err("the result is refused first");
        assert_eq!(error, FfiError::NonCanonicalBoolReturn(9));
    }

    #[test]
    fn nullable_handles_cross_as_pointers_in_both_directions() {
        let mut first = 0_u8;
        let mut second = 0_u8;
        let first: *mut c_void = (&raw mut first).cast();
        let second: *mut c_void = (&raw mut second).cast();
        let handle =
            |address| FfiValue::OpaqueHandle(OpaqueHandle::new(address).expect("non-null"));
        let signature = || {
            FfiSignature::new(
                vec![FfiType::OpaqueHandleOut, FfiType::NullableOpaqueHandle],
                FfiType::NullableOpaqueHandle,
            )
        };

        let mut arguments = [FfiValue::Unit, handle(first)];
        let held = call(swap_handle as *const (), signature(), &mut arguments);
        assert_eq!(held, Ok(FfiValue::Unit), "the slot held null");
        assert_eq!(arguments[0], handle(first));

        let mut arguments = [handle(first), handle(second)];
        let held = call(swap_handle as *const (), signature(), &mut arguments);
        assert_eq!(held, Ok(handle(first)));
        assert_eq!(arguments[0], handle(second));

        let mut arguments = [handle(second), FfiValue::Unit];
        let held = call(swap_handle as *const (), signature(), &mut arguments);
        assert_eq!(held, Ok(handle(second)));
        assert_eq!(arguments[0], FfiValue::Unit, "null was stored through");
        assert_eq!(
            FfiType::OpaqueHandleOut.to_string(),
            "mut nullable opaque handle"
        );

        let error = call(
            swap_handle as *const (),
            signature(),
            &mut [FfiValue::I64(1), FfiValue::Unit],
        )
        .expect_err("a handle slot takes a handle or None");
        assert_eq!(
            error,
            FfiError::ArgumentTypeMismatch {
                index: 0,
                expected: FfiType::OpaqueHandleOut,
                actual: FfiType::I64,
            }
        );
    }

    #[test]
    fn rejects_arity_type_and_view_return_contract_errors_before_calling_host_code() {
        let arity = call(
            i32_identity as *const (),
            FfiSignature::new(vec![FfiType::I32], FfiType::I32),
            &mut [],
        )
        .expect_err("missing FFI arguments must be rejected");
        assert_eq!(
            arity,
            FfiError::ArityMismatch {
                expected: 1,
                actual: 0,
            }
        );
        assert_eq!(
            arity.to_string(),
            "FFI call expected 1 argument(s), but received 0"
        );

        let type_mismatch = call(
            i32_identity as *const (),
            FfiSignature::new(vec![FfiType::I32], FfiType::I32),
            &mut [FfiValue::Bool(true)],
        )
        .expect_err("wrong FFI argument types must be rejected");
        assert_eq!(
            type_mismatch,
            FfiError::ArgumentTypeMismatch {
                index: 0,
                expected: FfiType::I32,
                actual: FfiType::Bool,
            }
        );
        assert_eq!(
            type_mismatch.to_string(),
            "FFI argument 1 expected int32, but received bool"
        );

        for view_type in [
            FfiType::StringView,
            FfiType::BytesView,
            FfiType::BytesViewMut,
        ] {
            let unsupported_return = call(
                i32_identity as *const (),
                FfiSignature::new(vec![FfiType::I32], view_type),
                &mut [FfiValue::I32(0)],
            )
            .expect_err("borrowed pointer-length views cannot be returned");
            assert_eq!(
                unsupported_return,
                FfiError::UnsupportedReturnType(view_type)
            );
            assert_eq!(
                unsupported_return.to_string(),
                format!("FFI functions cannot return {view_type}")
            );
        }

        let invalid_bool = call(
            invalid_bool_return as *const (),
            FfiSignature::new(vec![], FfiType::Bool),
            &mut [],
        )
        .expect_err("noncanonical bool returns must be rejected");
        assert_eq!(invalid_bool, FfiError::NonCanonicalBoolReturn(2));
        assert_eq!(
            invalid_bool.to_string(),
            "FFI bool return must be encoded as 0 or 1, but received 2"
        );
    }

    #[cfg(unix)]
    #[test]
    fn resolves_process_global_system_symbols_and_reports_lookup_failures() {
        let signature = FfiSignature::new(vec![], FfiType::I32);
        // SAFETY: POSIX getpid has the exact `int getpid(void)` signature.
        let process_id =
            unsafe { call_process_symbol("getpid", &signature, &mut []) }.expect("getpid");
        let FfiValue::I32(process_id) = process_id else {
            panic!("getpid should return i32");
        };
        assert!(process_id > 0);

        // SAFETY: no foreign call occurs when lookup fails.
        let error =
            unsafe { call_process_symbol("__aura_missing_ffi_test_symbol__", &signature, &mut []) }
                .expect_err("unknown symbol should fail safely");
        assert!(matches!(error, FfiError::SymbolNotFound { .. }));
        assert!(error
            .to_string()
            .starts_with("FFI symbol `__aura_missing_ffi_test_symbol__` was not found:"));

        // SAFETY: no lookup/call occurs after the interior-NUL check fails.
        let invalid_name = unsafe { call_process_symbol("getpid\0ignored", &signature, &mut []) }
            .expect_err("interior NUL must be rejected before lookup");
        assert_eq!(invalid_name, FfiError::InvalidSymbolName);
        assert_eq!(
            invalid_name.to_string(),
            "FFI symbol names cannot contain a NUL byte"
        );
    }

    /// A directory of this test's own under the system's temporary one.
    #[cfg(unix)]
    fn scratch_directory(label: &str) -> PathBuf {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("the clock follows the Unix epoch")
            .as_nanos();
        let path =
            std::env::temp_dir().join(format!("aura-ffi-{label}-{}-{unique}", std::process::id()));
        std::fs::create_dir_all(&path).expect("the scratch directory should be creatable");
        path
    }

    /// Builds a shared library at `file` whose one function, `symbol`,
    /// returns `answer`.
    #[cfg(unix)]
    fn build_fixture_library(file: &Path, symbol: &str, answer: i32) {
        use std::io::Write;
        let kind = if cfg!(target_os = "macos") {
            "-dynamiclib"
        } else {
            "-shared"
        };
        let mut compiler = std::process::Command::new("cc")
            .args([kind, "-fPIC", "-x", "c", "-", "-o"])
            .arg(file)
            .stdin(std::process::Stdio::piped())
            .spawn()
            .expect("the C compiler should start");
        compiler
            .stdin
            .take()
            .expect("the compiler reads its source from a pipe")
            .write_all(format!("int {symbol}(void) {{ return {answer}; }}\n").as_bytes())
            .expect("the source should reach the compiler");
        let status = compiler.wait().expect("the C compiler should finish");
        assert!(status.success(), "the fixture library should build");
    }

    #[cfg(unix)]
    fn call_fixture(symbol: &str) -> Result<FfiValue, FfiError> {
        let signature = FfiSignature::new(vec![], FfiType::I32);
        // SAFETY: every fixture function is `int f(void)`.
        unsafe { call_process_symbol(symbol, &signature, &mut []) }
    }

    #[test]
    fn library_names_follow_the_host_convention() {
        let names = library_file_names("sqlite3");
        if cfg!(target_os = "macos") {
            assert_eq!(names, ["libsqlite3.dylib", "libsqlite3.so"]);
        } else {
            assert_eq!(names, ["libsqlite3.so"]);
        }
    }

    #[cfg(unix)]
    #[test]
    fn load_libraries_loads_from_a_search_path_once() {
        let directory = scratch_directory("load");
        let file = directory.join(&library_file_names("aura_ffi_fixture_load")[0]);
        build_fixture_library(&file, "aura_ffi_fixture_load_answer", 41);
        assert!(matches!(
            call_fixture("aura_ffi_fixture_load_answer"),
            Err(FfiError::SymbolNotFound { .. })
        ));

        let libraries = ["aura_ffi_fixture_load".to_owned()];
        load_libraries(&libraries, std::slice::from_ref(&directory), &[])
            .expect("the library is in the search path");
        assert_eq!(
            call_fixture("aura_ffi_fixture_load_answer"),
            Ok(FfiValue::I32(41))
        );

        // A loaded library is not searched for again.
        std::fs::remove_dir_all(&directory).expect("the scratch directory should be removable");
        load_libraries(&libraries, &[], &[]).expect("the library is already loaded");
    }

    #[cfg(unix)]
    #[test]
    fn load_libraries_finds_a_library_that_has_only_a_versioned_file() {
        let directory = scratch_directory("versioned");
        let file = directory.join("libaura_ffi_fixture_versioned.so.3");
        build_fixture_library(&file, "aura_ffi_fixture_versioned_answer", 43);
        assert_eq!(
            versioned_library_files(&directory, "libaura_ffi_fixture_versioned.so"),
            [file]
        );
        assert!(versioned_library_files(&directory.join("missing"), "libx.so").is_empty());

        load_libraries(
            &["aura_ffi_fixture_versioned".to_owned()],
            std::slice::from_ref(&directory),
            &[],
        )
        .expect("the versioned file is in the search path");
        assert_eq!(
            call_fixture("aura_ffi_fixture_versioned_answer"),
            Ok(FfiValue::I32(43))
        );
        std::fs::remove_dir_all(&directory).expect("the scratch directory should be removable");
    }

    #[cfg(unix)]
    #[test]
    fn load_libraries_finds_a_system_library_by_its_bare_name() {
        load_libraries(&["m".to_owned()], &[], &[]).expect("the C math library is installed");
        assert!(system_library_directories().ends_with(&[PathBuf::from("/lib")]));
    }

    #[cfg(unix)]
    #[test]
    fn load_libraries_reports_a_library_it_cannot_load() {
        let directory = scratch_directory("missing");
        let not_a_library = directory.join(&library_file_names("aura_ffi_fixture_garbage")[0]);
        std::fs::write(&not_a_library, "not a library").expect("the file should be writable");

        for name in ["aura_ffi_fixture_absent", "aura_ffi_fixture_garbage"] {
            let error = load_libraries(&[name.to_owned()], std::slice::from_ref(&directory), &[])
                .expect_err("the library cannot be loaded");
            let FfiError::LibraryNotFound { library, detail } = &error else {
                panic!("expected a library failure, found {error:?}");
            };
            assert_eq!(library, name);
            assert!(!detail.is_empty());
            assert_eq!(
                error.to_string(),
                format!("FFI library `{name}` could not be loaded: {detail}")
            );
        }
        std::fs::remove_dir_all(&directory).expect("the scratch directory should be removable");

        assert_eq!(
            load_first("nothing", &[]),
            Err(FfiError::LibraryNotFound {
                library: "nothing".to_owned(),
                detail: "there is no file to try".to_owned(),
            })
        );
        assert_eq!(
            open_library(Path::new("lib\0.so")),
            Err("the path contains a NUL byte".to_owned())
        );
    }

    #[cfg(unix)]
    #[test]
    fn frameworks_load_on_macos_and_are_skipped_elsewhere() {
        assert_eq!(
            framework_candidates("Accelerate"),
            [
                PathBuf::from("/System/Library/Frameworks/Accelerate.framework/Accelerate"),
                PathBuf::from("/Library/Frameworks/Accelerate.framework/Accelerate"),
            ]
        );
        let missing = load_libraries(&[], &[], &["AuraNoSuchFramework".to_owned()]);
        if cfg!(target_os = "macos") {
            load_libraries(&[], &[], &["CoreFoundation".to_owned()])
                .expect("CoreFoundation ships with macOS");
            assert!(matches!(missing, Err(FfiError::LibraryNotFound { .. })));
        } else {
            assert_eq!(missing, Ok(()));
        }
    }
}
