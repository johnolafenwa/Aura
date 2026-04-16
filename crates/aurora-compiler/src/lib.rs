pub mod analysis;
pub mod ast;
pub mod call;
pub mod diag;
pub mod integer;
pub mod lexer;
pub mod mir;
pub mod mir_runtime;
mod native_codegen;
mod native_runtime;
mod package;
pub mod parser;
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
pub use mir_runtime::{run as run_mir, run_serialized_mir};
pub use native_codegen::{
    emit_host_object as emit_host_native_object,
    emit_host_object_with_metadata as emit_host_native_object_with_metadata,
};
pub use runtime_value::{RunOutput, Value};
pub use sema::{ImportedBinding, ModuleContext, ModuleNamespace, Program};

use ast::{ImportKind, Item};
pub use package::DependencyUpdateResult;
use package::PackageGraph;
pub fn parse_source(source: &str) -> Result<ast::Module> {
    parser::parse(source)
}

pub fn check_source(source: &str) -> Result<Program> {
    let module = parse_source(source)?;
    sema::check(module)
}

pub fn run_source(source: &str) -> Result<RunOutput> {
    let program = check_source(source)?;
    let mir = lower_to_mir(&program);
    run_mir(&mir)
}

pub fn run_path_with_source(path: &Path, source: &str) -> Result<RunOutput> {
    let program = check_path_with_source(path, source)?;
    let mir = lower_to_mir(&program);
    run_mir(&mir)
}

pub fn run_source_via_mir(source: &str) -> Result<RunOutput> {
    run_source(source)
}

pub fn run_path_with_source_via_mir(path: &Path, source: &str) -> Result<RunOutput> {
    run_path_with_source(path, source)
}

pub fn lower_source_to_mir(source: &str) -> Result<MirModule> {
    let program = check_source(source)?;
    Ok(lower_to_mir(&program))
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
    let mut loader = ModuleLoader::new_with_source(path, Some(source))?;
    let program = loader.load_program_with_source(path, source)?;
    loader.write_lockfile()?;
    Ok(program)
}

pub fn run_path(path: &Path) -> Result<RunOutput> {
    let program = check_path(path)?;
    let mir = lower_to_mir(&program);
    run_mir(&mir)
}

pub fn run_path_via_mir(path: &Path) -> Result<RunOutput> {
    run_path(path)
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

        let source = if let Some(source) = source_override {
            source.to_string()
        } else {
            fs::read_to_string(&path).map_err(|error| {
                Diagnostic::new(format!("failed to read `{}`: {}", path.display(), error))
            })?
        };
        let module = parse_source(&source)?;
        let module_name = self.module_name_for_path(&path);
        let imported_bindings = self.resolve_imports(&module, &path)?;
        let module_registry = self.build_module_registry();
        let program = sema::check_with_context(
            module,
            ModuleContext {
                module_name,
                imported_bindings,
                module_registry,
            },
        )?;
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
                    let imported = self.load_imported_module(current_path, path, import.span)?;
                    let leaf = exported_namespace(path, &imported);
                    insert_namespace_import(&mut bindings, path, leaf, import.span)?;
                }
            }
        }
        Ok(bindings)
    }

    fn build_module_registry(&self) -> BTreeMap<String, ModuleNamespace> {
        self.cache
            .values()
            .map(|loaded| {
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
            })
            .collect()
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
        let mut path = self.package_root.clone();
        for segment in module_path {
            path.push(segment);
        }
        path.set_extension("au");
        Ok(path)
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
        std::env::current_dir()
            .expect("current directory should be available")
            .join(path)
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
    let entry_dir = entry_path.parent().unwrap_or_else(|| Path::new("."));

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
                    return Ok(candidate.to_path_buf());
                }
            }
        }
    }

    Ok(entry_dir.to_path_buf())
}

fn import_exists_from_root(root: &Path, module_path: &[String]) -> bool {
    let mut path = root.to_path_buf();
    for segment in module_path {
        path.push(segment);
    }
    path.set_extension("au");
    path.exists()
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
            | "Channel"
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
        sema::Type::TypeParam(name) => sema::Type::TypeParam(name.clone()),
        sema::Type::Module(path) => sema::Type::Module(path.clone()),
        sema::Type::Unit => sema::Type::Unit,
    }
}

fn qualify_export_type_ref(program: &Program, type_ref: &ast::TypeRef) -> ast::TypeRef {
    let mut qualified = type_ref.clone();
    qualified.args = qualified
        .args
        .iter()
        .map(|arg| qualify_export_type_ref(program, arg))
        .collect();
    if qualified.name.contains('.')
        || qualified.name == "str"
        || is_builtin_export_type(&qualified.name)
    {
        return qualified;
    }
    if program.classes.contains_key(&qualified.name)
        || program.enums.contains_key(&qualified.name)
        || program.traits.contains_key(&qualified.name)
    {
        qualified.name = format!("{}.{}", program.module_name, qualified.name);
        return qualified;
    }
    let mut found = None;
    let mut ambiguous = false;
    find_type_namespace_path(
        &program.imported_modules,
        &qualified.name,
        &mut found,
        &mut ambiguous,
    );
    if let (Some(path), false) = (found, ambiguous) {
        qualified.name = format!("{}.{}", path, qualified.name);
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
    let mut prefix = root_name;
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
    current.modules.insert(
        path.last().cloned().expect("path should be non-empty"),
        leaf,
    );
    Ok(())
}

#[cfg(test)]
#[path = "lib_tests.rs"]
mod tests;
