//! Direct calls for the `sync` module's host functions: an atomic step
//! passes its integers as machine words, and a lock moves the guarded
//! value's box without an argument buffer.

use super::*;

/// The `aura_direct_sync_atom` operation a `sync` host function names, and
/// whether its result is a `bool`, an `int64`, or nothing.
fn atom_operation(name: &str) -> Option<(i64, Option<ScalarKind>)> {
    Some(match name {
        "sync::__aura_atom_load" => (0, Some(ScalarKind::Int64)),
        "sync::__aura_atom_store" => (1, None),
        "sync::__aura_atom_add" => (2, Some(ScalarKind::Int64)),
        "sync::__aura_atom_swap" => (3, Some(ScalarKind::Int64)),
        "sync::__aura_atom_compare_and_set" => (4, Some(ScalarKind::Bool)),
        _ => return None,
    })
}

/// The one field of a class that wraps a `sync` handle, such as
/// `sync.Mutex[T]` or `sync.AtomicInt`, when `ty` is such a class.
///
/// Generated code keeps an instance of such a class as the handle's own
/// box: reading the field is free, and a call on the instance passes one
/// word, as a call on a `Queue` does. An instance in a map of fields
/// would cost a lookup by name and a copy for every call.
pub(super) fn handle_class_field<'a>(
    ty: &DirectType,
    classes: &'a HashMap<String, MirClass>,
) -> Option<&'a crate::mir::MirClassField> {
    let DirectType::Opaque(Type::Named(name, _)) = ty else {
        return None;
    };
    handle_class_field_by_name(name, classes)
}

pub(super) fn handle_class_field_by_name<'a>(
    name: &str,
    classes: &'a HashMap<String, MirClass>,
) -> Option<&'a crate::mir::MirClassField> {
    if !crate::runtime_value::sync_host::is_handle_class(name) {
        return None;
    }
    let [field] = classes.get(name)?.fields.as_slice() else {
        return None;
    };
    matches!(
        &field.ty,
        Type::Named(handle, args)
            if args.is_empty() && matches!(handle.as_str(), "sync.Cell" | "sync.Atom")
    )
    .then_some(field)
}

/// The code `aura_direct_sync_cell_take_word` knows a guarded value of
/// direct type `ty` by, when the value passes as one word.
fn word_kind(ty: &DirectType) -> Option<i64> {
    match ty {
        DirectType::Scalar(ScalarKind::Int64) => Some(0),
        DirectType::Scalar(ScalarKind::Bool) => Some(1),
        _ => None,
    }
}

impl FunctionCompiler<'_> {
    pub(super) fn map_lookup_word(&mut self, place: &str) -> Option<ValueRef> {
        let address = *self.map_word_slots.get(place)?;
        let word = self.builder.load(types::I64, MemFlags::new(), address, 0);
        Some(ValueRef {
            values: vec![word],
            ty: DirectType::Scalar(ScalarKind::Int64),
        })
    }

    /// The checked match reads its own copied payload. No pointer into the
    /// shared map escapes the lock, and a stored zero keeps its presence bit.
    pub(super) fn compile_map_word_lookup(
        &mut self,
        value: &Rvalue,
        address: Val,
    ) -> std::result::Result<(), String> {
        let present = if let Rvalue::Use(Operand::MovePlace(source)) = value {
            let source = self.map_word_slots[source];
            let word = self.builder.load(types::I64, MemFlags::new(), source, 0);
            self.builder.store(MemFlags::new(), word, address, 0);
            self.builder.load(types::I64, MemFlags::new(), source, 8)
        } else {
            let (receiver, key) = lookups::map_word_get(value)
                .ok_or_else(|| "direct backend expected an integer map lookup".to_owned())?;
            let handle = match receiver {
                Operand::Place(place)
                    if self.moved_operands.contains(&std::ptr::from_ref(receiver)) =>
                {
                    self.take_place(place)?
                }
                receiver => self.load_operand(receiver)?,
            };
            let key_ty = DirectType::Scalar(ScalarKind::Int64);
            let key = self.load_operand_for_target(&key.value, &key_ty)?;
            let key = self.coerce_value(key, &key_ty)?;
            self.builder
                .call(
                    self.runtime[RuntimeFn::SyncMapGetWord],
                    &[handle.values[0], key.values[0], address],
                )
                .results[0]
        };
        self.builder.store(MemFlags::new(), present, address, 8);
        Ok(())
    }

    /// A `dict` `get` for a slot that keeps a found int64 as a word: the
    /// lookup writes the int64 to the slot and says whether it found one.
    /// `None` when no such slot asks, and the `get` compiles as usual.
    pub(super) fn dict_lookup_words(
        &mut self,
        lookup: RuntimeFn,
        words: &[Val],
    ) -> Option<ValueRef> {
        let address = self.lookup_word.take()?;
        let mut words = words.to_vec();
        words.push(address);
        let found = self.builder.call(self.runtime[lookup], &words).results[0];
        self.lookup_word_found = Some(found);
        // The slot's own word holds nothing.
        Some(ValueRef {
            values: vec![self.builder.iconst(types::I64, 0)],
            ty: DirectType::Opaque(Type::named("Unknown")),
        })
    }

    /// Assigns a `dict` `get` to `target`, a slot that keeps a found int64
    /// at `address` and whether it was found beside it. A `get` that
    /// compiled to a path that makes the found value's box, or the
    /// `Lookup`, reads the int64 out of it.
    pub(super) fn compile_dict_word_lookup(
        &mut self,
        target: &str,
        value: &Rvalue,
        address: Val,
    ) -> std::result::Result<(), String> {
        let target_ty = self.type_of_place(target)?;
        self.lookup_payload = Some(false);
        self.lookup_word = Some(address);
        let compiled = self.compile_rvalue_for_target(value, &target_ty);
        self.lookup_word = None;
        let payload_form = self.lookup_payload.take() == Some(true);
        let found = self.lookup_word_found.take();
        let compiled = compiled?;
        if let Some(found) = found {
            self.builder.store(MemFlags::new(), found, address, 8);
            return Ok(());
        }
        let compiled = self.ensure_opaque(compiled)?;
        let payload = if payload_form {
            self.transfer_opaque_arg(&compiled)
        } else {
            let lookup = self.transfer_owned_opaque_value(&compiled);
            self.builder
                .call(self.runtime[RuntimeFn::LookupPayload], &[lookup])
                .results[0]
        };
        // The branch that follows leaves nothing for only one of its sides
        // to release.
        self.release_all_temporary_owned();
        let found = self.builder.icmp_imm(IntCC::NotEqual, payload, 0);
        let found = self.builder.uextend(types::I64, found);
        self.builder.store(MemFlags::new(), found, address, 8);
        let found_block = self.builder.create_block();
        let done_block = self.builder.create_block();
        self.builder
            .brif(payload, found_block, &[], done_block, &[]);
        self.builder.switch_to_block(found_block);
        self.builder.seal_block(found_block);
        let boxed = self.owned_opaque_result(vec![payload], Type::named("Unknown"));
        let word = self.coerce_value(boxed, &DirectType::Scalar(ScalarKind::Int64))?;
        self.builder
            .store(MemFlags::new(), word.values[0], address, 0);
        self.release_all_temporary_owned();
        self.builder.jump(done_block, &[]);
        self.builder.switch_to_block(done_block);
        self.builder.seal_block(done_block);
        Ok(())
    }

    /// A new instance of a class that wraps a `sync` handle: the handle's
    /// box. An initializer starts from an instance with no fields, which
    /// takes the ordinary path until the initializer stores the handle.
    pub(super) fn construct_handle_class(
        &mut self,
        fields: &[crate::mir::MirFieldInit],
        target_ty: &Type,
    ) -> std::result::Result<Option<ValueRef>, String> {
        let target = DirectType::Opaque(target_ty.clone());
        let (Some(handle), [field]) = (handle_class_field(&target, &self.classes), fields) else {
            return Ok(None);
        };
        let handle_ty = DirectType::Opaque(handle.ty.clone());
        let loaded = self.load_operand_for_target(&field.value, &handle_ty)?;
        let loaded = self.ensure_opaque(loaded)?;
        // A handle never changes in place, so the instance shares the
        // handle's box instead of copying it.
        let word = self.transfer_opaque_arg(&loaded);
        Ok(Some(
            self.owned_opaque_result(vec![word], target_ty.clone()),
        ))
    }

    /// The type of `root` when it is an instance of a class that wraps a
    /// `sync` handle and `segments` names that handle's field.
    pub(super) fn handle_class_store(&self, root: &str, segments: &[&str]) -> Option<DirectType> {
        let root_ty = self.variable_types.get(root)?;
        let handle = handle_class_field(root_ty, &self.classes)?;
        (segments == [handle.name.as_str()]).then(|| root_ty.clone())
    }

    /// A call of a `sync` atomic or lock function as one direct runtime
    /// call, or `None` for any other host builtin.
    pub(super) fn compile_sync_access(
        &mut self,
        builtin: HostBuiltin,
        args: &[MirArg],
        target: &DirectType,
    ) -> std::result::Result<Option<ValueRef>, String> {
        let name = builtin.name();
        if let Some((operation, result)) = atom_operation(name) {
            let handle = self.load_operand(&args[0].value)?;
            let handle = self.ensure_opaque(handle)?;
            let mut words = vec![handle.values[0], self.builder.iconst(types::I64, operation)];
            for argument in &args[1..] {
                let value =
                    self.load_operand_with_integer_hint(&argument.value, Some(ScalarKind::Int64))?;
                let value = self.coerce_value(value, &DirectType::Scalar(ScalarKind::Int64))?;
                words.push(value.values[0]);
            }
            while words.len() < 4 {
                words.push(self.builder.iconst(types::I64, 0));
            }
            let results = self.runtime_call_results(self.runtime[RuntimeFn::SyncAtom], &words);
            return Ok(Some(match result {
                Some(kind) => ValueRef {
                    values: results,
                    ty: DirectType::Scalar(kind),
                },
                None => unit_value(&mut *self.builder),
            }));
        }
        match name {
            "sync::__aura_map_get" => {
                let (Operand::Place(place) | Operand::MovePlace(place), DirectType::Opaque(result)) =
                    (&args[1].value, target)
                else {
                    return Ok(None);
                };
                if self.type_of_place(place)? != DirectType::Scalar(ScalarKind::Int64) {
                    return Ok(None);
                }
                let key = self.load_operand(&args[1].value)?;
                let handle = self.load_operand(&args[0].value)?;
                let handle = self.ensure_opaque(handle)?;
                let words = [handle.values[0], key.values[0]];
                let results =
                    self.runtime_call_results(self.runtime[RuntimeFn::SyncMapGetInt64], &words);
                Ok(Some(self.owned_opaque_result(results, result.clone())))
            }
            "sync::__aura_cell_take" => {
                let handle = self.load_operand(&args[0].value)?;
                let handle = self.ensure_opaque(handle)?;
                if let Some(kind) = word_kind(target) {
                    let kind = self.builder.iconst(types::I64, kind);
                    let results = self.runtime_call_results(
                        self.runtime[RuntimeFn::SyncCellTakeWord],
                        &[handle.values[0], kind],
                    );
                    return Ok(Some(ValueRef {
                        values: results,
                        ty: target.clone(),
                    }));
                }
                let results = self
                    .runtime_call_results(self.runtime[RuntimeFn::SyncCellTake], &handle.values);
                let semantic_type = RuntimeOp::Host(builtin)
                    .result_type(None, &[])
                    .expect("a host builtin names its result type");
                let result = self.owned_opaque_result(results, semantic_type.clone());
                let target =
                    ensure_direct_type(&semantic_type, &self.classes, "the value of a lock")?;
                Ok(Some(self.coerce_value(result, &target)?))
            }
            "sync::__aura_cell_put" => {
                let handle = self.load_operand(&args[0].value)?;
                let handle = self.ensure_opaque(handle)?;
                let value = self.load_operand(&args[1].value)?;
                if let Some(kind) = word_kind(&value.ty) {
                    let kind = self.builder.iconst(types::I64, kind);
                    self.builder.call(
                        self.runtime[RuntimeFn::SyncCellPutWord],
                        &[handle.values[0], kind, value.values[0]],
                    );
                    return Ok(Some(unit_value(&mut *self.builder)));
                }
                let value = self.ensure_opaque(value)?;
                // The cell takes the value's box.
                let value = self.transfer_owned_opaque_value(&value);
                self.builder.call(
                    self.runtime[RuntimeFn::SyncCellPut],
                    &[handle.values[0], value],
                );
                Ok(Some(unit_value(&mut *self.builder)))
            }
            _ => Ok(None),
        }
    }
}
