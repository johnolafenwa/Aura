use super::*;
use crate::inline_asm::MachineAsm;

impl FunctionLowerer<'_, '_, '_> {
    pub(super) fn lower_inline_asm(
        &mut self,
        inst: &InstIr,
        asm: &MachineAsm,
        inputs: &[Val],
    ) -> Result<(), String> {
        let mut signature = Signature::new(CallConv::C);
        signature.params = inputs.iter().map(|v| self.ir.ty(*v)).collect();
        signature.returns = inst.results.iter().map(|v| self.ir.ty(*v)).collect();
        let ty = function_type(self.context, &signature);
        let instruction = self.context.create_inline_asm(
            ty,
            asm.template.clone(),
            asm.constraints.clone(),
            !asm.pure,
            false,
            None,
            false,
        );
        let args = inputs
            .iter()
            .map(|v| self.value(*v).map(BasicMetadataValueEnum::from))
            .collect::<Result<Vec<_>, _>>()?;
        let call = built(self.builder.build_indirect_call(ty, instruction, &args, ""))?;
        if asm.noreturn {
            call.add_attribute(
                AttributeLoc::Function,
                self.context
                    .create_enum_attribute(Attribute::get_named_enum_kind_id("noreturn"), 0),
            );
        }
        self.call_results(inst, call)
    }
}

/// LLVM's default diagnostic handler terminates the process on an assembly
/// error. Collect target diagnostics while the assembler writes this object.
pub(super) fn write_object(
    machine: &TargetMachine,
    module: &Module<'_>,
) -> Result<Vec<u8>, String> {
    use inkwell::context::AsContextRef;
    use inkwell::llvm_sys::core::*;
    use inkwell::llvm_sys::{prelude::LLVMDiagnosticInfoRef, LLVMDiagnosticSeverity};
    use std::ffi::{c_void, CStr};

    extern "C" fn collect(info: LLVMDiagnosticInfoRef, data: *mut c_void) {
        // SAFETY: LLVM supplies a live diagnostic during this synchronous
        // call. The enclosing write owns the list until it resets the handler.
        unsafe {
            if LLVMGetDiagInfoSeverity(info) == LLVMDiagnosticSeverity::LLVMDSError {
                let text = LLVMGetDiagInfoDescription(info);
                if !text.is_null() {
                    let message = CStr::from_ptr(text).to_string_lossy().into_owned();
                    LLVMDisposeMessage(text);
                    (*data.cast::<Vec<String>>()).push(message);
                }
            }
        }
    }
    let context = module.get_context().as_ctx_ref();
    let mut errors = Vec::<String>::new();
    // SAFETY: each object writer owns its LLVM context on this thread.
    // Restore both parts of the previous handler before dropping the list.
    let written = unsafe {
        let old = LLVMContextGetDiagnosticHandler(context);
        let previous_data = LLVMContextGetDiagnosticContext(context);
        LLVMContextSetDiagnosticHandler(context, Some(collect), (&raw mut errors).cast());
        let result = machine.write_to_memory_buffer(module, FileType::Object);
        LLVMContextSetDiagnosticHandler(context, old, previous_data);
        result
    };
    if !errors.is_empty() {
        return Err(format!("target assembler: {}", errors.join("\n")));
    }
    written
        .map(|data| data.as_slice().to_vec())
        .map_err(|error| format!("LLVM failed to write the object: {error}"))
}
