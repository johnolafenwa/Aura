//! The host part of the builtin `tensor` module.
//!
//! A `tensor.Node` is a reference-counted handle to memory a device owns.
//! `std/tensor.au` keeps one in each `Tensor[T]` and calls the functions
//! here for every operation. Shapes and dtypes are checked here, on the
//! host, with the Array rules, so a mistake is reported by the call that
//! made it on every device.
//!
//! The CPU device keeps a node's elements in an [`ArrayValue`] and runs
//! each operation through the Array kernels when it is asked for. Integer
//! arithmetic wraps, as it does on a GPU, so a result does not depend on
//! the device.
//!
//! A GPU device keeps an operation its library has as a waiting node, and
//! runs the whole expression behind a tensor as one [`Plan`] when the
//! tensor is read. An operation the library lacks runs through the Array
//! kernels at once, in host memory, so every operation exists on every
//! device with one meaning.

use super::*;
use host_builtins::HostBuiltinFn;
use std::sync::Mutex;

#[cfg(target_os = "macos")]
#[path = "tensor_metal.rs"]
pub(crate) mod tensor_metal;

#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
#[path = "tensor_codegen.rs"]
pub(crate) mod tensor_codegen;

#[cfg(target_os = "linux")]
#[path = "tensor_cuda.rs"]
pub(crate) mod tensor_cuda;

#[path = "tensor_kernels.rs"]
pub(crate) mod tensor_kernels;

#[path = "tensor_grad.rs"]
mod tensor_grad;

/// Where a tensor's memory lives. The codes are the ones `std/tensor.au`
/// passes.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TensorDevice {
    Cpu = 0,
    Metal = 1,
    Cuda = 2,
}

impl TensorDevice {
    pub const fn from_code(code: i64) -> Option<Self> {
        match code {
            0 => Some(Self::Cpu),
            1 => Some(Self::Metal),
            2 => Some(Self::Cuda),
            _ => None,
        }
    }

    pub const fn name(self) -> &'static str {
        match self {
            Self::Cpu => "cpu",
            Self::Metal => "metal",
            Self::Cuda => "cuda",
        }
    }

    /// Whether this process can run work on the device.
    pub fn is_available(self) -> bool {
        match self {
            Self::Cpu => true,
            #[cfg(target_os = "macos")]
            Self::Metal => tensor_metal::is_available(),
            #[cfg(not(target_os = "macos"))]
            Self::Metal => false,
            #[cfg(target_os = "linux")]
            Self::Cuda => tensor_cuda::is_available(),
            #[cfg(not(target_os = "linux"))]
            Self::Cuda => false,
        }
    }

    /// Whether the device has `float64` elements. Metal has none.
    pub const fn has_float64(self) -> bool {
        !matches!(self, Self::Metal)
    }

    /// Whether an operation on the device waits until its result is read.
    const fn is_gpu(self) -> bool {
        !matches!(self, Self::Cpu)
    }
}

/// One operation over tensors. A node that is waiting keeps the operation
/// that makes it, and a plan is a list of them.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub(crate) enum Operation {
    /// Arithmetic by [`ArrayBinaryOp`] code, or a pair operation by
    /// [`ArrayPairOp`] code plus [`PAIR_OPERATION_BASE`].
    Binary(i64),
    /// An [`ArrayUnaryOp`] code, or [`NEGATE_OPERATION`].
    Unary(i64),
    /// An [`ArrayCompareOp`] code.
    Compare(i64),
    /// Mask, value where it holds, value where it does not.
    Where,
    /// An [`ArrayReduction`] code over one axis, or over every element.
    Reduce {
        reduction: i64,
        axis: Option<i64>,
    },
    Matmul,
    Reshape(Vec<i64>),
    /// The axes in a new order. An empty order reverses them.
    Permute(Vec<i64>),
    /// An [`ArrayDType`] code.
    Cast(i64),
    /// `length` elements of each lane along `axis`, from `start`, `step`
    /// apart.
    Slice {
        axis: usize,
        start: usize,
        step: usize,
        length: usize,
    },
    /// The reverse of a slice: lanes of `size` elements that hold the
    /// operand's elements from `start`, `step` apart, and zero elsewhere.
    Unslice {
        axis: usize,
        start: usize,
        step: usize,
        size: usize,
    },
    /// The operand repeated to a shape its own broadcasts to.
    Expand(Vec<usize>),
    /// Each lane of the first operand followed by the lane of the second.
    Concat { axis: usize },
    /// For each index of the second operand, the entry of the first at
    /// that place along `axis`.
    Take { axis: usize },
    /// The reverse of a take: lanes of `size` entries, where entry `k` is
    /// the sum of the entries of the first operand whose index, in the
    /// second, is `k`.
    IndexAdd { axis: usize, size: usize },
    /// The place along `axis` of each lane's largest element, or smallest.
    ArgReduce { largest: bool, axis: usize },
}

/// The codes of the functions in [`Operation::Unary`] that `Array` does
/// not have.
pub(crate) const TANH_OPERATION: i64 = 65;
pub(crate) const SIGMOID_OPERATION: i64 = 66;
pub(crate) const ERF_OPERATION: i64 = 67;

/// Where a tensor's elements are.
#[derive(Clone)]
pub(crate) enum Storage {
    /// Host memory. The CPU device keeps every tensor here, and a GPU
    /// device keeps here an empty tensor and the result of an operation
    /// its library does not have.
    Host(ArrayValue),
    /// A buffer the Metal device owns.
    #[cfg(target_os = "macos")]
    Metal(Arc<tensor_metal::Buffer>),
    /// Memory the CUDA device owns.
    #[cfg(target_os = "linux")]
    Cuda(Arc<tensor_cuda::Buffer>),
}

impl Storage {
    /// The elements in host memory, for memory of `dtype` and `shape`.
    #[cfg_attr(
        not(any(target_os = "macos", target_os = "linux")),
        allow(unused_variables)
    )]
    fn to_host(&self, dtype: i64, shape: &[usize]) -> Result<ArrayValue> {
        match self {
            Self::Host(elements) => Ok(elements.clone()),
            #[cfg(target_os = "macos")]
            Self::Metal(buffer) => buffer.to_array(
                ArrayDType::from_code(dtype).expect("a plan holds known dtypes"),
                shape,
            ),
            #[cfg(target_os = "linux")]
            Self::Cuda(buffer) => buffer.to_array(
                ArrayDType::from_code(dtype).expect("a plan holds known dtypes"),
                shape,
            ),
        }
    }
}

/// A node that has not run yet.
struct Pending {
    operation: Operation,
    inputs: Vec<TensorValue>,
    /// The longest chain of waiting nodes below this one, counting it.
    depth: usize,
}

enum State {
    Ready(Storage),
    Pending(Pending),
}

struct TensorNode {
    device: TensorDevice,
    dtype: ArrayDType,
    shape: Box<[usize]>,
    state: Mutex<State>,
    /// Whether a gradient can be asked for with respect to this tensor:
    /// it is watched, or it was computed from a tensor that is.
    tracked: bool,
    /// How a tracked tensor was computed. A watched tensor has none.
    history: Option<History>,
    /// The gradients of this tensor with respect to each watched tensor
    /// it was computed from, once one has been asked for.
    gradients: Mutex<Option<Vec<(TensorValue, TensorValue)>>>,
}

/// The operation that gave a tracked tensor, and its operands.
struct History {
    operation: Operation,
    inputs: Vec<TensorValue>,
}

/// A `tensor.Node`: shared, immutable device memory, or the work that
/// will fill it.
#[derive(Clone)]
pub struct TensorValue(Arc<TensorNode>);

impl PartialEq for TensorValue {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}

impl std::fmt::Debug for TensorValue {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("<tensor-node>")
    }
}

/// A chain of waiting nodes longer than this runs when it is made, so a
/// loop that never reads a result cannot grow one without bound.
const MAX_PENDING_DEPTH: usize = 128;

impl TensorValue {
    fn ready(
        device: TensorDevice,
        dtype: ArrayDType,
        shape: Box<[usize]>,
        storage: Storage,
    ) -> Self {
        Self::in_state(device, dtype, shape, State::Ready(storage))
    }

    fn in_state(device: TensorDevice, dtype: ArrayDType, shape: Box<[usize]>, state: State) -> Self {
        Self(Arc::new(TensorNode {
            device,
            dtype,
            shape,
            state: Mutex::new(state),
            tracked: false,
            history: None,
            gradients: Mutex::new(None),
        }))
    }

    /// This new tensor as a tracked one. `history` is how it was computed,
    /// or `None` for a watched tensor.
    fn tracking(mut self, history: Option<History>) -> Self {
        let node = Arc::get_mut(&mut self.0).expect("a new tensor has one holder");
        node.tracked = true;
        node.history = history;
        self
    }

    /// Whether a gradient can be asked for with respect to this tensor.
    pub fn is_tracked(&self) -> bool {
        self.0.tracked
    }

    /// A tensor of the elements of `elements`, in host memory.
    fn host(device: TensorDevice, elements: ArrayValue) -> Self {
        Self::ready(
            device,
            elements.dtype(),
            elements.shape.clone(),
            Storage::Host(elements),
        )
    }

    pub fn device(&self) -> TensorDevice {
        self.0.device
    }

    pub fn dtype(&self) -> ArrayDType {
        self.0.dtype
    }

    pub fn shape(&self) -> &[usize] {
        &self.0.shape
    }

    pub fn len(&self) -> usize {
        self.0.shape.iter().product()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    fn state(&self) -> std::sync::MutexGuard<'_, State> {
        self.0
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    fn depth(&self) -> usize {
        match &*self.state() {
            State::Ready(_) => 0,
            State::Pending(pending) => pending.depth,
        }
    }

    /// Whether the tensor's work has run.
    pub fn is_ready(&self) -> bool {
        matches!(&*self.state(), State::Ready(_))
    }

    /// The tensor's memory, after running the work it waits on. Two
    /// threads that ask at once both run the work and store equal results.
    fn storage(&self) -> Result<Storage> {
        if let State::Ready(storage) = &*self.state() {
            return Ok(storage.clone());
        }
        let (plan, leaves) = Plan::collect(self);
        let storage = plan.run(self.device(), &leaves)?;
        *self.state() = State::Ready(storage.clone());
        Ok(storage)
    }

    /// The tensor's elements in host memory.
    fn elements(&self) -> Result<ArrayValue> {
        self.storage()?.to_host(self.dtype() as i64, self.shape())
    }
}

/// One value of a plan: memory that exists, or the result of a step over
/// earlier values.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub(crate) enum Slot {
    Leaf {
        dtype: i64,
        shape: Vec<usize>,
    },
    Step {
        operation: Operation,
        inputs: Vec<usize>,
        dtype: i64,
        shape: Vec<usize>,
    },
}

/// The work one tensor waits on, with every value before the values made
/// from it. The last slot is the tensor. A plan names no memory, so two
/// runs of one expression over tensors of the same shapes have equal
/// plans, and a device compiles the plan once.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub(crate) struct Plan {
    pub(crate) slots: Vec<Slot>,
}

impl Plan {
    /// The plan for `root` and the memory of its leaves, in slot order.
    fn collect(root: &TensorValue) -> (Self, Vec<Storage>) {
        fn visit(
            node: &TensorValue,
            slots: &mut Vec<Slot>,
            leaves: &mut Vec<Storage>,
            seen: &mut HashMap<*const TensorNode, usize>,
        ) -> usize {
            let key = Arc::as_ptr(&node.0);
            if let Some(&slot) = seen.get(&key) {
                return slot;
            }
            let waiting = match &*node.state() {
                State::Ready(storage) => {
                    leaves.push(storage.clone());
                    None
                }
                State::Pending(pending) => {
                    Some((pending.operation.clone(), pending.inputs.clone()))
                }
            };
            let dtype = node.dtype() as i64;
            let shape = node.shape().to_vec();
            let slot = match waiting {
                None => Slot::Leaf { dtype, shape },
                Some((operation, inputs)) => Slot::Step {
                    operation,
                    inputs: inputs
                        .iter()
                        .map(|input| visit(input, slots, leaves, seen))
                        .collect(),
                    dtype,
                    shape,
                },
            };
            slots.push(slot);
            seen.insert(key, slots.len() - 1);
            slots.len() - 1
        }
        let mut slots = Vec::new();
        let mut leaves = Vec::new();
        visit(root, &mut slots, &mut leaves, &mut HashMap::new());
        (Self { slots }, leaves)
    }

    fn run(&self, device: TensorDevice, leaves: &[Storage]) -> Result<Storage> {
        #[cfg(target_os = "macos")]
        if device == TensorDevice::Metal {
            return tensor_metal::run(self, leaves);
        }
        #[cfg(target_os = "linux")]
        if device == TensorDevice::Cuda {
            return tensor_cuda::run(self, leaves);
        }
        let _ = device;
        self.run_on_host(leaves).map(Storage::Host)
    }

    /// Runs every step through the Array kernels. This is what a plan
    /// means: a device's result is checked against it.
    pub(crate) fn run_on_host(&self, leaves: &[Storage]) -> Result<ArrayValue> {
        let mut leaves = leaves.iter();
        let mut values: Vec<ArrayValue> = Vec::with_capacity(self.slots.len());
        for slot in &self.slots {
            let value = match slot {
                Slot::Leaf { dtype, shape } => leaves
                    .next()
                    .expect("a plan has memory for each leaf")
                    .to_host(*dtype, shape)?,
                Slot::Step {
                    operation, inputs, ..
                } => {
                    let inputs: Vec<&ArrayValue> =
                        inputs.iter().map(|&input| &values[input]).collect();
                    apply(operation, &inputs)?
                }
            };
            values.push(value);
        }
        Ok(values.pop().expect("a plan has a last slot"))
    }
}

thread_local! {
    /// Makes the CPU device wait like a GPU device, so the tests of
    /// waiting nodes and plans run on every machine.
    static DEFER_ON_CPU: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// Whether an operation on `device` waits until its result is read.
fn defers(device: TensorDevice) -> bool {
    device.is_gpu() || DEFER_ON_CPU.with(std::cell::Cell::get)
}

/// The dtype and shape of an operation a GPU library runs, or `None` for
/// one it does not: an operand that is empty, a dtype or operation the
/// library lacks or gives a different meaning, or operands the Array rules
/// reject. Those run through the Array kernels at once, which also words
/// the error of a mistake.
fn planned_result(
    operation: &Operation,
    inputs: &[&TensorValue],
) -> Option<(ArrayDType, Box<[usize]>)> {
    use ArrayDType::{Bool, Float32, Int32, Int64};
    if inputs.iter().any(|input| input.is_empty()) {
        return None;
    }
    let dtype = inputs[0].dtype();
    let same_dtype = inputs.iter().all(|input| input.dtype() == dtype);
    // CUDA has `float64`. Metal does not.
    let double = inputs[0].device() == TensorDevice::Cuda;
    let is_real = |dtype: ArrayDType| dtype == Float32 || (double && dtype == ArrayDType::Float64);
    let real = is_real(dtype);
    let number = real || matches!(dtype, Int32 | Int64);
    let broadcast = || {
        inputs[1..]
            .iter()
            .try_fold(inputs[0].shape().to_vec(), |shape, input| {
                array_layout::broadcast_shape(&shape, input.shape())
            })
            .map(Vec::into_boxed_slice)
    };
    // The Metal device runs these through the Array kernels for now.
    let indexed = inputs[0].device() != TensorDevice::Metal;
    let resized = |axis: usize, length: usize| {
        let mut shape = inputs[0].shape().to_vec();
        shape[axis] = length;
        (dtype, shape.into_boxed_slice())
    };
    let rank = inputs[0].shape().len();
    let normalize = |axis: i64| {
        let rank = rank as i64;
        let axis = if axis < 0 { axis + rank } else { axis };
        (0..rank).contains(&axis).then_some(axis as usize)
    };
    match operation {
        Operation::Binary(code) => {
            let supported = match *code {
                0..=2 | 16 | 17 => number,
                3 | 18 => real,
                _ => false,
            };
            (supported && same_dtype)
                .then(broadcast)?
                .map(|shape| (dtype, shape))
        }
        Operation::Unary(code) => {
            let supported = match *code {
                NEGATE_OPERATION => number,
                0..=3 | 6 | 7 => real,
                TANH_OPERATION..=ERF_OPERATION => real && indexed,
                _ => false,
            };
            supported.then(|| (dtype, inputs[0].shape().into()))
        }
        Operation::Compare(code) => ((0..=5).contains(code) && number && same_dtype)
            .then(broadcast)?
            .map(|shape| (Bool, shape)),
        Operation::Where => {
            let value = inputs[1].dtype();
            (dtype == Bool
                && inputs[2].dtype() == value
                && (is_real(value) || matches!(value, Int32 | Int64)))
            .then(broadcast)?
            .map(|shape| (value, shape))
        }
        Operation::Reduce { reduction, axis } => {
            if !real || !(0..=3).contains(reduction) {
                return None;
            }
            let shape: Vec<usize> = match axis {
                None => vec![1],
                Some(axis) => {
                    let axis = normalize(*axis)?;
                    let mut shape = inputs[0].shape().to_vec();
                    shape.remove(axis);
                    if shape.is_empty() {
                        shape.push(1);
                    }
                    shape
                }
            };
            Some((dtype, shape.into()))
        }
        Operation::Matmul => {
            let (left, right) = (inputs[0].shape(), inputs[1].shape());
            if !real || !same_dtype || left.len() < 2 || right.len() < 2 {
                return None;
            }
            let (left_lead, left_matrix) = left.split_at(left.len() - 2);
            let (right_lead, right_matrix) = right.split_at(right.len() - 2);
            if left_matrix[1] != right_matrix[0] {
                return None;
            }
            let mut shape = array_layout::broadcast_shape(left_lead, right_lead)?;
            shape.extend([left_matrix[0], right_matrix[1]]);
            Some((dtype, shape.into()))
        }
        Operation::Reshape(shape) => {
            let len = inputs[0].len();
            let known: usize = shape
                .iter()
                .filter(|&&dimension| dimension != -1)
                .try_fold(1_usize, |known, &dimension| {
                    known.checked_mul(usize::try_from(dimension).ok()?)
                })?;
            let inferred = shape.iter().filter(|&&dimension| dimension == -1).count();
            if shape.is_empty() || inferred > 1 || known == 0 || !len.is_multiple_of(known) {
                return None;
            }
            if inferred == 0 && known != len {
                return None;
            }
            let shape = shape
                .iter()
                .map(|&dimension| {
                    if dimension == -1 {
                        len / known
                    } else {
                        dimension as usize
                    }
                })
                .collect();
            Some((dtype, shape))
        }
        Operation::Permute(axes) => {
            let axes: Vec<usize> = if axes.is_empty() {
                (0..rank).rev().collect()
            } else {
                axes.iter()
                    .map(|&axis| normalize(axis))
                    .collect::<Option<_>>()?
            };
            let mut seen = vec![false; rank];
            if axes.len() != rank
                || axes
                    .iter()
                    .any(|&axis| std::mem::replace(&mut seen[axis], true))
            {
                return None;
            }
            Some((
                dtype,
                axes.iter().map(|&axis| inputs[0].shape()[axis]).collect(),
            ))
        }
        Operation::Cast(target) => {
            let target = ArrayDType::from_code(*target)?;
            // A float to integer cast traps on a value out of range, which
            // a GPU cannot do, so it runs on the host.
            let exact = matches!((dtype, target), (Int32 | Int64, Float32) | (Int32, Int64))
                || (double
                    && matches!(
                        (dtype, target),
                        (Int32 | Int64 | Float32, ArrayDType::Float64)
                    ));
            exact.then(|| (target, inputs[0].shape().into()))
        }
        Operation::Slice { axis, length, .. } => indexed.then(|| resized(*axis, *length)),
        Operation::Unslice { axis, size, .. } => indexed.then(|| resized(*axis, *size)),
        Operation::Expand(shape) => indexed.then(|| (dtype, shape.clone().into())),
        Operation::Concat { axis } => (indexed && same_dtype)
            .then(|| resized(*axis, inputs[0].shape()[*axis] + inputs[1].shape()[*axis])),
        Operation::Take { axis } => indexed.then(|| {
            (
                dtype,
                tensor_kernels::take_shape(inputs[0].shape(), inputs[1].shape(), *axis),
            )
        }),
        Operation::IndexAdd { axis, size } => (indexed && real).then(|| {
            let after = *axis + inputs[1].shape().len();
            let shape = inputs[0].shape();
            (
                dtype,
                shape[..*axis]
                    .iter()
                    .copied()
                    .chain([*size])
                    .chain(shape[after..].iter().copied())
                    .collect(),
            )
        }),
        Operation::ArgReduce { axis, .. } => (indexed && number)
            .then(|| (Int64, tensor_kernels::lane_shape(inputs[0].shape(), *axis))),
    }
}

/// Integer tensors wrap on every device, because a GPU cannot trap. The
/// Array kernels take the wrapping mode for integer elements only.
fn arithmetic_mode(dtype: ArrayDType) -> IntegerArithmeticMode {
    if matches!(dtype, ArrayDType::Int32 | ArrayDType::Int64) {
        IntegerArithmeticMode::Wrapping
    } else {
        IntegerArithmeticMode::Checked
    }
}

/// The first code of the pair operations in [`Operation::Binary`]. The
/// codes below it are [`ArrayBinaryOp`]'s.
pub(crate) const PAIR_OPERATION_BASE: i64 = 16;

/// The code of negation in [`Operation::Unary`]. The codes below it are
/// [`ArrayUnaryOp`]'s.
pub(crate) const NEGATE_OPERATION: i64 = 64;

/// One function for `float32` elements and the same one for `float64`.
type FloatFunctions = (fn(f32) -> f32, fn(f64) -> f64);

/// A float function of every element, or `None` for an Array that is not
/// float. A tensor's float maths follows IEEE 754 on every device: an
/// overflow is an infinity and a value outside the domain is a NaN,
/// because a GPU cannot stop the program. The Array members stop it.
fn float_map(
    elements: &ArrayValue,
    single: fn(f32) -> f32,
    double: fn(f64) -> f64,
) -> Result<Option<ArrayValue>> {
    let values = elements.elements()?;
    let storage = match &*values {
        ArrayStorage::Float32(values) => ArrayStorage::Float32(
            values
                .iter()
                .map(|&value| single(value))
                .collect::<Box<[f32]>>()
                .into(),
        ),
        ArrayStorage::Float64(values) => ArrayStorage::Float64(
            values
                .iter()
                .map(|&value| double(value))
                .collect::<Box<[f64]>>()
                .into(),
        ),
        _ => return Ok(None),
    };
    ArrayValue::new(elements.try_shape()?, storage).map(Some)
}

/// A float function of each pair of elements of two float Arrays of one
/// dtype whose shapes broadcast, by the IEEE rules.
fn float_zip(
    left: &ArrayValue,
    right: &ArrayValue,
    single: fn(f32, f32) -> f32,
    double: fn(f64, f64) -> f64,
) -> Result<ArrayValue> {
    // Multiplying by ones of the other shape broadcasts each operand and
    // keeps every value, a NaN and a negative zero included.
    let ones =
        |like: &ArrayValue| ArrayValue::full(left.dtype(), like.shape.clone(), &Value::Float(1.0));
    let mode = IntegerArithmeticMode::Checked;
    let left = left.binary(&ones(right)?, ArrayBinaryOp::Mul, mode)?;
    let right = right.binary(&ones(&left)?, ArrayBinaryOp::Mul, mode)?;
    let (lefts, rights) = (left.elements()?, right.elements()?);
    let storage = match (&*lefts, &*rights) {
        (ArrayStorage::Float32(lefts), ArrayStorage::Float32(rights)) => ArrayStorage::Float32(
            lefts
                .iter()
                .zip(rights.iter())
                .map(|(&left, &right)| single(left, right))
                .collect::<Box<[f32]>>()
                .into(),
        ),
        (ArrayStorage::Float64(lefts), ArrayStorage::Float64(rights)) => ArrayStorage::Float64(
            lefts
                .iter()
                .zip(rights.iter())
                .map(|(&left, &right)| double(left, right))
                .collect::<Box<[f64]>>()
                .into(),
        ),
        _ => unreachable!("both operands are floats of one dtype"),
    };
    ArrayValue::new(left.try_shape()?, storage)
}

/// The result of `operation` by the Array kernels, which is what the
/// operation means on every device.
fn apply(operation: &Operation, inputs: &[&ArrayValue]) -> Result<ArrayValue> {
    let unknown = || tensor_error("tensor", "was given an unknown operation");
    let first = inputs[0];
    let other = |index: usize| Value::Array(inputs[index].clone());
    match operation {
        Operation::Binary(code) => {
            let floats = matches!(first.dtype(), ArrayDType::Float32 | ArrayDType::Float64)
                && inputs[1].dtype() == first.dtype();
            if *code < PAIR_OPERATION_BASE {
                let operation = ArrayBinaryOp::from_code(*code).ok_or_else(unknown)?;
                if operation == ArrayBinaryOp::Div && floats {
                    // The Array division stops the program on a zero
                    // divisor.
                    return float_zip(first, inputs[1], |a, b| a / b, |a, b| a / b);
                }
                return first.binary(inputs[1], operation, arithmetic_mode(first.dtype()));
            }
            let operation =
                ArrayPairOp::from_code(code - PAIR_OPERATION_BASE).ok_or_else(unknown)?;
            if operation == ArrayPairOp::Pow && floats {
                float_zip(first, inputs[1], f32::powf, f64::powf)
            } else {
                first.pairwise(&other(1), operation)
            }
        }
        Operation::Unary(NEGATE_OPERATION) => ArrayValue::zeros(first.dtype(), Box::new([1]))?
            .binary(first, ArrayBinaryOp::Sub, arithmetic_mode(first.dtype())),
        Operation::Unary(code @ TANH_OPERATION..=ERF_OPERATION) => {
            let (single, double): FloatFunctions = match *code {
                TANH_OPERATION => (f32::tanh, f64::tanh),
                SIGMOID_OPERATION => (|x| 1.0 / (1.0 + (-x).exp()), tensor_kernels::sigmoid),
                _ => (|x| tensor_kernels::erf(x.into()) as f32, tensor_kernels::erf),
            };
            float_map(first, single, double)?
                .ok_or_else(|| tensor_error("tensor", "expects a float tensor"))
        }
        Operation::Unary(code) => {
            let operation = ArrayUnaryOp::from_code(*code).ok_or_else(unknown)?;
            let functions: Option<FloatFunctions> = match operation {
                ArrayUnaryOp::Sqrt => Some((f32::sqrt, f64::sqrt)),
                ArrayUnaryOp::Exp => Some((f32::exp, f64::exp)),
                ArrayUnaryOp::Log => Some((f32::ln, f64::ln)),
                ArrayUnaryOp::Sin => Some((f32::sin, f64::sin)),
                ArrayUnaryOp::Cos => Some((f32::cos, f64::cos)),
                _ => None,
            };
            match functions {
                Some((single, double)) => match float_map(first, single, double)? {
                    Some(mapped) => Ok(mapped),
                    None => first.unary(operation),
                },
                None => first.unary(operation),
            }
        }
        Operation::Compare(code) => first.compare(
            &other(1),
            ArrayCompareOp::from_code(*code).ok_or_else(unknown)?,
        ),
        Operation::Where => {
            if inputs[1].dtype() != inputs[2].dtype() {
                return Err(tensor_error(
                    "tensor::choose",
                    "expects two tensors of one dtype",
                ));
            }
            first.choose(&other(1), &other(2), inputs[1].dtype())
        }
        Operation::Reduce { reduction, axis } => {
            let float = matches!(first.dtype(), ArrayDType::Float32 | ArrayDType::Float64);
            // A tensor's float sum has no fixed order, so the CPU device
            // takes the reordered sum, which is faster and loses less.
            let reduction = match ArrayReduction::from_code(*reduction).ok_or_else(unknown)? {
                ArrayReduction::Sum if float => ArrayReduction::FastSum,
                ArrayReduction::Mean if float => ArrayReduction::FastMean,
                reduction => reduction,
            };
            let reduced = match axis {
                Some(axis) => first.reduce_axis(reduction, *axis)?,
                None => {
                    let value = first.reduce(reduction)?;
                    let dtype = match reduction {
                        ArrayReduction::Mean | ArrayReduction::FastMean => ArrayDType::Float64,
                        _ => first.dtype(),
                    };
                    ArrayValue::full(dtype, Box::new([1]), &value)?
                }
            };
            // A mean keeps a float tensor's dtype.
            if reduced.dtype() != first.dtype()
                && matches!(first.dtype(), ArrayDType::Float32 | ArrayDType::Float64)
            {
                reduced.cast(first.dtype())
            } else {
                Ok(reduced)
            }
        }
        Operation::Matmul => first.matmul(inputs[1]),
        Operation::Reshape(shape) => first.reshape(shape),
        Operation::Permute(axes) if axes.is_empty() => first.transpose(),
        Operation::Permute(axes) => first.permute(axes),
        Operation::Cast(dtype) => first.cast(ArrayDType::from_code(*dtype).ok_or_else(unknown)?),
        Operation::Slice {
            axis,
            start,
            step,
            length,
        } => tensor_kernels::slice(first, *axis, *start, *step, *length),
        Operation::Unslice {
            axis,
            start,
            step,
            size,
        } => tensor_kernels::unslice(first, *axis, *start, *step, *size),
        Operation::Expand(shape) => tensor_kernels::expand(first, shape),
        Operation::Concat { axis } => tensor_kernels::concat(first, inputs[1], *axis),
        Operation::Take { axis } => tensor_kernels::take(first, inputs[1], *axis),
        Operation::IndexAdd { axis, size } => {
            tensor_kernels::index_add(first, inputs[1], *axis, *size)
        }
        Operation::ArgReduce { largest, axis } => tensor_kernels::arg_reduce(first, *axis, *largest),
    }
}

/// The tensor `operation` gives over `inputs`, which are on one device. A
/// GPU device keeps the operation to run with the ones after it. The CPU
/// device, and a GPU device whose library lacks the operation, run it now.
fn operate(operation: Operation, inputs: &[&TensorValue]) -> Result<Value> {
    let device = inputs[0].device();
    if defers(device) {
        if let Some((dtype, shape)) = planned_result(&operation, inputs) {
            let depth = 1 + inputs.iter().map(|input| input.depth()).max().unwrap_or(0);
            let node = TensorValue::in_state(
                device,
                dtype,
                shape,
                State::Pending(Pending {
                    operation: operation.clone(),
                    inputs: inputs.iter().map(|&input| input.clone()).collect(),
                    depth,
                }),
            );
            let node = recorded(node, operation, inputs);
            if depth > MAX_PENDING_DEPTH {
                node.storage()?;
            }
            return Ok(Value::Tensor(node));
        }
    }
    let arrays = inputs
        .iter()
        .map(|input| input.elements())
        .collect::<Result<Vec<_>>>()?;
    let result = apply(&operation, &arrays.iter().collect::<Vec<_>>())?;
    if !device.has_float64() && result.dtype() == ArrayDType::Float64 {
        return Err(float64_error(device));
    }
    Ok(Value::Tensor(recorded(
        TensorValue::host(device, result),
        operation,
        inputs,
    )))
}

thread_local! {
    /// Off while a gradient is being computed, so the operations of the
    /// gradient are not themselves recorded.
    static RECORDING: std::cell::Cell<bool> = const { std::cell::Cell::new(true) };
}

/// The new tensor `node`, tracked when it is a float tensor computed from
/// a tracked one.
fn recorded(node: TensorValue, operation: Operation, inputs: &[&TensorValue]) -> TensorValue {
    let float = matches!(node.dtype(), ArrayDType::Float32 | ArrayDType::Float64);
    if float && RECORDING.with(std::cell::Cell::get) && inputs.iter().any(|input| input.is_tracked())
    {
        node.tracking(Some(History {
            operation,
            inputs: inputs.iter().map(|&input| input.clone()).collect(),
        }))
    } else {
        node
    }
}

/// Metal has no `float64`, and a tensor never narrows one silently.
fn float64_error(device: TensorDevice) -> Diagnostic {
    Diagnostic::coded(
        "AU4007",
        format!(
            "the {} device has no float64 elements; convert to float32 first",
            device.name()
        ),
    )
}

/// A tensor of `elements` on `device`, in the device's own memory when it
/// has one.
fn place(device: TensorDevice, elements: ArrayValue) -> Result<Value> {
    if !device.has_float64() && elements.dtype() == ArrayDType::Float64 {
        return Err(float64_error(device));
    }
    #[cfg(target_os = "linux")]
    if device == TensorDevice::Cuda && !elements.is_empty() {
        let buffer = tensor_cuda::Buffer::from_array(&elements)?;
        return Ok(Value::Tensor(TensorValue::ready(
            device,
            elements.dtype(),
            elements.shape.clone(),
            Storage::Cuda(Arc::new(buffer)),
        )));
    }
    #[cfg(target_os = "macos")]
    if device == TensorDevice::Metal && !elements.is_empty() {
        let buffer = tensor_metal::Buffer::from_array(&elements)?;
        return Ok(Value::Tensor(TensorValue::ready(
            device,
            elements.dtype(),
            elements.shape.clone(),
            Storage::Metal(Arc::new(buffer)),
        )));
    }
    Ok(Value::Tensor(TensorValue::host(device, elements)))
}

fn tensor_error(name: &str, message: impl std::fmt::Display) -> Diagnostic {
    Diagnostic::coded(
        "AU4007",
        format!(
            "`{}` {message}",
            name.replace("::__aura_", ".").replace("::", ".")
        ),
    )
}

fn node_arg<'a>(args: &'a [Value], index: usize, name: &str) -> Result<&'a TensorValue> {
    match &args[index] {
        Value::Tensor(node) => Ok(node),
        _ => Err(tensor_error(name, "expects a tensor")),
    }
}

fn array_arg<'a>(args: &'a [Value], index: usize, name: &str) -> Result<&'a ArrayValue> {
    match &args[index] {
        Value::Array(array) => Ok(array),
        _ => Err(tensor_error(name, "expects an `Array`")),
    }
}

fn int_arg(args: &[Value], index: usize, name: &str) -> Result<i64> {
    match &args[index] {
        Value::Int(value) => value.as_i128().and_then(|value| i64::try_from(value).ok()),
        _ => None,
    }
    .ok_or_else(|| tensor_error(name, "expects an `int64`"))
}

fn bool_arg(args: &[Value], index: usize, name: &str) -> Result<bool> {
    match &args[index] {
        Value::Bool(value) => Ok(*value),
        _ => Err(tensor_error(name, "expects a `bool`")),
    }
}

fn int_list_arg(args: &[Value], index: usize, name: &str) -> Result<Vec<i64>> {
    let Value::Vec(list) = &args[index] else {
        return Err(tensor_error(name, "expects a `list[int64]`"));
    };
    list.elements
        .iter()
        .map(|element| match &*element {
            Value::Int(value) => value.as_i128().and_then(|value| i64::try_from(value).ok()),
            _ => None,
        })
        .collect::<Option<Vec<_>>>()
        .ok_or_else(|| tensor_error(name, "expects a `list[int64]`"))
}

fn device_arg(args: &[Value], index: usize, name: &str) -> Result<TensorDevice> {
    let device = TensorDevice::from_code(int_arg(args, index, name)?)
        .ok_or_else(|| tensor_error(name, "was given an unknown device"))?;
    if device.is_available() {
        Ok(device)
    } else {
        Err(tensor_error(
            name,
            format!("cannot use the {} device on this machine", device.name()),
        ))
    }
}

fn shape_arg(args: &[Value], index: usize, name: &str) -> Result<Box<[usize]>> {
    int_list_arg(args, index, name)?
        .into_iter()
        .map(|dimension| usize::try_from(dimension).ok())
        .collect::<Option<Box<[usize]>>>()
        .ok_or_else(|| tensor_error(name, "expects a shape with no negative dimension"))
}

/// Both operands of a two-tensor operation, which must be on one device.
fn pair_args<'a>(args: &'a [Value], name: &str) -> Result<(&'a TensorValue, &'a TensorValue)> {
    let left = node_arg(args, 0, name)?;
    let right = node_arg(args, 1, name)?;
    if left.device() != right.device() {
        return Err(tensor_error(
            name,
            format!(
                "expects tensors on one device, found {} and {}",
                left.device().name(),
                right.device().name()
            ),
        ));
    }
    Ok((left, right))
}

fn int_value(value: i64) -> Value {
    Value::Int(IntegerValue::from_i64(value))
}

/// `tensor::__aura_device_available`.
pub fn host_tensor_device_available(args: Vec<Value>, _: Option<&[String]>) -> Result<Value> {
    let name = "tensor::__aura_device_available";
    host_expect_arity(name, &args, 1)?;
    let device = TensorDevice::from_code(int_arg(&args, 0, name)?);
    Ok(Value::Bool(device.is_some_and(TensorDevice::is_available)))
}

/// `tensor::__aura_from_array`: a tensor holding a copy of an Array's
/// elements.
pub fn host_tensor_from_array(args: Vec<Value>, _: Option<&[String]>) -> Result<Value> {
    let name = "tensor::__aura_from_array";
    host_expect_arity(name, &args, 2)?;
    let device = device_arg(&args, 1, name)?;
    place(device, array_arg(&args, 0, name)?.detached()?)
}

/// `tensor::__aura_to_array`: an Array holding a copy of a tensor's
/// elements.
pub fn host_tensor_to_array(args: Vec<Value>, _: Option<&[String]>) -> Result<Value> {
    let name = "tensor::__aura_to_array";
    host_expect_arity(name, &args, 1)?;
    Ok(Value::Array(
        node_arg(&args, 0, name)?.elements()?.try_clone()?,
    ))
}

/// `tensor::__aura_item`: the element of a one-element tensor.
pub fn host_tensor_item(args: Vec<Value>, _: Option<&[String]>) -> Result<Value> {
    let name = "tensor::__aura_item";
    host_expect_arity(name, &args, 1)?;
    let node = node_arg(&args, 0, name)?;
    if node.len() != 1 {
        return Err(tensor_error(
            name,
            format!(
                "expects a tensor of one element, found shape {:?}",
                node.shape()
            ),
        ));
    }
    Ok(node.elements()?.value_at_flat(0))
}

/// `tensor::__aura_full`: a tensor of one value.
pub fn host_tensor_full(args: Vec<Value>, _: Option<&[String]>) -> Result<Value> {
    let name = "tensor::__aura_full";
    host_expect_arity(name, &args, 4)?;
    let shape = shape_arg(&args, 0, name)?;
    let dtype = ArrayDType::from_code(int_arg(&args, 2, name)?)
        .ok_or_else(|| tensor_error(name, "was given an unknown dtype"))?;
    let device = device_arg(&args, 3, name)?;
    place(device, ArrayValue::full(dtype, shape, &args[1])?)
}

/// `tensor::__aura_shape`.
pub fn host_tensor_shape(args: Vec<Value>, _: Option<&[String]>) -> Result<Value> {
    let name = "tensor::__aura_shape";
    host_expect_arity(name, &args, 1)?;
    let shape = node_arg(&args, 0, name)?.shape();
    let mut elements = try_array_buffer(shape.len(), "Tensor.shape result")?;
    elements.extend(shape.iter().map(|&dimension| {
        int_value(i64::try_from(dimension).expect("a live tensor's shape fits int64"))
    }));
    Ok(Value::Vec(VecValue {
        element_type: SharedType::named("int64"),
        elements: elements.into(),
        str_view: Default::default(),
    }))
}

/// `tensor::__aura_len`.
pub fn host_tensor_len(args: Vec<Value>, _: Option<&[String]>) -> Result<Value> {
    let name = "tensor::__aura_len";
    host_expect_arity(name, &args, 1)?;
    let len = node_arg(&args, 0, name)?.len();
    Ok(int_value(
        i64::try_from(len).expect("a live tensor's length fits int64"),
    ))
}

/// `tensor::__aura_device`.
pub fn host_tensor_device(args: Vec<Value>, _: Option<&[String]>) -> Result<Value> {
    let name = "tensor::__aura_device";
    host_expect_arity(name, &args, 1)?;
    Ok(int_value(node_arg(&args, 0, name)?.device() as i64))
}

/// `tensor::__aura_binary`: arithmetic, minimum, maximum, or power over
/// two tensors whose shapes broadcast.
pub fn host_tensor_binary(args: Vec<Value>, _: Option<&[String]>) -> Result<Value> {
    let name = "tensor::__aura_binary";
    host_expect_arity(name, &args, 3)?;
    let (left, right) = pair_args(&args, name)?;
    operate(Operation::Binary(int_arg(&args, 2, name)?), &[left, right])
}

/// `tensor::__aura_unary`: one function of every element.
pub fn host_tensor_unary(args: Vec<Value>, _: Option<&[String]>) -> Result<Value> {
    let name = "tensor::__aura_unary";
    host_expect_arity(name, &args, 2)?;
    let operand = node_arg(&args, 0, name)?;
    operate(Operation::Unary(int_arg(&args, 1, name)?), &[operand])
}

/// `tensor::__aura_compare`: a mask of where a comparison holds.
pub fn host_tensor_compare(args: Vec<Value>, _: Option<&[String]>) -> Result<Value> {
    let name = "tensor::__aura_compare";
    host_expect_arity(name, &args, 3)?;
    let (left, right) = pair_args(&args, name)?;
    operate(Operation::Compare(int_arg(&args, 2, name)?), &[left, right])
}

/// `tensor::__aura_where`: each element from the second tensor where the
/// mask holds, else from the third.
pub fn host_tensor_where(args: Vec<Value>, _: Option<&[String]>) -> Result<Value> {
    let name = "tensor::__aura_where";
    host_expect_arity(name, &args, 3)?;
    let (mask, when_true) = pair_args(&args, name)?;
    let (_, when_false) = pair_args(&args[1..], name)?;
    operate(Operation::Where, &[mask, when_true, when_false])
}

/// `tensor::__aura_reduce`: a reduction over one axis, or over every
/// element when `has_axis` is false, which gives a tensor of shape `[1]`.
/// A mean keeps a float tensor's dtype.
pub fn host_tensor_reduce(args: Vec<Value>, _: Option<&[String]>) -> Result<Value> {
    let name = "tensor::__aura_reduce";
    host_expect_arity(name, &args, 4)?;
    let operand = node_arg(&args, 0, name)?;
    let reduction = int_arg(&args, 1, name)?;
    let axis = bool_arg(&args, 3, name)?.then_some(int_arg(&args, 2, name)?);
    operate(Operation::Reduce { reduction, axis }, &[operand])
}

/// `tensor::__aura_matmul`.
pub fn host_tensor_matmul(args: Vec<Value>, _: Option<&[String]>) -> Result<Value> {
    let name = "tensor::__aura_matmul";
    host_expect_arity(name, &args, 2)?;
    let (left, right) = pair_args(&args, name)?;
    operate(Operation::Matmul, &[left, right])
}

/// `tensor::__aura_reshape`.
pub fn host_tensor_reshape(args: Vec<Value>, _: Option<&[String]>) -> Result<Value> {
    let name = "tensor::__aura_reshape";
    host_expect_arity(name, &args, 2)?;
    let operand = node_arg(&args, 0, name)?;
    operate(
        Operation::Reshape(int_list_arg(&args, 1, name)?),
        &[operand],
    )
}

/// `tensor::__aura_permute`: the axes in a new order. An empty order
/// reverses them.
pub fn host_tensor_permute(args: Vec<Value>, _: Option<&[String]>) -> Result<Value> {
    let name = "tensor::__aura_permute";
    host_expect_arity(name, &args, 2)?;
    let operand = node_arg(&args, 0, name)?;
    operate(
        Operation::Permute(int_list_arg(&args, 1, name)?),
        &[operand],
    )
}

/// `tensor::__aura_cast`: every element converted to another dtype.
pub fn host_tensor_cast(args: Vec<Value>, _: Option<&[String]>) -> Result<Value> {
    let name = "tensor::__aura_cast";
    host_expect_arity(name, &args, 2)?;
    let operand = node_arg(&args, 0, name)?;
    let dtype = int_arg(&args, 1, name)?;
    if dtype == operand.dtype() as i64 {
        return Ok(Value::Tensor(operand.clone()));
    }
    if !operand.device().has_float64() && dtype == ArrayDType::Float64 as i64 {
        return Err(float64_error(operand.device()));
    }
    operate(Operation::Cast(dtype), &[operand])
}

/// `tensor::__aura_to_device`: the tensor on another device, or the same
/// tensor when it is already there.
pub fn host_tensor_to_device(args: Vec<Value>, _: Option<&[String]>) -> Result<Value> {
    let name = "tensor::__aura_to_device";
    host_expect_arity(name, &args, 2)?;
    let operand = node_arg(&args, 0, name)?;
    let device = device_arg(&args, 1, name)?;
    if device == operand.device() {
        return Ok(Value::Tensor(operand.clone()));
    }
    place(device, operand.elements()?)
}

/// `tensor::__aura_eval`: runs the work a tensor is waiting on. The CPU
/// device runs each operation when it is asked for, so nothing waits.
pub fn host_tensor_eval(args: Vec<Value>, _: Option<&[String]>) -> Result<Value> {
    let name = "tensor::__aura_eval";
    host_expect_arity(name, &args, 1)?;
    node_arg(&args, 0, name)?.storage()?;
    Ok(Value::Unit)
}

/// `tensor::__aura_share`: another handle to the same memory.
pub fn host_tensor_share(args: Vec<Value>, _: Option<&[String]>) -> Result<Value> {
    let name = "tensor::__aura_share";
    host_expect_arity(name, &args, 1)?;
    Ok(Value::Tensor(node_arg(&args, 0, name)?.clone()))
}

/// `tensor::__aura_scalar_like`: one number as a tensor of the node's
/// dtype, on its device, for an operator with a number on one side. A
/// float tensor takes any number. An integer tensor takes an integer its
/// dtype holds, and no float, which would lose its fraction unseen.
pub fn host_tensor_scalar_like(args: Vec<Value>, _: Option<&[String]>) -> Result<Value> {
    let name = "tensor::__aura_scalar_like";
    host_expect_arity(name, &args, 2)?;
    let like = node_arg(&args, 0, name)?;
    let dtype = like.dtype();
    let type_name = dtype.runtime_type_name();
    let storage = match (&args[1], dtype) {
        (Value::Float(value), ArrayDType::Float32) => {
            ArrayStorage::Float32(vec![*value as f32].into())
        }
        (Value::Float(value), ArrayDType::Float64) => ArrayStorage::Float64(vec![*value].into()),
        (Value::Float(_), _) => {
            return Err(tensor_error(
                name,
                format!("cannot use a float with a `Tensor[{type_name}]`"),
            ))
        }
        (_, ArrayDType::Bool) => {
            return Err(tensor_error(
                name,
                "cannot use a number with a `Tensor[bool]`",
            ))
        }
        (_, dtype) => {
            let value = int_arg(&args, 1, name)?;
            match dtype {
                ArrayDType::Float32 => ArrayStorage::Float32(vec![value as f32].into()),
                ArrayDType::Float64 => ArrayStorage::Float64(vec![value as f64].into()),
                ArrayDType::Int64 => ArrayStorage::Int64(vec![value].into()),
                _ => ArrayStorage::Int32(
                    vec![i32::try_from(value).map_err(|_| {
                        tensor_error(
                            name,
                            format!("was given {value}, which `int32` cannot hold"),
                        )
                    })?]
                    .into(),
                ),
            }
        }
    };
    place(like.device(), ArrayValue::new(Box::new([1]), storage)?)
}

/// The axis `axis` names in a tensor of `rank` axes. A negative axis
/// counts from the last.
fn axis_arg(axis: i64, rank: usize, name: &str) -> Result<usize> {
    let wrapped = if axis < 0 { axis + rank as i64 } else { axis };
    usize::try_from(wrapped)
        .ok()
        .filter(|&axis| axis < rank)
        .ok_or_else(|| {
            tensor_error(
                name,
                format!("was given axis {axis} for a tensor of rank {rank}"),
            )
        })
}

fn parameters<const N: usize>(args: &[Value], index: usize, name: &str) -> Result<[i64; N]> {
    <[i64; N]>::try_from(int_list_arg(args, index, name)?)
        .map_err(|_| tensor_error(name, format!("expects {N} parameters")))
}

/// `tensor::__aura_node_op`: an operation on one tensor that takes
/// integer parameters. The codes are the ones `std/tensor.au` passes.
pub fn host_tensor_node_op(args: Vec<Value>, _: Option<&[String]>) -> Result<Value> {
    let name = "tensor::__aura_node_op";
    host_expect_arity(name, &args, 3)?;
    let operand = node_arg(&args, 0, name)?;
    let rank = operand.shape().len();
    let float = matches!(operand.dtype(), ArrayDType::Float32 | ArrayDType::Float64);
    match int_arg(&args, 1, name)? {
        0 => {
            let name = "tensor::slice";
            let [axis, start, stop, step] = parameters(&args, 2, name)?;
            let axis = axis_arg(axis, rank, name)?;
            let length = operand.shape()[axis] as i64;
            if step < 1 {
                return Err(tensor_error(name, "expects a step of at least 1"));
            }
            // A negative bound counts from the end, as in a list slice.
            let bound = |at: i64| (if at < 0 { at + length } else { at }).clamp(0, length);
            let (start, stop) = (bound(start), bound(stop));
            let taken = ((stop - start).max(0) + step - 1) / step;
            operate(
                Operation::Slice {
                    axis,
                    start: start as usize,
                    step: step as usize,
                    length: taken as usize,
                },
                &[operand],
            )
        }
        1 => {
            let name = "tensor::broadcast_to";
            let shape = shape_arg(&args, 2, name)?;
            if array_layout::broadcast_shape(operand.shape(), &shape).as_deref() != Some(&*shape) {
                return Err(tensor_error(
                    name,
                    format!(
                        "cannot broadcast shape {:?} to {:?}",
                        operand.shape(),
                        &*shape
                    ),
                ));
            }
            operate(Operation::Expand(shape.into_vec()), &[operand])
        }
        code @ (2 | 3) => {
            let name = "tensor::argmax";
            let [axis] = parameters(&args, 2, name)?;
            if operand.dtype() == ArrayDType::Bool || operand.is_empty() {
                return Err(tensor_error(
                    name,
                    "expects a tensor of numbers with at least one element",
                ));
            }
            operate(
                Operation::ArgReduce {
                    largest: code == 2,
                    axis: axis_arg(axis, rank, name)?,
                },
                &[operand],
            )
        }
        4 => {
            if !float {
                return Err(tensor_error(
                    "tensor::watch",
                    "expects a float tensor; a gradient exists for floats only",
                ));
            }
            Ok(Value::Tensor(
                TensorValue::ready(
                    operand.device(),
                    operand.dtype(),
                    operand.shape().into(),
                    operand.storage()?,
                )
                .tracking(None),
            ))
        }
        5 => {
            if !operand.is_tracked() {
                return Ok(Value::Tensor(operand.clone()));
            }
            let state = match &*operand.state() {
                State::Ready(storage) => State::Ready(storage.clone()),
                // The same waiting work, read through a reshape to the
                // same shape, which is not recorded.
                State::Pending(pending) => State::Pending(Pending {
                    operation: Operation::Reshape(
                        operand
                            .shape()
                            .iter()
                            .map(|&dimension| dimension as i64)
                            .collect(),
                    ),
                    inputs: vec![operand.clone()],
                    depth: pending.depth + 1,
                }),
            };
            Ok(Value::Tensor(TensorValue::in_state(
                operand.device(),
                operand.dtype(),
                operand.shape().into(),
                state,
            )))
        }
        _ => Err(tensor_error(name, "was given an unknown operation")),
    }
}

/// `tensor::__aura_pair_op`: an operation on two tensors that takes
/// integer parameters. The codes are the ones `std/tensor.au` passes.
pub fn host_tensor_pair_op(args: Vec<Value>, _: Option<&[String]>) -> Result<Value> {
    let name = "tensor::__aura_pair_op";
    host_expect_arity(name, &args, 4)?;
    let (left, right) = pair_args(&args, name)?;
    let rank = left.shape().len();
    let integer = |node: &TensorValue| matches!(node.dtype(), ArrayDType::Int32 | ArrayDType::Int64);
    match int_arg(&args, 2, name)? {
        0 => {
            let name = "tensor::concat";
            let [axis] = parameters(&args, 3, name)?;
            let axis = axis_arg(axis, rank, name)?;
            let fits = right.shape().len() == rank
                && (0..rank).all(|at| at == axis || left.shape()[at] == right.shape()[at]);
            if !fits || left.dtype() != right.dtype() {
                return Err(tensor_error(
                    name,
                    format!(
                        "expects tensors of one dtype whose shapes differ on axis {axis} only, found {:?} and {:?}",
                        left.shape(),
                        right.shape()
                    ),
                ));
            }
            operate(Operation::Concat { axis }, &[left, right])
        }
        1 => {
            let name = "tensor::take";
            let [axis] = parameters(&args, 3, name)?;
            if !integer(right) {
                return Err(tensor_error(name, "expects integer indices"));
            }
            operate(
                Operation::Take {
                    axis: axis_arg(axis, rank, name)?,
                },
                &[left, right],
            )
        }
        2 => {
            let name = "tensor::index_add";
            let [axis, size] = parameters(&args, 3, name)?;
            let axis = axis_arg(axis, rank, name)?;
            let size = usize::try_from(size)
                .map_err(|_| tensor_error(name, "expects a size that is not negative"))?;
            if !integer(right) {
                return Err(tensor_error(name, "expects integer indices"));
            }
            if left.dtype() == ArrayDType::Bool {
                return Err(tensor_error(name, "expects a tensor of numbers"));
            }
            if !left.shape()[axis..].starts_with(right.shape()) {
                return Err(tensor_error(
                    name,
                    format!(
                        "expects the indices' shape {:?} to be the tensor's shape {:?} from axis {axis}",
                        right.shape(),
                        left.shape()
                    ),
                ));
            }
            operate(Operation::IndexAdd { axis, size }, &[left, right])
        }
        3 => tensor_grad::gradient(left, right).map(Value::Tensor),
        _ => Err(tensor_error(name, "was given an unknown operation")),
    }
}

/// `tensor::__aura_filled`: a tensor of random floats, or of the integers
/// from zero. Element `j` of a random tensor depends on the seed and `j`
/// only, so one seed gives the same tensor on every device.
pub fn host_tensor_filled(args: Vec<Value>, _: Option<&[String]>) -> Result<Value> {
    let name = "tensor::__aura_filled";
    host_expect_arity(name, &args, 5)?;
    let shape = shape_arg(&args, 0, name)?;
    let fill = match int_arg(&args, 1, name)? {
        0 => tensor_kernels::Fill::Uniform,
        1 => tensor_kernels::Fill::Normal,
        2 => tensor_kernels::Fill::Count,
        _ => return Err(tensor_error(name, "was given an unknown fill")),
    };
    let seed = int_arg(&args, 2, name)? as u64;
    let dtype = ArrayDType::from_code(int_arg(&args, 3, name)?)
        .ok_or_else(|| tensor_error(name, "was given an unknown dtype"))?;
    let device = device_arg(&args, 4, name)?;
    place(device, tensor_kernels::filled(fill, shape, seed, dtype)?)
}

/// The suffixes of the host functions that exist once for each dtype,
/// because their Aura signatures name an `Array[T]` or a `T`.
const DTYPE_SUFFIXES: [&str; 5] = ["_i32", "_i64", "_f32", "_f64", "_bool"];

/// The interpreter's entry for a `tensor` host builtin.
pub fn tensor_host_function(name: &str) -> Option<HostBuiltinFn> {
    let generic = DTYPE_SUFFIXES
        .iter()
        .find_map(|suffix| name.strip_suffix(suffix))
        .unwrap_or(name);
    Some(match generic {
        "tensor::__aura_device_available" => host_tensor_device_available,
        "tensor::__aura_from_array" => host_tensor_from_array,
        "tensor::__aura_to_array" => host_tensor_to_array,
        "tensor::__aura_item" => host_tensor_item,
        "tensor::__aura_full" => host_tensor_full,
        "tensor::__aura_shape" => host_tensor_shape,
        "tensor::__aura_len" => host_tensor_len,
        "tensor::__aura_device" => host_tensor_device,
        "tensor::__aura_binary" => host_tensor_binary,
        "tensor::__aura_unary" => host_tensor_unary,
        "tensor::__aura_compare" => host_tensor_compare,
        "tensor::__aura_where" => host_tensor_where,
        "tensor::__aura_reduce" => host_tensor_reduce,
        "tensor::__aura_matmul" => host_tensor_matmul,
        "tensor::__aura_reshape" => host_tensor_reshape,
        "tensor::__aura_permute" => host_tensor_permute,
        "tensor::__aura_cast" => host_tensor_cast,
        "tensor::__aura_to_device" => host_tensor_to_device,
        "tensor::__aura_eval" => host_tensor_eval,
        "tensor::__aura_share" => host_tensor_share,
        "tensor::__aura_scalar_like" => host_tensor_scalar_like,
        "tensor::__aura_node_op" => host_tensor_node_op,
        "tensor::__aura_pair_op" => host_tensor_pair_op,
        "tensor::__aura_filled" => host_tensor_filled,
        _ => return None,
    })
}

#[cfg(test)]
#[path = "tensor_host_tests.rs"]
mod tests;
