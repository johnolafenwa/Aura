//! Models of list operations that read or build whole lists.
//!
//! Each model states the result with defined terms, or with a fresh
//! constant and the facts that hold of it. A result no fact pins down
//! stays a made-up one: the executor reports a check that reads it as not
//! modeled.

use super::exec::{Exec, Goal, IntTy, State, Store, Val};
use super::ObligationKind;
use crate::diag::Span;
use crate::mir::{MirArg, Operand};
use crate::runtime_ops::ListOp;

impl Exec<'_> {
    /// The result of the list operation `op` on `list`, when this module
    /// models it.
    pub(super) fn list_op(
        &mut self,
        state: &mut State,
        op: ListOp,
        list: Val,
        args: &[MirArg],
    ) -> Option<Val> {
        match op {
            ListOp::Contains => {
                let Val::List { elem, .. } = &list else {
                    return None;
                };
                let hint = IntTy::of(elem);
                let item = self.operand(state, &args.first()?.value, hint);
                self.list_contains(&list, &item)
            }
            ListOp::Slice => self.list_slice(state, list, args),
            _ => None,
        }
    }

    /// `list[start:end]`: a new list of the elements from `start` up to
    /// `end`. A bound counts from the end when negative, and a missing one
    /// is that end of the list. The program stops unless the bounds are in
    /// order and inside the list.
    fn list_slice(&mut self, state: &mut State, list: Val, args: &[MirArg]) -> Option<Val> {
        let Val::List { len, data, elem } = list else {
            return None;
        };
        let low = self.slice_bound(state, &len, args, 0, "0")?;
        let high = self.slice_bound(state, &len, args, 2, &len)?;
        let in_range = format!("(and (<= 0 {low}) (<= {low} {high}) (<= {high} {len}))");
        let span = match (
            args.get(4).map(|arg| &arg.value),
            args.get(5).map(|arg| &arg.value),
        ) {
            (Some(Operand::Int(line)), Some(Operand::Int(column))) => Span {
                line: *line as usize,
                column: *column as usize,
            },
            _ => self.span,
        };
        let pc = state.pc.clone();
        self.oblige(
            ObligationKind::IndexInRange,
            span,
            "slice bounds in order and inside the list",
            &pc,
            Goal::Holds(in_range.clone()),
        );
        self.assume(&pc, in_range);
        let data = match (data, low.as_str()) {
            // A slice from the start holds the list's own first elements.
            (Some(store), "0") => Some(store),
            (Some(store), _) => self.reindexed(&store, &|i| format!("(+ {i} {low})")),
            (None, _) => None,
        };
        let len = self.define("Int", "len", format!("(- {high} {low})"));
        Some(Val::List { len, data, elem })
    }

    /// The position a slice bound names: argument `first` when the flag
    /// after it says the bound was written, else `missing`.
    fn slice_bound(
        &mut self,
        state: &mut State,
        len: &str,
        args: &[MirArg],
        first: usize,
        missing: &str,
    ) -> Option<String> {
        let Val::Int(bound, _) = self.operand(state, &args.get(first)?.value, Some(IntTy::INT64))
        else {
            return None;
        };
        let Val::Bool(written) = self.operand(state, &args.get(first + 1)?.value, None) else {
            return None;
        };
        match written.as_str() {
            "false" => Some(missing.to_string()),
            "true" => Some(self.normalize_index(len, &bound)),
            _ => None,
        }
    }

    /// A store whose position `i` holds what `store` holds at `at(i)`.
    fn reindexed(&mut self, store: &Store, at: &dyn Fn(&str) -> String) -> Option<Store> {
        match store {
            Store::Leaf(array, sort) => {
                let copy = self.fresh_const(&format!("(Array Int {sort})"), "copy");
                let i = self.bound_name();
                let from = at(&i);
                self.fact(format!(
                    "(forall (({i} Int)) (= (select {copy} {i}) (select {array} {from})))"
                ));
                Some(Store::Leaf(copy, sort))
            }
            Store::Items(items) => items
                .iter()
                .map(|item| self.reindexed(item, at))
                .collect::<Option<Vec<_>>>()
                .map(Store::Items),
            // A span's store exists only while a contract is read.
            Store::Shifted(..) => None,
        }
    }

    /// `item in list`: some position of the list holds `item`. A contract
    /// states membership the same way, so the two agree. Floats are left
    /// out: `nan` is not equal to itself.
    fn list_contains(&mut self, list: &Val, item: &Val) -> Option<Val> {
        if !matches!(item, Val::Int(..) | Val::Bool(_) | Val::Str(_)) {
            return None;
        }
        let held = self.membership(item, list).ok()?;
        Some(Val::Bool(self.define("Bool", "contains", held)))
    }
}
