//! The Cranelift implementation of the native backend interface.
//!
//! Each handle is the index of the Cranelift entity it names, so the
//! emitter's calls build the same instructions in the same order as direct
//! Cranelift calls would. The Cranelift settings, the host ISA, and the
//! object builder stay here.

use cranelift_codegen::ir::{
    self, condcodes, immediates::Ieee64, InstBuilder, InstructionData, Opcode, StackSlotData,
    StackSlotKind, UserFuncName, ValueDef,
};
use cranelift_codegen::settings::{self, Configurable};
use cranelift_frontend::{FunctionBuilder, FunctionBuilderContext, Variable};
use cranelift_module::{default_libcall_names, DataId, FuncId, Module};
use cranelift_object::{ObjectBuilder, ObjectModule};

use super::{
    Blk, Call, CallConv, DataAddress, DataDescription, DataHandle, DataRef, DefineError, FloatCC,
    FuncHandle, FuncRef, FunctionBackend, FunctionBody, Inst, IntCC, Linkage, MemFlags,
    ModuleBackend, ScalarType, SigRef, Signature, Slot, TrapCode, Val, Var,
};

fn set_native_codegen_flag(
    flag_builder: &mut settings::Builder,
    name: &str,
    value: &str,
) -> Result<(), String> {
    match flag_builder.set(name, value) {
        Ok(()) => Ok(()),
        Err(error) => Err(format!("failed to configure native backend: {error}")),
    }
}

pub(super) fn native_codegen_flags() -> Result<settings::Flags, String> {
    let mut flag_builder = settings::builder();
    set_native_codegen_flag(&mut flag_builder, "opt_level", "speed")?;
    set_native_codegen_flag(&mut flag_builder, "is_pic", "true")?;
    set_native_codegen_flag(&mut flag_builder, "unwind_info", "true")?;
    // Aura's direct-call ABI is private to one generated object and flattens
    // mutable receiver/parameter writeback into additional result values. On
    // x86-64 that can exceed the two integer return registers. Cranelift must
    // lower the overflow through its implicit return area so every Aura
    // caller and callee keeps the same internal signature.
    set_native_codegen_flag(&mut flag_builder, "enable_multi_ret_implicit_sret", "true")?;
    Ok(settings::Flags::new(flag_builder))
}

/// One object built with Cranelift for the host ISA.
pub(super) struct CraneliftModule {
    object: ObjectModule,
}

impl CraneliftModule {
    pub(super) fn for_host() -> Result<Self, String> {
        let flags = native_codegen_flags()?;
        let isa_builder = match cranelift_native::builder() {
            Ok(builder) => builder,
            Err(error) => return Err(format!("failed to detect host ISA: {error}")),
        };
        let isa = match isa_builder.finish(flags) {
            Ok(isa) => isa,
            Err(error) => return Err(format!("failed to build host ISA: {error}")),
        };
        let mut builder =
            match ObjectBuilder::new(isa, "aura_direct".to_string(), default_libcall_names()) {
                Ok(builder) => builder,
                Err(error) => return Err(format!("failed to initialize object builder: {error}")),
            };
        // One section per function lets ELF linkers drop unused generated
        // functions with `--gc-sections`; Mach-O already strips per symbol.
        builder.per_function_section(true);
        Ok(Self {
            object: ObjectModule::new(builder),
        })
    }

    /// `signature` in the host's own calling convention. Both of Aura's
    /// conventions use it: the implicit return area flag carries the extra
    /// values of the private one.
    fn signature(&self, signature: &Signature) -> ir::Signature {
        let mut lowered = match signature.call_conv {
            CallConv::Aura | CallConv::C => self.object.make_signature(),
        };
        lowered.params = signature
            .params
            .iter()
            .map(|ty| ir::AbiParam::new(scalar_type(*ty)))
            .collect();
        lowered.returns = signature
            .returns
            .iter()
            .map(|ty| ir::AbiParam::new(scalar_type(*ty)))
            .collect();
        lowered
    }
}

impl ModuleBackend for CraneliftModule {
    fn declare_function(
        &mut self,
        name: &str,
        linkage: Linkage,
        signature: &Signature,
    ) -> Result<FuncHandle, String> {
        let signature = self.signature(signature);
        match self
            .object
            .declare_function(name, module_linkage(linkage), &signature)
        {
            Ok(id) => Ok(FuncHandle(id.as_u32())),
            Err(error) => Err(error.to_string()),
        }
    }

    fn declare_data(
        &mut self,
        name: &str,
        linkage: Linkage,
        writable: bool,
    ) -> Result<DataHandle, String> {
        match self
            .object
            .declare_data(name, module_linkage(linkage), writable, false)
        {
            Ok(id) => Ok(DataHandle(id.as_u32())),
            Err(error) => Err(error.to_string()),
        }
    }

    fn define_data(&mut self, data: DataHandle, contents: DataDescription) -> Result<(), String> {
        let mut description = cranelift_module::DataDescription::new();
        description.define(contents.bytes.into_boxed_slice());
        if let Some(align) = contents.align {
            description.set_align(align);
        }
        for (offset, address) in &contents.addresses {
            match address {
                DataAddress::Function(function) => {
                    let function = self
                        .object
                        .declare_func_in_data(FuncId::from_u32(function.0), &mut description);
                    description.write_function_addr(*offset, function);
                }
                DataAddress::Data(target) => {
                    let target = self
                        .object
                        .declare_data_in_data(DataId::from_u32(target.0), &mut description);
                    description.write_data_addr(*offset, target, 0);
                }
            }
        }
        match self
            .object
            .define_data(DataId::from_u32(data.0), &description)
        {
            Ok(()) => Ok(()),
            Err(error) => Err(error.to_string()),
        }
    }

    /// Cranelift has no attribute for this, and it never merges calls.
    fn mark_task_invariant(&mut self, _function: FuncHandle) {}

    fn define_function(
        &mut self,
        function: FuncHandle,
        signature: &Signature,
        body: FunctionBody<'_>,
    ) -> Result<(), DefineError> {
        let mut ctx = self.object.make_context();
        ctx.func.signature = self.signature(signature);
        ctx.func.name = UserFuncName::user(0, function.0);
        let mut builder_ctx = FunctionBuilderContext::new();
        let mut builder = CraneliftFunction {
            builder: FunctionBuilder::new(&mut ctx.func, &mut builder_ctx),
            module: self,
        };
        if let Err(error) = body(&mut builder) {
            return Err(DefineError::Body(error));
        }
        builder.builder.finalize();
        match self
            .object
            .define_function(FuncId::from_u32(function.0), &mut ctx)
        {
            Ok(()) => Ok(()),
            Err(error) => Err(DefineError::Backend(error.to_string())),
        }
    }

    fn finish(self: Box<Self>) -> Result<Vec<u8>, String> {
        match self.object.finish().emit() {
            Ok(bytes) => Ok(bytes),
            Err(error) => Err(error.to_string()),
        }
    }
}

fn module_linkage(linkage: Linkage) -> cranelift_module::Linkage {
    match linkage {
        Linkage::Import => cranelift_module::Linkage::Import,
        Linkage::Local => cranelift_module::Linkage::Local,
        Linkage::Export => cranelift_module::Linkage::Export,
    }
}

fn scalar_type(ty: ScalarType) -> ir::Type {
    match ty {
        ScalarType::I8 => ir::types::I8,
        ScalarType::I32 => ir::types::I32,
        ScalarType::I64 => ir::types::I64,
        ScalarType::F32 => ir::types::F32,
        ScalarType::F64 => ir::types::F64,
    }
}

fn int_cc(cond: IntCC) -> condcodes::IntCC {
    match cond {
        IntCC::Equal => condcodes::IntCC::Equal,
        IntCC::NotEqual => condcodes::IntCC::NotEqual,
        IntCC::SignedLessThan => condcodes::IntCC::SignedLessThan,
        IntCC::SignedGreaterThanOrEqual => condcodes::IntCC::SignedGreaterThanOrEqual,
        IntCC::SignedGreaterThan => condcodes::IntCC::SignedGreaterThan,
        IntCC::SignedLessThanOrEqual => condcodes::IntCC::SignedLessThanOrEqual,
        IntCC::UnsignedLessThan => condcodes::IntCC::UnsignedLessThan,
        IntCC::UnsignedGreaterThanOrEqual => condcodes::IntCC::UnsignedGreaterThanOrEqual,
        IntCC::UnsignedGreaterThan => condcodes::IntCC::UnsignedGreaterThan,
        IntCC::UnsignedLessThanOrEqual => condcodes::IntCC::UnsignedLessThanOrEqual,
    }
}

fn float_cc(cond: FloatCC) -> condcodes::FloatCC {
    match cond {
        FloatCC::Equal => condcodes::FloatCC::Equal,
        FloatCC::NotEqual => condcodes::FloatCC::NotEqual,
        FloatCC::LessThan => condcodes::FloatCC::LessThan,
        FloatCC::LessThanOrEqual => condcodes::FloatCC::LessThanOrEqual,
        FloatCC::GreaterThan => condcodes::FloatCC::GreaterThan,
        FloatCC::GreaterThanOrEqual => condcodes::FloatCC::GreaterThanOrEqual,
    }
}

fn mem_flags(flags: MemFlags) -> ir::MemFlags {
    if flags.trusted {
        ir::MemFlags::trusted()
    } else {
        ir::MemFlags::new()
    }
}

fn value(value: Val) -> ir::Value {
    ir::Value::from_u32(value.0)
}

fn values(values: &[Val]) -> Vec<ir::Value> {
    values.iter().map(|each| value(*each)).collect()
}

fn val(value: ir::Value) -> Val {
    Val(value.as_u32())
}

fn block(block: Blk) -> ir::Block {
    ir::Block::from_u32(block.0)
}

fn func_ref(function: FuncRef) -> ir::FuncRef {
    ir::FuncRef::from_u32(function.0)
}

fn inst(inst: Inst) -> ir::Inst {
    ir::Inst::from_u32(inst.0)
}

/// One function body under construction, with the module it belongs to.
struct CraneliftFunction<'a> {
    builder: FunctionBuilder<'a>,
    module: &'a mut CraneliftModule,
}

impl CraneliftFunction<'_> {
    fn call_results(&self, call: ir::Inst) -> Call {
        Call {
            inst: Inst(call.as_u32()),
            results: self
                .builder
                .inst_results(call)
                .iter()
                .map(|result| val(*result))
                .collect(),
        }
    }
}

impl FunctionBackend for CraneliftFunction<'_> {
    fn module(&mut self) -> &mut dyn ModuleBackend {
        self.module
    }

    fn create_block(&mut self) -> Blk {
        Blk(self.builder.create_block().as_u32())
    }

    fn switch_to_block(&mut self, target: Blk) {
        self.builder.switch_to_block(block(target));
    }

    fn seal_block(&mut self, target: Blk) {
        self.builder.seal_block(block(target));
    }

    fn seal_all_blocks(&mut self) {
        self.builder.seal_all_blocks();
    }

    fn set_cold_block(&mut self, target: Blk) {
        self.builder.set_cold_block(block(target));
    }

    fn current_block(&self) -> Option<Blk> {
        self.builder
            .current_block()
            .map(|current| Blk(current.as_u32()))
    }

    fn block_params(&self, target: Blk) -> Vec<Val> {
        self.builder
            .block_params(block(target))
            .iter()
            .map(|param| val(*param))
            .collect()
    }

    fn append_block_param(&mut self, target: Blk, ty: ScalarType) -> Val {
        val(self
            .builder
            .append_block_param(block(target), scalar_type(ty)))
    }

    fn append_block_params_for_function_params(&mut self, target: Blk) {
        self.builder
            .append_block_params_for_function_params(block(target));
    }

    fn declare_var(&mut self, var: Var, ty: ScalarType) {
        self.builder
            .declare_var(Variable::from_u32(var.0), scalar_type(ty));
    }

    fn def_var(&mut self, var: Var, defined: Val) {
        self.builder
            .def_var(Variable::from_u32(var.0), value(defined));
    }

    fn use_var(&mut self, var: Var) -> Val {
        val(self.builder.use_var(Variable::from_u32(var.0)))
    }

    fn create_sized_stack_slot(&mut self, size: u32, align_shift: u8) -> Slot {
        let slot = self.builder.create_sized_stack_slot(StackSlotData::new(
            StackSlotKind::ExplicitSlot,
            size,
            align_shift,
        ));
        Slot(slot.as_u32())
    }

    fn stack_addr(&mut self, ty: ScalarType, slot: Slot, offset: i32) -> Val {
        val(self
            .builder
            .ins()
            .stack_addr(scalar_type(ty), ir::StackSlot::from_u32(slot.0), offset))
    }

    fn import_function(&mut self, function: FuncHandle) -> FuncRef {
        let imported = self
            .module
            .object
            .declare_func_in_func(FuncId::from_u32(function.0), self.builder.func);
        FuncRef(imported.as_u32())
    }

    fn import_data(&mut self, data: DataHandle) -> DataRef {
        let imported = self
            .module
            .object
            .declare_data_in_func(DataId::from_u32(data.0), self.builder.func);
        DataRef(imported.as_u32())
    }

    fn import_signature(&mut self, signature: Signature) -> SigRef {
        let signature = self.module.signature(&signature);
        SigRef(self.builder.func.import_signature(signature).as_u32())
    }

    fn func_addr(&mut self, ty: ScalarType, function: FuncRef) -> Val {
        val(self
            .builder
            .ins()
            .func_addr(scalar_type(ty), func_ref(function)))
    }

    fn symbol_value(&mut self, ty: ScalarType, data: DataRef) -> Val {
        val(self
            .builder
            .ins()
            .symbol_value(scalar_type(ty), ir::GlobalValue::from_u32(data.0)))
    }

    fn call(&mut self, callee: FuncRef, args: &[Val]) -> Call {
        let call = self.builder.ins().call(func_ref(callee), &values(args));
        self.call_results(call)
    }

    fn call_indirect(&mut self, signature: SigRef, callee: Val, args: &[Val]) -> Call {
        let call = self.builder.ins().call_indirect(
            ir::SigRef::from_u32(signature.0),
            value(callee),
            &values(args),
        );
        self.call_results(call)
    }

    fn jump(&mut self, target: Blk, args: &[Val]) {
        self.builder.ins().jump(block(target), &values(args));
    }

    fn brif(
        &mut self,
        condition: Val,
        then_block: Blk,
        then_args: &[Val],
        else_block: Blk,
        else_args: &[Val],
    ) {
        self.builder.ins().brif(
            value(condition),
            block(then_block),
            &values(then_args),
            block(else_block),
            &values(else_args),
        );
    }

    fn return_(&mut self, returned: &[Val]) {
        self.builder.ins().return_(&values(returned));
    }

    fn trap(&mut self, code: TrapCode) {
        let code = match code {
            TrapCode::IntegerOverflow => ir::TrapCode::INTEGER_OVERFLOW,
            TrapCode::IntegerDivisionByZero => ir::TrapCode::INTEGER_DIVISION_BY_ZERO,
            TrapCode::User(code) => ir::TrapCode::unwrap_user(code),
        };
        self.builder.ins().trap(code);
    }

    fn value_type(&self, typed: Val) -> ScalarType {
        let ty = self.builder.func.dfg.value_type(value(typed));
        if ty == ir::types::I8 {
            ScalarType::I8
        } else if ty == ir::types::I32 {
            ScalarType::I32
        } else if ty == ir::types::F32 {
            ScalarType::F32
        } else if ty == ir::types::F64 {
            ScalarType::F64
        } else {
            ScalarType::I64
        }
    }

    fn integer_constant(&self, constant: Val) -> Option<i64> {
        let dfg = &self.builder.func.dfg;
        let ValueDef::Result(defining, _) = dfg.value_def(value(constant)) else {
            return None;
        };
        match dfg.insts[defining] {
            InstructionData::UnaryImm {
                opcode: Opcode::Iconst,
                imm,
            } => Some(imm.bits()),
            _ => None,
        }
    }

    fn only_calls_since(&self, since: Inst, allowed: &[FuncRef]) -> bool {
        let Some(current_block) = self.builder.current_block() else {
            return false;
        };
        let since = inst(since);
        let func = &self.builder.func;
        if func.layout.inst_block(since) != Some(current_block) {
            return false;
        }
        let mut cursor = func.layout.last_inst(current_block);
        while let Some(current) = cursor {
            if current == since {
                return true;
            }
            match func.dfg.insts[current] {
                InstructionData::Call {
                    func_ref: callee, ..
                } if allowed.iter().any(|each| func_ref(*each) == callee) => {}
                ref data if data.opcode().is_call() => return false,
                _ => {}
            }
            cursor = func.layout.prev_inst(current);
        }
        false
    }

    fn replace_with_call(&mut self, replaced: Inst, callee: FuncRef, args: &[Val]) {
        self.builder
            .func
            .dfg
            .replace(inst(replaced))
            .call(func_ref(callee), &values(args));
    }

    fn replace_with_nop(&mut self, replaced: Inst) {
        self.builder.func.dfg.replace(inst(replaced)).nop();
    }

    fn replace_with_copy(&mut self, replaced: Inst, copied: Val) {
        self.builder
            .func
            .dfg
            .replace(inst(replaced))
            .iadd_imm(value(copied), 0);
    }

    fn verify(&self) -> Result<(), String> {
        match cranelift_codegen::verify_function(self.builder.func, self.module.object.isa()) {
            Ok(()) => Ok(()),
            Err(error) => Err(format!("{error}\n{}", self.builder.func.display())),
        }
    }

    #[cfg(test)]
    fn instruction_count(&self) -> usize {
        self.builder.func.dfg.num_insts()
    }

    fn iconst(&mut self, ty: ScalarType, constant: i64) -> Val {
        val(self.builder.ins().iconst(scalar_type(ty), constant))
    }

    fn f64const(&mut self, constant: f64) -> Val {
        val(self.builder.ins().f64const(Ieee64::with_float(constant)))
    }

    fn iadd(&mut self, left: Val, right: Val) -> Val {
        val(self.builder.ins().iadd(value(left), value(right)))
    }

    fn iadd_imm(&mut self, operand: Val, imm: i64) -> Val {
        val(self.builder.ins().iadd_imm(value(operand), imm))
    }

    fn isub(&mut self, left: Val, right: Val) -> Val {
        val(self.builder.ins().isub(value(left), value(right)))
    }

    fn imul(&mut self, left: Val, right: Val) -> Val {
        val(self.builder.ins().imul(value(left), value(right)))
    }

    fn imul_imm(&mut self, operand: Val, imm: i64) -> Val {
        val(self.builder.ins().imul_imm(value(operand), imm))
    }

    fn ineg(&mut self, operand: Val) -> Val {
        val(self.builder.ins().ineg(value(operand)))
    }

    fn sdiv(&mut self, left: Val, right: Val) -> Val {
        val(self.builder.ins().sdiv(value(left), value(right)))
    }

    fn udiv(&mut self, left: Val, right: Val) -> Val {
        val(self.builder.ins().udiv(value(left), value(right)))
    }

    fn srem(&mut self, left: Val, right: Val) -> Val {
        val(self.builder.ins().srem(value(left), value(right)))
    }

    fn urem(&mut self, left: Val, right: Val) -> Val {
        val(self.builder.ins().urem(value(left), value(right)))
    }

    fn band(&mut self, left: Val, right: Val) -> Val {
        val(self.builder.ins().band(value(left), value(right)))
    }

    fn band_imm(&mut self, operand: Val, imm: i64) -> Val {
        val(self.builder.ins().band_imm(value(operand), imm))
    }

    fn bor(&mut self, left: Val, right: Val) -> Val {
        val(self.builder.ins().bor(value(left), value(right)))
    }

    fn bxor(&mut self, left: Val, right: Val) -> Val {
        val(self.builder.ins().bxor(value(left), value(right)))
    }

    fn bxor_imm(&mut self, operand: Val, imm: i64) -> Val {
        val(self.builder.ins().bxor_imm(value(operand), imm))
    }

    fn icmp(&mut self, cond: IntCC, left: Val, right: Val) -> Val {
        val(self
            .builder
            .ins()
            .icmp(int_cc(cond), value(left), value(right)))
    }

    fn icmp_imm(&mut self, cond: IntCC, operand: Val, imm: i64) -> Val {
        val(self
            .builder
            .ins()
            .icmp_imm(int_cc(cond), value(operand), imm))
    }

    fn select(&mut self, condition: Val, if_true: Val, if_false: Val) -> Val {
        val(self
            .builder
            .ins()
            .select(value(condition), value(if_true), value(if_false)))
    }

    fn uextend(&mut self, ty: ScalarType, operand: Val) -> Val {
        val(self.builder.ins().uextend(scalar_type(ty), value(operand)))
    }

    fn ireduce(&mut self, ty: ScalarType, operand: Val) -> Val {
        val(self.builder.ins().ireduce(scalar_type(ty), value(operand)))
    }

    fn bitcast(&mut self, ty: ScalarType, operand: Val) -> Val {
        val(self
            .builder
            .ins()
            .bitcast(scalar_type(ty), ir::MemFlags::new(), value(operand)))
    }

    fn sadd_overflow(&mut self, left: Val, right: Val) -> (Val, Val) {
        let (result, overflow) = self.builder.ins().sadd_overflow(value(left), value(right));
        (val(result), val(overflow))
    }

    fn uadd_overflow(&mut self, left: Val, right: Val) -> (Val, Val) {
        let (result, overflow) = self.builder.ins().uadd_overflow(value(left), value(right));
        (val(result), val(overflow))
    }

    fn ssub_overflow(&mut self, left: Val, right: Val) -> (Val, Val) {
        let (result, overflow) = self.builder.ins().ssub_overflow(value(left), value(right));
        (val(result), val(overflow))
    }

    fn usub_overflow(&mut self, left: Val, right: Val) -> (Val, Val) {
        let (result, overflow) = self.builder.ins().usub_overflow(value(left), value(right));
        (val(result), val(overflow))
    }

    fn smul_overflow(&mut self, left: Val, right: Val) -> (Val, Val) {
        let (result, overflow) = self.builder.ins().smul_overflow(value(left), value(right));
        (val(result), val(overflow))
    }

    fn umul_overflow(&mut self, left: Val, right: Val) -> (Val, Val) {
        let (result, overflow) = self.builder.ins().umul_overflow(value(left), value(right));
        (val(result), val(overflow))
    }

    fn load(&mut self, ty: ScalarType, flags: MemFlags, address: Val, offset: i32) -> Val {
        val(self
            .builder
            .ins()
            .load(scalar_type(ty), mem_flags(flags), value(address), offset))
    }

    fn uload8(&mut self, ty: ScalarType, flags: MemFlags, address: Val, offset: i32) -> Val {
        val(self
            .builder
            .ins()
            .uload8(scalar_type(ty), mem_flags(flags), value(address), offset))
    }

    fn uload32(&mut self, flags: MemFlags, address: Val, offset: i32) -> Val {
        val(self
            .builder
            .ins()
            .uload32(mem_flags(flags), value(address), offset))
    }

    fn sload32(&mut self, flags: MemFlags, address: Val, offset: i32) -> Val {
        val(self
            .builder
            .ins()
            .sload32(mem_flags(flags), value(address), offset))
    }

    fn store(&mut self, flags: MemFlags, stored: Val, address: Val, offset: i32) {
        self.builder
            .ins()
            .store(mem_flags(flags), value(stored), value(address), offset);
    }

    fn fadd(&mut self, left: Val, right: Val) -> Val {
        val(self.builder.ins().fadd(value(left), value(right)))
    }

    fn fsub(&mut self, left: Val, right: Val) -> Val {
        val(self.builder.ins().fsub(value(left), value(right)))
    }

    fn fmul(&mut self, left: Val, right: Val) -> Val {
        val(self.builder.ins().fmul(value(left), value(right)))
    }

    fn fdiv(&mut self, left: Val, right: Val) -> Val {
        val(self.builder.ins().fdiv(value(left), value(right)))
    }

    fn fneg(&mut self, operand: Val) -> Val {
        val(self.builder.ins().fneg(value(operand)))
    }

    fn fcmp(&mut self, cond: FloatCC, left: Val, right: Val) -> Val {
        val(self
            .builder
            .ins()
            .fcmp(float_cc(cond), value(left), value(right)))
    }

    fn fpromote(&mut self, ty: ScalarType, operand: Val) -> Val {
        val(self.builder.ins().fpromote(scalar_type(ty), value(operand)))
    }

    fn fdemote(&mut self, ty: ScalarType, operand: Val) -> Val {
        val(self.builder.ins().fdemote(scalar_type(ty), value(operand)))
    }

    fn fcvt_from_uint(&mut self, ty: ScalarType, operand: Val) -> Val {
        val(self
            .builder
            .ins()
            .fcvt_from_uint(scalar_type(ty), value(operand)))
    }

    fn fcvt_from_sint(&mut self, ty: ScalarType, operand: Val) -> Val {
        val(self
            .builder
            .ins()
            .fcvt_from_sint(scalar_type(ty), value(operand)))
    }
}

#[cfg(test)]
#[path = "native_backend_cranelift_tests.rs"]
mod tests;
