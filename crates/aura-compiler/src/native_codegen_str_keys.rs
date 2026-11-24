//! Looks a `str` key up where a list holds it.
//!
//! `keys[i] in table` and `table.get(keys[i])` read a `str` element only to
//! look it up. A lookup that takes a box needs a copy of the element in a
//! new box, and a release of the box after. A `list[str]` generated code
//! reads through its `DirectListAccess::Str` view gives the element's bytes
//! and length in place, and the runtime hashes and compares those. A list
//! with no such view takes the copy.
//!
//! A `get` is handed the element as a loan, which its call resolves. A
//! membership test is handed a temporary: MIR copies the element into it,
//! tests it, and releases it, in three instructions that follow each
//! other. [`str_key_tests`] finds those, and generated code makes no copy
//! for them.

use super::*;
use crate::mir::{terminator_place_mentions, MirArg};

/// Where the key of a membership test is read from.
#[derive(Clone, Debug, PartialEq)]
pub(super) enum StrKeySource {
    /// `list[index]`, with the line and column an index error names.
    Element {
        list: String,
        index: Operand,
        line: i64,
        column: i64,
    },
    /// A copy read out of this loan.
    Loan(String),
}

/// One membership test on a `str` temporary that is copied from a list
/// just before the test and released just after it.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct StrKeyTest {
    pub(super) source: StrKeySource,
    /// The local the test's result goes to.
    pub(super) test: String,
    /// The dict or set searched.
    pub(super) receiver: String,
    pub(super) set: bool,
}

fn is_temporary(place: &str) -> bool {
    place.starts_with("%t") && !place.contains('.')
}

fn key_source(instruction: &Instruction) -> Option<(&str, StrKeySource)> {
    match instruction {
        Instruction::ReadLoan { target, loan } => {
            Some((target.as_str(), StrKeySource::Loan(loan.clone())))
        }
        Instruction::Assign {
            target,
            value:
                Rvalue::Call {
                    callee:
                        CallTarget::Runtime {
                            op: RuntimeOp::List(ListOp::Index),
                            receiver: Some(Operand::Place(list)),
                            ..
                        },
                    args,
                    ..
                },
        } => {
            let [index, MirArg {
                value: Operand::Int(line),
                ..
            }, MirArg {
                value: Operand::Int(column),
                ..
            }] = args.as_slice()
            else {
                return None;
            };
            let plain_index = index.name.is_none()
                && index.writeback_place.is_none()
                && !matches!(&index.value, Operand::MovePlace(_));
            (plain_index && !list.contains('.')).then(|| {
                (
                    target.as_str(),
                    StrKeySource::Element {
                        list: list.clone(),
                        index: index.value.clone(),
                        line: i64::try_from(*line).unwrap_or(0),
                        column: i64::try_from(*column).unwrap_or(0),
                    },
                )
            })
        }
        _ => None,
    }
}

/// The membership tests of `function` on a `str` temporary copied from a
/// list for the test alone, by the temporary. The copy, the test, and the
/// release follow each other in one block, and nothing else names the
/// temporary. The receiver is a local, not a loan or a field.
pub(super) fn str_key_tests(function: &MirFunction) -> HashMap<String, StrKeyTest> {
    let mut named = HashMap::<&str, usize>::new();
    for block in &function.blocks {
        for instruction in &block.instructions {
            for place in super::entry_loops::mentioned_roots(instruction) {
                *named.entry(place).or_default() += 1;
            }
        }
        for place in terminator_place_mentions(&block.terminator) {
            // A temporary a terminator names is used beyond the test.
            *named
                .entry(place.split('.').next().unwrap_or(place))
                .or_default() += 4;
        }
    }
    let mut tests = HashMap::new();
    for block in &function.blocks {
        for window in block.instructions.windows(3) {
            let Some((key, source)) = key_source(&window[0]) else {
                continue;
            };
            let Instruction::Assign {
                target: test,
                value:
                    Rvalue::Call {
                        callee:
                            CallTarget::Runtime {
                                op,
                                receiver: Some(Operand::Place(receiver)),
                                ..
                            },
                        args,
                        ..
                    },
            } = &window[1]
            else {
                continue;
            };
            let set = match op {
                RuntimeOp::Dict(DictOp::ContainsKey) => false,
                RuntimeOp::Set(SetOp::Contains) => true,
                _ => continue,
            };
            let [MirArg {
                name: None,
                value: Operand::Place(tested),
                writeback_place: None,
            }] = args.as_slice()
            else {
                continue;
            };
            let released = matches!(&window[2], Instruction::Release { place } if place == key);
            if released
                && tested == key
                && is_temporary(key)
                && named.get(key) == Some(&3)
                && !receiver.contains('.')
                && !receiver.starts_with('%')
                && !test.contains('.')
                && test != key
            {
                tests.insert(
                    key.to_string(),
                    StrKeyTest {
                        source,
                        test: test.clone(),
                        receiver: receiver.clone(),
                        set,
                    },
                );
            }
        }
    }
    tests
}

/// The words of a `str` element generated code looks up in place: the
/// list, the position, and the line and column an index error names.
pub(super) type LentStrKey = [Val; 4];

impl FunctionCompiler<'_> {
    /// The lent `list[str]` element `operand` names, when it is one.
    pub(super) fn lent_str_key(
        &mut self,
        operand: &Operand,
    ) -> std::result::Result<Option<LentStrKey>, String> {
        let Some(selection) = self.borrowed_list_element(operand).filter(|selection| {
            matches!(
                &selection.element_type,
                Type::Named(name, args) if name == "str" && args.is_empty()
            )
        }) else {
            return Ok(None);
        };
        let (list, index) = self.load_borrowed_list_element(&selection)?;
        let (line, column) = self.span_operands(selection.span);
        Ok(Some([list, index, line, column]))
    }

    /// Begins a test in `str_key_tests`: reads where the key lies instead
    /// of copying it. Returns whether `key` is such a key.
    pub(super) fn begin_str_key_test(&mut self, key: &str) -> std::result::Result<bool, String> {
        let Some(test) = self.str_key_tests.get(key).cloned() else {
            return Ok(false);
        };
        let words = match &test.source {
            StrKeySource::Loan(loan) => match self.lent_str_key(&Operand::Place(loan.clone()))? {
                Some(words) => words,
                None => return Ok(false),
            },
            StrKeySource::Element {
                list,
                index,
                line,
                column,
            } => {
                let list = self.load_root(list)?.values[0];
                let int64 = DirectType::Scalar(ScalarKind::Int64);
                let loaded = self.load_operand_for_target(index, &int64)?;
                let index = self.coerce_value(loaded, &int64)?.values[0];
                let line = self.builder.iconst(types::I64, *line);
                let column = self.builder.iconst(types::I64, *column);
                [list, index, line, column]
            }
        };
        self.pending_str_key = Some((key.to_string(), words));
        Ok(true)
    }

    /// The test of the key `begin_str_key_test` read. Returns whether
    /// `target` is the result of one.
    pub(super) fn compile_str_key_test(
        &mut self,
        target: &str,
    ) -> std::result::Result<bool, String> {
        let Some((key, words)) = self.pending_str_key.clone() else {
            return Ok(false);
        };
        let Some(test) = self.str_key_tests.get(&key).cloned() else {
            return Ok(false);
        };
        if test.test != target {
            return Err(format!(
                "direct backend read the `str` key `{key}` in place for a test that did not follow"
            ));
        }
        let receiver = self.load_root(&test.receiver)?.values[0];
        let (in_place, boxed) = if test.set {
            (RuntimeFn::SetContainsStr, RuntimeFn::SetContains)
        } else {
            (RuntimeFn::MapContainsStr, RuntimeFn::MapContainsKey)
        };
        let found = self.call_with_lent_str_key(
            words,
            receiver,
            self.runtime[in_place],
            self.runtime[boxed],
        );
        let target_ty = self.type_of_place(target)?;
        let found = ValueRef {
            values: vec![found],
            ty: DirectType::Scalar(ScalarKind::Bool),
        };
        let coerced = self.coerce_value(found, &target_ty)?;
        self.store_place(target, coerced)?;
        Ok(true)
    }

    /// The result of a lookup in `receiver` under the lent `str` element
    /// `key`. `in_place` takes the receiver, the key's bytes and their
    /// length. `boxed` takes the receiver and a box, and is called with a
    /// copy of the element when the list has no `str` view or the position
    /// is outside it.
    pub(super) fn call_with_lent_str_key(
        &mut self,
        [list, index, line, column]: LentStrKey,
        receiver: Val,
        in_place: FuncRef,
        boxed: FuncRef,
    ) -> Val {
        use aura_runtime::native_runtime::{
            DirectListAccess, DIRECT_STR_VIEW_ENTRY_BYTES, DIRECT_STR_VIEW_LEN_OFFSET,
            DIRECT_STR_VIEW_PTR_OFFSET,
        };
        let layout = aura_runtime::native_runtime::direct_list_layout();
        let slow_block = self.builder.create_block();
        let fast_block = self.builder.create_block();
        let join_block = self.builder.create_block();
        self.builder.append_block_param(join_block, types::I64);
        self.emit_list_access_check(list, DirectListAccess::Str, &layout, slow_block, fast_block);

        self.builder.switch_to_block(fast_block);
        self.builder.seal_block(fast_block);
        let flags = MemFlags::trusted();
        let entries = self
            .builder
            .load(types::I64, flags, list, layout.elements_offset);
        let position = self.emit_list_view_bounds_check(list, index, &layout, slow_block);
        let offset = self
            .builder
            .imul_imm(position, i64::from(DIRECT_STR_VIEW_ENTRY_BYTES));
        let entry = self.builder.iadd(entries, offset);
        let bytes = self
            .builder
            .load(types::I64, flags, entry, DIRECT_STR_VIEW_PTR_OFFSET);
        let len = self
            .builder
            .load(types::I64, flags, entry, DIRECT_STR_VIEW_LEN_OFFSET);
        let found = self.builder.call(in_place, &[receiver, bytes, len]);
        self.builder.jump(join_block, &[found.results[0]]);

        self.builder.switch_to_block(slow_block);
        self.builder.seal_block(slow_block);
        let copy = self.builder.call(
            self.runtime[RuntimeFn::VecStrElement],
            &[list, index, line, column],
        );
        let copy = copy.results[0];
        let found = self.builder.call(boxed, &[receiver, copy]);
        self.release_opaque_handle(copy);
        self.builder.jump(join_block, &[found.results[0]]);

        self.builder.switch_to_block(join_block);
        self.builder.seal_block(join_block);
        self.builder.block_params(join_block)[0]
    }
}

#[cfg(test)]
#[path = "native_codegen_str_keys_tests.rs"]
mod tests;
