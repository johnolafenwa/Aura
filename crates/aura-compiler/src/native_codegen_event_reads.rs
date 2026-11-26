//! `try http.__aura_read_event(stream)`, the read `http.Response.read_event`
//! makes for each server-sent event, whose tuple the function takes apart
//! at once. One helper writes the tuple's five fields to words: whether an
//! event was found, the event's three `str`s as boxes, and its retry delay.
//! Neither the `Result` nor the tuple, its `bool` nor its `int64` is boxed.
//! The tuple's local holds the words, and each element read takes its word
//! (see [`WordTuple`]).

use super::*;

/// The builtin whose read this fuses.
const READ_EVENT: &str = "http::__aura_read_event";

/// The elements of the tuple the read gives, in order: the indices of its
/// `str` boxes are 1 to 3, and the others are `bool` and `int64` words.
const EVENT_FIELDS: usize = 5;

/// Whether element `index` of the event tuple is a box.
fn is_event_box(index: usize) -> bool {
    (1..=3).contains(&index)
}

/// The elements of a tuple a fused read gave, each taken by its read: a
/// scalar is copied, and a box moves out and leaves `None`.
pub(super) type WordTuple = Vec<Option<ValueRef>>;

impl FunctionCompiler<'_> {
    /// The result temporary and the stream of a read of an event when
    /// `instructions` open with it and the `try` that moves it out, and the
    /// tuple the `try` gives is only taken apart in the rest of the block:
    /// moved into another temporary, read by element, and released.
    pub(super) fn opens_with_try_event<'i>(
        &self,
        instructions: &'i [Instruction],
    ) -> Option<(&'i str, &'i Operand)> {
        let [Instruction::Assign {
            target: result,
            value:
                Rvalue::Call {
                    callee:
                        CallTarget::Runtime {
                            op: RuntimeOp::Host(builtin),
                            receiver: None,
                            ..
                        },
                    args,
                    ..
                },
        }, Instruction::Assign {
            target: tuple,
            value:
                Rvalue::Try {
                    value: Operand::MovePlace(moved),
                    ..
                },
        }, rest @ ..] = instructions
        else {
            return None;
        };
        let [stream] = args.as_slice() else {
            return None;
        };
        let fused = builtin.name() == READ_EVENT
            && result == moved
            && result.starts_with('%')
            && stream.writeback_place.is_none()
            && !self.lent_places.contains(result.as_str())
            && !self.lookup_slots.contains(result.as_str());
        let mut live = vec![tuple.as_str()];
        let taken_apart = rest.iter().take_while(|_| fused).all(|instruction| {
            let mut mentions = Vec::new();
            crate::mir::instruction_place_mentions(instruction, &mut mentions);
            let named = |live: &[&str], place: &str| live.contains(&place);
            match instruction {
                Instruction::Assign {
                    target,
                    value: Rvalue::Use(Operand::MovePlace(source)),
                } if named(&live, source) => {
                    live.retain(|place| place != source);
                    live.push(target);
                    target.starts_with('%')
                }
                Instruction::Assign {
                    value:
                        Rvalue::TupleElement {
                            tuple: Operand::Place(source),
                            index,
                            ..
                        },
                    ..
                } if named(&live, source) => *index < EVENT_FIELDS && !is_event_box(*index),
                Instruction::Assign {
                    value:
                        Rvalue::TupleTakeElement {
                            place: source,
                            index,
                            ..
                        },
                    ..
                } if named(&live, source) => is_event_box(*index),
                Instruction::Release { place } if named(&live, place) => {
                    live.retain(|live| live != place);
                    true
                }
                _ => !mentions
                    .iter()
                    .any(|place| named(&live, place.split('.').next().unwrap_or(place))),
            }
        });
        // The tuple is gone by the end of the block.
        (fused && taken_apart && live.is_empty()).then_some((result.as_str(), &stream.value))
    }

    /// The read of a `try` that [`Self::opens_with_try_event`] found: the
    /// helper writes the five words and, when the read fails, the error's
    /// `Result` to a word the `try` that follows tests.
    pub(super) fn compile_try_event(
        &mut self,
        result: &str,
        stream: &Operand,
    ) -> std::result::Result<(), String> {
        // A stream in a field is read where it lies, as the builtin's own
        // call passes it.
        let stream_ty = DirectType::Opaque(Type::named("http.Stream"));
        let loaded = self.load_borrowed_field_argument(stream, &stream_ty)?;
        let stream = self.ensure_opaque(loaded)?;
        let slot = self
            .builder
            .create_sized_stack_slot(8 * (EVENT_FIELDS as u32 + 1), 3);
        let words = self.builder.stack_addr(types::I64, slot, 0);
        let failure = self
            .builder
            .stack_addr(types::I64, slot, 8 * EVENT_FIELDS as i32);
        let zero = self.builder.iconst(types::I64, 0);
        self.builder.store(MemFlags::new(), zero, failure, 0);
        self.builder.call(
            self.runtime[RuntimeFn::HttpReadEventTry],
            &[stream.values[0], words, failure],
        );
        self.release_all_temporary_owned();
        self.pending_try_event = Some((result.to_string(), words, failure));
        Ok(())
    }

    /// The `try` of a fused event read: the error returns from the
    /// function, and otherwise `target` holds the words.
    pub(super) fn compile_try_of_event(
        &mut self,
        target: &str,
        source: &str,
        words: Val,
        failure: Val,
        releases_on_error: &[String],
        from: Option<&str>,
    ) -> std::result::Result<(), String> {
        let source_ty = self.type_of_place(source)?;
        let (source_error_ty, target_error_ty) = self.try_error_types(target, &source_ty)?;
        let ok_block = self.builder.create_block();
        let err_block = self.builder.create_block();
        self.builder.set_cold_block(err_block);
        let failed = self.builder.load(types::I64, MemFlags::new(), failure, 0);
        let read = self.builder.icmp_imm(IntCC::Equal, failed, 0);
        self.builder.brif(read, ok_block, &[], err_block, &[]);

        self.builder.switch_to_block(err_block);
        self.builder.seal_block(err_block);
        let value = ValueRef {
            values: vec![failed],
            ty: source_ty,
        };
        self.compile_try_error_return(
            &value,
            true,
            &source_error_ty,
            &target_error_ty,
            releases_on_error,
            from,
        )?;

        self.builder.switch_to_block(ok_block);
        self.builder.seal_block(ok_block);
        let mut elements = WordTuple::with_capacity(EVENT_FIELDS);
        for index in 0..EVENT_FIELDS {
            let word = self
                .builder
                .load(types::I64, MemFlags::new(), words, 8 * index as i32);
            let ty = match index {
                0 => DirectType::Scalar(ScalarKind::Bool),
                1..=3 => DirectType::Opaque(Type::named("str")),
                _ => DirectType::Scalar(ScalarKind::Int64),
            };
            elements.push(Some(ValueRef {
                values: vec![word],
                ty,
            }));
        }
        self.word_tuples.insert(target.to_string(), elements);
        Ok(())
    }

    /// Element `index` of the word tuple `place`: a scalar's word, or the
    /// box, which moves to the reader. `None` when `place` is no word
    /// tuple. [`Self::opens_with_try_event`] saw each box taken once.
    pub(super) fn take_word_tuple_element(
        &mut self,
        place: &str,
        index: usize,
    ) -> Option<ValueRef> {
        let element = &mut self.word_tuples.get_mut(place)?[index];
        let value = if is_event_box(index) {
            element.take()
        } else {
            element.clone()
        }
        .expect("a fused event read's tuple element is read once");
        // The reader owns a box now.
        self.mark_temporary_opaque_owned(&value);
        Some(value)
    }

    /// Moves word tuple `source` into `target`. Returns false when
    /// `source` is no word tuple.
    pub(super) fn move_word_tuple(&mut self, target: &str, source: &str) -> bool {
        let Some(elements) = self.word_tuples.remove(source) else {
            return false;
        };
        self.word_tuples.insert(target.to_string(), elements);
        true
    }

    /// Releases the boxes word tuple `place` still holds. Returns false
    /// when `place` is no word tuple.
    pub(super) fn release_word_tuple(&mut self, place: &str) -> bool {
        let Some(elements) = self.word_tuples.remove(place) else {
            return false;
        };
        // No other temporary is held between instructions.
        for element in elements.into_iter().flatten() {
            self.mark_temporary_opaque_owned(&element);
        }
        self.release_all_temporary_owned();
        true
    }
}
