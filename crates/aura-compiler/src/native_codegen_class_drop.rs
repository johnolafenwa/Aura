//! Native entry for a class hook. A mutable method returns its updated
//! receiver through the usual writeback ABI.

use super::*;

impl NativeCodegen<'_> {
    pub(super) fn define_drop_thunk(&mut self, function: &MirFunction) -> std::result::Result<(), String> {
        let id = self.function_thunks[&function.name];
        let target = self.functions[&function.name];
        self.define_body(id, &thunk_signature(), &function.name, |codegen, builder| {
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
            let unit = box_thunk_value(codegen, builder, &[zero], &DirectType::Scalar(ScalarKind::Unit))?;
            builder.return_(&[unit]);
            Ok(())
        })
    }
}

impl FunctionCompiler<'_> {
    pub(super) fn arm_drop_hook(&mut self, place: &str, function: &str) -> std::result::Result<(), String> {
        let owner = self.load_root(place)?;
        let thunk = self.function_thunk_refs.get(function).ok_or_else(|| format!("missing drop hook `{function}`"))?;
        let thunk = self.builder.func_addr(types::I64, *thunk);
        let id = declare_runtime_function(self.builder.module(), "aura_direct_arm_class_drop", &[types::I64, types::I64], None)?;
        let arm = self.builder.import_function(id);
        self.builder.call(arm, &[owner.values[0], thunk]);
        Ok(())
    }

    pub(super) fn drain_drop_hooks(&mut self) -> std::result::Result<(), String> {
        let id = declare_runtime_function(self.builder.module(), "aura_direct_drain_class_drops", &[], None)?;
        let drain = self.builder.import_function(id);
        self.builder.call(drain, &[]);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn class_drop_hook_emits_a_native_callback_only_for_hooked_programs() {
        for (source, expects_hook) in [
            (include_str!("../tests/fixtures/run-pass/class_drop_hook.au"), true),
            ("class Box:\n    value: int64\n\ndef main():\n    print(Box(3).value)\n", false),
        ] {
            let module = crate::lower_source_to_mir(source).unwrap();
            let object = super::emit_host_object(&module).unwrap();
            let name = b"aura_direct_arm_class_drop";
            assert_eq!(object.windows(name.len()).any(|slice| slice == name), expects_hook);
        }
    }
}
