use std::fs;
use std::io::Write;
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::process::{self, Command};

use aurora_compiler::{
    analyze_path_source, check_path, check_source, complete_path_source, lower_path_to_mir,
    lower_source_to_mir, parse_source, run_path, run_path_via_mir, run_source, run_source_via_mir,
    Diagnostic, MirModule, Value,
};

struct Input {
    path: String,
    source: String,
    from_stdin: bool,
}

fn main() {
    let mut args = std::env::args().skip(1);
    let Some(command) = args.next() else {
        print_usage_and_exit();
    };
    match command.as_str() {
        "check" => {
            let input = read_input(&mut args);
            let result = if input.from_stdin {
                check_source(&input.source)
            } else {
                check_path(Path::new(&input.path))
            };
            match result {
                Ok(_) => {
                    write_stdout("ok\n");
                }
                Err(error) => {
                    eprintln!("{}", render_error(&input.path, &input.source, &error));
                    process::exit(1);
                }
            }
        }
        "run" => {
            let input = read_input(&mut args);
            let result = if input.from_stdin {
                run_source(&input.source)
            } else {
                run_path(Path::new(&input.path))
            };
            match result {
                Ok(output) => {
                    write_stdout(&output.stdout);
                    if let Value::Int(code) = output.value {
                        process::exit(code as i32);
                    }
                }
                Err(error) => {
                    eprintln!("{}", render_error(&input.path, &input.source, &error));
                    process::exit(1);
                }
            }
        }
        "run-mir" => {
            let input = read_input(&mut args);
            let result = if input.from_stdin {
                run_source_via_mir(&input.source)
            } else {
                run_path_via_mir(Path::new(&input.path))
            };
            match result {
                Ok(output) => {
                    write_stdout(&output.stdout);
                    if let Value::Int(code) = output.value {
                        process::exit(code as i32);
                    }
                }
                Err(error) => {
                    eprintln!("{}", render_error(&input.path, &input.source, &error));
                    process::exit(1);
                }
            }
        }
        "build" => {
            let remaining = args.collect::<Vec<_>>();
            let (output_path, input_args) = parse_build_args(remaining);
            let input = read_input(&mut input_args.into_iter());
            let result = if input.from_stdin {
                lower_source_to_mir(&input.source)
            } else {
                lower_path_to_mir(Path::new(&input.path))
            };
            match result {
                Ok(mir) => {
                    if let Err(message) =
                        build_bootstrap_runner(&input.path, &input.source, &mir, &output_path)
                    {
                        eprintln!("{}", message);
                        process::exit(1);
                    }
                }
                Err(error) => {
                    eprintln!("{}", render_error(&input.path, &input.source, &error));
                    process::exit(1);
                }
            }
        }
        "ast" => {
            let input = read_input(&mut args);
            match parse_source(&input.source) {
                Ok(module) => {
                    write_stdout(&format!("{:#?}\n", module));
                }
                Err(error) => {
                    eprintln!("{}", render_error(&input.path, &input.source, &error));
                    process::exit(1);
                }
            }
        }
        "ast-json" => {
            let input = read_input(&mut args);
            match parse_source(&input.source) {
                Ok(module) => {
                    write_stdout(&format!(
                        "{}",
                        serde_json::to_string_pretty(&module)
                            .expect("AST should serialize to JSON")
                    ));
                    write_stdout("\n");
                }
                Err(error) => {
                    eprintln!("{}", render_error(&input.path, &input.source, &error));
                    process::exit(1);
                }
            }
        }
        "mir" => {
            let input = read_input(&mut args);
            let result = if input.from_stdin {
                lower_source_to_mir(&input.source)
            } else {
                lower_path_to_mir(Path::new(&input.path))
            };
            match result {
                Ok(module) => {
                    write_stdout(&format!("{:#?}\n", module));
                }
                Err(error) => {
                    eprintln!("{}", render_error(&input.path, &input.source, &error));
                    process::exit(1);
                }
            }
        }
        "analyze" => {
            let input = read_input(&mut args);
            let analysis = analyze_path_source(Path::new(&input.path), &input.source);
            write_stdout(&format!(
                "{}",
                serde_json::to_string(&analysis).expect("analysis should serialize to JSON")
            ));
            write_stdout("\n");
        }
        "complete" => {
            let remaining = args.collect::<Vec<_>>();
            let (line, character, trigger, input_args) = parse_complete_args(remaining);
            let input = read_input(&mut input_args.into_iter());
            match complete_path_source(
                Path::new(&input.path),
                &input.source,
                line,
                character,
                trigger,
            ) {
                Ok(completions) => {
                    write_stdout(&format!(
                        "{}",
                        serde_json::to_string(&completions)
                            .expect("completions should serialize to JSON")
                    ));
                    write_stdout("\n");
                }
                Err(error) => {
                    eprintln!("{}", render_error(&input.path, &input.source, &error));
                    process::exit(1);
                }
            }
        }
        _ => print_usage_and_exit(),
    }
}

fn parse_complete_args(args: Vec<String>) -> (usize, usize, Option<char>, Vec<String>) {
    let mut line = None;
    let mut character = None;
    let mut trigger = None;
    let mut index = 0;

    while index < args.len() {
        match args[index].as_str() {
            "--line" => {
                index += 1;
                line = Some(
                    args.get(index)
                        .and_then(|value| value.parse::<usize>().ok())
                        .unwrap_or_else(|| print_usage_and_exit()),
                );
                index += 1;
            }
            "--character" => {
                index += 1;
                character = Some(
                    args.get(index)
                        .and_then(|value| value.parse::<usize>().ok())
                        .unwrap_or_else(|| print_usage_and_exit()),
                );
                index += 1;
            }
            "--trigger" => {
                index += 1;
                trigger = Some(
                    args.get(index)
                        .and_then(|value| value.chars().next())
                        .unwrap_or_else(|| print_usage_and_exit()),
                );
                index += 1;
            }
            _ => break,
        }
    }

    (
        line.unwrap_or_else(|| print_usage_and_exit()),
        character.unwrap_or_else(|| print_usage_and_exit()),
        trigger,
        args[index..].to_vec(),
    )
}

fn parse_build_args(args: Vec<String>) -> (PathBuf, Vec<String>) {
    let mut output = None;
    let mut input_args = Vec::new();
    let mut index = 0;

    while index < args.len() {
        match args[index].as_str() {
            "-o" | "--output" => {
                index += 1;
                output = Some(PathBuf::from(
                    args.get(index)
                        .cloned()
                        .unwrap_or_else(|| print_usage_and_exit()),
                ));
                index += 1;
            }
            _ => {
                input_args.push(args[index].clone());
                index += 1;
            }
        }
    }

    let output = output.unwrap_or_else(|| print_usage_and_exit());
    if input_args.is_empty() {
        print_usage_and_exit();
    }

    (output, input_args)
}

fn read_input(args: &mut impl Iterator<Item = String>) -> Input {
    let Some(first) = args.next() else {
        print_usage_and_exit();
    };

    if first == "--stdin" {
        let Some(virtual_path) = args.next() else {
            print_usage_and_exit();
        };
        if args.next().is_some() {
            print_usage_and_exit();
        }
        let mut source = String::new();
        io::stdin()
            .read_to_string(&mut source)
            .expect("failed to read source from stdin");
        return Input {
            path: virtual_path,
            source,
            from_stdin: true,
        };
    }

    if args.next().is_some() {
        print_usage_and_exit();
    }

    let path = first;
    let source = match fs::read_to_string(&path) {
        Ok(source) => source,
        Err(error) => {
            eprintln!("failed to read `{}`: {}", path, error);
            process::exit(1);
        }
    };

    Input {
        path,
        source,
        from_stdin: false,
    }
}

fn render_error(path: &str, source: &str, error: &Diagnostic) -> String {
    error.render_with_source(path, source)
}

fn build_bootstrap_runner(
    path: &str,
    source: &str,
    mir: &MirModule,
    output_path: &Path,
) -> std::result::Result<(), String> {
    if let Some(parent) = output_path.parent() {
        fs::create_dir_all(parent).map_err(|error| {
            format!(
                "failed to create output directory `{}`: {}",
                parent.display(),
                error
            )
        })?;
    }

    let deps_dir = compiler_deps_dir()?;
    let compiler_rlib = find_compiler_rlib(&deps_dir)?;
    let serde_json_rlib = find_named_rlib(&deps_dir, "serde_json")?;
    let rustc = std::env::var_os("RUSTC").unwrap_or_else(|| "rustc".into());
    let temp_source = temporary_runner_path(output_path);
    let runner_source = render_bootstrap_runner(path, source, mir);

    fs::write(&temp_source, runner_source).map_err(|error| {
        format!(
            "failed to write temporary runner source `{}`: {}",
            temp_source.display(),
            error
        )
    })?;

    let result = Command::new(rustc)
        .arg("--edition=2021")
        .arg("-L")
        .arg(format!("dependency={}", deps_dir.display()))
        .arg("--extern")
        .arg(format!("aurora_compiler={}", compiler_rlib.display()))
        .arg("--extern")
        .arg(format!("serde_json={}", serde_json_rlib.display()))
        .arg("-o")
        .arg(output_path)
        .arg(&temp_source)
        .output()
        .map_err(|error| format!("failed to run rustc for `aura build`: {}", error));

    let _ = fs::remove_file(&temp_source);

    let output = result?;
    if !output.status.success() {
        return Err(format!(
            "bootstrap build failed:\n{}",
            String::from_utf8_lossy(&output.stderr)
        ));
    }

    Ok(())
}

fn temporary_runner_path(output_path: &Path) -> PathBuf {
    let file_name = output_path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("aurora-output");
    let unique = format!(
        "aurora-build-{}-{}-{}.rs",
        file_name,
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("system time should be after unix epoch")
            .as_nanos()
    );
    std::env::temp_dir().join(unique)
}

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|path| path.parent())
        .expect("workspace root should be available")
        .to_path_buf()
}

fn compiler_deps_dir() -> std::result::Result<PathBuf, String> {
    let mut candidates = Vec::new();

    if let Ok(current_exe) = std::env::current_exe() {
        if let Some(parent) = current_exe.parent() {
            if parent.file_name().and_then(|name| name.to_str()) == Some("deps") {
                candidates.push(parent.to_path_buf());
            } else {
                candidates.push(parent.join("deps"));
            }
        }
    }

    let repo_target = repo_root().join("target");
    candidates.push(repo_target.join("debug").join("deps"));
    candidates.push(repo_target.join("release").join("deps"));

    for candidate in candidates {
        if find_compiler_rlib_in_dir(&candidate).is_some() {
            return Ok(candidate);
        }
    }

    Err(format!(
        "failed to locate Cargo dependency artifacts for `aurora_compiler`; run `cargo build -p aura` first"
    ))
}

fn find_compiler_rlib(deps_dir: &Path) -> std::result::Result<PathBuf, String> {
    find_named_rlib(deps_dir, "aurora_compiler")
}

fn find_named_rlib(deps_dir: &Path, crate_name: &str) -> std::result::Result<PathBuf, String> {
    find_named_rlib_in_dir(deps_dir, crate_name).ok_or_else(|| {
        format!(
            "failed to locate `{}` build artifacts in `{}`; run `cargo build -p aura` first",
            crate_name,
            deps_dir.display(),
        )
    })
}

fn find_compiler_rlib_in_dir(deps_dir: &Path) -> Option<PathBuf> {
    find_named_rlib_in_dir(deps_dir, "aurora_compiler")
}

fn find_named_rlib_in_dir(deps_dir: &Path, crate_name: &str) -> Option<PathBuf> {
    let prefix = format!("lib{}-", crate_name.replace('-', "_"));
    let mut candidates = fs::read_dir(deps_dir)
        .ok()?
        .filter_map(|entry| entry.ok())
        .filter_map(|entry| {
            let path = entry.path();
            let name = path.file_name()?.to_str()?;
            if !name.starts_with(&prefix) || !name.ends_with(".rlib") {
                return None;
            }
            let modified = entry
                .metadata()
                .ok()
                .and_then(|metadata| metadata.modified().ok());
            Some((modified, path))
        })
        .collect::<Vec<_>>();

    candidates.sort_by(|left, right| right.0.cmp(&left.0));
    candidates.into_iter().map(|(_, path)| path).next()
}

fn render_bootstrap_runner(path: &str, source: &str, mir: &MirModule) -> String {
    let mir_json = serde_json::to_string(mir).expect("MIR should serialize to JSON");
    format!(
        r#"use std::io::{{self, Write}};
use std::process;

const MIR_JSON: &str = {mir_json_literal};
const SOURCE_PATH: &str = {path_literal};
const SOURCE: &str = {source_literal};

fn write_stdout(text: &str) {{
    let mut stdout = io::stdout().lock();
    if let Err(error) = stdout.write_all(text.as_bytes()).and_then(|_| stdout.flush()) {{
        if error.kind() == io::ErrorKind::BrokenPipe {{
            process::exit(0);
        }}
        eprintln!("failed to write to stdout: {{}}", error);
        process::exit(1);
    }}
}}

fn main() {{
    let mir: aurora_compiler::MirModule =
        serde_json::from_str(MIR_JSON).expect("embedded MIR should deserialize");
    match aurora_compiler::run_mir(&mir) {{
        Ok(output) => {{
            write_stdout(&output.stdout);
            if let aurora_compiler::Value::Int(code) = output.value {{
                process::exit(code as i32);
            }}
        }}
        Err(error) => {{
            eprintln!("{{}}", error.render_with_source(SOURCE_PATH, SOURCE));
            process::exit(1);
        }}
    }}
}}
"#,
        mir_json_literal = format!("{:?}", mir_json),
        source_literal = format!("{:?}", source),
        path_literal = format!("{:?}", path),
    )
}

fn write_stdout(text: &str) {
    let mut stdout = io::stdout().lock();
    if let Err(error) = stdout
        .write_all(text.as_bytes())
        .and_then(|_| stdout.flush())
    {
        if error.kind() == io::ErrorKind::BrokenPipe {
            process::exit(0);
        }
        eprintln!("failed to write to stdout: {}", error);
        process::exit(1);
    }
}

fn print_usage_and_exit() -> ! {
    eprintln!("usage: aura <check|run|run-mir|build|ast|ast-json|mir|analyze> <file.au>");
    eprintln!(
        "   or: aura <check|run|run-mir|build|ast|ast-json|mir|analyze> --stdin <virtual-path>"
    );
    eprintln!("   or: aura build -o <output> <file.au>");
    eprintln!("   or: aura build -o <output> --stdin <virtual-path>");
    eprintln!("   or: aura complete --line <n> --character <n> [--trigger .] <file.au>");
    eprintln!(
        "   or: aura complete --line <n> --character <n> [--trigger .] --stdin <virtual-path>"
    );
    process::exit(2);
}

#[cfg(test)]
mod tests {
    use aurora_compiler::lower_source_to_mir;

    use super::render_bootstrap_runner;

    #[test]
    fn bootstrap_runner_uses_mir_first_backend_path() {
        let mir = lower_source_to_mir("print(value=1)\n").expect("source should lower to MIR");
        let runner = render_bootstrap_runner("/virtual/test.au", "print(value=1)\n", &mir);
        assert!(
            runner.contains("aurora_compiler::run_mir(&mir)"),
            "bootstrap runner should execute embedded MIR directly"
        );
        assert!(
            runner.contains("const MIR_JSON: &str"),
            "bootstrap runner should embed serialized MIR"
        );
    }
}
