//! Emit the data for an immutable module binding. Hosted runtime boxes have
//! process-local ownership metadata, so the first reader attaches a box to
//! the static payload. Entry thunks never make or initialize these values.

use super::*;
use aura_runtime::frozen_value::FrozenValue;

impl FunctionCompiler<'_> {
    pub(super) fn read_frozen_constant(
        &mut self,
        constant: &MirConstant,
        value: &FrozenValue,
        target: &DirectType,
    ) -> std::result::Result<ValueRef, String> {
        let payload = serde_json::to_vec(value).map_err(|error| error.to_string())?;
        let (data, len) = self.string_constant(&payload)?;
        let ty = crate::native_runtime::canonical_runtime_type_name(&constant.ty);
        let (name, name_len) = self.string_constant(ty.as_bytes())?;
        let cell = self.tag_cell(&module_constant_cell_key(&constant.key))?;
        let call = self.builder.call(
            self.runtime[RuntimeFn::FrozenConstant],
            &[cell, data, len, name, name_len],
        );
        let value = self.owned_opaque_result(call.results, constant.ty.clone());
        self.coerce_value(value, target)
    }
}
