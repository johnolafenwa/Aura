pub mod analysis;
pub mod ast;
pub mod call;
pub mod diag;
pub mod integer;
pub mod interpreter;
pub mod lexer;
pub mod mir;
pub mod mir_runtime;
mod native_codegen;
mod native_runtime;
pub mod parser;
pub mod sema;

use std::fs;
use std::path::{Path, PathBuf};
use std::{collections::BTreeMap, collections::HashMap};

pub use analysis::{
    analyze_path_source, analyze_program, analyze_source, complete_path_source, complete_source,
    AnalysisCompletion, AnalysisOutput,
};
pub use diag::{Diagnostic, Result, Span};
pub use interpreter::{run, RunOutput, Value};
pub use mir::{lower as lower_to_mir, MirModule};
pub use mir_runtime::{run as run_mir, run_serialized_mir};
pub use native_codegen::{
    emit_host_object as emit_host_native_object,
    emit_host_object_with_metadata as emit_host_native_object_with_metadata,
};
pub use sema::{ImportedBinding, ModuleContext, ModuleNamespace, Program};

use ast::{ImportKind, Item};
pub fn parse_source(source: &str) -> Result<ast::Module> {
    parser::parse(source)
}

pub fn check_source(source: &str) -> Result<Program> {
    let module = parse_source(source)?;
    sema::check(module)
}

pub fn run_source(source: &str) -> Result<RunOutput> {
    let program = check_source(source)?;
    run(&program)
}

pub fn run_path_with_source(path: &Path, source: &str) -> Result<RunOutput> {
    let program = check_path_with_source(path, source)?;
    run(&program)
}

pub fn run_source_via_mir(source: &str) -> Result<RunOutput> {
    let program = check_source(source)?;
    let mir = lower_to_mir(&program);
    run_mir(&mir)
}

pub fn run_path_with_source_via_mir(path: &Path, source: &str) -> Result<RunOutput> {
    let program = check_path_with_source(path, source)?;
    let mir = lower_to_mir(&program);
    run_mir(&mir)
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
    ModuleLoader::new(path)?.load_program(path)
}

pub fn check_path_with_source(path: &Path, source: &str) -> Result<Program> {
    ModuleLoader::new_with_source(path, Some(source))?.load_program_with_source(path, source)
}

pub fn run_path(path: &Path) -> Result<RunOutput> {
    let program = check_path(path)?;
    run(&program)
}

pub fn run_path_via_mir(path: &Path) -> Result<RunOutput> {
    let program = check_path(path)?;
    let mir = lower_to_mir(&program);
    run_mir(&mir)
}

pub fn lower_path_to_mir(path: &Path) -> Result<MirModule> {
    let program = check_path(path)?;
    Ok(lower_to_mir(&program))
}

#[derive(Clone)]
struct LoadedModule {
    program: Program,
}

struct ModuleLoader {
    package_root: PathBuf,
    cache: HashMap<PathBuf, LoadedModule>,
    stack: Vec<PathBuf>,
}

impl ModuleLoader {
    fn new(entry_path: &Path) -> Result<Self> {
        Self::new_with_source(entry_path, None)
    }

    fn new_with_source(entry_path: &Path, source_override: Option<&str>) -> Result<Self> {
        let absolute_entry = absolutize(entry_path);
        let package_root = infer_package_root(&absolute_entry, source_override)?;
        Ok(Self {
            package_root,
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
        let module_name = logical_module_name(&self.package_root, &path);
        let imported_bindings = self.resolve_imports(&module)?;
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
    ) -> Result<BTreeMap<String, ImportedBinding>> {
        let mut bindings = BTreeMap::new();
        for import in &module.imports {
            match &import.kind {
                ImportKind::From { module_path, names } => {
                    let imported = self.load_imported_module(module_path, import.span)?;
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
                    let imported = self.load_imported_module(path, import.span)?;
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

    fn load_imported_module(&mut self, module_path: &[String], span: Span) -> Result<Program> {
        let mut path = self.package_root.clone();
        for segment in module_path {
            path.push(segment);
        }
        path.set_extension("au");
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
}

fn absolutize(path: &Path) -> PathBuf {
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .expect("current directory should be available")
            .join(path)
    }
}

fn infer_package_root(entry_path: &Path, source_override: Option<&str>) -> Result<PathBuf> {
    let entry_dir = entry_path.parent().ok_or_else(|| {
        Diagnostic::new(format!(
            "cannot determine package root for `{}`",
            entry_path.display()
        ))
    })?;

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
            qualified_variant.payload = qualified_variant
                .payload
                .as_ref()
                .map(|payload| qualify_export_type_ref(program, payload));
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
        if let Some(payload) = &variant.payload {
            variant.payload = Some(qualify_export_type(program, payload));
        }
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
mod tests {
    use super::{
        check_path, check_source, lower_path_with_source_to_mir, lower_source_to_mir, parse_source,
        run_mir, run_path, run_path_via_mir, run_serialized_mir, run_source, run_source_via_mir,
        Value,
    };
    use std::fs;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    const POINT_SOURCE: &str = include_str!("../../../examples/point.au");
    const BASIC_ADDITION_SOURCE: &str = include_str!("../../../examples/basic_addition.au");
    const TOP_LEVEL_ADDITION_SOURCE: &str = include_str!("../../../examples/top_level_addition.au");
    const CONTROL_FLOW_SOURCE: &str = include_str!("../../../examples/control_flow.au");
    const EXAMPLE_CASES: &[(&str, &str)] = &[
        (
            "examples/basics/top_level_script.au",
            include_str!("../../../examples/basics/top_level_script.au"),
        ),
        (
            "examples/basics/main_function.au",
            include_str!("../../../examples/basics/main_function.au"),
        ),
        (
            "examples/basics/mutable_bindings.au",
            include_str!("../../../examples/basics/mutable_bindings.au"),
        ),
        (
            "examples/basics/default_arguments.au",
            include_str!("../../../examples/basics/default_arguments.au"),
        ),
        (
            "examples/basics/pass_keyword.au",
            include_str!("../../../examples/basics/pass_keyword.au"),
        ),
        (
            "examples/classes/point_distance.au",
            include_str!("../../../examples/classes/point_distance.au"),
        ),
        (
            "examples/classes/default_fields.au",
            include_str!("../../../examples/classes/default_fields.au"),
        ),
        (
            "examples/classes/methods.au",
            include_str!("../../../examples/classes/methods.au"),
        ),
        (
            "examples/control_flow/if_elif_else.au",
            include_str!("../../../examples/control_flow/if_elif_else.au"),
        ),
        (
            "examples/control_flow/for_range.au",
            include_str!("../../../examples/control_flow/for_range.au"),
        ),
        (
            "examples/control_flow/while_break_continue.au",
            include_str!("../../../examples/control_flow/while_break_continue.au"),
        ),
        (
            "examples/enums/result_match.au",
            include_str!("../../../examples/enums/result_match.au"),
        ),
        (
            "examples/enums/result_option.au",
            include_str!("../../../examples/enums/result_option.au"),
        ),
        (
            "examples/enums/explicit_type_args.au",
            include_str!("../../../examples/enums/explicit_type_args.au"),
        ),
        (
            "examples/generics/box_and_wrapper.au",
            include_str!("../../../examples/generics/box_and_wrapper.au"),
        ),
        (
            "examples/traits/greeter.au",
            include_str!("../../../examples/traits/greeter.au"),
        ),
        (
            "examples/traits/multiple_bounds.au",
            include_str!("../../../examples/traits/multiple_bounds.au"),
        ),
        (
            "examples/numbers/float_sqrt.au",
            include_str!("../../../examples/numbers/float_sqrt.au"),
        ),
        (
            "examples/numbers/float32_values.au",
            include_str!("../../../examples/numbers/float32_values.au"),
        ),
        (
            "examples/numbers/numeric_casts.au",
            include_str!("../../../examples/numbers/numeric_casts.au"),
        ),
        (
            "examples/strings/greeting.au",
            include_str!("../../../examples/strings/greeting.au"),
        ),
        (
            "examples/concurrency/task_group_select.au",
            include_str!("../../../examples/concurrency/task_group_select.au"),
        ),
        (
            "examples/concurrency/task_group_cancel.au",
            include_str!("../../../examples/concurrency/task_group_cancel.au"),
        ),
        (
            "examples/concurrency/select_timeout.au",
            include_str!("../../../examples/concurrency/select_timeout.au"),
        ),
        (
            "examples/concurrency/sleep_builtin.au",
            include_str!("../../../examples/concurrency/sleep_builtin.au"),
        ),
        (
            "examples/concurrency/send_result.au",
            include_str!("../../../examples/concurrency/send_result.au"),
        ),
        (
            "examples/concurrency/spawn_detached.au",
            include_str!("../../../examples/concurrency/spawn_detached.au"),
        ),
        (
            "examples/concurrency/select_send.au",
            include_str!("../../../examples/concurrency/select_send.au"),
        ),
        (
            "examples/enums/wildcard_match.au",
            include_str!("../../../examples/enums/wildcard_match.au"),
        ),
        (
            "examples/generics/generic_method_calls.au",
            include_str!("../../../examples/generics/generic_method_calls.au"),
        ),
        (
            "examples/generics/bounded_types.au",
            include_str!("../../../examples/generics/bounded_types.au"),
        ),
        (
            "examples/traits/marker_trait.au",
            include_str!("../../../examples/traits/marker_trait.au"),
        ),
        (
            "examples/traits/specialized_generic_impl.au",
            include_str!("../../../examples/traits/specialized_generic_impl.au"),
        ),
        (
            "examples/concurrency/minute_duration.au",
            include_str!("../../../examples/concurrency/minute_duration.au"),
        ),
        (
            "examples/traits/generic_dispatch_multiple_types.au",
            include_str!("../../../examples/traits/generic_dispatch_multiple_types.au"),
        ),
        (
            "examples/strings/string_methods.au",
            include_str!("../../../examples/strings/string_methods.au"),
        ),
        (
            "examples/numbers/numeric_builtins.au",
            include_str!("../../../examples/numbers/numeric_builtins.au"),
        ),
        (
            "examples/collections/map_basics.au",
            include_str!("../../../examples/collections/map_basics.au"),
        ),
        (
            "examples/collections/set_basics.au",
            include_str!("../../../examples/collections/set_basics.au"),
        ),
        (
            "examples/strings/string_parsing_and_formatting.au",
            include_str!("../../../examples/strings/string_parsing_and_formatting.au"),
        ),
        (
            "examples/traits/generic_trait_bounds.au",
            include_str!("../../../examples/traits/generic_trait_bounds.au"),
        ),
        (
            "examples/traits/operator_traits.au",
            include_str!("../../../examples/traits/operator_traits.au"),
        ),
    ];

    struct TempDir {
        path: PathBuf,
    }

    impl TempDir {
        fn new(prefix: &str) -> Self {
            let unique = format!(
                "{}-{}-{}",
                prefix,
                std::process::id(),
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .expect("system time should be after unix epoch")
                    .as_nanos()
            );
            let path = std::env::temp_dir().join(unique);
            fs::create_dir_all(&path).expect("failed to create temp dir");
            Self { path }
        }

        fn path(&self) -> &PathBuf {
            &self.path
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.path);
        }
    }

    fn zero_exit_value() -> Value {
        Value::Int(crate::integer::IntegerValue::zero())
    }

    #[test]
    fn parses_the_point_milestone() {
        let module = parse_source(POINT_SOURCE).expect("point program should parse");
        assert_eq!(module.items.len(), 3);
        assert_eq!(module.top_level_stmts.len(), 0);
    }

    #[test]
    fn type_checks_the_point_milestone() {
        check_source(POINT_SOURCE).expect("point program should type-check");
    }

    #[test]
    fn runs_the_point_milestone() {
        let output = run_source(POINT_SOURCE).expect("point program should run");
        assert_eq!(output.stdout, "5.0\n");
        assert_eq!(output.value, zero_exit_value());
    }

    #[test]
    fn mir_runtime_runs_the_point_milestone() {
        let output = run_source_via_mir(POINT_SOURCE).expect("point program should run via MIR");
        assert_eq!(output.stdout, "5.0\n");
        assert_eq!(output.value, zero_exit_value());
    }

    #[test]
    fn omitted_none_return_type_is_allowed() {
        let module = parse_source(BASIC_ADDITION_SOURCE).expect("basic addition should parse");
        assert_eq!(module.items.len(), 1);
        assert_eq!(module.top_level_stmts.len(), 0);

        let output = run_source(BASIC_ADDITION_SOURCE).expect("basic addition should run");
        assert_eq!(output.stdout, "16\n");
        assert_eq!(output.value, Value::Unit);
    }

    #[test]
    fn top_level_scripts_run_without_main() {
        let module =
            parse_source(TOP_LEVEL_ADDITION_SOURCE).expect("top-level addition should parse");
        assert_eq!(module.items.len(), 0);
        assert_eq!(module.top_level_stmts.len(), 4);

        let output = run_source(TOP_LEVEL_ADDITION_SOURCE).expect("top-level addition should run");
        assert_eq!(output.stdout, "16\n");
        assert_eq!(output.value, zero_exit_value());
    }

    #[test]
    fn control_flow_example_runs() {
        check_source(CONTROL_FLOW_SOURCE).expect("control flow example should type-check");
        let output = run_source(CONTROL_FLOW_SOURCE).expect("control flow example should run");
        assert_eq!(output.stdout, "ok\n");
        assert_eq!(output.value, zero_exit_value());
    }

    #[test]
    fn mir_runtime_runs_class_methods_example() {
        let source = include_str!("../../../examples/classes/methods.au");
        let output = run_source_via_mir(source).expect("methods example should run via MIR");
        assert_eq!(output.stdout, "4\n8\n0\n");
        assert_eq!(output.value, zero_exit_value());
    }

    #[test]
    fn mir_runtime_runs_enum_match_example() {
        let source = include_str!("../../../examples/enums/result_match.au");
        let output = run_source_via_mir(source).expect("enum match example should run via MIR");
        assert_eq!(output.stdout, "42\nbad\n0\n");
        assert_eq!(output.value, zero_exit_value());
    }

    #[test]
    fn mir_runtime_runs_try_example_natively() {
        let source = include_str!("../../../examples/error_handling/try_result.au");
        let mir = lower_source_to_mir(source).expect("try example should lower to MIR");
        let output = run_mir(&mir).expect("try example should run directly through MIR");
        assert_eq!(output.stdout, "6\ndivision by zero\n");
        assert_eq!(output.value, zero_exit_value());
    }

    #[test]
    fn backend_path_runs_try_example_natively() {
        let source = include_str!("../../../examples/error_handling/try_result.au");
        let output =
            run_source_via_mir(source).expect("try example should run through backend path");
        assert_eq!(output.stdout, "6\ndivision by zero\n");
        assert_eq!(output.value, zero_exit_value());
    }

    #[test]
    fn backend_path_runs_with_example_natively() {
        let source = include_str!("../../../examples/resources/with_resource.au");
        let output =
            run_source_via_mir(source).expect("with example should run through backend path");
        assert_eq!(output.stdout, "demo\nclosed demo\ndone\n");
        assert_eq!(output.value, zero_exit_value());
    }

    #[test]
    fn mir_runtime_runs_with_example_natively() {
        let source = include_str!("../../../examples/resources/with_resource.au");
        let mir = lower_source_to_mir(source).expect("with example should lower to MIR");
        let output = run_mir(&mir).expect("with example should run directly through MIR");
        assert_eq!(output.stdout, "demo\nclosed demo\ndone\n");
        assert_eq!(output.value, zero_exit_value());
    }

    #[test]
    fn mir_runtime_runs_channels_example_natively() {
        let source = include_str!("../../../examples/concurrency/channels_spawn.au");
        let mir = lower_source_to_mir(source).expect("channels example should lower to MIR");
        let output = run_mir(&mir).expect("channels example should run directly through MIR");
        assert_eq!(output.stdout, "2\n4\n");
        assert_eq!(output.value, zero_exit_value());
    }

    #[test]
    fn mir_runtime_runs_send_result_example_natively() {
        let source = include_str!("../../../examples/concurrency/send_result.au");
        let mir = lower_source_to_mir(source).expect("send_result example should lower to MIR");
        let output = run_mir(&mir).expect("send_result example should run directly through MIR");
        assert_eq!(output.stdout, "7\n");
        assert_eq!(output.value, zero_exit_value());
    }

    #[test]
    fn mir_runtime_runs_spawn_detached_example_natively() {
        let source = include_str!("../../../examples/concurrency/spawn_detached.au");
        let mir = lower_source_to_mir(source).expect("spawn_detached example should lower to MIR");
        let output = run_mir(&mir).expect("spawn_detached example should run directly through MIR");
        assert_eq!(output.stdout, "9\n");
        assert_eq!(output.value, zero_exit_value());
    }

    #[test]
    fn mir_runtime_runs_select_timeout_example_natively() {
        let source = include_str!("../../../examples/concurrency/select_timeout.au");
        let mir = lower_source_to_mir(source).expect("select_timeout example should lower to MIR");
        let output = run_mir(&mir).expect("select_timeout example should run directly through MIR");
        assert_eq!(output.stdout, "timeout\n");
        assert_eq!(output.value, zero_exit_value());
    }

    #[test]
    fn mir_runtime_runs_select_send_example_natively() {
        let source = include_str!("../../../examples/concurrency/select_send.au");
        let mir = lower_source_to_mir(source).expect("select_send example should lower to MIR");
        let output = run_mir(&mir).expect("select_send example should run directly through MIR");
        assert_eq!(output.stdout, "sent\n4\n");
        assert_eq!(output.value, zero_exit_value());
    }

    #[test]
    fn mir_runtime_runs_task_group_select_example_natively() {
        let source = include_str!("../../../examples/concurrency/task_group_select.au");
        let mir =
            lower_source_to_mir(source).expect("task_group_select example should lower to MIR");
        let output =
            run_mir(&mir).expect("task_group_select example should run directly through MIR");
        assert_eq!(output.stdout, "3\n");
        assert_eq!(output.value, zero_exit_value());
    }

    #[test]
    fn mir_runtime_runs_task_group_cancel_example_natively() {
        let source = include_str!("../../../examples/concurrency/task_group_cancel.au");
        let mir =
            lower_source_to_mir(source).expect("task_group_cancel example should lower to MIR");
        let output =
            run_mir(&mir).expect("task_group_cancel example should run directly through MIR");
        assert_eq!(output.stdout, "0\n1\n");
        assert_eq!(output.value, zero_exit_value());
    }

    #[test]
    fn serialized_mir_runner_executes_point_example() {
        let source = include_str!("../../../examples/point.au");
        let mir = lower_source_to_mir(source).expect("point example should lower to MIR");
        let mir_json = serde_json::to_vec(&mir).expect("MIR should serialize to JSON bytes");
        let output = run_serialized_mir(&mir_json, "/virtual/point.au", source)
            .expect("serialized MIR runner should execute point example");
        assert_eq!(output.stdout, "5.0\n");
        assert_eq!(output.value, zero_exit_value());
    }

    #[test]
    fn serialized_mir_runner_reports_invalid_embedded_mir() {
        let error = run_serialized_mir(b"{not json", "/virtual/bad.au", "print(value=1)\n")
            .expect_err("invalid embedded MIR should return a diagnostic");
        assert!(
            error.message.contains("failed to deserialize embedded MIR"),
            "unexpected diagnostic: {}",
            error
        );
    }

    #[test]
    fn path_with_source_mir_lowering_resolves_local_module_imports() {
        let temp = TempDir::new("aurora-compiler-lower-path-source");
        fs::create_dir_all(temp.path().join("helpers")).expect("failed to create helper dir");
        fs::write(
            temp.path().join("helpers/math.au"),
            "public def double(value: int32) -> int32:\n    return value * 2\n",
        )
        .expect("failed to write helper module");
        let main_path = temp.path().join("main.au");
        let source = "import helpers.math\n\ndef main() -> int32:\n    print(helpers.math.double(value=5))\n    return 0\n";
        let mir = lower_path_with_source_to_mir(&main_path, source)
            .expect("path-aware MIR lowering should resolve local imports");
        let output = run_mir(&mir).expect("path-aware MIR lowering should produce runnable MIR");
        assert_eq!(output.stdout, "10\n");
        assert_eq!(output.value, zero_exit_value());
    }

    #[test]
    fn imported_function_return_types_keep_members_visible_across_modules() {
        let temp = TempDir::new("aurora-compiler-imported-return-members");
        fs::create_dir_all(temp.path().join("helpers")).expect("failed to create helper dir");
        fs::write(
            temp.path().join("helpers/counter.au"),
            [
                "public class Counter:",
                "    public value: int32",
                "",
                "public def make_counter() -> Counter:",
                "    return Counter(value=41)",
                "",
            ]
            .join("\n"),
        )
        .expect("failed to write helper module");
        let main_path = temp.path().join("main.au");
        fs::write(
            &main_path,
            [
                "from helpers.counter import make_counter",
                "",
                "def main() -> int32:",
                "    counter = make_counter()",
                "    print(counter.value)",
                "    return 0",
                "",
            ]
            .join("\n"),
        )
        .expect("failed to write main module");

        let checked = check_path(&main_path)
            .expect("return type members from imported functions should stay visible");
        assert!(
            checked.functions.get("main").is_some(),
            "main should still type-check"
        );

        let output = run_path(&main_path).expect("module program should run");
        assert_eq!(output.stdout, "41\n");
        assert_eq!(output.value, zero_exit_value());

        let mir_output = run_path_via_mir(&main_path).expect("module program should run via MIR");
        assert_eq!(mir_output.stdout, "41\n");
        assert_eq!(mir_output.value, zero_exit_value());
    }

    #[test]
    fn backend_path_runs_channels_example_natively() {
        let source = include_str!("../../../examples/concurrency/channels_spawn.au");
        let output =
            run_source_via_mir(source).expect("channels example should run through backend path");
        assert_eq!(output.stdout, "2\n4\n");
        assert_eq!(output.value, zero_exit_value());
    }

    #[test]
    fn mir_lowering_creates_blocks_for_control_flow() {
        let mir = lower_source_to_mir(CONTROL_FLOW_SOURCE).expect("control flow MIR should lower");
        let script = mir
            .top_level
            .expect("top-level script MIR should be present for control flow example");

        assert!(script.blocks.len() >= 4);
        assert!(script
            .blocks
            .iter()
            .any(|block| block.label.contains("while_cond")));
        assert!(script
            .blocks
            .iter()
            .any(|block| block.label.contains("if_then")));
    }

    #[test]
    fn categorized_examples_type_check() {
        for (path, source) in EXAMPLE_CASES {
            check_source(source).unwrap_or_else(|error| {
                panic!("{} should type-check: {}", path, error);
            });
        }
    }

    #[test]
    fn categorized_examples_run_with_expected_output() {
        let cases = [
            (
                "examples/basics/top_level_script.au",
                EXAMPLE_CASES[0].1,
                "156\n",
            ),
            (
                "examples/basics/main_function.au",
                EXAMPLE_CASES[1].1,
                "16\n",
            ),
            (
                "examples/basics/mutable_bindings.au",
                EXAMPLE_CASES[2].1,
                "5\n",
            ),
            (
                "examples/basics/default_arguments.au",
                EXAMPLE_CASES[3].1,
                "hello world\nhello aurora\n6\n12\n",
            ),
            ("examples/basics/pass_keyword.au", EXAMPLE_CASES[4].1, "0\n"),
            (
                "examples/classes/point_distance.au",
                EXAMPLE_CASES[5].1,
                "5.0\n",
            ),
            (
                "examples/classes/default_fields.au",
                EXAMPLE_CASES[6].1,
                "localhost\n8080\n",
            ),
            (
                "examples/classes/methods.au",
                EXAMPLE_CASES[7].1,
                "4\n8\n0\n",
            ),
            (
                "examples/control_flow/if_elif_else.au",
                EXAMPLE_CASES[8].1,
                "high\n",
            ),
            (
                "examples/control_flow/for_range.au",
                EXAMPLE_CASES[9].1,
                "7\n",
            ),
            (
                "examples/control_flow/while_break_continue.au",
                EXAMPLE_CASES[10].1,
                "ok\n",
            ),
            (
                "examples/enums/result_match.au",
                EXAMPLE_CASES[11].1,
                "42\nbad\n0\n",
            ),
            (
                "examples/enums/result_option.au",
                EXAMPLE_CASES[12].1,
                "4\ndivision by zero\n7\n",
            ),
            (
                "examples/enums/explicit_type_args.au",
                EXAMPLE_CASES[13].1,
                "7\nbad\n",
            ),
            (
                "examples/generics/box_and_wrapper.au",
                EXAMPLE_CASES[14].1,
                "7\nok\n",
            ),
            (
                "examples/traits/greeter.au",
                EXAMPLE_CASES[15].1,
                "hello aurora\nhello aurora\n",
            ),
            (
                "examples/traits/multiple_bounds.au",
                EXAMPLE_CASES[16].1,
                "9\n",
            ),
            (
                "examples/numbers/float_sqrt.au",
                EXAMPLE_CASES[17].1,
                "9.0\n",
            ),
            (
                "examples/numbers/float32_values.au",
                EXAMPLE_CASES[18].1,
                "3.25\n2.0\n5.0\n",
            ),
            (
                "examples/numbers/numeric_casts.au",
                EXAMPLE_CASES[19].1,
                "7\n3.0\n1.25\n2.0\n",
            ),
            (
                "examples/strings/greeting.au",
                EXAMPLE_CASES[20].1,
                "hello, aurora\n",
            ),
            (
                "examples/concurrency/task_group_select.au",
                EXAMPLE_CASES[21].1,
                "3\n",
            ),
            (
                "examples/concurrency/task_group_cancel.au",
                EXAMPLE_CASES[22].1,
                "0\n1\n",
            ),
            (
                "examples/concurrency/select_timeout.au",
                EXAMPLE_CASES[23].1,
                "timeout\n",
            ),
            (
                "examples/concurrency/sleep_builtin.au",
                EXAMPLE_CASES[24].1,
                "start\nend\n",
            ),
            (
                "examples/concurrency/send_result.au",
                EXAMPLE_CASES[25].1,
                "7\n",
            ),
            (
                "examples/concurrency/spawn_detached.au",
                EXAMPLE_CASES[26].1,
                "9\n",
            ),
            (
                "examples/concurrency/select_send.au",
                EXAMPLE_CASES[27].1,
                "sent\n4\n",
            ),
            (
                "examples/enums/wildcard_match.au",
                EXAMPLE_CASES[28].1,
                "2\n",
            ),
            (
                "examples/generics/generic_method_calls.au",
                EXAMPLE_CASES[29].1,
                "7\n",
            ),
            (
                "examples/generics/bounded_types.au",
                EXAMPLE_CASES[30].1,
                "aurora\nempty\n",
            ),
            (
                "examples/traits/marker_trait.au",
                EXAMPLE_CASES[31].1,
                "1\n",
            ),
            (
                "examples/traits/specialized_generic_impl.au",
                EXAMPLE_CASES[32].1,
                "hello\n",
            ),
            (
                "examples/concurrency/minute_duration.au",
                EXAMPLE_CASES[33].1,
                "120000ms\n",
            ),
            (
                "examples/traits/generic_dispatch_multiple_types.au",
                EXAMPLE_CASES[34].1,
                "dog\ncat\n",
            ),
            (
                "examples/strings/string_methods.au",
                EXAMPLE_CASES[35].1,
                "15\ntrue\ntrue\ntrue\naurora repo\n2\naurora\nrepo\naurora lang\naurora repo\nAURORA REPO\nrepo\nnone\naurora\nnone\n11\n",
            ),
            (
                "examples/numbers/numeric_builtins.au",
                EXAMPLE_CASES[36].1,
                "7\n3.5\n2\n12\n9.0\n9.0\n",
            ),
            (
                "examples/collections/map_basics.au",
                EXAMPLE_CASES[37].1,
                "3\ntrue\n1\n1\n5\naurora\n3\n3\n3\n3\ntrue\n",
            ),
            (
                "examples/collections/set_basics.au",
                EXAMPLE_CASES[38].1,
                "3\ntrue\nfalse\ntrue\ntrue\n9\ntrue\ntrue\n1\n",
            ),
            (
                "examples/strings/string_parsing_and_formatting.au",
                EXAMPLE_CASES[39].1,
                "42\n-9000000000\n3.5\ntrue\naurora-lang-tests\ntrue\n12\n4\n9\n3.0\n",
            ),
            (
                "examples/traits/generic_trait_bounds.au",
                EXAMPLE_CASES[40].1,
                "20\n",
            ),
            (
                "examples/traits/operator_traits.au",
                EXAMPLE_CASES[41].1,
                "6\n8\n-6\n-8\n",
            ),
        ];

        for (path, source, expected_stdout) in cases {
            let output = run_source(source).unwrap_or_else(|error| {
                panic!("{} should run: {}", path, error);
            });
            assert_eq!(
                output.stdout, expected_stdout,
                "unexpected stdout for {}",
                path
            );
        }
    }

    #[test]
    fn categorized_examples_run_through_backend_path_with_expected_output() {
        let cases = [
            (
                "examples/basics/top_level_script.au",
                EXAMPLE_CASES[0].1,
                "156\n",
            ),
            (
                "examples/basics/main_function.au",
                EXAMPLE_CASES[1].1,
                "16\n",
            ),
            (
                "examples/basics/mutable_bindings.au",
                EXAMPLE_CASES[2].1,
                "5\n",
            ),
            (
                "examples/basics/default_arguments.au",
                EXAMPLE_CASES[3].1,
                "hello world\nhello aurora\n6\n12\n",
            ),
            ("examples/basics/pass_keyword.au", EXAMPLE_CASES[4].1, "0\n"),
            (
                "examples/classes/point_distance.au",
                EXAMPLE_CASES[5].1,
                "5.0\n",
            ),
            (
                "examples/classes/default_fields.au",
                EXAMPLE_CASES[6].1,
                "localhost\n8080\n",
            ),
            (
                "examples/classes/methods.au",
                EXAMPLE_CASES[7].1,
                "4\n8\n0\n",
            ),
            (
                "examples/control_flow/if_elif_else.au",
                EXAMPLE_CASES[8].1,
                "high\n",
            ),
            (
                "examples/control_flow/for_range.au",
                EXAMPLE_CASES[9].1,
                "7\n",
            ),
            (
                "examples/control_flow/while_break_continue.au",
                EXAMPLE_CASES[10].1,
                "ok\n",
            ),
            (
                "examples/enums/result_match.au",
                EXAMPLE_CASES[11].1,
                "42\nbad\n0\n",
            ),
            (
                "examples/enums/result_option.au",
                EXAMPLE_CASES[12].1,
                "4\ndivision by zero\n7\n",
            ),
            (
                "examples/enums/explicit_type_args.au",
                EXAMPLE_CASES[13].1,
                "7\nbad\n",
            ),
            (
                "examples/generics/box_and_wrapper.au",
                EXAMPLE_CASES[14].1,
                "7\nok\n",
            ),
            (
                "examples/traits/greeter.au",
                EXAMPLE_CASES[15].1,
                "hello aurora\nhello aurora\n",
            ),
            (
                "examples/traits/multiple_bounds.au",
                EXAMPLE_CASES[16].1,
                "9\n",
            ),
            (
                "examples/numbers/float_sqrt.au",
                EXAMPLE_CASES[17].1,
                "9.0\n",
            ),
            (
                "examples/numbers/float32_values.au",
                EXAMPLE_CASES[18].1,
                "3.25\n2.0\n5.0\n",
            ),
            (
                "examples/numbers/numeric_casts.au",
                EXAMPLE_CASES[19].1,
                "7\n3.0\n1.25\n2.0\n",
            ),
            (
                "examples/strings/greeting.au",
                EXAMPLE_CASES[20].1,
                "hello, aurora\n",
            ),
            (
                "examples/concurrency/task_group_select.au",
                EXAMPLE_CASES[21].1,
                "3\n",
            ),
            (
                "examples/concurrency/task_group_cancel.au",
                EXAMPLE_CASES[22].1,
                "0\n1\n",
            ),
            (
                "examples/concurrency/select_timeout.au",
                EXAMPLE_CASES[23].1,
                "timeout\n",
            ),
            (
                "examples/concurrency/sleep_builtin.au",
                EXAMPLE_CASES[24].1,
                "start\nend\n",
            ),
            (
                "examples/concurrency/send_result.au",
                EXAMPLE_CASES[25].1,
                "7\n",
            ),
            (
                "examples/concurrency/spawn_detached.au",
                EXAMPLE_CASES[26].1,
                "9\n",
            ),
            (
                "examples/concurrency/select_send.au",
                EXAMPLE_CASES[27].1,
                "sent\n4\n",
            ),
            (
                "examples/enums/wildcard_match.au",
                EXAMPLE_CASES[28].1,
                "2\n",
            ),
            (
                "examples/generics/generic_method_calls.au",
                EXAMPLE_CASES[29].1,
                "7\n",
            ),
            (
                "examples/generics/bounded_types.au",
                EXAMPLE_CASES[30].1,
                "aurora\nempty\n",
            ),
            (
                "examples/traits/marker_trait.au",
                EXAMPLE_CASES[31].1,
                "1\n",
            ),
            (
                "examples/traits/specialized_generic_impl.au",
                EXAMPLE_CASES[32].1,
                "hello\n",
            ),
            (
                "examples/concurrency/minute_duration.au",
                EXAMPLE_CASES[33].1,
                "120000ms\n",
            ),
            (
                "examples/traits/generic_dispatch_multiple_types.au",
                EXAMPLE_CASES[34].1,
                "dog\ncat\n",
            ),
            (
                "examples/strings/string_methods.au",
                EXAMPLE_CASES[35].1,
                "15\ntrue\ntrue\ntrue\naurora repo\n2\naurora\nrepo\naurora lang\naurora repo\nAURORA REPO\nrepo\nnone\naurora\nnone\n11\n",
            ),
            (
                "examples/numbers/numeric_builtins.au",
                EXAMPLE_CASES[36].1,
                "7\n3.5\n2\n12\n9.0\n9.0\n",
            ),
            (
                "examples/collections/map_basics.au",
                EXAMPLE_CASES[37].1,
                "3\ntrue\n1\n1\n5\naurora\n3\n3\n3\n3\ntrue\n",
            ),
            (
                "examples/collections/set_basics.au",
                EXAMPLE_CASES[38].1,
                "3\ntrue\nfalse\ntrue\ntrue\n9\ntrue\ntrue\n1\n",
            ),
            (
                "examples/strings/string_parsing_and_formatting.au",
                EXAMPLE_CASES[39].1,
                "42\n-9000000000\n3.5\ntrue\naurora-lang-tests\ntrue\n12\n4\n9\n3.0\n",
            ),
            (
                "examples/traits/generic_trait_bounds.au",
                EXAMPLE_CASES[40].1,
                "20\n",
            ),
            (
                "examples/traits/operator_traits.au",
                EXAMPLE_CASES[41].1,
                "6\n8\n-6\n-8\n",
            ),
        ];

        for (path, source, expected_stdout) in cases {
            let output = run_source_via_mir(source).unwrap_or_else(|error| {
                panic!("{} should run through backend path: {}", path, error);
            });
            assert_eq!(
                output.stdout, expected_stdout,
                "unexpected backend-path stdout for {}",
                path
            );
        }
    }

    #[test]
    fn module_example_runs_with_expected_output() {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../examples/modules/simple_import.au");
        let output = run_path(&path).expect("module example should run");
        assert_eq!(output.stdout, "10\n2\n");
        assert_eq!(output.value, zero_exit_value());
    }

    #[test]
    fn module_example_runs_through_backend_path_with_expected_output() {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../examples/modules/simple_import.au");
        let output = run_path_via_mir(&path).expect("module example should run via MIR");
        assert_eq!(output.stdout, "10\n2\n");
        assert_eq!(output.value, zero_exit_value());
    }
}
