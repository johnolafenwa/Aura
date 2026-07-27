pub mod analysis;
pub mod ast;
mod builtin_modules;
pub(crate) mod bytes_codec;
pub mod call;
pub mod diag;
pub mod integer;
pub(crate) mod json_codec;
pub mod lexer;
pub mod limits;
pub mod mir;
pub mod mir_runtime;
mod native_codegen;
mod native_runtime;
mod package;
pub mod parser;
mod randomness;
mod runtime_reactor;
pub mod runtime_value;
pub mod sema;

use std::fs;
use std::path::{Path, PathBuf};
use std::{collections::BTreeMap, collections::BTreeSet, collections::HashMap};

pub use analysis::{
    analyze_path_source, analyze_program, analyze_source, complete_path_source, complete_source,
    AnalysisCompletion, AnalysisOutput,
};
pub use diag::{Diagnostic, Result, Span};
pub use mir::{lower as lower_to_mir, MirModule};
pub use mir_runtime::{
    run as run_mir, run_entry_with_stdout_sink_and_program_args as run_mir_entry,
    run_serialized_mir, run_with_stdout_sink as run_mir_with_stdout_sink,
    run_with_stdout_sink_and_program_args as run_mir_with_stdout_sink_and_program_args, StdoutSink,
};
pub use native_codegen::{
    emit_host_object as emit_host_native_object,
    emit_host_object_with_metadata as emit_host_native_object_with_metadata,
};
pub use runtime_value::{RunOutput, Value};

/// Version of the compiler's exported semantic interface.
///
/// Every persisted artifact or long-lived tooling cache that can contain
/// compiler semantic metadata must bind this value. Bump it whenever the
/// meaning or representation of checked source changes incompatibly.
pub const SEMANTIC_INTERFACE_SCHEMA_VERSION: u32 = 2;

/// Lowercase hexadecimal SHA-256 of `bytes`, for content-addressed identities.
pub fn sha256_hex(bytes: &[u8]) -> String {
    let digest = bytes_codec::sha256_bytes(bytes).expect("SHA-256 output always fits its buffer");
    let mut hex = String::with_capacity(digest.len() * 2);
    for byte in digest {
        hex.push_str(&format!("{byte:02x}"));
    }
    hex
}
pub use sema::{ImportedBinding, ModuleContext, ModuleNamespace, Program};

use ast::{ImportKind, Item};
pub use package::DependencyUpdateResult;
use package::PackageGraph;

#[cfg(coverage)]
#[doc(hidden)]
pub mod native_runtime_coverage {
    pub use super::native_runtime::aurora_direct_tag_value_type;
    pub use super::native_runtime::{
        aurora_direct_binary_value, aurora_direct_binary_value_at, aurora_direct_box_bool,
        aurora_direct_box_i64, aurora_direct_box_u64, aurora_direct_cast_float_to_integer,
        aurora_direct_cast_integer_to_float, aurora_direct_cast_integer_to_integer,
        aurora_direct_cast_value, aurora_direct_channel_new, aurora_direct_channel_recv,
        aurora_direct_channel_send_timeout_value, aurora_direct_channel_try_send,
        aurora_direct_close_value, aurora_direct_coverage_clone_value,
        aurora_direct_duration_from_i64, aurora_direct_duration_literal,
        aurora_direct_duration_to_float, aurora_direct_enum_variant, aurora_direct_file_close,
        aurora_direct_file_flush, aurora_direct_file_read_all, aurora_direct_file_write_all,
        aurora_direct_fs_append_string, aurora_direct_fs_create, aurora_direct_fs_create_dir,
        aurora_direct_fs_open, aurora_direct_fs_read_dir, aurora_direct_http_listener_accept,
        aurora_direct_http_listener_close, aurora_direct_http_listener_local_addr,
        aurora_direct_http_response_bytes, aurora_direct_http_response_headers,
        aurora_direct_http_response_reason, aurora_direct_http_response_status,
        aurora_direct_http_response_text, aurora_direct_instance_get_field,
        aurora_direct_instance_new, aurora_direct_integer_to_float, aurora_direct_io_flush,
        aurora_direct_io_write, aurora_direct_map_clear_in_place, aurora_direct_map_contains_key,
        aurora_direct_map_empty, aurora_direct_map_entries, aurora_direct_map_extend_in_place,
        aurora_direct_map_get, aurora_direct_map_index, aurora_direct_map_is_empty,
        aurora_direct_map_items, aurora_direct_map_keys, aurora_direct_map_len,
        aurora_direct_map_remove_in_place, aurora_direct_map_set_in_place,
        aurora_direct_map_set_index_in_place, aurora_direct_map_values,
        aurora_direct_monotonic_time_ms, aurora_direct_net_connect, aurora_direct_net_http_listen,
        aurora_direct_net_http_request_bytes_timeout, aurora_direct_net_listen,
        aurora_direct_net_udp_bind, aurora_direct_net_unix_connect, aurora_direct_net_unix_listen,
        aurora_direct_net_websocket_connect, aurora_direct_net_websocket_listen,
        aurora_direct_process_child_close, aurora_direct_process_child_stderr,
        aurora_direct_process_child_stdin, aurora_direct_process_child_stdout,
        aurora_direct_process_child_wait, aurora_direct_process_child_wait_ok,
        aurora_direct_process_child_wait_or_none, aurora_direct_process_completed_check,
        aurora_direct_process_completed_status, aurora_direct_process_completed_stderr,
        aurora_direct_process_completed_stderr_bytes, aurora_direct_process_completed_stdout,
        aurora_direct_process_completed_stdout_bytes, aurora_direct_process_completed_success,
        aurora_direct_process_null, aurora_direct_process_pipe, aurora_direct_process_pipe_close,
        aurora_direct_process_pipe_flush, aurora_direct_process_pipe_read_all,
        aurora_direct_process_pipe_read_bytes, aurora_direct_process_pipe_write_all,
        aurora_direct_process_pipe_write_bytes, aurora_direct_process_run,
        aurora_direct_process_start, aurora_direct_random_secure_bytes,
        aurora_direct_random_secure_int, aurora_direct_release_value, aurora_direct_rng_new,
        aurora_direct_rng_next_float, aurora_direct_rng_next_int, aurora_direct_rng_shuffle,
        aurora_direct_set_contains, aurora_direct_set_empty, aurora_direct_set_index_option,
        aurora_direct_set_insert_in_place, aurora_direct_set_is_empty, aurora_direct_set_len,
        aurora_direct_set_remove_in_place, aurora_direct_sleep_ms, aurora_direct_sleep_value,
        aurora_direct_sleep_value_void, aurora_direct_string_byte_len, aurora_direct_string_len,
        aurora_direct_string_literal, aurora_direct_tcp_listener_accept,
        aurora_direct_tcp_listener_close, aurora_direct_tcp_listener_local_addr,
        aurora_direct_tcp_stream_close, aurora_direct_tcp_stream_flush,
        aurora_direct_tcp_stream_local_addr, aurora_direct_tcp_stream_peer_addr,
        aurora_direct_tcp_stream_read_all, aurora_direct_tcp_stream_read_exact,
        aurora_direct_tcp_stream_shutdown_read, aurora_direct_tcp_stream_shutdown_write,
        aurora_direct_tcp_stream_write_all, aurora_direct_tcp_stream_write_bytes,
        aurora_direct_tuple_element, aurora_direct_tuple_new, aurora_direct_tuple_take_element,
        aurora_direct_udp_datagram_address, aurora_direct_udp_datagram_bytes,
        aurora_direct_udp_datagram_text, aurora_direct_udp_socket_close,
        aurora_direct_udp_socket_local_addr, aurora_direct_udp_socket_recv,
        aurora_direct_udp_socket_recv_from, aurora_direct_udp_socket_send_bytes,
        aurora_direct_unary_value, aurora_direct_unary_value_at, aurora_direct_unbox_bool,
        aurora_direct_unbox_i64, aurora_direct_unbox_int64, aurora_direct_unbox_u64,
        aurora_direct_unix_listener_accept, aurora_direct_unix_listener_close,
        aurora_direct_unix_stream_close, aurora_direct_unix_stream_read_exact,
        aurora_direct_unix_stream_write_all, aurora_direct_value_as_condition,
        aurora_direct_variant_payload, aurora_direct_vec_clear_in_place,
        aurora_direct_vec_contains, aurora_direct_vec_empty, aurora_direct_vec_extend_in_place,
        aurora_direct_vec_get, aurora_direct_vec_index, aurora_direct_vec_index_option,
        aurora_direct_vec_insert_in_place, aurora_direct_vec_is_empty, aurora_direct_vec_len,
        aurora_direct_vec_pop_in_place, aurora_direct_vec_push_in_place,
        aurora_direct_vec_remove_in_place, aurora_direct_vec_reverse_in_place,
        aurora_direct_vec_set_in_place, aurora_direct_vec_set_index_in_place,
        aurora_direct_vec_swap_in_place, aurora_direct_vec_take_index_in_place,
        aurora_direct_wait_all, aurora_direct_wait_all_timeout_value, aurora_direct_wait_any,
        aurora_direct_wait_any_timeout_value, aurora_direct_websocket_close,
        aurora_direct_websocket_listener_accept, aurora_direct_websocket_listener_local_addr,
        aurora_direct_websocket_recv_bytes, aurora_direct_websocket_recv_text,
        aurora_direct_websocket_send_bytes, aurora_direct_websocket_send_text, OpaqueValue,
    };
}

pub fn parse_source(source: &str) -> Result<ast::Module> {
    parser::parse(source)
}

pub fn check_source(source: &str) -> Result<Program> {
    let module = parse_source(source)?;
    check_module_with_builtin_imports(module)
}

pub fn run_source(source: &str) -> Result<RunOutput> {
    let program = check_source(source)?;
    let mir = lower_to_mir(&program);
    run_mir(&mir)
}

pub fn run_source_with_stdout_sink(source: &str, stdout_sink: StdoutSink) -> Result<RunOutput> {
    let program = check_source(source)?;
    let mir = lower_to_mir(&program);
    run_mir_with_stdout_sink(&mir, Some(stdout_sink))
}

pub fn run_path_with_source(path: &Path, source: &str) -> Result<RunOutput> {
    let program = check_path_with_source(path, source)?;
    let mir = lower_to_mir(&program);
    run_mir(&mir)
}

pub fn run_path_with_source_and_stdout_sink(
    path: &Path,
    source: &str,
    stdout_sink: StdoutSink,
) -> Result<RunOutput> {
    let program = check_path_with_source(path, source)?;
    let mir = lower_to_mir(&program);
    run_mir_with_stdout_sink(&mir, Some(stdout_sink))
}

pub fn run_path_with_source_and_stdout_sink_and_program_args(
    path: &Path,
    source: &str,
    stdout_sink: StdoutSink,
    program_args: Vec<String>,
) -> Result<RunOutput> {
    let program = check_path_with_source(path, source)?;
    let mir = lower_to_mir(&program);
    run_mir_with_stdout_sink_and_program_args(&mir, Some(stdout_sink), program_args)
}

pub fn lower_source_to_mir(source: &str) -> Result<MirModule> {
    let program = check_source(source)?;
    Ok(lower_to_mir(&program))
}

fn builtin_imports(module: &ast::Module) -> Result<BTreeMap<String, ImportedBinding>> {
    let mut bindings = BTreeMap::new();
    for import in &module.imports {
        match &import.kind {
            ImportKind::Module { path } => {
                if let Some(namespace) = builtin_modules::builtin_module_namespace(path) {
                    insert_namespace_import(&mut bindings, path, namespace, import.span)?;
                }
            }
            ImportKind::From { module_path, names } => {
                if builtin_modules::builtin_module_namespace(module_path).is_some() {
                    for name in names {
                        let binding = builtin_modules::builtin_imported_binding(
                            module_path,
                            name,
                            import.span,
                        )?;
                        if bindings.insert(name.clone(), binding).is_some() {
                            return Err(Diagnostic::at(
                                import.span,
                                format!("duplicate import binding `{}`", name),
                            ));
                        }
                    }
                }
            }
        }
    }
    Ok(bindings)
}

fn builtin_module_registry_with_user(
    user_modules: impl IntoIterator<Item = (String, ModuleNamespace)>,
) -> BTreeMap<String, ModuleNamespace> {
    let mut registry = builtin_modules::builtin_module_registry();
    registry.extend(user_modules);
    registry
}

fn check_module_with_builtin_imports(module: ast::Module) -> Result<Program> {
    let imported_bindings = builtin_imports(&module)?;
    let module_registry = builtin_module_registry_with_user(BTreeMap::new());
    sema::check_with_context(
        module,
        ModuleContext {
            module_name: "<main>".to_string(),
            imported_bindings,
            module_registry,
            is_entry_module: true,
        },
    )
}

pub fn lower_path_with_source_to_mir(path: &Path, source: &str) -> Result<MirModule> {
    let program = check_path_with_source(path, source)?;
    Ok(lower_to_mir(&program))
}

pub fn check_path(path: &Path) -> Result<Program> {
    let mut loader = ModuleLoader::new(path)?;
    let program = loader.load_program(path)?;
    loader.write_lockfile()?;
    Ok(program)
}

pub fn check_path_with_source(path: &Path, source: &str) -> Result<Program> {
    check_path_with_source_inner(path, source, true)
}

fn check_path_with_source_without_lockfile(path: &Path, source: &str) -> Result<Program> {
    check_path_with_source_inner(path, source, false)
}

fn check_path_with_source_inner(
    path: &Path,
    source: &str,
    write_lockfile: bool,
) -> Result<Program> {
    let mut loader = ModuleLoader::new_with_source(path, Some(source))?;
    let program = loader.load_program_with_source(path, source)?;
    if write_lockfile {
        loader.write_lockfile()?;
    }
    Ok(program)
}

pub fn run_path(path: &Path) -> Result<RunOutput> {
    let program = check_path(path)?;
    let mir = lower_to_mir(&program);
    run_mir(&mir)
}

pub fn run_path_with_stdout_sink(path: &Path, stdout_sink: StdoutSink) -> Result<RunOutput> {
    let program = check_path(path)?;
    let mir = lower_to_mir(&program);
    run_mir_with_stdout_sink(&mir, Some(stdout_sink))
}

pub fn run_path_with_stdout_sink_and_program_args(
    path: &Path,
    stdout_sink: StdoutSink,
    program_args: Vec<String>,
) -> Result<RunOutput> {
    let program = check_path(path)?;
    let mir = lower_to_mir(&program);
    run_mir_with_stdout_sink_and_program_args(&mir, Some(stdout_sink), program_args)
}

pub fn lower_path_to_mir(path: &Path) -> Result<MirModule> {
    let program = check_path(path)?;
    Ok(lower_to_mir(&program))
}

pub fn update_git_dependencies_in_working_dir(
    path: &Path,
    target_package: Option<&str>,
) -> Result<DependencyUpdateResult> {
    package::update_git_dependencies_in_working_dir(path, target_package)
}

#[derive(Clone)]
struct LoadedModule {
    program: Program,
}

struct ModuleLoader {
    package_root: PathBuf,
    package_graph: Option<PackageGraph>,
    cache: HashMap<PathBuf, LoadedModule>,
    stack: Vec<PathBuf>,
}

impl ModuleLoader {
    fn new(entry_path: &Path) -> Result<Self> {
        Self::new_with_source(entry_path, None)
    }

    fn new_with_source(entry_path: &Path, source_override: Option<&str>) -> Result<Self> {
        let absolute_entry = absolutize(entry_path);
        let package_graph = PackageGraph::discover_for_entry(&absolute_entry)?;
        let package_root = if let Some(graph) = &package_graph {
            graph.root_source_root.clone()
        } else {
            infer_package_root(&absolute_entry, source_override)?
        };
        Ok(Self {
            package_root,
            package_graph,
            cache: HashMap::new(),
            stack: Vec::new(),
        })
    }

    fn load_program(&mut self, path: &Path) -> Result<Program> {
        self.load_program_internal(path, None)
    }

    fn load_program_with_source(&mut self, path: &Path, source: &str) -> Result<Program> {
        self.load_program_internal(path, Some(source))
    }

    fn load_program_internal(
        &mut self,
        path: &Path,
        source_override: Option<&str>,
    ) -> Result<Program> {
        let path = absolutize(path);
        if let Some(loaded) = self.cache.get(&path) {
            return Ok(loaded.program.clone());
        }
        if self.stack.contains(&path) {
            return Err(Diagnostic::new(format!(
                "cyclic import involving `{}`",
                path.display()
            )));
        }

        self.stack.push(path.clone());
        let is_entry_module = self.stack.len() == 1;

        let source = if let Some(source) = source_override {
            source.to_string()
        } else {
            fs::read_to_string(&path).map_err(|error| {
                Diagnostic::new(format!("failed to read `{}`: {}", path.display(), error))
            })?
        };
        let display_path = path.display().to_string();
        let module = parse_source(&source)
            .map_err(|error| error.with_render_context(display_path.clone(), source.clone()))?;
        let module_name = self.module_name_for_path(&path);
        let imported_bindings = self.resolve_imports(&module, &path)?;
        let module_registry = self.build_module_registry();
        let program = sema::check_with_context(
            module,
            ModuleContext {
                module_name,
                imported_bindings,
                module_registry,
                is_entry_module,
            },
        )
        .map_err(|error| error.with_render_context(display_path, source.clone()))?;
        let mut program = program;
        self.qualify_program_imported_modules(&path, &mut program);
        program.source_path = Some(path.display().to_string());

        self.cache.insert(
            path.clone(),
            LoadedModule {
                program: program.clone(),
            },
        );
        self.stack.pop();
        Ok(program)
    }

    fn resolve_imports(
        &mut self,
        module: &ast::Module,
        current_path: &Path,
    ) -> Result<BTreeMap<String, ImportedBinding>> {
        let mut bindings = BTreeMap::new();
        for import in &module.imports {
            match &import.kind {
                ImportKind::From { module_path, names } => {
                    if builtin_modules::builtin_module_namespace(module_path).is_some() {
                        for name in names {
                            let binding = builtin_modules::builtin_imported_binding(
                                module_path,
                                name,
                                import.span,
                            )?;
                            if bindings.insert(name.clone(), binding).is_some() {
                                return Err(Diagnostic::at(
                                    import.span,
                                    format!("duplicate import binding `{}`", name),
                                ));
                            }
                        }
                        continue;
                    }
                    let imported =
                        self.load_imported_module(current_path, module_path, import.span)?;
                    for name in names {
                        let binding = exported_binding(&imported, name).ok_or_else(|| {
                            let logical_name = module_path.join(".");
                            if local_item_exists(&imported, name) {
                                Diagnostic::at(
                                    import.span,
                                    format!(
                                        "item `{}` is private in module `{}`",
                                        name, logical_name
                                    ),
                                )
                            } else {
                                Diagnostic::at(
                                    import.span,
                                    format!(
                                        "module `{}` has no export named `{}`",
                                        logical_name, name
                                    ),
                                )
                            }
                        })?;
                        if bindings.insert(name.clone(), binding).is_some() {
                            return Err(Diagnostic::at(
                                import.span,
                                format!("duplicate import binding `{}`", name),
                            ));
                        }
                    }
                }
                ImportKind::Module { path } => {
                    if let Some(leaf) = builtin_modules::builtin_module_namespace(path) {
                        insert_namespace_import(&mut bindings, path, leaf, import.span)?;
                        continue;
                    }
                    let imported = self.load_imported_module(current_path, path, import.span)?;
                    let leaf = exported_namespace(path, &imported);
                    insert_namespace_import(&mut bindings, path, leaf, import.span)?;
                }
            }
        }
        Ok(bindings)
    }

    fn build_module_registry(&self) -> BTreeMap<String, ModuleNamespace> {
        builtin_module_registry_with_user(self.cache.values().map(|loaded| {
            let path = loaded
                .program
                .module_name
                .split('.')
                .map(|segment| segment.to_string())
                .collect::<Vec<_>>();
            (
                loaded.program.module_name.clone(),
                exported_namespace(&path, &loaded.program),
            )
        }))
    }

    fn load_imported_module(
        &mut self,
        current_path: &Path,
        module_path: &[String],
        span: Span,
    ) -> Result<Program> {
        let path = self.resolve_import_path(current_path, module_path)?;
        if !path.exists() {
            return Err(Diagnostic::at(
                span,
                format!(
                    "cannot resolve module `{}` at `{}`",
                    module_path.join("."),
                    path.display()
                ),
            ));
        }
        self.load_program(&path)
    }

    fn resolve_import_path(&self, current_path: &Path, module_path: &[String]) -> Result<PathBuf> {
        if let Some(graph) = &self.package_graph {
            return graph.resolve_import_path(current_path, module_path);
        }
        checked_module_path(&self.package_root, module_path)
    }

    fn module_name_for_path(&self, path: &Path) -> String {
        self.package_graph
            .as_ref()
            .and_then(|graph| graph.module_name_for_path(path))
            .unwrap_or_else(|| logical_module_name(&self.package_root, path))
    }

    fn qualify_program_imported_modules(&self, path: &Path, program: &mut Program) {
        let Some(graph) = &self.package_graph else {
            return;
        };
        let Some(package) = graph.source_for_path(path) else {
            return;
        };
        let Some(prefix) = package.external_prefix.as_deref() else {
            return;
        };
        let dependency_aliases = graph.dependency_aliases_for_path(path);
        qualify_imported_module_namespaces(
            &mut program.imported_modules,
            prefix,
            &dependency_aliases,
        );
    }

    fn write_lockfile(&self) -> Result<()> {
        if let Some(graph) = &self.package_graph {
            graph.write_lockfile()?;
        }
        Ok(())
    }
}

fn absolutize(path: &Path) -> PathBuf {
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        match std::env::current_dir() {
            Ok(current) => current.join(path),
            Err(_) => path.to_path_buf(),
        }
    };
    if let Ok(canonical) = fs::canonicalize(&absolute) {
        return canonical;
    }

    let mut existing_ancestor = absolute.as_path();
    while !existing_ancestor.exists() {
        let Some(parent) = existing_ancestor.parent() else {
            return absolute;
        };
        existing_ancestor = parent;
    }

    let Ok(canonical_ancestor) = fs::canonicalize(existing_ancestor) else {
        return absolute;
    };
    let Ok(suffix) = absolute.strip_prefix(existing_ancestor) else {
        return absolute;
    };
    if suffix.as_os_str().is_empty() {
        canonical_ancestor
    } else {
        canonical_ancestor.join(suffix)
    }
}

fn infer_package_root(entry_path: &Path, source_override: Option<&str>) -> Result<PathBuf> {
    let entry_dir = entry_path.parent().unwrap_or(Path::new("."));

    let parsed_entry = source_override
        .map(str::to_string)
        .or_else(|| fs::read_to_string(entry_path).ok())
        .and_then(|source| parse_source(&source).ok());

    if let Some(module) = parsed_entry {
        let import_paths = module
            .imports
            .iter()
            .map(|import| match &import.kind {
                ImportKind::From { module_path, .. } => module_path.clone(),
                ImportKind::Module { path } => path.clone(),
            })
            .collect::<Vec<_>>();

        if !import_paths.is_empty() {
            for candidate in entry_dir.ancestors() {
                if import_paths
                    .iter()
                    .all(|import_path| import_exists_from_root(candidate, import_path))
                {
                    return canonicalize_if_exists(candidate);
                }
            }
        }
    }

    canonicalize_if_exists(entry_dir)
}

fn import_exists_from_root(root: &Path, module_path: &[String]) -> bool {
    checked_module_path(root, module_path)
        .map(|path| path.exists())
        .unwrap_or(false)
}

fn logical_module_name(package_root: &Path, path: &Path) -> String {
    let relative = path.strip_prefix(package_root).unwrap_or(path);
    let mut without_extension = relative.to_path_buf();
    without_extension.set_extension("");
    without_extension
        .iter()
        .map(|segment| segment.to_string_lossy().to_string())
        .collect::<Vec<_>>()
        .join(".")
}

fn checked_module_path(package_root: &Path, module_path: &[String]) -> Result<PathBuf> {
    let canonical_root = canonicalize_if_exists(package_root)?;
    let mut path = package_root.to_path_buf();
    for segment in module_path {
        path.push(segment);
    }
    path.set_extension("au");
    let canonical = canonicalize_if_exists(&path)?;
    if !canonical.starts_with(&canonical_root) {
        return Err(Diagnostic::new(format!(
            "resolved import path `{}` escapes package source root `{}`",
            canonical.display(),
            canonical_root.display()
        )));
    }
    Ok(canonical)
}

fn canonicalize_if_exists(path: &Path) -> Result<PathBuf> {
    if let Ok(canonical) = fs::canonicalize(path) {
        return Ok(canonical);
    }

    let mut existing_ancestor = path;
    while !existing_ancestor.exists() {
        let Some(parent) = existing_ancestor.parent() else {
            return Ok(path.to_path_buf());
        };
        existing_ancestor = parent;
    }

    let canonical_ancestor = match fs::canonicalize(existing_ancestor) {
        Ok(canonical) => canonical,
        Err(error) => {
            return Err(Diagnostic::new(format!(
                "failed to resolve path `{}`: {}",
                existing_ancestor.display(),
                error
            )));
        }
    };
    let Ok(suffix) = path.strip_prefix(existing_ancestor) else {
        return Ok(path.to_path_buf());
    };
    Ok(if suffix.as_os_str().is_empty() {
        canonical_ancestor
    } else {
        canonical_ancestor.join(suffix)
    })
}

fn qualify_imported_module_namespaces(
    modules: &mut BTreeMap<String, ModuleNamespace>,
    prefix: &str,
    dependency_aliases: &BTreeSet<String>,
) {
    for (name, namespace) in modules.iter_mut() {
        if dependency_aliases.contains(name) {
            continue;
        }
        qualify_namespace_path(namespace, prefix);
    }
}

fn qualify_namespace_path(namespace: &mut ModuleNamespace, prefix: &str) {
    namespace.path = format!("{}.{}", prefix, namespace.path);
    for module in namespace.modules.values_mut() {
        qualify_namespace_path(module, prefix);
    }
}

fn local_item_exists(program: &Program, name: &str) -> bool {
    program.module.items.iter().any(|item| item.name() == name)
}

fn is_builtin_export_type(name: &str) -> bool {
    matches!(
        name,
        "bool"
            | "int"
            | "int8"
            | "int16"
            | "int32"
            | "int64"
            | "int128"
            | "intsize"
            | "uint8"
            | "uint16"
            | "uint32"
            | "uint64"
            | "uint128"
            | "uintsize"
            | "float32"
            | "float64"
            | "String"
            | "Range"
            | "Option"
            | "Result"
            | "Task"
            | "SendError"
            | "TaskGroup"
            | "Duration"
    )
}

fn find_type_namespace_path(
    modules: &BTreeMap<String, ModuleNamespace>,
    name: &str,
    found: &mut Option<String>,
    ambiguous: &mut bool,
) {
    for namespace in modules.values() {
        if namespace.classes.contains_key(name)
            || namespace.all_classes.contains_key(name)
            || namespace.enums.contains_key(name)
            || namespace.all_enums.contains_key(name)
            || namespace.traits.contains_key(name)
            || namespace.all_traits.contains_key(name)
        {
            if let Some(existing) = found {
                if existing != &namespace.path {
                    *ambiguous = true;
                }
            } else {
                *found = Some(namespace.path.clone());
            }
        }
        find_type_namespace_path(&namespace.modules, name, found, ambiguous);
    }
}

fn qualify_export_type(program: &Program, ty: &sema::Type) -> sema::Type {
    match ty {
        sema::Type::Named(name, args) => {
            let qualified_args = args
                .iter()
                .map(|arg| qualify_export_type(program, arg))
                .collect::<Vec<_>>();
            if name.contains('.') || is_builtin_export_type(name) {
                return sema::Type::Named(name.clone(), qualified_args);
            }
            if program.classes.contains_key(name)
                || program.enums.contains_key(name)
                || program.traits.contains_key(name)
            {
                return sema::Type::Named(
                    format!("{}.{}", program.module_name, name),
                    qualified_args,
                );
            }
            let mut found = None;
            let mut ambiguous = false;
            find_type_namespace_path(&program.imported_modules, name, &mut found, &mut ambiguous);
            if let (Some(path), false) = (found, ambiguous) {
                return sema::Type::Named(format!("{}.{}", path, name), qualified_args);
            }
            sema::Type::Named(name.clone(), qualified_args)
        }
        sema::Type::Tuple(elements) => sema::Type::Tuple(
            elements
                .iter()
                .map(|element| qualify_export_type(program, element))
                .collect(),
        ),
        sema::Type::TypeParam(name) => sema::Type::TypeParam(name.clone()),
        sema::Type::Module(path) => sema::Type::Module(path.clone()),
        sema::Type::Unit => sema::Type::Unit,
    }
}

fn qualify_export_type_ref(program: &Program, type_ref: &ast::TypeRef) -> ast::TypeRef {
    let mut qualified = type_ref.clone();
    match &mut qualified.kind {
        ast::TypeRefKind::Tuple(elements) => {
            *elements = elements
                .iter()
                .map(|element| qualify_export_type_ref(program, element))
                .collect();
        }
        ast::TypeRefKind::Named { name, args } => {
            *args = args
                .iter()
                .map(|arg| qualify_export_type_ref(program, arg))
                .collect();
            if name.contains('.') || name == "str" || is_builtin_export_type(name) {
                return qualified;
            }
            if program.classes.contains_key(name)
                || program.enums.contains_key(name)
                || program.traits.contains_key(name)
            {
                *name = format!("{}.{}", program.module_name, name);
                return qualified;
            }
            let mut found = None;
            let mut ambiguous = false;
            find_type_namespace_path(&program.imported_modules, name, &mut found, &mut ambiguous);
            if let (Some(path), false) = (found, ambiguous) {
                *name = format!("{}.{}", path, name);
            }
        }
    }
    qualified
}

fn qualify_export_bounds(
    program: &Program,
    bounds: &BTreeMap<String, Vec<ast::TypeRef>>,
) -> BTreeMap<String, Vec<ast::TypeRef>> {
    bounds
        .iter()
        .map(|(name, refs)| {
            (
                name.clone(),
                refs.iter()
                    .map(|type_ref| qualify_export_type_ref(program, type_ref))
                    .collect(),
            )
        })
        .collect()
}

fn qualify_function_decl_for_export(
    program: &Program,
    decl: &ast::FunctionDecl,
) -> ast::FunctionDecl {
    let mut qualified = decl.clone();
    qualified.type_param_bounds = qualify_export_bounds(program, &qualified.type_param_bounds);
    qualified.params = qualified
        .params
        .iter()
        .map(|param| {
            let mut qualified_param = param.clone();
            qualified_param.ty = qualify_export_type_ref(program, &qualified_param.ty);
            qualified_param
        })
        .collect();
    qualified.return_type = qualify_export_type_ref(program, &qualified.return_type);
    qualified
}

fn qualify_class_decl_for_export(program: &Program, decl: &ast::ClassDecl) -> ast::ClassDecl {
    let mut qualified = decl.clone();
    qualified.type_param_bounds = qualify_export_bounds(program, &qualified.type_param_bounds);
    qualified.fields = qualified
        .fields
        .iter()
        .map(|field| {
            let mut qualified_field = field.clone();
            qualified_field.ty = qualify_export_type_ref(program, &qualified_field.ty);
            qualified_field
        })
        .collect();
    qualified.methods = qualified
        .methods
        .iter()
        .map(|method| qualify_function_decl_for_export(program, method))
        .collect();
    qualified
}

fn qualify_enum_decl_for_export(program: &Program, decl: &ast::EnumDecl) -> ast::EnumDecl {
    let mut qualified = decl.clone();
    qualified.type_param_bounds = qualify_export_bounds(program, &qualified.type_param_bounds);
    qualified.variants = qualified
        .variants
        .iter()
        .map(|variant| {
            let mut qualified_variant = variant.clone();
            qualified_variant.payloads = qualified_variant
                .payloads
                .iter()
                .map(|payload| {
                    let mut qualified_payload = payload.clone();
                    qualified_payload.ty = qualify_export_type_ref(program, &payload.ty);
                    qualified_payload
                })
                .collect();
            qualified_variant
        })
        .collect();
    qualified
}

fn qualify_trait_decl_for_export(program: &Program, decl: &ast::TraitDecl) -> ast::TraitDecl {
    let mut qualified = decl.clone();
    qualified.methods = qualified
        .methods
        .iter()
        .map(|method| qualify_function_decl_for_export(program, method))
        .collect();
    qualified
}

fn qualify_impl_decl_for_export(program: &Program, decl: &ast::ImplDecl) -> ast::ImplDecl {
    let mut qualified = decl.clone();
    qualified.type_param_bounds = qualify_export_bounds(program, &qualified.type_param_bounds);
    qualified.trait_args = qualified
        .trait_args
        .iter()
        .map(|arg| qualify_export_type_ref(program, arg))
        .collect();
    qualified.for_type = qualify_export_type_ref(program, &qualified.for_type);
    qualified.methods = qualified
        .methods
        .iter()
        .map(|method| qualify_function_decl_for_export(program, method))
        .collect();
    qualified
}

fn qualify_function_info_for_export(
    program: &Program,
    info: &sema::FunctionInfo,
) -> sema::FunctionInfo {
    let mut qualified = info.clone();
    qualified.decl = qualify_function_decl_for_export(program, &qualified.decl);
    qualified.signature.params = qualified
        .signature
        .params
        .iter()
        .map(|ty| qualify_export_type(program, ty))
        .collect();
    qualified.signature.return_type =
        qualify_export_type(program, &qualified.signature.return_type);
    qualified
}

fn qualify_class_info_for_export(program: &Program, info: &sema::ClassInfo) -> sema::ClassInfo {
    let mut qualified = info.clone();
    qualified.decl = qualify_class_decl_for_export(program, &qualified.decl);
    for field in qualified.fields.values_mut() {
        field.ty = qualify_export_type(program, &field.ty);
    }
    for method in qualified.methods.values_mut() {
        method.decl = qualify_function_decl_for_export(program, &method.decl);
        method.signature.params = method
            .signature
            .params
            .iter()
            .map(|ty| qualify_export_type(program, ty))
            .collect();
        method.signature.return_type = qualify_export_type(program, &method.signature.return_type);
    }
    qualified
}

fn qualify_enum_info_for_export(program: &Program, info: &sema::EnumInfo) -> sema::EnumInfo {
    let mut qualified = info.clone();
    qualified.decl = qualify_enum_decl_for_export(program, &qualified.decl);
    for variant in qualified.variants.values_mut() {
        variant.payloads = variant
            .payloads
            .iter()
            .map(|payload| sema::EnumPayloadFieldInfo {
                name: payload.name.clone(),
                ty: qualify_export_type(program, &payload.ty),
                span: payload.span,
            })
            .collect();
    }
    qualified
}

fn qualify_trait_info_for_export(program: &Program, info: &sema::TraitInfo) -> sema::TraitInfo {
    let mut qualified = info.clone();
    qualified.decl = qualify_trait_decl_for_export(program, &qualified.decl);
    for method in qualified.methods.values_mut() {
        method.decl = qualify_function_decl_for_export(program, &method.decl);
        method.signature.params = method
            .signature
            .params
            .iter()
            .map(|ty| qualify_export_type(program, ty))
            .collect();
        method.signature.return_type = qualify_export_type(program, &method.signature.return_type);
    }
    qualified
}

fn qualify_trait_impl_info_for_export(
    program: &Program,
    info: &sema::TraitImplInfo,
) -> sema::TraitImplInfo {
    let mut qualified = info.clone();
    qualified.decl = qualify_impl_decl_for_export(program, &qualified.decl);
    qualified.trait_args = qualified
        .trait_args
        .iter()
        .map(|ty| qualify_export_type(program, ty))
        .collect();
    qualified.for_type = qualify_export_type(program, &qualified.for_type);
    for method in qualified.methods.values_mut() {
        method.decl = qualify_function_decl_for_export(program, &method.decl);
        method.signature.params = method
            .signature
            .params
            .iter()
            .map(|ty| qualify_export_type(program, ty))
            .collect();
        method.signature.return_type = qualify_export_type(program, &method.signature.return_type);
    }
    qualified
}

fn exported_binding(program: &Program, name: &str) -> Option<ImportedBinding> {
    for item in &program.module.items {
        match item {
            Item::Function(decl) if decl.name == name && decl.public => {
                return program
                    .functions
                    .get(name)
                    .map(|info| qualify_function_info_for_export(program, info))
                    .map(ImportedBinding::Function);
            }
            Item::Class(decl) if decl.name == name && decl.public => {
                return program
                    .classes
                    .get(name)
                    .map(|info| qualify_class_info_for_export(program, info))
                    .map(ImportedBinding::Class);
            }
            Item::Enum(decl) if decl.name == name && decl.public => {
                return program
                    .enums
                    .get(name)
                    .map(|info| qualify_enum_info_for_export(program, info))
                    .map(ImportedBinding::Enum);
            }
            Item::Trait(decl) if decl.name == name && decl.public => {
                return program
                    .traits
                    .get(name)
                    .map(|info| qualify_trait_info_for_export(program, info))
                    .map(ImportedBinding::Trait);
            }
            _ => {}
        }
    }
    None
}

fn exported_namespace(path: &[String], program: &Program) -> ModuleNamespace {
    let name = path
        .last()
        .cloned()
        .unwrap_or_else(|| program.module_name.clone());
    let mut namespace = ModuleNamespace {
        name,
        path: path.join("."),
        source_path: program.source_path.clone(),
        modules: BTreeMap::new(),
        functions: BTreeMap::new(),
        classes: BTreeMap::new(),
        enums: BTreeMap::new(),
        traits: BTreeMap::new(),
        trait_impls: program
            .trait_impls
            .iter()
            .map(|info| qualify_trait_impl_info_for_export(program, info))
            .collect(),
        all_functions: program
            .functions
            .iter()
            .map(|(name, info)| {
                (
                    name.clone(),
                    qualify_function_info_for_export(program, info),
                )
            })
            .collect(),
        all_classes: program
            .classes
            .iter()
            .map(|(name, info)| (name.clone(), qualify_class_info_for_export(program, info)))
            .collect(),
        all_enums: program
            .enums
            .iter()
            .map(|(name, info)| (name.clone(), qualify_enum_info_for_export(program, info)))
            .collect(),
        all_traits: program
            .traits
            .iter()
            .map(|(name, info)| (name.clone(), qualify_trait_info_for_export(program, info)))
            .collect(),
        imported_modules: program.imported_modules.clone(),
    };

    for item in &program.module.items {
        match item {
            Item::Function(decl) if decl.public => {
                if let Some(info) = program.functions.get(&decl.name) {
                    namespace.functions.insert(
                        decl.name.clone(),
                        qualify_function_info_for_export(program, info),
                    );
                }
            }
            Item::Class(decl) if decl.public => {
                if let Some(info) = program.classes.get(&decl.name) {
                    namespace.classes.insert(
                        decl.name.clone(),
                        qualify_class_info_for_export(program, info),
                    );
                }
            }
            Item::Enum(decl) if decl.public => {
                if let Some(info) = program.enums.get(&decl.name) {
                    namespace.enums.insert(
                        decl.name.clone(),
                        qualify_enum_info_for_export(program, info),
                    );
                }
            }
            Item::Trait(decl) if decl.public => {
                if let Some(info) = program.traits.get(&decl.name) {
                    namespace.traits.insert(
                        decl.name.clone(),
                        qualify_trait_info_for_export(program, info),
                    );
                }
            }
            _ => {}
        }
    }

    namespace
}

fn insert_namespace_import(
    bindings: &mut BTreeMap<String, ImportedBinding>,
    path: &[String],
    leaf: ModuleNamespace,
    span: Span,
) -> Result<()> {
    if path.is_empty() {
        return Ok(());
    }
    let root_name = path[0].clone();
    let root = bindings.entry(root_name.clone()).or_insert_with(|| {
        ImportedBinding::Module(ModuleNamespace {
            name: root_name.clone(),
            path: root_name.clone(),
            source_path: None,
            modules: BTreeMap::new(),
            functions: BTreeMap::new(),
            classes: BTreeMap::new(),
            enums: BTreeMap::new(),
            traits: BTreeMap::new(),
            trait_impls: Vec::new(),
            all_functions: BTreeMap::new(),
            all_classes: BTreeMap::new(),
            all_enums: BTreeMap::new(),
            all_traits: BTreeMap::new(),
            imported_modules: BTreeMap::new(),
        })
    });
    let ImportedBinding::Module(root_namespace) = root else {
        return Err(Diagnostic::at(
            span,
            format!("duplicate import binding `{}`", root_name),
        ));
    };

    if path.len() == 1 {
        *root_namespace = leaf;
        return Ok(());
    }

    let mut current = root_namespace;
    let mut prefix = root_name.clone();
    for segment in &path[1..path.len() - 1] {
        prefix = format!("{}.{}", prefix, segment);
        current = current
            .modules
            .entry(segment.clone())
            .or_insert_with(|| ModuleNamespace {
                name: segment.clone(),
                path: prefix.clone(),
                source_path: None,
                modules: BTreeMap::new(),
                functions: BTreeMap::new(),
                classes: BTreeMap::new(),
                enums: BTreeMap::new(),
                traits: BTreeMap::new(),
                trait_impls: Vec::new(),
                all_functions: BTreeMap::new(),
                all_classes: BTreeMap::new(),
                all_enums: BTreeMap::new(),
                all_traits: BTreeMap::new(),
                imported_modules: BTreeMap::new(),
            });
    }
    let last = path[path.len() - 1].clone();
    current.modules.insert(last, leaf);
    Ok(())
}

#[cfg(test)]
#[path = "lib_tests.rs"]
mod tests;
