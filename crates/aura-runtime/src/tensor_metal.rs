//! The Metal device of the `tensor` module.
//!
//! A tensor on this device is an `MTLBuffer` Aura owns. A plan becomes one
//! MPSGraph, which reads its leaves from their buffers and writes its
//! result into a buffer made for it, so nothing is copied and a kernel
//! compiled later can use the same memory. The Objective-C side is
//! `metal_shim.m`. It loads Metal when a program first asks for the
//! device, so a program that never does pays nothing.

use super::{Plan, Slot, Storage, NEGATE_OPERATION, PAIR_OPERATION_BASE};
use crate::runtime_value::*;
use std::collections::HashMap;
use std::ffi::c_void;
use std::sync::{Arc, Mutex, OnceLock};

extern "C" {
    fn aura_metal_available() -> i32;
    fn aura_metal_indexed() -> i32;
    fn aura_metal_buffer_new(bytes: usize) -> *mut c_void;
    fn aura_metal_buffer_contents(buffer: *mut c_void) -> *mut u8;
    fn aura_metal_buffer_release(buffer: *mut c_void);
    fn aura_metal_graph_new() -> *mut c_void;
    fn aura_metal_graph_release(graph: *mut c_void);
    fn aura_metal_graph_placeholder(
        graph: *mut c_void,
        dtype: i32,
        rank: i32,
        shape: *const i64,
    ) -> i32;
    #[allow(clippy::too_many_arguments)]
    fn aura_metal_graph_step(
        graph: *mut c_void,
        kind: i32,
        code: i32,
        inputs: *const i32,
        dtype: i32,
        rank: i32,
        shape: *const i64,
        count: i32,
        numbers: *const i64,
    ) -> i32;
    fn aura_metal_graph_run(
        graph: *mut c_void,
        feeds: i32,
        placeholders: *const i32,
        buffers: *const *mut c_void,
        count: i32,
        targets: *const i32,
        outputs: *const *mut c_void,
    ) -> i32;
}

/// Whether this machine has a Metal device that can run a graph.
pub(crate) fn is_available() -> bool {
    static AVAILABLE: OnceLock<bool> = OnceLock::new();
    // SAFETY: the shim call takes no arguments and may run on any thread.
    *AVAILABLE.get_or_init(|| unsafe { aura_metal_available() } == 1)
}

/// Whether the device runs the operations that move elements by index:
/// slices, broadcasts to a shape, joins, index selects and adds, and the
/// place of a maximum. They need macOS 12.
pub(crate) fn moves_by_index() -> bool {
    static INDEXED: OnceLock<bool> = OnceLock::new();
    // SAFETY: the shim call takes no arguments and may run on any thread.
    *INDEXED.get_or_init(|| unsafe { aura_metal_indexed() } == 1)
}

/// Whether the device makes random tensors. The random bits are 64-bit
/// integer arithmetic, which the library must do exactly, so the first
/// call makes a few floats on the device and compares them with the
/// floats of the CPU kernel: the uniform ones bit for bit, and the normal
/// ones within the rounding of the library's own logarithm and cosine. A
/// device that differs, or cannot build the graph, leaves random tensors
/// to the CPU kernel.
pub(crate) fn makes_random() -> bool {
    static RANDOM: OnceLock<bool> = OnceLock::new();
    *RANDOM.get_or_init(|| {
        const COUNT: usize = 96;
        let agrees = |fill: i64, seed: i64| -> Result<bool> {
            let plan = Plan {
                slots: vec![
                    Slot::Leaf {
                        dtype: ArrayDType::Int64 as i64,
                        shape: vec![1],
                    },
                    Slot::Step {
                        operation: super::Operation::Random {
                            fill,
                            dtype: ArrayDType::Float32 as i64,
                            shape: vec![COUNT],
                        },
                        inputs: vec![0],
                        dtype: ArrayDType::Float32 as i64,
                        shape: vec![COUNT],
                    },
                ],
                outputs: vec![1],
            };
            let leaf = ArrayValue::new(Box::new([1]), ArrayStorage::Int64(vec![seed].into()))?;
            let storage = run(&plan, &[Storage::Host(leaf)])?
                .pop()
                .expect("a plan gives its result");
            let found = storage.to_host(ArrayDType::Float32 as i64, &[COUNT])?;
            let kernel = if fill == 0 {
                super::tensor_kernels::Fill::Uniform
            } else {
                super::tensor_kernels::Fill::Normal
            };
            let expected = super::tensor_kernels::filled(
                kernel,
                Box::new([COUNT]),
                seed as u64,
                ArrayDType::Float32,
            )?;
            let same = match (&*found.elements()?, &*expected.elements()?) {
                (ArrayStorage::Float32(found), ArrayStorage::Float32(expected)) if fill == 0 => {
                    found[..] == expected[..]
                }
                (ArrayStorage::Float32(found), ArrayStorage::Float32(expected)) => {
                    found.iter().zip(expected.iter()).all(|(found, expected)| {
                        (found - expected).abs() <= 8.0 * f32::EPSILON * expected.abs().max(1.0)
                    })
                }
                _ => false,
            };
            Ok(same)
        };
        moves_by_index()
            && [0_i64, 1].into_iter().all(|fill| {
                [7_i64, -3, i64::MIN + 12_345]
                    .into_iter()
                    .all(|seed| agrees(fill, seed).unwrap_or(false))
            })
    })
}

fn metal_error(message: &str) -> Diagnostic {
    Diagnostic::coded("AU4007", format!("the metal device {message}"))
}

/// An `MTLBuffer` in shared storage, which the CPU and the GPU both
/// address.
pub(crate) struct Buffer {
    handle: *mut c_void,
    bytes: usize,
}

// SAFETY: an `MTLBuffer` may be retained, released and read on any thread,
// and a buffer is written only before it is shared.
unsafe impl Send for Buffer {}
// SAFETY: as above.
unsafe impl Sync for Buffer {}

/// Buffers no tensor holds any more, by size. Allocating a large buffer
/// costs more than the arithmetic that fills it, and a loop makes and
/// drops buffers of the same few sizes, so a dropped buffer waits here for
/// the next tensor of its size.
struct Pool {
    free: HashMap<usize, Vec<usize>>,
    bytes: usize,
}

/// The most memory the pool keeps. A buffer dropped beyond it is released.
const MAX_POOLED_BYTES: usize = 1 << 29;

fn pool() -> std::sync::MutexGuard<'static, Pool> {
    static POOL: OnceLock<Mutex<Pool>> = OnceLock::new();
    POOL.get_or_init(|| {
        Mutex::new(Pool {
            free: HashMap::new(),
            bytes: 0,
        })
    })
    .lock()
    .unwrap_or_else(std::sync::PoisonError::into_inner)
}

impl Drop for Buffer {
    fn drop(&mut self) {
        let mut pool = pool();
        if pool.bytes + self.bytes <= MAX_POOLED_BYTES {
            pool.bytes += self.bytes;
            pool.free
                .entry(self.bytes)
                .or_default()
                .push(self.handle as usize);
            return;
        }
        drop(pool);
        // SAFETY: `handle` is the one reference this value owns.
        unsafe { aura_metal_buffer_release(self.handle) }
    }
}

fn element_bytes(dtype: ArrayDType) -> usize {
    match dtype {
        ArrayDType::Bool => 1,
        ArrayDType::Int32 | ArrayDType::Float32 => 4,
        ArrayDType::Int64 | ArrayDType::Float64 => 8,
    }
}

impl Buffer {
    /// A buffer of `bytes` bytes whose contents are not set.
    fn new(bytes: usize) -> Result<Self> {
        let pooled = {
            let mut pool = pool();
            let handle = pool.free.get_mut(&bytes).and_then(Vec::pop);
            if handle.is_some() {
                pool.bytes -= bytes;
            }
            handle
        };
        if let Some(handle) = pooled {
            return Ok(Self {
                handle: handle as *mut c_void,
                bytes,
            });
        }
        // SAFETY: the shim returns a retained buffer or null.
        let handle = unsafe { aura_metal_buffer_new(bytes) };
        if handle.is_null() {
            return Err(metal_error(&format!(
                "could not allocate a buffer of {bytes} bytes"
            )));
        }
        Ok(Self { handle, bytes })
    }

    fn contents(&self) -> *mut u8 {
        // SAFETY: `handle` is a live shared-storage buffer.
        unsafe { aura_metal_buffer_contents(self.handle) }
    }

    /// A buffer holding the elements of a non-empty Array in row-major
    /// order.
    pub(crate) fn from_array(elements: &ArrayValue) -> Result<Self> {
        let storage = elements.elements()?;
        fn bytes_of<T>(values: &[T]) -> &[u8] {
            // SAFETY: the element types are plain numbers and `bool`, which
            // have no padding.
            unsafe {
                std::slice::from_raw_parts(values.as_ptr().cast(), std::mem::size_of_val(values))
            }
        }
        let bytes = match &*storage {
            ArrayStorage::Int32(values) => bytes_of::<i32>(values),
            ArrayStorage::Int64(values) => bytes_of::<i64>(values),
            ArrayStorage::Float32(values) => bytes_of::<f32>(values),
            ArrayStorage::Float64(values) => bytes_of::<f64>(values),
            ArrayStorage::Bool(values) => bytes_of::<bool>(values),
        };
        let buffer = Self::new(bytes.len())?;
        // SAFETY: the buffer holds `bytes.len()` bytes and nothing else
        // refers to it yet.
        unsafe { std::ptr::copy_nonoverlapping(bytes.as_ptr(), buffer.contents(), bytes.len()) };
        Ok(buffer)
    }

    /// An Array holding a copy of the buffer's elements.
    pub(crate) fn to_array(&self, dtype: ArrayDType, shape: &[usize]) -> Result<ArrayValue> {
        let len: usize = shape.iter().product();
        assert_eq!(
            len * element_bytes(dtype),
            self.bytes,
            "a tensor's buffer holds its shape"
        );
        fn read<T: Copy + Default>(buffer: &Buffer, len: usize) -> Box<[T]> {
            let mut values = vec![T::default(); len];
            // SAFETY: the buffer holds `len` elements of `T`, and `values`
            // is a separate allocation of the same size.
            unsafe {
                std::ptr::copy_nonoverlapping(
                    buffer.contents(),
                    values.as_mut_ptr().cast::<u8>(),
                    len * std::mem::size_of::<T>(),
                );
            }
            values.into()
        }
        let storage = match dtype {
            ArrayDType::Int32 => ArrayStorage::Int32(read::<i32>(self, len).into()),
            ArrayDType::Int64 => ArrayStorage::Int64(read::<i64>(self, len).into()),
            ArrayDType::Float32 => ArrayStorage::Float32(read::<f32>(self, len).into()),
            ArrayDType::Float64 => ArrayStorage::Float64(read::<f64>(self, len).into()),
            ArrayDType::Bool => {
                // A byte the GPU wrote may be any non-zero value for true.
                let flags: Box<[bool]> = read::<u8>(self, len)
                    .iter()
                    .map(|&byte| byte != 0)
                    .collect();
                ArrayStorage::Bool(flags.into())
            }
        };
        ArrayValue::new(shape.into(), storage)
    }
}

/// A compiled plan: an MPSGraph, the index in it of each leaf's tensor,
/// and the index of each output's.
struct Graph {
    handle: *mut c_void,
    placeholders: Vec<i32>,
    targets: Vec<i32>,
}

// SAFETY: a graph is used only under the cache's lock.
unsafe impl Send for Graph {}

impl Drop for Graph {
    fn drop(&mut self) {
        // SAFETY: `handle` is the one reference this value owns.
        unsafe { aura_metal_graph_release(self.handle) }
    }
}

/// The shim's code for a dtype.
fn dtype_code(dtype: i64) -> i32 {
    dtype as i32
}

fn shape_numbers(shape: &[usize]) -> Vec<i64> {
    shape.iter().map(|&dimension| dimension as i64).collect()
}

impl Graph {
    fn compile(plan: &Plan) -> Result<Self> {
        // SAFETY: the shim returns a retained graph or null.
        let handle = unsafe { aura_metal_graph_new() };
        if handle.is_null() {
            return Err(metal_error("could not make a graph"));
        }
        let mut graph = Self {
            handle,
            placeholders: Vec::new(),
            targets: Vec::new(),
        };
        let shape_of = |slot: usize| match &plan.slots[slot] {
            Slot::Leaf { shape, .. } | Slot::Step { shape, .. } => shape,
        };
        let mut tensors: Vec<i32> = Vec::with_capacity(plan.slots.len());
        for slot in &plan.slots {
            let tensor = match slot {
                Slot::Leaf { dtype, shape } => {
                    let shape = shape_numbers(shape);
                    // SAFETY: `shape` holds `rank` numbers.
                    let tensor = unsafe {
                        aura_metal_graph_placeholder(
                            handle,
                            dtype_code(*dtype),
                            shape.len() as i32,
                            shape.as_ptr(),
                        )
                    };
                    graph.placeholders.push(tensor);
                    tensor
                }
                Slot::Step {
                    operation,
                    inputs,
                    dtype,
                    shape,
                } => {
                    use super::Operation;
                    let (kind, code, numbers): (i32, i64, Vec<i64>) = match operation {
                        Operation::Binary(code) if *code < PAIR_OPERATION_BASE => {
                            (0, *code, Vec::new())
                        }
                        Operation::Binary(code) => (0, 4 + code - PAIR_OPERATION_BASE, Vec::new()),
                        Operation::Unary(NEGATE_OPERATION) => (1, 8, Vec::new()),
                        Operation::Unary(code) => (1, *code, Vec::new()),
                        Operation::Compare(code) => (2, *code, Vec::new()),
                        Operation::Where => (3, 0, Vec::new()),
                        Operation::Reduce { reduction, axis } => {
                            let rank = match &plan.slots[inputs[0]] {
                                Slot::Leaf { shape, .. } | Slot::Step { shape, .. } => {
                                    shape.len() as i64
                                }
                            };
                            let axes = match axis {
                                Some(axis) if *axis < 0 => vec![axis + rank],
                                Some(axis) => vec![*axis],
                                None => (0..rank).collect(),
                            };
                            (4, *reduction, axes)
                        }
                        Operation::Matmul => (5, 0, Vec::new()),
                        Operation::Reshape(_) => (6, 0, Vec::new()),
                        Operation::Permute(axes) => {
                            let rank = shape.len() as i64;
                            let axes = if axes.is_empty() {
                                (0..rank).rev().collect()
                            } else {
                                axes.iter()
                                    .map(|&axis| if axis < 0 { axis + rank } else { axis })
                                    .collect()
                            };
                            (7, 0, axes)
                        }
                        Operation::Cast(_) => (8, 0, Vec::new()),
                        Operation::Slice { axis, step, length } => {
                            (9, 0, vec![*axis as i64, *step as i64, *length as i64])
                        }
                        Operation::Unslice { axis, step, .. } => (
                            10,
                            0,
                            vec![
                                *axis as i64,
                                *step as i64,
                                shape_of(inputs[0])[*axis] as i64,
                            ],
                        ),
                        Operation::Expand(_) => (11, 0, Vec::new()),
                        Operation::Concat { axis } => (12, 0, vec![*axis as i64]),
                        Operation::Take { axis } => {
                            (13, 0, vec![*axis as i64, shape_of(inputs[0])[*axis] as i64])
                        }
                        Operation::IndexAdd { axis, size } => {
                            (14, 0, vec![*axis as i64, *size as i64])
                        }
                        Operation::ArgReduce { largest, axis } => (
                            15,
                            i64::from(*largest),
                            vec![*axis as i64, shape_of(inputs[0])[*axis] as i64],
                        ),
                        Operation::Random { fill, .. } => (16, *fill, Vec::new()),
                    };
                    let mut operands = [-1_i32; 3];
                    for (operand, &input) in operands.iter_mut().zip(inputs) {
                        *operand = tensors[input];
                    }
                    let shape = shape_numbers(shape);
                    // SAFETY: `operands` holds three indices, `shape` holds
                    // `rank` numbers and `numbers` holds `count`.
                    unsafe {
                        aura_metal_graph_step(
                            handle,
                            kind,
                            code as i32,
                            operands.as_ptr(),
                            dtype_code(*dtype),
                            shape.len() as i32,
                            shape.as_ptr(),
                            numbers.len() as i32,
                            numbers.as_ptr(),
                        )
                    }
                }
            };
            if tensor < 0 {
                return Err(metal_error("could not build an operation"));
            }
            tensors.push(tensor);
        }
        graph.targets = plan.outputs.iter().map(|&output| tensors[output]).collect();
        Ok(graph)
    }
}

/// The most compiled plans kept. A program has a handful of expression
/// shapes, so this only bounds one that makes a new shape forever.
const MAX_COMPILED_PLANS: usize = 256;

/// Runs `plan` over `leaves` and gives the buffer holding each of its
/// outputs, in order.
pub(crate) fn run(plan: &Plan, leaves: &[Storage]) -> Result<Vec<Storage>> {
    static COMPILED: OnceLock<Mutex<HashMap<Plan, Graph>>> = OnceLock::new();
    let trace = super::trace_level() > 0;
    if trace {
        eprintln!("aura tensor: program={:.2}ms", super::trace_gap());
    }
    // A leaf in host memory is copied to a buffer for this run.
    let mut uploaded: Vec<Arc<Buffer>> = Vec::new();
    let mut buffers: Vec<*mut c_void> = Vec::with_capacity(leaves.len());
    for leaf in leaves {
        match leaf {
            Storage::Metal(buffer) => buffers.push(buffer.handle),
            Storage::Host(elements) => {
                let buffer = Arc::new(Buffer::from_array(elements)?);
                buffers.push(buffer.handle);
                uploaded.push(buffer);
            }
        }
    }
    let mut outputs: Vec<Buffer> = Vec::with_capacity(plan.outputs.len());
    for &output in &plan.outputs {
        let (dtype, shape) = match &plan.slots[output] {
            Slot::Leaf { dtype, shape } | Slot::Step { dtype, shape, .. } => (*dtype, shape),
        };
        let dtype = ArrayDType::from_code(dtype).expect("a plan holds known dtypes");
        outputs.push(Buffer::new(
            shape.iter().product::<usize>() * element_bytes(dtype),
        )?);
    }
    let handles: Vec<*mut c_void> = outputs.iter().map(|output| output.handle).collect();
    let mut compiled = COMPILED
        .get_or_init(Default::default)
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let known = compiled.contains_key(plan);
    if !known {
        if compiled.len() >= MAX_COMPILED_PLANS {
            compiled.clear();
        }
        compiled.insert(plan.clone(), Graph::compile(plan)?);
    }
    let graph = &compiled[plan];
    let built = if trace { super::trace_gap() } else { 0.0 };
    // SAFETY: `placeholders` and `buffers` hold one entry per leaf,
    // `targets` and `handles` one per output, and every buffer is alive
    // and holds its tensor's shape.
    let status = unsafe {
        aura_metal_graph_run(
            graph.handle,
            buffers.len() as i32,
            graph.placeholders.as_ptr(),
            buffers.as_ptr(),
            handles.len() as i32,
            graph.targets.as_ptr(),
            handles.as_ptr(),
        )
    };
    drop(compiled);
    drop(uploaded);
    if status != 0 {
        return Err(metal_error("could not run a graph"));
    }
    if trace {
        // The first run of a graph includes the library's own compile.
        eprintln!(
            "aura tensor: metal steps={} leaves={} outputs={} new={} build={built:.2}ms run={:.2}ms",
            plan.slots.len() - leaves.len(),
            leaves.len(),
            outputs.len(),
            !known,
            super::trace_gap()
        );
    }
    Ok(outputs
        .into_iter()
        .map(|output| Storage::Metal(Arc::new(output)))
        .collect())
}
