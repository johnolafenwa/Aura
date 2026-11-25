//! Check each target without invoking a target linker or executing a program.

use super::{emit_diagnostics, write_stdout, DiagnosticFormat, Input};
use aura_compiler::{
    check_path, check_path_with_source, current_build_config, enter_freestanding_target,
    with_build_config, FreestandingTarget,
};
use std::path::Path;

pub(super) fn check(input: &Input, format: DiagnosticFormat) {
    let path = Path::new(&input.path);
    let mut diagnostics = Vec::new();
    for (name, config) in current_build_config().supported_targets() {
        let result = with_build_config(config, || {
            let _target = FreestandingTarget::parse(&name).map(enter_freestanding_target);
            if input.from_stdin {
                check_path_with_source(path, &input.source)
            } else {
                check_path(path)
            }
        });
        if let Err(error) = result {
            diagnostics.push(error.with_note(format!("while checking target `{name}`")));
        }
    }
    if !diagnostics.is_empty() {
        emit_diagnostics(format, &input.path, &input.source, diagnostics);
        std::process::exit(1);
    }
    match format {
        DiagnosticFormat::Human => write_stdout("ok\n"),
        DiagnosticFormat::Json => write_stdout("{\"schema_version\":1,\"diagnostics\":[]}\n"),
    }
}
