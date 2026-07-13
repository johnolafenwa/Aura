use std::fs;
use std::io::Write;
use std::io::{self, BufRead, Read};
use std::path::{Path, PathBuf};
use std::process::{self, Command};

use aurora_compiler::{
    analyze_path_source, check_path, check_path_with_source, complete_path_source,
    emit_host_native_object_with_metadata, lower_path_to_mir, lower_path_with_source_to_mir,
    parse_source, run_path, run_path_with_source_and_stdout_sink_and_program_args,
    run_path_with_stdout_sink_and_program_args, update_git_dependencies_in_working_dir, Diagnostic,
    MirModule, Value,
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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum SelectedBuildBackend {
    Direct,
    MirRuntime,
}

#[derive(Debug)]
struct BuildOutcome {
    selected: SelectedBuildBackend,
    fallback_reason: Option<String>,
}

fn main() {
    let mut args = std::env::args().skip(1);
    let Some(command) = args.next() else {
        print_usage_and_exit(2);
    };
    match command.as_str() {
        "help" | "--help" | "-h" => print_usage_and_exit(0),
        "version" | "--version" | "-V" => print_version_and_exit(),
        "lsp" => handle_lsp_service(),
        "new" => handle_new_command(args.collect()),
        "fmt" => handle_fmt_command(args.collect()),
        "test" => handle_test_command(args.collect()),
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
            let remaining = args.collect::<Vec<_>>();
            let delimiter = remaining.iter().position(|argument| argument == "--");
            let (input_args, program_args) = match delimiter {
                Some(index) => (&remaining[..index], &remaining[index + 1..]),
                None => (remaining.as_slice(), &[][..]),
            };
            let input = read_input(&mut input_args.iter().cloned());
            let stdout_sink = std::sync::Arc::new(|chunk: &str| write_stdout(chunk));
            let result = if input.from_stdin {
                run_path_with_source_and_stdout_sink_and_program_args(
                    Path::new(&input.path),
                    &input.source,
                    stdout_sink,
                    program_args.to_vec(),
                )
            } else {
                run_path_with_stdout_sink_and_program_args(
                    Path::new(&input.path),
                    stdout_sink,
                    program_args.to_vec(),
                )
            };
            match result {
                Ok(output) => {
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
                    match build_binary_with_backend(
                        &input.path,
                        &input.source,
                        &mir,
                        &output_path,
                        backend,
                    ) {
                        Ok(outcome) => {
                            if let Some(reason) = outcome.fallback_reason {
                                eprintln!(
                                    "aura: direct backend failed; using MIR runtime fallback:\n{}",
                                    reason
                                );
                            }
                            eprintln!(
                                "aura: built `{}` with {} backend",
                                output_path.display(),
                                match outcome.selected {
                                    SelectedBuildBackend::Direct => "direct",
                                    SelectedBuildBackend::MirRuntime => "MIR runtime",
                                }
                            );
                        }
                        Err(message) => {
                            eprintln!("{}", message);
                            process::exit(1);
                        }
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

fn handle_new_command(args: Vec<String>) {
    let [path] = args.as_slice() else {
        eprintln!("usage: aura new <project-path>");
        process::exit(2);
    };
    let project = PathBuf::from(path);
    if project.exists() {
        eprintln!(
            "refusing to overwrite existing path `{}`",
            project.display()
        );
        process::exit(1);
    }
    let Some(package_name) = project.file_name().and_then(|name| name.to_str()) else {
        eprintln!("project path must end in a valid UTF-8 package name");
        process::exit(2);
    };
    let valid_name = package_name.chars().enumerate().all(|(index, character)| {
        character.is_ascii_alphanumeric() || character == '_' || (character == '-' && index > 0)
    }) && package_name
        .chars()
        .next()
        .is_some_and(|character| character.is_ascii_alphabetic());
    if !valid_name {
        eprintln!(
            "package name `{package_name}` must start with a letter and contain only ASCII letters, digits, `_`, or `-`"
        );
        process::exit(2);
    }
    let manifest_name = package_name.replace('-', "_");

    let source_dir = project.join("src");
    let tests_dir = project.join("tests");
    if let Err(error) =
        fs::create_dir_all(&source_dir).and_then(|()| fs::create_dir_all(&tests_dir))
    {
        eprintln!("failed to create `{}`: {error}", source_dir.display());
        process::exit(1);
    }
    let manifest =
        format!("[package]\nname = \"{manifest_name}\"\nversion = \"0.1.0\"\nedition = \"2026\"\n");
    if let Err(error) = fs::write(project.join("Aurora.toml"), manifest)
        .and_then(|()| fs::write(project.join(".gitignore"), "target/\n"))
        .and_then(|()| {
            fs::write(
                source_dir.join("main.au"),
                "def main() -> int32:\n    print(\"Hello from Aurora\")\n    return 0\n",
            )
        })
        .and_then(|()| {
            fs::write(
                tests_dir.join("smoke.au"),
                "def main() -> int32:\n    return 0\n",
            )
        })
    {
        let _ = fs::remove_dir_all(&project);
        eprintln!(
            "failed to create Aurora project `{}`: {error}",
            project.display()
        );
        process::exit(1);
    }
    write_stdout(&format!("created `{}`\n", project.display()));
}

fn handle_fmt_command(args: Vec<String>) {
    let mut check_only = false;
    let mut inputs = Vec::new();
    for argument in args {
        if argument == "--check" {
            check_only = true;
        } else if argument.starts_with('-') {
            eprintln!("unknown aura fmt option `{argument}`");
            process::exit(2);
        } else {
            inputs.push(PathBuf::from(argument));
        }
    }
    if inputs.is_empty() {
        inputs.push(PathBuf::from("."));
    }
    let paths = collect_aurora_source_paths(&inputs).unwrap_or_else(|error| {
        eprintln!("{error}");
        process::exit(1);
    });
    let mut changed = Vec::new();
    for path in paths {
        let source = fs::read_to_string(&path).unwrap_or_else(|error| {
            eprintln!("failed to read `{}`: {error}", path.display());
            process::exit(1);
        });
        let formatted = format_aurora_source(&source);
        if let Err(error) = parse_source(&formatted) {
            eprintln!(
                "{}",
                error.render_with_source(&path.display().to_string(), &formatted)
            );
            process::exit(1);
        }
        if source != formatted {
            changed.push(path.clone());
            if !check_only {
                fs::write(&path, formatted).unwrap_or_else(|error| {
                    eprintln!("failed to write `{}`: {error}", path.display());
                    process::exit(1);
                });
            }
        }
    }
    if check_only && !changed.is_empty() {
        for path in changed {
            eprintln!("would format `{}`", path.display());
        }
        process::exit(1);
    }
}

fn format_aurora_source(source: &str) -> String {
    let mut formatted = source
        .lines()
        .map(|line| line.trim_end_matches([' ', '\t', '\r']))
        .collect::<Vec<_>>()
        .join("\n");
    if !formatted.is_empty() {
        formatted.push('\n');
    }
    formatted
}

fn collect_aurora_source_paths(inputs: &[PathBuf]) -> Result<Vec<PathBuf>, String> {
    fn visit(path: &Path, paths: &mut Vec<PathBuf>) -> Result<(), String> {
        if path.is_file() {
            if path.extension().and_then(|extension| extension.to_str()) == Some("au") {
                paths.push(path.to_path_buf());
            }
            return Ok(());
        }
        if !path.is_dir() {
            return Err(format!(
                "Aurora source path `{}` does not exist",
                path.display()
            ));
        }
        for entry in fs::read_dir(path)
            .map_err(|error| format!("failed to read `{}`: {error}", path.display()))?
        {
            let entry = entry.map_err(|error| {
                format!("failed to read entry under `{}`: {error}", path.display())
            })?;
            let child = entry.path();
            let name = child
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("");
            if child.is_dir()
                && (name.starts_with('.') || matches!(name, "target" | "node_modules"))
            {
                continue;
            }
            visit(&child, paths)?;
        }
        Ok(())
    }

    let mut paths = Vec::new();
    for input in inputs {
        visit(input, &mut paths)?;
    }
    paths.sort();
    paths.dedup();
    Ok(paths)
}

fn handle_test_command(args: Vec<String>) {
    let mut timeout_ms = 30_000u64;
    let mut inputs = Vec::new();
    let mut args = args.into_iter();
    while let Some(argument) = args.next() {
        if argument == "--timeout-ms" {
            timeout_ms = args
                .next()
                .and_then(|value| value.parse::<u64>().ok())
                .filter(|value| *value > 0)
                .unwrap_or_else(|| {
                    eprintln!("aura test --timeout-ms requires a positive integer");
                    process::exit(2);
                });
        } else if argument.starts_with('-') {
            eprintln!("unknown aura test option `{argument}`");
            process::exit(2);
        } else {
            inputs.push(PathBuf::from(argument));
        }
    }
    let inputs = if inputs.is_empty() {
        vec![PathBuf::from("tests")]
    } else {
        inputs
    };
    let paths = collect_aurora_source_paths(&inputs).unwrap_or_else(|error| {
        eprintln!("{error}");
        process::exit(1);
    });
    if paths.is_empty() {
        eprintln!("no Aurora test files found");
        process::exit(1);
    }

    let mut passed = 0usize;
    let mut failed = 0usize;
    for path in paths {
        let source = fs::read_to_string(&path).unwrap_or_else(|error| {
            eprintln!("failed to read `{}`: {error}", path.display());
            process::exit(1);
        });
        let (sender, receiver) = std::sync::mpsc::sync_channel(1);
        let test_path = path.clone();
        std::thread::Builder::new()
            .name(format!("aura-test-{}", path.display()))
            .stack_size(64 * 1024 * 1024)
            .spawn(move || {
                let _ = sender.send(run_path(&test_path));
            })
            .unwrap_or_else(|error| {
                eprintln!("failed to start test `{}`: {error}", path.display());
                process::exit(1);
            });
        match receiver.recv_timeout(std::time::Duration::from_millis(timeout_ms)) {
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                failed += 1;
                eprintln!("FAILED {} (timed out after {timeout_ms}ms)", path.display());
            }
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                failed += 1;
                eprintln!("FAILED {} (test worker disconnected)", path.display());
            }
            Ok(Ok(output)) => {
                let success = match output.value {
                    Value::Int(code) => code.as_i128() == Some(0),
                    _ => true,
                };
                if success {
                    passed += 1;
                    write_stdout(&format!("ok {}\n", path.display()));
                } else {
                    failed += 1;
                    eprintln!("FAILED {} (non-zero main return)", path.display());
                }
            }
            Ok(Err(error)) => {
                failed += 1;
                eprintln!(
                    "FAILED {}\n{}",
                    path.display(),
                    error.render_with_source(&path.display().to_string(), &source)
                );
            }
        }
    }
    write_stdout(&format!("{passed} passed; {failed} failed\n"));
    if failed > 0 {
        process::exit(1);
    }
}

fn handle_lsp_service() {
    let stdin = io::stdin();
    let stdout = io::stdout();
    let mut writer = stdout.lock();

    for line in stdin.lock().lines() {
        let response = match line {
            Ok(line) => lsp_response_for_line(&line),
            Err(error) => serde_json::json!({
                "id": JsonValue::Null,
                "error": format!("failed to read LSP compiler request: {error}")
            }),
        };
        let write_result = serde_json::to_writer(&mut writer, &response)
            .map_err(io::Error::other)
            .and_then(|_| writer.write_all(b"\n"))
            .and_then(|_| writer.flush());
        if let Err(error) = write_result {
            if error.kind() == io::ErrorKind::BrokenPipe {
                return;
            }
            eprintln!("failed to write LSP compiler response: {error}");
            process::exit(1);
        }
    }
}

fn lsp_response_for_line(line: &str) -> JsonValue {
    let request = match serde_json::from_str::<JsonValue>(line) {
        Ok(request) => request,
        Err(error) => {
            return serde_json::json!({
                "id": JsonValue::Null,
                "error": format!("invalid LSP compiler request JSON: {error}")
            });
        }
    };
    let id = request.get("id").cloned().unwrap_or(JsonValue::Null);
    let result = lsp_result_for_request(&request);
    match result {
        Ok(result) => serde_json::json!({ "id": id, "result": result }),
        Err(error) => serde_json::json!({ "id": id, "error": error }),
    }
}

fn lsp_result_for_request(request: &JsonValue) -> Result<JsonValue, String> {
    let method = request
        .get("method")
        .and_then(JsonValue::as_str)
        .ok_or_else(|| "LSP compiler request requires string field `method`".to_string())?;
    let path = request
        .get("path")
        .and_then(JsonValue::as_str)
        .ok_or_else(|| "LSP compiler request requires string field `path`".to_string())?;
    let source = request
        .get("source")
        .and_then(JsonValue::as_str)
        .ok_or_else(|| "LSP compiler request requires string field `source`".to_string())?;

    match method {
        "analyze" => serde_json::to_value(analyze_path_source(Path::new(path), source))
            .map_err(|error| format!("failed to serialize compiler analysis: {error}")),
        "complete" => {
            let line = request
                .get("line")
                .and_then(JsonValue::as_u64)
                .and_then(|value| usize::try_from(value).ok())
                .ok_or_else(|| {
                    "completion request requires non-negative integer field `line`".to_string()
                })?;
            let character = request
                .get("character")
                .and_then(JsonValue::as_u64)
                .and_then(|value| usize::try_from(value).ok())
                .ok_or_else(|| {
                    "completion request requires non-negative integer field `character`".to_string()
                })?;
            let trigger = request
                .get("trigger")
                .and_then(JsonValue::as_str)
                .and_then(|value| value.chars().next());
            let completions =
                complete_path_source(Path::new(path), source, line, character, trigger)
                    .map_err(|error| error.render_with_source(path, source))?;
            serde_json::to_value(completions)
                .map_err(|error| format!("failed to serialize compiler completions: {error}"))
        }
        _ => Err(format!("unknown LSP compiler request method `{method}`")),
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
) -> std::result::Result<BuildOutcome, String> {
    select_build_backend(
        backend,
        || build_direct_native_binary(path, source, mir, output_path),
        || build_mir_runtime_binary(path, source, mir, output_path),
    )
}

fn select_build_backend(
    backend: BuildBackend,
    direct: impl FnOnce() -> std::result::Result<(), String>,
    mir_runtime: impl FnOnce() -> std::result::Result<(), String>,
) -> std::result::Result<BuildOutcome, String> {
    match backend {
        BuildBackend::Direct => direct().map(|()| BuildOutcome {
            selected: SelectedBuildBackend::Direct,
            fallback_reason: None,
        }),
        BuildBackend::Auto => match direct() {
            Ok(()) => Ok(BuildOutcome {
                selected: SelectedBuildBackend::Direct,
                fallback_reason: None,
            }),
            Err(direct_error) => match mir_runtime() {
                Ok(()) => Ok(BuildOutcome {
                    selected: SelectedBuildBackend::MirRuntime,
                    fallback_reason: Some(direct_error),
                }),
                Err(mir_error) => Err(format!(
                    "both native build backends failed\n\ndirect backend:\n{direct_error}\n\nMIR runtime backend:\n{mir_error}"
                )),
            },
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
            rendered.push('0');
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
    let executable = std::env::current_exe()
        .map_err(|error| format!("failed to locate the running aura executable: {}", error))?;
    if let Some(installed) = resolve_installed_runtime_artifacts_from_executable(&executable)? {
        return Ok(installed);
    }

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

fn resolve_installed_runtime_artifacts_from_executable(
    executable: &Path,
) -> std::result::Result<Option<NativeRuntimeArtifacts>, String> {
    let Some(prefix) = executable.parent().and_then(Path::parent) else {
        return Ok(None);
    };
    let runtime_dir = prefix.join("lib").join("aurora");
    let staticlib = runtime_dir.join(static_library_file_name());
    let manifest = runtime_dir.join("native-link-args.json");
    let staticlib_exists = staticlib.is_file();
    let manifest_exists = manifest.is_file();

    if !staticlib_exists && !manifest_exists {
        return Ok(None);
    }
    if !staticlib_exists || !manifest_exists {
        return Err(format!(
            "incomplete Aurora runtime installation in `{}`: expected both `{}` and `{}`",
            runtime_dir.display(),
            staticlib.display(),
            manifest.display()
        ));
    }

    let manifest_bytes = fs::read(&manifest).map_err(|error| {
        format!(
            "failed to read Aurora runtime link manifest `{}`: {}",
            manifest.display(),
            error
        )
    })?;
    let native_link_args =
        serde_json::from_slice::<Vec<String>>(&manifest_bytes).map_err(|error| {
            format!(
                "invalid Aurora runtime link manifest `{}`: {}",
                manifest.display(),
                error
            )
        })?;

    Ok(Some(NativeRuntimeArtifacts {
        staticlib,
        native_link_args,
    }))
}

fn build_native_runtime_staticlib() -> std::result::Result<Option<PathBuf>, String> {
    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    let mut command = Command::new(cargo);
    command.current_dir(repo_root());
    configure_native_runtime_cargo(
        &mut command,
        std::env::var_os("LLVM_PROFILE_FILE").is_some(),
    );
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
    configure_native_runtime_cargo(
        &mut command,
        std::env::var_os("LLVM_PROFILE_FILE").is_some(),
    );
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

fn configure_native_runtime_cargo(command: &mut Command, coverage_active: bool) {
    if !coverage_active {
        return;
    }

    for variable in [
        "CARGO_ENCODED_RUSTFLAGS",
        "CARGO_LLVM_COV",
        "CARGO_LLVM_COV_BUILD_DIR",
        "CARGO_LLVM_COV_SHOW_ENV",
        "CARGO_LLVM_COV_TARGET_DIR",
        "LLVM_PROFILE_FILE",
        "RUSTC_WRAPPER",
        "RUSTDOCFLAGS",
        "RUSTFLAGS",
        "__CARGO_LLVM_COV_RUSTC_WRAPPER",
        "__CARGO_LLVM_COV_RUSTC_WRAPPER_CRATE_NAMES",
        "__CARGO_LLVM_COV_RUSTC_WRAPPER_RUSTFLAGS",
    ] {
        command.env_remove(variable);
    }
    command.env(
        "CARGO_TARGET_DIR",
        repo_root().join("target/native-runtime-uninstrumented"),
    );
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
       or: aura run <file.au> [-- <program-args>...]\n\
       or: aura build [-o <output>] [--backend auto|direct] <file.au>\n\
       or: aura build [-o <output>] [--backend auto|direct] --stdin <virtual-path>\n\
       or: aura complete --line <n> --character <n> [--trigger .] <file.au>\n\
       or: aura complete --line <n> --character <n> [--trigger .] --stdin <virtual-path>\n\
       or: aura lsp\n\
       or: aura new <project-path>\n\
       or: aura fmt [--check] [path ...]\n\
       or: aura test [--timeout-ms <n>] [path ...]\n\
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
    use std::process::Command;
    use std::thread;
    use std::time::{Duration, SystemTime, UNIX_EPOCH};

    use super::{
        configure_native_runtime_cargo, parse_static_library_artifact_path,
        resolve_installed_runtime_artifacts_from_executable, resolve_static_library_path,
        select_build_backend, write_unique_temp_file, write_unique_temp_file_with_writer,
        BuildBackend, SelectedBuildBackend,
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
    fn coverage_builds_isolate_uninstrumented_native_runtime_artifacts() {
        let mut command = Command::new("cargo");
        command.env("LLVM_PROFILE_FILE", "coverage.profraw");
        command.env("RUSTC_WRAPPER", "cargo-llvm-cov");
        configure_native_runtime_cargo(&mut command, true);

        let environments = command
            .get_envs()
            .map(|(name, value)| {
                (
                    name.to_string_lossy().to_string(),
                    value.map(|value| value.to_string_lossy().to_string()),
                )
            })
            .collect::<std::collections::BTreeMap<_, _>>();
        assert_eq!(environments.get("LLVM_PROFILE_FILE"), Some(&None));
        assert_eq!(environments.get("RUSTC_WRAPPER"), Some(&None));
        assert!(environments
            .get("CARGO_TARGET_DIR")
            .and_then(Option::as_ref)
            .is_some_and(|path| path.ends_with("target/native-runtime-uninstrumented")));
    }

    #[test]
    fn installed_runtime_artifacts_resolve_relative_to_packaged_executable() {
        let root = unique_temp_dir("installed-runtime");
        let executable = root.join("bin").join("aura");
        let runtime_dir = root.join("lib").join("aurora");
        fs::create_dir_all(executable.parent().expect("binary should have a parent"))
            .expect("bin dir should exist");
        fs::create_dir_all(&runtime_dir).expect("runtime dir should exist");
        fs::write(&executable, b"test executable").expect("test executable should write");
        let staticlib = runtime_dir.join("libaurora_compiler.a");
        fs::write(&staticlib, b"test runtime").expect("test runtime should write");
        fs::write(
            runtime_dir.join("native-link-args.json"),
            br#"["-framework","Security","-lc"]"#,
        )
        .expect("runtime manifest should write");

        let artifacts = resolve_installed_runtime_artifacts_from_executable(&executable)
            .expect("installed runtime manifest should be valid")
            .expect("installed runtime should resolve");
        assert_eq!(artifacts.staticlib, staticlib);
        assert_eq!(
            artifacts.native_link_args,
            vec!["-framework", "Security", "-lc"]
        );

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn auto_backend_reports_direct_failure_when_falling_back() {
        let outcome = select_build_backend(
            BuildBackend::Auto,
            || Err("direct failed".to_string()),
            || Ok(()),
        )
        .expect("MIR fallback should succeed");
        assert_eq!(outcome.selected, SelectedBuildBackend::MirRuntime);
        assert_eq!(outcome.fallback_reason.as_deref(), Some("direct failed"));

        let error = select_build_backend(
            BuildBackend::Auto,
            || Err("direct failed".to_string()),
            || Err("MIR failed".to_string()),
        )
        .expect_err("both backend failures should be preserved");
        assert!(error.contains("direct failed"));
        assert!(error.contains("MIR failed"));
    }

    #[test]
    fn forced_direct_backend_never_invokes_fallback() {
        let outcome = select_build_backend(
            BuildBackend::Direct,
            || Ok(()),
            || panic!("forced direct mode must not invoke MIR fallback"),
        )
        .expect("direct backend should succeed");
        assert_eq!(outcome.selected, SelectedBuildBackend::Direct);
        assert!(outcome.fallback_reason.is_none());
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
