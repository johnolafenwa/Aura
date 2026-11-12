//! Gradients of tensors.
//!
//! A float tensor computed from a watched tensor keeps the operation that
//! made it and its operands. [`gradient`] walks those records from a
//! result back to the watched tensors and builds each gradient from the
//! ordinary tensor operations, so on a GPU a gradient is a waiting
//! expression like any other and runs as part of a plan.

use super::*;
use std::collections::HashSet;

fn run(operation: Operation, inputs: &[&TensorValue]) -> Result<TensorValue> {
    match operate(operation, inputs)? {
        Value::Tensor(node) => Ok(node),
        _ => unreachable!("an operation gives a tensor"),
    }
}

fn binary(code: i64, left: &TensorValue, right: &TensorValue) -> Result<TensorValue> {
    run(Operation::Binary(code), &[left, right])
}

fn add(left: &TensorValue, right: &TensorValue) -> Result<TensorValue> {
    binary(0, left, right)
}

fn mul(left: &TensorValue, right: &TensorValue) -> Result<TensorValue> {
    binary(2, left, right)
}

fn div(left: &TensorValue, right: &TensorValue) -> Result<TensorValue> {
    binary(3, left, right)
}

fn unary(code: i64, operand: &TensorValue) -> Result<TensorValue> {
    run(Operation::Unary(code), &[operand])
}

fn negate(operand: &TensorValue) -> Result<TensorValue> {
    unary(NEGATE_OPERATION, operand)
}

/// One number as a tensor of the dtype of `like`, on its device.
fn number(like: &TensorValue, value: f64) -> Result<TensorValue> {
    let storage = match like.dtype() {
        ArrayDType::Float32 => ArrayStorage::Float32(vec![value as f32].into()),
        _ => ArrayStorage::Float64(vec![value].into()),
    };
    match place(like.device(), ArrayValue::new(Box::new([1]), storage)?)? {
        Value::Tensor(node) => Ok(node),
        _ => unreachable!("placing elements gives a tensor"),
    }
}

fn scaled(operand: &TensorValue, factor: f64) -> Result<TensorValue> {
    mul(operand, &number(operand, factor)?)
}

fn choose(
    mask: &TensorValue,
    when_true: &TensorValue,
    when_false: &TensorValue,
) -> Result<TensorValue> {
    run(Operation::Where, &[mask, when_true, when_false])
}

fn reshape(operand: &TensorValue, shape: &[usize]) -> Result<TensorValue> {
    if operand.shape() == shape {
        return Ok(operand.clone());
    }
    run(
        Operation::Reshape(shape.iter().map(|&dimension| dimension as i64).collect()),
        &[operand],
    )
}

fn expand(operand: &TensorValue, shape: &[usize]) -> Result<TensorValue> {
    if operand.shape() == shape {
        return Ok(operand.clone());
    }
    run(Operation::Expand(shape.to_vec()), &[operand])
}

fn sum_axis(operand: &TensorValue, axis: usize) -> Result<TensorValue> {
    run(
        Operation::Reduce {
            reduction: 0,
            axis: Some(axis as i64),
        },
        &[operand],
    )
}

/// The gradient `gradient` of a broadcast result, summed back to the
/// `shape` of the operand that was broadcast.
fn sum_to(gradient: &TensorValue, shape: &[usize]) -> Result<TensorValue> {
    let mut gradient = gradient.clone();
    while gradient.shape().len() > shape.len() {
        gradient = sum_axis(&gradient, 0)?;
    }
    for axis in 0..shape.len() {
        if shape[axis] == 1 && gradient.shape()[axis] != 1 {
            let mut kept = gradient.shape().to_vec();
            kept[axis] = 1;
            gradient = reshape(&sum_axis(&gradient, axis)?, &kept)?;
        }
    }
    Ok(gradient)
}

/// The last two axes of `operand` exchanged.
fn swap_matrix_axes(operand: &TensorValue) -> Result<TensorValue> {
    let rank = operand.shape().len() as i64;
    let mut axes: Vec<i64> = (0..rank).collect();
    axes.swap(rank as usize - 1, rank as usize - 2);
    run(Operation::Permute(axes), &[operand])
}

/// The gradients that the gradient `flow` of the tensor `result` gives
/// its operands, by operand position. `wants` says which operands are
/// tracked. The others get none.
fn pull(
    result: &TensorValue,
    history: &History,
    flow: &TensorValue,
    wants: &dyn Fn(usize) -> bool,
) -> Result<Vec<(usize, TensorValue)>> {
    let inputs = &history.inputs;
    let first = &inputs[0];
    let mut pulled = Vec::new();
    let mut give = |position: usize, make: &dyn Fn() -> Result<TensorValue>| -> Result<()> {
        if wants(position) {
            pulled.push((position, make()?));
        }
        Ok(())
    };
    match &history.operation {
        Operation::Binary(code) => {
            let second = &inputs[1];
            let (to_first, to_second): (TensorValue, Box<dyn Fn() -> Result<TensorValue>>) =
                match *code {
                    0 => (flow.clone(), Box::new(|| Ok(flow.clone()))),
                    1 => (flow.clone(), Box::new(|| negate(flow))),
                    2 => (
                        if wants(0) {
                            mul(flow, second)?
                        } else {
                            flow.clone()
                        },
                        Box::new(|| mul(flow, first)),
                    ),
                    3 => (
                        if wants(0) {
                            div(flow, second)?
                        } else {
                            flow.clone()
                        },
                        Box::new(|| negate(&div(&mul(flow, result)?, second)?)),
                    ),
                    16 | 17 => {
                        // The smaller operand, or the larger, takes the
                        // whole gradient. The first takes it on a tie.
                        let mask = run(
                            Operation::Compare(if *code == 16 { 1 } else { 3 }),
                            &[first, second],
                        )?;
                        let zero = number(flow, 0.0)?;
                        let to_first = choose(&mask, flow, &zero)?;
                        (
                            to_first,
                            Box::new(move || choose(&mask, &number(flow, 0.0)?, flow)),
                        )
                    }
                    18 => (
                        if wants(0) {
                            let lower = binary(18, first, &binary(1, second, &number(flow, 1.0)?)?)?;
                            mul(&mul(flow, second)?, &lower)?
                        } else {
                            flow.clone()
                        },
                        Box::new(|| mul(&mul(flow, result)?, &unary(3, first)?)),
                    ),
                    _ => return Ok(pulled),
                };
            give(0, &|| sum_to(&to_first, first.shape()))?;
            give(1, &|| sum_to(&to_second()?, second.shape()))?;
        }
        Operation::Unary(code) => {
            let made = match *code {
                NEGATE_OPERATION => negate(flow)?,
                0 => {
                    let below = run(Operation::Compare(0), &[first, &number(first, 0.0)?])?;
                    choose(&below, &negate(flow)?, flow)?
                }
                1 => div(&scaled(flow, 0.5)?, result)?,
                2 => mul(flow, result)?,
                3 => div(flow, first)?,
                6 => mul(flow, &unary(7, first)?)?,
                7 => negate(&mul(flow, &unary(6, first)?)?)?,
                TANH_OPERATION => {
                    let one = number(flow, 1.0)?;
                    mul(flow, &binary(1, &one, &mul(result, result)?)?)?
                }
                SIGMOID_OPERATION => {
                    let one = number(flow, 1.0)?;
                    mul(&mul(flow, result)?, &binary(1, &one, result)?)?
                }
                ERF_OPERATION => {
                    let bell = unary(2, &negate(&mul(first, first)?)?)?;
                    scaled(&mul(flow, &bell)?, 2.0 / std::f64::consts::PI.sqrt())?
                }
                _ => return Ok(pulled),
            };
            give(0, &|| Ok(made.clone()))?;
        }
        Operation::Where => {
            let zero = number(flow, 0.0)?;
            give(1, &|| {
                sum_to(&choose(first, flow, &zero)?, inputs[1].shape())
            })?;
            give(2, &|| {
                sum_to(&choose(first, &zero, flow)?, inputs[2].shape())
            })?;
        }
        Operation::Reduce { reduction, axis } => {
            let rank = first.shape().len();
            let (kept, lane): (Vec<usize>, usize) = match axis {
                None => (vec![1; rank], first.len()),
                Some(axis) => {
                    let axis = if *axis < 0 {
                        (*axis + rank as i64) as usize
                    } else {
                        *axis as usize
                    };
                    let mut kept = first.shape().to_vec();
                    kept[axis] = 1;
                    (kept, first.shape()[axis])
                }
            };
            let flow = reshape(flow, &kept)?;
            let made = match *reduction {
                0 => expand(&flow, first.shape())?,
                3 => expand(&scaled(&flow, 1.0 / lane as f64)?, first.shape())?,
                _ => {
                    // The elements equal to the lane's minimum or maximum
                    // share the gradient.
                    let mask = run(Operation::Compare(4), &[first, &reshape(result, &kept)?])?;
                    let (one, zero) = (number(&flow, 1.0)?, number(&flow, 0.0)?);
                    let mut count = choose(&mask, &one, &zero)?;
                    for axis in 0..rank {
                        if kept[axis] == 1 && first.shape()[axis] != 1 {
                            let mut shape = count.shape().to_vec();
                            shape[axis] = 1;
                            count = reshape(&sum_axis(&count, axis)?, &shape)?;
                        }
                    }
                    choose(&mask, &div(&flow, &count)?, &zero)?
                }
            };
            give(0, &|| Ok(made.clone()))?;
        }
        Operation::Matmul => {
            let second = &inputs[1];
            if first.shape().len() < 2 || second.shape().len() < 2 {
                return Err(tensor_error(
                    "tensor::gradient",
                    "cannot go through a `matmul` with a rank-one operand; reshape it to a matrix",
                ));
            }
            give(0, &|| {
                let made = run(Operation::Matmul, &[flow, &swap_matrix_axes(second)?])?;
                sum_to(&made, first.shape())
            })?;
            give(1, &|| {
                let made = run(Operation::Matmul, &[&swap_matrix_axes(first)?, flow])?;
                sum_to(&made, second.shape())
            })?;
        }
        Operation::Reshape(_) => give(0, &|| reshape(flow, first.shape()))?,
        Operation::Permute(axes) => {
            let rank = first.shape().len();
            let mut back = vec![0_i64; rank];
            for place in 0..rank {
                let from = if axes.is_empty() {
                    rank - 1 - place
                } else if axes[place] < 0 {
                    (axes[place] + rank as i64) as usize
                } else {
                    axes[place] as usize
                };
                back[from] = place as i64;
            }
            give(0, &|| run(Operation::Permute(back.clone()), &[flow]))?;
        }
        Operation::Cast(_) => give(0, &|| {
            if first.dtype() == flow.dtype() {
                Ok(flow.clone())
            } else {
                run(Operation::Cast(first.dtype() as i64), &[flow])
            }
        })?,
        Operation::Expand(_) => give(0, &|| sum_to(flow, first.shape()))?,
        Operation::Slice {
            axis, start, step, ..
        } => give(0, &|| {
            run(
                Operation::Unslice {
                    axis: *axis,
                    start: *start,
                    step: *step,
                    size: first.shape()[*axis],
                },
                &[flow],
            )
        })?,
        Operation::Unslice {
            axis, start, step, ..
        } => give(0, &|| {
            run(
                Operation::Slice {
                    axis: *axis,
                    start: *start,
                    step: *step,
                    length: first.shape()[*axis],
                },
                &[flow],
            )
        })?,
        Operation::Concat { axis } => {
            let part = |start: usize, length: usize| {
                run(
                    Operation::Slice {
                        axis: *axis,
                        start,
                        step: 1,
                        length,
                    },
                    &[flow],
                )
            };
            let split = first.shape()[*axis];
            give(0, &|| part(0, split))?;
            give(1, &|| part(split, inputs[1].shape()[*axis]))?;
        }
        Operation::Take { axis } => give(0, &|| {
            run(
                Operation::IndexAdd {
                    axis: *axis,
                    size: first.shape()[*axis],
                },
                &[flow, &inputs[1]],
            )
        })?,
        Operation::IndexAdd { axis, .. } => {
            give(0, &|| run(Operation::Take { axis: *axis }, &[flow, &inputs[1]]))?;
        }
        Operation::Compare(_) | Operation::ArgReduce { .. } | Operation::Random { .. } => {}
    }
    Ok(pulled)
}

/// The gradient of `root` with respect to every watched tensor it was
/// computed from.
fn backward(root: &TensorValue) -> Result<Vec<(TensorValue, TensorValue)>> {
    // Every tracked tensor behind `root`, each after its operands.
    let mut order: Vec<TensorValue> = Vec::new();
    let mut seen: HashSet<*const TensorNode> = HashSet::new();
    let mut stack = vec![(root.clone(), false)];
    while let Some((node, visited)) = stack.pop() {
        if visited {
            order.push(node);
            continue;
        }
        if !seen.insert(Arc::as_ptr(&node.0)) {
            continue;
        }
        stack.push((node.clone(), true));
        if let Some(history) = &node.0.history {
            for input in &history.inputs {
                if input.is_tracked() {
                    stack.push((input.clone(), false));
                }
            }
        }
    }
    let mut flows: HashMap<*const TensorNode, TensorValue> = HashMap::new();
    flows.insert(
        Arc::as_ptr(&root.0),
        expand(&number(root, 1.0)?, root.shape())?,
    );
    let mut watched = Vec::new();
    for node in order.iter().rev() {
        let Some(flow) = flows.remove(&Arc::as_ptr(&node.0)) else {
            continue;
        };
        let Some(history) = &node.0.history else {
            watched.push((node.clone(), flow));
            continue;
        };
        let wants = |position: usize| history.inputs[position].is_tracked();
        for (position, made) in pull(node, history, &flow, &wants)? {
            let key = Arc::as_ptr(&history.inputs[position].0);
            let total = match flows.remove(&key) {
                Some(before) => add(&before, &made)?,
                None => made,
            };
            flows.insert(key, total);
        }
    }
    Ok(watched)
}

/// The gradient of `result` with respect to the watched tensor `wrt`: a
/// tensor of the shape of `wrt`. The first call computes the gradients
/// for every watched tensor behind `result`, and later calls take them.
pub(super) fn gradient(result: &TensorValue, wrt: &TensorValue) -> Result<TensorValue> {
    let name = "tensor::gradient";
    if !wrt.is_tracked() || wrt.0.history.is_some() {
        return Err(tensor_error(
            name,
            "expects a watched tensor; make one with `watch()`",
        ));
    }
    if !matches!(result.dtype(), ArrayDType::Float32 | ArrayDType::Float64) {
        return Err(tensor_error(name, "expects a float tensor"));
    }
    let mut kept = result
        .0
        .gradients
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if kept.is_none() {
        let before = RECORDING.with(|recording| recording.replace(false));
        let made = backward(result);
        RECORDING.with(|recording| recording.set(before));
        *kept = Some(made?);
    }
    let found = kept
        .as_ref()
        .expect("the gradients were just computed")
        .iter()
        .find(|(watched, _)| watched == wrt)
        .map(|(_, gradient)| gradient.clone());
    match found {
        Some(gradient) => Ok(gradient),
        // The result does not depend on the tensor.
        None => {
            let before = RECORDING.with(|recording| recording.replace(false));
            let zeros = number(wrt, 0.0).and_then(|zero| expand(&zero, wrt.shape()));
            RECORDING.with(|recording| recording.set(before));
            zeros
        }
    }
}
