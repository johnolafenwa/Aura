use super::*;
use aura_core::integer::IntegerKind;
use aura_core::integer_bits::BitMethod;

impl FunctionCompiler<'_> {
    pub(super) fn compile_bit_method(&mut self, method: BitMethod, object: ValueRef, args: &[MirArg]) -> std::result::Result<ValueRef, String> {
        let count = match args {
            [] => self.builder.iconst(types::I64, 0),
            [arg] => self.load_operand_for_target(&arg.value, &DirectType::Scalar(ScalarKind::Int64))?.values[0],
            _ => return Err(format!("invalid argument count for {}", method.name())),
        };
        if wide_numbers::signed(&object.ty).is_some() {
            return self.wide_bit_method(method, object, count);
        }
        let target = if method.counts() { DirectType::Scalar(ScalarKind::Int64) } else { object.ty.clone() };
        let opcode = self.builder.iconst(types::I64, method as i64);
        if let Some(kind) = object.ty.scalar_kind().and_then(|k| IntegerKind::from_runtime_type_name(k.type_name())) {
            if !args.is_empty() && !matches!(args[0].value, Operand::Int(_)) {
                let negative = self.builder.icmp_imm(IntCC::SignedLessThan, count, 0);
                let fail = self.builder.create_block();
                let okay = self.builder.create_block();
                self.builder.brif(negative, fail, &[], okay, &[]);
                self.builder.switch_to_block(fail);
                self.builder.seal_block(fail);
                self.builder.set_cold_block(fail);
                if self.freestanding.is_some() {
                    let (line, column) = self.span_values(None);
                    let code = self.builder.iconst(types::I64, freestanding::TRAP_SHIFT);
                    self.call_trap_hook_with(code, line, column);
                } else {
                    let zero = self.builder.iconst(types::I64, 0);
                    self.builder.call(self.runtime[RuntimeFn::IntegerBits], &[zero, count, opcode]);
                }
                self.builder.trap(TrapCode::IntegerOverflow);
                self.builder.switch_to_block(okay);
                self.builder.seal_block(okay);
            }
            let word = self.builder.integer_bits(method, kind, object.values[0], count);
            return Ok(ValueRef { values: vec![word], ty: target });
        }
        let results = self.runtime_call_results(self.runtime[RuntimeFn::IntegerBits], &[object.values[0], count, opcode]);
        let boxed = self.owned_opaque_result(results, direct_type_to_type(&target));
        self.coerce_value(boxed, &target)
    }
}
