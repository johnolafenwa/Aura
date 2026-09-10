//! Module namespaces, visible declarations, and canonical nominal lookup.

use super::{
    is_builtin_type, BTreeMap, ClassInfo, Diagnostic, EnumInfo, Expr, ExprKind, ExternFunctionInfo,
    FunctionChecker, FunctionInfo, HashMap, LocalBinding, ModuleNamespace, OpaqueHandleInfo,
    ReceiverKind, Result, Type,
};

pub(super) fn reject_reserved_type_name(name: &str, span: crate::diag::Span) -> Result<()> {
    if is_builtin_type(name) {
        return Err(Diagnostic::at(
            span,
            format!("`{}` is a reserved built-in type name", name),
        ));
    }
    Ok(())
}

pub(super) fn find_namespace_in_modules<'a>(
    modules: &'a BTreeMap<String, ModuleNamespace>,
    path: &str,
) -> Option<&'a ModuleNamespace> {
    for namespace in modules.values() {
        if namespace.path == path {
            return Some(namespace);
        }
        if let Some(found) = find_namespace_in_modules(&namespace.modules, path) {
            return Some(found);
        }
        if let Some(found) = find_namespace_in_modules(&namespace.imported_modules, path) {
            return Some(found);
        }
    }
    None
}

pub(super) fn validate_type_params(
    type_params: &[String],
    span: crate::diag::Span,
    owner: &str,
) -> Result<()> {
    let mut seen = BTreeMap::new();
    for name in type_params {
        if name == "Self" {
            return Err(Diagnostic::at(
                span,
                format!(
                    "`Self` is reserved and cannot be used as a type parameter on {}",
                    owner
                ),
            ));
        }
        if seen.insert(name.clone(), ()).is_some() {
            return Err(Diagnostic::at(
                span,
                format!("duplicate type parameter `{}` on {}", name, owner),
            ));
        }
    }
    Ok(())
}

impl<'a> FunctionChecker<'a> {
    pub(super) fn seed_imported_modules(&self, locals: &mut HashMap<String, LocalBinding>) {
        let imported_modules = self
            .current_module_namespace()
            .map(|namespace| &namespace.imported_modules)
            .unwrap_or(self.imported_modules);
        for (name, namespace) in imported_modules {
            locals.insert(
                name.clone(),
                LocalBinding {
                    ty: Type::Module(namespace.path.clone()),
                    assignable: false,
                    mutable_place: false,
                    managed_resource: false,
                    passing: ReceiverKind::Value,
                    borrow_origin: None,
                    borrowed_at: None,
                    match_borrow_place: None,
                    stale_match_borrow_place: None,
                    shared_match_scrutinee: None,
                    moved: false,
                    moved_at: None,
                    moved_fields: BTreeMap::new(),
                    frozen_places: BTreeMap::new(),
                    shared_match_places: BTreeMap::new(),
                    captured: false,
                    view: None,
                    closure_loans: Vec::new(),
                    narrowed: BTreeMap::new(),
                    stale_narrowing: BTreeMap::new(),
                },
            );
        }
    }

    pub(super) fn seed_module_constants(&self, locals: &mut HashMap<String, LocalBinding>) {
        let constants = self
            .current_module_namespace()
            .map(|namespace| &namespace.all_constants)
            .unwrap_or(self.constants);
        for (name, constant) in constants {
            locals.entry(name.clone()).or_insert_with(|| LocalBinding {
                ty: constant.ty.clone(),
                assignable: false,
                mutable_place: false,
                managed_resource: false,
                passing: ReceiverKind::Borrow,
                borrow_origin: Some(format!("module constant `{name}`")),
                borrowed_at: Some(constant.decl.span),
                match_borrow_place: None,
                stale_match_borrow_place: None,
                shared_match_scrutinee: None,
                moved: false,
                moved_at: None,
                moved_fields: BTreeMap::new(),
                frozen_places: BTreeMap::new(),
                shared_match_places: BTreeMap::new(),
                captured: false,
                view: None,
                closure_loans: Vec::new(),
                narrowed: BTreeMap::new(),
                stale_narrowing: BTreeMap::new(),
            });
        }
    }

    pub(super) fn seed_module_scope(&self, locals: &mut HashMap<String, LocalBinding>) {
        self.seed_imported_modules(locals);
        self.seed_module_constants(locals);
    }

    pub(super) fn module_enum_type_name(&self, module_path: &str, enum_info: &EnumInfo) -> String {
        format!("{}.{}", module_path, enum_info.decl.name)
    }

    pub(super) fn module_namespace(&self, path: &str) -> Option<&ModuleNamespace> {
        if let Some(namespace) = self.module_registry.get(path) {
            return Some(namespace);
        }
        self.current_module_namespace()
            .and_then(|current| find_namespace_in_modules(&current.imported_modules, path))
            .or_else(|| find_namespace_in_modules(self.imported_modules, path))
    }

    pub(super) fn current_module_namespace(&self) -> Option<&ModuleNamespace> {
        if self.module_name == self.root_module_name {
            None
        } else {
            self.module_registry.get(self.module_name)
        }
    }

    pub(super) fn infer_module_path(&self, expr: &Expr) -> Option<String> {
        match &expr.kind {
            ExprKind::Name(name) => self
                .current_module_namespace()
                .and_then(|namespace| namespace.imported_modules.get(name))
                .or_else(|| self.imported_modules.get(name))
                .map(|namespace| namespace.path.clone()),
            ExprKind::Specialize { expr, .. } => self.infer_module_path(expr),
            ExprKind::Member { object, field } => {
                let module_path = self.infer_module_path(object)?;
                let namespace = self.module_namespace(&module_path)?;
                namespace.modules.get(field).map(|child| child.path.clone())
            }
            ExprKind::Group(inner) => self.infer_module_path(inner),
            ExprKind::Index { object, .. } => self.infer_module_path(object),
            _ => None,
        }
    }

    pub(super) fn qualified_module_item(&self, expr: &Expr) -> Option<(String, String)> {
        match &expr.kind {
            ExprKind::Specialize { expr, .. } => self.qualified_module_item(expr),
            ExprKind::Member { object, field } => self
                .infer_module_path(object)
                .map(|path| (path, field.clone())),
            ExprKind::Group(inner) => self.qualified_module_item(inner),
            ExprKind::Index { object, .. } => self.qualified_module_item(object),
            _ => None,
        }
    }

    pub(super) fn find_class_in_modules<'b>(
        modules: &'b BTreeMap<String, ModuleNamespace>,
        name: &str,
        found: &mut Option<&'b ClassInfo>,
        ambiguous: &mut bool,
    ) {
        for namespace in modules.values() {
            if let Some(class_info) = namespace
                .classes
                .get(name)
                .or_else(|| namespace.all_classes.get(name))
            {
                if found.is_some() {
                    *ambiguous = true;
                } else {
                    *found = Some(class_info);
                }
            }
            Self::find_class_in_modules(&namespace.modules, name, found, ambiguous);
        }
    }

    pub(super) fn find_enum_in_modules<'b>(
        modules: &'b BTreeMap<String, ModuleNamespace>,
        name: &str,
        found: &mut Option<&'b EnumInfo>,
        ambiguous: &mut bool,
    ) {
        for namespace in modules.values() {
            if let Some(enum_info) = namespace
                .enums
                .get(name)
                .or_else(|| namespace.all_enums.get(name))
            {
                if found.is_some() {
                    *ambiguous = true;
                } else {
                    *found = Some(enum_info);
                }
            }
            Self::find_enum_in_modules(&namespace.modules, name, found, ambiguous);
        }
    }

    pub(super) fn imported_class_info(&self, name: &str) -> Option<&ClassInfo> {
        let modules = self
            .current_module_namespace()
            .map(|namespace| &namespace.imported_modules)
            .unwrap_or(self.imported_modules);
        let mut found = None;
        let mut ambiguous = false;
        Self::find_class_in_modules(modules, name, &mut found, &mut ambiguous);
        if ambiguous {
            None
        } else {
            found
        }
    }

    pub(super) fn imported_enum_info(&self, name: &str) -> Option<&EnumInfo> {
        let modules = self
            .current_module_namespace()
            .map(|namespace| &namespace.imported_modules)
            .unwrap_or(self.imported_modules);
        let mut found = None;
        let mut ambiguous = false;
        Self::find_enum_in_modules(modules, name, &mut found, &mut ambiguous);
        if ambiguous {
            None
        } else {
            found
        }
    }

    pub(super) fn resolve_function_info(&self, name: &str) -> Option<&FunctionInfo> {
        self.current_module_namespace()
            .and_then(|namespace| namespace.all_functions.get(name))
            .or_else(|| self.functions.get(name))
    }

    pub(super) fn resolve_extern_function_info(&self, name: &str) -> Option<&ExternFunctionInfo> {
        self.current_module_namespace()
            .and_then(|namespace| namespace.all_extern_functions.get(name))
            .or_else(|| self.extern_functions.get(name))
    }

    pub(super) fn resolve_opaque_handle_info(&self, name: &str) -> Option<&OpaqueHandleInfo> {
        if let Some((module_path, item_name)) = name.rsplit_once('.') {
            if let Some(namespace) = self.module_namespace(module_path) {
                if let Some(handle) = namespace.opaque_handles.get(item_name) {
                    return Some(handle);
                }
            }
        }
        self.opaque_handles.get(name)
    }

    pub(super) fn is_opaque_handle_type(&self, ty: &Type) -> bool {
        let Type::Named(name, args) = ty else {
            return false;
        };
        args.is_empty() && self.resolve_opaque_handle_info(name).is_some()
    }

    pub(super) fn resolve_class_info(&self, name: &str) -> Option<&ClassInfo> {
        if let Some((module_path, item_name)) = name.rsplit_once('.') {
            if let Some(namespace) = self.module_namespace(module_path) {
                if let Some(class_info) = namespace
                    .classes
                    .get(item_name)
                    .or_else(|| namespace.all_classes.get(item_name))
                {
                    return Some(class_info);
                }
            }
        }
        self.current_module_namespace()
            .and_then(|namespace| namespace.all_classes.get(name))
            .or_else(|| self.classes.get(name))
            .or_else(|| self.imported_class_info(name))
    }

    pub(super) fn resolve_enum_info(&self, name: &str) -> Option<&EnumInfo> {
        if let Some((module_path, item_name)) = name.rsplit_once('.') {
            if let Some(namespace) = self.module_namespace(module_path) {
                if let Some(enum_info) = namespace
                    .enums
                    .get(item_name)
                    .or_else(|| namespace.all_enums.get(item_name))
                {
                    return Some(enum_info);
                }
            }
        }
        self.current_module_namespace()
            .and_then(|namespace| namespace.all_enums.get(name))
            .or_else(|| self.enums.get(name))
            .or_else(|| self.imported_enum_info(name))
    }

    pub(super) fn canonical_nominal_type_name(
        &self,
        surface_name: &str,
        owner_module: &str,
        declared_name: &str,
    ) -> String {
        self.canonical_type_names
            .get(surface_name)
            .cloned()
            .unwrap_or_else(|| {
                if !surface_name.contains('.') {
                    // Unqualified names without an explicit imported-binding
                    // mapping are local lexical names. Production import
                    // contexts always provide that mapping; this fallback
                    // also keeps direct checker construction honest.
                    surface_name.to_string()
                } else if owner_module == self.module_name {
                    declared_name.to_string()
                } else {
                    format!("{}.{}", owner_module, declared_name)
                }
            })
    }

    pub(super) fn canonical_class_name(
        &self,
        surface_name: &str,
        class_info: &ClassInfo,
    ) -> String {
        self.canonical_nominal_type_name(
            surface_name,
            &class_info.module_name,
            &class_info.decl.name,
        )
    }

    pub(super) fn canonical_enum_info_name(
        &self,
        surface_name: &str,
        enum_info: &EnumInfo,
    ) -> String {
        self.canonical_nominal_type_name(surface_name, &enum_info.module_name, &enum_info.decl.name)
    }

    pub(super) fn canonical_enum_name(&self, name: &str) -> String {
        if let Some(enum_info) = self.resolve_enum_info(name) {
            return self.canonical_enum_info_name(name, enum_info);
        }
        name.rsplit_once('.')
            .map(|(_, leaf)| leaf.to_string())
            .unwrap_or_else(|| name.to_string())
    }

    pub(super) fn is_external_module(&self, owner_module: &str) -> bool {
        owner_module != self.module_name
    }
}
