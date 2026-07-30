//! Backend-neutral execution support for Aurora's deliberately small C ABI.
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

use libffi::middle::{Arg, Cif, CodePtr, Type};
use std::error::Error;
use std::ffi::{c_void, CString};
use std::fmt;
use std::ptr::NonNull;

/// A source-level value's representation at Aurora's C ABI boundary.
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
}

impl fmt::Display for FfiType {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
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
            Self::StringView => "String view",
            Self::BytesView => "Vec[uint8] view",
            Self::BytesViewMut => "mut Vec[uint8] view",
            Self::OpaqueHandle => "opaque handle",
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
/// Aurora's semantic layer owns the consume/share rules and the foreign
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
        }
    }
}

fn type_accepts_value(expected: FfiType, value: &FfiValue) -> bool {
    match (expected, value) {
        (FfiType::BytesView | FfiType::BytesViewMut, FfiValue::Bytes(_)) => true,
        (expected, value) => expected == value.ffi_type(),
    }
}

fn append_argument(
    index: usize,
    expected: FfiType,
    value: &mut FfiValue,
    mutable_bytes: Option<&mut Vec<u8>>,
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
        (FfiType::OpaqueHandle, FfiValue::OpaqueHandle(value)) => {
            types.push(Type::pointer());
            values.push(AbiArgument::OpaquePointer(value.as_ptr()));
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
        FfiType::OpaqueHandle => Type::pointer(),
        FfiType::StringView | FfiType::BytesView | FfiType::BytesViewMut => {
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
        FfiType::StringView | FfiType::BytesView | FfiType::BytesViewMut => {
            return Err(FfiError::UnsupportedReturnType(result));
        }
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
    result
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
            (FfiType::StringView, "String view"),
            (FfiType::BytesView, "Vec[uint8] view"),
            (FfiType::BytesViewMut, "mut Vec[uint8] view"),
            (FfiType::OpaqueHandle, "opaque handle"),
        ];
        for (ffi_type, source_name) in cases {
            let error = call(
                i32_identity as *const (),
                FfiSignature::new(vec![ffi_type], FfiType::Unit),
                &mut [FfiValue::Unit],
            )
            .expect_err("an Aurora None value must not cross a typed C parameter");
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
        let error = unsafe {
            call_process_symbol("__aurora_missing_ffi_test_symbol__", &signature, &mut [])
        }
        .expect_err("unknown symbol should fail safely");
        assert!(matches!(error, FfiError::SymbolNotFound { .. }));
        assert!(error
            .to_string()
            .starts_with("FFI symbol `__aurora_missing_ffi_test_symbol__` was not found:"));

        // SAFETY: no lookup/call occurs after the interior-NUL check fails.
        let invalid_name = unsafe { call_process_symbol("getpid\0ignored", &signature, &mut []) }
            .expect_err("interior NUL must be rejected before lookup");
        assert_eq!(invalid_name, FfiError::InvalidSymbolName);
        assert_eq!(
            invalid_name.to_string(),
            "FFI symbol names cannot contain a NUL byte"
        );
    }
}
