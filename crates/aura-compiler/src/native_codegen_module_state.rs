//! Private, naturally aligned data definitions for freestanding module cells.
use super::*;

impl<'a> FunctionCompiler<'a> {
    fn state_cell(
        &mut self,
        key: &str,
    ) -> Result<(Val, &'a crate::sema::StateImage, Type), String> {
        if self.freestanding.is_none() {
            return Err("mutable module storage requires freestanding execution".into());
        }
        let cell = self
            .module_constants
            .iter()
            .find(|c| c.key == key)
            .ok_or_else(|| format!("missing module storage for {key}"))?;
        let image = cell
            .state
            .as_ref()
            .ok_or_else(|| format!("missing static initializer for {key}"))?;
        let intern = format!("\0module state {key}");
        let id = match self.tag_cells.get(&intern) {
            Some(id) => *id,
            None => {
                // Encode the qualified key injectively. Module names may contain dots.
                let mut symbol = String::from("__aura_state_");
                use std::fmt::Write;
                for byte in key.bytes() {
                    write!(symbol, "{byte:02x}").unwrap();
                }
                let module = self.builder.module();
                let id = module
                    .declare_data(&symbol, Linkage::Local, true)
                    .map_err(|e| e.to_string())?;
                let mut data = DataDescription::new(image.bytes.clone());
                data.preserve_address = true;
                data.align = Some(u64::from(image.alignment));
                module.define_data(id, data).map_err(|e| e.to_string())?;
                self.tag_cells.insert(intern, id);
                id
            }
        };
        let global = self.builder.import_data(id);
        Ok((
            self.builder.symbol_value(types::I64, global),
            image,
            cell.ty.clone(),
        ))
    }

    pub(super) fn address_of_state(&mut self, key: &str) -> Result<ValueRef, String> {
        let (address, ..) = self.state_cell(key)?;
        Ok(ValueRef {
            values: vec![address],
            ty: DirectType::Scalar(ScalarKind::Uint64),
        })
    }

    pub(super) fn load_state(&mut self, key: &str) -> Result<ValueRef, String> {
        let (address, image, ty) = self.state_cell(key)?;
        let slots = &image.slots;
        let ty = ensure_direct_type(&ty, &self.classes, "module state")?;
        let mut values = Vec::with_capacity(slots.len());
        for (offset, ty) in slots {
            if ty.as_fixed_array().is_some() {
                let storage =
                    ensure_direct_type(ty, &self.classes, "fixed array storage")?.abi_types()[0];
                values.push(
                    self.builder
                        .load(storage, MemFlags::trusted(), address, *offset),
                );
            } else {
                let kind = state_scalar(ty)?;
                values.push(self.load_leaf(
                    plain::PlainLeaf {
                        offset: *offset,
                        kind,
                    },
                    address,
                ));
            }
        }
        Ok(ValueRef { ty, values })
    }

    pub(super) fn store_state(&mut self, key: &str, operand: &Operand) -> Result<ValueRef, String> {
        let (address, image, ty) = self.state_cell(key)?;
        let slots = &image.slots;
        let ty = ensure_direct_type(&ty, &self.classes, "module state")?;
        let value = self.load_operand_for_target(operand, &ty)?;
        let value = self.coerce_value(value, &ty)?;
        if value.values.len() != slots.len() {
            return Err("module storage layout disagrees with its value".into());
        }
        for ((offset, ty), word) in slots.iter().zip(value.values) {
            if ty.as_fixed_array().is_some() {
                self.builder
                    .store(MemFlags::trusted(), word, address, *offset);
            } else {
                let kind = state_scalar(ty)?;
                self.store_leaf(
                    plain::PlainLeaf {
                        offset: *offset,
                        kind,
                    },
                    word,
                    address,
                );
            }
        }
        Ok(unit_value(self.builder))
    }
}

fn state_scalar(ty: &Type) -> Result<ScalarKind, String> {
    if ty == &Type::Unit {
        return Ok(ScalarKind::Unit);
    }
    ScalarKind::of_name(&ty.to_string())
        .ok_or_else(|| format!("module storage component {ty} has no native representation"))
}
