//! Record placement and entry conventions on the backend declarations.
use super::*;

impl NativeCodegen<'_> {
    pub(super) fn define_machine_items(&mut self) -> std::result::Result<(), String> {
        for data in &self.module.foreign_link.placed_data {
            let id = self
                .module_backend()?
                .declare_data(&data.symbol, Linkage::Local, false)?;
            let mut contents = DataDescription::new(data.bytes.clone());
            contents.align = Some(u64::from(
                data.placement.align.unwrap_or(data.bytes.len() as u32),
            ));
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
