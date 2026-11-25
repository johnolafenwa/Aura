//! Target selection shared by commands that accept freestanding source.
use aura_compiler::FreestandingTarget;

/// Removes profile and target flags, leaving the command's remaining arguments.
pub(super) fn take_target(args: Vec<String>) -> (FreestandingTarget, Vec<String>) {
    let (name, mut rest) = super::take_build_target(args).unwrap_or_else(|message| {
        eprintln!("aura: {message}");
        std::process::exit(2);
    });
    let kernel = rest.iter().any(|arg| arg == "--kernel");
    rest.retain(|arg| arg != "--freestanding" && arg != "--kernel");

    let target = select_target(name.as_deref(), kernel).unwrap_or_else(|message| {
        eprintln!("aura: {message}");
        std::process::exit(2);
    });
    (target, rest)
}

fn select_target(name: Option<&str>, kernel: bool) -> Result<FreestandingTarget, String> {
    let known = || {
        FreestandingTarget::ALL
            .iter()
            .map(|target| format!("`{}`", target.name()))
            .collect::<Vec<_>>()
            .join(", ")
    };
    let target = match name {
        Some(name) => FreestandingTarget::parse(name).ok_or_else(|| {
            format!(
                "unknown freestanding target `{name}`; the targets are {}",
                known()
            )
        })?,
        None => FreestandingTarget::host().ok_or_else(|| {
            format!(
                "no freestanding target matches this machine; pass `--target` with one of {}",
                known()
            )
        })?,
    };
    match (kernel, target) {
        (false, arch) => Ok(arch),
        (true, FreestandingTarget::X86_64) => Ok(FreestandingTarget::X86_64Kernel),
        (true, _) => Err("`--kernel` requires the x86_64-unknown-none freestanding target".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selects_each_target_and_kernel_combination() {
        assert_eq!(select_target(None, false).ok(), FreestandingTarget::host());
        for arch in FreestandingTarget::ALL {
            assert_eq!(select_target(Some(arch.name()), false), Ok(arch));
            assert_eq!(
                select_target(Some(arch.name()), true).is_ok(),
                arch == FreestandingTarget::X86_64
            );
        }
        assert!(select_target(Some("invalid"), false)
            .unwrap_err()
            .contains("riscv64gc"));
        let args = [
            "--kernel",
            "kernel.au",
            "--target",
            "x86_64-unknown-none",
            "--freestanding",
        ];
        let (target, rest) = take_target(args.map(String::from).to_vec());
        assert_eq!(target, FreestandingTarget::X86_64Kernel);
        assert_eq!(rest, ["kernel.au"]);
    }
}
