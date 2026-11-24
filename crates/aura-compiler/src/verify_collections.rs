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
        receiver_place: Option<&str>,
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
            ListOp::Reverse | ListOp::Sort | ListOp::SortByKeys => {
                let Val::List { len, data, elem } = list else {
                    return None;
                };
                let data = match (op, data) {
                    (_, None) => None,
                    (ListOp::Reverse, Some(store)) => {
                        Some(self.reindexed(&store, &|i| format!("(- (- {len} 1) {i})")))
                    }
                    (_, Some(store)) => {
                        // `sort(reverse)` orders by the elements' own
                        // order, and a sort by keys by an order of its own.
                        let descending = match args.first() {
                            Some(arg) if op == ListOp::Sort => {
                                match self.operand(state, &arg.value, None) {
                                    Val::Bool(descending) => Some(descending),
                                    _ => None,
                                }
                            }
                            _ => None,
                        };
                        Some(self.sorted(&store, &len, descending))
                    }
                };
                if let Some(place) = receiver_place {
                    self.write_place(state, place, Val::List { len, data, elem });
                }
                Some(Val::Unit)
            }
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
        // A slice from the start holds the list's own first elements.
        if low == "0" {
            return Some(Val::List {
                len: high,
                data,
                elem,
            });
        }
        let data = data.map(|store| self.reindexed(&store, &|i| format!("(+ {i} {low})")));
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
    fn reindexed(&mut self, store: &Store, at: &dyn Fn(&str) -> String) -> Store {
        let mut leaves = Vec::new();
        let copy = self.fresh_like(store, "copy", &mut leaves);
        let i = self.bound_name();
        let from = at(&i);
        for (old, new) in leaves {
            self.fact(format!(
                "(forall (({i} Int)) (= (select {new} {i}) (select {old} {from})))"
            ));
        }
        copy
    }

    /// A store of the shape of `store` with a fresh array for each of its
    /// arrays. Each pair of the two is added to `leaves`.
    fn fresh_like(
        &mut self,
        store: &Store,
        hint: &str,
        leaves: &mut Vec<(String, String)>,
    ) -> Store {
        match store {
            Store::Leaf(array, sort) => {
                let fresh = self.fresh_const(&format!("(Array Int {sort})"), hint);
                leaves.push((array.clone(), fresh.clone()));
                Store::Leaf(fresh, sort)
            }
            Store::Items(items) => Store::Items(
                items
                    .iter()
                    .map(|item| self.fresh_like(item, hint, leaves))
                    .collect(),
            ),
            // A span's store exists only while a contract is read: the
            // function's own steps run on its base list.
            Store::Shifted(base, _) => self.fresh_like(base, hint, leaves),
        }
    }

    /// The store of a list of `len` elements after a sort: the elements of
    /// `store` in another order. For integer elements sorted by their own
    /// order, descending when `descending` holds, that leaves one result.
    /// Any other order is the sort's own, which the encoding does not
    /// model, so the result stays a made-up one that holds the same
    /// elements.
    fn sorted(&mut self, store: &Store, len: &str, descending: Option<String>) -> Store {
        let ordered = match (store, descending) {
            (Store::Leaf(_, "Int"), Some(descending)) => Some(descending),
            _ => None,
        };
        let mut leaves = Vec::new();
        let result = match &ordered {
            Some(_) => self.fresh_like(store, "sorted", &mut leaves),
            None => self.unmodeled("the order of the elements after a sort", |exec| {
                exec.fresh_like(store, "sorted", &mut leaves)
            }),
        };
        // Position `i` of the result holds the element that was at
        // `from(i)`, and the element that was at `j` is now at `to(j)`.
        // The two are functions and not arrays: the solver finds a
        // counterexample to a wrong claim far sooner that way.
        let (i, j) = (self.bound_name(), self.bound_name());
        let (from, to) = (i.replace("k!q", "from!"), j.replace("k!q", "to!"));
        for function in [&from, &to] {
            self.decls
                .push(format!("(declare-fun {function} (Int) Int)"));
        }
        let inside = |k: &str| format!("(and (<= 0 {k}) (< {k} {len}))");
        let (source, target) = (format!("({from} {i})"), format!("({to} {j})"));
        let mut moved = vec![inside(&source), format!("(= ({to} {source}) {i})")];
        let mut kept = vec![inside(&target), format!("(= ({from} {target}) {j})")];
        for (old, new) in &leaves {
            moved.push(format!("(= (select {new} {i}) (select {old} {source}))"));
            kept.push(format!("(= (select {new} {target}) (select {old} {j}))"));
        }
        self.fact(format!(
            "(forall (({i} Int)) (=> {} (and {})))",
            inside(&i),
            moved.join(" ")
        ));
        self.fact(format!(
            "(forall (({j} Int)) (=> {} (and {})))",
            inside(&j),
            kept.join(" ")
        ));
        if let (Some(descending), Some((_, new))) = (ordered, leaves.first()) {
            let (first, second) = (format!("(select {new} {i})"), format!("(select {new} {j})"));
            self.fact(format!(
                "(forall (({i} Int) ({j} Int)) (=> (and (<= 0 {i}) (<= {i} {j}) (< {j} {len})) (ite {descending} (>= {first} {second}) (<= {first} {second}))))"
            ));
        }
        result
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
