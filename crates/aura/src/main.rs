use std::fs;
use std::io::Write;
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::process::{self, Command};

use aurora_compiler::{
    analyze_path_source, check_path, check_path_with_source, complete_path_source,
    emit_host_native_object_with_metadata, lower_path_to_mir, lower_path_with_source_to_mir,
    parse_source, run_path, run_path_with_source, update_git_dependencies_in_working_dir,
    Diagnostic, MirModule, Value,
};
use serde_json::Value as JsonValue;

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
        print_usage_and_exit(2);
    };
    match command.as_str() {
        "help" | "--help" | "-h" => print_usage_and_exit(0),
        "version" | "--version" | "-V" => print_version_and_exit(),
        "deps" => {
            let remaining = args.collect::<Vec<_>>();
            handle_deps_command(remaining);
        }
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
                    if let Some(stdout) = error.partial_stdout() {
                        write_stdout(stdout);
                    }
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
                    match serde_json::to_string_pretty(&module) {
                        Ok(json) => write_stdout(&json),
                        Err(error) => {
                            eprintln!("failed to serialize AST to JSON: {}", error);
                            process::exit(1);
                        }
                    }
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
            match serde_json::to_string(&analysis) {
                Ok(json) => write_stdout(&json),
                Err(error) => {
                    eprintln!("failed to serialize analysis to JSON: {}", error);
                    process::exit(1);
                }
            }
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
                    match serde_json::to_string(&completions) {
                        Ok(json) => write_stdout(&json),
                        Err(error) => {
                            eprintln!("failed to serialize completions to JSON: {}", error);
                            process::exit(1);
                        }
                    }
                    write_stdout("\n");
                }
                Err(error) => {
                    eprintln!("{}", render_error(&input.path, &input.source, &error));
                    process::exit(1);
                }
            }
        }
        _ => print_usage_and_exit(2),
    }
}

fn handle_deps_command(args: Vec<String>) -> ! {
    let Some(subcommand) = args.first() else {
        print_usage_and_exit(2);
    };
    if subcommand != "update" || args.len() > 2 {
        print_usage_and_exit(2);
    }

    let target_package = args.get(1).map(String::as_str);
    let current_dir = std::env::current_dir().unwrap_or_else(|error| {
        eprintln!("failed to determine current directory: {}", error);
        process::exit(1);
    });

    match update_git_dependencies_in_working_dir(&current_dir, target_package) {
        Ok(result) => {
            if result.updated_packages.is_empty() {
                write_stdout("Aurora.lock is already up to date\n");
            } else {
                for package in result.updated_packages {
                    write_stdout(&format!("updated {}\n", package));
                }
            }
            process::exit(0);
        }
        Err(error) => {
            eprintln!("error: {}", error.message);
            process::exit(1);
        }
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
                        .unwrap_or_else(|| print_usage_and_exit(2)),
                );
                index += 1;
            }
            "--character" => {
                index += 1;
                character = Some(
                    args.get(index)
                        .and_then(|value| value.parse::<usize>().ok())
                        .unwrap_or_else(|| print_usage_and_exit(2)),
                );
                index += 1;
            }
            "--trigger" => {
                index += 1;
                trigger = Some(
                    args.get(index)
                        .and_then(|value| value.chars().next())
                        .unwrap_or_else(|| print_usage_and_exit(2)),
                );
                index += 1;
            }
            _ => break,
        }
    }

    (
        line.unwrap_or_else(|| print_usage_and_exit(2)),
        character.unwrap_or_else(|| print_usage_and_exit(2)),
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
                        .unwrap_or_else(|| print_usage_and_exit(2)),
                ));
                index += 1;
            }
            "--backend" => {
                index += 1;
                let value = args
                    .get(index)
                    .cloned()
                    .unwrap_or_else(|| print_usage_and_exit(2));
                backend = match value.as_str() {
                    "auto" => BuildBackend::Auto,
                    "direct" => BuildBackend::Direct,
                    _ => print_usage_and_exit(2),
                };
                index += 1;
            }
            _ => {
                input_args.push(args[index].clone());
                index += 1;
            }
        }
    }

    let output = output.unwrap_or_else(|| print_usage_and_exit(2));
    if input_args.is_empty() {
        print_usage_and_exit(2);
    }

    (output, backend, input_args)
}

fn read_input(args: &mut impl Iterator<Item = String>) -> Input {
    let Some(first) = args.next() else {
        print_usage_and_exit(2);
    };

    if first == "--stdin" {
        let Some(virtual_path) = args.next() else {
            print_usage_and_exit(2);
        };
        if args.next().is_some() {
            print_usage_and_exit(2);
        }
        let mut source = String::new();
        if let Err(error) = io::stdin().read_to_string(&mut source) {
            eprintln!("failed to read source from stdin: {}", error);
            process::exit(1);
        }
        return Input {
            path: virtual_path,
            source,
            from_stdin: true,
        };
    }

    if args.next().is_some() {
        print_usage_and_exit(2);
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
    path: &str,
    source: &str,
    mir: &MirModule,
    output_path: &Path,
    backend: BuildBackend,
) -> std::result::Result<(), String> {
    match backend {
        BuildBackend::Direct => build_direct_native_binary(path, source, mir, output_path),
        BuildBackend::Auto => match build_direct_native_binary(path, source, mir, output_path) {
            Ok(()) => Ok(()),
            Err(_) => build_mir_runtime_binary(path, source, mir, output_path),
        },
    }
}

fn build_direct_native_binary(
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

    let native_runtime = ensure_native_runtime_artifacts()?;
    let object_bytes = emit_host_native_object_with_metadata(mir, path, source)?;
    let temp_object = temporary_direct_object_path(output_path);
    let temp_staticlib = temporary_direct_staticlib_path(output_path);
    fs::write(&temp_object, object_bytes).map_err(|error| {
        format!(
            "failed to write direct backend object `{}`: {}",
            temp_object.display(),
            error
        )
    })?;
    let staticlib_bytes = fs::read(&native_runtime.staticlib).or_else(|_| {
        resolve_static_library_path(repo_root(), current_profile()).and_then(|refreshed| {
            fs::read(&refreshed).map_err(|error| {
                format!(
                    "failed to read Aurora runtime library `{}`: {}",
                    refreshed.display(),
                    error
                )
            })
        })
    })?;
    fs::write(&temp_staticlib, staticlib_bytes).map_err(|error| {
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

fn build_mir_runtime_binary(
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

    let native_runtime = ensure_native_runtime_artifacts()?;
    let mir_json =
        serde_json::to_vec(mir).map_err(|error| format!("failed to serialize MIR: {}", error))?;
    let launcher_source =
        emit_mir_runtime_launcher_source(&mir_json, path.as_bytes(), source.as_bytes());
    let temp_source = temporary_mir_runtime_source_path(output_path);
    let temp_staticlib = temporary_direct_staticlib_path(output_path);
    write_unique_temp_file(
        &temp_source,
        launcher_source.as_bytes(),
        "MIR runtime launcher source",
    )?;
    let staticlib_bytes = fs::read(&native_runtime.staticlib).or_else(|_| {
        resolve_static_library_path(repo_root(), current_profile()).and_then(|refreshed| {
            fs::read(&refreshed).map_err(|error| {
                format!(
                    "failed to read Aurora runtime library `{}`: {}",
                    refreshed.display(),
                    error
                )
            })
        })
    })?;
    write_unique_temp_file(
        &temp_staticlib,
        &staticlib_bytes,
        "staged Aurora runtime library",
    )?;

    let cc = std::env::var_os("CC").unwrap_or_else(|| "cc".into());
    let mut command = Command::new(cc);
    command
        .arg(&temp_source)
        .arg(&temp_staticlib)
        .arg("-o")
        .arg(output_path);
    for arg in &native_runtime.native_link_args {
        command.arg(arg);
    }

    let result = command.output().map_err(|error| {
        format!(
            "failed to run native linker for MIR runtime backend: {}",
            error
        )
    });

    let _ = fs::remove_file(&temp_source);
    let _ = fs::remove_file(&temp_staticlib);

    let output = result?;
    if !output.status.success() {
        return Err(format!(
            "MIR runtime backend link failed:\n{}",
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
        system_time_nanos()
    );
    std::env::temp_dir().join(unique)
}

fn temporary_mir_runtime_source_path(output_path: &Path) -> PathBuf {
    let file_name = output_path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("aurora-output");
    let unique = format!(
        "aurora-mir-runtime-{}-{}-{}.c",
        file_name,
        std::process::id(),
        system_time_nanos()
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
        system_time_nanos()
    );
    std::env::temp_dir().join(unique)
}

fn write_unique_temp_file(path: &Path, contents: &[u8], description: &str) -> Result<(), String> {
    write_unique_temp_file_with_writer(path, description, |file| file.write_all(contents))
}

fn write_unique_temp_file_with_writer(
    path: &Path,
    description: &str,
    writer: impl FnOnce(&mut fs::File) -> io::Result<()>,
) -> Result<(), String> {
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|error| {
            format!(
                "failed to create {} `{}`: {}",
                description,
                path.display(),
                error
            )
        })?;

    let write_result = writer(&mut file).map_err(|error| {
        format!(
            "failed to write {} `{}`: {}",
            description,
            path.display(),
            error
        )
    });
    let flush_result = if write_result.is_ok() {
        file.flush().map_err(|error| {
            format!(
                "failed to flush {} `{}`: {}",
                description,
                path.display(),
                error
            )
        })
    } else {
        Ok(())
    };

    let result = write_result.and(flush_result);
    if result.is_err() {
        let _ = fs::remove_file(path);
    }
    result
}

fn emit_mir_runtime_launcher_source(mir_json: &[u8], source_path: &[u8], source: &[u8]) -> String {
    fn render_bytes(name: &str, bytes: &[u8]) -> String {
        let mut rendered = String::new();
        rendered.push_str(&format!("static const uint8_t {}[] = {{", name));
        if bytes.is_empty() {
            rendered.push_str("0");
        } else {
            for (index, byte) in bytes.iter().enumerate() {
                if index > 0 {
                    rendered.push_str(", ");
                }
                rendered.push_str(&byte.to_string());
            }
        }
        rendered.push_str("};\n");
        rendered
    }

    format!(
        "#include <stddef.h>\n#include <stdint.h>\n\nextern int aurora_native_run(const uint8_t*, size_t, const uint8_t*, size_t, const uint8_t*, size_t);\n\n{}{}{}int main(void) {{\n    return aurora_native_run(AURORA_MIR, {mir_len}, AURORA_SOURCE_PATH, {path_len}, AURORA_SOURCE, {source_len});\n}}\n",
        render_bytes("AURORA_MIR", mir_json),
        render_bytes("AURORA_SOURCE_PATH", source_path),
        render_bytes("AURORA_SOURCE", source),
        mir_len = mir_json.len(),
        path_len = source_path.len(),
        source_len = source.len(),
    )
}

fn repo_root() -> PathBuf {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    if let Some(root) = manifest_dir.parent().and_then(|path| path.parent()) {
        return root.to_path_buf();
    }
    manifest_dir
}

fn system_time_nanos() -> u128 {
    match std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH) {
        Ok(duration) => duration.as_nanos(),
        Err(_) => 0,
    }
}

struct NativeRuntimeArtifacts {
    staticlib: PathBuf,
    native_link_args: Vec<String>,
}

fn ensure_native_runtime_artifacts() -> std::result::Result<NativeRuntimeArtifacts, String> {
    let staticlib = build_native_runtime_staticlib()?
        .or_else(|| resolve_static_library_path(repo_root(), current_profile()).ok())
        .ok_or_else(|| {
            format!(
                "failed to locate compiled Aurora runtime library from Cargo artifact output or `{}`",
                repo_root()
                    .join("target")
                    .join(current_profile())
                    .join(static_library_file_name())
                    .display()
            )
        })?;
    if !staticlib.exists() {
        return Err(format!(
            "failed to locate compiled Aurora runtime library `{}` after build",
            staticlib.display()
        ));
    }

    let native_link_args = query_native_runtime_link_args()?;

    Ok(NativeRuntimeArtifacts {
        staticlib,
        native_link_args,
    })
}

fn build_native_runtime_staticlib() -> std::result::Result<Option<PathBuf>, String> {
    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    let mut command = Command::new(cargo);
    command.current_dir(repo_root());
    command
        .arg("build")
        .arg("-q")
        .arg("-p")
        .arg("aurora-compiler")
        .arg("--lib")
        .arg("--message-format=json-render-diagnostics");
    if current_profile() == "release" {
        command.arg("--release");
    }

    let output = command
        .output()
        .map_err(|error| format!("failed to build Aurora runtime artifacts: {}", error))?;

    if !output.status.success() {
        return Err(format!(
            "failed to build Aurora runtime artifacts:\n{}",
            String::from_utf8_lossy(&output.stderr)
        ));
    }

    Ok(parse_static_library_artifact_path(&output.stdout))
}

fn query_native_runtime_link_args() -> std::result::Result<Vec<String>, String> {
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
        .map_err(|error| format!("failed to query Aurora runtime link args: {}", error))?;
    if !output.status.success() {
        return Err(format!(
            "failed to query Aurora runtime link args:\n{}",
            String::from_utf8_lossy(&output.stderr)
        ));
    }

    Ok(parse_native_static_libs(&String::from_utf8_lossy(
        &output.stderr,
    )))
}

fn parse_static_library_artifact_path(stdout: &[u8]) -> Option<PathBuf> {
    let stdout = std::str::from_utf8(stdout).ok()?;
    let mut candidate = None;
    for line in stdout.lines() {
        let Ok(message) = serde_json::from_str::<JsonValue>(line) else {
            continue;
        };
        if message.get("reason").and_then(|value| value.as_str()) != Some("compiler-artifact") {
            continue;
        }
        let Some(target) = message.get("target") else {
            continue;
        };
        if target.get("name").and_then(|value| value.as_str()) != Some("aurora_compiler") {
            continue;
        }
        let Some(filenames) = message.get("filenames").and_then(|value| value.as_array()) else {
            continue;
        };
        for filename in filenames {
            let Some(path) = filename.as_str() else {
                continue;
            };
            let path = PathBuf::from(path);
            let Some(name) = path.file_name().and_then(|value| value.to_str()) else {
                continue;
            };
            if name.starts_with("libaurora_compiler") && name.ends_with(".a") {
                candidate = Some(path);
            }
        }
    }
    candidate
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
    if candidates.len() == 1 {
        if let Some(candidate) = candidates.pop() {
            return Ok(candidate);
        }
    }
    if !candidates.is_empty() {
        candidates.sort();
        return Err(format!(
            "found multiple hashed Aurora runtime archives in `{}` but no canonical `{}`: {}; rebuild the workspace so the current static runtime path is unambiguous",
            deps_dir.display(),
            primary.display(),
            candidates
                .iter()
                .map(|path| path.display().to_string())
                .collect::<Vec<_>>()
                .join(", ")
        ));
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

fn usage_text() -> &'static str {
    "usage: aura <check|run|build|ast|ast-json|mir|analyze> <file.au>\n\
       or: aura <check|run|build|ast|ast-json|mir|analyze> --stdin <virtual-path>\n\
       or: aura build [-o <output>] [--backend auto|direct] <file.au>\n\
       or: aura build [-o <output>] [--backend auto|direct] --stdin <virtual-path>\n\
       or: aura complete --line <n> --character <n> [--trigger .] <file.au>\n\
       or: aura complete --line <n> --character <n> [--trigger .] --stdin <virtual-path>\n\
       or: aura deps update [package]\n\
       or: aura help\n\
       or: aura version"
}

fn print_usage_and_exit(exit_code: i32) -> ! {
    if exit_code == 0 {
        write_stdout(&format!("{}\n", usage_text()));
    } else {
        eprintln!("{}", usage_text());
    }
    process::exit(exit_code);
}

fn print_version_and_exit() -> ! {
    write_stdout(&format!("aura {}\n", env!("CARGO_PKG_VERSION")));
    process::exit(0);
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::io;
    use std::path::PathBuf;
    use std::thread;
    use std::time::{Duration, SystemTime, UNIX_EPOCH};

    use super::{
        parse_static_library_artifact_path, resolve_static_library_path, write_unique_temp_file,
        write_unique_temp_file_with_writer,
    };

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
    fn resolve_static_library_path_uses_single_hashed_archive_when_primary_missing() {
        let root = unique_temp_dir("single-hashed");
        let deps = root.join("target").join("debug").join("deps");
        fs::create_dir_all(&deps).expect("deps dir should exist");
        let archive = deps.join("libaurora_compiler-only.a");
        fs::write(&archive, b"archive").expect("hashed archive should write");

        let resolved = resolve_static_library_path(root.clone(), "debug")
            .expect("should resolve the only hashed runtime library");
        assert_eq!(resolved, archive);

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn resolve_static_library_path_rejects_ambiguous_hashed_archives() {
        let root = unique_temp_dir("ambiguous-hashed");
        let deps = root.join("target").join("debug").join("deps");
        fs::create_dir_all(&deps).expect("deps dir should exist");
        let first = deps.join("libaurora_compiler-first.a");
        fs::write(&first, b"first").expect("first archive should write");
        thread::sleep(Duration::from_millis(10));
        let second = deps.join("libaurora_compiler-second.a");
        fs::write(&second, b"second").expect("second archive should write");

        let error = resolve_static_library_path(root.clone(), "debug")
            .expect_err("ambiguous hashed archives should be rejected");
        assert!(
            error.contains("multiple hashed Aurora runtime archives"),
            "unexpected error message: {}",
            error
        );

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn parse_static_library_artifact_path_prefers_cargo_reported_archive() {
        let stdout = br#"{"reason":"compiler-artifact","target":{"name":"aurora_compiler"},"filenames":["/tmp/libaurora_compiler-abc123.rlib","/tmp/libaurora_compiler-abc123.a"]}
{"reason":"compiler-artifact","target":{"name":"other"},"filenames":["/tmp/libother.a"]}"#;
        let resolved = parse_static_library_artifact_path(stdout)
            .expect("cargo artifact output should expose a static archive");
        assert_eq!(resolved, PathBuf::from("/tmp/libaurora_compiler-abc123.a"));
    }

    #[test]
    fn write_unique_temp_file_rejects_existing_paths() {
        let root = unique_temp_dir("unique-temp-file");
        let path = root.join("launcher.c");

        write_unique_temp_file(&path, b"first", "test temp file")
            .expect("first write should create the temp file");
        let error = write_unique_temp_file(&path, b"second", "test temp file")
            .expect_err("existing temp paths should be rejected");
        assert!(error.contains("failed to create"));

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn write_unique_temp_file_removes_partial_file_when_write_fails() {
        let root = unique_temp_dir("unique-temp-file-cleanup");
        let path = root.join("launcher.c");

        let error = write_unique_temp_file_with_writer(&path, "test temp file", |file| {
            use std::io::Write;

            file.write_all(b"partial")?;
            Err(io::Error::other("simulated write failure"))
        })
        .expect_err("partial temp files should be cleaned up after write failures");
        assert!(error.contains("failed to write"));
        assert!(
            !path.exists(),
            "failed unique-temp writes should not leave a stale partial file behind"
        );

        let _ = fs::remove_dir_all(root);
    }
}
