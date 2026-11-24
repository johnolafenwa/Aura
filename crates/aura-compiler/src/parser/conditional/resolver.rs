//! Only resolve the imports an active guard references. All other imports
//! stay with the normal loader until selection has finished.
use super::*;
use std::path::Path;

pub(in crate::parser) struct Resolver<'a> {
    path: Option<(&'a mut crate::ModuleLoader, &'a Path)>,
    active: Vec<Span>,
}

impl<'a> Resolver<'a> {
    pub(in crate::parser) fn new(path: Option<(&'a mut crate::ModuleLoader, &'a Path)>) -> Self {
        Self {
            path,
            active: Vec::new(),
        }
    }

    pub(super) fn check_dependencies(
        &mut self,
        probe: &mut Module,
        tokens: &[Token],
        span: Span,
    ) -> Result<()> {
        if probe.items.is_empty() {
            return Ok(());
        }
        if self.active.contains(&span) {
            return Err(Diagnostic::coded_at(
                "AU2071",
                span,
                "cyclic compile-time condition",
            ));
        }
        if self.active.len() >= RECURSION_LIMIT {
            return Err(Diagnostic::coded_at(
                "AU2071",
                span,
                "compile-time condition nesting exceeds the supported recursion limit",
            ));
        }
        self.active.push(span);
        let result = statements::select(self, probe, tokens, &mut Vec::new());
        self.active.pop();
        result
    }

    pub(super) fn potential(&self, import: &ImportDecl) -> bool {
        let path = match &import.kind {
            ImportKind::Module { path, .. } => path,
            ImportKind::From { module_path, .. } => module_path,
        };
        predicates::build_import(import)
            || (self.path.is_some()
                && path
                    .first()
                    .is_some_and(|part| !crate::builtin_modules::is_builtin_root_module(part)))
    }

    pub(super) fn marked(&mut self, import: &ImportDecl, reference: &[&str]) -> Result<bool> {
        let [root, tail @ ..] = reference else {
            return Ok(false);
        };
        if !predicates::imports_name(import, root) {
            return Ok(false);
        }
        if predicates::build_import(import) {
            return Ok(true);
        }
        let Some((loader, file)) = &mut self.path else {
            return Ok(false);
        };
        let (path, name) = match &import.kind {
            ImportKind::From { module_path, names } => {
                let Some(imported) = names
                    .iter()
                    .find(|item| item.alias.as_ref().unwrap_or(&item.name) == root)
                else {
                    return Ok(false);
                };
                (module_path, imported.name.as_str())
            }
            ImportKind::Module { path, alias } => {
                let skip = if alias.is_some() {
                    0
                } else {
                    path.len().saturating_sub(1)
                };
                let Some(name) = tail.get(skip) else {
                    return Ok(false);
                };
                (path, *name)
            }
        };
        if path
            .first()
            .is_some_and(|part| crate::builtin_modules::is_builtin_root_module(part))
        {
            return Ok(false);
        }
        let dependency = loader.load_imported_module(file, path, import.span)?;
        Ok(dependency
            .constants
            .get(name)
            .is_some_and(|entry| entry.decl.public && entry.decl.required))
    }

    pub(super) fn compile(&mut self, probe: Module) -> Result<crate::sema::Program> {
        let Some((loader, path)) = &mut self.path else {
            return crate::check_module_with_builtin_imports(probe);
        };
        let imported_bindings = loader.resolve_imports(&probe, path)?;
        let context = crate::sema::ModuleContext {
            module_name: loader.module_name_for_path(path),
            imported_bindings,
            module_registry: loader.build_module_registry(),
            is_entry_module: true,
        };
        let mut plan = Vec::new();
        for import in &probe.imports {
            let dependency = match &import.kind {
                ImportKind::Module { path, .. } => path,
                ImportKind::From { module_path, .. } => module_path,
            };
            if !dependency
                .first()
                .is_some_and(|part| crate::builtin_modules::is_builtin_root_module(part))
            {
                let imported = loader.resolve_import_path(path, dependency)?;
                plan.extend(loader.build_constant_init_plan(&imported)?);
            }
        }
        let mut program = crate::sema::check_with_context(probe, context)?;
        loader.qualify_program_imported_modules(path, &mut program);
        plan.append(&mut program.constant_init_plan);
        let mut seen = BTreeSet::new();
        plan.retain(|item| seen.insert((item.module_name.clone(), item.decl.name.clone())));
        program.constant_init_plan = plan;
        crate::const_eval::evaluate(&mut program)?;
        Ok(program)
    }
}
