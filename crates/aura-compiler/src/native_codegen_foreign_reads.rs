//! Numeric member reads from the private C array that a mutable union-array
//! call just wrote back. Only a list whose element loans exclusively read
//! such members can reuse the bytes. The ordinary list is still published.

use std::collections::BTreeMap;

use super::*;
use crate::ffi::FfiScalar;
use crate::mir::ownership::instruction_accesses;
use crate::mir::{instruction_place_mentions, terminator_place_mentions};

/// The exact body of a numeric union accessor. Returning a computed value
/// after a reinterpretation is not a projection of the C bytes.
pub(super) fn numeric_projection(function: &MirFunction) -> Option<(String, usize, FfiScalar)> {
    if function.receiver != Some(MirReceiverKind::Borrow) || !function.params.is_empty() {
        return None;
    }
    let [block] = function.blocks.as_slice() else {
        return None;
    };
    let [first @ Instruction::Assign {
        target: boxed,
        value: Rvalue::Call {
            callee: CallTarget::Extern(call),
            ..
        },
    }, Instruction::Assign {
        target: result,
        value:
            Rvalue::Member {
                object: Operand::Place(object),
                field,
            },
    }] = block.instructions.as_slice()
    else {
        return None;
    };
    if !reads_numeric_c_union_member(first)
        || boxed != object
        || !matches!(&block.terminator, Terminator::Return(Operand::Place(value)) if value == result)
    {
        return None;
    }
    let member = &call.c_struct_result.as_ref()?.fields[0];
    let crate::c_layout::CFieldKind::Scalar(scalar) = member.kind else {
        return None;
    };
    let union = call.params[0].c_struct.as_ref()?;
    (member.name == *field).then(|| (union.name.clone(), union.size(), scalar))
}

struct Read<'a> {
    list: &'a str,
    loan: &'a str,
    temp: &'a str,
    index: &'a Operand,
    target: &'a str,
    function: &'a str,
}

fn read(instructions: &[Instruction]) -> Option<Read<'_>> {
    let [Instruction::BeginElementLoan {
        loan,
        source,
        selector,
        projection,
        mutable: false,
        ..
    }, Instruction::Lend {
        place,
        temp,
        mutable: false,
    }, Instruction::Assign {
        target,
        value:
            Rvalue::Call {
                callee:
                    CallTarget::Member {
                        object: Operand::Place(receiver),
                        receiver_place: Some(back),
                        dispatch: Some(MethodDispatch::Static { function }),
                        ..
                    },
                args,
                ..
            },
    }, Instruction::EndLend {
        place: ended,
        temp: ended_temp,
        mutable: false,
    }, ..] = instructions
    else {
        return None;
    };
    (projection.is_empty()
        && loan == place
        && place == ended
        && temp == receiver
        && temp == back
        && temp == ended_temp
        && args.is_empty())
    .then_some(Read {
        list: source,
        loan,
        temp,
        index: selector,
        target,
        function,
    })
}

fn is_root(place: &str, name: &str) -> bool {
    place.split(['.', '[']).next() == Some(name)
}

/// The single mutable C argument that both consumes and writes back this
/// particular local, with no second operand naming it.
fn fill<'a>(inst: &'a Instruction, list: &str) -> Option<&'a crate::c_layout::CStruct> {
    let Instruction::Assign {
        value:
            Rvalue::Call {
                callee: CallTarget::Extern(call),
                args,
                ..
            },
        ..
    } = inst
    else {
        return None;
    };
    let mut found = None;
    for (arg, param) in args.iter().zip(&call.params) {
        if matches!(&arg.value, Operand::Place(place) if place == list)
            && arg.writeback_place.as_deref() == Some(list)
            && param.passing == MirReceiverKind::BorrowMut
            && param.pointer
            && found.is_none()
        {
            found = Some(param.c_struct.as_ref().filter(|layout| layout.union)?);
        } else if matches!(&arg.value, Operand::Place(place) | Operand::MovePlace(place) if is_root(place, list))
            || arg
                .writeback_place
                .as_ref()
                .is_some_and(|p| is_root(p, list))
        {
            return None;
        }
    }
    found
}

/// A list may be built or replaced, filled by C, released, or read by a
/// numeric getter. An escaping or mutable loan rules out the whole list,
/// including writes reached through a field of that loan.
fn plan(
    function: &MirFunction,
    projections: &HashMap<String, (String, usize, FfiScalar)>,
) -> BTreeMap<String, usize> {
    let mut candidates = BTreeMap::new();
    for block in &function.blocks {
        for inst in &block.instructions {
            let Instruction::Assign {
                value:
                    Rvalue::Call {
                        callee: CallTarget::Extern(call),
                        args,
                        ..
                    },
                ..
            } = inst
            else {
                continue;
            };
            for (arg, param) in args.iter().zip(&call.params) {
                let Some(list) = &arg.writeback_place else {
                    continue;
                };
                if param.pointer
                    && param.c_struct.as_ref().is_some_and(|layout| layout.union)
                    && !list.contains(['.', '['])
                {
                    if let Some(layout) = fill(inst, list) {
                        candidates.insert(list.clone(), (layout.name.clone(), layout.size()));
                    }
                }
            }
        }
    }
    candidates
        .into_iter()
        .filter_map(|(list, (class, size))| {
            only_reads_after_fill(function, &list, &class, size, projections)
                .then_some((list, size))
        })
        .collect()
}

fn only_reads_after_fill(
    function: &MirFunction,
    list: &str,
    class: &str,
    size: usize,
    projections: &HashMap<String, (String, usize, FfiScalar)>,
) -> bool {
    let eligible = |r: &Read<'_>| {
        r.list == list
            && projections
                .get(r.function)
                .is_some_and(|(name, width, _)| name == class && *width == size)
    };
    let mut loans = HashSet::new();
    let mut temps = HashSet::new();
    for block in &function.blocks {
        for i in 0..block.instructions.len() {
            if let Some(r) = read(&block.instructions[i..]).filter(&eligible) {
                loans.insert(r.loan);
                temps.insert(r.temp);
            }
        }
    }
    if loans.is_empty() {
        return false;
    }
    let lent = |place: &str| {
        let root = place.split(['.', '[']).next().unwrap_or(place);
        loans.contains(root) || temps.contains(root)
    };
    let protected = |place: &str| is_root(place, list) || lent(place);
    for block in &function.blocks {
        let mut remaining = &block.instructions[..];
        while let Some((inst, tail)) = remaining.split_first() {
            if let Some(r) = read(remaining).filter(&eligible) {
                if protected(r.target)
                    || matches!(r.index, Operand::Place(p) | Operand::MovePlace(p) if protected(p))
                {
                    return false;
                }
                remaining = &remaining[4..];
                continue;
            }
            remaining = tail;
            if matches!(inst, Instruction::EndLoan { loan } if loans.contains(loan.as_str()))
                || matches!(inst, Instruction::Release { place } if place == list)
            {
                continue;
            }
            let mut mentions = Vec::new();
            instruction_place_mentions(inst, &mut mentions);
            if let Some(layout) = fill(inst, list) {
                if layout.name != class || layout.size() != size || mentions.iter().any(|p| lent(p))
                {
                    return false;
                }
                continue;
            }
            // A fresh literal shares none of C's old output after writeback.
            if matches!(inst, Instruction::Assign { target, value: Rvalue::VecLiteral { .. } } if target == list)
            {
                if mentions.into_iter().any(protected) {
                    return false;
                }
                continue;
            }
            if mentions.into_iter().any(protected)
                || instruction_accesses(inst)
                    .into_iter()
                    .any(|(place, _)| protected(place))
                || matches!(inst, Instruction::Assign { target, .. } if protected(target))
            {
                return false;
            }
        }
        if terminator_place_mentions(&block.terminator)
            .into_iter()
            .any(protected)
        {
            return false;
        }
    }
    true
}

#[derive(Clone, Copy)]
pub(super) struct Buffer {
    pub(super) data: Var,
    pub(super) count: Var,
    stride: usize,
    pub(super) literal_count: Option<usize>,
}

impl FunctionCompiler<'_> {
    pub(super) fn prepare_union_read_buffers(&mut self, function: &MirFunction) {
        if self.program_shapes.numeric_union_projections.is_empty() {
            return;
        }
        for (root, stride) in plan(function, &self.program_shapes.numeric_union_projections) {
            let data = self.declare_word_var();
            let count = self.declare_word_var();
            let zero = self.builder.iconst(types::I64, 0);
            self.builder.def_var(data, zero);
            self.builder.def_var(count, zero);
            let mut literals = function
                .blocks
                .iter()
                .flat_map(|b| &b.instructions)
                .filter_map(|inst| match inst {
                    Instruction::Assign {
                        target,
                        value: Rvalue::VecLiteral { elements, .. },
                    } if target == &root => Some(elements.len()),
                    _ => None,
                });
            let literal_count = literals.next().filter(|first| {
                !function.params.iter().any(|p| p.name == root) && literals.all(|n| n == *first)
            });
            self.union_read_buffers.insert(
                root,
                Buffer {
                    data,
                    count,
                    stride,
                    literal_count,
                },
            );
        }
    }

    pub(super) fn release_union_buffer(&mut self, buffer: Buffer) {
        let old = self.builder.use_var(buffer.data);
        self.release_foreign_allocation(old, RuntimeFn::FfiReleaseStructs);
        let zero = self.builder.iconst(types::I64, 0);
        self.builder.def_var(buffer.data, zero);
        self.builder.def_var(buffer.count, zero);
    }

    /// A valid index reads the scalar from the private bytes. The cold
    /// branch runs the original instructions, including their source spans,
    /// for negative indices, missing entries, and reads before any fill.
    pub(super) fn compile_union_buffer_read(
        &mut self,
        block: &BasicBlock,
        pos: usize,
    ) -> std::result::Result<bool, String> {
        if self.union_read_buffers.is_empty() {
            return Ok(false);
        }
        if self.deferred_element_retain.is_some()
            || (!self.element_promotions.is_empty()
                && (pos..pos + 4).any(|i| {
                    self.element_promotions
                        .contains_key(&(block.label.clone(), i))
                }))
        {
            return Ok(false);
        }
        let Some(r) = read(&block.instructions[pos..]) else {
            return Ok(false);
        };
        let Some(buffer) = self.union_read_buffers.get(r.list).copied() else {
            return Ok(false);
        };
        let Some((_, _, scalar)) = self
            .program_shapes
            .numeric_union_projections
            .get(r.function)
        else {
            return Ok(false);
        };
        let Some(scalar) = foreign::foreign_scalar(scalar.ffi_type()) else {
            return Ok(false);
        };
        let index_ty = DirectType::Scalar(ScalarKind::Int64);
        let value = self.load_operand_for_target(r.index, &index_ty)?;
        let index = self.coerce_value(value, &index_ty)?.values[0];
        let count = self.builder.use_var(buffer.count);
        let valid = self.builder.icmp(IntCC::UnsignedLessThan, index, count);
        let fast = self.builder.create_block();
        let slow = self.builder.create_block();
        let done = self.builder.create_block();
        self.builder.set_cold_block(slow);
        self.builder.brif(valid, fast, &[], slow, &[]);
        self.builder.switch_to_block(fast);
        self.builder.seal_block(fast);
        let array = self.builder.use_var(buffer.data);
        let offset = self.builder.imul_imm(index, buffer.stride as i64);
        let ptr = self.builder.iadd(array, offset);
        let raw = self.builder.load(scalar.ty, MemFlags::new(), ptr, 0);
        let result = self.foreign_scalar_result(&scalar, raw, r.function)?;
        self.store_place(r.target, result)?;
        self.builder.jump(done, &[]);
        self.builder.switch_to_block(slow);
        self.builder.seal_block(slow);
        for (i, inst) in block.instructions[pos..pos + 4].iter().enumerate() {
            self.current_snapshot_instruction = pos + i;
            self.compile_instruction(&block.label, pos + i, inst)?;
        }
        self.builder.jump(done, &[]);
        self.builder.switch_to_block(done);
        self.builder.seal_block(done);
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_an_unescaped_union_list_can_read_the_private_writeback() {
        let source = r#"
extern "C" union Event:
    code: int32
    flags: uint32

extern "C" def fill(events: mut ptr[Event], count: int32) -> None

def read(events: mut list[Event], count: int32, index: int64) -> int32:
    fill(events, count)
    return events[index].code

def edit(events: mut list[Event], count: int32, index: int64) -> int32:
    fill(events, count)
    events[index].bytes.append(0)
    return events[index].code

def change(events: mut list[Event], index: int64):
    events[index].bytes = Event.from_code(90).bytes

def call_out(events: mut list[Event], count: int32, index: int64) -> int32:
    fill(events, count)
    change(events, index)
    return events[index].code
"#;
        let module = crate::parse_source(source).unwrap();
        let checked = crate::check_module_with_builtin_imports(module).unwrap();
        let module = crate::mir::lower(&checked);
        let getters = module
            .functions
            .iter()
            .filter_map(|f| numeric_projection(f).map(|projection| (f.name.clone(), projection)))
            .collect();
        let planned = |name: &str| {
            let function = module.functions.iter().find(|f| f.name == name).unwrap();
            plan(function, &getters).contains_key("events")
        };
        assert!(planned("read"));
        assert!(!planned("edit"));
        assert!(!planned("call_out"));
    }
}
