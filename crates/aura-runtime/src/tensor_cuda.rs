//! The CUDA device of the `tensor` module.
//!
//! A tensor on this device is CUDA device memory Aura allocates and owns.
//! A plan becomes CUDA C (`tensor_codegen.rs`), which NVRTC compiles once
//! for each distinct plan, and `matmul` is a cuBLAS call on the same
//! memory. The three libraries, the driver, NVRTC and cuBLAS, are opened
//! with `dlopen` the first time a program asks for the device, so Aura
//! needs no CUDA toolkit to build and a program that never asks pays
//! nothing.

use super::tensor_codegen::{self, Action, Program, BLOCK_THREADS};
use super::{Plan, Slot, Storage};
use crate::runtime_value::*;
use std::collections::HashMap;
use std::ffi::{c_char, c_int, c_uint, c_void, CStr, CString};
use std::sync::{Arc, Mutex, OnceLock};

type Status = c_int;
type DevicePointer = u64;

/// The functions Aura calls, by their exported names.
#[allow(non_snake_case)]
struct Api {
    cuInit: unsafe extern "C" fn(c_uint) -> Status,
    cuDeviceGetCount: unsafe extern "C" fn(*mut c_int) -> Status,
    cuDeviceGet: unsafe extern "C" fn(*mut c_int, c_int) -> Status,
    cuDevicePrimaryCtxRetain: unsafe extern "C" fn(*mut *mut c_void, c_int) -> Status,
    cuCtxSetCurrent: unsafe extern "C" fn(*mut c_void) -> Status,
    cuCtxSynchronize: unsafe extern "C" fn() -> Status,
    cuMemAlloc_v2: unsafe extern "C" fn(*mut DevicePointer, usize) -> Status,
    cuMemGetInfo_v2: unsafe extern "C" fn(*mut usize, *mut usize) -> Status,
    cuMemFree_v2: unsafe extern "C" fn(DevicePointer) -> Status,
    cuMemcpyHtoD_v2: unsafe extern "C" fn(DevicePointer, *const c_void, usize) -> Status,
    cuMemsetD8_v2: unsafe extern "C" fn(DevicePointer, u8, usize) -> Status,
    cuMemcpyDtoH_v2: unsafe extern "C" fn(*mut c_void, DevicePointer, usize) -> Status,
    cuModuleLoadData: unsafe extern "C" fn(*mut *mut c_void, *const c_void) -> Status,
    cuModuleGetFunction:
        unsafe extern "C" fn(*mut *mut c_void, *mut c_void, *const c_char) -> Status,
    #[allow(clippy::type_complexity)]
    cuLaunchKernel: unsafe extern "C" fn(
        *mut c_void,
        c_uint,
        c_uint,
        c_uint,
        c_uint,
        c_uint,
        c_uint,
        c_uint,
        *mut c_void,
        *mut *mut c_void,
        *mut *mut c_void,
    ) -> Status,
    nvrtcCreateProgram: unsafe extern "C" fn(
        *mut *mut c_void,
        *const c_char,
        *const c_char,
        c_int,
        *const *const c_char,
        *const *const c_char,
    ) -> Status,
    nvrtcCompileProgram: unsafe extern "C" fn(*mut c_void, c_int, *const *const c_char) -> Status,
    nvrtcGetPTXSize: unsafe extern "C" fn(*mut c_void, *mut usize) -> Status,
    nvrtcGetPTX: unsafe extern "C" fn(*mut c_void, *mut c_char) -> Status,
    nvrtcGetProgramLogSize: unsafe extern "C" fn(*mut c_void, *mut usize) -> Status,
    nvrtcGetProgramLog: unsafe extern "C" fn(*mut c_void, *mut c_char) -> Status,
    nvrtcDestroyProgram: unsafe extern "C" fn(*mut *mut c_void) -> Status,
    nvrtcVersion: unsafe extern "C" fn(*mut c_int, *mut c_int) -> Status,
    cublasCreate_v2: unsafe extern "C" fn(*mut *mut c_void) -> Status,
    #[allow(clippy::type_complexity)]
    cublasSgemm_v2: unsafe extern "C" fn(
        *mut c_void,
        c_int,
        c_int,
        c_int,
        c_int,
        c_int,
        *const f32,
        DevicePointer,
        c_int,
        DevicePointer,
        c_int,
        *const f32,
        DevicePointer,
        c_int,
    ) -> Status,
    #[allow(clippy::type_complexity)]
    cublasDgemm_v2: unsafe extern "C" fn(
        *mut c_void,
        c_int,
        c_int,
        c_int,
        c_int,
        c_int,
        *const f64,
        DevicePointer,
        c_int,
        DevicePointer,
        c_int,
        *const f64,
        DevicePointer,
        c_int,
    ) -> Status,
    #[allow(clippy::type_complexity)]
    cublasSgemmBatched: unsafe extern "C" fn(
        *mut c_void,
        c_int,
        c_int,
        c_int,
        c_int,
        c_int,
        *const f32,
        DevicePointer,
        c_int,
        DevicePointer,
        c_int,
        *const f32,
        DevicePointer,
        c_int,
        c_int,
    ) -> Status,
    #[allow(clippy::type_complexity)]
    cublasDgemmBatched: unsafe extern "C" fn(
        *mut c_void,
        c_int,
        c_int,
        c_int,
        c_int,
        c_int,
        *const f64,
        DevicePointer,
        c_int,
        DevicePointer,
        c_int,
        *const f64,
        DevicePointer,
        c_int,
        c_int,
    ) -> Status,
    #[allow(clippy::type_complexity)]
    cublasSgemmStridedBatched: unsafe extern "C" fn(
        *mut c_void,
        c_int,
        c_int,
        c_int,
        c_int,
        c_int,
        *const f32,
        DevicePointer,
        c_int,
        i64,
        DevicePointer,
        c_int,
        i64,
        *const f32,
        DevicePointer,
        c_int,
        i64,
        c_int,
    ) -> Status,
    #[allow(clippy::type_complexity)]
    cublasDgemmStridedBatched: unsafe extern "C" fn(
        *mut c_void,
        c_int,
        c_int,
        c_int,
        c_int,
        c_int,
        *const f64,
        DevicePointer,
        c_int,
        i64,
        DevicePointer,
        c_int,
        i64,
        *const f64,
        DevicePointer,
        c_int,
        i64,
        c_int,
    ) -> Status,
}

/// Opens the first of `names` the loader finds.
fn open(names: &[&str]) -> Option<*mut c_void> {
    names.iter().find_map(|name| {
        let name = CString::new(*name).ok()?;
        // SAFETY: `name` is a NUL-terminated path or library name.
        let handle = unsafe { libc::dlopen(name.as_ptr(), libc::RTLD_NOW | libc::RTLD_GLOBAL) };
        (!handle.is_null()).then_some(handle)
    })
}

/// The address of `name` in `library`.
fn symbol(library: *mut c_void, name: &str) -> Option<*mut c_void> {
    let name = CString::new(name).ok()?;
    // SAFETY: `library` is an open library and `name` is NUL-terminated.
    let address = unsafe { libc::dlsym(library, name.as_ptr()) };
    (!address.is_null()).then_some(address)
}

macro_rules! load {
    ($library:expr, $name:ident) => {{
        let address = symbol($library, stringify!($name))?;
        // SAFETY: the symbol is the CUDA function of this name, whose C
        // signature is the field's type.
        #[allow(clippy::missing_transmute_annotations)]
        unsafe {
            std::mem::transmute(address)
        }
    }};
}

impl Api {
    fn load() -> Option<Self> {
        // The driver library that matches the kernel driver is the system
        // one. A toolkit can carry a newer copy that the driver refuses.
        let driver = open(&[
            "/usr/lib/x86_64-linux-gnu/libcuda.so.1",
            "/usr/lib/aarch64-linux-gnu/libcuda.so.1",
            "/usr/lib64/libcuda.so.1",
            "libcuda.so.1",
            "libcuda.so",
        ])?;
        let versions = ["", ".13", ".12", ".11.2", ".11"];
        let candidates = |stem: &str| -> Vec<String> {
            let mut names = Vec::new();
            for version in versions {
                names.push(format!("{stem}.so{version}"));
            }
            for root in [
                std::env::var("CUDA_HOME").ok(),
                std::env::var("CUDA_PATH").ok(),
                Some("/usr/local/cuda".to_string()),
            ]
            .into_iter()
            .flatten()
            {
                for version in versions {
                    names.push(format!("{root}/lib64/{stem}.so{version}"));
                }
            }
            names
        };
        let open_library = |stem: &str| {
            let names = candidates(stem);
            open(&names.iter().map(String::as_str).collect::<Vec<_>>())
        };
        let nvrtc = open_library("libnvrtc")?;
        let cublas = open_library("libcublas")?;
        Some(Self {
            cuInit: load!(driver, cuInit),
            cuDeviceGetCount: load!(driver, cuDeviceGetCount),
            cuDeviceGet: load!(driver, cuDeviceGet),
            cuDevicePrimaryCtxRetain: load!(driver, cuDevicePrimaryCtxRetain),
            cuCtxSetCurrent: load!(driver, cuCtxSetCurrent),
            cuCtxSynchronize: load!(driver, cuCtxSynchronize),
            cuMemAlloc_v2: load!(driver, cuMemAlloc_v2),
            cuMemGetInfo_v2: load!(driver, cuMemGetInfo_v2),
            cuMemFree_v2: load!(driver, cuMemFree_v2),
            cuMemcpyHtoD_v2: load!(driver, cuMemcpyHtoD_v2),
            cuMemsetD8_v2: load!(driver, cuMemsetD8_v2),
            cuMemcpyDtoH_v2: load!(driver, cuMemcpyDtoH_v2),
            cuModuleLoadData: load!(driver, cuModuleLoadData),
            cuModuleGetFunction: load!(driver, cuModuleGetFunction),
            cuLaunchKernel: load!(driver, cuLaunchKernel),
            nvrtcCreateProgram: load!(nvrtc, nvrtcCreateProgram),
            nvrtcCompileProgram: load!(nvrtc, nvrtcCompileProgram),
            nvrtcGetPTXSize: load!(nvrtc, nvrtcGetPTXSize),
            nvrtcGetPTX: load!(nvrtc, nvrtcGetPTX),
            nvrtcGetProgramLogSize: load!(nvrtc, nvrtcGetProgramLogSize),
            nvrtcGetProgramLog: load!(nvrtc, nvrtcGetProgramLog),
            nvrtcDestroyProgram: load!(nvrtc, nvrtcDestroyProgram),
            nvrtcVersion: load!(nvrtc, nvrtcVersion),
            cublasCreate_v2: load!(cublas, cublasCreate_v2),
            cublasSgemm_v2: load!(cublas, cublasSgemm_v2),
            cublasDgemm_v2: load!(cublas, cublasDgemm_v2),
            cublasSgemmBatched: load!(cublas, cublasSgemmBatched),
            cublasDgemmBatched: load!(cublas, cublasDgemmBatched),
            cublasSgemmStridedBatched: load!(cublas, cublasSgemmStridedBatched),
            cublasDgemmStridedBatched: load!(cublas, cublasDgemmStridedBatched),
        })
    }
}

/// Buffers no tensor holds any more, by size, for the next tensor of that
/// size.
#[derive(Default)]
struct Pool {
    free: HashMap<usize, Vec<DevicePointer>>,
    bytes: usize,
    /// Runs under way. While there is one, a dropped buffer is always
    /// kept: freeing device memory would wait for the kernels launched
    /// so far.
    running: usize,
}

impl Pool {
    /// Takes buffers out until at most `keep` bytes are left, and gives
    /// them to be freed.
    fn trim(&mut self, keep: usize) -> Vec<DevicePointer> {
        let mut freed = Vec::new();
        // The largest sizes go first: they are the least likely to be
        // asked for again.
        let mut sizes: Vec<usize> = self.free.keys().copied().collect();
        sizes.sort_unstable();
        while self.bytes > keep {
            let Some(&size) = sizes.last() else {
                break;
            };
            match self.free.get_mut(&size).and_then(Vec::pop) {
                Some(pointer) => {
                    self.bytes -= size;
                    freed.push(pointer);
                }
                None => {
                    self.free.remove(&size);
                    sizes.pop();
                }
            }
        }
        freed
    }
}

/// The least memory the pool may keep between runs.
const MIN_POOLED_BYTES: usize = 2 << 30;

/// The most memory the pool keeps between runs on a device with `total`
/// bytes: a quarter of it. A pass of a model then finds the buffers of
/// the pass before it, and the rest of the device stays free for other
/// programs.
fn pooled_bytes(total: usize) -> usize {
    (total / 4).max(MIN_POOLED_BYTES)
}

/// A compiled plan: the kernels by name and the actions that run them.
struct Compiled {
    program: Program,
    functions: HashMap<String, usize>,
    /// For each action, the buffers no later action reads and the run
    /// does not give back: the run lets go of them once the action is
    /// launched, so a later action can use their memory.
    finished: Vec<Vec<usize>>,
}

struct Runtime {
    api: Api,
    context: *mut c_void,
    cublas: *mut c_void,
    /// The NVRTC version, part of the name of a cached kernel.
    compiler: (c_int, c_int),
    pool: Mutex<Pool>,
    /// Held for a whole run: the cuBLAS handle serves one thread at a
    /// time.
    compiled: Mutex<HashMap<Plan, Compiled>>,
    /// The small host leaves of earlier runs on the device, by their
    /// staged bytes.
    constants: Mutex<HashMap<Vec<u8>, Arc<Buffer>>>,
    /// The most bytes the pool keeps between runs.
    pooled: usize,
}

// SAFETY: the driver API and the context may be used from any thread, and
// the cuBLAS handle is used only under the `compiled` lock.
unsafe impl Send for Runtime {}
// SAFETY: as above.
unsafe impl Sync for Runtime {}

fn cuda_error(message: impl std::fmt::Display) -> Diagnostic {
    Diagnostic::coded("AU4007", format!("the cuda device {message}"))
}

fn check(status: Status, what: &str) -> Result<()> {
    if status == 0 {
        Ok(())
    } else {
        Err(cuda_error(format!("could not {what} (status {status})")))
    }
}

impl Runtime {
    fn start() -> Option<Self> {
        let api = Api::load()?;
        let mut count = 0;
        let mut device = 0;
        let mut context = std::ptr::null_mut();
        let mut cublas = std::ptr::null_mut();
        // SAFETY: each call follows the driver API: initialize, pick the
        // first device, and take its primary context, which cuBLAS shares.
        unsafe {
            if (api.cuInit)(0) != 0
                || (api.cuDeviceGetCount)(&mut count) != 0
                || count < 1
                || (api.cuDeviceGet)(&mut device, 0) != 0
                || (api.cuDevicePrimaryCtxRetain)(&mut context, device) != 0
                || (api.cuCtxSetCurrent)(context) != 0
                || (api.cublasCreate_v2)(&mut cublas) != 0
            {
                return None;
            }
        }
        let mut compiler = (0, 0);
        // SAFETY: both pointers are to live integers.
        unsafe { (api.nvrtcVersion)(&mut compiler.0, &mut compiler.1) };
        let (mut free, mut total) = (0_usize, 0_usize);
        // SAFETY: both pointers are to live integers, and the context is
        // current. A failed call leaves the total at zero.
        unsafe { (api.cuMemGetInfo_v2)(&mut free, &mut total) };
        Some(Self {
            pooled: pooled_bytes(total),
            api,
            context,
            cublas,
            compiler,
            pool: Mutex::default(),
            compiled: Mutex::default(),
            constants: Mutex::default(),
        })
    }

    /// Makes the context current on the calling thread. Every entry does
    /// this first, because a context is current for one thread.
    fn enter(&self) -> Result<()> {
        // SAFETY: `context` is the retained primary context.
        check(
            unsafe { (self.api.cuCtxSetCurrent)(self.context) },
            "use its context",
        )
    }

    fn pool(&self) -> std::sync::MutexGuard<'_, Pool> {
        self.pool
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

static RUNTIME: OnceLock<Option<Runtime>> = OnceLock::new();

fn runtime() -> Option<&'static Runtime> {
    RUNTIME.get_or_init(Runtime::start).as_ref()
}

fn started() -> Result<&'static Runtime> {
    runtime().ok_or_else(|| cuda_error("is not available on this machine"))
}

/// Whether this machine has an NVIDIA GPU with a driver, NVRTC and cuBLAS.
pub(crate) fn is_available() -> bool {
    runtime().is_some()
}

/// The elements of an Array as bytes.
fn storage_bytes(storage: &ArrayStorage) -> &[u8] {
    fn bytes_of<T>(values: &[T]) -> &[u8] {
        // SAFETY: the element types are plain numbers and `bool`, which
        // have no padding.
        unsafe { std::slice::from_raw_parts(values.as_ptr().cast(), std::mem::size_of_val(values)) }
    }
    match storage {
        ArrayStorage::Int32(values) => bytes_of::<i32>(values),
        ArrayStorage::Int64(values) => bytes_of::<i64>(values),
        ArrayStorage::Float32(values) => bytes_of::<f32>(values),
        ArrayStorage::Float64(values) => bytes_of::<f64>(values),
        ArrayStorage::Bool(values) => bytes_of::<bool>(values),
    }
}

use super::tensor_host::SMALL_LEAF_BYTES as MAX_STAGED_LEAF_BYTES;

/// CUDA device memory.
pub(crate) struct Buffer {
    pointer: DevicePointer,
    bytes: usize,
    /// The buffer this one is a part of, which owns the memory.
    part_of: Option<Arc<Buffer>>,
}

impl Drop for Buffer {
    fn drop(&mut self) {
        // A buffer exists only after the runtime started. Reading the
        // started runtime, and not starting one, keeps the code that
        // loads the CUDA libraries out of a program that makes no tensor:
        // every program can drop a value, and a value can hold a tensor.
        let Some(runtime) = RUNTIME.get().and_then(Option::as_ref) else {
            return;
        };
        if self.part_of.is_some() {
            return;
        }
        // The pool keeps every dropped buffer. Freeing one here would
        // wait for the kernels launched so far, in the middle of the
        // program. The run the program next waits for trims the pool.
        let mut pool = runtime.pool();
        pool.bytes += self.bytes;
        pool.free.entry(self.bytes).or_default().push(self.pointer);
    }
}

impl Buffer {
    /// A buffer of `bytes` bytes whose contents are not set.
    fn new(bytes: usize) -> Result<Self> {
        let runtime = started()?;
        let pooled = {
            let mut pool = runtime.pool();
            let pointer = pool.free.get_mut(&bytes).and_then(Vec::pop);
            if pointer.is_some() {
                pool.bytes -= bytes;
            }
            pointer
        };
        if let Some(pointer) = pooled {
            return Ok(Self {
                pointer,
                bytes,
                part_of: None,
            });
        }
        runtime.enter()?;
        let mut pointer = 0;
        // SAFETY: `pointer` receives the allocation.
        let mut status = unsafe { (runtime.api.cuMemAlloc_v2)(&mut pointer, bytes) };
        if status != 0 {
            // The pool may hold the memory this buffer needs.
            let freed = runtime.pool().trim(0);
            for pointer in freed {
                // SAFETY: the pool owned the allocation, and freeing waits
                // for any kernel still using it.
                unsafe { (runtime.api.cuMemFree_v2)(pointer) };
            }
            // SAFETY: `pointer` receives the allocation.
            status = unsafe { (runtime.api.cuMemAlloc_v2)(&mut pointer, bytes) };
        }
        check(status, &format!("allocate {bytes} bytes"))?;
        Ok(Self {
            pointer,
            bytes,
            part_of: None,
        })
    }

    /// A buffer holding the elements of a non-empty Array in row-major
    /// order.
    pub(crate) fn from_array(elements: &ArrayValue) -> Result<Self> {
        let storage = elements.elements()?;
        Self::from_bytes(storage_bytes(&storage))
    }

    /// A buffer holding `bytes`.
    fn from_bytes(bytes: &[u8]) -> Result<Self> {
        let runtime = started()?;
        let buffer = Self::new(bytes.len())?;
        runtime.enter()?;
        // SAFETY: the buffer holds `bytes.len()` bytes.
        let status = unsafe {
            (runtime.api.cuMemcpyHtoD_v2)(buffer.pointer, bytes.as_ptr().cast(), bytes.len())
        };
        check(status, "copy elements to the device")?;
        Ok(buffer)
    }

    /// The `bytes` bytes of `whole` that start `offset` bytes in.
    fn part(whole: &Arc<Buffer>, offset: usize, bytes: usize) -> Self {
        Self {
            pointer: whole.pointer + offset as DevicePointer,
            bytes,
            part_of: Some(Arc::clone(whole)),
        }
    }

    /// An Array holding a copy of the buffer's elements.
    pub(crate) fn to_array(&self, dtype: ArrayDType, shape: &[usize]) -> Result<ArrayValue> {
        let runtime = started()?;
        let len: usize = shape.iter().product();
        assert_eq!(
            len * tensor_codegen::element_bytes(dtype),
            self.bytes,
            "a tensor's buffer holds its shape"
        );
        runtime.enter()?;
        fn read<T: Copy + Default>(
            runtime: &Runtime,
            buffer: &Buffer,
            len: usize,
        ) -> Result<Box<[T]>> {
            let mut values = vec![T::default(); len];
            // SAFETY: the buffer holds `len` elements of `T`, and `values`
            // has room for them.
            let status = unsafe {
                (runtime.api.cuMemcpyDtoH_v2)(
                    values.as_mut_ptr().cast(),
                    buffer.pointer,
                    len * std::mem::size_of::<T>(),
                )
            };
            check(status, "copy elements from the device")?;
            Ok(values.into())
        }
        let storage = match dtype {
            ArrayDType::Int32 => ArrayStorage::Int32(read::<i32>(runtime, self, len)?.into()),
            ArrayDType::Int64 => ArrayStorage::Int64(read::<i64>(runtime, self, len)?.into()),
            ArrayDType::Float32 => ArrayStorage::Float32(read::<f32>(runtime, self, len)?.into()),
            ArrayDType::Float64 => ArrayStorage::Float64(read::<f64>(runtime, self, len)?.into()),
            ArrayDType::Bool => {
                let flags: Box<[bool]> = read::<u8>(runtime, self, len)?
                    .iter()
                    .map(|&byte| byte != 0)
                    .collect();
                ArrayStorage::Bool(flags.into())
            }
        };
        ArrayValue::new(shape.into(), storage)
    }
}

/// The file that keeps the compiled form of `source` between runs of a
/// program: `cuda-kernels` under `AURA_CACHE_DIR` when it is set, else
/// under `~/.cache/aura`. The name is a hash of the source and of the
/// compiler version, so a new compiler or a changed kernel is a new file.
fn kernel_cache_file(source: &str, compiler: (c_int, c_int)) -> Option<std::path::PathBuf> {
    use sha2::{Digest, Sha256};
    let root = match std::env::var_os("AURA_CACHE_DIR").filter(|dir| !dir.is_empty()) {
        Some(dir) => std::path::PathBuf::from(dir),
        None => std::path::PathBuf::from(std::env::var_os("HOME").filter(|home| !home.is_empty())?)
            .join(".cache")
            .join("aura"),
    };
    let mut hash = Sha256::new();
    hash.update(format!(
        "nvrtc {}.{} {COMPILE_OPTION}\n",
        compiler.0, compiler.1
    ));
    hash.update(source);
    let name: String = hash
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    Some(root.join("cuda-kernels").join(format!("{name}.ptx")))
}

/// Keeps `image` in `file`. The cache only saves time, so a failure to
/// write it is not an error. The image is written beside the file and
/// renamed, so another process never reads half of it.
fn keep_kernel_image(file: &std::path::Path, image: &[u8]) {
    let Some(directory) = file.parent() else {
        return;
    };
    if std::fs::create_dir_all(directory).is_err() {
        return;
    }
    let partial = file.with_extension(format!("{}.partial", std::process::id()));
    if std::fs::write(&partial, image).is_err() || std::fs::rename(&partial, file).is_err() {
        let _ = std::fs::remove_file(&partial);
    }
}

/// A fused multiply-add would round differently from the CPU device,
/// whose `a * b + c` rounds twice.
const COMPILE_OPTION: &str = "--fmad=false";

impl Compiled {
    /// The PTX image NVRTC compiles `source` to.
    fn compile(runtime: &Runtime, source: &str) -> Result<Vec<u8>> {
        let source =
            CString::new(source).map_err(|_| cuda_error("made a kernel source with a NUL byte"))?;
        let api = &runtime.api;
        let mut handle = std::ptr::null_mut();
        // SAFETY: `source` and the name are NUL-terminated and there are
        // no headers.
        let status = unsafe {
            (api.nvrtcCreateProgram)(
                &mut handle,
                source.as_ptr(),
                c"aura_plan.cu".as_ptr(),
                0,
                std::ptr::null(),
                std::ptr::null(),
            )
        };
        check(status, "start a kernel compile")?;
        let option = CString::new(COMPILE_OPTION).expect("the option has no NUL byte");
        let options = [option.as_ptr()];
        // SAFETY: `handle` is the program just created.
        let status = unsafe { (api.nvrtcCompileProgram)(handle, 1, options.as_ptr()) };
        let image = if status == 0 {
            let mut size = 0;
            // SAFETY: `image` has the size NVRTC reports.
            unsafe {
                (api.nvrtcGetPTXSize)(handle, &mut size);
                let mut image = vec![0_u8; size.max(1)];
                (api.nvrtcGetPTX)(handle, image.as_mut_ptr().cast());
                Ok(image)
            }
        } else {
            let mut size = 0;
            // SAFETY: `log` has the size NVRTC reports.
            let log = unsafe {
                (api.nvrtcGetProgramLogSize)(handle, &mut size);
                let mut log = vec![0_u8; size.max(1)];
                (api.nvrtcGetProgramLog)(handle, log.as_mut_ptr().cast());
                CStr::from_bytes_until_nul(&log)
                    .map(|log| log.to_string_lossy().into_owned())
                    .unwrap_or_default()
            };
            Err(cuda_error(format!(
                "could not compile a kernel (status {status}): {log}"
            )))
        };
        // SAFETY: `handle` is a live program.
        unsafe { (api.nvrtcDestroyProgram)(&mut handle) };
        image
    }

    /// The loaded module of a PTX `image`, or `None` when the driver does
    /// not take it.
    fn load(runtime: &Runtime, image: &[u8]) -> Option<*mut c_void> {
        // The driver reads the image as a NUL-terminated string.
        if image.last() != Some(&0) {
            return None;
        }
        let mut module = std::ptr::null_mut();
        // SAFETY: `image` is NUL-terminated.
        let status = unsafe { (runtime.api.cuModuleLoadData)(&mut module, image.as_ptr().cast()) };
        (status == 0).then_some(module)
    }

    fn new(runtime: &Runtime, plan: &Plan) -> Result<Self> {
        let program = tensor_codegen::generate(plan);
        let api = &runtime.api;
        // A kernel compiled by an earlier run of any program is read from
        // the cache. A file the driver does not take is compiled again.
        let file = kernel_cache_file(&program.source, runtime.compiler);
        let kept = file
            .as_deref()
            .and_then(|file| std::fs::read(file).ok())
            .and_then(|image| Self::load(runtime, &image));
        let module = match kept {
            Some(module) => module,
            None => {
                let image = Self::compile(runtime, &program.source)?;
                let module = Self::load(runtime, &image)
                    .ok_or_else(|| cuda_error("could not load a compiled kernel"))?;
                if let Some(file) = &file {
                    keep_kernel_image(file, &image);
                }
                module
            }
        };
        let mut functions = HashMap::new();
        for action in &program.actions {
            if let Action::Kernel { name, .. } = action {
                let symbol = CString::new(name.as_str()).expect("a kernel name has no NUL byte");
                let mut function = std::ptr::null_mut();
                // SAFETY: `module` is loaded and `symbol` is NUL-terminated.
                let status =
                    unsafe { (api.cuModuleGetFunction)(&mut function, module, symbol.as_ptr()) };
                check(status, "find a compiled kernel")?;
                functions.insert(name.clone(), function as usize);
            }
        }
        let finished = tensor_codegen::finished_buffers(&program, plan);
        Ok(Self {
            program,
            functions,
            finished,
        })
    }
}

/// The times of one traced run.
struct Traced {
    known: bool,
    /// Host time since the last traced event: building the expression,
    /// collecting the plan, and everything else the program did.
    host: f64,
    prepared: std::time::Duration,
    launched: std::time::Duration,
    waited: std::time::Duration,
    /// Leaves copied from host memory for this run, and their elements.
    copied: usize,
    copied_elements: usize,
}

/// Writes one traced run: its size, the host time before it, the time to
/// find or compile its kernels, to launch its actions and to wait for
/// them, and with the fuller trace its actions by kind and its slowest
/// actions.
fn report(plan: &Plan, traced: &Traced, timings: &mut [(String, std::time::Duration)]) {
    let millis = |duration: std::time::Duration| duration.as_secs_f64() * 1000.0;
    let of = |kind: &str| -> (usize, f64) {
        let chosen = timings.iter().filter(|(label, _)| label.starts_with(kind));
        (
            chosen.clone().count(),
            chosen.fold(0.0, |sum, (_, time)| sum + millis(*time)),
        )
    };
    let (kernels, kernel_time) = of("kernel");
    let (products, product_time) = of("matmul");
    eprintln!(
        "aura tensor: plan slots={} outputs={} {} host={:.2}ms prepare={:.2}ms launch={:.2}ms wait={:.2}ms copied={} of {} elements kernels={kernels} in {kernel_time:.2}ms matmuls={products} in {product_time:.2}ms",
        plan.slots.len(),
        plan.outputs.len(),
        if traced.known { "known" } else { "new" },
        traced.host,
        millis(traced.prepared),
        millis(traced.launched),
        millis(traced.waited),
        traced.copied,
        traced.copied_elements,
    );
    let level = super::tensor_host::trace_level();
    if level != 2 {
        timings.sort_by_key(|entry| std::cmp::Reverse(entry.1));
        let shown = if level == 3 { timings.len() } else { 6 };
        for (label, time) in timings.iter().take(shown) {
            eprintln!("aura tensor:   {:.2}ms {label}", millis(*time));
        }
    }
}

/// The most staged sets of small leaves kept on the device. Each is a
/// few kilobytes at most.
const MAX_KEPT_CONSTANTS: usize = 512;

/// The device copy of `staged`, kept between runs by its bytes.
fn kept_constant(runtime: &Runtime, staged: &[u8]) -> Result<Arc<Buffer>> {
    let mut constants = runtime
        .constants
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if let Some(whole) = constants.get(staged) {
        return Ok(Arc::clone(whole));
    }
    if constants.len() >= MAX_KEPT_CONSTANTS {
        constants.clear();
    }
    let whole = Arc::new(Buffer::from_bytes(staged)?);
    constants.insert(staged.to_vec(), Arc::clone(&whole));
    Ok(whole)
}

/// The most compiled plans kept.
const MAX_COMPILED_PLANS: usize = 256;

/// Runs `plan` over `leaves` and gives the buffer holding its result.
pub(crate) fn run(plan: &Plan, leaves: &[Storage]) -> Result<Vec<Storage>> {
    let runtime = started()?;
    runtime.enter()?;
    let api = &runtime.api;
    let trace = super::tensor_host::trace_level();
    let host = if trace > 0 {
        super::tensor_host::trace_gap()
    } else {
        0.0
    };
    let began = std::time::Instant::now();
    let mut compiled = runtime
        .compiled
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let known = compiled.contains_key(plan);
    if !known {
        if compiled.len() >= MAX_COMPILED_PLANS {
            compiled.clear();
        }
        compiled.insert(plan.clone(), Compiled::new(runtime, plan)?);
    }
    let compiled = &compiled[plan];
    let prepared = began.elapsed();
    let mut timings: Vec<(String, std::time::Duration)> = Vec::new();
    let mut buffers: Vec<Option<Arc<Buffer>>> = vec![None; compiled.program.buffers];
    let mut leaves = leaves.iter();
    let mut copied = 0_usize;
    let mut copied_elements = 0_usize;
    // The small leaves in host memory, such as the numbers of an
    // expression, go to the device as one copy: their bytes one after
    // another, each starting at a multiple of eight.
    let mut staged: Vec<u8> = Vec::new();
    let mut parts: Vec<(usize, usize, usize)> = Vec::new();
    for (index, (slot, buffer)) in plan.slots.iter().zip(buffers.iter_mut()).enumerate() {
        if matches!(slot, Slot::Leaf { .. }) {
            match leaves.next().expect("a plan has memory for each leaf") {
                Storage::Cuda(held) => *buffer = Some(Arc::clone(held)),
                // A leaf in host memory is copied to the device for this
                // run.
                Storage::Host(elements) => {
                    copied += 1;
                    copied_elements += elements.len();
                    let storage = elements.elements()?;
                    let bytes = storage_bytes(&storage);
                    if bytes.len() <= MAX_STAGED_LEAF_BYTES {
                        staged.resize(staged.len().next_multiple_of(8), 0);
                        parts.push((index, staged.len(), bytes.len()));
                        staged.extend_from_slice(bytes);
                    } else {
                        *buffer = Some(Arc::new(Buffer::from_bytes(bytes)?));
                    }
                }
            }
        }
    }
    if !parts.is_empty() {
        // The numbers of an expression are the same on every pass of a
        // loop, so their copy on the device is kept. A copy waits for
        // the kernels launched so far, which would stop a run started
        // early from running beside the program.
        let whole = kept_constant(runtime, &staged)?;
        for (index, offset, bytes) in parts {
            buffers[index] = Some(Arc::new(Buffer::part(&whole, offset, bytes)));
        }
    }
    let pointer = |buffers: &[Option<Arc<Buffer>>], index: usize| {
        buffers[index]
            .as_ref()
            .expect("an action reads buffers made before it")
            .pointer
    };
    let mut outcome = Ok(());
    // The address tables of batched products, kept until the run ends.
    let mut tables: Vec<Arc<Buffer>> = Vec::new();
    runtime.pool().running += 1;
    for (index, action) in compiled.program.actions.iter().enumerate() {
        let action_began = std::time::Instant::now();
        outcome = match action {
            Action::Kernel {
                name,
                output,
                bytes,
                threads,
                inputs,
                clear,
                more,
            } => Buffer::new(*bytes).and_then(|made| {
                if *clear {
                    // SAFETY: the buffer holds `bytes` bytes.
                    let status = unsafe { (api.cuMemsetD8_v2)(made.pointer, 0, *bytes) };
                    check(status, "clear a buffer")?;
                }
                let mut arguments: Vec<DevicePointer> = vec![made.pointer];
                for &(other, bytes) in more {
                    let beside = Buffer::new(bytes)?;
                    arguments.push(beside.pointer);
                    buffers[other] = Some(Arc::new(beside));
                }
                arguments.extend(inputs.iter().map(|&input| pointer(&buffers, input)));
                let mut addresses: Vec<*mut c_void> = arguments
                    .iter_mut()
                    .map(|argument| std::ptr::from_mut(argument).cast())
                    .collect();
                let blocks = threads.div_ceil(BLOCK_THREADS).max(1) as c_uint;
                buffers[*output] = Some(Arc::new(made));
                // SAFETY: the kernel takes one pointer for each entry of
                // `addresses`, and every buffer is alive and large enough
                // for the shapes the plan compiled in.
                let status = unsafe {
                    (api.cuLaunchKernel)(
                        compiled.functions[name] as *mut c_void,
                        blocks,
                        1,
                        1,
                        BLOCK_THREADS as c_uint,
                        1,
                        1,
                        0,
                        std::ptr::null_mut(),
                        addresses.as_mut_ptr(),
                        std::ptr::null_mut(),
                    )
                };
                check(status, "launch a kernel")
            }),
            Action::Matmul {
                left,
                right,
                output,
                bytes,
                double,
                rows,
                inner,
                columns,
                pairs,
                swapped,
                spread,
            } => Buffer::new(*bytes).and_then(|made| {
                let (left, right) = (pointer(&buffers, *left), pointer(&buffers, *right));
                let element = if *double { 8 } else { 4 };
                let (rows, inner, columns) = (*rows as c_int, *inner as c_int, *columns as c_int);
                // cuBLAS reads a matrix stored with its rows and columns
                // swapped when told so: the operation and how far apart
                // its columns are, for the right and the left operand.
                let (right_op, right_step): (c_int, c_int) =
                    if swapped.1 { (1, inner) } else { (0, columns) };
                let (left_op, left_step): (c_int, c_int) =
                    if swapped.0 { (1, rows) } else { (0, inner) };
                let product = (rows as usize) * (columns as usize) * element;
                let mut status = 0;
                let count = pairs.len();
                let stepped = |side: fn(&(usize, usize)) -> usize, step: usize| {
                    pairs
                        .iter()
                        .enumerate()
                        .all(|(index, pair)| side(pair) == index * step)
                };
                let left_stacked = stepped(|pair| pair.0, (rows * inner) as usize);
                let right_shared = pairs.iter().all(|pair| pair.1 == 0);
                let right_stacked = stepped(|pair| pair.1, (inner * columns) as usize);
                let tall = rows as usize * count;
                if let Some((step, starts)) = spread {
                    // The products go beside each other in wider rows,
                    // so each has its own place: one batched call takes
                    // the address of every matrix. The addresses repeat
                    // from run to run, since the pool hands out the same
                    // buffers, and their copy on the device is kept.
                    let mut addresses = Vec::with_capacity(count * 24);
                    let place = |base: DevicePointer, offset: usize| {
                        (base + (offset * element) as DevicePointer).to_ne_bytes()
                    };
                    for pair in pairs {
                        addresses.extend(place(right, pair.1));
                    }
                    for pair in pairs {
                        addresses.extend(place(left, pair.0));
                    }
                    for &start in starts {
                        addresses.extend(place(made.pointer, start));
                    }
                    match kept_constant(runtime, &addresses) {
                        Ok(table) => {
                            let each = (count * 8) as DevicePointer;
                            let (a, b, c) = (
                                table.pointer,
                                table.pointer + each,
                                table.pointer + 2 * each,
                            );
                            // SAFETY: each address is a whole matrix
                            // inside its buffer, and the rows of a product
                            // are `step` elements apart in the result.
                            status = unsafe {
                                if *double {
                                    (api.cublasDgemmBatched)(
                                        runtime.cublas,
                                        right_op,
                                        left_op,
                                        columns,
                                        rows,
                                        inner,
                                        &1.0,
                                        a,
                                        right_step,
                                        b,
                                        left_step,
                                        &0.0,
                                        c,
                                        *step as c_int,
                                        count as c_int,
                                    )
                                } else {
                                    (api.cublasSgemmBatched)(
                                        runtime.cublas,
                                        right_op,
                                        left_op,
                                        columns,
                                        rows,
                                        inner,
                                        &1.0,
                                        a,
                                        right_step,
                                        b,
                                        left_step,
                                        &0.0,
                                        c,
                                        *step as c_int,
                                        count as c_int,
                                    )
                                }
                            };
                            tables.push(table);
                        }
                        Err(error) => {
                            buffers[*output] = Some(Arc::new(made));
                            return Err(error);
                        }
                    }
                } else if count > 1
                    && left_stacked
                    && right_shared
                    && !swapped.0
                    && tall <= c_int::MAX as usize
                {
                    // A stack of matrices times one shared matrix is one
                    // product with the stack read as a single tall matrix.
                    let tall = tall as c_int;
                    // SAFETY: the left buffer holds `count` matrices in a
                    // row and the output holds as many products.
                    status = unsafe {
                        if *double {
                            (api.cublasDgemm_v2)(
                                runtime.cublas,
                                right_op,
                                left_op,
                                columns,
                                tall,
                                inner,
                                &1.0,
                                right,
                                right_step,
                                left,
                                left_step,
                                &0.0,
                                made.pointer,
                                columns,
                            )
                        } else {
                            (api.cublasSgemm_v2)(
                                runtime.cublas,
                                right_op,
                                left_op,
                                columns,
                                tall,
                                inner,
                                &1.0,
                                right,
                                right_step,
                                left,
                                left_step,
                                &0.0,
                                made.pointer,
                                columns,
                            )
                        }
                    };
                } else if count > 1 && left_stacked && right_stacked && count <= c_int::MAX as usize
                {
                    // Matching stacks on both sides are one batched call.
                    let (a_step, b_step, c_step) = (
                        i64::from(inner) * i64::from(columns),
                        i64::from(rows) * i64::from(inner),
                        i64::from(rows) * i64::from(columns),
                    );
                    // SAFETY: every buffer holds `count` whole matrices
                    // spaced by its step.
                    status = unsafe {
                        if *double {
                            (api.cublasDgemmStridedBatched)(
                                runtime.cublas,
                                right_op,
                                left_op,
                                columns,
                                rows,
                                inner,
                                &1.0,
                                right,
                                right_step,
                                a_step,
                                left,
                                left_step,
                                b_step,
                                &0.0,
                                made.pointer,
                                columns,
                                c_step,
                                count as c_int,
                            )
                        } else {
                            (api.cublasSgemmStridedBatched)(
                                runtime.cublas,
                                right_op,
                                left_op,
                                columns,
                                rows,
                                inner,
                                &1.0,
                                right,
                                right_step,
                                a_step,
                                left,
                                left_step,
                                b_step,
                                &0.0,
                                made.pointer,
                                columns,
                                c_step,
                                count as c_int,
                            )
                        }
                    };
                } else {
                    for (index, (left_offset, right_offset)) in pairs.iter().enumerate() {
                        // cuBLAS matrices are column-major. A row-major product
                        // A B is the column-major product of B and A as stored,
                        // which is the transpose of the same result.
                        let a = right + (right_offset * element) as u64;
                        let b = left + (left_offset * element) as u64;
                        let c = made.pointer + (index * product) as u64;
                        // SAFETY: each pointer addresses a whole matrix of the
                        // given size inside its buffer.
                        status = unsafe {
                            if *double {
                                (api.cublasDgemm_v2)(
                                    runtime.cublas,
                                    right_op,
                                    left_op,
                                    columns,
                                    rows,
                                    inner,
                                    &1.0,
                                    a,
                                    right_step,
                                    b,
                                    left_step,
                                    &0.0,
                                    c,
                                    columns,
                                )
                            } else {
                                (api.cublasSgemm_v2)(
                                    runtime.cublas,
                                    right_op,
                                    left_op,
                                    columns,
                                    rows,
                                    inner,
                                    &1.0,
                                    a,
                                    right_step,
                                    b,
                                    left_step,
                                    &0.0,
                                    c,
                                    columns,
                                )
                            }
                        };
                        if status != 0 {
                            break;
                        }
                    }
                }
                buffers[*output] = Some(Arc::new(made));
                check(status, "multiply matrices")
            }),
            Action::Alias { output, input } => {
                buffers[*output] = buffers[*input].clone();
                Ok(())
            }
        };
        if outcome.is_err() {
            break;
        }
        // Later actions run after this one on the device, so they can
        // write over what only this one and earlier ones read.
        for &buffer in &compiled.finished[index] {
            buffers[buffer] = None;
        }
        if trace > 0 {
            if trace != 2 {
                // Waiting after each action gives the action its own time.
                // SAFETY: the context is current on this thread.
                unsafe { (api.cuCtxSynchronize)() };
            }
            let label = match action {
                Action::Kernel {
                    name,
                    bytes,
                    threads,
                    ..
                } => format!("kernel {name} bytes={bytes} threads={threads}"),
                Action::Matmul {
                    rows,
                    inner,
                    columns,
                    pairs,
                    ..
                } => format!("matmul {rows}x{inner}x{columns} products={}", pairs.len()),
                Action::Alias { .. } => continue,
            };
            timings.push((label, action_began.elapsed()));
        }
    }
    let launched = began.elapsed();
    // The kernels run after the launch returns. A run started early
    // leaves them running: every later launch, copy and wait on this
    // device comes after them. Any other run waits here, before the
    // program reads a result, and the traces that time each action wait
    // for every run.
    let waits = !super::tensor_host::runs_early() || (trace > 0 && trace != 2);
    let finished = if waits {
        // SAFETY: the context is current on this thread.
        check(unsafe { (api.cuCtxSynchronize)() }, "finish a run")
    } else {
        Ok(())
    };
    let freed = {
        let mut pool = runtime.pool();
        pool.running -= 1;
        if pool.running == 0 && waits {
            pool.trim(runtime.pooled)
        } else {
            Vec::new()
        }
    };
    for pointer in freed {
        // SAFETY: the pool owned the allocation and no kernel is running.
        unsafe { (api.cuMemFree_v2)(pointer) };
    }
    if trace > 0 {
        let traced = Traced {
            known,
            host,
            prepared,
            launched,
            waited: began.elapsed() - launched,
            copied,
            copied_elements,
        };
        report(plan, &traced, &mut timings);
        super::tensor_host::trace_gap();
    }
    outcome?;
    finished?;
    Ok(plan
        .outputs
        .iter()
        .map(|&output| {
            Storage::Cuda(Arc::clone(
                buffers[output]
                    .as_ref()
                    .expect("an action writes each output of a plan"),
            ))
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_pool_keeps_a_quarter_of_a_device_and_two_gibibytes_at_least() {
        assert_eq!(pooled_bytes(48 << 30), 12 << 30);
        assert_eq!(pooled_bytes(4 << 30), MIN_POOLED_BYTES);
        assert_eq!(pooled_bytes(0), MIN_POOLED_BYTES);
    }

    #[test]
    fn a_kernel_image_is_kept_under_a_name_made_from_its_source_and_compiler() {
        let one = kernel_cache_file("kernel one", (13, 2)).expect("the tests run with a home");
        let same = kernel_cache_file("kernel one", (13, 2)).unwrap();
        let other_source = kernel_cache_file("kernel two", (13, 2)).unwrap();
        let other_compiler = kernel_cache_file("kernel one", (13, 3)).unwrap();
        assert_eq!(one, same);
        assert_ne!(one, other_source);
        assert_ne!(one, other_compiler);
        assert_eq!(one.extension().unwrap(), "ptx");
        assert_eq!(one.parent().unwrap().file_name().unwrap(), "cuda-kernels");

        let directory = std::env::temp_dir().join(format!("aura-kernels-{}", std::process::id()));
        let file = directory.join("deep").join("image.ptx");
        keep_kernel_image(&file, b"image\0");
        assert_eq!(std::fs::read(&file).unwrap(), b"image\0");
        keep_kernel_image(&file, b"again\0");
        assert_eq!(std::fs::read(&file).unwrap(), b"again\0");
        assert_eq!(
            std::fs::read_dir(file.parent().unwrap()).unwrap().count(),
            1,
            "no partial file is left"
        );
        std::fs::remove_dir_all(&directory).unwrap();
    }
}
