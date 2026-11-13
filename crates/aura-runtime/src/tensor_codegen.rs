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

use super::{
    Operation, Plan, Slot, ERF_OPERATION, NEGATE_OPERATION, PAIR_OPERATION_BASE, SIGMOID_OPERATION,
    TANH_OPERATION,
};
use crate::runtime_value::ArrayDType;
use std::fmt::Write;

/// One step of running a compiled plan. A buffer number below the plan's
/// slot count is that slot's memory. Higher numbers are scratch.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum Action {
    /// Launches kernel `name` over `threads` threads. Its first argument is
    /// a new buffer of `bytes` bytes, numbered `output`, and the rest are
    /// the `inputs` buffers in order. `clear` asks for the new buffer to
    /// hold zeros, for a kernel that adds into it.
    Kernel {
        name: String,
        output: usize,
        bytes: usize,
        threads: usize,
        inputs: Vec<usize>,
        clear: bool,
        /// More buffers the kernel writes, each a buffer number and its
        /// bytes. Their pointers follow the first argument.
        more: Vec<(usize, usize)>,
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
        /// Whether the left and the right buffer hold each matrix with
        /// its rows and columns swapped, so no kernel copies a transpose.
        swapped: (bool, bool),
    },
    /// Buffer `output` is buffer `input`: a reshape of memory that
    /// exists moves no element.
    Alias { output: usize, input: usize },
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
AURA_INLINE unsigned long long aura_bits(long long seed, unsigned long long index) {
    unsigned long long bits = (unsigned long long)seed * 0x9E3779B97F4A7C15ULL
        + index * 0xD1B54A32D192ED03ULL + 0x9E3779B97F4A7C15ULL;
    bits = (bits ^ (bits >> 30)) * 0xBF58476D1CE4E5B9ULL;
    bits = (bits ^ (bits >> 27)) * 0x94D049BB133111EBULL;
    return bits ^ (bits >> 31);
}
AURA_INLINE float aura_unit_f(long long seed, unsigned long long index) {
    return (float)(aura_bits(seed, index) >> 40) * 5.9604644775390625e-8f;
}
AURA_INLINE double aura_unit_d(long long seed, unsigned long long index) {
    return (double)(aura_bits(seed, index) >> 11) * 1.1102230246251565e-16;
}
AURA_INLINE float aura_normal_f(long long seed, unsigned long long index) {
    float first = aura_unit_f(seed, 2 * index), second = aura_unit_f(seed, 2 * index + 1);
    return sqrtf(-2.0f * logf(1.0f - first)) * cosf(6.283185307179586f * second);
}
AURA_INLINE double aura_normal_d(long long seed, unsigned long long index) {
    double first = aura_unit_d(seed, 2 * index), second = aura_unit_d(seed, 2 * index + 1);
    return sqrt(-2.0 * log(1.0 - first)) * cos(6.283185307179586 * second);
}
"#;

/// The elements one thread reduces in a stage of a whole reduction.
const STAGE_RUN: usize = 64;

/// The most elements the single thread of a last stage reduces.
const LAST_STAGE: usize = 128;

/// The shortest lane of a last axis that a group of threads reduces.
const SPLIT_LANE: usize = 32;

/// The shortest lane of another axis that a group of threads reduces.
/// Threads beside each other already read memory beside each other
/// there, so a group only pays for a lane long enough to keep one thread
/// busy.
const SPLIT_STRIDED_LANE: usize = 512;

/// The threads that share a lane of `length` elements `inner` apart, or
/// `None` when one thread reduces the lane. On a last axis, threads
/// beside each other then read memory beside each other, which one thread
/// per lane does not do when a lane is a long run of memory.
fn lane_parts(inner: usize, length: usize) -> Option<usize> {
    let shortest = if inner == 1 {
        SPLIT_LANE
    } else {
        SPLIT_STRIDED_LANE
    };
    if length < shortest {
        return None;
    }
    let mut parts = 8;
    while parts * parts < length {
        parts *= 2;
    }
    Some(parts)
}

/// The most threads of an elementwise kernel that shares its launch.
const GROUPED_THREADS: usize = 1 << 20;

/// The most buffers the kernels of one launch name. A CUDA launch passes
/// at most 4,096 bytes of arguments.
const GROUPED_POINTERS: usize = 256;

/// The most buffers a host test passes to a kernel one by one.
pub(crate) const HARNESS_POINTERS: usize = 7;

/// Threads in one block of a kernel launch.
pub(crate) const BLOCK_THREADS: usize = 256;

/// The C expressions that split the linear index `j` around an axis whose
/// lanes have `length` elements `inner` apart: the lane number before the
/// axis, the place on the axis, and the place after it.
fn split(inner: usize, length: usize) -> (String, String, String) {
    (
        format!("(j / {}LL)", inner * length),
        format!("((j / {inner}LL) % {length}LL)"),
        format!("(j % {inner}LL)"),
    )
}

/// The C expression for the linear index of the element at `at` on an
/// axis of `length` elements `inner` apart, in the lane `split` names.
fn join(inner: usize, length: usize, lane: &(String, String, String), at: &str) -> String {
    format!(
        "(({} * {length}LL + {at}) * {inner}LL + {})",
        lane.0, lane.2
    )
}

fn inner_of(shape: &[usize], axis: usize) -> usize {
    shape[axis + 1..].iter().product()
}

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
pub(crate) fn broadcast_offset(to: &[usize], from: &[usize], index: usize) -> usize {
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
    /// The results each kernel writes beside its own: steps only that
    /// kernel reads, which the program keeps.
    beside: Vec<Vec<usize>>,
    /// The places in `actions` of the elementwise kernels, whose launch
    /// kernels are emitted last.
    maps: Vec<usize>,
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
    !matches!(
        operation,
        Operation::Reduce { .. }
            | Operation::Matmul
            | Operation::IndexAdd { .. }
            | Operation::ArgReduce { .. }
    )
}

/// Whether `operation` over an operand of `rank` axes swaps the last two
/// axes and keeps the rest.
fn swaps_matrix(operation: &Operation, rank: usize) -> bool {
    let Operation::Permute(axes) = operation else {
        return false;
    };
    if axes.is_empty() {
        return rank == 2;
    }
    rank >= 2
        && axes.len() == rank
        && axes.iter().enumerate().all(|(place, &axis)| {
            let wanted = match place {
                place if place == rank - 2 => rank - 1,
                place if place == rank - 1 => rank - 2,
                place => place,
            };
            axis as usize == wanted
        })
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
        let mut materialized: Vec<bool> = plan
            .slots
            .iter()
            .enumerate()
            .map(|(index, slot)| match slot {
                Slot::Leaf { .. } => true,
                // A step read more than once is computed once.
                Slot::Step { operation, .. } => {
                    !fuses(operation)
                        || plan.outputs.contains(&index)
                        || uses[index] > 1
                        || feeds_matmul[index]
                }
            })
            .collect();
        // A reshape of memory that exists is that memory, at no cost.
        for index in 0..count {
            if let Slot::Step {
                operation: Operation::Reshape(_),
                inputs,
                ..
            } = &plan.slots[index]
            {
                materialized[index] |= materialized[inputs[0]];
            }
        }
        // A product reads a transpose of memory that exists in place, so
        // a transpose only products read is never written.
        let mut matmul_uses = vec![0_usize; count];
        for slot in &plan.slots {
            if let Slot::Step {
                operation: Operation::Matmul,
                inputs,
                ..
            } = slot
            {
                for &input in inputs {
                    matmul_uses[input] += 1;
                }
            }
        }
        for index in 0..count {
            if Self::transpose_of(plan, &materialized, index).is_some()
                && uses[index] == matmul_uses[index]
                && !plan.outputs.contains(&index)
            {
                materialized[index] = false;
            }
        }
        // A result the program keeps that only one elementwise kernel
        // reads is written by that kernel, as the moving averages of an
        // optimizer are by the kernel of the new weight.
        let mut user = vec![usize::MAX; count];
        for (index, slot) in plan.slots.iter().enumerate() {
            if let Slot::Step { inputs, .. } = slot {
                for &input in inputs {
                    user[input] = index;
                }
            }
        }
        let elementwise = |index: usize| {
            matches!(&plan.slots[index], Slot::Step { operation, .. }
                if fuses(operation) && !matches!(operation, Operation::Reshape(_)))
        };
        let mut beside = vec![Vec::new(); count];
        for index in (0..count).rev() {
            if !(materialized[index]
                && elementwise(index)
                && uses[index] == 1
                && !feeds_matmul[index]
                && plan.outputs.contains(&index))
            {
                continue;
            }
            let mut owner = user[index];
            while !materialized[owner] {
                owner = user[owner];
            }
            if elementwise(owner)
                && slot_shape(&plan.slots[owner]) == slot_shape(&plan.slots[index])
            {
                materialized[index] = false;
                beside[owner].push(index);
            }
        }
        Self {
            plan,
            materialized,
            beside,
            maps: Vec::new(),
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
        // The element of operand `position` at the linear index `index`.
        let at = |position: usize, index: &str| {
            self.element(kernel, inputs[position], index, &arguments)
        };
        let source = self.shape(inputs[0]);
        let body = match operation {
            Operation::Binary(code) => binary(*code, operand_dtype, &operand(0), &operand(1)),
            Operation::Unary(code) => unary(*code, operand_dtype, &operand(0)),
            Operation::Compare(code) => {
                let symbol = ["<", "<=", ">", ">=", "==", "!="][*code as usize];
                format!("(unsigned char)({} {symbol} {})", operand(0), operand(1))
            }
            Operation::Where => format!("({} ? {} : {})", operand(0), operand(1), operand(2)),
            Operation::Reshape(_) | Operation::Permute(_) | Operation::Expand(_) => operand(0),
            Operation::Cast(_) => format!("({result})({})", operand(0)),
            Operation::Slice { axis, step, .. } => {
                let inner = inner_of(shape, *axis);
                let lane = split(inner, shape[*axis]);
                let place = format!("({} * {step}LL + (long long){})", lane.1, at(1, "0"));
                at(0, &join(inner, source[*axis], &lane, &place))
            }
            Operation::Unslice { axis, step, .. } => {
                let inner = inner_of(shape, *axis);
                let lane = split(inner, shape[*axis]);
                let length = source[*axis];
                let element = at(0, &join(inner, length, &lane, &format!("(c / {step}LL)")));
                format!(
                    "0; long long c = {} - (long long){}; \
                     if (c >= 0 && c % {step}LL == 0 && c / {step}LL < {length}LL) return {element}; \
                     return 0",
                    lane.1,
                    at(1, "0"),
                )
            }
            Operation::Concat { axis } => {
                let inner = inner_of(shape, *axis);
                let lane = split(inner, shape[*axis]);
                let (first, second) = (source[*axis], self.shape(inputs[1])[*axis]);
                format!(
                    "({} < {first}LL ? {} : {})",
                    lane.1,
                    at(0, &join(inner, first, &lane, &lane.1)),
                    at(
                        1,
                        &join(inner, second, &lane, &format!("({} - {first}LL)", lane.1))
                    ),
                )
            }
            Operation::Take { axis } => {
                // An index outside the axis reads as zero. The CPU device
                // reports it.
                let inner = inner_of(source, *axis);
                let lane = split(inner, len(self.shape(inputs[1])));
                let length = source[*axis];
                format!(
                    "0; long long i = (long long){}; \
                     if (i >= 0 && i < {length}LL) return {}; \
                     return 0",
                    at(1, &lane.1),
                    at(0, &join(inner, length, &lane, "i")),
                )
            }
            Operation::Random { fill, dtype, .. } => format!(
                "aura_{}_{}({}, (unsigned long long)j)",
                if *fill == 0 { "unit" } else { "normal" },
                if dtype_of(*dtype) == ArrayDType::Float64 {
                    "d"
                } else {
                    "f"
                },
                at(0, "0"),
            ),
            Operation::Reduce { .. }
            | Operation::Matmul
            | Operation::IndexAdd { .. }
            | Operation::ArgReduce { .. } => {
                unreachable!("a reduction, a product and an indexed sum are never fused")
            }
        };
        // A body that begins `0;` is statements that return on their own.
        let body = match body.strip_prefix("0; ") {
            Some(statements) => format!("{statements};"),
            None => format!("return {body};"),
        };
        writeln!(
            self.source,
            "AURA_INLINE {result} v{kernel}_{slot}(long long j{parameters}) {{ {body} }}"
        )
        .expect("writing to a string cannot fail");
    }

    /// Emits the functions that compute an element of operand `slot`.
    fn emit_operand(&mut self, kernel: usize, slot: usize, leaves: &[usize]) {
        if !self.materialized[slot] {
            self.emit_value(kernel, slot, leaves, &mut Vec::new());
        }
    }

    /// The slot `slot` is the matrix transpose of, when that slot's
    /// memory exists in a run.
    fn transpose_of(plan: &Plan, materialized: &[bool], slot: usize) -> Option<usize> {
        let Slot::Step {
            operation, inputs, ..
        } = &plan.slots[slot]
        else {
            return None;
        };
        let rank = match &plan.slots[inputs[0]] {
            Slot::Leaf { shape, .. } | Slot::Step { shape, .. } => shape.len(),
        };
        (swaps_matrix(operation, rank) && materialized[inputs[0]]).then_some(inputs[0])
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
        // The results written beside this one have its shape, and their
        // functions are already emitted as parts of this kernel.
        let beside = self.beside[slot].clone();
        let (mut outputs, mut writes) = (String::new(), String::new());
        for &other in &beside {
            write!(outputs, ", {}* out{other}", c_type(self.dtype(other)))
                .expect("writing to a string cannot fail");
            write!(writes, " out{other}[j] = v{slot}_{other}(j{arguments});")
                .expect("writing to a string cannot fail");
        }
        // The body is a function of its own, so that one launch can run
        // the bodies of several small kernels.
        writeln!(
            self.source,
            "AURA_INLINE void b{slot}(long long j, {result}* out{outputs}{parameters}) {{\n    \
             out[j] = v{slot}_{slot}(j{arguments});{writes}\n}}"
        )
        .expect("writing to a string cannot fail");
        let more = beside
            .iter()
            .map(|&other| (other, count * element_bytes(self.dtype(other))))
            .collect();
        self.maps.push(self.actions.len());
        self.actions.push(Action::Kernel {
            name: format!("k{slot}"),
            output: slot,
            bytes: count * element_bytes(self.dtype(slot)),
            threads: count,
            inputs: leaves,
            clear: false,
            more,
        });
    }

    /// Emits the kernels of the elementwise actions. Small ones that
    /// follow each other and do not read each other's results share one
    /// launch: each thread runs the body its index falls in. An optimizer
    /// step updates every parameter this way in a few launches.
    fn launch_maps(&mut self) {
        let maps = std::mem::take(&mut self.maps);
        let actions = std::mem::take(&mut self.actions);
        // A small elementwise kernel whose results no later action reads,
        // such as the update of a parameter, moves to the end, where it
        // meets the others like it.
        let small_map = |index: usize, action: &Action| {
            matches!(action, Action::Kernel { threads, .. }
                if maps.contains(&index) && *threads <= GROUPED_THREADS)
        };
        let mut read_later = vec![false; self.buffers];
        let mut last = vec![false; actions.len()];
        for (index, action) in actions.iter().enumerate().rev() {
            if let Action::Kernel { output, more, .. } = action {
                last[index] = small_map(index, action)
                    && !read_later[*output]
                    && !more.iter().any(|(other, _)| read_later[*other]);
            }
            let read: &[usize] = match action {
                Action::Kernel { inputs, .. } => inputs,
                Action::Matmul { left, right, .. } => &[*left, *right],
                Action::Alias { input, .. } => std::slice::from_ref(input),
            };
            for &buffer in read {
                read_later[buffer] = true;
            }
        }
        let (ended, kept): (Vec<_>, Vec<_>) = actions
            .into_iter()
            .enumerate()
            .partition(|(index, _)| last[*index]);
        let actions: Vec<(usize, Action)> = kept.into_iter().chain(ended).collect();
        let maps: Vec<usize> = actions
            .iter()
            .enumerate()
            .filter(|(_, (index, _))| maps.contains(index))
            .map(|(place, _)| place)
            .collect();
        let actions: Vec<Action> = actions.into_iter().map(|(_, action)| action).collect();
        let mut grouped: Vec<Action> = Vec::with_capacity(actions.len());
        // The actions of the group being gathered, and the buffers they
        // write.
        let mut group: Vec<Action> = Vec::new();
        let mut written: Vec<usize> = Vec::new();
        let mut pointers = 0;
        for (index, action) in actions.into_iter().enumerate() {
            let small = match &action {
                Action::Kernel {
                    threads,
                    inputs,
                    more,
                    ..
                } if maps.contains(&index) && *threads <= GROUPED_THREADS => {
                    Some((inputs.clone(), 1 + more.len() + inputs.len()))
                }
                _ => None,
            };
            let joins = small.as_ref().is_some_and(|(inputs, count)| {
                pointers + count <= GROUPED_POINTERS
                    && !inputs.iter().any(|input| written.contains(input))
            });
            if !joins {
                self.launch_group(std::mem::take(&mut group), &mut grouped);
                written.clear();
                pointers = 0;
            }
            match small {
                Some((_, count)) => {
                    if let Action::Kernel { output, more, .. } = &action {
                        written.push(*output);
                        written.extend(more.iter().map(|(other, _)| *other));
                    }
                    pointers += count;
                    group.push(action);
                }
                None if maps.contains(&index) => {
                    self.launch_group(vec![action], &mut grouped);
                }
                None => grouped.push(action),
            }
        }
        self.launch_group(group, &mut grouped);
        self.actions = grouped;
    }

    /// Emits one kernel for the elementwise actions of `group` and adds
    /// its action to `actions`.
    fn launch_group(&mut self, group: Vec<Action>, actions: &mut Vec<Action>) {
        let Some(Action::Kernel { output: first, .. }) = group.first() else {
            return;
        };
        let first = *first;
        let name = if group.len() == 1 {
            format!("k{first}")
        } else {
            format!("g{first}")
        };
        let (mut outputs, mut inputs): (Vec<(usize, usize)>, Vec<usize>) = (Vec::new(), Vec::new());
        let (mut body, mut threads) = (String::new(), 0);
        for action in &group {
            let Action::Kernel {
                output,
                bytes,
                threads: count,
                inputs: read,
                more,
                ..
            } = action
            else {
                unreachable!("a group holds kernels")
            };
            let mut call = format!("b{output}(j, o{output}");
            outputs.push((*output, *bytes));
            for (other, bytes) in more {
                write!(call, ", o{other}").expect("writing to a string cannot fail");
                outputs.push((*other, *bytes));
            }
            for &input in read {
                write!(call, ", p{input}").expect("writing to a string cannot fail");
                if !inputs.contains(&input) {
                    inputs.push(input);
                }
            }
            writeln!(
                body,
                "    if (j < {count}LL) {{ {call}); return; }}\n    j -= {count}LL;"
            )
            .expect("writing to a string cannot fail");
            threads += count;
        }
        let parameters: Vec<String> = outputs
            .iter()
            .map(|(slot, _)| format!("{}* o{slot}", c_type(self.dtype(*slot))))
            .chain(
                inputs
                    .iter()
                    .map(|&slot| format!("const {}* p{slot}", c_type(self.dtype(slot)))),
            )
            .collect();
        writeln!(
            self.source,
            "extern \"C\" __global__ void {name}({}) {{\n    \
             long long j = (long long)blockIdx.x * blockDim.x + threadIdx.x;\n{body}}}",
            parameters.join(", ")
        )
        .expect("writing to a string cannot fail");
        if outputs.len() + inputs.len() > HARNESS_POINTERS {
            // A test on the host calls a kernel of many buffers through
            // one array of them.
            let passed: Vec<String> = outputs
                .iter()
                .map(|(slot, _)| c_type(self.dtype(*slot)))
                .chain(inputs.iter().map(|&slot| c_type(self.dtype(slot))))
                .enumerate()
                .map(|(place, ty)| format!("({ty}*)buffers[{place}]"))
                .collect();
            writeln!(
                self.source,
                "#ifdef AURA_HOST_HARNESS\nextern \"C\" void {name}_all(void** buffers) {{ {name}({}); }}\n#endif",
                passed.join(", ")
            )
            .expect("writing to a string cannot fail");
        }
        actions.push(Action::Kernel {
            name,
            output: outputs[0].0,
            bytes: outputs[0].1,
            threads,
            inputs,
            clear: false,
            more: outputs[1..].to_vec(),
        });
    }

    /// The buffers a kernel reads to compute `operands`, after emitting
    /// the functions of the ones that are fused into kernel `kernel`.
    fn emit_operands(&mut self, kernel: usize, operands: &[usize]) -> Vec<usize> {
        let mut leaves = Vec::new();
        for &operand in operands {
            for leaf in self.operand_leaves(operand) {
                if !leaves.contains(&leaf) {
                    leaves.push(leaf);
                }
            }
        }
        leaves.sort_unstable();
        let mut done = Vec::new();
        for &operand in operands {
            if !self.materialized[operand] {
                self.emit_value(kernel, operand, &leaves, &mut done);
            }
        }
        leaves
    }

    /// The kernel of the indexed sum `slot`: one thread adds one element
    /// of `updates` into a buffer of zeros.
    fn index_add(&mut self, slot: usize, axis: usize, size: usize, updates: usize, indices: usize) {
        let leaves = self.emit_operands(slot, &[updates, indices]);
        let (parameters, arguments) = self.signature(&leaves);
        let ty = c_type(self.dtype(slot));
        let shape = self.shape(slot);
        let inner = inner_of(shape, axis);
        let count = len(self.shape(updates));
        let lane = split(inner, len(self.shape(indices)));
        let index = self.element(slot, indices, &lane.1, &arguments);
        let value = self.element(slot, updates, "j", &arguments);
        let target = join(inner, size, &lane, "i");
        writeln!(
            self.source,
            "extern \"C\" __global__ void k{slot}({ty}* out{parameters}) {{\n    \
             long long j = (long long)blockIdx.x * blockDim.x + threadIdx.x;\n    \
             if (j >= {count}LL) return;\n    \
             long long i = (long long){index};\n    \
             if (i >= 0 && i < {size}LL) atomicAdd(&out[{target}], {value});\n}}"
        )
        .expect("writing to a string cannot fail");
        self.actions.push(Action::Kernel {
            name: format!("k{slot}"),
            output: slot,
            bytes: len(shape) * element_bytes(self.dtype(slot)),
            threads: count,
            inputs: leaves,
            clear: true,
            more: Vec::new(),
        });
    }

    /// The kernel of `slot`, the place of each lane's largest or smallest
    /// element: one thread reads one lane.
    fn arg_reduce(&mut self, slot: usize, largest: bool, axis: usize, operand: usize) {
        let leaves = self.emit_operands(slot, &[operand]);
        let (parameters, arguments) = self.signature(&leaves);
        let ty = c_type(self.dtype(operand));
        let source = self.shape(operand);
        let (length, inner) = (source[axis], inner_of(source, axis));
        let lanes = len(source) / length;
        let first = self.element(slot, operand, "start", &arguments);
        let next = self.element(slot, operand, "(start + k * step)", &arguments);
        let wins = if largest { ">" } else { "<" };
        if let Some(parts) = lane_parts(inner, length).filter(|_| inner == 1) {
            // Thread `part` of a lane finds the best of the places `part`,
            // `part + parts` and so on, then one thread for each lane
            // picks among what its threads found.
            let (first, next, found) = (
                self.element(slot, operand, "(start + part)", &arguments),
                self.element(slot, operand, "(start + k)", &arguments),
                self.element(slot, operand, "(start + at)", &arguments),
            );
            let threads = lanes * parts;
            writeln!(
                self.source,
                "extern \"C\" __global__ void k{slot}r0(long long* out{parameters}) {{\n    \
                 long long t = (long long)blockIdx.x * blockDim.x + threadIdx.x;\n    \
                 if (t >= {threads}LL) return;\n    \
                 long long part = t % {parts}LL;\n    \
                 long long start = (t / {parts}LL) * {length}LL;\n    \
                 {ty} best = {first};\n    \
                 long long place = part;\n    \
                 for (long long k = part + {parts}LL; k < {length}LL; k += {parts}LL) {{\n        \
                 {ty} value = {next};\n        \
                 if (value {wins} best || (value != value && best == best)) {{ best = value; place = k; }}\n    \
                 }}\n    \
                 out[t] = place;\n}}\n\
                 extern \"C\" __global__ void k{slot}(long long* out{parameters}, const long long* in) {{\n    \
                 long long lane = (long long)blockIdx.x * blockDim.x + threadIdx.x;\n    \
                 if (lane >= {lanes}LL) return;\n    \
                 long long start = lane * {length}LL;\n    \
                 long long place = in[lane * {parts}LL];\n    \
                 long long at = place;\n    \
                 {ty} best = {found};\n    \
                 for (long long part = 1; part < {parts}LL; part++) {{\n        \
                 at = in[lane * {parts}LL + part];\n        \
                 {ty} value = {found};\n        \
                 bool none = value != value, held = best != best;\n        \
                 if (none ? (!held || at < place) : (!held && (value {wins} best || (value == best && at < place)))) {{ best = value; place = at; }}\n    \
                 }}\n    \
                 out[lane] = place;\n}}"
            )
            .expect("writing to a string cannot fail");
            let partial = self.scratch();
            self.actions.push(Action::Kernel {
                name: format!("k{slot}r0"),
                output: partial,
                bytes: threads * 8,
                threads,
                inputs: leaves.clone(),
                clear: false,
                more: Vec::new(),
            });
            let mut inputs = leaves;
            inputs.push(partial);
            self.actions.push(Action::Kernel {
                name: format!("k{slot}"),
                output: slot,
                bytes: lanes * 8,
                threads: lanes,
                inputs,
                clear: false,
                more: Vec::new(),
            });
            return;
        }
        writeln!(
            self.source,
            "extern \"C\" __global__ void k{slot}(long long* out{parameters}) {{\n    \
             long long lane = (long long)blockIdx.x * blockDim.x + threadIdx.x;\n    \
             if (lane >= {lanes}LL) return;\n    \
             const long long step = {inner}LL;\n    \
             long long start = (lane / step) * ({length}LL * step) + lane % step;\n    \
             {ty} best = {first};\n    \
             long long place = 0;\n    \
             for (long long k = 1; k < {length}LL; k++) {{\n        \
             {ty} value = {next};\n        \
             if (value {wins} best || (value != value && best == best)) {{ best = value; place = k; }}\n    \
             }}\n    \
             out[lane] = place;\n}}"
        )
        .expect("writing to a string cannot fail");
        self.actions.push(Action::Kernel {
            name: format!("k{slot}"),
            output: slot,
            bytes: lanes * 8,
            threads: lanes,
            inputs: leaves,
            clear: false,
            more: Vec::new(),
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
                if let Some(parts) = lane_parts(inner, length) {
                    // Thread `part` of a lane reduces the places `part`,
                    // `part + parts` and so on, then one thread for each
                    // lane reduces what its threads wrote.
                    // Thread `t` is place `t % inner` after the axis,
                    // part `(t / inner) % parts`, so threads beside each
                    // other read memory beside each other.
                    let threads = lanes * parts;
                    let step = combine("acc", &at("(start + k * step)"));
                    let join = combine("acc", "in[base + part * step]");
                    writeln!(
                        self.source,
                        "extern \"C\" __global__ void k{slot}r0({ty}* out{parameters}) {{\n    \
                         long long t = (long long)blockIdx.x * blockDim.x + threadIdx.x;\n    \
                         if (t >= {threads}LL) return;\n    \
                         const long long step = {inner}LL;\n    \
                         long long part = (t / step) % {parts}LL;\n    \
                         long long start = (t / (step * {parts}LL)) * ({length}LL * step) + t % step;\n    \
                         {ty} acc = {first};\n    \
                         for (long long k = part + {parts}LL; k < {length}LL; k += {parts}LL) acc = {step_code};\n    \
                         out[t] = acc;\n}}\n\
                         extern \"C\" __global__ void k{slot}({ty}* out, const {ty}* in) {{\n    \
                         long long lane = (long long)blockIdx.x * blockDim.x + threadIdx.x;\n    \
                         if (lane >= {lanes}LL) return;\n    \
                         const long long step = {inner}LL;\n    \
                         long long base = (lane / step) * ({parts}LL * step) + lane % step;\n    \
                         {ty} acc = in[base];\n    \
                         for (long long part = 1; part < {parts}LL; part++) acc = {join};\n    \
                         out[lane] = {finish};\n}}",
                        first = at("(start + part * step)"),
                        step_code = step,
                    )
                    .expect("writing to a string cannot fail");
                    let partial = self.scratch();
                    self.actions.push(Action::Kernel {
                        name: format!("k{slot}r0"),
                        output: partial,
                        bytes: threads * bytes,
                        threads,
                        inputs: leaves,
                        clear: false,
                        more: Vec::new(),
                    });
                    self.actions.push(Action::Kernel {
                        name: format!("k{slot}"),
                        output: slot,
                        bytes: lanes * bytes,
                        threads: lanes,
                        inputs: vec![partial],
                        clear: false,
                        more: Vec::new(),
                    });
                    return;
                }
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
                    clear: false,
                    more: Vec::new(),
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
                        clear: false,
                        more: Vec::new(),
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
        let stored = |side: usize| Self::transpose_of(self.plan, &self.materialized, side);
        let swapped = (stored(left).is_some(), stored(right).is_some());
        let (left, right) = (stored(left).unwrap_or(left), stored(right).unwrap_or(right));
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
            swapped,
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
                Operation::IndexAdd { axis, size } => {
                    self.index_add(slot, *axis, *size, inputs[0], inputs[1]);
                }
                Operation::ArgReduce { largest, axis } => {
                    self.arg_reduce(slot, *largest, *axis, inputs[0]);
                }
                Operation::Reshape(_) if self.materialized[inputs[0]] => {
                    self.actions.push(Action::Alias {
                        output: slot,
                        input: inputs[0],
                    });
                }
                _ => self.map(slot),
            }
        }
        self.launch_maps();
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
    if code == SIGMOID_OPERATION {
        return if dtype == ArrayDType::Float64 {
            format!("(1.0 / (1.0 + exp(-({operand}))))")
        } else {
            format!("(1.0f / (1.0f + expf(-({operand}))))")
        };
    }
    let function = match code {
        TANH_OPERATION => "tanh",
        ERF_OPERATION => "erf",
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

/// The buffers each action of `program` is the last to read, leaving out
/// the plan's outputs.
pub(crate) fn finished_buffers(program: &Program, plan: &Plan) -> Vec<Vec<usize>> {
    let mut last: std::collections::HashMap<usize, usize> = std::collections::HashMap::new();
    for (index, action) in program.actions.iter().enumerate() {
        let read: &[usize] = match action {
            Action::Kernel { inputs, .. } => inputs,
            Action::Matmul { left, right, .. } => &[*left, *right],
            Action::Alias { input, .. } => std::slice::from_ref(input),
        };
        for &buffer in read {
            last.insert(buffer, index);
        }
    }
    let mut finished = vec![Vec::new(); program.actions.len()];
    for (buffer, index) in last {
        if !plan.outputs.contains(&buffer) {
            finished[index].push(buffer);
        }
    }
    finished
}
