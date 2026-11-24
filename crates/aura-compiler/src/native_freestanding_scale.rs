//! Checked unsigned address and offset arithmetic in a freestanding object.
use super::*;

struct ScaleOffset<'a> {
    input: &'a str,
    scale: u64,
    limit: u64,
    span: Span,
}

/// Match the literal temporaries and both arithmetic steps of k*x+b. The
/// failure hook sees only a code and source position, so two overflows with
/// the same position can share the check x <= (MAX-b)/k.
fn scale_offset(instructions: &[Instruction]) -> Option<ScaleOffset<'_>> {
    let [Instruction::Assign {
        target: factor,
        value: Rvalue::Use(Operand::Int(scale)),
    }, Instruction::Assign {
        target: product,
        value:
            Rvalue::Binary {
                op: BinaryOp::Mul,
                left: Operand::Place(lhs),
                right: Operand::Place(rhs),
                trap: ArithmeticTrap::Overflow,
                span,
            },
    }, Instruction::Assign {
        target: bias,
        value: Rvalue::Use(Operand::Int(offset)),
    }, Instruction::Assign {
        value:
            Rvalue::Binary {
                op: BinaryOp::Add,
                left: Operand::Place(first),
                right: Operand::Place(second),
                trap: ArithmeticTrap::Overflow,
                span: add_span,
            },
        ..
    }, ..] = instructions
    else {
        return None;
    };
    if span != add_span
        || first != product
        || second != bias
        || ![factor, product, bias]
            .iter()
            .all(|name| name.starts_with("%t"))
        || factor == product
        || product == bias
        || factor == bias
    {
        return None;
    }
    let input = if lhs == factor {
        rhs
    } else if rhs == factor {
        lhs
    } else {
        return None;
    };
    if input == factor {
        return None;
    }
    let scale = u64::try_from(*scale).ok()?;
    let limit = (u64::MAX - u64::try_from(*offset).ok()?).checked_div(scale)?;
    Some(ScaleOffset {
        input,
        scale,
        limit,
        span: *span,
    })
}

impl FunctionCompiler<'_> {
    pub(super) fn compile_scale_offset(
        &mut self,
        block: &BasicBlock,
        index: usize,
    ) -> std::result::Result<bool, String> {
        let ops = &block.instructions[index..];
        let Some(scale) = scale_offset(ops) else {
            return Ok(false);
        };
        let unsigned = DirectType::Scalar(ScalarKind::Uint64);
        if self.variable_types.get(scale.input) != Some(&unsigned) || !ops[..4].iter().all(|op|
            matches!(op, Instruction::Assign { target, .. } if self.variable_types.get(target) == Some(&unsigned))) {
            return Ok(false);
        }
        let value = self.load_place(scale.input)?.values[0];
        let overflow = self
            .builder
            .icmp_imm(IntCC::UnsignedGreaterThan, value, scale.limit as i64);
        let factor = self.builder.iconst(types::I64, scale.scale as i64);
        self.emit_integer_overflow_failure_branch(
            overflow,
            WideIntegerKind::Uint64,
            WideOverflowOp::Mul,
            value,
            factor,
            Some(scale.span),
        )?;
        for (position, instruction) in ops[..4].iter().enumerate() {
            let mut instruction = instruction.clone();
            if let Instruction::Assign {
                value: Rvalue::Binary { trap, .. },
                ..
            } = &mut instruction
            {
                *trap = ArithmeticTrap::Proved;
            }
            self.current_snapshot_instruction = index + position;
            self.compile_instruction(&block.label, index + position, &instruction)?;
        }
        Ok(true)
    }
}

#[cfg(test)]
pub(super) fn guard_limit(instructions: &[Instruction]) -> Option<u64> {
    scale_offset(instructions).map(|scale| scale.limit)
}
