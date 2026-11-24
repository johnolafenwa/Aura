//! Models of `control.recover`, of the values tasks share through the
//! `sync` module and queues, and of the shapes of `tensor.Tensor` values.
//!
//! `control.recover(f, args...)` runs `f` to its end in the calling task.
//! Its result is `Ok` with what `f` returned, which then has `f`'s
//! `ensures`, or `Err` with a trap the program does not choose. The call
//! cannot change the caller's own values: its arguments move or are
//! copied in, and no parameter is `mut`. What it may change is state other
//! tasks share, which the encoding never keeps facts about.

use super::exec::{and, array_parts, array_shape, conjoin, Exec, Goal, IntTy, State, Store, Val};
use super::ObligationKind;
use crate::mir::{CallTarget, MethodDispatch, MirArg};
use crate::runtime_ops::{FunctionOp, QueueOp, RuntimeOp};
use crate::sema::Type;

impl<'a> Exec<'a> {
    /// The `Result` that `control.recover` gives, with `call` running the
    /// callee on its `Ok` path only. `None` when the result type is not a
    /// `Result`.
    pub(super) fn recovered(
        &mut self,
        state: &mut State,
        result_type: Option<&Type>,
        call: impl FnOnce(&mut Self, &mut State) -> Val,
    ) -> Option<Val> {
        let fresh = result_type.map(|ty| self.fresh(ty, "recovered"));
        let (Some(Val::Enum(name, tag, mut payloads)), Some(Type::Named(_, args))) =
            (fresh, result_type)
        else {
            call(self, state);
            return None;
        };
        let ok = self.cx.variant_index(&name, "Ok")?;
        let err = self.cx.variant_index(&name, "Err")?;
        // Which trap ended the call, and where, is not modeled.
        let trap = args.get(1)?;
        let what = "the trap that `control.recover` returns";
        payloads[err] = vec![self.unmodeled(what, |exec| exec.fresh(trap, "trap"))];
        // The callee's `ensures` holds only when it returned: a trap may
        // end it before any of them is true.
        let outer = state.pc.clone();
        state.pc = and(&outer, &format!("(= {tag} {ok})"));
        let value = call(self, state);
        state.pc = outer;
        payloads[ok] = vec![value];
        Some(Val::Enum(name, tag, payloads))
    }

    /// The result of a step that reads what other tasks share: a receive
    /// from a queue, the value a lock block starts with, and whether the
    /// task was cancelled. Another task may have changed it at any time,
    /// so it is any value of its type, which the program does not know
    /// either. `None` for every other call.
    pub(super) fn shared_value(&mut self, callee: &CallTarget, ty: Option<&Type>) -> Option<Val> {
        let shared = match callee {
            CallTarget::Runtime {
                op: RuntimeOp::Queue(op),
                ..
            } => !matches!(
                op,
                QueueOp::Put | QueueOp::PutTimeout | QueueOp::TryPut | QueueOp::Close
            ),
            CallTarget::Runtime {
                op: RuntimeOp::Function(FunctionOp::Cancelled),
                ..
            } => true,
            CallTarget::Runtime {
                op: RuntimeOp::Host(host),
                ..
            } => host.name() == "sync::__aura_cell_take",
            _ => false,
        };
        if !shared {
            return None;
        }
        Some(self.fresh(ty?, "shared"))
    }
}

// ----- Tensor shapes -----------------------------------------------------

/// The class of a tensor value, and the type of its one field. A modeled
/// tensor holds, in place of the node, a record of its element count and
/// its shape, as an `Array` value does. Its elements are not modeled.
const TENSOR: &str = "tensor.Tensor";
const NODE: &str = "tensor.Node";

/// Facts about the axes of a shape are written out for this many axes.
/// A rank above it is allowed: its other axes are left unknown.
const UNROLLED: usize = 4;

/// The longest literal shape a `reshape` is modeled for.
const RESHAPE_AXES: usize = 8;

/// A tensor's element count, its rank, and its dimensions as an
/// `(Array Int Int)`.
#[derive(Clone)]
struct Shape {
    len: String,
    rank: String,
    dims: String,
}

impl Shape {
    fn dim(&self, axis: impl std::fmt::Display) -> String {
        format!("(select {} {axis})", self.dims)
    }

    /// The dimension `back` axes before the last one, or 1 past the
    /// first axis, as shapes line up to broadcast.
    fn back_dim(&self, back: impl std::fmt::Display) -> String {
        let rank = &self.rank;
        format!(
            "(ite (< {back} {rank}) {} 1)",
            self.dim(format!("(- {rank} 1 {back})"))
        )
    }

    fn node(self) -> Val {
        Val::Record(
            NODE.to_string(),
            vec![
                ("len".to_string(), Val::Int(self.len, IntTy::INT64)),
                ("shape".to_string(), array_shape(self.rank, self.dims)),
            ],
        )
    }

    fn tensor(self) -> Val {
        Val::Record(TENSOR.to_string(), vec![("node".to_string(), self.node())])
    }

    /// What holds of every tensor's shape: at least one axis, no negative
    /// dimension, and as many elements as the dimensions multiply to.
    fn facts(&self) -> String {
        let (len, rank) = (&self.len, &self.rank);
        let mut parts = vec![
            format!("(<= 1 {rank})"),
            format!("(<= 0 {len} {})", i64::MAX),
        ];
        for axis in 0..UNROLLED {
            parts.push(format!("(=> (< {axis} {rank}) (<= 0 {}))", self.dim(axis)));
            let dims: Vec<String> = (0..=axis).map(|axis| self.dim(axis)).collect();
            parts.push(format!(
                "(=> (= {rank} {}) (= {len} {}))",
                axis + 1,
                product(&dims)
            ));
        }
        conjoin(&parts)
    }
}

fn product(items: &[String]) -> String {
    match items {
        [] => "1".to_string(),
        [one] => one.clone(),
        _ => format!("(* {})", items.join(" ")),
    }
}

fn node_shape(node: &Val) -> Option<Shape> {
    let Val::Record(name, fields) = node else {
        return None;
    };
    let [(_, Val::Int(len, _)), (_, shape)] = fields.as_slice() else {
        return None;
    };
    let (
        NODE,
        Val::List {
            len: rank,
            data: Some(Store::Leaf(dims, _)),
            ..
        },
    ) = (name.as_str(), shape)
    else {
        return None;
    };
    Some(Shape {
        len: len.clone(),
        rank: rank.clone(),
        dims: dims.clone(),
    })
}

fn tensor_shape(value: &Val) -> Option<Shape> {
    match value {
        Val::Record(class, fields) if class == TENSOR => node_shape(&fields.first()?.1),
        _ => None,
    }
}

/// The element count, the rank, and the dimensions of a modeled tensor,
/// as `array_parts` gives them for an Array.
pub(super) fn tensor_parts(value: &Val) -> Option<(String, String, String)> {
    tensor_shape(value).map(|shape| (shape.len, shape.rank, shape.dims))
}

/// How a tensor function is called.
#[derive(Clone, Copy)]
enum TensorCall {
    /// A method of a tensor, or an operator with a tensor on its left.
    Method,
    /// An operator with a number on its left and a tensor on its right.
    Reversed,
    /// A function that makes a tensor from a shape or an Array.
    Made,
}

/// The name of a member without its type arguments.
fn bare(member: &str) -> &str {
    member.split('[').next().unwrap_or(member)
}

/// The member after the last `.` outside brackets, without type
/// arguments: `add` in `tensor.Tensor[T].add[R, T]`.
fn last_member(path: &str) -> &str {
    let mut depth = 0usize;
    let mut start = 0;
    for (at, ch) in path.char_indices() {
        match ch {
            '[' => depth += 1,
            ']' => depth = depth.saturating_sub(1),
            '.' if depth == 0 => start = at + 1,
            _ => {}
        }
    }
    bare(&path[start..])
}

/// How the tensor function `function` is called, and its member name.
fn tensor_call(function: &str) -> Option<(TensorCall, &str)> {
    if let Some(member) = function.strip_prefix("tensor::Tensor.") {
        return Some((TensorCall::Method, bare(member)));
    }
    if matches!(function, "tensor::rand" | "tensor::randn") {
        return Some((TensorCall::Made, "rand"));
    }
    let (implemented, on) = function.split_once(" for ")?;
    let member = last_member(on);
    if on.starts_with("tensor.Tensor[") {
        return Some(match member {
            "zeros" | "full" | "from_array" => (TensorCall::Made, member),
            _ => (TensorCall::Method, member),
        });
    }
    let operator = ["Add[", "Sub[", "Mul[", "Div["]
        .iter()
        .any(|name| implemented.starts_with(name));
    (operator && implemented.contains("tensor.Tensor")).then_some((TensorCall::Reversed, member))
}

/// Members whose result has the shape of their tensor.
const SAME_SHAPE: &[&str] = &[
    "relu",
    "abs",
    "sqrt",
    "exp",
    "log",
    "sin",
    "cos",
    "tanh",
    "sigmoid",
    "erf",
    "gelu",
    "neg",
    "clone",
    "to",
    "detach",
    "requires_grad",
    "powf",
    "clamp",
];

/// Members that combine two tensors element by element, broadcasting
/// their shapes, or a tensor and a number.
const ELEMENTWISE: &[&str] = &[
    "add",
    "sub",
    "mul",
    "div",
    "minimum",
    "maximum",
    "pow",
    "less",
    "less_equal",
    "greater",
    "greater_equal",
    "equal",
    "not_equal",
];

impl<'a> Exec<'a> {
    /// A call the models in this file give a value: a step that reads
    /// shared state, or a tensor member. `None` leaves the call to the
    /// executor.
    pub(super) fn modeled_call(
        &mut self,
        state: &mut State,
        callee: &CallTarget,
        args: &[MirArg],
        ty: Option<&Type>,
    ) -> Option<Val> {
        if let Some(value) = self.shared_value(callee, ty) {
            return Some(value);
        }
        let (function, object) = match callee {
            CallTarget::Name(name) => (name, None),
            CallTarget::Member {
                object,
                dispatch: Some(MethodDispatch::Static { function }),
                ..
            }
            | CallTarget::TraitMember {
                object,
                dispatch: Some(MethodDispatch::Static { function }),
                ..
            } => (function, Some(object)),
            _ => return None,
        };
        let (call, member) = tensor_call(function)?;
        let receiver = object.map(|object| self.operand(state, object, None));
        let values: Vec<Val> = args
            .iter()
            .map(|arg| self.operand(state, &arg.value, None))
            .collect();
        let pc = state.pc.clone();
        let modeled = match call {
            TensorCall::Method => {
                let shape = receiver.as_ref().and_then(tensor_shape);
                self.tensor_member(&pc, member, shape, &values)
            }
            TensorCall::Reversed => values.first().and_then(tensor_shape).map(Shape::tensor),
            TensorCall::Made => self.made_tensor(&pc, member, &values),
        };
        if modeled.is_some() {
            return modeled;
        }
        // Any other member that gives a tensor is a step the encoding
        // does not model; one that gives something else keeps the
        // executor's call.
        let ty = ty.filter(|ty| matches!(ty, Type::Named(name, _) if name == TENSOR))?;
        let what = format!("`{member}` of a tensor");
        // Its shape checks are not known either.
        let goal = Goal::Unsupported(format!("the shape checks of {what} are not modeled"));
        self.oblige_shape(&pc, "the shapes of a tensor member agree", goal);
        Some(self.unmodeled(what, |exec| exec.fresh(ty, "tensor")))
    }

    /// A tensor node with an unknown shape, for a fresh value of type
    /// `tensor.Node`. `None` for every other type.
    pub(super) fn fresh_tensor_node(&mut self, name: &str, hint: &str) -> Option<Val> {
        if name != NODE {
            return None;
        }
        let shape = self.fresh_shape(hint);
        self.fact(shape.facts());
        Some(shape.node())
    }

    fn fresh_shape(&mut self, hint: &str) -> Shape {
        Shape {
            len: self.fresh_const("Int", &format!("{hint}_len")),
            rank: self.fresh_const("Int", &format!("{hint}_rank")),
            dims: self.fresh_const("(Array Int Int)", &format!("{hint}_dims")),
        }
    }

    /// A tensor whose shape has the facts of every shape, and also
    /// `relation` on the paths `pc` describes where the member's check
    /// `valid` held. A member that traps gives no tensor, and a relation
    /// it cannot meet must not make the paths after it impossible.
    fn shaped(
        &mut self,
        pc: &str,
        valid: &str,
        relation: impl FnOnce(&Shape) -> Vec<String>,
    ) -> Val {
        let shape = self.fresh_shape("tensor");
        self.fact(shape.facts());
        let relation = conjoin(&relation(&shape));
        self.assume(pc, format!("(=> {valid} {relation})"));
        shape.tensor()
    }

    /// Proves `goal`, the shape check a tensor member makes before it
    /// runs: a mistake stops the program with `AU4007`.
    fn oblige_shape(&mut self, pc: &str, message: &str, goal: Goal) {
        let span = self.span;
        self.oblige(ObligationKind::Requires, span, message, pc, goal);
    }

    fn tensor_member(
        &mut self,
        pc: &str,
        member: &str,
        shape: Option<Shape>,
        values: &[Val],
    ) -> Option<Val> {
        let shape = shape?;
        match member {
            "shape" => Some(array_shape(shape.rank, shape.dims)),
            "len" => Some(Val::Int(shape.len, IntTy::INT64)),
            "item" => {
                let goal = Goal::Holds(format!("(= {} 1)", shape.len));
                self.oblige_shape(pc, "`item` of a tensor of one element", goal);
                None
            }
            "scalar" => Some(self.shaped(pc, "true", |made| {
                vec![
                    format!("(= {} 1)", made.rank),
                    format!("(= {} 1)", made.dim(0)),
                ]
            })),
            "flatten" => Some(self.shaped(pc, "true", |made| {
                vec![
                    format!("(= {} 1)", made.rank),
                    format!("(= {} {})", made.dim(0), shape.len),
                ]
            })),
            "transpose" => Some(self.shaped(pc, "true", |made| {
                let mut parts = vec![
                    format!("(= {} {})", made.rank, shape.rank),
                    format!("(= {} {})", made.len, shape.len),
                ];
                for axis in 0..UNROLLED {
                    let reversed = shape.dim(format!("(- {} 1 {axis})", shape.rank));
                    parts.push(format!(
                        "(=> (< {axis} {}) (= {} {reversed}))",
                        shape.rank,
                        made.dim(axis)
                    ));
                }
                parts
            })),
            "matmul" => {
                let other = tensor_shape(values.first()?)?;
                Some(self.matmul(pc, &shape, &other))
            }
            "reshape" => self.reshape(pc, &shape, values.first()?),
            "sum" | "min" | "max" | "mean" => self.reduce(pc, &shape, values),
            _ if SAME_SHAPE.contains(&member) => Some(shape.tensor()),
            _ if ELEMENTWISE.contains(&member) => match tensor_shape(values.first()?) {
                Some(other) => Some(self.broadcast(pc, member, &shape, &other)),
                // A number is one element of the tensor's dtype.
                None => Some(shape.tensor()),
            },
            _ => None,
        }
    }

    /// `a op b` for two tensors: their shapes, lined up at the last axis,
    /// agree where each pair of dimensions is equal or has a one.
    fn broadcast(&mut self, pc: &str, member: &str, a: &Shape, b: &Shape) -> Val {
        let rank = format!("(ite (>= {0} {1}) {0} {1})", a.rank, b.rank);
        let back = self.bound_name();
        let (x, y) = (a.back_dim(&back), b.back_dim(&back));
        let goal = format!(
            "(forall (({back} Int)) (=> (and (<= 0 {back}) (< {back} {rank})) (or (= {x} {y}) (= {x} 1) (= {y} 1))))"
        );
        let message = format!("the shapes of `{member}` broadcast");
        self.oblige_shape(pc, &message, Goal::Holds(goal.clone()));
        self.shaped(pc, &goal, |made| {
            let mut parts = vec![format!("(= {} {rank})", made.rank)];
            for back in 0..UNROLLED {
                let (x, y) = (a.back_dim(back), b.back_dim(back));
                let axis = made.dim(format!("(- {} 1 {back})", made.rank));
                parts.push(format!(
                    "(=> (< {back} {}) (= {axis} (ite (= {x} 1) {y} {x})))",
                    made.rank
                ));
            }
            parts
        })
    }

    /// `a.matmul(b)`: `[.., n, k]` by `[.., k, m]` is `[.., n, m]`, by `[k]`
    /// is `[.., n]`, and `[k]` by `[.., k, m]` is `[.., m]`. The leading
    /// batch dimensions broadcast.
    fn matmul(&mut self, pc: &str, a: &Shape, b: &Shape) -> Val {
        let (ra, rb) = (&a.rank, &b.rank);
        let a_matrix = format!("(<= 2 {ra})");
        let b_matrix = format!("(<= 2 {rb})");
        let inner_a = a.dim(format!("(- {ra} 1)"));
        let inner_b = format!(
            "(ite {b_matrix} {} {})",
            b.dim(format!("(- {rb} 2)")),
            b.dim(0)
        );
        // The batch axes are all but the last two of a matrix.
        let batch = |shape: &Shape, matrix: &str| Shape {
            len: shape.len.clone(),
            rank: format!("(ite {matrix} (- {} 2) 0)", shape.rank),
            dims: shape.dims.clone(),
        };
        let (batch_a, batch_b) = (batch(a, &a_matrix), batch(b, &b_matrix));
        let batch_rank = format!("(ite (>= {0} {1}) {0} {1})", batch_a.rank, batch_b.rank);
        let back = self.bound_name();
        let (x, y) = (batch_a.back_dim(&back), batch_b.back_dim(&back));
        let goal = format!(
            "(and (or {a_matrix} {b_matrix}) (= {inner_a} {inner_b}) (forall (({back} Int)) (=> (and (<= 0 {back}) (< {back} {batch_rank})) (or (= {x} {y}) (= {x} 1) (= {y} 1)))))"
        );
        self.oblige_shape(
            pc,
            "the shapes of `matmul` agree",
            Goal::Holds(goal.clone()),
        );
        let rows = a.dim(format!("(- {ra} 2)"));
        let columns = b.dim(format!("(- {rb} 1)"));
        self.shaped(pc, &goal, |made| {
            let mut parts = vec![
                format!(
                    "(= {} (+ {batch_rank} (ite {a_matrix} 1 0) (ite {b_matrix} 1 0)))",
                    made.rank
                ),
                // The matrix axes follow the batch axes.
                format!(
                    "(= {} (ite {a_matrix} {rows} {columns}))",
                    made.dim(&batch_rank)
                ),
                format!(
                    "(=> (and {a_matrix} {b_matrix}) (= {} {columns}))",
                    made.dim(format!("(+ {batch_rank} 1)"))
                ),
            ];
            for back in 0..UNROLLED {
                let (x, y) = (batch_a.back_dim(back), batch_b.back_dim(back));
                let axis = made.dim(format!("(- {batch_rank} 1 {back})"));
                parts.push(format!(
                    "(=> (< {back} {batch_rank}) (= {axis} (ite (= {x} 1) {y} {x})))"
                ));
            }
            parts
        })
    }

    /// `t.reshape(dims)`: the same elements under a literal shape, where
    /// one dimension may be `-1` for what the others leave.
    fn reshape(&mut self, pc: &str, shape: &Shape, dims: &Val) -> Option<Val> {
        let Val::List {
            len,
            data: Some(Store::Leaf(wanted, _)),
            ..
        } = dims
        else {
            return None;
        };
        let count = len
            .parse::<usize>()
            .ok()
            .filter(|count| *count <= RESHAPE_AXES)?;
        let item = |axis: usize| format!("(select {wanted} {axis})");
        let others = |skip: Option<usize>| -> Vec<String> {
            (0..count)
                .filter(|axis| Some(*axis) != skip)
                .map(item)
                .collect()
        };
        let all_known: Vec<String> = others(None)
            .iter()
            .map(|dim| format!("(<= 0 {dim})"))
            .collect();
        let mut cases = vec![format!(
            "(and {} (= {} {}))",
            conjoin(&all_known),
            shape.len,
            product(&others(None))
        )];
        let mut inferred = Vec::new();
        for axis in 0..count {
            let rest = product(&others(Some(axis)));
            let known: Vec<String> = others(Some(axis))
                .iter()
                .map(|dim| format!("(<= 0 {dim})"))
                .collect();
            cases.push(format!(
                "(and (= {} -1) {} (< 0 {rest}) (= (mod {} {rest}) 0))",
                item(axis),
                conjoin(&known),
                shape.len
            ));
            inferred.push(format!("(div {} {rest})", shape.len));
        }
        let goal = if count == 0 {
            "false".to_string()
        } else {
            format!("(or {})", cases.join(" "))
        };
        self.oblige_shape(
            pc,
            "`reshape` keeps the element count",
            Goal::Holds(goal.clone()),
        );
        Some(self.shaped(pc, &goal, |made| {
            // The element count follows from the dimensions, so a wrong
            // shape does not make the paths after it impossible.
            let mut parts = vec![format!("(= {} {count})", made.rank)];
            for (axis, inferred) in inferred.iter().enumerate() {
                let given = item(axis);
                parts.push(format!(
                    "(= {} (ite (= {given} -1) {inferred} {given}))",
                    made.dim(axis)
                ));
            }
            parts
        }))
    }

    /// `sum`, `min`, `max`, or `mean`, over every element or along one
    /// axis, with or without keeping that axis as a one.
    fn reduce(&mut self, pc: &str, shape: &Shape, values: &[Val]) -> Option<Val> {
        let [dim, Val::Bool(keep)] = values else {
            return None;
        };
        // `dim` is an `int64 | None`.
        let Val::Union(tag, members) = dim else {
            return None;
        };
        let (index, axis) =
            members
                .iter()
                .enumerate()
                .find_map(|(index, member)| match member {
                    Val::Int(axis, _) => Some((index, axis.clone())),
                    _ => None,
                })?;
        let along = format!("(= {tag} {index})");
        let rank = &shape.rank;
        let goal = format!("(=> {along} (and (<= (- {rank}) {axis}) (< {axis} {rank})))");
        self.oblige_shape(
            pc,
            "the axis of a reduction is inside the rank",
            Goal::Holds(goal.clone()),
        );
        let at = format!("(ite (< {axis} 0) (+ {axis} {rank}) {axis})");
        let keep = keep.clone();
        Some(self.shaped(pc, &goal, |made| {
            let removed = format!("(and {along} (not {keep}) (< 1 {rank}))");
            let mut parts = vec![format!(
                "(= {} (ite {keep} {rank} (ite {removed} (- {rank} 1) 1)))",
                made.rank
            )];
            for axis in 0..UNROLLED {
                let kept = format!(
                    "(ite (and {along} (not (= {axis} {at}))) {} 1)",
                    shape.dim(axis)
                );
                let dropped = format!(
                    "(ite (< {axis} {at}) {} {})",
                    shape.dim(axis),
                    shape.dim(axis + 1)
                );
                parts.push(format!(
                    "(=> (< {axis} {}) (= {} (ite {keep} {kept} (ite {removed} {dropped} 1))))",
                    made.rank,
                    made.dim(axis)
                ));
            }
            parts
        }))
    }

    /// `zeros`, `full`, `rand`, or `randn` of a shape list, and
    /// `from_array` of an Array.
    fn made_tensor(&mut self, pc: &str, member: &str, values: &[Val]) -> Option<Val> {
        let (shape, valid) = if member == "from_array" {
            let (len, rank, dims) = array_parts(values.first()?)?;
            (Shape { len, rank, dims }, "true".to_string())
        } else {
            let Val::List {
                len: rank,
                data: Some(Store::Leaf(dims, _)),
                ..
            } = values.first()?
            else {
                return None;
            };
            let at = self.bound_name();
            let goal = format!(
                "(and (<= 1 {rank}) (forall (({at} Int)) (=> (and (<= 0 {at}) (< {at} {rank})) (<= 0 (select {dims} {at})))))"
            );
            let message = "a tensor shape has an axis and no negative dimension";
            self.oblige_shape(pc, message, Goal::Holds(goal.clone()));
            let shape = Shape {
                len: self.fresh_const("Int", "tensor_len"),
                rank: rank.clone(),
                dims: dims.clone(),
            };
            (shape, goal)
        };
        let facts = shape.facts();
        self.assume(pc, format!("(=> {valid} {facts})"));
        Some(shape.tensor())
    }
}
