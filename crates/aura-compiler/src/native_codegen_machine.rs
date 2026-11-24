//! Record placement and entry conventions on the backend declarations.
use super::*;

impl NativeCodegen<'_> {
    pub(super) fn define_machine_items(&mut self) -> std::result::Result<(), String> {
        for data in &self.module.foreign_link.placed_data {
            let (bytes, align) = if let Some(key) = &data.state_key {
                let image = self
                    .module
                    .constants
                    .iter()
                    .find(|cell| cell.key == *key)
                    .and_then(|cell| cell.state.as_ref())
                    .ok_or_else(|| {
                        format!("missing initializer for placed module state `{key}`")
                    })?;
                (
                    &image.bytes,
                    image.alignment.max(data.placement.align.unwrap_or(1)),
                )
            } else {
                (
                    &data.bytes,
                    data.placement.align.unwrap_or(data.bytes.len() as u32),
                )
            };
            let id = self.module_backend()?.declare_data(
                &data.symbol,
                Linkage::Local,
                data.state_key.is_some(),
            )?;
            if let Some(key) = &data.state_key {
                // Subsequent loads, stores and address-of operations reuse this definition.
                self.tag_cells.insert(format!("\0module state {key}"), id);
            }
            let mut contents = DataDescription::new(bytes.clone());
            contents.align = Some(u64::from(align));
            contents.placement = Some(data.placement.clone());
            self.module_backend()?.define_data(id, contents)?;
        }
        for (name, attrs) in &self.module.foreign_link.machine {
            if let Some(&id) = self.functions.get(name) {
                let exported = self
                    .module
                    .foreign_link
                    .exports
                    .iter()
                    .any(|export| export.function == *name);
                let body = if exported {
                    crate::machine_attributes::MachineAttributes {
                        inline: attrs.inline,
                        ..Default::default()
                    }
                } else {
                    attrs.clone()
                };
                self.module_backend()?.mark_machine(id, body);
            }
        }
        Ok(())
    }
}
