//! Models of list operations that read or build whole lists.
//!
//! Each model states the result with defined terms, or with a fresh
//! constant and the facts that hold of it. A result no fact pins down
//! stays a made-up one: the executor reports a check that reads it as not
//! modeled.

use std::collections::{BTreeMap, BTreeSet};

use super::exec::{
    element_sort, loan_roots, root_of, structural_eq, symbols, terminator_targets, Exec, Goal,
    IntTy, Obligation, State, Store, Val,
};
use super::ObligationKind;
use crate::ast::BinaryOp;
use crate::diag::Span;
use crate::mir::{BasicBlock, CallTarget, Instruction, MirArg, Operand, Rvalue, Terminator};
use crate::runtime_ops::{ListOp, RuntimeOp};
use crate::sema::Type;

/// The fewest and the most times something happens on a path.
type Count = (u64, u64);

fn widen(slot: &mut Option<Count>, (low, high): Count) -> bool {
    let merged = match *slot {
        Some((a, b)) => (a.min(low), b.max(high)),
        None => (low, high),
    };
    let changed = *slot != Some(merged);
    *slot = Some(merged);
    changed
}

/// What each array of `store` holds for position `index`, in order.
fn leaf_selects(store: &Store, index: &str) -> Vec<String> {
    match store {
        Store::Leaf(array, _) => vec![format!("(select {array} {index})")],
        Store::Items(items) => items
            .iter()
            .flat_map(|item| leaf_selects(item, index))
            .collect(),
        Store::Shifted(base, start) => leaf_selects(base, &format!("(+ {index} {start})")),
        Store::Reversed(base, last) => leaf_selects(base, &format!("(- {last} {index})")),
        Store::Lists(lens, rows) => std::iter::once(lens)
            .chain(rows.as_ref().map(|(rows, _)| rows))
            .map(|array| format!("(select {array} {index})"))
            .collect(),
    }
}

/// The list a store of lists holds at `index`: its length, and its own
/// elements as the array the store keeps for it.
pub(super) fn row(
    lens: &str,
    rows: &Option<(String, &'static str)>,
    elem: &Type,
    index: &str,
) -> Val {
    let inner = match elem {
        Type::Named(_, args) if args.len() == 1 => args[0].clone(),
        _ => Type::Unit,
    };
    Val::List {
        len: format!("(select {lens} {index})"),
        data: rows
            .as_ref()
            .map(|(rows, sort)| Store::Leaf(format!("(select {rows} {index})"), sort)),
        elem: inner,
    }
}

/// Every array of a store of arrays of its own.
fn leaf_arrays(store: &Store) -> Vec<String> {
    match store {
        Store::Leaf(array, _) => vec![array.clone()],
        Store::Items(items) => items.iter().flat_map(leaf_arrays).collect(),
        Store::Shifted(base, _) | Store::Reversed(base, _) => leaf_arrays(base),
        Store::Lists(lens, rows) => std::iter::once(lens.clone())
            .chain(rows.iter().map(|(rows, _)| rows.clone()))
            .collect(),
    }
}

/// The first array of a store.
fn first_array(store: &Store) -> String {
    match store {
        Store::Leaf(array, _) | Store::Lists(array, _) => array.clone(),
        Store::Items(items) => items.first().map(first_array).unwrap_or_default(),
        Store::Shifted(base, _) | Store::Reversed(base, _) => first_array(base),
    }
}

/// The sort of the elements of a store of scalars.
fn scalar_sort(store: &Store) -> Option<&'static str> {
    match store {
        Store::Leaf(_, sort) => Some(sort),
        Store::Items(_) | Store::Lists(..) => None,
        Store::Shifted(base, _) | Store::Reversed(base, _) => scalar_sort(base),
    }
}

/// What the one array of a store of scalars holds for position `index`.
fn same_leaf(store: &Store, index: &str) -> String {
    leaf_selects(store, index).concat()
}

/// The array of a store of scalars, and the positions of it that hold the
/// store's positions from `low` up to `high`: its own from a low up to a
/// high. A quantifier over those reads the array at its bound variable,
/// which a solver matches more readily than at a sum.
fn leaf_range(store: &Store, low: String, high: String) -> Option<(String, String, String)> {
    match store {
        Store::Leaf(array, _) => Some((array.clone(), low, high)),
        Store::Items(_) | Store::Lists(..) => None,
        Store::Shifted(base, start) => leaf_range(
            base,
            format!("(+ {low} {start})"),
            format!("(+ {high} {start})"),
        ),
        Store::Reversed(base, last) => leaf_range(
            base,
            format!("(+ (- {last} {high}) 1)"),
            format!("(+ (- {last} {low}) 1)"),
        ),
    }
}

/// Whether the MIR text `text` names the local `root` or a place in it.
fn names(text: &str, root: &str) -> bool {
    text.contains(&format!("\"{root}\"")) || text.contains(&format!("\"{root}."))
}

/// Whether `instruction` appends one element to the list local `root`
/// and names it nowhere else.
fn is_push(instruction: &Instruction, root: &str) -> bool {
    let Instruction::Assign {
        value:
            Rvalue::Call {
                callee:
                    CallTarget::Runtime {
                        op: RuntimeOp::List(ListOp::Push | ListOp::PushScalar(_)),
                        receiver: Some(Operand::Place(receiver)),
                        receiver_place: Some(place),
                    },
                args,
                ..
            },
        target,
    } = instruction
    else {
        return false;
    };
    receiver == root && place == root && !names(&format!("{target:?}{args:?}"), root)
}

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
                        let last = self.define("Int", "last", format!("(- {len} 1)"));
                        Some(Store::Reversed(Box::new(store), last))
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
        let data = data.map(|store| Store::Shifted(Box::new(store), low.clone()));
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

    /// A store of arrays of its own that holds what `store` holds, for a
    /// store that reads another one's arrays at other positions.
    pub(super) fn copied_store(&mut self, store: &Store) -> Store {
        let copy = self.fresh_like(store, "copy");
        let i = self.bound_name();
        let same = leaf_selects(&copy, &i)
            .iter()
            .zip(leaf_selects(store, &i))
            .map(|(new, old)| format!("(= {new} {old})"))
            .collect::<Vec<_>>();
        let arrays = leaf_arrays(&copy);
        self.lazy_fact(
            &arrays.iter().map(String::as_str).collect::<Vec<_>>(),
            format!("(forall (({i} Int)) (and {}))", same.join(" ")),
        );
        copy
    }

    /// A store of the shape of `store`, with a fresh array for each of
    /// its arrays.
    fn fresh_like(&mut self, store: &Store, hint: &str) -> Store {
        match store {
            Store::Leaf(_, sort) => {
                Store::Leaf(self.fresh_const(&format!("(Array Int {sort})"), hint), sort)
            }
            Store::Items(items) => Store::Items(
                items
                    .iter()
                    .map(|item| self.fresh_like(item, hint))
                    .collect(),
            ),
            Store::Shifted(base, _) | Store::Reversed(base, _) => self.fresh_like(base, hint),
            Store::Lists(_, rows) => {
                let lens = self.fresh_const("(Array Int Int)", hint);
                let rows = rows
                    .as_ref()
                    .map(|(_, sort)| (self.fresh_rows(sort, hint), *sort));
                Store::Lists(lens, rows)
            }
        }
    }

    // ----- Facts asked only where they matter --------------------------------

    /// States `fact`, which says what the fresh constants `made` hold and
    /// nothing else: whatever the rest holds, some value of them makes it
    /// true. A quantified fact in every query makes the solver give up
    /// where it would find a counterexample, so this one is left out of a
    /// check that cannot reach `made`.
    fn lazy_fact(&mut self, made: &[&str], fact: String) {
        let made = made.iter().map(|name| name.to_string()).collect();
        self.lazy.insert(self.facts.len(), made);
        self.facts.push(fact);
    }

    /// The lazy facts the check `obligation` may depend on, as assertions:
    /// those whose constants its goal or its path reach through the facts
    /// known at the check.
    pub(super) fn lazy_asserts(&self, obligation: &Obligation) -> String {
        let mut text = String::new();
        let Goal::Holds(goal) = &obligation.goal else {
            return text;
        };
        if self.lazy.range(..obligation.facts).next().is_none() {
            return text;
        }
        let facts: Vec<Vec<&str>> = self.facts[..obligation.facts]
            .iter()
            .map(|fact| symbols(fact))
            .collect();
        let guard = obligation.guard.as_deref().unwrap_or("");
        let mut reached: BTreeSet<&str> = symbols(goal)
            .into_iter()
            .chain(symbols(&obligation.pc))
            .chain(symbols(guard))
            .collect();
        let mut used = vec![false; facts.len()];
        loop {
            let mut changed = false;
            for (index, names) in facts.iter().enumerate() {
                if used[index] {
                    continue;
                }
                let lazy = self.lazy.get(&index);
                let wanted = match lazy {
                    Some(made) => made.iter().any(|name| reached.contains(name.as_str())),
                    None => names.iter().any(|name| reached.contains(name)),
                };
                if wanted {
                    used[index] = true;
                    changed = true;
                    reached.extend(names.iter().copied());
                    if lazy.is_some() {
                        text.push_str(&format!("(assert {})\n", self.facts[index]));
                    }
                }
            }
            if !changed {
                return text;
            }
        }
    }

    // ----- Lists of lists ---------------------------------------------------

    fn fresh_rows(&mut self, sort: &str, hint: &str) -> String {
        self.fresh_const(&format!("(Array Int (Array Int {sort}))"), hint)
    }

    /// A fresh store for elements of type `elem` when they are lists: the
    /// length of each, and its elements when they are scalars.
    pub(super) fn fresh_lists_store(&mut self, elem: &Type, hint: &str) -> Option<Store> {
        let Type::Named(name, args) = elem else {
            return None;
        };
        if name != "list" || args.len() != 1 {
            return None;
        }
        let lens = self.fresh_const("(Array Int Int)", &format!("{hint}_lens"));
        let rows = element_sort(&args[0]).map(|sort| (self.fresh_rows(sort, hint), sort));
        Some(Store::Lists(lens, rows))
    }

    /// A list read from a store of lists, with its length named: no list
    /// is longer than `int64` can count.
    pub(super) fn named_row(&mut self, row: Val, hint: &str) -> Val {
        let Val::List { len, data, elem } = row else {
            return row;
        };
        let len = self.define("Int", &format!("{hint}_len"), len);
        self.fact(format!("(and (<= 0 {len}) (<= {len} {}))", i64::MAX));
        Val::List { len, data, elem }
    }

    /// The store of lists `store` with the list `value` at `index`.
    pub(super) fn put_row(&mut self, store: &Store, index: &str, value: &Val) -> Option<Store> {
        let (Store::Lists(lens, rows), Val::List { len, data, .. }) = (store, value) else {
            return None;
        };
        let lens = self.define(
            "(Array Int Int)",
            "lens",
            format!("(store {lens} {index} {len})"),
        );
        let rows = rows.as_ref().map(|(rows, sort)| {
            // The elements of the list, as one array. A list whose
            // elements the encoding lost has some array of them.
            let plain = match data {
                Some(data @ (Store::Shifted(..) | Store::Reversed(..))) => {
                    Some(self.copied_store(data))
                }
                other => other.clone(),
            };
            let own = match plain {
                Some(Store::Leaf(array, _)) => array,
                _ => self.fresh_const(&format!("(Array Int {sort})"), "row"),
            };
            let rows = self.define(
                &format!("(Array Int (Array Int {sort}))"),
                "rows",
                format!("(store {rows} {index} {own})"),
            );
            (rows, *sort)
        });
        Some(Store::Lists(lens, rows))
    }

    /// The store of lists each of `stores` is on the path its condition
    /// in `pcs` describes.
    pub(super) fn merge_rows(
        &mut self,
        pcs: &[String],
        stores: &[&Store],
        hint: &str,
    ) -> Option<Store> {
        let mut all_lens = Vec::new();
        let mut all_rows = Vec::new();
        for store in stores {
            let Store::Lists(lens, rows) = store else {
                return None;
            };
            all_lens.push(lens.as_str());
            all_rows.extend(rows.as_ref());
        }
        let lens = self.merge_terms("(Array Int Int)", pcs, &all_lens, &format!("{hint}_lens"));
        let rows = match all_rows.first() {
            Some((_, sort)) if all_rows.len() == stores.len() => {
                let terms: Vec<&str> = all_rows.iter().map(|(rows, _)| rows.as_str()).collect();
                let sort_text = format!("(Array Int (Array Int {sort}))");
                Some((self.merge_terms(&sort_text, pcs, &terms, hint), *sort))
            }
            _ => None,
        };
        Some(Store::Lists(lens, rows))
    }

    /// The store of a list of `len` elements after a sort: the elements of
    /// `store` in another order. For integer elements sorted by their own
    /// order, descending when `descending` holds, that leaves one result.
    /// Any other order is the sort's own, which the encoding does not
    /// model, so the result stays a made-up one that holds the same
    /// elements.
    fn sorted(&mut self, store: &Store, len: &str, descending: Option<String>) -> Store {
        let ordered = match (scalar_sort(store), descending) {
            (Some("Int"), Some(descending)) => Some(descending),
            _ => None,
        };
        let result = match &ordered {
            Some(_) => self.fresh_like(store, "sorted"),
            None => self.unmodeled("the order of the elements after a sort", |exec| {
                exec.fresh_like(store, "sorted")
            }),
        };
        // The arrays the list reads, and the positions of them it holds:
        // the facts read those arrays at a position itself, which a
        // solver matches more readily than at a sum. Any other store is
        // read where it is: a quantified copy of it, over every position,
        // makes the solver give up on checks that never read the result.
        let (source, low, high) = match leaf_range(store, "0".to_string(), len.to_string()) {
            Some((array, low, high)) => {
                let sort = scalar_sort(store).expect("a store with one array is of scalars");
                (Store::Leaf(array, sort), low, high)
            }
            None => (store.clone(), "0".to_string(), len.to_string()),
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
        let in_result = |k: &str| format!("(and (<= 0 {k}) (< {k} {len}))");
        let in_source = |k: &str| format!("(and (<= {low} {k}) (< {k} {high}))");
        let same = |new: &str, old: &str| {
            leaf_selects(&result, new)
                .iter()
                .zip(leaf_selects(&source, old))
                .map(|(new, old)| format!("(= {new} {old})"))
                .collect::<Vec<_>>()
                .join(" ")
        };
        let moved = |i: &str| {
            let origin = format!("({from} {i})");
            format!(
                "(and {} (= ({to} {origin}) {i}) {})",
                in_source(&origin),
                same(i, &origin)
            )
        };
        let kept = |j: &str| {
            let target = format!("({to} {j})");
            format!(
                "(and {} (= ({from} {target}) {j}) {})",
                in_result(&target),
                same(&target, j)
            )
        };
        let in_order = |i: &str, j: &str| {
            let Some(descending) = &ordered else {
                return "true".to_string();
            };
            let (first, second) = (same_leaf(&result, i), same_leaf(&result, j));
            format!("(ite {descending} (>= {first} {second}) (<= {first} {second}))")
        };
        // A short list of a known length has each fact once for each
        // position: the solver then decides every question about it.
        let mut facts = Vec::new();
        match len.parse::<u8>() {
            Ok(count) if count <= 16 => {
                for position in 0..count {
                    let (here, next) = (position.to_string(), (position + 1).to_string());
                    facts.push(moved(&here));
                    facts.push(kept(&format!("(+ {low} {here})")));
                    if position + 1 < count {
                        facts.push(in_order(&here, &next));
                    }
                }
            }
            _ => {
                facts.push(format!(
                    "(forall (({i} Int)) (=> {} {}))",
                    in_result(&i),
                    moved(&i)
                ));
                facts.push(format!(
                    "(forall (({j} Int)) (=> {} {}))",
                    in_source(&j),
                    kept(&j)
                ));
                if ordered.is_some() {
                    facts.push(format!(
                        "(forall (({i} Int) ({j} Int)) (=> (and (<= 0 {i}) (<= {i} {j}) (< {j} {len})) {}))",
                        in_order(&i, &j)
                    ));
                }
            }
        }
        // The facts are one, about the result: a check that reads only
        // the list that was sorted does not depend on them.
        let first = first_array(&result);
        self.definitions.insert(first.clone(), self.facts.len());
        let fact = format!(
            "(and (= (select {first} 0) (select {first} 0)) {})",
            facts.join(" ")
        );
        // Some result and order satisfy them whatever the list holds, so
        // they go only to the checks that read the result: every sort of a
        // short list in each query makes a counterexample slow to find.
        let mut made = leaf_arrays(&result);
        made.extend([from, to]);
        self.lazy_fact(&made.iter().map(String::as_str).collect::<Vec<_>>(), fact);
        result
    }

    /// `item in list`: some position of the list holds `item`, as a
    /// contract states membership. When it holds, a position is named
    /// that holds the item; when it does not, no position does. Floats
    /// are left out: `nan` is not equal to itself.
    fn list_contains(&mut self, list: &Val, item: &Val) -> Option<Val> {
        let Val::List {
            len,
            data: Some(data),
            elem,
        } = list
        else {
            return None;
        };
        if !matches!(item, Val::Int(..) | Val::Bool(_) | Val::Str(_)) {
            return None;
        }
        let holds = |exec: &Self, position: &str| {
            structural_eq(item, &exec.store_select(data, elem, position))
        };
        // A short list of a known length holds the item at one of its
        // positions: no quantifier is needed.
        if let Some(count) = len.parse::<u8>().ok().filter(|count| *count <= 16) {
            let mut held = "false".to_string();
            for position in (0..count).rev() {
                held = format!("(or {} {held})", holds(self, &position.to_string())?);
            }
            return Some(Val::Bool(self.define("Bool", "contains", held)));
        }
        holds(self, "0")?;
        let held = self.fresh_const("Bool", "contains");
        let (at, k) = (self.fresh_const("Int", "at"), self.bound_name());
        let (found, other) = (holds(self, &at)?, holds(self, &k)?);
        let inside = |k: &str| format!("(and (<= 0 {k}) (< {k} {len}))");
        self.lazy_fact(
            &[&held, &at],
            format!(
                "(and (=> {held} (and {} {found})) (=> (not {held}) (forall (({k} Int)) (=> {} (not {other})))))",
                inside(&at),
                inside(&k)
            ),
        );
        Some(Val::Bool(held))
    }

    // ----- Loops the compiler can describe ----------------------------------

    /// What holds each time the loop `(body, header)` reaches its header,
    /// for the shapes read here from its MIR. `entry` is the state that
    /// enters the loop and `havoc` the state of any later arrival, whose
    /// values this replaces or states facts about.
    ///
    /// A list the loop only appends to, a fixed number of times for each
    /// step of a counter, has a length that follows the counter. A local
    /// that keeps the least or the greatest element of a list read so far
    /// is a bound of those elements and one of them.
    pub(super) fn loop_facts(
        &mut self,
        (body, header): (&BTreeSet<usize>, usize),
        bounds: &BTreeMap<String, Operand>,
        writes: &BTreeSet<String>,
        entry: &State,
        havoc: &mut State,
    ) {
        for counter in bounds.keys() {
            let (Some(Val::Int(start, _)), Some(Val::Int(now, _))) =
                (entry.env.get(counter), havoc.env.get(counter))
            else {
                continue;
            };
            let (start, now) = (start.clone(), now.clone());
            let steps = |block: &BasicBlock| {
                let step = |instruction: &&Instruction| matches!(instruction, Instruction::Assign { target, .. } if target == counter);
                block.instructions.iter().filter(step).count() as u64
            };
            // Each pass through the loop steps the counter once.
            if self.per_pass(body, header, &steps).1 != Some((1, 1)) {
                continue;
            }
            for root in writes {
                self.grown_list(body, header, root, (&start, &now), entry, havoc);
            }
            self.reduction(body, header, writes, (counter, &start, &now), entry, havoc);
        }
    }

    /// Whether something other than an instruction that names the local
    /// `root` may write it: a closure that captures it, or a view of it.
    fn shared(&self, root: &str) -> bool {
        self.captured.contains(root)
            || loan_roots(&self.function.blocks)
                .values()
                .any(|viewed| viewed.contains(root))
    }

    /// How many times a path from the loop's header runs what `weight`
    /// counts in each block: on the way to each block of the loop, and on
    /// the way back to the header. Nothing for the way back when a path
    /// can count without end, inside a loop of its own.
    fn per_pass(
        &self,
        body: &BTreeSet<usize>,
        header: usize,
        weight: &dyn Fn(&BasicBlock) -> u64,
    ) -> (BTreeMap<usize, Count>, Option<Count>) {
        let blocks = &self.function.blocks;
        let mut reach: BTreeMap<usize, Option<Count>> = BTreeMap::from([(header, Some((0, 0)))]);
        for _ in 0..=body.len() {
            let mut back = None;
            let mut changed = false;
            for (block, count) in reach.clone() {
                let (low, high) = count.expect("a reached block has a count");
                let added = weight(&blocks[block]);
                for label in terminator_targets(&blocks[block].terminator) {
                    let target = self.labels[label];
                    if target == header {
                        widen(&mut back, (low + added, high + added));
                    } else if body.contains(&target) {
                        changed |= widen(
                            reach.entry(target).or_default(),
                            (low + added, high + added),
                        );
                    }
                }
            }
            if !changed {
                let reach = reach
                    .into_iter()
                    .filter_map(|(block, count)| count.map(|count| (block, count)))
                    .collect();
                return (reach, back);
            }
        }
        (BTreeMap::new(), None)
    }

    /// States the length of the list local `root` at the loop's header,
    /// when every write of it in the loop is an append: its length on
    /// entry plus the appends of each pass, for the passes the counter has
    /// counted from `start` to `now`.
    fn grown_list(
        &mut self,
        body: &BTreeSet<usize>,
        header: usize,
        root: &str,
        (start, now): (&str, &str),
        entry: &State,
        havoc: &mut State,
    ) {
        let blocks = &self.function.blocks;
        let (Some(Val::List { len: before, .. }), Some(Val::List { len, data, elem })) =
            (entry.env.get(root), havoc.env.get(root))
        else {
            return;
        };
        // The loop names the list only where it appends to it, and no
        // closure can change it. A block that ends the function never
        // comes back to the header.
        let only_pushes = body.iter().all(|&block| {
            let block = &blocks[block];
            terminator_targets(&block.terminator).is_empty()
                || block.instructions.iter().all(|instruction| {
                    is_push(instruction, root) || !names(&format!("{instruction:?}"), root)
                }) && !names(&format!("{:?}", block.terminator), root)
        });
        if !only_pushes || self.shared(root) {
            return;
        }
        let pushes = |block: &BasicBlock| {
            let push = |instruction: &&Instruction| is_push(instruction, root);
            block.instructions.iter().filter(push).count() as u64
        };
        let Some((least, most)) = self.per_pass(body, header, &pushes).1 else {
            return;
        };
        let after = |count: u64| format!("(+ {before} (* {count} (- {now} {start})))");
        if least == most {
            // The length is known: it is no longer a made-up one.
            let len = self.define("Int", "len", after(least));
            // No list is longer than `int64` can count.
            let pc = havoc.pc.clone();
            self.assume(&pc, format!("(and (<= 0 {len}) (<= {len} {}))", i64::MAX));
            let value = Val::List {
                len,
                data: data.clone(),
                elem: elem.clone(),
            };
            havoc.env.insert(root.to_string(), value);
        } else {
            let pc = havoc.pc.clone();
            let (low, high) = (after(least), after(most));
            // The fact is about the length: a check that reads only the
            // counter does not depend on it.
            self.definitions.insert(len.clone(), self.facts.len());
            self.assume(&pc, format!("(and (>= {len} {low}) (<= {len} {high}))"));
        }
    }

    /// The loop of `min(list)` or `max(list)`: each pass reads the next
    /// element, and keeps it in `kept` when it is the first or `kept` does
    /// not compare to it as the built-in asks. `seen` says an element was
    /// read. At the header, `seen` holds once the counter has moved, and
    /// then `kept` is a bound of the elements read and one of them.
    ///
    /// Every block the shape names is matched whole, and the loop may name
    /// `kept` and `seen` nowhere else, so nothing else writes them.
    fn reduction(
        &mut self,
        body: &BTreeSet<usize>,
        header: usize,
        writes: &BTreeSet<String>,
        (counter, start, now): (&str, &str, &str),
        entry: &State,
        havoc: &mut State,
    ) {
        let blocks = &self.function.blocks;
        let label = |label: &str| self.labels[label];
        // The pass reads `list[counter]` into `element` and then steps the
        // counter, in one block.
        let fetch = body.iter().find_map(|&block| {
            let instructions = &blocks[block].instructions;
            let read = instructions.iter().position(|instruction| {
                matches!(
                    instruction,
                    Instruction::Assign {
                        value: Rvalue::Call {
                            callee: CallTarget::Runtime {
                                op: RuntimeOp::List(
                                    ListOp::IndexUnchecked | ListOp::IndexScalarUnchecked(_),
                                ),
                                receiver: Some(Operand::Place(_)),
                                ..
                            },
                            args,
                            ..
                        },
                        ..
                    } if matches!(args.first(), Some(MirArg { value: Operand::Place(index), .. }) if index == counter)
                )
            })?;
            let step = instructions.iter().position(
                |instruction| matches!(instruction, Instruction::Assign { target, .. } if target == counter),
            )?;
            let Instruction::Assign {
                target: element,
                value:
                    Rvalue::Call {
                        callee:
                            CallTarget::Runtime {
                                receiver: Some(Operand::Place(list)),
                                ..
                            },
                        ..
                    },
            } = &instructions[read]
            else {
                return None;
            };
            (read < step).then_some((block, read, element, list))
        });
        let Some((fetch_block, fetch_at, element, list)) = fetch else {
            return;
        };
        // The test of `seen`, then the first element or the comparison,
        // then the replacement.
        let shape = body.iter().find_map(|&test| {
            let Terminator::Branch {
                condition: Operand::Place(seen),
                then_label,
                else_label,
            } = &blocks[test].terminator
            else {
                return None;
            };
            let (compare, first) = (label(then_label), label(else_label));
            let [Instruction::Assign {
                target: flag,
                value: Rvalue::Use(Operand::Bool(true)),
            }] = blocks[first].instructions.as_slice()
            else {
                return None;
            };
            let Terminator::Goto(replace_label) = &blocks[first].terminator else {
                return None;
            };
            let replace = label(replace_label);
            let [Instruction::Assign {
                target: kept,
                value: Rvalue::Use(Operand::Place(taken)),
            }] = blocks[replace].instructions.as_slice()
            else {
                return None;
            };
            let [Instruction::Assign {
                target: holds,
                value:
                    Rvalue::Binary {
                        op,
                        left: Operand::Place(left),
                        right: Operand::Place(right),
                        ..
                    },
            }] = blocks[compare].instructions.as_slice()
            else {
                return None;
            };
            let Terminator::Branch {
                condition: Operand::Place(condition),
                then_label: keep_label,
                else_label: other_label,
            } = &blocks[compare].terminator
            else {
                return None;
            };
            let order = match op {
                BinaryOp::LessEq => "<=",
                BinaryOp::GreaterEq => ">=",
                _ => return None,
            };
            let matched = flag == seen
                && taken == element
                && left == kept
                && right == element
                && condition == holds
                && label(other_label) == replace
                && label(keep_label) != replace
                && ![seen, kept, holds].contains(&element)
                && seen != kept
                && ![seen, kept, element].contains(&holds);
            matched.then_some((test, compare, first, replace, seen, kept, order))
        });
        let Some((test, compare, first, replace, seen, kept, order)) = shape else {
            return;
        };
        // Only the test leads to the comparison and to the first element,
        // and only those two to the replacement.
        let mut preds: BTreeMap<usize, BTreeSet<usize>> = BTreeMap::new();
        for (index, block) in blocks.iter().enumerate() {
            for target in terminator_targets(&block.terminator) {
                preds.entry(label(target)).or_default().insert(index);
            }
        }
        let entered = |block: usize, from: &[usize]| {
            preds.get(&block) == Some(&from.iter().copied().collect())
        };
        if !entered(compare, &[test])
            || !entered(first, &[test])
            || !entered(replace, &[first, compare])
        {
            return;
        }
        // Every pass reads the element once, and tests `seen` once, after
        // the read.
        let at = |wanted: usize| {
            move |block: &BasicBlock| u64::from(std::ptr::eq(block, &blocks[wanted]))
        };
        let (before_test, fetched) = self.per_pass(body, header, &at(fetch_block));
        let tested = self.per_pass(body, header, &at(test)).1;
        if fetched != Some((1, 1))
            || tested != Some((1, 1))
            || before_test.get(&test) != Some(&(1, 1))
        {
            return;
        }
        // Nothing else in the loop names the locals of the shape, and no
        // closure can change them. The loop does not write the list.
        let allowed = |block: usize, position: Option<usize>| {
            [first, replace, compare].contains(&block)
                || (block, position) == (fetch_block, Some(fetch_at))
                || (block, position) == (test, None)
        };
        let named = |text: String| {
            [seen, kept, element]
                .iter()
                .any(|local| names(&text, local))
        };
        let stray = body.iter().any(|&index| {
            let block = &blocks[index];
            if terminator_targets(&block.terminator).is_empty() {
                return false;
            }
            let stray_instruction =
                block
                    .instructions
                    .iter()
                    .enumerate()
                    .any(|(position, instruction)| {
                        !allowed(index, Some(position)) && named(format!("{instruction:?}"))
                    });
            stray_instruction || (!allowed(index, None) && named(format!("{:?}", block.terminator)))
        });
        let list_root = root_of(list);
        if stray
            || writes.contains(list_root)
            || [seen, kept, element].iter().any(|local| self.shared(local))
        {
            return;
        }
        let listed = self.read_place(&mut entry.clone(), list);
        let (
            Val::List {
                data: Some(store), ..
            },
            Some(Val::Int(initial, int)),
            Some(Val::Bool(unseen)),
        ) = (&listed, entry.env.get(kept), entry.env.get(seen))
        else {
            return;
        };
        if start != "0" || unseen != "false" || scalar_sort(store) != Some("Int") {
            return;
        }
        let Some((array, low, high)) = leaf_range(store, start.to_string(), now.to_string()) else {
            return;
        };
        let (initial, int) = (initial.clone(), *int);
        let moved = format!("(> {now} {start})");
        let bound = self.fresh_const("Int", kept);
        self.fact(int.contains(&bound));
        let (k, witness) = (self.bound_name(), self.fresh_const("Int", "at"));
        self.fact(format!("(=> (not {moved}) (= {bound} {initial}))"));
        let inside = |k: &str| format!("(and (<= {low} {k}) (< {k} {high}))");
        self.lazy_fact(
            &[&bound, &witness],
            format!(
                "(=> {moved} (and (forall (({k} Int)) (=> {} ({order} {bound} (select {array} {k})))) {} (= {bound} (select {array} {witness}))))",
                inside(&k),
                inside(&witness)
            ),
        );
        havoc.env.insert(kept.clone(), Val::Int(bound, int));
        havoc.env.insert(seen.clone(), Val::Bool(moved));
    }
}
