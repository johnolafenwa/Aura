//! LLVM configuration for a freestanding x86-64 kernel object (ADR-0148).
use super::{Attribute, AttributeLoc, CodeModel, Context, Module};

pub(super) const TARGET: (&str, &str, &str, Option<&str>, CodeModel) = (
    "x86_64-unknown-none-elf",
    "x86-64",
    "-mmx,-sse,-sse2,-x87,+soft-float",
    None,
    CodeModel::Kernel,
);

/// Mark functions after optimization to cover helpers created by the passes.
pub(super) fn disable_red_zone(context: &Context, module: &Module<'_>) {
    let kind = Attribute::get_named_enum_kind_id("noredzone");
    let attribute = context.create_enum_attribute(kind, 0);
    for function in module.get_functions() {
        function.add_attribute(AttributeLoc::Function, attribute);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_function_disables_the_red_zone() {
        let context = Context::create();
        let module = context.create_module("kernel");
        let signature = context.void_type().fn_type(&[], false);
        for name in ["entry", "memcpy"] {
            module.add_function(name, signature, None);
        }
        disable_red_zone(&context, &module);
        assert!(module.get_functions().all(|function| function
            .get_enum_attribute(
                AttributeLoc::Function,
                Attribute::get_named_enum_kind_id("noredzone")
            )
            .is_some()));
    }
}
