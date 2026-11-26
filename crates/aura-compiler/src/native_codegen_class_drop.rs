//! Native entry for a class hook. A mutable method returns its updated
//! receiver through the usual writeback ABI.

use super::*;

impl NativeCodegen<'_> {
    pub(super) fn define_drop_thunk(
        &mut self,
        function: &MirFunction,
    ) -> std::result::Result<(), String> {
        let id = self.function_thunks[&function.name];
        let target = self.functions[&function.name];
        self.define_body(
            id,
            &thunk_signature(),
            &function.name,
            |codegen, builder| {
                let entry = builder.create_block();
                builder.append_block_params_for_function_params(entry);
                builder.switch_to_block(entry);
                builder.seal_block(entry);
                let argv = builder.block_params(entry)[0];
                let receiver = builder.load(types::I64, MemFlags::new(), argv, 0);
                let keep = builder.import_function(codegen.runtime[RuntimeFn::RetainValue]);
                let receiver = builder.call(keep, &[receiver]).results[0];
                let target = builder.import_function(target);
                let result = builder.call(target, &[receiver]);
                release_direct_call_results(codegen, builder, &function.name, &result.results)?;
                let zero = builder.iconst(types::I64, 0);
                let unit = box_thunk_value(
                    codegen,
                    builder,
                    &[zero],
                    &DirectType::Scalar(ScalarKind::Unit),
                )?;
                builder.return_(&[unit]);
                Ok(())
            },
        )
    }
}

impl FunctionCompiler<'_> {
    pub(super) fn arm_drop_hook(
        &mut self,
        place: &str,
        function: &str,
    ) -> std::result::Result<(), String> {
        let owner = self.load_root(place)?;
        let thunk = self
            .function_thunk_refs
            .get(function)
            .ok_or_else(|| format!("missing drop hook `{function}`"))?;
        let thunk = self.builder.func_addr(types::I64, *thunk);
        let id = declare_runtime_function(
            self.builder.module(),
            "aura_direct_arm_class_drop",
            &[types::I64, types::I64],
            None,
        )?;
        let arm = self.builder.import_function(id);
        self.builder.call(arm, &[owner.values[0], thunk]);
        Ok(())
    }

    pub(super) fn drain_drop_hooks(&mut self) -> std::result::Result<(), String> {
        let id = declare_runtime_function(
            self.builder.module(),
            "aura_direct_drain_class_drops",
            &[],
            None,
        )?;
        let drain = self.builder.import_function(id);
        self.builder.call(drain, &[]);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn class_drop_stamps_a_decorated_callable_without_leaving_temporaries() {
        let module = crate::lower_source_to_mir(include_str!(
            "../tests/fixtures/run-pass/class_items_together.au"
        ))
        .unwrap();
        super::emit_host_object(&module).expect("an owned inline callable can enter its body");
    }

    #[test]
    fn class_drop_nested_records_preserve_each_owned_field_as_a_handle() {
        use aura_core::layout::LayoutKind;
        let module = crate::lower_source_to_mir(include_str!(
            "../tests/fixtures/run-pass/class_drop_nested_record_copy.au"
        ))
        .unwrap();
        let records = super::plain::PlainRecords::new(&module);
        assert!(records.class_layout("Ticket").is_none());
        for class in ["Envelope", "Wrapper"] {
            let (layouts, root) = records.class_layout(class).unwrap();
            let LayoutKind::Struct { fields, .. } = &layouts.get(root).kind else {
                panic!("expected record")
            };
            assert!(matches!(
                layouts.get(fields[0].layout).kind,
                LayoutKind::Handle
            ));
        }
    }

    #[test]
    fn class_drop_hook_emits_a_native_callback_only_for_hooked_programs() {
        for (source, expects_hook) in [
            (
                include_str!("../tests/fixtures/run-pass/class_drop_aggregate_scope.au"),
                true,
            ),
            (
                include_str!("../tests/fixtures/run-pass/class_drop_hook.au"),
                true,
            ),
            (
                "class Box:\n    value: int64\n\ndef main():\n    print(Box(3).value)\n",
                false,
            ),
        ] {
            let module = crate::lower_source_to_mir(source).unwrap();
            let object = super::emit_host_object(&module).unwrap();
            let name = b"aura_direct_arm_class_drop";
            assert_eq!(
                object.windows(name.len()).any(|slice| slice == name),
                expects_hook
            );
        }
    }
}

/// The MIR roots whose layout can contain a hook, and rank registers of
/// lexical owners. Compiler temporaries need release but have no lexical rank.
#[derive(Default)]
pub(super) struct Owners {
    pub(super) roots: HashSet<String>,
    ranks: HashMap<String, Var>,
}

impl Owners {
    pub(super) fn prepare(
        function: &MirFunction,
        types: &HashSet<String>,
        builder: &mut dyn FunctionBackend,
        index: &mut usize,
    ) -> Self {
        let mut owners = Owners::default();
        if types.is_empty() {
            return owners;
        }
        owners.roots.extend(
            function
                .local_types
                .iter()
                .map(|local| (&local.name, &local.ty))
                .chain(function.params.iter().map(|param| (&param.name, &param.ty)))
                .filter(|(_, ty)| can_hold_owner(ty, types))
                .map(|(name, _)| name.clone()),
        );
        for instruction in function.blocks.iter().flat_map(|b| &b.instructions) {
            if let Instruction::EndOwner { place } = instruction {
                owners.ranks.entry(place.clone()).or_insert_with(|| {
                    let rank = Var(*index as u32);
                    *index += 1;
                    builder.declare_var(rank, types::I64);
                    let zero = builder.iconst(types::I64, 0);
                    builder.def_var(rank, zero);
                    rank
                });
            }
        }
        owners
    }
}

/// All nominal layouts that can contain an owner, including through fields
/// of another class. Native field and local storage use the same set.
pub(super) fn owner_types(module: &MirModule) -> HashSet<String> {
    let hooks: HashSet<_> = module
        .classes
        .iter()
        .filter(|class| {
            class
                .methods
                .iter()
                .any(|method| method.name == crate::sema::DROP_HOOK)
        })
        .map(|class| class.name.clone())
        .collect();
    if hooks.is_empty() {
        return hooks;
    }
    let mut types = hooks;
    loop {
        let count = types.len();
        for class in &module.classes {
            if class
                .fields
                .iter()
                .any(|field| can_hold_owner(&field.ty, &types))
            {
                types.insert(class.name.clone());
            }
        }
        for enumeration in &module.enums {
            if enumeration
                .variants
                .iter()
                .flat_map(|v| &v.payloads)
                .any(|ty| can_hold_owner(ty, &types))
            {
                types.insert(enumeration.name.clone());
            }
        }
        if count == types.len() {
            break;
        }
    }

    types
}

pub(super) fn can_hold_owner(ty: &Type, hooked: &HashSet<String>) -> bool {
    match ty {
        Type::TypeParam(_) | Type::Callable(_) | Type::Closure { .. } => true,
        Type::Named(name, args) => {
            ty.is_trait_object()
                || hooked.contains(name.as_str())
                || args.iter().any(|ty| can_hold_owner(ty, hooked))
        }
        Type::Tuple(items) => items.iter().any(|ty| can_hold_owner(ty, hooked)),
        Type::Union(union) => union.members.iter().any(|ty| can_hold_owner(ty, hooked)),
        _ => false,
    }
}

impl FunctionCompiler<'_> {
    pub(super) fn stamp_lexical_owner(&mut self, place: &str) -> std::result::Result<(), String> {
        let Some(rank) = self.class_owners.ranks.get(place).copied() else {
            return Ok(());
        };
        // Boxing an inline callable or union for this borrowed runtime call
        // creates a temporary. Its lifetime ends with the rank update.
        let baseline = self.layout_scratch.clone();
        let owner = self.load_root(place)?;
        let owner = self.ensure_opaque(owner)?;
        let previous = self.builder.use_var(rank);
        let id = declare_runtime_function(
            self.builder.module(),
            "aura_direct_stamp_owner",
            &[types::I64, types::I64],
            Some(types::I64),
        )?;
        let stamp = self.builder.import_function(id);
        let result = self.builder.call(stamp, &[owner.values[0], previous]);
        self.builder.def_var(rank, result.results[0]);
        self.release_temporary_owned_since(&baseline);
        Ok(())
    }

    pub(super) fn after_owner_instruction(
        &mut self,
        instruction: &Instruction,
    ) -> std::result::Result<(), String> {
        if self.class_owners.roots.is_empty() {
            return Ok(());
        }
        match instruction {
            Instruction::Assign { target, .. } => self.stamp_lexical_owner(target)?,
            Instruction::EndOwner { place } => {
                if let Some(rank) = self.class_owners.ranks.get(place) {
                    let zero = self.builder.iconst(types::I64, 0);
                    self.builder.def_var(*rank, zero);
                }
                return Ok(());
            }
            Instruction::Release { .. }
            | Instruction::Eval { .. }
            | Instruction::WriteLoan { .. } => {}
            _ => return Ok(()),
        }
        let mut places = Vec::new();
        crate::mir::instruction_place_mentions(instruction, &mut places);
        if places.iter().any(|place| {
            self.class_owners
                .roots
                .contains(place.split('.').next().unwrap_or(place))
        }) {
            self.drain_drop_hooks()?;
        }
        Ok(())
    }
}
