//! Checked declarations, module inventory, and program checking.

use super::{
    collect_type_ref_type_params, lower_supertraits, lower_trait_bounds,
    lower_trait_bounds_with_self, lower_type, lower_type_with_self, merge_trait_bounds,
    merged_type_param_scope, recursive_field_message, reject_builtin_trait_method_collisions,
    reject_reserved_type_name, resolve_param_passings,
    rng_clone_obligation_params_in_context_with_modules, rng_clone_safety_in_context_with_modules,
    self_type_substitutions, substitute_trait_bounds, substitute_type, type_contains_named,
    type_is_copy_in_context_with_modules, type_param_scope,
    type_reaches_class_through_non_indirect_fields, validate_ffi_signature, validate_params,
    validate_type_params, view_return_contract_key, AssignStmt, AssignTarget, BTreeMap, BTreeSet,
    BuiltinClassConstructor, BuiltinFunction, ClassDecl, ClosureId, ClosureInfo, ComprehensionId,
    ComprehensionInfo, Diagnostic, EnumDecl, Expr, ExprKind, FunctionChecker, FunctionDecl,
    HashMap, ImplDecl, Item, Module, Rc, ReceiverKind, RefCell, Result, RngCloneSafety, Stmt,
    TraitBound, TraitDecl, Type, TypeDefinitions,
};

#[derive(Clone, Debug)]
pub struct Program {
    pub module: Module,
    pub module_name: String,
    pub source_path: Option<String>,
    pub aliases: BTreeMap<String, AliasInfo>,
    pub type_definitions: TypeDefinitions,
    pub classes: BTreeMap<String, ClassInfo>,
    pub enums: BTreeMap<String, EnumInfo>,
    pub functions: BTreeMap<String, FunctionInfo>,
    /// Module constants visible by their source-level local names. Imported
    /// aliases retain the defining module and storage identity in ConstantInfo.
    pub constants: BTreeMap<String, ConstantInfo>,
    /// Dependency-first, import-source-order plan for eager module constant
    /// initialization. The file loader replaces the local-only seed with the
    /// complete transitive plan for an entry program.
    pub constant_init_plan: Vec<ConstantInfo>,
    pub extern_functions: BTreeMap<String, ExternFunctionInfo>,
    pub opaque_handles: BTreeMap<String, OpaqueHandleInfo>,
    pub traits: BTreeMap<String, TraitInfo>,
    pub trait_impls: Vec<TraitImplInfo>,
    pub imported_modules: BTreeMap<String, ModuleNamespace>,
    pub module_registry: BTreeMap<String, ModuleNamespace>,
    /// Canonical nominal identities for names visible in the checked module.
    /// Imported aliases map to their defining module, while local names map
    /// to themselves. Tooling uses this to mirror checker type identity.
    pub canonical_type_names: BTreeMap<String, String>,
    /// Closure-conversion metadata for lambdas defined by this module.
    ///
    /// The key carries the defining module and callable owner so identical
    /// source positions in different files or bodies cannot collide.
    pub closures: BTreeMap<ClosureId, ClosureInfo>,
    /// Checked result and clause-binding types for comprehensions defined by
    /// this module. MIR lowering consumes these rather than reimplementing
    /// progressively scoped iterable inference.
    pub comprehensions: BTreeMap<ComprehensionId, ComprehensionInfo>,
    pub top_level_stmts: Vec<Stmt>,
}

#[derive(Clone, Debug)]
pub struct AliasInfo {
    pub module_name: String,
    pub decl: crate::ast::TypeAliasDecl,
    pub target: Type,
    pub type_param_bounds: BTreeMap<String, Vec<TraitBound>>,
}

impl AliasInfo {
    /// A constructor alias reuses the expanded nominal constructor. No scalar
    /// or union constructor is synthesized by this adapter.
    pub(crate) fn constructor_callee(
        &self,
        type_args: Option<&[Type]>,
        span: crate::diag::Span,
        module_name: &str,
        canonical_names: &BTreeMap<String, String>,
        budget: &super::type_budget::ExpansionBudget,
    ) -> Result<Option<Expr>> {
        let target = if let Some(args) = type_args {
            if args.len() != self.decl.type_params.len() {
                return Err(Diagnostic::coded_at(
                    "AU2002",
                    span,
                    format!(
                        "type alias `{}` expects {} type arguments, found {}",
                        self.decl.name,
                        self.decl.type_params.len(),
                        args.len()
                    ),
                ));
            }
            let substitutions =
                super::substitutions_from_decl_type_args(&self.decl.type_params, args);
            budget.check_substitution(&self.target, &substitutions, span)?;
            budget.check_substitution_key(
                &self.target,
                &substitutions,
                module_name,
                canonical_names,
                span,
            )?;
            super::substitute_alias_type(&self.target, &substitutions, module_name, canonical_names)
        } else {
            budget.check_substitution(&self.target, &HashMap::new(), span)?;
            self.target.clone()
        };
        let Type::Named(name, args) = target else {
            return Ok(None);
        };
        let base = Expr {
            kind: ExprKind::Name(name),
            span,
        };
        if args.is_empty()
            || (type_args.is_none() && args.iter().any(super::has_unresolved_type_params))
        {
            return Ok(Some(base));
        }
        let type_args = args
            .iter()
            .map(|arg| arg.source_type_ref(span))
            .collect::<Result<Vec<_>>>()?;
        Ok(Some(Expr {
            kind: ExprKind::Specialize {
                expr: Box::new(base),
                type_args,
            },
            span,
        }))
    }
}

#[derive(Clone, Debug)]
pub struct ConstantInfo {
    pub module_name: String,
    pub decl: crate::ast::ConstantDecl,
    pub ty: Type,
}

impl Program {
    pub(crate) fn aliases_in_scope(&self) -> impl Iterator<Item = (&String, &AliasInfo)> {
        self.type_definitions
            .imported_aliases
            .iter()
            .chain(self.aliases.iter())
    }

    pub(crate) fn alias_info(&self, name: &str, module_name: &str) -> Option<&AliasInfo> {
        if module_name == self.module_name {
            self.aliases
                .get(name)
                .or_else(|| self.type_definitions.imported_aliases.get(name))
        } else {
            self.module_registry
                .get(module_name)
                .and_then(|namespace| namespace.all_aliases.get(name))
        }
    }
    pub(crate) fn resolve_alias_type(
        &self,
        name: &str,
        args: &[Type],
        module_name: &str,
    ) -> Option<Type> {
        let alias = self.alias_info(name, module_name)?;
        if alias.decl.type_params.len() != args.len() {
            return None;
        }
        let substitutions = super::substitutions_from_decl_type_args(&alias.decl.type_params, args);
        Some(super::substitute_alias_type(
            &alias.target,
            &substitutions,
            module_name,
            &self.canonical_type_names,
        ))
    }
    pub fn closure_info(&self, id: &ClosureId) -> Option<&ClosureInfo> {
        self.closures.get(id)
    }
}

#[derive(Clone, Debug)]
pub struct ClassInfo {
    pub module_name: String,
    /// True only for classes synthesized by a builtin module namespace.
    /// A user module may have the same logical name without acquiring builtin behavior.
    pub is_builtin: bool,
    pub decl: ClassDecl,
    pub type_param_bounds: BTreeMap<String, Vec<TraitBound>>,
    pub fields: BTreeMap<String, FieldInfo>,
    pub methods: BTreeMap<String, MethodInfo>,
}

impl ClassInfo {
    pub(crate) fn builtin_constructor(&self) -> Option<BuiltinClassConstructor> {
        self.is_builtin
            .then(|| BuiltinClassConstructor::resolve(&self.module_name, &self.decl.name))
            .flatten()
    }
}

#[derive(Clone, Debug)]
pub struct FieldInfo {
    pub public: bool,
    pub ty: Type,
    pub span: crate::diag::Span,
}

#[derive(Clone, Debug)]
pub struct EnumInfo {
    pub module_name: String,
    pub decl: EnumDecl,
    pub type_param_bounds: BTreeMap<String, Vec<TraitBound>>,
    pub variants: BTreeMap<String, EnumVariantInfo>,
}

#[derive(Clone, Debug)]
pub struct EnumVariantInfo {
    pub payloads: Vec<EnumPayloadFieldInfo>,
    pub named_payloads: bool,
    pub span: crate::diag::Span,
}

#[derive(Clone, Debug)]
pub struct EnumPayloadFieldInfo {
    pub name: Option<String>,
    pub ty: Type,
    pub span: crate::diag::Span,
}

#[derive(Clone, Debug)]
pub struct FunctionInfo {
    pub module_name: String,
    pub decl: FunctionDecl,
    pub signature: FunctionSignature,
    pub type_param_bounds: BTreeMap<String, Vec<TraitBound>>,
}

#[derive(Clone, Debug)]
pub struct ExternFunctionInfo {
    pub module_name: String,
    pub decl: crate::ast::ExternFunctionDecl,
    pub signature: FunctionSignature,
}

#[derive(Clone, Debug)]
pub struct OpaqueHandleInfo {
    pub module_name: String,
    pub decl: crate::ast::ExternOpaqueClassDecl,
}

#[derive(Clone, Debug)]
pub struct MethodInfo {
    pub decl: FunctionDecl,
    pub signature: FunctionSignature,
    pub type_param_bounds: BTreeMap<String, Vec<TraitBound>>,
}

#[derive(Clone, Debug)]
pub struct TraitInfo {
    pub module_name: String,
    pub decl: TraitDecl,
    pub supertraits: Vec<TraitBound>,
    pub methods: BTreeMap<String, TraitMethodInfo>,
}

#[derive(Clone, Debug)]
pub struct TraitMethodInfo {
    pub decl: FunctionDecl,
    pub signature: FunctionSignature,
    pub type_param_bounds: BTreeMap<String, Vec<TraitBound>>,
}

#[derive(Clone, Debug)]
pub struct TraitImplInfo {
    pub module_name: String,
    pub decl: ImplDecl,
    pub type_params: Vec<String>,
    pub type_param_bounds: BTreeMap<String, Vec<TraitBound>>,
    pub trait_name: String,
    pub trait_args: Vec<Type>,
    pub for_type: Type,
    pub methods: BTreeMap<String, TraitImplMethodInfo>,
}

#[derive(Clone, Debug)]
pub struct TraitImplMethodInfo {
    pub decl: FunctionDecl,
    pub signature: FunctionSignature,
    pub type_param_bounds: BTreeMap<String, Vec<TraitBound>>,
}

#[derive(Clone, Debug)]
pub enum ImportedBinding {
    Alias(AliasInfo),
    Function(FunctionInfo),
    ExternFunction(ExternFunctionInfo),
    OpaqueHandle(OpaqueHandleInfo),
    Class(ClassInfo),
    Enum(EnumInfo),
    Trait(TraitInfo),
    Constant(ConstantInfo),
    Module(ModuleNamespace),
}

#[derive(Clone, Debug)]
pub struct ModuleNamespace {
    pub all_aliases: BTreeMap<String, AliasInfo>,
    pub aliases: BTreeMap<String, AliasInfo>,
    pub name: String,
    pub path: String,
    pub source_path: Option<String>,
    pub modules: BTreeMap<String, ModuleNamespace>,
    pub functions: BTreeMap<String, FunctionInfo>,
    pub constants: BTreeMap<String, ConstantInfo>,
    pub extern_functions: BTreeMap<String, ExternFunctionInfo>,
    pub opaque_handles: BTreeMap<String, OpaqueHandleInfo>,
    pub classes: BTreeMap<String, ClassInfo>,
    pub enums: BTreeMap<String, EnumInfo>,
    pub traits: BTreeMap<String, TraitInfo>,
    pub trait_impls: Vec<TraitImplInfo>,
    pub all_functions: BTreeMap<String, FunctionInfo>,
    pub all_constants: BTreeMap<String, ConstantInfo>,
    pub all_extern_functions: BTreeMap<String, ExternFunctionInfo>,
    pub all_opaque_handles: BTreeMap<String, OpaqueHandleInfo>,
    pub all_classes: BTreeMap<String, ClassInfo>,
    pub all_enums: BTreeMap<String, EnumInfo>,
    pub all_traits: BTreeMap<String, TraitInfo>,
    pub imported_modules: BTreeMap<String, ModuleNamespace>,
    /// Closure metadata exported with this module's callable bodies.
    pub closures: BTreeMap<ClosureId, ClosureInfo>,
    /// Comprehension metadata exported with this module's callable bodies.
    pub comprehensions: BTreeMap<ComprehensionId, ComprehensionInfo>,
}

#[derive(Clone, Debug)]
pub struct ModuleContext {
    pub module_name: String,
    pub imported_bindings: BTreeMap<String, ImportedBinding>,
    pub module_registry: BTreeMap<String, ModuleNamespace>,
    pub is_entry_module: bool,
}

impl Default for ModuleContext {
    fn default() -> Self {
        Self {
            module_name: String::new(),
            imported_bindings: BTreeMap::new(),
            module_registry: BTreeMap::new(),
            is_entry_module: true,
        }
    }
}

#[derive(Clone, Debug)]
pub struct FunctionSignature {
    pub params: Vec<Type>,
    /// Parameter conventions resolved from the declaration before any
    /// generic substitution is applied.
    pub param_passings: Vec<ReceiverKind>,
    pub return_type: Type,
    /// Generic type parameters that must not resolve to a type containing
    /// non-cloneable `random.Rng` state. These obligations are inferred from
    /// clone-producing operations in the callable body and propagated through
    /// generic calls.
    pub rng_clone_safe_type_params: BTreeSet<String>,
    /// Generic type parameters whose concrete substitutions must support
    /// equality. These obligations are inferred from equality-bearing
    /// operations in the callable body and propagated through generic calls.
    pub array_equality_safe_type_params: BTreeSet<String>,
}

// This checker accepts already-authorized module context and therefore stays
// crate-private. Public callers must use the source wrappers (which reject
// unmanifested FFI) or path APIs (which enforce package opt-in and reports).
pub(crate) fn check_with_context(module: Module, context: ModuleContext) -> Result<Program> {
    let module_name = if context.module_name.is_empty() {
        "<main>".to_string()
    } else {
        context.module_name.clone()
    };
    let mut type_names = TypeDefinitions::default();
    type_names.module_name = module_name.clone();
    let mut type_arities = BTreeMap::<String, usize>::new();
    let mut canonical_type_names = BTreeMap::<String, String>::new();
    let mut item_names = BTreeMap::<String, (&'static str, crate::diag::Span)>::new();
    let mut imported_modules = BTreeMap::new();

    let mut aliases = BTreeMap::new();
    let mut imported_functions = BTreeMap::new();
    let mut constants = BTreeMap::new();
    let mut imported_extern_functions = BTreeMap::new();
    let mut imported_opaque_handles = BTreeMap::new();
    let mut imported_classes = BTreeMap::new();
    let mut imported_enums = BTreeMap::new();
    let mut imported_traits = BTreeMap::new();

    for (name, binding) in &context.imported_bindings {
        match binding {
            ImportedBinding::Alias(alias) => {
                item_names.insert(name.clone(), ("type alias", alias.decl.span));
                type_names.insert(name.clone(), alias.decl.span);
                type_arities.insert(name.clone(), alias.decl.type_params.len());
                type_names
                    .imported_aliases
                    .insert(name.clone(), alias.clone());
                aliases.insert(name.clone(), alias.clone());
                if let Some(namespace) = context.module_registry.get(&alias.module_name) {
                    register_module_namespace_types(namespace, &mut type_names, &mut type_arities);
                }
            }
            ImportedBinding::Function(function) => {
                item_names.insert(name.clone(), ("function", function.decl.span));
                if let Some(namespace) = context.module_registry.get(&function.module_name) {
                    register_module_namespace_types(namespace, &mut type_names, &mut type_arities);
                }
                imported_functions.insert(name.clone(), function.clone());
            }
            ImportedBinding::ExternFunction(function) => {
                item_names.insert(name.clone(), ("extern function", function.decl.name_span));
                if let Some(namespace) = context.module_registry.get(&function.module_name) {
                    register_module_namespace_types(namespace, &mut type_names, &mut type_arities);
                }
                imported_extern_functions.insert(name.clone(), function.clone());
            }
            ImportedBinding::OpaqueHandle(handle) => {
                canonical_type_names.insert(
                    name.clone(),
                    format!("{}.{}", handle.module_name, handle.decl.name),
                );
                type_names.insert(name.clone(), handle.decl.span);
                type_arities.insert(name.clone(), 0);
                item_names.insert(name.clone(), ("opaque class", handle.decl.name_span));
                if let Some(namespace) = context.module_registry.get(&handle.module_name) {
                    register_module_namespace_types(namespace, &mut type_names, &mut type_arities);
                }
                imported_opaque_handles.insert(name.clone(), handle.clone());
            }
            ImportedBinding::Class(class_info) => {
                canonical_type_names.insert(
                    name.clone(),
                    format!("{}.{}", class_info.module_name, class_info.decl.name),
                );
                type_names.insert(name.clone(), class_info.decl.span);
                type_arities.insert(name.clone(), class_info.decl.type_params.len());
                item_names.insert(name.clone(), ("class", class_info.decl.span));
                if let Some(namespace) = context.module_registry.get(&class_info.module_name) {
                    register_module_namespace_types(namespace, &mut type_names, &mut type_arities);
                }
                imported_classes.insert(name.clone(), class_info.clone());
            }
            ImportedBinding::Enum(enum_info) => {
                canonical_type_names.insert(
                    name.clone(),
                    format!("{}.{}", enum_info.module_name, enum_info.decl.name),
                );
                type_names.insert(name.clone(), enum_info.decl.span);
                type_arities.insert(name.clone(), enum_info.decl.type_params.len());
                item_names.insert(name.clone(), ("enum", enum_info.decl.span));
                if let Some(namespace) = context.module_registry.get(&enum_info.module_name) {
                    register_module_namespace_types(namespace, &mut type_names, &mut type_arities);
                }
                imported_enums.insert(name.clone(), enum_info.clone());
            }
            ImportedBinding::Trait(trait_info) => {
                type_names.non_value_names.insert(name.clone());
                canonical_type_names.insert(
                    name.clone(),
                    format!("{}.{}", trait_info.module_name, trait_info.decl.name),
                );
                type_names.insert(name.clone(), trait_info.decl.span);
                type_arities.insert(name.clone(), trait_info.decl.type_params.len());
                item_names.insert(name.clone(), ("trait", trait_info.decl.span));
                if let Some(namespace) = context.module_registry.get(&trait_info.module_name) {
                    register_module_namespace_types(namespace, &mut type_names, &mut type_arities);
                }
                imported_traits.insert(name.clone(), trait_info.clone());
            }
            ImportedBinding::Constant(constant) => {
                item_names.insert(name.clone(), ("module constant", constant.decl.span));
                constants.insert(name.clone(), constant.clone());
            }
            ImportedBinding::Module(namespace) => {
                item_names.insert(name.clone(), ("module", crate::diag::Span::new(1, 1)));
                register_module_namespace_types(namespace, &mut type_names, &mut type_arities);
                register_visible_namespace_types(
                    namespace,
                    name,
                    &mut type_names,
                    &mut type_arities,
                    &mut canonical_type_names,
                );
                imported_modules.insert(name.clone(), namespace.clone());
            }
        }
    }

    for item in &module.items {
        match item {
            Item::TypeAlias(alias) => {
                reject_reserved_type_name(&alias.name, alias.span)?;
                validate_type_params(&alias.type_params, alias.span, "type alias")?;
                if let Some((kind, existing)) =
                    item_names.insert(alias.name.clone(), ("type alias", alias.span))
                {
                    return Err(Diagnostic::at(
                        alias.span,
                        format!(
                            "duplicate item `{}` (previously declared as {} at {})",
                            alias.name, kind, existing
                        ),
                    ));
                }
                type_names.insert(alias.name.clone(), alias.span);
                type_arities.insert(alias.name.clone(), alias.type_params.len());
                type_names.aliases.insert(alias.name.clone(), alias.clone());
            }
            Item::Class(class_decl) => {
                reject_reserved_type_name(&class_decl.name, class_decl.span)?;
                if let Some((kind, existing)) =
                    item_names.insert(class_decl.name.clone(), ("class", class_decl.span))
                {
                    return Err(Diagnostic::at(
                        class_decl.span,
                        format!(
                            "duplicate item `{}` (previously declared as {} at {})",
                            class_decl.name, kind, existing
                        ),
                    ));
                }
                type_names.insert(class_decl.name.clone(), class_decl.span);
                type_arities.insert(class_decl.name.clone(), class_decl.type_params.len());
                canonical_type_names.insert(class_decl.name.clone(), class_decl.name.clone());
            }
            Item::Enum(enum_decl) => {
                reject_reserved_type_name(&enum_decl.name, enum_decl.span)?;
                if let Some((kind, existing)) =
                    item_names.insert(enum_decl.name.clone(), ("enum", enum_decl.span))
                {
                    return Err(Diagnostic::at(
                        enum_decl.span,
                        format!(
                            "duplicate item `{}` (previously declared as {} at {})",
                            enum_decl.name, kind, existing
                        ),
                    ));
                }
                type_names.insert(enum_decl.name.clone(), enum_decl.span);
                type_arities.insert(enum_decl.name.clone(), enum_decl.type_params.len());
                canonical_type_names.insert(enum_decl.name.clone(), enum_decl.name.clone());
            }
            Item::ExternOpaqueClass(class_decl) => {
                reject_reserved_type_name(&class_decl.name, class_decl.span)?;
                if let Some((kind, existing)) =
                    item_names.insert(class_decl.name.clone(), ("opaque class", class_decl.span))
                {
                    return Err(Diagnostic::at(
                        class_decl.span,
                        format!(
                            "duplicate item `{}` (previously declared as {} at {})",
                            class_decl.name, kind, existing
                        ),
                    ));
                }
                type_names.insert(class_decl.name.clone(), class_decl.span);
                type_arities.insert(class_decl.name.clone(), 0);
                canonical_type_names.insert(class_decl.name.clone(), class_decl.name.clone());
            }
            Item::ExternFunction(function_decl) => {
                if BuiltinFunction::from_name(&function_decl.name).is_some() {
                    return Err(Diagnostic::coded_at(
                        "AU2007",
                        function_decl.span,
                        format!(
                            "`{}` is a builtin function name and cannot be redefined",
                            function_decl.name
                        ),
                    ));
                }
                if let Some((kind, existing)) = item_names.insert(
                    function_decl.name.clone(),
                    ("extern function", function_decl.span),
                ) {
                    return Err(Diagnostic::at(
                        function_decl.span,
                        format!(
                            "duplicate item `{}` (previously declared as {} at {})",
                            function_decl.name, kind, existing
                        ),
                    ));
                }
            }
            Item::Function(function_decl) => {
                if BuiltinFunction::from_name(&function_decl.name).is_some() {
                    return Err(Diagnostic::coded_at(
                        "AU2007",
                        function_decl.span,
                        format!(
                            "`{}` is a builtin function name and cannot be redefined",
                            function_decl.name
                        ),
                    ));
                }
                if let Some((kind, existing)) =
                    item_names.insert(function_decl.name.clone(), ("function", function_decl.span))
                {
                    return Err(Diagnostic::at(
                        function_decl.span,
                        format!(
                            "duplicate item `{}` (previously declared as {} at {})",
                            function_decl.name, kind, existing
                        ),
                    ));
                }
            }
            Item::Trait(trait_decl) => {
                type_names.non_value_names.insert(trait_decl.name.clone());
                reject_reserved_type_name(&trait_decl.name, trait_decl.span)?;
                if let Some((kind, existing)) =
                    item_names.insert(trait_decl.name.clone(), ("trait", trait_decl.span))
                {
                    return Err(Diagnostic::at(
                        trait_decl.span,
                        format!(
                            "duplicate item `{}` (previously declared as {} at {})",
                            trait_decl.name, kind, existing
                        ),
                    ));
                }
            }
            Item::Impl(_) => {}
        }
    }

    let alias_order = module
        .items
        .iter()
        .filter_map(|item| match item {
            Item::TypeAlias(alias) => Some(alias.name.clone()),
            _ => None,
        })
        .collect::<Vec<_>>();
    type_names.validate_alias_cycles(&alias_order)?;
    for name in &alias_order {
        let alias = &type_names.aliases[name];
        let target = lower_type(
            &alias.target,
            &type_names,
            &type_arities,
            &canonical_type_names,
            &type_param_scope(&alias.type_params),
        )?;
        aliases.insert(
            name.clone(),
            AliasInfo {
                module_name: module_name.clone(),
                decl: alias.clone(),
                target,
                type_param_bounds: BTreeMap::new(),
            },
        );
    }

    let mut traits = imported_traits.clone();
    for item in &module.items {
        let Item::Trait(trait_decl) = item else {
            continue;
        };
        validate_type_params(&trait_decl.type_params, trait_decl.span, "trait")?;
        let trait_type_param_scope = type_param_scope(&trait_decl.type_params);
        let self_placeholder = Type::TypeParam("Self".to_string());
        let supertraits = lower_supertraits(
            &trait_decl.supertraits,
            &traits,
            &type_names,
            &type_arities,
            &canonical_type_names,
            &trait_type_param_scope,
            Some(&self_placeholder),
        )?;
        let mut methods = BTreeMap::new();
        for method in &trait_decl.methods {
            validate_type_params(&method.type_params, method.span, "trait method")?;
            validate_params(
                method.receiver,
                &method.params,
                &format!("trait method `{}`", method.name),
            )?;
            let method_type_param_scope =
                merged_type_param_scope(&trait_type_param_scope, &method.type_params);
            let params = method
                .params
                .iter()
                .map(|param| {
                    lower_type_with_self(
                        &param.ty,
                        &type_names,
                        &type_arities,
                        &canonical_type_names,
                        &method_type_param_scope,
                        Some(&self_placeholder),
                    )
                })
                .collect::<Result<Vec<_>>>()?;
            let return_type = lower_type_with_self(
                &method.return_type,
                &type_names,
                &type_arities,
                &canonical_type_names,
                &method_type_param_scope,
                Some(&self_placeholder),
            )?;
            let type_param_bounds = lower_trait_bounds_with_self(
                &method.type_param_bounds,
                &traits,
                &type_names,
                &type_arities,
                &canonical_type_names,
                &method_type_param_scope,
                Some(&self_placeholder),
            )?;
            if methods
                .insert(
                    method.name.clone(),
                    TraitMethodInfo {
                        decl: method.clone(),
                        signature: FunctionSignature {
                            params,
                            param_passings: Vec::new(),
                            return_type,
                            rng_clone_safe_type_params: BTreeSet::new(),
                            array_equality_safe_type_params: BTreeSet::new(),
                        },
                        type_param_bounds,
                    },
                )
                .is_some()
            {
                return Err(Diagnostic::at(
                    method.span,
                    format!(
                        "duplicate method `{}` in trait `{}`",
                        method.name, trait_decl.name
                    ),
                ));
            }
        }
        traits.insert(
            trait_decl.name.clone(),
            TraitInfo {
                module_name: module_name.clone(),
                decl: trait_decl.clone(),
                supertraits,
                methods,
            },
        );
    }

    let mut enums = imported_enums.clone();
    for item in &module.items {
        let Item::Enum(enum_decl) = item else {
            continue;
        };
        validate_type_params(&enum_decl.type_params, enum_decl.span, "enum")?;
        let enum_type_param_scope = type_param_scope(&enum_decl.type_params);
        let type_param_bounds = lower_trait_bounds(
            &enum_decl.type_param_bounds,
            &traits,
            &type_names,
            &type_arities,
            &canonical_type_names,
            &enum_type_param_scope,
        )?;
        let mut variants = BTreeMap::new();
        for variant in &enum_decl.variants {
            let payloads = variant
                .payloads
                .iter()
                .map(|payload| {
                    Ok(EnumPayloadFieldInfo {
                        name: payload.name.clone(),
                        ty: lower_type(
                            &payload.ty,
                            &type_names,
                            &type_arities,
                            &canonical_type_names,
                            &enum_type_param_scope,
                        )?,
                        span: payload.span,
                    })
                })
                .collect::<Result<Vec<_>>>()?;
            if variants
                .insert(
                    variant.name.clone(),
                    EnumVariantInfo {
                        payloads,
                        named_payloads: variant.named_payloads,
                        span: variant.span,
                    },
                )
                .is_some()
            {
                return Err(Diagnostic::at(
                    variant.span,
                    format!(
                        "duplicate variant `{}` in enum `{}`",
                        variant.name, enum_decl.name
                    ),
                ));
            }
        }
        enums.insert(
            enum_decl.name.clone(),
            EnumInfo {
                module_name: module_name.clone(),
                decl: enum_decl.clone(),
                type_param_bounds,
                variants,
            },
        );
    }

    for alias in aliases
        .values_mut()
        .filter(|alias| alias.module_name == module_name)
    {
        alias.type_param_bounds = lower_trait_bounds(
            &alias.decl.type_param_bounds,
            &traits,
            &type_names,
            &type_arities,
            &canonical_type_names,
            &type_param_scope(&alias.decl.type_params),
        )?;
    }

    type_names.checked_aliases = aliases.clone();
    let mut classes = imported_classes.clone();
    for item in &module.items {
        let Item::Class(class_decl) = item else {
            continue;
        };
        validate_type_params(&class_decl.type_params, class_decl.span, "class")?;
        let class_type_param_scope = type_param_scope(&class_decl.type_params);
        let class_self_type = Type::Named(
            class_decl.name.clone(),
            class_decl
                .type_params
                .iter()
                .cloned()
                .map(Type::TypeParam)
                .collect(),
        );
        let type_param_bounds = lower_trait_bounds(
            &class_decl.type_param_bounds,
            &traits,
            &type_names,
            &type_arities,
            &canonical_type_names,
            &class_type_param_scope,
        )?;
        let mut fields = BTreeMap::new();
        let mut methods = BTreeMap::new();
        for field in &class_decl.fields {
            let lowered = lower_type(
                &field.ty,
                &type_names,
                &type_arities,
                &canonical_type_names,
                &class_type_param_scope,
            )?;
            if !field.ty.indirect && type_contains_named(&lowered, &class_decl.name) {
                return Err(Diagnostic::at(
                    field.span,
                    recursive_field_message(&class_decl.name, &field.name, &field.ty),
                ));
            }
            if fields
                .insert(
                    field.name.clone(),
                    FieldInfo {
                        public: field.public,
                        ty: lowered,
                        span: field.span,
                    },
                )
                .is_some()
            {
                return Err(Diagnostic::at(
                    field.span,
                    format!(
                        "duplicate field `{}` in class `{}`",
                        field.name, class_decl.name
                    ),
                ));
            }
        }

        for method in &class_decl.methods {
            validate_type_params(&method.type_params, method.span, "method")?;
            validate_params(
                method.receiver,
                &method.params,
                &format!("method `{}`", method.name),
            )?;
            let method_type_param_scope =
                merged_type_param_scope(&class_type_param_scope, &method.type_params);
            let type_param_bounds = merge_trait_bounds(
                &type_param_bounds,
                &lower_trait_bounds_with_self(
                    &method.type_param_bounds,
                    &traits,
                    &type_names,
                    &type_arities,
                    &canonical_type_names,
                    &method_type_param_scope,
                    Some(&class_self_type),
                )?,
            );
            let params = method
                .params
                .iter()
                .map(|param| {
                    lower_type_with_self(
                        &param.ty,
                        &type_names,
                        &type_arities,
                        &canonical_type_names,
                        &method_type_param_scope,
                        Some(&class_self_type),
                    )
                })
                .collect::<Result<Vec<_>>>()?;
            let return_type = lower_type_with_self(
                &method.return_type,
                &type_names,
                &type_arities,
                &canonical_type_names,
                &method_type_param_scope,
                Some(&class_self_type),
            )?;
            if methods
                .insert(
                    method.name.clone(),
                    MethodInfo {
                        decl: method.clone(),
                        signature: FunctionSignature {
                            params,
                            param_passings: Vec::new(),
                            return_type,
                            rng_clone_safe_type_params: BTreeSet::new(),
                            array_equality_safe_type_params: BTreeSet::new(),
                        },
                        type_param_bounds,
                    },
                )
                .is_some()
            {
                return Err(Diagnostic::at(
                    method.span,
                    format!(
                        "duplicate method `{}` in class `{}`",
                        method.name, class_decl.name
                    ),
                ));
            }
        }

        classes.insert(
            class_decl.name.clone(),
            ClassInfo {
                module_name: module_name.clone(),
                is_builtin: false,
                decl: class_decl.clone(),
                type_param_bounds,
                fields,
                methods,
            },
        );
    }

    for item in &module.items {
        let Item::Class(class_decl) = item else {
            continue;
        };
        // This pass walks the same declarations that populated `classes`
        // above, so absence here is a compiler invariant rather than a source
        // diagnostic.
        let class_info = &classes[&class_decl.name];
        for field_decl in &class_decl.fields {
            if field_decl.ty.indirect {
                continue;
            }
            let field_ty = &class_info.fields[&field_decl.name].ty;
            if type_reaches_class_through_non_indirect_fields(
                field_ty,
                &class_decl.name,
                &classes,
                &mut BTreeSet::new(),
            ) {
                return Err(Diagnostic::at(
                    field_decl.span,
                    recursive_field_message(&class_decl.name, &field_decl.name, &field_decl.ty),
                ));
            }
        }
    }

    for class in classes.values() {
        if !class.decl.copy {
            continue;
        }
        for field_decl in &class.decl.fields {
            let field_ty = &class.fields[&field_decl.name].ty;
            if !type_is_copy_in_context_with_modules(
                field_ty,
                &classes,
                &enums,
                &imported_modules,
                &context.module_registry,
            ) {
                return Err(Diagnostic::at(
                    field_decl.span,
                    format!(
                        "field `{}` on `copy class {}` must be a copy type, found `{}`",
                        field_decl.name, class.decl.name, field_ty
                    ),
                ));
            }
        }
    }

    // Class and enum copy-ness is now complete, so source-level default
    // parameter modes can be resolved without accidentally treating a user
    // `copy class` as a move type. Trait and class signatures were collected
    // earlier to support forward references; finalize their declaration ABI
    // here, before any generic substitution or body checking occurs.
    let mut trait_signature_updates = Vec::new();
    for item in &module.items {
        let Item::Trait(trait_decl) = item else {
            continue;
        };
        let trait_info = traits
            .get(&trait_decl.name)
            .expect("collected trait should remain available during signature finalization");
        for method in trait_info.methods.values() {
            let param_passings = resolve_param_passings(&method.decl.params);
            trait_signature_updates.push((
                trait_decl.name.clone(),
                method.decl.name.clone(),
                param_passings,
            ));
        }
    }
    for (trait_name, method_name, param_passings) in trait_signature_updates {
        let signature = &mut traits
            .get_mut(&trait_name)
            .expect("finalized trait should exist")
            .methods
            .get_mut(&method_name)
            .expect("finalized trait method should exist")
            .signature;
        signature.param_passings = param_passings;
    }

    let mut class_signature_updates = Vec::new();
    for item in &module.items {
        let Item::Class(class_decl) = item else {
            continue;
        };
        let class_info = classes
            .get(&class_decl.name)
            .expect("collected class should remain available during signature finalization");
        for method in class_info.methods.values() {
            let param_passings = resolve_param_passings(&method.decl.params);
            class_signature_updates.push((
                class_decl.name.clone(),
                method.decl.name.clone(),
                param_passings,
            ));
        }
    }
    for (class_name, method_name, param_passings) in class_signature_updates {
        let signature = &mut classes
            .get_mut(&class_name)
            .expect("finalized class should exist")
            .methods
            .get_mut(&method_name)
            .expect("finalized class method should exist")
            .signature;
        signature.param_passings = param_passings;
    }

    let empty_functions = BTreeMap::new();
    let empty_trait_impls = Vec::new();
    let default_checker = FunctionChecker::new(
        &module_name,
        &type_names,
        &type_arities,
        &canonical_type_names,
        &classes,
        &enums,
        &empty_functions,
        &constants,
        &traits,
        &empty_trait_impls,
        &imported_modules,
        &context.module_registry,
    );
    for class in classes.values() {
        let class_type_param_scope = type_param_scope(&class.decl.type_params);
        for field in &class.decl.fields {
            let Some(default) = &field.default else {
                continue;
            };
            // Field lowering and default checking consume the same collected
            // declaration, so missing metadata is not recoverable user input.
            let lowered = class.fields[&field.name].ty.clone();
            let default_ty = default_checker
                .with_type_params(class_type_param_scope.clone(), BTreeMap::new())
                .type_of_expr_hint(default, &mut HashMap::new(), Some(&lowered))
                .map_err(|diagnostic| {
                    if diagnostic.code == "AU2001"
                        && matches!(
                            &default.kind,
                            ExprKind::Call { callee, .. }
                                if matches!(callee.kind, ExprKind::Name(_))
                        )
                    {
                        // Class defaults are checked before this module's
                        // function bodies enter the callable registry. Keep
                        // the established boundary diagnostic instead of
                        // exposing the implementation-order "unknown name".
                        Diagnostic::at(default.span, "unsupported call target")
                    } else {
                        diagnostic
                    }
                })?;
            if default_ty != lowered {
                return Err(Diagnostic::at(
                    field.span,
                    format!(
                        "default value for field `{}` has type `{}`, expected `{}`",
                        field.name, default_ty, lowered
                    ),
                ));
            }
        }
    }
    let field_default_comprehensions = default_checker.comprehension_infos.borrow().clone();

    let mut functions = imported_functions.clone();
    for item in &module.items {
        let Item::Function(function_decl) = item else {
            continue;
        };
        validate_type_params(&function_decl.type_params, function_decl.span, "function")?;
        validate_params(
            function_decl.receiver,
            &function_decl.params,
            &format!("function `{}`", function_decl.name),
        )?;
        let function_type_param_scope = type_param_scope(&function_decl.type_params);
        let type_param_bounds = lower_trait_bounds(
            &function_decl.type_param_bounds,
            &traits,
            &type_names,
            &type_arities,
            &canonical_type_names,
            &type_param_scope(&function_decl.type_params),
        )?;
        let params = function_decl
            .params
            .iter()
            .map(|param| {
                lower_type(
                    &param.ty,
                    &type_names,
                    &type_arities,
                    &canonical_type_names,
                    &function_type_param_scope,
                )
            })
            .collect::<Result<Vec<_>>>()?;
        let return_type = lower_type(
            &function_decl.return_type,
            &type_names,
            &type_arities,
            &canonical_type_names,
            &function_type_param_scope,
        )?;
        let param_passings = resolve_param_passings(&function_decl.params);
        functions.insert(
            function_decl.name.clone(),
            FunctionInfo {
                module_name: module_name.clone(),
                decl: function_decl.clone(),
                signature: FunctionSignature {
                    params,
                    param_passings,
                    return_type,
                    rng_clone_safe_type_params: BTreeSet::new(),
                    array_equality_safe_type_params: BTreeSet::new(),
                },
                type_param_bounds,
            },
        );
    }

    let mut opaque_handles = imported_opaque_handles;
    for item in &module.items {
        let Item::ExternOpaqueClass(extern_decl) = item else {
            continue;
        };
        opaque_handles.insert(
            extern_decl.name.clone(),
            OpaqueHandleInfo {
                module_name: module_name.clone(),
                decl: extern_decl.clone(),
            },
        );
    }
    let mut ffi_signature_opaque_handles = opaque_handles.clone();
    for namespace in imported_modules.values() {
        register_public_namespace_opaque_handles(namespace, &mut ffi_signature_opaque_handles);
    }

    let mut extern_functions = imported_extern_functions;
    for item in &module.items {
        let Item::ExternFunction(extern_decl) = item else {
            continue;
        };
        validate_ffi_signature(
            extern_decl,
            &ffi_signature_opaque_handles,
            &type_names,
            &type_arities,
            &canonical_type_names,
        )?;
        let params = extern_decl
            .params
            .iter()
            .map(|param| {
                lower_type(
                    &param.ty,
                    &type_names,
                    &type_arities,
                    &canonical_type_names,
                    &BTreeMap::new(),
                )
            })
            .collect::<Result<Vec<_>>>()?;
        let return_type = lower_type(
            &extern_decl.return_type,
            &type_names,
            &type_arities,
            &canonical_type_names,
            &BTreeMap::new(),
        )?;
        extern_functions.insert(
            extern_decl.name.clone(),
            ExternFunctionInfo {
                module_name: module_name.clone(),
                decl: extern_decl.clone(),
                signature: FunctionSignature {
                    params,
                    param_passings: resolve_param_passings(&extern_decl.params),
                    return_type,
                    rng_clone_safe_type_params: BTreeSet::new(),
                    array_equality_safe_type_params: BTreeSet::new(),
                },
            },
        );
    }

    let mut trait_impls = Vec::new();
    for item in &module.items {
        let Item::Impl(impl_decl) = item else {
            continue;
        };
        validate_type_params(&impl_decl.type_params, impl_decl.span, "impl")?;
        let trait_info = traits.get(&impl_decl.trait_name).ok_or_else(|| {
            Diagnostic::at(
                impl_decl.span,
                format!("unknown trait `{}`", impl_decl.trait_name),
            )
        })?;
        let mut implicit_type_params = BTreeSet::new();
        collect_type_ref_type_params(
            &impl_decl.for_type,
            &type_names,
            &mut implicit_type_params,
            false,
        );
        for trait_arg in &impl_decl.trait_args {
            collect_type_ref_type_params(trait_arg, &type_names, &mut implicit_type_params, true);
        }
        let mut impl_type_params = impl_decl.type_params.clone();
        for type_param in implicit_type_params {
            if !impl_type_params.contains(&type_param) {
                impl_type_params.push(type_param);
            }
        }
        let impl_type_param_scope = type_param_scope(&impl_type_params);
        let impl_type_param_bounds = lower_trait_bounds(
            &impl_decl.type_param_bounds,
            &traits,
            &type_names,
            &type_arities,
            &canonical_type_names,
            &type_param_scope(&impl_decl.type_params),
        )?;
        if impl_decl.trait_args.len() != trait_info.decl.type_params.len() {
            return Err(Diagnostic::at(
                impl_decl.span,
                format!(
                    "trait `{}` expects exactly {} type argument{}, found {}",
                    impl_decl.trait_name,
                    trait_info.decl.type_params.len(),
                    if trait_info.decl.type_params.len() == 1 {
                        ""
                    } else {
                        "s"
                    },
                    impl_decl.trait_args.len()
                ),
            ));
        }
        let trait_args = impl_decl
            .trait_args
            .iter()
            .map(|arg| {
                lower_type(
                    arg,
                    &type_names,
                    &type_arities,
                    &canonical_type_names,
                    &impl_type_param_scope,
                )
            })
            .collect::<Result<Vec<_>>>()?;
        let for_type = lower_type(
            &impl_decl.for_type,
            &type_names,
            &type_arities,
            &canonical_type_names,
            &impl_type_param_scope,
        )?;
        if matches!(for_type, Type::TypeParam(_)) {
            return Err(Diagnostic::at(
                impl_decl.span,
                "trait impl target must name a concrete or generic outer type",
            ));
        }
        if trait_impls.iter().any(|existing: &TraitImplInfo| {
            existing.trait_name == impl_decl.trait_name
                && existing.trait_args == trait_args
                && existing.for_type == for_type
        }) {
            return Err(Diagnostic::at(
                impl_decl.span,
                format!(
                    "duplicate impl of trait `{}` for `{}`",
                    impl_decl.trait_name, for_type
                ),
            ));
        }

        let mut methods = BTreeMap::new();
        for method in &impl_decl.methods {
            let Some(trait_method) = trait_info.methods.get(&method.name) else {
                return Err(Diagnostic::at(
                    method.span,
                    format!(
                        "method `{}` is not part of trait `{}`",
                        method.name, impl_decl.trait_name
                    ),
                ));
            };
            if method.receiver != trait_method.decl.receiver {
                return Err(Diagnostic::at(
                    method.span,
                    format!(
                        "method `{}` receiver does not match trait `{}`",
                        method.name, impl_decl.trait_name
                    ),
                ));
            }
            validate_type_params(&method.type_params, method.span, "impl method")?;
            validate_params(
                method.receiver,
                &method.params,
                &format!("impl method `{}`", method.name),
            )?;
            let method_type_param_scope =
                merged_type_param_scope(&impl_type_param_scope, &method.type_params);
            let type_param_bounds = lower_trait_bounds_with_self(
                &method.type_param_bounds,
                &traits,
                &type_names,
                &type_arities,
                &canonical_type_names,
                &method_type_param_scope,
                Some(&for_type),
            )?;
            let params = method
                .params
                .iter()
                .map(|param| {
                    lower_type_with_self(
                        &param.ty,
                        &type_names,
                        &type_arities,
                        &canonical_type_names,
                        &method_type_param_scope,
                        Some(&for_type),
                    )
                })
                .collect::<Result<Vec<_>>>()?;
            let return_type = lower_type_with_self(
                &method.return_type,
                &type_names,
                &type_arities,
                &canonical_type_names,
                &method_type_param_scope,
                Some(&for_type),
            )?;
            let param_passings = resolve_param_passings(&method.params);
            let trait_substitutions =
                self_type_substitutions(&trait_info.decl, &trait_args, for_type.clone());
            let expected_params = trait_method
                .signature
                .params
                .iter()
                .map(|param| substitute_type(param, &trait_substitutions))
                .collect::<Vec<_>>();
            let expected_return_type =
                substitute_type(&trait_method.signature.return_type, &trait_substitutions);
            let params_have_matching_passing =
                param_passings == trait_method.signature.param_passings;
            if params != expected_params
                || !params_have_matching_passing
                || return_type != expected_return_type
                || view_return_contract_key(method) != view_return_contract_key(&trait_method.decl)
            {
                return Err(Diagnostic::at(
                    method.span,
                    format!(
                        "method `{}` in impl of `{}` does not match the trait signature",
                        method.name, impl_decl.trait_name
                    ),
                ));
            }
            methods.insert(
                method.name.clone(),
                TraitImplMethodInfo {
                    decl: method.clone(),
                    signature: FunctionSignature {
                        params,
                        param_passings,
                        return_type,
                        rng_clone_safe_type_params: BTreeSet::new(),
                        array_equality_safe_type_params: BTreeSet::new(),
                    },
                    type_param_bounds,
                },
            );
        }
        for (trait_method_name, trait_method) in &trait_info.methods {
            if methods.contains_key(trait_method_name) {
                continue;
            }
            if trait_method.decl.body.is_empty() {
                return Err(Diagnostic::at(
                    impl_decl.span,
                    format!(
                        "impl of `{}` for `{}` is missing method `{}`",
                        impl_decl.trait_name, for_type, trait_method_name
                    ),
                ));
            }
            let trait_substitutions =
                self_type_substitutions(&trait_info.decl, &trait_args, for_type.clone());
            methods.insert(
                trait_method_name.clone(),
                TraitImplMethodInfo {
                    decl: trait_method.decl.clone(),
                    signature: FunctionSignature {
                        params: trait_method
                            .signature
                            .params
                            .iter()
                            .map(|param| substitute_type(param, &trait_substitutions))
                            .collect(),
                        param_passings: trait_method.signature.param_passings.clone(),
                        return_type: substitute_type(
                            &trait_method.signature.return_type,
                            &trait_substitutions,
                        ),
                        rng_clone_safe_type_params: trait_method
                            .signature
                            .rng_clone_safe_type_params
                            .clone(),
                        array_equality_safe_type_params: trait_method
                            .signature
                            .array_equality_safe_type_params
                            .clone(),
                    },
                    type_param_bounds: substitute_trait_bounds(
                        &trait_method.type_param_bounds,
                        &trait_substitutions,
                    ),
                },
            );
        }
        reject_builtin_trait_method_collisions(impl_decl, &for_type, &methods)?;
        trait_impls.push(TraitImplInfo {
            module_name: module_name.clone(),
            decl: impl_decl.clone(),
            type_params: impl_type_params,
            type_param_bounds: impl_type_param_bounds,
            trait_name: impl_decl.trait_name.clone(),
            trait_args,
            for_type,
            methods,
        });
    }

    // Constants become visible strictly in declaration order. Items were
    // collected above, so initializers may call functions and constructors
    // regardless of textual position without exposing a later constant.
    let declared_constant_spans = module
        .constants
        .iter()
        .map(|constant| (constant.name.clone(), constant.span))
        .collect::<BTreeMap<_, _>>();
    let declared_top_level_local_spans = module
        .top_level_stmts
        .iter()
        .filter_map(|statement| match statement {
            Stmt::Assign(AssignStmt {
                mutable: true,
                target: AssignTarget::Name(name),
                span,
                ..
            }) => Some((name.clone(), *span)),
            _ => None,
        })
        .collect::<BTreeMap<_, _>>();
    let mut constant_closure_infos = BTreeMap::new();
    let mut constant_comprehension_infos = BTreeMap::new();
    for constant in &module.constants {
        if let Some((kind, existing)) =
            item_names.insert(constant.name.clone(), ("module constant", constant.span))
        {
            return Err(Diagnostic::coded_at(
                "AU2999",
                constant.span,
                format!(
                    "module constant `{}` collides with an existing {}",
                    constant.name, kind
                ),
            )
            .with_secondary(existing, format!("the existing {kind} is declared here")));
        }
        let expected = constant
            .annotation
            .as_ref()
            .map(|annotation| {
                lower_type(
                    annotation,
                    &type_names,
                    &type_arities,
                    &canonical_type_names,
                    &BTreeMap::new(),
                )
            })
            .transpose()?;
        let checker = FunctionChecker::new(
            &module_name,
            &type_names,
            &type_arities,
            &canonical_type_names,
            &classes,
            &enums,
            &functions,
            &constants,
            &traits,
            &trait_impls,
            &imported_modules,
            &context.module_registry,
        )
        .with_ffi(&extern_functions, &opaque_handles);
        let mut scope = HashMap::new();
        checker.seed_module_scope(&mut scope);
        let inferred = match checker.type_of_expr_hint(
            &constant.value,
            &mut scope,
            expected.as_ref(),
        ) {
            Ok(ty) => ty,
            Err(error) => {
                let blocked = declared_constant_spans.iter().find(|(name, span)| {
                    (**span == constant.span || span.line > constant.span.line)
                        && error.message.contains(&format!("unknown name `{name}`"))
                });
                if let Some((name, declaration)) = blocked {
                    return Err(Diagnostic::coded_at(
                        "AU2001",
                        constant.value.span,
                        format!("module constant `{name}` is used before initialization"),
                    )
                    .with_secondary(*declaration, format!("`{name}` is declared here")));
                }
                let script_local = declared_top_level_local_spans
                    .iter()
                    .find(|(name, _)| error.message.contains(&format!("unknown name `{name}`")));
                if let Some((name, declaration)) = script_local {
                    return Err(Diagnostic::coded_at(
                            "AU2001",
                            error.span.unwrap_or(constant.value.span),
                            format!(
                                "module constant `{}` cannot read top-level script local `{name}`",
                                constant.name
                            ),
                        )
                        .with_secondary(
                            *declaration,
                            format!(
                                "`{name}` is initialized when top-level entry statements run"
                            ),
                        )
                        .with_help(format!(
                            "declare `{}` with `mut` to make it a top-level script local, or move this work into `main`",
                            constant.name
                        )));
                }
                return Err(error);
            }
        };
        if let Some(expected) = &expected {
            if &inferred != expected {
                return Err(Diagnostic::coded_at(
                    "AU2002",
                    constant.value.span,
                    format!(
                        "initializer for module constant `{}` has type `{}`, expected `{}`",
                        constant.name, inferred, expected
                    ),
                ));
            }
        }
        checker.consume_value_expr(&constant.value, &mut scope)?;
        constant_closure_infos.extend(checker.closure_infos.borrow().clone());
        constant_comprehension_infos.extend(checker.comprehension_infos.borrow().clone());
        constants.insert(
            constant.name.clone(),
            ConstantInfo {
                module_name: module_name.clone(),
                decl: constant.clone(),
                ty: expected.unwrap_or(inferred),
            },
        );
    }

    let mut constant_init_plan = Vec::new();
    let mut planned_constants = BTreeSet::new();
    for constant in imported_modules
        .values()
        .flat_map(|namespace| namespace.all_constants.values())
        .chain(
            constants
                .values()
                .filter(|constant| constant.module_name != module_name),
        )
    {
        let key = (constant.module_name.clone(), constant.decl.name.clone());
        if planned_constants.insert(key) {
            constant_init_plan.push(constant.clone());
        }
    }
    let mut local_constants = constants
        .values()
        .filter(|constant| constant.module_name == module_name)
        .cloned()
        .collect::<Vec<_>>();
    local_constants.sort_by_key(|constant| (constant.decl.span.line, constant.decl.span.column));
    constant_init_plan.extend(local_constants);
    let mut program = Program {
        aliases,
        type_definitions: type_names.clone(),
        module: module.clone(),
        module_name,
        source_path: None,
        classes,
        enums,
        functions,
        constants,
        constant_init_plan,
        extern_functions,
        opaque_handles,
        traits,
        trait_impls,
        imported_modules,
        module_registry: context.module_registry,
        canonical_type_names: canonical_type_names.clone(),
        closures: BTreeMap::new(),
        comprehensions: field_default_comprehensions.clone(),
        top_level_stmts: module.top_level_stmts.clone(),
    };

    let local_main = program
        .functions
        .get("main")
        .filter(|function| function.module_name == program.module_name);
    if context.is_entry_module && local_main.is_some() {
        if let Some(Stmt::Assign(assign)) = program
            .top_level_stmts
            .iter()
            .find(|stmt| matches!(stmt, Stmt::Assign(assign) if assign.mutable))
        {
            return Err(Diagnostic::coded_at(
                "AU3003",
                assign.span,
                "module bindings are immutable; `mut` module state is not supported",
            )
            .with_help(
                "put mutable state in a local value owned by `main` or another explicit owner",
            ));
        }
    }
    if let (true, false, Some(main)) = (
        context.is_entry_module,
        program.top_level_stmts.is_empty(),
        local_main,
    ) {
        return Err(Diagnostic::at(
            main.decl.span,
            "files cannot mix top-level statements, including declarations, with an explicit `main` function",
        ));
    }

    if let (true, Some(main)) = (context.is_entry_module, local_main) {
        if !main.signature.params.is_empty() {
            return Err(Diagnostic::at(
                main.decl.span,
                "`main` must not take parameters in the bootstrap runtime",
            ));
        }
        if main.signature.return_type != Type::Unit
            && main.signature.return_type != Type::named("int32")
        {
            return Err(Diagnostic::at(
                main.decl.span,
                "`main` must return `int32` or `None` in the bootstrap runtime",
            ));
        }
    }

    // Clone safety is an inferred generic obligation, much like an implicit
    // effect. Check callable bodies to a fixed point so an obligation arising
    // in one generic callable propagates through generic-to-generic calls,
    // regardless of declaration order. The lattice is finite: each callable
    // can acquire only names from its declared type-parameter scope.
    loop {
        type CallableKey = (String, String);
        let (
            function_obligations,
            function_array_equality_obligations,
            class_method_obligations,
            class_method_array_equality_obligations,
            trait_method_obligations,
            trait_method_array_equality_obligations,
            impl_method_obligations,
            impl_method_array_equality_obligations,
            closure_infos,
            comprehension_infos,
        ) = {
            let checker = FunctionChecker::new(
                &program.module_name,
                &type_names,
                &type_arities,
                &canonical_type_names,
                &program.classes,
                &program.enums,
                &program.functions,
                &program.constants,
                &program.traits,
                &program.trait_impls,
                &program.imported_modules,
                &program.module_registry,
            )
            .with_ffi(&program.extern_functions, &program.opaque_handles);
            let closure_infos = checker.closure_infos.clone();
            let comprehension_infos = checker.comprehension_infos.clone();
            let mut function_obligations = BTreeMap::<String, BTreeSet<String>>::new();
            let mut function_array_equality_obligations =
                BTreeMap::<String, BTreeSet<String>>::new();
            let mut class_method_obligations = BTreeMap::<CallableKey, BTreeSet<String>>::new();
            let mut class_method_array_equality_obligations =
                BTreeMap::<CallableKey, BTreeSet<String>>::new();
            let mut trait_method_obligations = BTreeMap::<CallableKey, BTreeSet<String>>::new();
            let mut trait_method_array_equality_obligations =
                BTreeMap::<CallableKey, BTreeSet<String>>::new();
            let mut impl_method_obligations = BTreeMap::<(usize, String), BTreeSet<String>>::new();
            let mut impl_method_array_equality_obligations =
                BTreeMap::<(usize, String), BTreeSet<String>>::new();

            for (trait_name, trait_info) in &program.traits {
                let trait_type_param_scope = type_param_scope(&trait_info.decl.type_params);
                let self_placeholder = Type::TypeParam("Self".to_string());
                for (method_name, method) in &trait_info.methods {
                    let method_type_param_scope =
                        merged_type_param_scope(&trait_type_param_scope, &method.decl.type_params);
                    checker.check_param_defaults(
                        &method.decl.params,
                        &method_type_param_scope,
                        Some(&self_placeholder),
                        false,
                        "trait method",
                    )?;
                    let sink = Rc::new(RefCell::new(BTreeSet::new()));
                    let equality_sink = Rc::new(RefCell::new(BTreeSet::new()));
                    checker
                        .with_module_name(&trait_info.module_name)
                        .with_rng_clone_obligation_sink(sink.clone())
                        .with_array_equality_obligation_sink(equality_sink.clone())
                        .check_trait_method(trait_info, method)?;
                    trait_method_obligations.insert(
                        (trait_name.clone(), method_name.clone()),
                        sink.borrow().clone(),
                    );
                    trait_method_array_equality_obligations.insert(
                        (trait_name.clone(), method_name.clone()),
                        equality_sink.borrow().clone(),
                    );
                }
            }
            for (function_name, function) in &program.functions {
                if function.module_name != program.module_name {
                    continue;
                }
                let sink = Rc::new(RefCell::new(BTreeSet::new()));
                let equality_sink = Rc::new(RefCell::new(BTreeSet::new()));
                checker
                    .with_module_name(&function.module_name)
                    .with_rng_clone_obligation_sink(sink.clone())
                    .with_array_equality_obligation_sink(equality_sink.clone())
                    .check_function(function)?;
                function_obligations.insert(function_name.clone(), sink.borrow().clone());
                function_array_equality_obligations
                    .insert(function_name.clone(), equality_sink.borrow().clone());
            }

            for (class_name, class) in &program.classes {
                for (method_name, method) in &class.methods {
                    let sink = Rc::new(RefCell::new(BTreeSet::new()));
                    let equality_sink = Rc::new(RefCell::new(BTreeSet::new()));
                    checker
                        .with_module_name(&class.module_name)
                        .with_rng_clone_obligation_sink(sink.clone())
                        .with_array_equality_obligation_sink(equality_sink.clone())
                        .check_method(&class.decl, method)?;
                    class_method_obligations.insert(
                        (class_name.clone(), method_name.clone()),
                        sink.borrow().clone(),
                    );
                    class_method_array_equality_obligations.insert(
                        (class_name.clone(), method_name.clone()),
                        equality_sink.borrow().clone(),
                    );
                }
            }

            for (impl_index, trait_impl) in program.trait_impls.iter().enumerate() {
                checker
                    .with_module_name(&trait_impl.module_name)
                    .with_type_params(
                        type_param_scope(&trait_impl.type_params),
                        trait_impl.type_param_bounds.clone(),
                    )
                    .check_trait_impl_supertraits(trait_impl)?;
                let explicit_method_names = trait_impl
                    .decl
                    .methods
                    .iter()
                    .map(|method| method.name.as_str())
                    .collect::<BTreeSet<_>>();
                for (method_name, method) in &trait_impl.methods {
                    if !explicit_method_names.contains(method_name.as_str()) {
                        continue;
                    }
                    let sink = Rc::new(RefCell::new(BTreeSet::new()));
                    let equality_sink = Rc::new(RefCell::new(BTreeSet::new()));
                    checker
                        .with_module_name(&trait_impl.module_name)
                        .with_rng_clone_obligation_sink(sink.clone())
                        .with_array_equality_obligation_sink(equality_sink.clone())
                        .check_trait_impl_method(
                            &trait_impl.trait_name,
                            &trait_impl.for_type,
                            &trait_impl.type_params,
                            &trait_impl.type_param_bounds,
                            method,
                        )?;
                    impl_method_obligations
                        .insert((impl_index, method_name.clone()), sink.borrow().clone());
                    impl_method_array_equality_obligations.insert(
                        (impl_index, method_name.clone()),
                        equality_sink.borrow().clone(),
                    );
                }
            }

            let closure_infos = closure_infos.borrow().clone();
            let comprehension_infos = comprehension_infos.borrow().clone();
            (
                function_obligations,
                function_array_equality_obligations,
                class_method_obligations,
                class_method_array_equality_obligations,
                trait_method_obligations,
                trait_method_array_equality_obligations,
                impl_method_obligations,
                impl_method_array_equality_obligations,
                closure_infos,
                comprehension_infos,
            )
        };

        let mut changed = false;
        for (function_name, obligations) in function_obligations {
            let target = &mut program
                .functions
                .get_mut(&function_name)
                .expect("checked function should still exist")
                .signature
                .rng_clone_safe_type_params;
            let before = target.len();
            target.extend(obligations);
            changed |= target.len() != before;
        }
        for (function_name, obligations) in function_array_equality_obligations {
            let target = &mut program
                .functions
                .get_mut(&function_name)
                .expect("checked function should still exist")
                .signature
                .array_equality_safe_type_params;
            let before = target.len();
            target.extend(obligations);
            changed |= target.len() != before;
        }
        for ((class_name, method_name), obligations) in class_method_obligations {
            let target = &mut program
                .classes
                .get_mut(&class_name)
                .expect("checked class should still exist")
                .methods
                .get_mut(&method_name)
                .expect("checked class method should still exist")
                .signature
                .rng_clone_safe_type_params;
            let before = target.len();
            target.extend(obligations);
            changed |= target.len() != before;
        }
        for ((class_name, method_name), obligations) in class_method_array_equality_obligations {
            let target = &mut program
                .classes
                .get_mut(&class_name)
                .expect("checked class should still exist")
                .methods
                .get_mut(&method_name)
                .expect("checked class method should still exist")
                .signature
                .array_equality_safe_type_params;
            let before = target.len();
            target.extend(obligations);
            changed |= target.len() != before;
        }
        for ((trait_name, method_name), obligations) in trait_method_obligations {
            let target = &mut program
                .traits
                .get_mut(&trait_name)
                .expect("checked trait should still exist")
                .methods
                .get_mut(&method_name)
                .expect("checked trait method should still exist")
                .signature
                .rng_clone_safe_type_params;
            let before = target.len();
            target.extend(obligations);
            changed |= target.len() != before;
        }
        for ((trait_name, method_name), obligations) in trait_method_array_equality_obligations {
            let target = &mut program
                .traits
                .get_mut(&trait_name)
                .expect("checked trait should still exist")
                .methods
                .get_mut(&method_name)
                .expect("checked trait method should still exist")
                .signature
                .array_equality_safe_type_params;
            let before = target.len();
            target.extend(obligations);
            changed |= target.len() != before;
        }
        let body_impl_obligations = impl_method_obligations.clone();
        let body_impl_array_equality_obligations = impl_method_array_equality_obligations.clone();
        for ((impl_index, method_name), obligations) in impl_method_obligations {
            let target = &mut program.trait_impls[impl_index]
                .methods
                .get_mut(&method_name)
                .expect("checked impl method should still exist")
                .signature
                .rng_clone_safe_type_params;
            let before = target.len();
            target.extend(obligations);
            changed |= target.len() != before;
        }
        for ((impl_index, method_name), obligations) in impl_method_array_equality_obligations {
            let target = &mut program.trait_impls[impl_index]
                .methods
                .get_mut(&method_name)
                .expect("checked impl method should still exist")
                .signature
                .array_equality_safe_type_params;
            let before = target.len();
            target.extend(obligations);
            changed |= target.len() != before;
        }

        // A trait method's inferred requirements are part of its callable
        // contract. Map them through each impl header so direct concrete
        // dispatch observes the same contract as dispatch through a bound.
        let mut mapped_impl_contracts = BTreeMap::<(usize, String), BTreeSet<String>>::new();
        for (impl_index, trait_impl) in program.trait_impls.iter().enumerate() {
            // Trait impl collection has already resolved and validated this
            // nominal identity; the fixed-point pass cannot observe a missing
            // trait or method without an internal Program corruption.
            let trait_info = &program.traits[&trait_impl.trait_name];
            let substitutions = self_type_substitutions(
                &trait_info.decl,
                &trait_impl.trait_args,
                trait_impl.for_type.clone(),
            );
            for (method_name, impl_method) in &trait_impl.methods {
                let trait_method = &trait_info.methods[method_name];
                let mut mapped = BTreeSet::new();
                for requirement in &trait_method.signature.rng_clone_safe_type_params {
                    let resolved =
                        substitute_type(&Type::TypeParam(requirement.clone()), &substitutions);
                    match rng_clone_safety_in_context_with_modules(
                        &resolved,
                        &program.classes,
                        &program.enums,
                        &program.imported_modules,
                        &program.module_registry,
                    ) {
                        RngCloneSafety::Safe => {}
                        RngCloneSafety::ContainsRng => {
                            return Err(Diagnostic::coded_at(
                                "AU3007",
                                impl_method.decl.span,
                                format!(
                                    "impl method `{}` cannot satisfy the trait's clone-safety contract because `{}` contains non-cloneable `random.Rng` state",
                                    method_name, resolved
                                ),
                            ));
                        }
                        RngCloneSafety::Unknown => {
                            let params = rng_clone_obligation_params_in_context_with_modules(
                                &resolved,
                                &program.classes,
                                &program.enums,
                                &program.imported_modules,
                                &program.module_registry,
                            );
                            if params.is_empty() {
                                return Err(Diagnostic::coded_at(
                                    "AU3007",
                                    impl_method.decl.span,
                                    format!(
                                        "impl method `{}` cannot prove the trait's clone-safety requirement for `{}`",
                                        method_name, resolved
                                    ),
                                ));
                            }
                            mapped.extend(params);
                        }
                    }
                }
                mapped_impl_contracts.insert((impl_index, method_name.clone()), mapped);
            }
        }
        for ((impl_index, method_name), obligations) in &mapped_impl_contracts {
            let target = &mut program.trait_impls[*impl_index]
                .methods
                .get_mut(method_name)
                .expect("contract-mapped impl method should still exist")
                .signature
                .rng_clone_safe_type_params;
            let before = target.len();
            target.extend(obligations.iter().cloned());
            changed |= target.len() != before;
        }
        let mapped_impl_array_equality_contracts = {
            let contract_checker = FunctionChecker::new(
                &program.module_name,
                &type_names,
                &type_arities,
                &canonical_type_names,
                &program.classes,
                &program.enums,
                &program.functions,
                &program.constants,
                &program.traits,
                &program.trait_impls,
                &program.imported_modules,
                &program.module_registry,
            )
            .with_ffi(&program.extern_functions, &program.opaque_handles);
            let mut mapped = BTreeMap::<(usize, String), BTreeSet<String>>::new();
            for (impl_index, trait_impl) in program.trait_impls.iter().enumerate() {
                // Trait implementations have already been validated and linked
                // before contract propagation begins.
                let trait_info = &program.traits[&trait_impl.trait_name];
                let substitutions = self_type_substitutions(
                    &trait_info.decl,
                    &trait_impl.trait_args,
                    trait_impl.for_type.clone(),
                );
                for (method_name, impl_method) in &trait_impl.methods {
                    let trait_method = &trait_info.methods[method_name];
                    let mut requirements = BTreeSet::new();
                    for requirement in &trait_method.signature.array_equality_safe_type_params {
                        let resolved =
                            substitute_type(&Type::TypeParam(requirement.clone()), &substitutions);
                        if let Some(array_ty) = contract_checker.array_in_equality_type(&resolved) {
                            return Err(Diagnostic::coded_at(
                                "AU2003",
                                impl_method.decl.span,
                                format!(
                                    "impl method `{method_name}` cannot satisfy the trait's equality contract because `{resolved}` contains `{array_ty}`, whose equality is unavailable"
                                ),
                            )
                            .with_help(
                                "compare Array elements explicitly, or compare a chosen scalar summary such as shape, length, or a reduction result",
                            ));
                        }
                        requirements.extend(contract_checker.array_equality_type_params(&resolved));
                    }
                    mapped.insert((impl_index, method_name.clone()), requirements);
                }
            }
            mapped
        };
        for ((impl_index, method_name), obligations) in &mapped_impl_array_equality_contracts {
            let target = &mut program.trait_impls[*impl_index]
                .methods
                .get_mut(method_name)
                .expect("equality-contract-mapped impl method should still exist")
                .signature
                .array_equality_safe_type_params;
            let before = target.len();
            target.extend(obligations.iter().cloned());
            changed |= target.len() != before;
        }
        if !changed {
            program.closures = constant_closure_infos.clone();
            program.closures.extend(closure_infos);
            program.comprehensions = field_default_comprehensions.clone();
            program
                .comprehensions
                .extend(constant_comprehension_infos.clone());
            program.comprehensions.extend(comprehension_infos);
            // An explicit impl may honor a trait clone-safety contract, but it
            // may not silently strengthen it: bound-based callers can enforce
            // only requirements declared by the trait method itself.
            for ((impl_index, method_name), body_obligations) in body_impl_obligations {
                let allowed = mapped_impl_contracts
                    .get(&(impl_index, method_name.clone()))
                    .cloned()
                    .unwrap_or_default();
                let unsupported = body_obligations
                    .difference(&allowed)
                    .cloned()
                    .collect::<Vec<_>>();
                if !unsupported.is_empty() {
                    let method = &program.trait_impls[impl_index].methods[&method_name];
                    return Err(Diagnostic::coded_at(
                        "AU3007",
                        method.decl.span,
                        format!(
                            "impl method `{}` would strengthen its trait's clone-safety contract for type parameter{} {}; put the clone-producing behavior in the trait default method so callers can enforce it",
                            method_name,
                            if unsupported.len() == 1 { "" } else { "s" },
                            unsupported
                                .iter()
                                .map(|name| format!("`{}`", name))
                                .collect::<Vec<_>>()
                                .join(", ")
                        ),
                    ));
                }
            }
            for ((impl_index, method_name), body_obligations) in
                body_impl_array_equality_obligations
            {
                let allowed = mapped_impl_array_equality_contracts
                    .get(&(impl_index, method_name.clone()))
                    .cloned()
                    .unwrap_or_default();
                let unsupported = body_obligations
                    .difference(&allowed)
                    .cloned()
                    .collect::<Vec<_>>();
                if !unsupported.is_empty() {
                    let method = &program.trait_impls[impl_index].methods[&method_name];
                    return Err(Diagnostic::coded_at(
                        "AU2003",
                        method.decl.span,
                        format!(
                            "impl method `{method_name}` would strengthen its trait's equality contract for type parameter{} {}; put the equality-bearing behavior in the trait default method so callers can enforce it",
                            if unsupported.len() == 1 { "" } else { "s" },
                            unsupported
                                .iter()
                                .map(|name| format!("`{name}`"))
                                .collect::<Vec<_>>()
                                .join(", ")
                        ),
                    ));
                }
            }
            break;
        }
    }

    let top_level_checker = FunctionChecker::new(
        &program.module_name,
        &type_names,
        &type_arities,
        &canonical_type_names,
        &program.classes,
        &program.enums,
        &program.functions,
        &program.constants,
        &program.traits,
        &program.trait_impls,
        &program.imported_modules,
        &program.module_registry,
    )
    .with_ffi(&program.extern_functions, &program.opaque_handles);
    top_level_checker.check_top_level(&program.top_level_stmts)?;
    program
        .closures
        .extend(top_level_checker.closure_infos.borrow().clone());
    program
        .comprehensions
        .extend(top_level_checker.comprehension_infos.borrow().clone());

    super::aliases::validate_alias_contracts(&program, &type_names, &type_arities)?;
    Ok(program)
}

fn register_visible_namespace_types(
    namespace: &ModuleNamespace,
    prefix: &str,
    definitions: &mut TypeDefinitions,
    arities: &mut BTreeMap<String, usize>,
    canonical_names: &mut BTreeMap<String, String>,
) {
    definitions.non_value_names.insert(prefix.to_string());
    for (name, alias) in &namespace.aliases {
        let visible = format!("{prefix}.{name}");
        definitions.insert(visible.clone(), alias.decl.span);
        arities.insert(visible.clone(), alias.decl.type_params.len());
        definitions.imported_aliases.insert(visible, alias.clone());
    }
    let nominals = namespace
        .classes
        .iter()
        .map(|(name, info)| {
            (
                name,
                &info.module_name,
                &info.decl.name,
                info.decl.span,
                info.decl.type_params.len(),
            )
        })
        .chain(namespace.enums.iter().map(|(name, info)| {
            (
                name,
                &info.module_name,
                &info.decl.name,
                info.decl.span,
                info.decl.type_params.len(),
            )
        }))
        .chain(
            namespace
                .opaque_handles
                .iter()
                .map(|(name, info)| (name, &info.module_name, &info.decl.name, info.decl.span, 0)),
        )
        .chain(namespace.traits.iter().map(|(name, info)| {
            (
                name,
                &info.module_name,
                &info.decl.name,
                info.decl.span,
                info.decl.type_params.len(),
            )
        }));
    for (name, module, original, span, arity) in nominals {
        let visible = format!("{prefix}.{name}");
        definitions.insert(visible.clone(), span);
        arities.insert(visible.clone(), arity);
        canonical_names.insert(visible.clone(), format!("{module}.{original}"));
        if namespace.traits.contains_key(name) {
            definitions.non_value_names.insert(visible);
        }
    }
    for (name, child) in &namespace.modules {
        register_visible_namespace_types(
            child,
            &format!("{prefix}.{name}"),
            definitions,
            arities,
            canonical_names,
        );
    }
}

pub(super) fn register_module_namespace_types(
    namespace: &ModuleNamespace,
    type_names: &mut TypeDefinitions,
    type_arities: &mut BTreeMap<String, usize>,
) {
    for alias in namespace.aliases.values() {
        let qualified_name = format!("{}.{}", namespace.path, alias.decl.name);
        type_names.insert(qualified_name.clone(), alias.decl.span);
        type_arities.insert(qualified_name.clone(), alias.decl.type_params.len());
        type_names
            .imported_aliases
            .insert(qualified_name, alias.clone());
    }
    for handle in namespace.opaque_handles.values() {
        let qualified_name = format!("{}.{}", namespace.path, handle.decl.name);
        type_names.insert(qualified_name.clone(), handle.decl.span);
        type_arities.insert(qualified_name, 0);
    }
    for class in namespace.classes.values() {
        let qualified_name = format!("{}.{}", namespace.path, class.decl.name);
        type_names.insert(qualified_name.clone(), class.decl.span);
        type_arities.insert(qualified_name, class.decl.type_params.len());
    }
    for enum_info in namespace.enums.values() {
        let qualified_name = format!("{}.{}", namespace.path, enum_info.decl.name);
        type_names.insert(qualified_name.clone(), enum_info.decl.span);
        type_arities.insert(qualified_name, enum_info.decl.type_params.len());
    }
    for trait_info in namespace.traits.values() {
        let qualified_name = format!("{}.{}", namespace.path, trait_info.decl.name);
        type_names.non_value_names.insert(qualified_name.clone());
        type_names.insert(qualified_name.clone(), trait_info.decl.span);
        type_arities.insert(qualified_name, trait_info.decl.type_params.len());
    }
    for child in namespace.modules.values() {
        register_module_namespace_types(child, type_names, type_arities);
    }
    for imported in namespace.imported_modules.values() {
        register_module_namespace_types(imported, type_names, type_arities);
    }
}

pub(super) fn register_public_namespace_opaque_handles(
    namespace: &ModuleNamespace,
    handles: &mut BTreeMap<String, OpaqueHandleInfo>,
) {
    for handle in namespace.opaque_handles.values() {
        handles.insert(
            format!("{}.{}", namespace.path, handle.decl.name),
            handle.clone(),
        );
    }
    for child in namespace.modules.values() {
        register_public_namespace_opaque_handles(child, handles);
    }
    for imported in namespace.imported_modules.values() {
        register_public_namespace_opaque_handles(imported, handles);
    }
}
