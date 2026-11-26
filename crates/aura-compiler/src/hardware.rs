//! Target-specific Aura source for privileged operations in `mem` (ADR-0147).
use crate::diag::{Diagnostic, Result, Span};
use crate::FreestandingTarget;
use std::collections::BTreeMap;
use std::sync::OnceLock;

pub(crate) fn source(target: FreestandingTarget) -> &'static str {
    match target {
        FreestandingTarget::X86_64 | FreestandingTarget::X86_64Kernel => {
            include_str!("../std/mem_x86_64.au")
        }
        FreestandingTarget::Aarch64 => include_str!("../std/mem_aarch64.au"),
        FreestandingTarget::Riscv64 => include_str!("../std/mem_riscv64.au"),
    }
}

/// An otherwise missing `mem` member can be a machine operation from a
/// different profile. Use the declarations themselves as the name catalogue.
pub(crate) fn check_missing_member(module: &str, member: &str, span: Span) -> Result<()> {
    if module == "mem" {
        static TARGETS: OnceLock<BTreeMap<String, Vec<&'static str>>> = OnceLock::new();
        let targets = TARGETS.get_or_init(|| {
            let mut targets = BTreeMap::<String, Vec<&'static str>>::new();
            for target in FreestandingTarget::ALL {
                let module =
                    crate::parse_source(source(target)).expect("the hardware library parses");
                for item in module.items {
                    targets
                        .entry(item.name().to_owned())
                        .or_default()
                        .push(target.name());
                }
            }
            targets
        });
        if let Some(supported) = targets.get(member) {
            return Err(Diagnostic::coded_at(
                "AU2097",
                span,
                format!(
                    "mem.{member} requires a freestanding build for {}",
                    supported.join(", ")
                ),
            ));
        }
    }
    Ok(())
}
