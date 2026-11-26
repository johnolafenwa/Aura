//! Array parameters carry the address of contiguous storage through Aura calls.
use super::*;

impl<'ctx> FunctionLowerer<'ctx, '_, '_> {
    pub(super) fn load_array_parameters(
        &self,
        mut params: Vec<BasicValueEnum<'ctx>>,
    ) -> Result<Vec<BasicValueEnum<'ctx>>, String> {
        for (param, ty) in params.iter_mut().zip(&self.ir.signature.params) {
            if matches!(
                ty,
                ScalarType::Inline { .. } | ScalarType::OverAligned { .. }
            ) {
                *param = built(self.builder.build_load(
                    basic_type(self.context, *ty),
                    param.into_pointer_value(),
                    "",
                ))?;
            }
        }
        Ok(params)
    }

    pub(super) fn is_initial_array_binding(&self, var: Var, value: Val) -> bool {
        self.ir.borrowed_arrays.get(&var.0).and_then(|index| {
            self.ir
                .params_block
                .and_then(|block| self.ir.blocks[block as usize].params.get(*index))
        }) == Some(&value)
    }

    pub(super) fn array_arguments(
        &self,
        values: &[Val],
    ) -> Result<Vec<BasicMetadataValueEnum<'ctx>>, String> {
        values
            .iter()
            .map(|&v| {
                let ty = self.ir.ty(v);
                if !matches!(
                    ty,
                    ScalarType::Inline { .. } | ScalarType::OverAligned { .. }
                ) {
                    return Ok(self.value(v)?.into());
                }
                if let Some(inst) = self.ir.value_defs[v.0 as usize] {
                    if let Op::ArrayBorrow(_, address) = self.ir.insts[inst as usize].op {
                        return Ok(self.address(address, 0)?.into());
                    }
                }
                // A temporary argument's lifetime spans the call. Reuse its
                // entry-block stack slot on every iteration of an enclosing loop.
                let entry = self
                    .function
                    .get_first_basic_block()
                    .ok_or("missing array call entry")?;
                let first = entry
                    .get_first_instruction()
                    .ok_or("missing array call prologue")?;
                let slots = self.context.create_builder();
                slots.position_before(&first);
                let slot =
                    built(slots.build_alloca(basic_type(self.context, ty), "array_argument"))?;
                if let ScalarType::OverAligned { align, .. } = ty {
                    slot.as_instruction_value()
                        .expect("an alloca is an instruction")
                        .set_alignment(u32::from(align))
                        .map_err(|_| "invalid fixed array argument alignment")?;
                }
                built(self.builder.build_store(slot, self.value(v)?))?;
                Ok(slot.into())
            })
            .collect()
    }
}
