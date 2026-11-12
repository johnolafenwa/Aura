//! Turns a tensor [`Plan`] into CUDA C.
//!
//! The CUDA libraries have no general elementwise operations, so the CUDA
//! device compiles each plan itself. Elementwise steps, with reshapes and
//! permutes, fuse: a chain of them becomes one kernel that reads the
//! leaves and writes one result, with no buffer in between. A reduction
//! fuses the expression it reduces. `matmul` is a call to cuBLAS.
//!
//! A plan holds every shape, so the source has them as constants and the
//! compiled plan is reused for every run of the same expression over the
//! same shapes.
//!
//! Nothing here calls CUDA. `tensor_cuda.rs` compiles the source with
//! NVRTC and runs the actions.

use super::{Operation, Plan, Slot, NEGATE_OPERATION, PAIR_OPERATION_BASE};
use crate::runtime_value::ArrayDType;
use std::fmt::Write;

/// One step of running a compiled plan. A buffer number below the plan's
/// slot count is that slot's memory. Higher numbers are scratch.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum Action {
    /// Launches kernel `name` over `threads` threads. Its first argument is
    /// a new buffer of `bytes` bytes, numbered `output`, and the rest are
    /// the `inputs` buffers in order.
    Kernel {
        name: String,
        output: usize,
        bytes: usize,
        threads: usize,
        inputs: Vec<usize>,
    },
    /// A row-major matrix product of `rows` by `inner` and `inner` by
    /// `columns` matrices into a new buffer numbered `output`. Each pair
    /// is the element offsets of one product's operands, and product `n`
    /// is written at element `n * rows * columns`.
    Matmul {
        left: usize,
        right: usize,
        output: usize,
        bytes: usize,
        double: bool,
        rows: usize,
        inner: usize,
        columns: usize,
        pairs: Vec<(usize, usize)>,
    },
}

/// A plan as CUDA C and the actions that run it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct Program {
    pub(crate) source: String,
    pub(crate) actions: Vec<Action>,
    /// How many buffer numbers the actions use.
    pub(crate) buffers: usize,
}

/// The helpers every kernel may call. Aura's `minimum` and `maximum` give
/// a NaN when either operand is one and order `-0.0` below `0.0`.
const PREAMBLE: &str = r#"#define AURA_INLINE static __device__ __forceinline__
#define AURA_MINMAX(NAME, T, ONE, BEFORE) \
    AURA_INLINE T NAME(T a, T b) { \
        if (a != a) return a; \
        if (b != b) return b; \
        bool first = a BEFORE b; \
        if (a == b) first = copysign(ONE, a) BEFORE copysign(ONE, b); \
        return first ? a : b; \
    }
AURA_MINMAX(aura_min_f, float, 1.0f, <)
AURA_MINMAX(aura_min_d, double, 1.0, <)
AURA_MINMAX(aura_max_f, float, 1.0f, >)
AURA_MINMAX(aura_max_d, double, 1.0, >)
"#;

/// Threads in one block of a kernel launch.
/// The elements one thread reduces in a stage of a whole reduction.
const STAGE_RUN: usize = 64;

/// The most elements the single thread of a last stage reduces.
const LAST_STAGE: usize = 128;

pub(crate) const BLOCK_THREADS: usize = 256;

fn dtype_of(code: i64) -> ArrayDType {
    ArrayDType::from_code(code).expect("a plan holds known dtypes")
}

pub(crate) fn element_bytes(dtype: ArrayDType) -> usize {
    match dtype {
        ArrayDType::Bool => 1,
        ArrayDType::Int32 | ArrayDType::Float32 => 4,
        ArrayDType::Int64 | ArrayDType::Float64 => 8,
    }
}

fn c_type(dtype: ArrayDType) -> &'static str {
    match dtype {
        ArrayDType::Int32 => "int",
        ArrayDType::Int64 => "long long",
        ArrayDType::Float32 => "float",
        ArrayDType::Float64 => "double",
        ArrayDType::Bool => "unsigned char",
    }
}

fn is_float(dtype: ArrayDType) -> bool {
    matches!(dtype, ArrayDType::Float32 | ArrayDType::Float64)
}

/// The row-major stride of each axis of `shape`.
fn strides(shape: &[usize]) -> Vec<usize> {
    let mut strides = vec![1; shape.len()];
    for axis in (0..shape.len().saturating_sub(1)).rev() {
        strides[axis] = strides[axis + 1] * shape[axis + 1];
    }
    strides
}

fn len(shape: &[usize]) -> usize {
    shape.iter().product()
}

/// The C expression for coordinate `axis` of the element at linear index
/// `j` of `shape`.
fn coordinate(shape: &[usize], axis: usize) -> String {
    let stride = strides(shape)[axis];
    match (stride, axis) {
        (1, 0) => "j".to_string(),
        (1, _) => format!("(j % {}LL)", shape[axis]),
        (_, 0) => format!("(j / {stride}LL)"),
        _ => format!("((j / {stride}LL) % {}LL)", shape[axis]),
    }
}

/// The C expression for the linear index in an operand of shape `from`
/// of the element that broadcasts to linear index `j` of shape `to`.
fn broadcast_index(to: &[usize], from: &[usize]) -> String {
    if to == from {
        return "j".to_string();
    }
    if len(from) == 1 {
        return "0".to_string();
    }
    let from_strides = strides(from);
    let lead = to.len() - from.len();
    let terms: Vec<String> = (0..from.len())
        .filter(|&axis| from[axis] != 1)
        .map(|axis| format!("{} * {}LL", coordinate(to, lead + axis), from_strides[axis]))
        .collect();
    format!("({})", terms.join(" + "))
}

/// The linear index in an operand of shape `from` of the element that
/// broadcasts to linear index `index` of shape `to`.
fn broadcast_offset(to: &[usize], from: &[usize], index: usize) -> usize {
    let (to_strides, from_strides) = (strides(to), strides(from));
    let lead = to.len() - from.len();
    (0..from.len())
        .filter(|&axis| from[axis] != 1)
        .map(|axis| (index / to_strides[lead + axis]) % to[lead + axis] * from_strides[axis])
        .sum()
}

struct Generator<'a> {
    plan: &'a Plan,
    /// Whether each slot has a buffer when the steps after it run.
    materialized: Vec<bool>,
    source: String,
    actions: Vec<Action>,
    buffers: usize,
}

fn slot_shape(slot: &Slot) -> &[usize] {
    match slot {
        Slot::Leaf { shape, .. } | Slot::Step { shape, .. } => shape,
    }
}

fn slot_dtype(slot: &Slot) -> ArrayDType {
    match slot {
        Slot::Leaf { dtype, .. } | Slot::Step { dtype, .. } => dtype_of(*dtype),
    }
}

/// Whether a step computes each element from the operand elements at one
/// position, so it can be part of a larger kernel.
fn fuses(operation: &Operation) -> bool {
    !matches!(operation, Operation::Reduce { .. } | Operation::Matmul)
}

impl<'a> Generator<'a> {
    fn new(plan: &'a Plan) -> Self {
        let count = plan.slots.len();
        let mut uses = vec![0_usize; count];
        let mut feeds_matmul = vec![false; count];
        for slot in &plan.slots {
            if let Slot::Step {
                operation, inputs, ..
            } = slot
            {
                for &input in inputs {
                    uses[input] += 1;
                    feeds_matmul[input] |= matches!(operation, Operation::Matmul);
                }
            }
        }
        let materialized = plan
            .slots
            .iter()
            .enumerate()
            .map(|(index, slot)| match slot {
                Slot::Leaf { .. } => true,
                // A step read more than once is computed once.
                Slot::Step { operation, .. } => {
                    !fuses(operation)
                        || index + 1 == count
                        || uses[index] > 1
                        || feeds_matmul[index]
                }
            })
            .collect();
        Self {
            plan,
            materialized,
            source: PREAMBLE.to_string(),
            actions: Vec::new(),
            buffers: count,
        }
    }

    fn shape(&self, slot: usize) -> &'a [usize] {
        slot_shape(&self.plan.slots[slot])
    }

    fn dtype(&self, slot: usize) -> ArrayDType {
        slot_dtype(&self.plan.slots[slot])
    }

    /// The buffers a kernel that computes `slot` reads, in ascending
    /// order.
    fn kernel_leaves(&self, slot: usize, leaves: &mut Vec<usize>) {
        if let Slot::Step { inputs, .. } = &self.plan.slots[slot] {
            for &input in inputs {
                if self.materialized[input] {
                    if !leaves.contains(&input) {
                        leaves.push(input);
                    }
                } else {
                    self.kernel_leaves(input, leaves);
                }
            }
        }
        leaves.sort_unstable();
    }

    /// The buffers the operand `slot` of a kernel reads.
    fn operand_leaves(&self, slot: usize) -> Vec<usize> {
        let mut leaves = Vec::new();
        if self.materialized[slot] {
            leaves.push(slot);
        } else {
            self.kernel_leaves(slot, &mut leaves);
        }
        leaves
    }

    /// The parameters after the output, and the same names as arguments.
    fn signature(&self, leaves: &[usize]) -> (String, String) {
        let parameters = leaves
            .iter()
            .map(|&leaf| format!(", const {}* p{leaf}", c_type(self.dtype(leaf))))
            .collect();
        let arguments = leaves.iter().map(|leaf| format!(", p{leaf}")).collect();
        (parameters, arguments)
    }

    /// The C expression for the element of `slot` at the linear index the
    /// expression `index` gives, inside kernel `kernel`.
    fn element(&self, kernel: usize, slot: usize, index: &str, arguments: &str) -> String {
        if self.materialized[slot] {
            format!("p{slot}[{index}]")
        } else {
            format!("v{kernel}_{slot}({index}{arguments})")
        }
    }

    /// Emits the function that computes one element of the fused step
    /// `slot`, after the functions of its fused operands.
    fn emit_value(&mut self, kernel: usize, slot: usize, leaves: &[usize], done: &mut Vec<usize>) {
        if done.contains(&slot) {
            return;
        }
        done.push(slot);
        let Slot::Step {
            operation, inputs, ..
        } = &self.plan.slots[slot]
        else {
            unreachable!("a leaf is always materialized")
        };
        for &input in inputs {
            if !self.materialized[input] {
                self.emit_value(kernel, input, leaves, done);
            }
        }
        let (parameters, arguments) = self.signature(leaves);
        let shape = self.shape(slot);
        let operand = |position: usize| {
            let input = inputs[position];
            let index = match operation {
                Operation::Reshape(_) => "j".to_string(),
                Operation::Permute(axes) => {
                    let source = self.shape(input);
                    let rank = source.len();
                    let source_strides = strides(source);
                    let terms: Vec<String> = (0..rank)
                        .map(|axis| {
                            let from = if axes.is_empty() {
                                rank - 1 - axis
                            } else if axes[axis] < 0 {
                                (axes[axis] + rank as i64) as usize
                            } else {
                                axes[axis] as usize
                            };
                            format!("{} * {}LL", coordinate(shape, axis), source_strides[from])
                        })
                        .collect();
                    format!("({})", terms.join(" + "))
                }
                _ => broadcast_index(shape, self.shape(input)),
            };
            self.element(kernel, input, &index, &arguments)
        };
        let operand_dtype = self.dtype(inputs[0]);
        let result = c_type(self.dtype(slot));
        let body = match operation {
            Operation::Binary(code) => binary(*code, operand_dtype, &operand(0), &operand(1)),
            Operation::Unary(code) => unary(*code, operand_dtype, &operand(0)),
            Operation::Compare(code) => {
                let symbol = ["<", "<=", ">", ">=", "==", "!="][*code as usize];
                format!("(unsigned char)({} {symbol} {})", operand(0), operand(1))
            }
            Operation::Where => format!("({} ? {} : {})", operand(0), operand(1), operand(2)),
            Operation::Reshape(_) | Operation::Permute(_) => operand(0),
            Operation::Cast(_) => format!("({result})({})", operand(0)),
            Operation::Reduce { .. } | Operation::Matmul => {
                unreachable!("a reduction and a product are never fused")
            }
        };
        writeln!(
            self.source,
            "AURA_INLINE {result} v{kernel}_{slot}(long long j{parameters}) {{ return {body}; }}"
        )
        .expect("writing to a string cannot fail");
    }

    /// Emits the functions that compute an element of operand `slot`.
    fn emit_operand(&mut self, kernel: usize, slot: usize, leaves: &[usize]) {
        if !self.materialized[slot] {
            self.emit_value(kernel, slot, leaves, &mut Vec::new());
        }
    }

    fn scratch(&mut self) -> usize {
        self.buffers += 1;
        self.buffers - 1
    }

    /// A kernel that writes every element of the fused step `slot`.
    fn map(&mut self, slot: usize) {
        let mut leaves = Vec::new();
        self.kernel_leaves(slot, &mut leaves);
        // The step itself is materialized, so emit its function directly.
        self.materialized[slot] = false;
        self.emit_value(slot, slot, &leaves, &mut Vec::new());
        self.materialized[slot] = true;
        let (parameters, arguments) = self.signature(&leaves);
        let count = len(self.shape(slot));
        let result = c_type(self.dtype(slot));
        writeln!(
            self.source,
            "extern \"C\" __global__ void k{slot}({result}* out{parameters}) {{\n    \
             long long j = (long long)blockIdx.x * blockDim.x + threadIdx.x;\n    \
             if (j < {count}LL) out[j] = v{slot}_{slot}(j{arguments});\n}}"
        )
        .expect("writing to a string cannot fail");
        self.actions.push(Action::Kernel {
            name: format!("k{slot}"),
            output: slot,
            bytes: count * element_bytes(self.dtype(slot)),
            threads: count,
            inputs: leaves,
        });
    }

    /// The kernels of the reduction step `slot`.
    fn reduce(&mut self, slot: usize, reduction: i64, axis: Option<i64>, operand: usize) {
        let dtype = self.dtype(slot);
        let ty = c_type(dtype);
        let suffix = if dtype == ArrayDType::Float64 {
            "d"
        } else {
            "f"
        };
        let combine = |acc: &str, value: &str| match reduction {
            1 => format!("aura_min_{suffix}({acc}, {value})"),
            2 => format!("aura_max_{suffix}({acc}, {value})"),
            _ => format!("{acc} + {value}"),
        };
        let leaves = self.operand_leaves(operand);
        self.emit_operand(slot, operand, &leaves);
        let (parameters, arguments) = self.signature(&leaves);
        let source = self.shape(operand);
        let count = len(source);
        let at = |index: &str| self.element(slot, operand, index, &arguments);
        let bytes = element_bytes(dtype);
        match axis {
            Some(axis) => {
                // One thread reduces one lane.
                let axis = if axis < 0 {
                    (axis + source.len() as i64) as usize
                } else {
                    axis as usize
                };
                let length = source[axis];
                let inner = strides(source)[axis];
                let lanes = count / length;
                let finish = if reduction == 3 {
                    format!("acc / ({ty}){length}")
                } else {
                    "acc".to_string()
                };
                let (first, next) = (at("start"), at("(start + k * step)"));
                let step = combine("acc", &next);
                writeln!(
                    self.source,
                    "extern \"C\" __global__ void k{slot}({ty}* out{parameters}) {{\n    \
                     long long lane = (long long)blockIdx.x * blockDim.x + threadIdx.x;\n    \
                     if (lane >= {lanes}LL) return;\n    \
                     const long long step = {inner}LL;\n    \
                     long long start = (lane / step) * ({length}LL * step) + lane % step;\n    \
                     {ty} acc = {first};\n    \
                     for (long long k = 1; k < {length}LL; k++) acc = {step};\n    \
                     out[lane] = {finish};\n}}"
                )
                .expect("writing to a string cannot fail");
                self.actions.push(Action::Kernel {
                    name: format!("k{slot}"),
                    output: slot,
                    bytes: lanes * bytes,
                    threads: lanes,
                    inputs: leaves,
                });
            }
            None => {
                // The reduction runs in stages. In each stage thread `t`
                // of `threads` reduces the elements `t`, `t + threads`,
                // `t + 2 * threads` and so on, so the threads beside each
                // other read memory beside each other, and the next stage
                // reduces what the threads wrote. The last stage is one
                // thread.
                let operand_element = (at("t"), at("j"));
                let mut remaining = count;
                let mut input: Option<usize> = None;
                let mut stage = 0;
                loop {
                    let threads = if remaining <= LAST_STAGE {
                        1
                    } else {
                        remaining.div_ceil(STAGE_RUN)
                    };
                    let last = threads == 1;
                    let (first, next) = match input {
                        None => operand_element.clone(),
                        Some(_) => ("in[t]".to_string(), "in[j]".to_string()),
                    };
                    let step = combine("acc", &next);
                    let finish = if last && reduction == 3 {
                        format!("acc / ({ty}){count}")
                    } else {
                        "acc".to_string()
                    };
                    let parameters = match input {
                        None => parameters.clone(),
                        Some(_) => format!(", const {ty}* in"),
                    };
                    writeln!(
                        self.source,
                        "extern \"C\" __global__ void k{slot}r{stage}({ty}* out{parameters}) {{\n    \
                         long long t = (long long)blockIdx.x * blockDim.x + threadIdx.x;\n    \
                         if (t >= {threads}LL) return;\n    \
                         {ty} acc = {first};\n    \
                         for (long long j = t + {threads}LL; j < {remaining}LL; j += {threads}LL) acc = {step};\n    \
                         out[t] = {finish};\n}}"
                    )
                    .expect("writing to a string cannot fail");
                    let output = if last { slot } else { self.scratch() };
                    self.actions.push(Action::Kernel {
                        name: format!("k{slot}r{stage}"),
                        output,
                        bytes: threads * bytes,
                        threads,
                        inputs: match input {
                            None => leaves.clone(),
                            Some(input) => vec![input],
                        },
                    });
                    if last {
                        break;
                    }
                    input = Some(output);
                    remaining = threads;
                    stage += 1;
                }
            }
        }
    }

    fn matmul(&mut self, slot: usize, left: usize, right: usize) {
        let (left_shape, right_shape, shape) =
            (self.shape(left), self.shape(right), self.shape(slot));
        let (rows, inner) = (
            left_shape[left_shape.len() - 2],
            left_shape[left_shape.len() - 1],
        );
        let columns = right_shape[right_shape.len() - 1];
        let lead = &shape[..shape.len() - 2];
        let left_lead = &left_shape[..left_shape.len() - 2];
        let right_lead = &right_shape[..right_shape.len() - 2];
        let pairs = (0..len(lead))
            .map(|product| {
                (
                    broadcast_offset(lead, left_lead, product) * rows * inner,
                    broadcast_offset(lead, right_lead, product) * inner * columns,
                )
            })
            .collect();
        let dtype = self.dtype(slot);
        self.actions.push(Action::Matmul {
            left,
            right,
            output: slot,
            bytes: len(shape) * element_bytes(dtype),
            double: dtype == ArrayDType::Float64,
            rows,
            inner,
            columns,
            pairs,
        });
    }

    fn generate(mut self) -> Program {
        for slot in 0..self.plan.slots.len() {
            let Slot::Step {
                operation, inputs, ..
            } = &self.plan.slots[slot]
            else {
                continue;
            };
            if !self.materialized[slot] {
                continue;
            }
            match operation {
                Operation::Reduce { reduction, axis } => {
                    self.reduce(slot, *reduction, *axis, inputs[0]);
                }
                Operation::Matmul => self.matmul(slot, inputs[0], inputs[1]),
                _ => self.map(slot),
            }
        }
        Program {
            source: self.source,
            actions: self.actions,
            buffers: self.buffers,
        }
    }
}

/// Integer arithmetic wraps. Signed overflow is undefined in C, so the
/// operation runs on the unsigned type.
fn wrapping(dtype: ArrayDType, left: &str, symbol: &str, right: &str) -> String {
    let (signed, unsigned) = match dtype {
        ArrayDType::Int32 => ("int", "unsigned int"),
        _ => ("long long", "unsigned long long"),
    };
    format!("({signed})(({unsigned}){left} {symbol} ({unsigned}){right})")
}

fn binary(code: i64, dtype: ArrayDType, left: &str, right: &str) -> String {
    let float = is_float(dtype);
    let suffix = if dtype == ArrayDType::Float64 {
        "d"
    } else {
        "f"
    };
    match code {
        0..=2 => {
            let symbol = ["+", "-", "*"][code as usize];
            if float {
                format!("({left} {symbol} {right})")
            } else {
                wrapping(dtype, left, symbol, right)
            }
        }
        3 => format!("({left} / {right})"),
        _ => match code - PAIR_OPERATION_BASE {
            0 if float => format!("aura_min_{suffix}({left}, {right})"),
            1 if float => format!("aura_max_{suffix}({left}, {right})"),
            0 => format!("({left} < {right} ? {left} : {right})"),
            1 => format!("({left} > {right} ? {left} : {right})"),
            _ if dtype == ArrayDType::Float64 => format!("pow({left}, {right})"),
            _ => format!("powf({left}, {right})"),
        },
    }
}

fn unary(code: i64, dtype: ArrayDType, operand: &str) -> String {
    if code == NEGATE_OPERATION {
        return match dtype {
            // The CPU device computes zero minus the element.
            ArrayDType::Float32 => format!("(0.0f - {operand})"),
            ArrayDType::Float64 => format!("(0.0 - {operand})"),
            _ => wrapping(dtype, "0", "-", operand),
        };
    }
    let function = match code {
        0 => "fabs",
        1 => "sqrt",
        2 => "exp",
        3 => "log",
        6 => "sin",
        _ => "cos",
    };
    if dtype == ArrayDType::Float64 {
        format!("{function}({operand})")
    } else {
        format!("{function}f({operand})")
    }
}

/// The CUDA C for `plan` and the actions that run it.
pub(crate) fn generate(plan: &Plan) -> Program {
    Generator::new(plan).generate()
}
