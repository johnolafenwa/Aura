//! Stable sorting of `list` elements by the order of a built-in type.
//!
//! `list.sort()` and `list.sort(key=...)` sort here when the elements or
//! keys are integers, floats, `Duration`s, or `str`s, which order without
//! user code. Both backends call these functions, so they sort the same
//! way. The sorts are stable merge sorts (Rust's `sort_by`), so equal
//! elements or keys keep their input order in both directions, and
//! `reverse` sorts by the mirrored order instead of reversing the result.
//!
//! Floats sort by `<`, except that NaN, which `<` cannot place, sorts after
//! every other value and equals every other NaN. `-0.0` and `0.0` are equal,
//! as `==` says, so they keep their input order. `str` sorts by its UTF-8
//! bytes, which is the order of its Unicode scalar values.
//!
//! A sort whose elements order through a user `Ord` implementation runs in
//! generated code instead, which computes the sorted positions and hands
//! them to [`permute_elements`].

use std::cmp::Ordering;

use crate::diag::{Diagnostic, Result};
use crate::list_elements::ListElements;
use crate::native_runtime::ElementBox;
use crate::runtime_value::Value;

/// The order a sort uses for two floats: `<`, with NaN after every other
/// value and equal to every other NaN. It is total, so a sort can rely on it.
pub fn float_sort_order(left: f64, right: f64) -> Ordering {
    left.partial_cmp(&right)
        .unwrap_or_else(|| left.is_nan().cmp(&right.is_nan()))
}

/// The built-in order of two values of one orderable type, or `None` when
/// the pair has none.
fn value_sort_order(left: &Value, right: &Value) -> Option<Ordering> {
    Some(match (left, right) {
        (Value::Int(left), Value::Int(right)) => left.cmp(right),
        (Value::Float(left), Value::Float(right)) => float_sort_order(*left, *right),
        (Value::String(left), Value::String(right)) => left.as_bytes().cmp(right.as_bytes()),
        (Value::Duration(left), Value::Duration(right)) => left.cmp(right),
        _ => return None,
    })
}

/// Checks that every value orders against the first, so the comparator a
/// sort runs never meets a pair without an order.
fn check_orderable<'a>(mut values: impl Iterator<Item = &'a Value>) -> Result<()> {
    let Some(first) = values.next() else {
        return Ok(());
    };
    if value_sort_order(first, first).is_some()
        && values.all(|value| value_sort_order(first, value).is_some())
    {
        return Ok(());
    }
    Err(unorderable())
}

fn itself(value: &Value) -> &Value {
    value
}

/// The error for values without one built-in order. Class instances kept
/// in place in a list have none, so a list of them gets this error too.
fn unorderable() -> Diagnostic {
    Diagnostic::new("list sort found values without one built-in order")
}

fn directed(ordering: Ordering, reverse: bool) -> Ordering {
    if reverse {
        ordering.reverse()
    } else {
        ordering
    }
}

/// Sorts `items` stably by the built-in order of the value each holds.
fn sort_values<T>(items: &mut [T], value: impl Fn(&T) -> &Value, reverse: bool) -> Result<()> {
    check_orderable(items.iter().map(&value))?;
    items.sort_by(|left, right| {
        directed(
            value_sort_order(value(left), value(right)).unwrap_or(Ordering::Equal),
            reverse,
        )
    });
    Ok(())
}

/// The positions of `keys` in the stable order of the built-in order of the
/// value each holds.
fn value_positions<T>(
    keys: &[T],
    value: impl Fn(&T) -> &Value,
    reverse: bool,
) -> Result<Vec<usize>> {
    check_orderable(keys.iter().map(&value))?;
    let mut order = (0..keys.len()).collect::<Vec<_>>();
    order.sort_by(|left, right| {
        directed(
            value_sort_order(value(&keys[*left]), value(&keys[*right])).unwrap_or(Ordering::Equal),
            reverse,
        )
    });
    Ok(order)
}

/// Sorts one unboxed integer form. Equal integers cannot be told apart, so
/// an unstable sort gives the stable result.
fn sort_integers<T: Ord>(values: &mut [T], reverse: bool) {
    if reverse {
        values.sort_unstable_by(|left, right| right.cmp(left));
    } else {
        values.sort_unstable();
    }
}

/// The stable order of one unboxed integer form of keys, as
/// [`sorted_positions`] gives it. A tie on the key falls back to the
/// position, which is what a stable sort does, so the faster unstable sort
/// can run.
fn integer_key_positions<T: Ord + Copy>(keys: &[T], reverse: bool) -> Vec<usize> {
    let mut pairs = keys.iter().copied().zip(0..keys.len()).collect::<Vec<_>>();
    pairs.sort_unstable_by(|(left, left_at), (right, right_at)| {
        directed(left.cmp(right), reverse).then(left_at.cmp(right_at))
    });
    pairs.into_iter().map(|(_, position)| position).collect()
}

/// The positions of float `keys` in the stable order of their built-in
/// order. A tie on the key falls back to the position, as for integers.
fn float_positions(keys: impl Iterator<Item = f64>, reverse: bool) -> Vec<usize> {
    let mut pairs = keys.zip(0..).collect::<Vec<(f64, usize)>>();
    pairs.sort_unstable_by(|(left, left_at), (right, right_at)| {
        directed(float_sort_order(*left, *right), reverse).then(left_at.cmp(right_at))
    });
    pairs.into_iter().map(|(_, position)| position).collect()
}

/// Sorts `elements` stably by the built-in order of the elements.
pub fn sort_elements(elements: &mut ListElements, reverse: bool) -> Result<()> {
    if elements.len() < 2 {
        return Ok(());
    }
    match elements {
        ListElements::Int64(values) => sort_integers(values, reverse),
        ListElements::Int8(values) => sort_integers(values, reverse),
        ListElements::Int16(values) => sort_integers(values, reverse),
        ListElements::Int32(values) => sort_integers(values, reverse),
        ListElements::Uint8(values) => sort_integers(values, reverse),
        ListElements::Uint16(values) => sort_integers(values, reverse),
        ListElements::Uint32(values) => sort_integers(values, reverse),
        ListElements::Uint64(values) => sort_integers(values, reverse),
        ListElements::Float64(values) => {
            values.sort_by(|left, right| directed(float_sort_order(*left, *right), reverse))
        }
        ListElements::Float32(values) => values.sort_by(|left, right| {
            directed(
                float_sort_order(f64::from(*left), f64::from(*right)),
                reverse,
            )
        }),
        ListElements::Values(values) => sort_values(values, itself, reverse)?,
        ListElements::Boxes(boxes) => sort_values(boxes, ElementBox::value, reverse)?,
        ListElements::Bool(_) => {
            return Err(Diagnostic::new("list sort cannot order `bool` values"));
        }
        ListElements::Typed(_) => return Err(unorderable()),
    }
    Ok(())
}

/// The positions of `keys` in the stable order of their built-in order:
/// entry `k` is the position of the key that sorts `k`th.
fn sorted_positions(keys: &ListElements, reverse: bool) -> Result<Vec<usize>> {
    Ok(match keys {
        ListElements::Int64(keys) => integer_key_positions(keys, reverse),
        ListElements::Int8(keys) => integer_key_positions(keys, reverse),
        ListElements::Int16(keys) => integer_key_positions(keys, reverse),
        ListElements::Int32(keys) => integer_key_positions(keys, reverse),
        ListElements::Uint8(keys) => integer_key_positions(keys, reverse),
        ListElements::Uint16(keys) => integer_key_positions(keys, reverse),
        ListElements::Uint32(keys) => integer_key_positions(keys, reverse),
        ListElements::Uint64(keys) => integer_key_positions(keys, reverse),
        ListElements::Float64(keys) => float_positions(keys.iter().copied(), reverse),
        ListElements::Float32(keys) => {
            float_positions(keys.iter().map(|key| f64::from(*key)), reverse)
        }
        ListElements::Values(keys) => value_positions(keys, itself, reverse)?,
        ListElements::Boxes(keys) => value_positions(keys, ElementBox::value, reverse)?,
        ListElements::Bool(_) => {
            return Err(Diagnostic::new("list sort cannot order `bool` keys"));
        }
        ListElements::Typed(_) => return Err(unorderable()),
    })
}

/// Sorts `elements` stably by `keys`, where `keys[i]` is the key of
/// `elements[i]`.
pub fn sort_elements_by_keys(
    elements: &mut ListElements,
    keys: &ListElements,
    reverse: bool,
) -> Result<()> {
    if keys.len() != elements.len() {
        return Err(Diagnostic::new(format!(
            "list sort has {} keys for {} elements",
            keys.len(),
            elements.len()
        )));
    }
    if elements.len() < 2 {
        return Ok(());
    }
    let order = sorted_positions(keys, reverse)?;
    apply_positions(elements, order);
    Ok(())
}

/// Moves the element at `order[k]` to position `k`, for every `k`.
/// `order` must hold each position of `elements` once.
fn apply_positions(elements: &mut ListElements, mut order: Vec<usize>) {
    // Follow each cycle of the permutation once, swapping each element
    // into place, and mark a placed position by pointing it at itself.
    for start in 0..order.len() {
        let mut current = start;
        while order[current] != start {
            let next = order[current];
            elements.swap(current, next);
            order[current] = current;
            current = next;
        }
        order[current] = current;
    }
}

/// Reorders `elements` so that position `k` holds the element that was at
/// `order[k]`. Generated code passes the positions a sort through a user
/// `Ord` implementation computed.
pub fn permute_elements(elements: &mut ListElements, order: &ListElements) -> Result<()> {
    let len = elements.len();
    let mut positions = Vec::with_capacity(order.len());
    let mut seen = vec![false; len];
    for entry in order.iter() {
        let position = match entry.as_ref() {
            Value::Int(position) => position
                .as_i128()
                .and_then(|position| usize::try_from(position).ok())
                .filter(|position| *position < len),
            _ => None,
        };
        let Some(position) = position.filter(|position| !seen[*position]) else {
            return Err(Diagnostic::new(
                "list sort positions are not a permutation of the list",
            ));
        };
        seen[position] = true;
        positions.push(position);
    }
    if positions.len() != len {
        return Err(Diagnostic::new(
            "list sort positions are not a permutation of the list",
        ));
    }
    apply_positions(elements, positions);
    Ok(())
}

#[cfg(test)]
#[path = "list_sort_tests.rs"]
mod tests;
