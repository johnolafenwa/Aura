//! Models of list operations that read or build whole lists.
//!
//! Each model states the result with defined terms, or with a fresh
//! constant and the facts that hold of it. A result no fact pins down
//! stays a made-up one: the executor reports a check that reads it as not
//! modeled.

use std::collections::{BTreeMap, BTreeSet};

use super::exec::{loan_roots, root_of, terminator_targets, Exec, Goal, IntTy, State, Store, Val};
use super::ObligationKind;
use crate::ast::BinaryOp;
use crate::diag::Span;
use crate::mir::{BasicBlock, CallTarget, Instruction, MirArg, Operand, Rvalue, Terminator};
use crate::runtime_ops::{ListOp, RuntimeOp};

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
                .any(|viewed| viewed == root)
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
            let value = Val::List {
                len,
                data: data.clone(),
                elem: elem.clone(),
            };
            havoc.env.insert(root.to_string(), value);
        } else {
            let pc = havoc.pc.clone();
            let (low, high) = (after(least), after(most));
            self.assume(&pc, format!("(and (<= {low} {len}) (<= {len} {high}))"));
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
        let (
            Some(Val::List {
                data: Some(Store::Leaf(array, "Int")),
                ..
            }),
            Some(Val::Int(initial, int)),
            Some(Val::Bool(unseen)),
        ) = (
            entry.env.get(list),
            entry.env.get(kept),
            entry.env.get(seen),
        )
        else {
            return;
        };
        if start != "0" || unseen != "false" {
            return;
        }
        let (array, initial, int) = (array.clone(), initial.clone(), *int);
        let moved = format!("(> {now} {start})");
        let bound = self.fresh_const("Int", kept);
        self.fact(int.contains(&bound));
        let (k, witness) = (self.bound_name(), self.fresh_const("Int", "at"));
        self.fact(format!("(=> (not {moved}) (= {bound} {initial}))"));
        self.fact(format!(
            "(=> {moved} (forall (({k} Int)) (=> (and (<= {start} {k}) (< {k} {now})) ({order} {bound} (select {array} {k})))))"
        ));
        self.fact(format!(
            "(=> {moved} (and (<= {start} {witness}) (< {witness} {now}) (= {bound} (select {array} {witness}))))"
        ));
        havoc.env.insert(kept.clone(), Val::Int(bound, int));
        havoc.env.insert(seen.clone(), Val::Bool(moved));
    }
}
