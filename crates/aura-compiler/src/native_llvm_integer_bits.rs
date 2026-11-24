use super::*;
use aura_core::integer::IntegerKind;
use aura_core::integer_bits::BitMethod;

impl<'ctx> FunctionLowerer<'ctx, '_, '_> {
    pub(super) fn lower_integer_bits(
        &self,
        method: BitMethod,
        kind: IntegerKind,
        word: Val,
        count: Val,
    ) -> Result<IntValue<'ctx>, String> {
        let ty = match kind.bit_width() {
            8 => self.context.i8_type(),
            16 => self.context.i16_type(),
            32 => self.context.i32_type(),
            64 => self.context.i64_type(),
            _ => return Err("expected a word-sized integer".into()),
        };
        let operand = built(self.builder.build_int_cast(self.int(word)?, ty, ""))?;
        let mut args = vec![operand.into()];
        let name = match method {
            BitMethod::RotateLeft | BitMethod::RotateRight => {
                args.push(operand.into());
                args.push(built(self.builder.build_int_cast(self.int(count)?, ty, ""))?.into());
                if method == BitMethod::RotateLeft {
                    "llvm.fshl"
                } else {
                    "llvm.fshr"
                }
            }
            BitMethod::CountOnes => "llvm.ctpop",
            BitMethod::LeadingZeros | BitMethod::TrailingZeros => {
                args.push(self.context.bool_type().const_zero().into());
                if method == BitMethod::LeadingZeros {
                    "llvm.ctlz"
                } else {
                    "llvm.cttz"
                }
            }
            BitMethod::SwapBytes if kind.bit_width() == 8 => return self.int(word),
            BitMethod::SwapBytes => "llvm.bswap",
        };
        let function = Intrinsic::find(name)
            .and_then(|op| op.get_declaration(self.object.module, &[ty.into()]))
            .ok_or_else(|| format!("LLVM has no {name}"))?;
        let result = built(self.builder.build_call(function, &args, ""))?
            .try_as_basic_value()
            .basic()
            .expect("integer intrinsic has a result")
            .into_int_value();
        built(self.builder.build_int_cast_sign_flag(
            result,
            self.context.i64_type(),
            kind.is_signed() && !method.counts(),
            "",
        ))
    }
}
