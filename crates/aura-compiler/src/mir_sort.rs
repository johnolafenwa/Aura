//! Lowering of `list.sort()` and `list.sort(key=...)`.
//!
//! Elements or keys with a built-in order (integers, floats, `Duration`,
//! and `str`) sort in the runtime with one call, `__sort` or
//! `__sort_by_keys`. Any other ordering type orders through a user `Ord`
//! implementation, so the sort runs here: a stable bottom-up merge sort of
//! the elements' positions that calls `lt` on the elements or keys at them,
//! followed by one `__permute` that moves every element to its place. The
//! list does not change until every comparison has run, so a trapping `lt`
//! leaves it as it was.

use super::{
    binary_operator_trait, is_builtin_binary_operator, ArithmeticTrap, CallTarget, Instruction,
    Lowerer, MirArg, Operand, Rvalue, Terminator, INTERNAL_VEC_INDEX_FIELD,
    INTERNAL_VEC_SET_INDEX_FIELD,
};
use crate::ast::BinaryOp;
use crate::diag::Span;
use crate::runtime_ops::{PERMUTE_MEMBER, SORT_BY_KEYS_MEMBER, SORT_MEMBER};
use crate::sema::Type;

fn int64() -> Type {
    Type::named("int64")
}

fn arg(value: Operand) -> MirArg {
    MirArg {
        name: None,
        value,
        writeback_place: None,
    }
}

fn place(name: &str) -> Operand {
    Operand::Place(name.to_string())
}

impl Lowerer<'_> {
    /// Sorts the list at `source_place` stably in place. `ordering_ty` is
    /// the element type, or the key type when `keys` names the list of
    /// keys the key function computed. `reverse` orders by the mirrored
    /// order, so equal elements keep their order in both directions.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn lower_stable_vec_sort(
        &mut self,
        source: Operand,
        source_place: &str,
        ordering_ty: &Type,
        keys: Option<(&str, &Type)>,
        reverse: Operand,
        span: Span,
        result: &str,
    ) {
        if is_builtin_binary_operator(BinaryOp::Less, ordering_ty, ordering_ty) {
            let (field, mut args) = match keys {
                Some((keys, _)) => (SORT_BY_KEYS_MEMBER, vec![arg(place(keys))]),
                None => (SORT_MEMBER, Vec::new()),
            };
            args.push(arg(reverse));
            self.emit_list_mutation(source, source_place, field, args);
        } else {
            let ordering = keys.map_or(source_place, |(keys, _)| keys);
            let order = self.lower_merge_sort_positions(ordering, ordering_ty, reverse, span);
            self.emit_list_mutation(
                source,
                source_place,
                PERMUTE_MEMBER,
                vec![arg(place(&order))],
            );
        }
        self.emit(Instruction::Assign {
            target: result.to_string(),
            value: Rvalue::Use(Operand::Unit),
        });
    }

    /// Emits a bottom-up merge sort of the positions of the list at
    /// `ordering`, which holds values of `ordering_ty`, and returns the
    /// `list[int64]` local holding them in sorted order. Each pass merges
    /// neighboring runs of `width` positions into `merged`, then copies the
    /// merged run back. A position from the right run goes first only when
    /// its value orders strictly before the left one's, which keeps the
    /// sort stable.
    fn lower_merge_sort_positions(
        &mut self,
        ordering: &str,
        ordering_ty: &Type,
        reverse: Operand,
        span: Span,
    ) -> String {
        let list = Type::Named("list".to_string(), vec![int64()]);
        let len = self.new_typed_temp(int64());
        self.emit(Instruction::Assign {
            target: len.clone(),
            value: Rvalue::Call {
                callee: CallTarget::Member {
                    object: place(ordering),
                    field: "len".to_string(),
                    receiver_place: Some(ordering.to_string()),
                    dispatch: None,
                },
                args: Vec::new(),
                type_args: Default::default(),
            },
        });
        let order = self.new_typed_temp(list.clone());
        let merged = self.new_typed_temp(list);
        for target in [&order, &merged] {
            self.emit(Instruction::Assign {
                target: target.clone(),
                value: Rvalue::VecLiteral {
                    elements: Vec::new(),
                    element_type: int64(),
                },
            });
        }

        // order = [0, 1, ..., len - 1], and merged as long. Both take
        // `int64` values rather than literals, so they keep the scalar form
        // on `aura run` too.
        let fill = self.new_typed_temp(int64());
        self.assign(&fill, Operand::Int(0));
        let fill_condition = self.new_block("vec_sort_fill_condition");
        let fill_body = self.new_block("vec_sort_fill_body");
        let pass_condition = self.new_block("vec_sort_pass_condition");
        self.terminate(Terminator::Goto(self.label(fill_condition)));
        self.switch_to(fill_condition);
        let filling = self.int_binary(BinaryOp::Less, place(&fill), place(&len), span);
        self.branch(&filling, fill_body, pass_condition);
        self.switch_to(fill_body);
        self.emit_list_mutation(place(&order), &order, "append", vec![arg(place(&fill))]);
        self.emit_list_mutation(place(&merged), &merged, "append", vec![arg(place(&fill))]);
        self.emit_vec_index_increment(&fill, span);
        self.terminate(Terminator::Goto(self.label(fill_condition)));

        let width = self.new_typed_temp(int64());
        let start = self.new_typed_temp(int64());
        let mid = self.new_typed_temp(int64());
        let end = self.new_typed_temp(int64());
        let left = self.new_typed_temp(int64());
        let right = self.new_typed_temp(int64());
        let out = self.new_typed_temp(int64());
        let pass_body = self.new_block("vec_sort_pass_body");
        let run_condition = self.new_block("vec_sort_run_condition");
        let run_body = self.new_block("vec_sort_run_body");
        let clamp_mid = self.new_block("vec_sort_clamp_mid");
        let mid_ready = self.new_block("vec_sort_mid_ready");
        let clamp_end = self.new_block("vec_sort_clamp_end");
        let end_ready = self.new_block("vec_sort_end_ready");
        let merge_left = self.new_block("vec_sort_merge_left");
        let merge_right = self.new_block("vec_sort_merge_right");
        let compare = self.new_block("vec_sort_compare");
        let compare_forward = self.new_block("vec_sort_compare_forward");
        let compare_reverse = self.new_block("vec_sort_compare_reverse");
        let take_right = self.new_block("vec_sort_take_right");
        let take_left = self.new_block("vec_sort_take_left");
        let rest_condition = self.new_block("vec_sort_rest_condition");
        let rest_body = self.new_block("vec_sort_rest_body");
        let copy_back = self.new_block("vec_sort_copy_back");
        let copy_condition = self.new_block("vec_sort_copy_condition");
        let copy_body = self.new_block("vec_sort_copy_body");
        let next_run = self.new_block("vec_sort_next_run");
        let next_pass = self.new_block("vec_sort_next_pass");
        let done = self.new_block("vec_sort_done");

        self.switch_to(pass_condition);
        self.assign(&width, Operand::Int(1));
        self.terminate(Terminator::Goto(self.label(pass_body)));

        // while width < len: merge every pair of runs.
        self.switch_to(pass_body);
        let passing = self.int_binary(BinaryOp::Less, place(&width), place(&len), span);
        self.assign(&start, Operand::Int(0));
        self.branch(&passing, run_condition, done);

        // while start < len: merge order[start:mid] and order[mid:end],
        // where mid = min(start + width, len) and end = min(mid + width, len).
        self.switch_to(run_condition);
        let running = self.int_binary(BinaryOp::Less, place(&start), place(&len), span);
        self.branch(&running, run_body, next_pass);
        self.switch_to(run_body);
        let sum = self.int_binary(BinaryOp::Add, place(&start), place(&width), span);
        self.assign(&mid, place(&sum));
        let past = self.int_binary(BinaryOp::Greater, place(&mid), place(&len), span);
        self.branch(&past, clamp_mid, mid_ready);
        self.switch_to(clamp_mid);
        self.assign(&mid, place(&len));
        self.terminate(Terminator::Goto(self.label(mid_ready)));
        self.switch_to(mid_ready);
        let sum = self.int_binary(BinaryOp::Add, place(&mid), place(&width), span);
        self.assign(&end, place(&sum));
        let past = self.int_binary(BinaryOp::Greater, place(&end), place(&len), span);
        self.branch(&past, clamp_end, end_ready);
        self.switch_to(clamp_end);
        self.assign(&end, place(&len));
        self.terminate(Terminator::Goto(self.label(end_ready)));
        // The last run of a pass may have no right half; it stays as it is.
        self.switch_to(end_ready);
        self.assign(&left, place(&start));
        self.assign(&right, place(&mid));
        self.assign(&out, place(&start));
        let paired = self.int_binary(BinaryOp::Less, place(&mid), place(&end), span);
        self.branch(&paired, merge_left, next_pass);

        // Once the left run is used up, the rest of the right run is
        // already in place, so only the merged prefix is copied back.
        self.switch_to(merge_left);
        let left_open = self.int_binary(BinaryOp::Less, place(&left), place(&mid), span);
        self.branch(&left_open, merge_right, copy_back);
        self.switch_to(merge_right);
        let right_open = self.int_binary(BinaryOp::Less, place(&right), place(&end), span);
        self.branch(&right_open, compare, rest_condition);

        self.switch_to(compare);
        let left_at = self.emit_vec_read(place(&order), &left, &int64(), span);
        let right_at = self.emit_vec_read(place(&order), &right, &int64(), span);
        let left_value = self.emit_vec_read(place(ordering), &left_at, ordering_ty, span);
        let right_value = self.emit_vec_read(place(ordering), &right_at, ordering_ty, span);
        self.terminate(Terminator::Branch {
            condition: reverse,
            then_label: self.label(compare_reverse),
            else_label: self.label(compare_forward),
        });
        self.switch_to(compare_forward);
        let right_first = self.emit_ordering_lt(&right_value, &left_value);
        self.branch(&right_first, take_right, take_left);
        self.switch_to(compare_reverse);
        let right_first = self.emit_ordering_lt(&left_value, &right_value);
        self.branch(&right_first, take_right, take_left);

        self.switch_to(take_right);
        self.emit_list_write(&merged, &out, place(&right_at), span);
        self.emit_vec_index_increment(&right, span);
        self.emit_vec_index_increment(&out, span);
        self.terminate(Terminator::Goto(self.label(merge_left)));
        self.switch_to(take_left);
        self.emit_list_write(&merged, &out, place(&left_at), span);
        self.emit_vec_index_increment(&left, span);
        self.emit_vec_index_increment(&out, span);
        self.terminate(Terminator::Goto(self.label(merge_left)));

        // The right run is used up: move the rest of the left run after
        // the merged prefix.
        self.switch_to(rest_condition);
        let rest_open = self.int_binary(BinaryOp::Less, place(&left), place(&mid), span);
        self.branch(&rest_open, rest_body, copy_back);
        self.switch_to(rest_body);
        let rest_at = self.emit_vec_read(place(&order), &left, &int64(), span);
        self.emit_list_write(&merged, &out, place(&rest_at), span);
        self.emit_vec_index_increment(&left, span);
        self.emit_vec_index_increment(&out, span);
        self.terminate(Terminator::Goto(self.label(rest_condition)));

        // order[start:out] = merged[start:out]
        self.switch_to(copy_back);
        self.assign(&left, place(&start));
        self.terminate(Terminator::Goto(self.label(copy_condition)));
        self.switch_to(copy_condition);
        let copying = self.int_binary(BinaryOp::Less, place(&left), place(&out), span);
        self.branch(&copying, copy_body, next_run);
        self.switch_to(copy_body);
        let copied = self.emit_vec_read(place(&merged), &left, &int64(), span);
        self.emit_list_write(&order, &left, place(&copied), span);
        self.emit_vec_index_increment(&left, span);
        self.terminate(Terminator::Goto(self.label(copy_condition)));

        self.switch_to(next_run);
        self.assign(&start, place(&end));
        self.emit(Instruction::Safepoint);
        self.terminate(Terminator::Goto(self.label(run_condition)));

        self.switch_to(next_pass);
        let doubled = self.int_binary(BinaryOp::Add, place(&width), place(&width), span);
        self.assign(&width, place(&doubled));
        self.terminate(Terminator::Goto(self.label(pass_body)));

        self.switch_to(done);
        order
    }

    fn assign(&mut self, target: &str, value: Operand) {
        self.emit(Instruction::Assign {
            target: target.to_string(),
            value: Rvalue::Use(value),
        });
    }

    /// `left op right` on `int64` operands, in a new local: `bool` for a
    /// comparison and `int64` for arithmetic, which cannot overflow here
    /// because every operand is at most twice a list length.
    fn int_binary(&mut self, op: BinaryOp, left: Operand, right: Operand, span: Span) -> String {
        let ty = if matches!(op, BinaryOp::Less | BinaryOp::Greater) {
            Type::named("bool")
        } else {
            int64()
        };
        let target = self.new_typed_temp(ty);
        self.emit(Instruction::Assign {
            target: target.clone(),
            value: Rvalue::Binary {
                op,
                left,
                right,
                span,
                trap: ArithmeticTrap::None,
            },
        });
        target
    }

    fn branch(&mut self, condition: &str, then_block: usize, else_block: usize) {
        self.terminate(Terminator::Branch {
            condition: place(condition),
            then_label: self.label(then_block),
            else_label: self.label(else_block),
        });
    }

    /// Calls the list member `field`, which changes the list at
    /// `list_place`.
    fn emit_list_mutation(
        &mut self,
        list: Operand,
        list_place: &str,
        field: &str,
        args: Vec<MirArg>,
    ) {
        let ignored = self.new_typed_temp(Type::Unit);
        self.emit(Instruction::Assign {
            target: ignored,
            value: Rvalue::Call {
                callee: CallTarget::Member {
                    object: list,
                    field: field.to_string(),
                    receiver_place: Some(list_place.to_string()),
                    dispatch: None,
                },
                args,
                type_args: Default::default(),
            },
        });
    }

    /// `list[index] = value` for the `list[int64]` local `list`.
    fn emit_list_write(&mut self, list: &str, index: &str, value: Operand, span: Span) {
        self.emit_list_mutation(
            place(list),
            list,
            INTERNAL_VEC_SET_INDEX_FIELD,
            vec![
                arg(place(index)),
                arg(value),
                arg(Operand::Int(span.line as u128)),
                arg(Operand::Int(span.column as u128)),
            ],
        );
    }

    /// The checked element read `source[index]`, in a new local of type
    /// `element_ty`. A list in a place is read there, as `xs[i]` is, so the
    /// read does not copy the list.
    fn emit_vec_read(
        &mut self,
        source: Operand,
        index: &str,
        element_ty: &Type,
        span: Span,
    ) -> String {
        let target = self.new_typed_temp(element_ty.clone());
        let receiver_place = match &source {
            Operand::Place(place) | Operand::MovePlace(place) => Some(place.clone()),
            _ => None,
        };
        self.emit(Instruction::Assign {
            target: target.clone(),
            value: Rvalue::Call {
                callee: CallTarget::Member {
                    object: source,
                    field: INTERNAL_VEC_INDEX_FIELD.to_string(),
                    receiver_place,
                    dispatch: None,
                },
                args: vec![
                    arg(place(index)),
                    arg(Operand::Int(span.line as u128)),
                    arg(Operand::Int(span.column as u128)),
                ],
                type_args: Default::default(),
            },
        });
        target
    }

    /// `left < right` in a new `bool` local, through the ordering type's
    /// `Ord` implementation as the operator itself lowers. A type
    /// parameter bounded by `Ord` dispatches the same way.
    fn emit_ordering_lt(&mut self, left: &str, right: &str) -> String {
        let (trait_name, field) =
            binary_operator_trait(BinaryOp::Less).expect("`<` has an operator trait");
        let result = self.new_typed_temp(Type::named("bool"));
        self.emit(Instruction::Assign {
            target: result.clone(),
            value: Rvalue::Call {
                callee: CallTarget::TraitMember {
                    object: place(left),
                    trait_name: trait_name.to_string(),
                    field: field.to_string(),
                    receiver_place: None,
                    dispatch: None,
                },
                args: vec![arg(place(right))],
                type_args: Default::default(),
            },
        });
        result
    }
}
