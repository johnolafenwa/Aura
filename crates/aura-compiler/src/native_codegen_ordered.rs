//! The int64 operations of concrete standard collection classes. A class
//! field, temporary receiver, or unspecialized method keeps its Aura body.

use super::*;
use aura_runtime::runtime_value::ordered_collections::I64Op;

/// A local whose only uses are standard push/pop operations and release can
/// keep the address of its packed storage. The opaque receiver owns the
/// storage for the entire interval. A projection, copy, capture, loan,
/// writeback, argument, return, or call to any other method ends this proof.
pub(super) fn word_local_roots(
    function: &MirFunction,
    shapes: &shapes::ProgramShapes,
) -> Vec<String> {
    let mut roots = Vec::new();
    for block in &function.blocks {
        for pair in block.instructions.windows(2) {
            let [Instruction::Assign {
                target: temp,
                value:
                    Rvalue::Call {
                        callee: CallTarget::Name(init),
                        args,
                        ..
                    },
            }, bind @ Instruction::Assign {
                target: local,
                value: Rvalue::Use(Operand::MovePlace(source)),
            }] = pair
            else {
                continue;
            };
            let sequence = match init.as_str() {
                "collections::Deque.__init__[int64]" => "collections.Deque",
                "collections::Heap.__init__[int64]" | "collections::Heap.max[int64]" => {
                    "collections.Heap"
                }
                _ => continue,
            };
            if source != temp
                || !temp.starts_with('%')
                || !args.is_empty()
                || !shapes.is_standard_collection_method(init)
                || local.contains('.')
                || local.starts_with('%')
                || local == "self"
                || function.params.iter().any(|param| &param.name == local)
            {
                continue;
            }
            let expected = Type::Named(sequence.to_string(), vec![Type::named("int64")]);
            if !function
                .local_types
                .iter()
                .any(|decl| &decl.name == local && decl.ty == expected)
                || revisits_initializer(function, block)
            {
                continue;
            }
            if is_word_local(function, local, bind, shapes) {
                roots.push(local.clone());
            }
        }
    }
    roots
}

/// Each cached address belongs to one constructor invocation in this frame.
fn revisits_initializer(function: &MirFunction, initial: &BasicBlock) -> bool {
    let mut successors = direct_terminator_successors(&initial.terminator);
    let mut visited = HashSet::new();
    while let Some(label) = successors.pop() {
        if label == initial.label {
            return true;
        }
        if visited.insert(label) {
            let Some(block) = function.blocks.iter().find(|block| block.label == label) else {
                return true;
            };
            successors.extend(direct_terminator_successors(&block.terminator));
        }
    }
    false
}

fn is_word_local(
    function: &MirFunction,
    local: &str,
    bind: &Instruction,
    shapes: &shapes::ProgramShapes,
) -> bool {
    let touches = |place: &str| place.split('.').next() == Some(local);
    let mut found_call = false;
    let mut mentions = Vec::new();
    for block in &function.blocks {
        if crate::mir::terminator_place_mentions(&block.terminator)
            .into_iter()
            .any(touches)
        {
            return false;
        }
        for instruction in &block.instructions {
            if std::ptr::eq(instruction, bind) {
                continue;
            }
            if matches!(
                instruction,
                Instruction::SourceLocation { .. } | Instruction::SourceName { .. }
            ) || matches!(instruction, Instruction::Assign { target, .. } if touches(target))
            {
                return false;
            }
            if matches!(instruction, Instruction::Release { place } if place == local) {
                continue;
            }
            mentions.clear();
            crate::mir::instruction_place_mentions(instruction, &mut mentions);
            let count = mentions.iter().filter(|place| touches(place)).count();
            if count == 0 {
                continue;
            }
            // Both mentions must be the receiver and its writeback. The
            // arguments cannot alias it even through a projected place.
            let Instruction::Assign {
                value:
                    Rvalue::Call {
                        callee:
                            CallTarget::Member {
                                object: Operand::Place(object),
                                receiver_place: Some(writeback),
                                dispatch: Some(crate::mir::MethodDispatch::Static { function }),
                                ..
                            },
                        args,
                        ..
                    },
                ..
            } = instruction
            else {
                return false;
            };
            if count != 2
                || object != local
                || writeback != local
                || !shapes.is_standard_collection_method(function)
                || !I64Op::for_method(function).is_some_and(I64Op::is_word_sequence)
                || args
                    .iter()
                    .any(|arg| arg.name.is_some() || arg.writeback_place.is_some())
            {
                return false;
            }
            found_call = true;
        }
    }
    found_call
}

impl FunctionCompiler<'_> {
    pub(super) fn compile_ordered_i64_method(
        &mut self,
        function: &str,
        place: &str,
        args: &[MirArg],
    ) -> Result<Option<ValueRef>, String> {
        let Some(op) = I64Op::for_method(function) else {
            return Ok(None);
        };
        let Some(DirectType::Opaque(Type::Named(_, types))) = self.variable_types.get(place) else {
            return Ok(None);
        };
        let int = Type::named("int64");
        let Some(method_span) = self.program_shapes.collection_method_span(function) else {
            return Ok(None);
        };
        if types.is_empty()
            || !types.iter().all(|ty| ty == &int)
            || self.view_places.contains_key(place)
            || self.loan_locals.contains(place)
            || args
                .iter()
                .any(|arg| arg.name.is_some() || arg.writeback_place.is_some())
        {
            return Ok(None);
        }

        // This entry edits a live receiver in place. Read the local before
        // the ordinary method ABI takes it and later writes it back.
        let local_words = self.ordered_word_views.get(place).copied();
        let receiver = if local_words.is_some() {
            let vars = self.local_vars(place)?;
            self.builder.use_var(vars[0])
        } else {
            self.load_place(place)?.values[0]
        };
        let zero = self.builder.iconst(types::I64, 0);
        let code = self.builder.iconst(types::I64, op as i64);
        let mut words = [receiver, code, zero, zero];
        if op.returns_number() {
            // A pop takes no source argument. Its unused word carries the
            // method's definition so a failure reproduces its Aura frame.
            words[2] = self
                .builder
                .iconst(types::I64, I64Op::pop_site(method_span));
        }
        for (word, arg) in words[2..].iter_mut().zip(args) {
            let value = self.load_operand_with_integer_hint(&arg.value, Some(ScalarKind::Int64))?;
            let value = self.coerce_value(value, &DirectType::Scalar(ScalarKind::Int64))?;
            *word = value.values[0];
        }
        let value = if op.returns_lookup() {
            let call = self
                .builder
                .call(self.runtime[RuntimeFn::OrderedI64Lookup], &words);
            self.owned_opaque_result(call.results, Type::Named("Lookup".to_string(), vec![int]))
        } else {
            let results = if let Some(cache) = local_words {
                vec![self.emit_ordered_local_step(cache, &words)]
            } else {
                self.builder
                    .call(self.runtime[RuntimeFn::OrderedI64], &words)
                    .results
            };
            if op.returns_number() {
                ValueRef {
                    values: results,
                    ty: DirectType::Scalar(ScalarKind::Int64),
                }
            } else if op.returns_bool() {
                ValueRef {
                    values: results,
                    ty: DirectType::Scalar(ScalarKind::Bool),
                }
            } else {
                unit_value(&mut *self.builder)
            }
        };
        Ok(Some(value))
    }

    fn emit_ordered_local_step(&mut self, cache: Var, words: &[Val; 4]) -> Val {
        let [receiver, code, word, _] = *words;
        let open = self.builder.create_block();
        let step = self.builder.create_block();
        let fallback = self.builder.create_block();
        let join = self.builder.create_block();
        self.builder.append_block_param(step, types::I64);
        self.builder.append_block_param(join, types::I64);
        let cached = self.builder.use_var(cache);
        let valid = self.builder.icmp_imm(IntCC::NotEqual, cached, 0);
        self.builder.brif(valid, step, &[cached], open, &[]);

        self.builder.switch_to_block(open);
        self.builder.seal_block(open);
        let address = self
            .builder
            .call(
                self.runtime[RuntimeFn::OrderedLocalWords],
                &[receiver, code],
            )
            .results[0];
        self.builder.def_var(cache, address);
        let valid = self.builder.icmp_imm(IntCC::NotEqual, address, 0);
        self.builder.brif(valid, step, &[address], fallback, &[]);

        self.builder.switch_to_block(step);
        self.builder.seal_block(step);
        let address = self.builder.block_params(step)[0];
        let result = self
            .builder
            .call(
                self.runtime[RuntimeFn::OrderedLocalStep],
                &[address, code, word],
            )
            .results[0];
        self.builder.jump(join, &[result]);

        self.builder.switch_to_block(fallback);
        self.builder.seal_block(fallback);
        let result = self
            .builder
            .call(self.runtime[RuntimeFn::OrderedI64], words)
            .results[0];
        self.builder.jump(join, &[result]);
        self.builder.switch_to_block(join);
        self.builder.seal_block(join);
        self.builder.block_params(join)[0]
    }
}
