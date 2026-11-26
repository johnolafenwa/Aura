//! A run of `+` on `str`s, such as `hex(n) + "\r\n" + piece + "\r\n"`,
//! which MIR lowers to one `+` per step, each step's temporary the next
//! step's left side and released after it. The run joins every part in one
//! new box, so no step builds a box of its own or takes the generic path.

use super::*;

/// The `str` parts of a run of `+` and the instructions it covers.
pub(super) struct StrChain {
    /// The parts, left to right.
    parts: Vec<Operand>,
    /// The span every step shares, which a result over the limit reports.
    span: Span,
    /// The index of the last step, relative to the run's start.
    last: usize,
    /// The reads of loans between its steps, which come before the run's
    /// result is built, and the releases of places outside the run there,
    /// which come after it.
    reads: Vec<usize>,
    releases: Vec<usize>,
    /// How many instructions the run covers.
    pub(super) covered: usize,
}

impl FunctionCompiler<'_> {
    /// Whether `operand` is a `str` a run of `+` takes as a part.
    fn is_str_part(&self, operand: &Operand) -> bool {
        match operand {
            Operand::String(_) => true,
            Operand::Place(place) => {
                !place.contains('.')
                    && self.type_of_place(place).ok()
                        == Some(DirectType::Opaque(Type::named("str")))
            }
            _ => false,
        }
    }

    /// The run of `+` on `str`s `instructions` open with, when there are
    /// two steps or more: each step after the first adds to the temporary
    /// the step before it made, only releases and reads of loans come
    /// between them, and each
    /// temporary between steps is released in the run and read nowhere
    /// else.
    pub(super) fn opens_with_str_chain(
        &self,
        label: &str,
        start: usize,
        instructions: &[Instruction],
    ) -> Option<StrChain> {
        let Some(Instruction::Assign {
            target,
            value:
                Rvalue::Binary {
                    op: BinaryOp::Add,
                    left,
                    right,
                    span,
                    trap,
                },
        }) = instructions.first()
        else {
            return None;
        };
        if !self.is_str_part(left) || !self.is_str_part(right) {
            return None;
        }
        let mut parts = vec![left.clone(), right.clone()];
        let mut joined = vec![];
        let mut previous = target.as_str();
        let mut last = 0;
        let mut reads = vec![];
        let mut releases = vec![];
        for (index, instruction) in instructions.iter().enumerate().skip(1) {
            match instruction {
                // The run's result going ends the run.
                Instruction::Release { place } if place == previous => break,
                Instruction::Release { place } => {
                    if !joined.contains(&place.as_str()) {
                        releases.push(index);
                    }
                }
                Instruction::ReadLoan { .. } => reads.push(index),
                Instruction::Assign {
                    target,
                    value:
                        Rvalue::Binary {
                            op: BinaryOp::Add,
                            left: Operand::Place(left),
                            right,
                            span: step_span,
                            trap: step_trap,
                        },
                } if left == previous
                    && step_span == span
                    && step_trap == trap
                    && previous.starts_with('%')
                    && !self.str_slots.contains_key(previous)
                    && !self.lent_places.contains(previous)
                    && self.is_str_part(right)
                    && !matches!(right, Operand::Place(place) if place == previous) =>
                {
                    joined.push(previous);
                    parts.push(right.clone());
                    previous = target;
                    last = index;
                }
                _ => break,
            }
        }
        // The releases after the last step belong to the run only while
        // they release its temporaries.
        let trailing = instructions[last + 1..]
            .iter()
            .take_while(|instruction| {
                matches!(instruction, Instruction::Release { place } if joined.contains(&place.as_str()))
            })
            .count();
        let covered = last + 1 + trailing;
        reads.retain(|index| *index < last);
        releases.retain(|index| *index < last);
        let every_temporary_released = joined.iter().all(|temporary| {
            instructions[..covered]
                .iter()
                .any(|instruction| matches!(instruction, Instruction::Release { place } if place == temporary))
        });
        let promoted = (start..start + covered).any(|index| {
            self.element_promotions
                .contains_key(&(label.to_string(), index))
        });
        (!joined.is_empty() && every_temporary_released && !promoted).then_some(StrChain {
            parts,
            span: *span,
            last,
            reads,
            releases,
            covered,
        })
    }

    /// Compiles the run `chain` opening at `start` in `block`: the reads
    /// of loans in it, then its last step, which builds the whole result,
    /// then the releases outside the run.
    pub(super) fn compile_str_chain_run(
        &mut self,
        block: &BasicBlock,
        start: usize,
        chain: StrChain,
    ) -> std::result::Result<(), String> {
        let last = &block.instructions[start + chain.last];
        let Instruction::Assign { value, .. } = last else {
            unreachable!("a run of `+` ends with a step");
        };
        for index in chain.reads {
            let index = start + index;
            self.compile_instruction(&block.label, index, &block.instructions[index])?;
        }
        self.pending_str_chain = Some((value as *const Rvalue, chain.parts, chain.span));
        self.compile_instruction(&block.label, start + chain.last, last)?;
        debug_assert!(
            self.pending_str_chain.is_none(),
            "the run's last step joins it"
        );
        for index in chain.releases {
            let index = start + index;
            self.compile_instruction(&block.label, index, &block.instructions[index])?;
        }
        Ok(())
    }

    /// The whole run of `+` whose last step is `rvalue`, when the run is
    /// pending: every part's words, joined in one call.
    pub(super) fn compile_pending_str_chain(
        &mut self,
        rvalue: &Rvalue,
    ) -> std::result::Result<Option<ValueRef>, String> {
        if !self
            .pending_str_chain
            .as_ref()
            .is_some_and(|(pending, ..)| std::ptr::eq(*pending, rvalue))
        {
            return Ok(None);
        }
        let (_, parts, span) = self.pending_str_chain.take().expect("the run is pending");
        let mut words = Vec::with_capacity(parts.len() * 2);
        for part in &parts {
            let (ptr, len) = match part {
                Operand::String(text) => self.string_constant(text.as_bytes())?,
                _ => match self.str_slot_words(part) {
                    Some(words) => words,
                    None => {
                        let value = self.load_operand(part)?;
                        let value = self.ensure_opaque(value)?;
                        self.str_box_words(value.values[0])
                    }
                },
            };
            words.extend([ptr, len]);
        }
        let slot = self
            .builder
            .create_sized_stack_slot(8 * words.len() as u32, 3);
        let parts_at = self.builder.stack_addr(types::I64, slot, 0);
        for (index, word) in words.iter().enumerate() {
            self.builder
                .store(MemFlags::trusted(), *word, parts_at, 8 * index as i32);
        }
        let count = self.builder.iconst(types::I64, parts.len() as i64);
        let (line, column) = self.span_values(Some(span));
        let results = self.runtime_call_results(
            self.runtime[RuntimeFn::ConcatStrParts],
            &[parts_at, count, line, column],
        );
        Ok(Some(self.owned_opaque_result(results, Type::named("str"))))
    }
}
