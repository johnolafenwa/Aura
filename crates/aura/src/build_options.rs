use aura_compiler::{current_build_config, BuildConfig, Diagnostic, FreestandingTarget};
use std::path::Path;

/// Extract compiler flags without consuming the Aura program's own arguments.
pub(super) fn take_cfg(
    args: Vec<String>,
    config: &mut BuildConfig,
) -> aura_compiler::Result<Vec<String>> {
    let mut args = args.into_iter();
    let mut rest = Vec::new();
    while let Some(arg) = args.next() {
        if arg == "--" {
            rest.push(arg);
            rest.extend(args);
            break;
        }
        if arg == "--cfg" {
            let value = args
                .next()
                .filter(|value| !value.starts_with("--"))
                .ok_or_else(|| {
                    Diagnostic::coded("AU2070", "`--cfg` requires a name or name=value")
                })?;
            config.apply_option(&value)?;
        } else if let Some(value) = arg.strip_prefix("--cfg=") {
            config.apply_option(value)?;
        } else {
            rest.push(arg);
        }
    }
    Ok(rest)
}

pub(super) fn target_scope(
    target: Option<&str>,
    debug: bool,
) -> aura_compiler::build_config::BuildConfigGuard {
    let current = current_build_config();
    let mut next = target
        .and_then(|name| BuildConfig::for_target(name, debug))
        .unwrap_or_default();
    next.cfg = current.cfg;
    next.release = !debug;
    next.debug = debug;
    next.enter()
}

pub(super) fn bare_host_scope() -> aura_compiler::build_config::BuildConfigGuard {
    target_scope(
        FreestandingTarget::host().map(|target| target.name()),
        false,
    )
}

pub(super) fn package_scope(
    path: &Path,
    format: super::DiagnosticFormat,
) -> aura_compiler::build_config::BuildConfigGuard {
    BuildConfig::for_path(path)
        .unwrap_or_else(|error| {
            super::emit_diagnostic(format, &path.display().to_string(), "", &error);
            std::process::exit(1);
        })
        .enter()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compiler_cfg_stops_at_program_argument_separator() {
        let mut config = BuildConfig::default();
        let args = [
            "--cfg",
            "first",
            "file.au",
            "--cfg=other=",
            "--",
            "--cfg",
            "last",
        ]
        .map(String::from);
        let rest = take_cfg(args.into(), &mut config).unwrap();
        assert_eq!(rest, ["file.au", "--", "--cfg", "last"]);
        assert_eq!(config.cfg["first"], "true");
        assert_eq!(config.cfg["other"], "");
        assert!(!config.cfg.contains_key("last"));
        for args in [vec!["--cfg"], vec!["--cfg", "--"]] {
            assert_eq!(
                take_cfg(args.into_iter().map(String::from).collect(), &mut config)
                    .unwrap_err()
                    .code,
                "AU2070"
            );
        }
    }

    #[test]
    fn target_scope_retains_explicit_flags_and_changes_profile() {
        let mut config = current_build_config();
        config.apply_option("trace").unwrap();
        let _config = config.enter();
        let _target = target_scope(Some("x86_64-unknown-linux-musl"), true);
        let target = current_build_config();
        assert!(target.debug && !target.release);
        assert_eq!(target.target, "x86_64-unknown-linux-musl");
        assert_eq!(target.cfg["trace"], "true");
        let _bare = bare_host_scope();
        assert!(current_build_config().freestanding);
    }
}
