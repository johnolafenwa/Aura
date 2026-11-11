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

use libffi::middle::{Arg, Cif, CodePtr, Type};
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
    /// A result-only opaque pointer that may be null; null yields `None`
    /// (ADR-0052 A10).
    NullableOpaqueHandle,
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
            Self::StringView => "str view",
            Self::BytesView => "list[uint8] view",
            Self::BytesViewMut => "mut list[uint8] view",
            Self::OpaqueHandle => "opaque handle",
            Self::NullableOpaqueHandle => "nullable opaque handle",
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
        FfiType::OpaqueHandle | FfiType::NullableOpaqueHandle => Type::pointer(),
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
        FfiType::NullableOpaqueHandle => {
            let pointer: *mut c_void = unsafe { cif.call(function, arguments) };
            match OpaqueHandle::new(pointer) {
                Some(handle) => FfiValue::OpaqueHandle(handle),
                None => FfiValue::Unit,
            }
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
#[derive(Default)]
struct LoadedLibraries {
    by_request: std::collections::BTreeSet<(u8, String, Vec<PathBuf>)>,
    loaded_names: std::collections::BTreeSet<(u8, String)>,
    handles: Vec<usize>,
}

#[cfg(unix)]
fn loaded_libraries_registry() -> &'static std::sync::Mutex<LoadedLibraries> {
    static LOADED: std::sync::OnceLock<std::sync::Mutex<LoadedLibraries>> =
        std::sync::OnceLock::new();
    LOADED.get_or_init(|| std::sync::Mutex::new(LoadedLibraries::default()))
}

#[cfg(unix)]
fn host_image_base() -> *mut libc::c_void {
    let mut info = std::mem::MaybeUninit::<libc::Dl_info>::zeroed();
    // SAFETY: `host_image_base` is a valid code address inside this binary image,
    // and `info` points to writable `Dl_info` storage.
    let ok = unsafe { libc::dladdr(host_image_base as *const libc::c_void, info.as_mut_ptr()) };
    if ok != 0 {
        // SAFETY: `dladdr` initialized `info` on non-zero return.
        unsafe { info.assume_init().dli_fbase }
    } else {
        std::ptr::null_mut()
    }
}

#[cfg(unix)]
fn is_internal_host_symbol(symbol: &str, pointer: *const libc::c_void) -> bool {
    if symbol.starts_with("aura_direct_")
        || symbol.starts_with("aura_debug_")
        || symbol.starts_with("LLVM")
        || symbol.starts_with("_ZN")
        || symbol.starts_with("__rust_")
        || symbol == "rust_eh_personality"
    {
        return true;
    }
    if pointer.is_null() {
        return false;
    }
    let mut info = std::mem::MaybeUninit::<libc::Dl_info>::zeroed();
    // SAFETY: `pointer` is a non-null symbol address returned by the dynamic loader
    // (or a test probe function pointer) and `info` is writable.
    if unsafe { libc::dladdr(pointer, info.as_mut_ptr()) } != 0 {
        // SAFETY: `dladdr` succeeded and populated `info`.
        let info = unsafe { info.assume_init() };
        let self_base = host_image_base();
        if !self_base.is_null() && info.dli_fbase == self_base {
            return true;
        }
        if !info.dli_fname.is_null() {
            // SAFETY: `dli_fname` is a loader-owned NUL-terminated path string.
            let image_name = unsafe { std::ffi::CStr::from_ptr(info.dli_fname) }.to_string_lossy();
            if let Some(base_name) = Path::new(image_name.as_ref())
                .file_name()
                .and_then(|n| n.to_str())
            {
                if base_name.starts_with("libLLVM") || base_name.starts_with("LLVM") {
                    return true;
                }
            }
        }
    }
    false
}

#[cfg(unix)]
fn process_symbol(symbol: &str) -> Result<HostFunction, FfiError> {
    let c_symbol = CString::new(symbol).map_err(|_| FfiError::InvalidSymbolName)?;
    let handles = loaded_libraries_registry()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .handles
        .clone();
    for handle in handles {
        // SAFETY: `handle` is a live `dlopen` handle kept for the lifetime of the
        // process, and `c_symbol` is NUL terminated.
        let ptr = unsafe {
            libc::dlerror();
            libc::dlsym(handle as *mut libc::c_void, c_symbol.as_ptr())
        };
        if let Some(function) = HostFunction::new(ptr) {
            return Ok(function);
        }
    }

    // SAFETY: `c_symbol` is NUL terminated. Clearing dlerror before dlsym lets
    // us distinguish a stale loader error from this lookup.
    let pointer = unsafe {
        libc::dlerror();
        libc::dlsym(libc::RTLD_DEFAULT, c_symbol.as_ptr())
    };
    if !pointer.is_null() && is_internal_host_symbol(symbol, pointer) {
        return Err(FfiError::SymbolNotFound {
            symbol: symbol.to_owned(),
            detail: "symbol resolves to internal Aura host image".to_owned(),
        });
    }
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

#[cfg(unix)]
fn parse_soname_version(suffix: &str) -> Option<Vec<u64>> {
    if suffix.is_empty() {
        return None;
    }
    suffix
        .split('.')
        .map(|part| part.parse::<u64>().ok())
        .collect()
}

/// The files in `directory` that are versions of `file_name`, such as
/// `libsqlite3.so.0` for `libsqlite3.so`, sorted by descending numeric version.
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
    files.sort_by(|left, right| {
        let left_ver = left
            .file_name()
            .and_then(|n| n.to_str())
            .and_then(|n| n.strip_prefix(&prefix))
            .and_then(parse_soname_version);
        let right_ver = right
            .file_name()
            .and_then(|n| n.to_str())
            .and_then(|n| n.strip_prefix(&prefix))
            .and_then(parse_soname_version);
        match (left_ver, right_ver) {
            (Some(l), Some(r)) => r.cmp(&l).then_with(|| left.cmp(right)),
            (Some(_), None) => std::cmp::Ordering::Less,
            (None, Some(_)) => std::cmp::Ordering::Greater,
            (None, None) => left.cmp(right),
        }
    });
    files
}

#[cfg(unix)]
fn open_library(path: &Path) -> Result<usize, String> {
    use std::os::unix::ffi::OsStrExt;
    let c_path = CString::new(path.as_os_str().as_bytes())
        .map_err(|_| "the path contains a NUL byte".to_string())?;
    // SAFETY: `c_path` is NUL terminated. `RTLD_LOCAL` keeps loaded FFI symbols
    // out of `RTLD_DEFAULT`; the returned handle is retained in `LoadedLibraries`.
    let handle = unsafe {
        libc::dlerror();
        libc::dlopen(c_path.as_ptr(), libc::RTLD_NOW | libc::RTLD_LOCAL)
    };
    if !handle.is_null() {
        return Ok(handle as usize);
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
fn load_first(library: &str, candidates: &[PathBuf]) -> Result<usize, FfiError> {
    let mut first_failure = None;
    for candidate in candidates {
        match open_library(candidate) {
            Ok(handle) => return Ok(handle),
            Err(detail) => first_failure = first_failure.or(Some(detail)),
        }
    }
    Err(FfiError::LibraryNotFound {
        library: library.to_owned(),
        detail: first_failure.unwrap_or_else(|| "there is no file to try".to_owned()),
    })
}

/// Loads every library and framework a program links into this process,
/// once per `(kind, name, canonical_search_paths)` tuple, so the symbols
/// resolve via explicit `RTLD_LOCAL` handles as they do in a built program.
/// Frameworks exist only on macOS and are skipped elsewhere.
#[cfg(unix)]
pub fn load_libraries(
    libraries: &[String],
    search_paths: &[PathBuf],
    frameworks: &[String],
) -> Result<(), FfiError> {
    const KIND_LIBRARY: u8 = 0;
    const KIND_FRAMEWORK: u8 = 1;

    let raw_paths = search_paths.to_vec();
    let canonical_paths = search_paths
        .iter()
        .map(|path| std::fs::canonicalize(path).unwrap_or_else(|_| path.clone()))
        .collect::<Vec<_>>();
    let frameworks = frameworks.iter().filter(|_| cfg!(target_os = "macos"));
    let wanted = libraries
        .iter()
        .map(|name| {
            (
                KIND_LIBRARY,
                name,
                raw_paths.clone(),
                canonical_paths.clone(),
                library_candidates(name, search_paths),
            )
        })
        .chain(frameworks.map(|name| {
            (
                KIND_FRAMEWORK,
                name,
                Vec::new(),
                Vec::new(),
                framework_candidates(name),
            )
        }));
    let mut loaded = loaded_libraries_registry()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    for (kind, name, req_raw, req_canon, candidates) in wanted {
        let raw_key = (kind, name.clone(), req_raw);
        let canon_key = (kind, name.clone(), req_canon);
        let bare_satisfied =
            raw_key.2.is_empty() && loaded.loaded_names.contains(&(kind, name.clone()));
        if !bare_satisfied
            && !loaded.by_request.contains(&raw_key)
            && !loaded.by_request.contains(&canon_key)
        {
            let handle = load_first(name, &candidates)?;
            if !loaded.handles.contains(&handle) {
                loaded.handles.push(handle);
            }
            loaded.loaded_names.insert((kind, name.clone()));
            loaded.by_request.insert(raw_key);
            loaded.by_request.insert(canon_key);
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
            (FfiType::NullableOpaqueHandle, "nullable opaque handle"),
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

    #[cfg(unix)]
    #[no_mangle]
    pub extern "C" fn aura_ffi_host_internal_probe_for_test() -> i32 {
        77
    }

    #[cfg(unix)]
    #[test]
    fn versioned_library_files_sort_numerically_descending() {
        let directory = scratch_directory("version-order");
        for suffix in ["0", "1", "2", "10", "1.2", "1.10"] {
            std::fs::write(directory.join(format!("libver.so.{suffix}")), b"")
                .expect("touch versioned file");
        }
        let ordered = versioned_library_files(&directory, "libver.so")
            .into_iter()
            .map(|path| {
                path.file_name()
                    .expect("file name")
                    .to_string_lossy()
                    .into_owned()
            })
            .collect::<Vec<_>>();
        std::fs::remove_dir_all(&directory).expect("cleanup scratch directory");
        assert_eq!(
            ordered,
            [
                "libver.so.10",
                "libver.so.2",
                "libver.so.1.10",
                "libver.so.1.2",
                "libver.so.1",
                "libver.so.0",
            ]
        );
    }

    #[cfg(unix)]
    #[test]
    fn load_libraries_keys_cache_by_search_paths_uses_rtld_local_and_hides_host_symbols() {
        // 1. Host executable symbols must not be callable via default process lookup.
        assert!(is_internal_host_symbol(
            "custom_symbol_inside_host_image",
            aura_ffi_host_internal_probe_for_test as *const libc::c_void
        ));
        assert!(is_internal_host_symbol(
            "aura_direct_frame_stack",
            std::ptr::null()
        ));
        assert!(is_internal_host_symbol(
            "LLVMContextCreate",
            std::ptr::null()
        ));
        let getpid_ptr = unsafe { libc::dlsym(libc::RTLD_DEFAULT, c"getpid".as_ptr()) };
        assert!(!getpid_ptr.is_null());
        assert!(!is_internal_host_symbol("getpid", getpid_ptr));
        assert!(
            matches!(
                call_fixture("aura_ffi_host_internal_probe_for_test"),
                Err(FfiError::SymbolNotFound { .. })
            ),
            "internal host binary symbols must not be exposed to FFI lookup"
        );

        // 2. Loaded FFI libraries must use RTLD_LOCAL (not leaked into RTLD_DEFAULT)
        //    while remaining callable through explicit handle lookup.
        let local_dir = scratch_directory("rtld-local");
        let local_file = local_dir.join(&library_file_names("aura_ffi_fixture_local")[0]);
        build_fixture_library(&local_file, "aura_ffi_fixture_local_only", 91);
        load_libraries(
            &["aura_ffi_fixture_local".to_owned()],
            std::slice::from_ref(&local_dir),
            &[],
        )
        .expect("load local fixture library");
        assert_eq!(
            call_fixture("aura_ffi_fixture_local_only"),
            Ok(FfiValue::I32(91))
        );
        let leaked_ptr = unsafe {
            let sym = CString::new("aura_ffi_fixture_local_only").unwrap();
            libc::dlsym(libc::RTLD_DEFAULT, sym.as_ptr())
        };
        std::fs::remove_dir_all(&local_dir).expect("cleanup local_dir");
        assert!(
            leaked_ptr.is_null(),
            "dlopen must use RTLD_LOCAL rather than polluting RTLD_DEFAULT"
        );

        // 3. Libraries with the same logical name under distinct search_paths must both load.
        let dir_a = scratch_directory("same-name-a");
        let dir_b = scratch_directory("same-name-b");
        let file_name = &library_file_names("aura_ffi_same_name")[0];
        build_fixture_library(&dir_a.join(file_name), "aura_ffi_same_name_from_a", 11);
        build_fixture_library(&dir_b.join(file_name), "aura_ffi_same_name_from_b", 22);

        let names = ["aura_ffi_same_name".to_owned()];
        load_libraries(&names, std::slice::from_ref(&dir_a), &[]).expect("load from dir_a");
        load_libraries(&names, std::slice::from_ref(&dir_b), &[]).expect("load from dir_b");

        assert_eq!(
            call_fixture("aura_ffi_same_name_from_a"),
            Ok(FfiValue::I32(11))
        );
        assert_eq!(
            call_fixture("aura_ffi_same_name_from_b"),
            Ok(FfiValue::I32(22))
        );
        std::fs::remove_dir_all(&dir_a).expect("cleanup dir_a");
        std::fs::remove_dir_all(&dir_b).expect("cleanup dir_b");
    }
}
