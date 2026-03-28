use std::fs;
use std::io::Write;
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::process::{self, Command};

use aurora_compiler::{
    analyze_path_source, check_path, check_path_with_source, complete_path_source,
    emit_host_native_object, lower_path_to_mir, lower_path_with_source_to_mir, parse_source,
    run_path, run_path_via_mir, run_path_with_source, run_path_with_source_via_mir, Diagnostic,
    MirModule, Value,
};

struct Input {
    path: String,
    source: String,
    from_stdin: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum BuildBackend {
    Auto,
    Direct,
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
                check_path_with_source(Path::new(&input.path), &input.source)
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
                run_path_with_source(Path::new(&input.path), &input.source)
            } else {
                run_path(Path::new(&input.path))
            };
            match result {
                Ok(output) => {
                    write_stdout(&output.stdout);
                    if let Value::Int(code) = output.value {
                        process::exit(code.as_i128().unwrap_or(1) as i32);
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
                run_path_with_source_via_mir(Path::new(&input.path), &input.source)
            } else {
                run_path_via_mir(Path::new(&input.path))
            };
            match result {
                Ok(output) => {
                    write_stdout(&output.stdout);
                    if let Value::Int(code) = output.value {
                        process::exit(code.as_i128().unwrap_or(1) as i32);
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
            let (output_path, backend, input_args) = parse_build_args(remaining);
            let input = read_input(&mut input_args.into_iter());
            let result = if input.from_stdin {
                lower_path_with_source_to_mir(Path::new(&input.path), &input.source)
            } else {
                lower_path_to_mir(Path::new(&input.path))
            };
            match result {
                Ok(mir) => {
                    if let Err(message) = build_binary_with_backend(
                        &input.path,
                        &input.source,
                        &mir,
                        &output_path,
                        backend,
                    ) {
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
                lower_path_with_source_to_mir(Path::new(&input.path), &input.source)
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

fn parse_build_args(args: Vec<String>) -> (PathBuf, BuildBackend, Vec<String>) {
    let mut output = None;
    let mut backend = BuildBackend::Auto;
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
            "--backend" => {
                index += 1;
                let value = args
                    .get(index)
                    .cloned()
                    .unwrap_or_else(|| print_usage_and_exit());
                backend = match value.as_str() {
                    "auto" => BuildBackend::Auto,
                    "direct" => BuildBackend::Direct,
                    _ => print_usage_and_exit(),
                };
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

    (output, backend, input_args)
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

fn build_binary_with_backend(
    _path: &str,
    _source: &str,
    mir: &MirModule,
    output_path: &Path,
    backend: BuildBackend,
) -> std::result::Result<(), String> {
    match backend {
        BuildBackend::Direct => build_direct_native_binary(mir, output_path),
        BuildBackend::Auto => build_direct_native_binary(mir, output_path),
    }
}

fn build_direct_native_binary(
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

    let native_runtime = ensure_native_runtime_artifacts()?;
    let object_bytes = emit_host_native_object(mir)?;
    let temp_object = temporary_direct_object_path(output_path);
    let temp_staticlib = temporary_direct_staticlib_path(output_path);
    fs::write(&temp_object, object_bytes).map_err(|error| {
        format!(
            "failed to write direct backend object `{}`: {}",
            temp_object.display(),
            error
        )
    })?;
    fs::copy(&native_runtime.staticlib, &temp_staticlib).map_err(|error| {
        format!(
            "failed to stage Aurora runtime library `{}` as `{}`: {}",
            native_runtime.staticlib.display(),
            temp_staticlib.display(),
            error
        )
    })?;

    let cc = std::env::var_os("CC").unwrap_or_else(|| "cc".into());
    let mut command = Command::new(cc);
    command
        .arg(&temp_object)
        .arg(&temp_staticlib)
        .arg("-o")
        .arg(output_path);
    for arg in &native_runtime.native_link_args {
        command.arg(arg);
    }

    let result = command
        .output()
        .map_err(|error| format!("failed to run native linker for direct backend: {}", error));

    let _ = fs::remove_file(&temp_object);
    let _ = fs::remove_file(&temp_staticlib);

    let output = result?;
    if !output.status.success() {
        return Err(format!(
            "direct backend link failed:\n{}",
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    Ok(())
}

fn temporary_direct_object_path(output_path: &Path) -> PathBuf {
    let file_name = output_path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("aurora-output");
    let unique = format!(
        "aurora-direct-object-{}-{}-{}.o",
        file_name,
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("system time should be after unix epoch")
            .as_nanos()
    );
    std::env::temp_dir().join(unique)
}

fn temporary_direct_staticlib_path(output_path: &Path) -> PathBuf {
    let file_name = output_path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("aurora-output");
    let unique = format!(
        "aurora-direct-runtime-{}-{}-{}.a",
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

struct NativeRuntimeArtifacts {
    staticlib: PathBuf,
    native_link_args: Vec<String>,
}

fn ensure_native_runtime_artifacts() -> std::result::Result<NativeRuntimeArtifacts, String> {
    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    let mut command = Command::new(cargo);
    command.current_dir(repo_root());
    command
        .arg("rustc")
        .arg("-q")
        .arg("-p")
        .arg("aurora-compiler")
        .arg("--lib");
    if current_profile() == "release" {
        command.arg("--release");
    }
    command.arg("--").arg("--print").arg("native-static-libs");

    let output = command
        .output()
        .map_err(|error| format!("failed to build Aurora runtime artifacts: {}", error))?;

    if !output.status.success() {
        return Err(format!(
            "failed to build Aurora runtime artifacts:\n{}",
            String::from_utf8_lossy(&output.stderr)
        ));
    }

    let staticlib = resolve_static_library_path(repo_root(), current_profile())?;
    if !staticlib.exists() {
        return Err(format!(
            "failed to locate compiled Aurora runtime library `{}` after build",
            staticlib.display()
        ));
    }

    let native_link_args = parse_native_static_libs(&String::from_utf8_lossy(&output.stderr));

    Ok(NativeRuntimeArtifacts {
        staticlib,
        native_link_args,
    })
}

fn current_profile() -> &'static str {
    if cfg!(debug_assertions) {
        "debug"
    } else {
        "release"
    }
}

fn static_library_file_name() -> &'static str {
    "libaurora_compiler.a"
}

fn resolve_static_library_path(
    root: PathBuf,
    profile: &str,
) -> std::result::Result<PathBuf, String> {
    let primary = root
        .join("target")
        .join(profile)
        .join(static_library_file_name());
    if primary.exists() {
        return Ok(primary);
    }

    let deps_dir = root.join("target").join(profile).join("deps");
    let mut candidates = fs::read_dir(&deps_dir)
        .map_err(|error| {
            format!(
                "failed to inspect Aurora runtime library directory `{}`: {}",
                deps_dir.display(),
                error
            )
        })?
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .map(|name| name.starts_with("libaurora_compiler-") && name.ends_with(".a"))
                .unwrap_or(false)
        })
        .collect::<Vec<_>>();
    candidates.sort_by_key(|path| {
        fs::metadata(path)
            .and_then(|metadata| metadata.modified())
            .ok()
    });
    if let Some(candidate) = candidates.pop() {
        return Ok(candidate);
    }

    Err(format!(
        "failed to locate compiled Aurora runtime library `{}` or a matching archive in `{}`",
        primary.display(),
        deps_dir.display()
    ))
}

fn parse_native_static_libs(output: &str) -> Vec<String> {
    output
        .lines()
        .rev()
        .find_map(|line| line.split_once("native-static-libs:"))
        .map(|(_, libs)| {
            libs.split_whitespace()
                .map(|item| item.to_string())
                .collect()
        })
        .unwrap_or_default()
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
    eprintln!("   or: aura build [-o <output>] [--backend auto|direct] <file.au>");
    eprintln!("   or: aura build [-o <output>] [--backend auto|direct] --stdin <virtual-path>");
    eprintln!("   or: aura complete --line <n> --character <n> [--trigger .] <file.au>");
    eprintln!(
        "   or: aura complete --line <n> --character <n> [--trigger .] --stdin <virtual-path>"
    );
    process::exit(2);
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;
    use std::thread;
    use std::time::{Duration, SystemTime, UNIX_EPOCH};

    use super::resolve_static_library_path;

    fn unique_temp_dir(name: &str) -> PathBuf {
        let unique = format!(
            "aurora-aura-tests-{}-{}-{}",
            name,
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("system time should be after unix epoch")
                .as_nanos()
        );
        let path = std::env::temp_dir().join(unique);
        fs::create_dir_all(&path).expect("temp dir should exist");
        path
    }

    #[test]
    fn resolve_static_library_path_prefers_primary_staticlib() {
        let root = unique_temp_dir("primary-staticlib");
        let target = root.join("target").join("debug");
        let deps = target.join("deps");
        fs::create_dir_all(&deps).expect("deps dir should exist");
        let primary = target.join("libaurora_compiler.a");
        fs::write(&primary, b"primary").expect("primary staticlib should write");
        fs::write(
            deps.join("libaurora_compiler-old.a"),
            b"stale hashed archive",
        )
        .expect("hashed archive should write");

        let resolved = resolve_static_library_path(root.clone(), "debug")
            .expect("should resolve runtime library");
        assert_eq!(resolved, primary);

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn resolve_static_library_path_uses_newest_hashed_archive_when_primary_missing() {
        let root = unique_temp_dir("newest-hashed");
        let deps = root.join("target").join("debug").join("deps");
        fs::create_dir_all(&deps).expect("deps dir should exist");
        let older = deps.join("libaurora_compiler-older.a");
        fs::write(&older, b"older").expect("older archive should write");
        thread::sleep(Duration::from_millis(10));
        let newer = deps.join("libaurora_compiler-newer.a");
        fs::write(&newer, b"newer").expect("newer archive should write");

        let resolved = resolve_static_library_path(root.clone(), "debug")
            .expect("should resolve newest hashed runtime library");
        assert_eq!(resolved, newer);

        let _ = fs::remove_dir_all(root);
    }
}
