use super::*;
use crate::inline_asm::MachineAsm;

fn width(bits: u8) -> Result<ScalarType, String> {
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
            let word = self.load_operand(input)?.values[0];
            let ty = width(*bits)?;
            operands.push(if ty == types::I64 {
                word
            } else {
                self.builder.ireduce(ty, word)
            });
        }
        let types = asm
            .output_widths
            .iter()
            .copied()
            .map(width)
            .collect::<Result<Vec<_>, _>>()?;
        let values = self.builder.inline_asm(asm, &operands, &types);
        for ((value, name), bits) in values.into_iter().zip(outputs).zip(&asm.output_widths) {
            let ty = self.type_of_place(name)?;
            let word = if *bits == 64 {
                value
            } else {
                let word = self.builder.uextend(types::I64, value);
                if ty
                    .scalar_kind()
                    .is_some_and(|kind| kind.is_unsigned_integer())
                {
                    word
                } else {
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
