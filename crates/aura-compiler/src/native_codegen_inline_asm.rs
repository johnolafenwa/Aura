use super::*;
use crate::inline_asm::MachineAsm;

fn width(kind: Option<ScalarKind>, bits: u8) -> Result<ScalarType, String> {
    match (kind, bits) {
        (Some(ScalarKind::Float32), 32) => return Ok(types::F32),
        (Some(ScalarKind::Float64), 64) => return Ok(types::F64),
        _ => {}
    }
    match bits {
        8 => Ok(types::I8),
        16 => Ok(types::I16),
        32 => Ok(types::I32),
        64 => Ok(types::I64),
        _ => Err(format!("invalid assembly operand width {bits}")),
    }
}

impl FunctionCompiler<'_> {
    pub(super) fn compile_inline_asm(
        &mut self,
        asm: &MachineAsm,
        inputs: &[Operand],
        outputs: &[String],
    ) -> Result<(), String> {
        if self
            .freestanding
            .is_none_or(|target| target.arch() != asm.arch)
        {
            return Err("inline assembly requires its checked freestanding target".into());
        }
        if inputs.len() != asm.input_widths.len() || outputs.len() != asm.output_widths.len() {
            return Err("assembly operand count mismatch".into());
        }
        let mut operands = Vec::new();
        for (input, bits) in inputs.iter().zip(&asm.input_widths) {
            let input = self.load_operand(input)?;
            let word = input.values[0];
            let ty = width(input.ty.scalar_kind(), *bits)?;
            operands.push(match ty {
                ScalarType::I64 | ScalarType::F64 => word,
                ScalarType::F32 => self.builder.fdemote(types::F32, word),
                _ => self.builder.ireduce(ty, word),
            });
        }
        let kinds = outputs
            .iter()
            .map(|name| self.type_of_place(name))
            .collect::<Result<Vec<_>, _>>()?;
        let types = kinds
            .iter()
            .zip(&asm.output_widths)
            .map(|(kind, bits)| width(kind.scalar_kind(), *bits))
            .collect::<Result<Vec<_>, _>>()?;
        let values = self.builder.inline_asm(asm, &operands, &types);
        for (((value, name), bits), ty) in values
            .into_iter()
            .zip(outputs)
            .zip(&asm.output_widths)
            .zip(kinds)
        {
            let word = match ty.scalar_kind() {
                Some(ScalarKind::Float32) => self.builder.fpromote(types::F64, value),
                Some(ScalarKind::Bool) => {
                    let bit = self.builder.icmp_imm(IntCC::NotEqual, value, 0);
                    self.builder.uextend(types::I64, bit)
                }
                _ if *bits == 64 => value,
                Some(kind) if kind.is_unsigned_integer() => self.builder.uextend(types::I64, value),
                _ => {
                    let word = self.builder.uextend(types::I64, value);
                    let shift = self.builder.iconst(types::I64, i64::from(64 - bits));
                    let high = self.builder.ishl(word, shift);
                    self.builder.sshr(high, shift)
                }
            };
            self.store_place(
                name,
                ValueRef {
                    ty,
                    values: vec![word],
                },
            )?;
        }
        Ok(())
    }
}
