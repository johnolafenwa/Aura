use std::collections::{BTreeMap, HashMap};
use std::fmt;

use serde::{Deserialize, Serialize};

use crate::ast::{
    Argument, AssignStmt, AssignTarget, BinaryOp, ClassDecl, EnumDecl, Expr, ExprKind,
    FunctionDecl, ImplDecl, Item, MatchStmt, Module, Param, Pattern, ReceiverKind, SelectStmt,
    Stmt, TraitDecl, TypeRef, UnaryOp, WithStmt,
};
use crate::call::{
    bind_call_arguments, callable_params_from_decl, BuiltinFunction, BuiltinMember, CallConvention,
};
use crate::diag::{Diagnostic, Result};
use crate::integer::{
    integer_type_bounds as integer_type_bounds_impl, IntegerBounds, IntegerValue,
};

#[derive(Clone, Debug)]
pub struct Program {
    pub module: Module,
    pub module_name: String,
    pub classes: BTreeMap<String, ClassInfo>,
    pub enums: BTreeMap<String, EnumInfo>,
    pub functions: BTreeMap<String, FunctionInfo>,
    pub traits: BTreeMap<String, TraitInfo>,
    pub trait_impls: Vec<TraitImplInfo>,
    pub imported_modules: BTreeMap<String, ModuleNamespace>,
    pub module_registry: BTreeMap<String, ModuleNamespace>,
    pub top_level_stmts: Vec<Stmt>,
}

#[derive(Clone, Debug)]
pub struct ClassInfo {
    pub module_name: String,
    pub decl: ClassDecl,
    pub type_param_bounds: BTreeMap<String, Vec<String>>,
    pub fields: BTreeMap<String, FieldInfo>,
    pub methods: BTreeMap<String, MethodInfo>,
}

#[derive(Clone, Debug)]
pub struct FieldInfo {
    pub public: bool,
    pub ty: Type,
}

#[derive(Clone, Debug)]
pub struct EnumInfo {
    pub module_name: String,
    pub decl: EnumDecl,
    pub type_param_bounds: BTreeMap<String, Vec<String>>,
    pub variants: BTreeMap<String, EnumVariantInfo>,
}

#[derive(Clone, Debug)]
pub struct EnumVariantInfo {
    pub payload: Option<Type>,
}

#[derive(Clone, Debug)]
pub struct FunctionInfo {
    pub module_name: String,
    pub decl: FunctionDecl,
    pub signature: FunctionSignature,
    pub type_param_bounds: BTreeMap<String, Vec<String>>,
}

#[derive(Clone, Debug)]
pub struct MethodInfo {
    pub decl: FunctionDecl,
    pub signature: FunctionSignature,
    pub type_param_bounds: BTreeMap<String, Vec<String>>,
}

#[derive(Clone, Debug)]
pub struct TraitInfo {
    pub module_name: String,
    pub decl: TraitDecl,
    pub methods: BTreeMap<String, TraitMethodInfo>,
}

#[derive(Clone, Debug)]
pub struct TraitMethodInfo {
    pub decl: FunctionDecl,
    pub signature: FunctionSignature,
    pub type_param_bounds: BTreeMap<String, Vec<String>>,
}

#[derive(Clone, Debug)]
pub struct TraitImplInfo {
    pub decl: ImplDecl,
    pub trait_name: String,
    pub for_type: Type,
    pub methods: BTreeMap<String, TraitImplMethodInfo>,
}

#[derive(Clone, Debug)]
pub struct TraitImplMethodInfo {
    pub decl: FunctionDecl,
    pub signature: FunctionSignature,
    pub type_param_bounds: BTreeMap<String, Vec<String>>,
}

#[derive(Clone, Debug)]
pub enum ImportedBinding {
    Function(FunctionInfo),
    Class(ClassInfo),
    Enum(EnumInfo),
    Trait(TraitInfo),
    Module(ModuleNamespace),
}

#[derive(Clone, Debug)]
pub struct ModuleNamespace {
    pub name: String,
    pub path: String,
    pub modules: BTreeMap<String, ModuleNamespace>,
    pub functions: BTreeMap<String, FunctionInfo>,
    pub classes: BTreeMap<String, ClassInfo>,
    pub enums: BTreeMap<String, EnumInfo>,
    pub traits: BTreeMap<String, TraitInfo>,
    pub trait_impls: Vec<TraitImplInfo>,
    pub all_functions: BTreeMap<String, FunctionInfo>,
    pub all_classes: BTreeMap<String, ClassInfo>,
    pub all_enums: BTreeMap<String, EnumInfo>,
    pub all_traits: BTreeMap<String, TraitInfo>,
    pub imported_modules: BTreeMap<String, ModuleNamespace>,
}

#[derive(Clone, Debug, Default)]
pub struct ModuleContext {
    pub module_name: String,
    pub imported_bindings: BTreeMap<String, ImportedBinding>,
    pub module_registry: BTreeMap<String, ModuleNamespace>,
}

#[derive(Clone, Debug)]
pub struct FunctionSignature {
    pub params: Vec<Type>,
    pub return_type: Type,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum Type {
    Named(String, Vec<Type>),
    TypeParam(String),
    Module(String),
    Unit,
}

impl Type {
    pub fn named(name: impl Into<String>) -> Self {
        Self::Named(name.into(), Vec::new())
    }

    pub fn is_copy(&self) -> bool {
        match self {
            Type::Unit => true,
            Type::Module(_) => false,
            Type::TypeParam(_) => false,
            Type::Named(name, args) => is_builtin_copy_named_type(name, args),
        }
    }
}

fn is_builtin_copy_named_type(name: &str, args: &[Type]) -> bool {
    args.is_empty()
        && matches!(
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
                | "Duration"
        )
}

fn type_is_copy_in_context(
    ty: &Type,
    classes: &BTreeMap<String, ClassInfo>,
    enums: &BTreeMap<String, EnumInfo>,
) -> bool {
    match ty {
        Type::Unit => true,
        Type::Module(_) => false,
        Type::TypeParam(_) => false,
        Type::Named(name, args) if is_builtin_copy_named_type(name, args) => true,
        Type::Named(name, args) if name == "Option" && args.len() == 1 => {
            type_is_copy_in_context(&args[0], classes, enums)
        }
        Type::Named(name, args) if name == "Result" && args.len() == 2 => args
            .iter()
            .all(|arg| type_is_copy_in_context(arg, classes, enums)),
        Type::Named(name, args) if name == "SendError" && args.len() == 1 => {
            type_is_copy_in_context(&args[0], classes, enums)
        }
        Type::Named(name, args) => {
            if let Some(class_info) = classes.get(name) {
                return class_info.decl.copy
                    && args
                        .iter()
                        .all(|arg| type_is_copy_in_context(arg, classes, enums));
            }
            if let Some(enum_info) = enums.get(name) {
                return enum_info.variants.values().all(|variant| {
                    variant
                        .payload
                        .as_ref()
                        .map(|payload| type_is_copy_in_context(payload, classes, enums))
                        .unwrap_or(true)
                });
            }
            false
        }
    }
}

impl fmt::Display for Type {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Type::Unit => write!(f, "None"),
            Type::Module(name) => write!(f, "module {}", name),
            Type::TypeParam(name) => write!(f, "{}", name),
            Type::Named(name, args) if args.is_empty() => write!(f, "{}", name),
            Type::Named(name, args) => {
                write!(f, "{}[", name)?;
                for (index, arg) in args.iter().enumerate() {
                    if index > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{}", arg)?;
                }
                write!(f, "]")
            }
        }
    }
}

pub fn check(module: Module) -> Result<Program> {
    check_with_context(module, ModuleContext::default())
}

pub fn check_with_context(module: Module, context: ModuleContext) -> Result<Program> {
    let module_name = if context.module_name.is_empty() {
        "<main>".to_string()
    } else {
        context.module_name.clone()
    };
    let mut type_names = BTreeMap::<String, crate::diag::Span>::new();
    let mut type_arities = BTreeMap::<String, usize>::new();
    let mut function_names = BTreeMap::<String, crate::diag::Span>::new();
    let mut item_names = BTreeMap::<String, (&'static str, crate::diag::Span)>::new();
    let mut imported_modules = BTreeMap::new();

    let mut imported_functions = BTreeMap::new();
    let mut imported_classes = BTreeMap::new();
    let mut imported_enums = BTreeMap::new();
    let mut imported_traits = BTreeMap::new();

    for (name, binding) in &context.imported_bindings {
        match binding {
            ImportedBinding::Function(function) => {
                function_names.insert(name.clone(), function.decl.span);
                item_names.insert(name.clone(), ("function", function.decl.span));
                imported_functions.insert(name.clone(), function.clone());
            }
            ImportedBinding::Class(class_info) => {
                type_names.insert(name.clone(), class_info.decl.span);
                type_arities.insert(name.clone(), class_info.decl.type_params.len());
                item_names.insert(name.clone(), ("class", class_info.decl.span));
                imported_classes.insert(name.clone(), class_info.clone());
            }
            ImportedBinding::Enum(enum_info) => {
                type_names.insert(name.clone(), enum_info.decl.span);
                type_arities.insert(name.clone(), enum_info.decl.type_params.len());
                item_names.insert(name.clone(), ("enum", enum_info.decl.span));
                imported_enums.insert(name.clone(), enum_info.clone());
            }
            ImportedBinding::Trait(trait_info) => {
                type_names.insert(name.clone(), trait_info.decl.span);
                type_arities.insert(name.clone(), trait_info.decl.type_params.len());
                item_names.insert(name.clone(), ("trait", trait_info.decl.span));
                imported_traits.insert(name.clone(), trait_info.clone());
            }
            ImportedBinding::Module(namespace) => {
                item_names.insert(name.clone(), ("module", crate::diag::Span::new(1, 1)));
                register_module_namespace_types(namespace, &mut type_names, &mut type_arities);
                imported_modules.insert(name.clone(), namespace.clone());
            }
        }
    }

    for item in &module.items {
        match item {
            Item::Class(class_decl) => {
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
            }
            Item::Enum(enum_decl) => {
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
            }
            Item::Function(function_decl) => {
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
                if let Some(existing) =
                    function_names.insert(function_decl.name.clone(), function_decl.span)
                {
                    return Err(Diagnostic::at(
                        function_decl.span,
                        format!(
                            "duplicate function `{}` (previously declared at {})",
                            function_decl.name, existing
                        ),
                    ));
                }
            }
            Item::Trait(trait_decl) => {
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

    let mut traits = imported_traits.clone();
    for item in &module.items {
        let Item::Trait(trait_decl) = item else {
            continue;
        };
        validate_type_params(&trait_decl.type_params, trait_decl.span, "trait")?;
        let trait_type_param_scope = type_param_scope(&trait_decl.type_params);
        let mut methods = BTreeMap::new();
        for method in &trait_decl.methods {
            validate_type_params(&method.type_params, method.span, "trait method")?;
            let method_type_param_scope =
                merged_type_param_scope(&trait_type_param_scope, &method.type_params);
            let params = method
                .params
                .iter()
                .map(|param| {
                    lower_type(
                        &param.ty,
                        &type_names,
                        &type_arities,
                        &method_type_param_scope,
                    )
                })
                .collect::<Result<Vec<_>>>()?;
            let return_type = lower_type(
                &method.return_type,
                &type_names,
                &type_arities,
                &method_type_param_scope,
            )?;
            let type_param_bounds =
                lower_trait_bounds(&method.type_param_bounds, &traits, method.span)?;
            if methods
                .insert(
                    method.name.clone(),
                    TraitMethodInfo {
                        decl: method.clone(),
                        signature: FunctionSignature {
                            params,
                            return_type,
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
        let type_param_bounds =
            lower_trait_bounds(&enum_decl.type_param_bounds, &traits, enum_decl.span)?;
        let mut variants = BTreeMap::new();
        let type_param_scope = type_param_scope(&enum_decl.type_params);
        for variant in &enum_decl.variants {
            let payload = variant
                .payload
                .as_ref()
                .map(|payload| lower_type(payload, &type_names, &type_arities, &type_param_scope))
                .transpose()?;
            if variants
                .insert(variant.name.clone(), EnumVariantInfo { payload })
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

    let mut classes = imported_classes.clone();
    for item in &module.items {
        let Item::Class(class_decl) = item else {
            continue;
        };
        validate_type_params(&class_decl.type_params, class_decl.span, "class")?;
        let type_param_bounds =
            lower_trait_bounds(&class_decl.type_param_bounds, &traits, class_decl.span)?;
        let mut fields = BTreeMap::new();
        let mut methods = BTreeMap::new();
        let class_type_param_scope = type_param_scope(&class_decl.type_params);
        for field in &class_decl.fields {
            let lowered = lower_type(
                &field.ty,
                &type_names,
                &type_arities,
                &class_type_param_scope,
            )?;
            if !field.ty.indirect && type_contains_named(&lowered, &class_decl.name) {
                return Err(Diagnostic::at(
                    field.span,
                    format!(
                        "recursive field `{}` on class `{}` requires `indirect`",
                        field.name, class_decl.name
                    ),
                ));
            }
            if fields
                .insert(
                    field.name.clone(),
                    FieldInfo {
                        public: field.public,
                        ty: lowered,
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
            let method_type_param_scope =
                merged_type_param_scope(&class_type_param_scope, &method.type_params);
            let type_param_bounds = merge_trait_bounds(
                &type_param_bounds,
                &lower_trait_bounds(&method.type_param_bounds, &traits, method.span)?,
            );
            let params = method
                .params
                .iter()
                .map(|param| {
                    lower_type(
                        &param.ty,
                        &type_names,
                        &type_arities,
                        &method_type_param_scope,
                    )
                })
                .collect::<Result<Vec<_>>>()?;
            let return_type = lower_type(
                &method.return_type,
                &type_names,
                &type_arities,
                &method_type_param_scope,
            )?;
            if methods
                .insert(
                    method.name.clone(),
                    MethodInfo {
                        decl: method.clone(),
                        signature: FunctionSignature {
                            params,
                            return_type,
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
                decl: class_decl.clone(),
                type_param_bounds,
                fields,
                methods,
            },
        );
    }

    for class in classes.values() {
        if !class.decl.copy {
            continue;
        }
        for field_decl in &class.decl.fields {
            let field_ty = &class
                .fields
                .get(&field_decl.name)
                .expect("class field should have lowered type")
                .ty;
            if !type_is_copy_in_context(field_ty, &classes, &enums) {
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

    let empty_functions = BTreeMap::new();
    let empty_trait_impls = Vec::new();
    let default_checker = FunctionChecker::new(
        &module_name,
        &type_names,
        &type_arities,
        &classes,
        &enums,
        &empty_functions,
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
            let lowered = class.fields.get(&field.name).unwrap().ty.clone();
            let default_ty = default_checker
                .with_type_params(class_type_param_scope.clone(), BTreeMap::new())
                .type_of_expr_hint(default, &mut HashMap::new(), Some(&lowered))?;
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

    let mut functions = imported_functions.clone();
    for item in &module.items {
        let Item::Function(function_decl) = item else {
            continue;
        };
        validate_type_params(&function_decl.type_params, function_decl.span, "function")?;
        let function_type_param_scope = type_param_scope(&function_decl.type_params);
        let type_param_bounds = lower_trait_bounds(
            &function_decl.type_param_bounds,
            &traits,
            function_decl.span,
        )?;
        let params = function_decl
            .params
            .iter()
            .map(|param| {
                lower_type(
                    &param.ty,
                    &type_names,
                    &type_arities,
                    &function_type_param_scope,
                )
            })
            .collect::<Result<Vec<_>>>()?;
        let return_type = lower_type(
            &function_decl.return_type,
            &type_names,
            &type_arities,
            &function_type_param_scope,
        )?;
        functions.insert(
            function_decl.name.clone(),
            FunctionInfo {
                module_name: module_name.clone(),
                decl: function_decl.clone(),
                signature: FunctionSignature {
                    params,
                    return_type,
                },
                type_param_bounds,
            },
        );
    }

    let mut trait_impls = Vec::new();
    for item in &module.items {
        let Item::Impl(impl_decl) = item else {
            continue;
        };
        let trait_info = traits.get(&impl_decl.trait_name).ok_or_else(|| {
            Diagnostic::at(
                impl_decl.span,
                format!("unknown trait `{}`", impl_decl.trait_name),
            )
        })?;
        let for_type = lower_type(
            &impl_decl.for_type,
            &type_names,
            &type_arities,
            &BTreeMap::new(),
        )?;
        if matches!(for_type, Type::TypeParam(_)) {
            return Err(Diagnostic::at(
                impl_decl.span,
                "trait impl target must be a concrete type",
            ));
        }
        if trait_impls.iter().any(|existing: &TraitImplInfo| {
            existing.trait_name == impl_decl.trait_name && existing.for_type == for_type
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
            let type_param_bounds =
                lower_trait_bounds(&method.type_param_bounds, &traits, method.span)?;
            let method_type_param_scope = type_param_scope(&method.type_params);
            let params = method
                .params
                .iter()
                .map(|param| {
                    lower_type(
                        &param.ty,
                        &type_names,
                        &type_arities,
                        &method_type_param_scope,
                    )
                })
                .collect::<Result<Vec<_>>>()?;
            let return_type = lower_type(
                &method.return_type,
                &type_names,
                &type_arities,
                &method_type_param_scope,
            )?;
            if params != trait_method.signature.params
                || return_type != trait_method.signature.return_type
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
                        return_type,
                    },
                    type_param_bounds,
                },
            );
        }
        for trait_method_name in trait_info.methods.keys() {
            if !methods.contains_key(trait_method_name) {
                return Err(Diagnostic::at(
                    impl_decl.span,
                    format!(
                        "impl of `{}` for `{}` is missing method `{}`",
                        impl_decl.trait_name, for_type, trait_method_name
                    ),
                ));
            }
        }
        trait_impls.push(TraitImplInfo {
            decl: impl_decl.clone(),
            trait_name: impl_decl.trait_name.clone(),
            for_type,
            methods,
        });
    }

    let program = Program {
        module: module.clone(),
        module_name,
        classes,
        enums,
        functions,
        traits,
        trait_impls,
        imported_modules,
        module_registry: context.module_registry,
        top_level_stmts: module.top_level_stmts.clone(),
    };

    if !program.top_level_stmts.is_empty() && program.functions.contains_key("main") {
        let main = program.functions.get("main").unwrap();
        return Err(Diagnostic::at(
            main.decl.span,
            "files cannot mix top-level executable statements with an explicit `main` function",
        ));
    }

    if let Some(main) = program.functions.get("main") {
        if !main.signature.params.is_empty() {
            return Err(Diagnostic::at(
                main.decl.span,
                "`main` must not take parameters in the bootstrap runtime",
            ));
        }
    }

    let checker = FunctionChecker::new(
        &program.module_name,
        &type_names,
        &type_arities,
        &program.classes,
        &program.enums,
        &program.functions,
        &program.traits,
        &program.trait_impls,
        &program.imported_modules,
        &program.module_registry,
    );
    for trait_info in program.traits.values() {
        let trait_type_param_scope = type_param_scope(&trait_info.decl.type_params);
        for method in trait_info.methods.values() {
            let method_type_param_scope =
                merged_type_param_scope(&trait_type_param_scope, &method.decl.type_params);
            checker.check_param_defaults(
                &method.decl.params,
                &method_type_param_scope,
                false,
                "trait method",
            )?;
        }
    }
    for function in program.functions.values() {
        checker
            .with_module_name(&function.module_name)
            .check_function(&function.decl)?;
    }

    for class in program.classes.values() {
        for method in class.methods.values() {
            checker
                .with_module_name(&class.module_name)
                .check_method(&class.decl, &method.decl)?;
        }
    }

    for trait_impl in &program.trait_impls {
        for method in trait_impl.methods.values() {
            checker.check_trait_impl_method(&trait_impl.for_type, &method.decl)?;
        }
    }

    checker.check_top_level(&program.top_level_stmts)?;

    Ok(program)
}

fn lower_type(
    type_ref: &TypeRef,
    type_names: &BTreeMap<String, crate::diag::Span>,
    type_arities: &BTreeMap<String, usize>,
    type_params: &BTreeMap<String, ()>,
) -> Result<Type> {
    let type_name = if type_ref.name == "str" {
        "String"
    } else {
        type_ref.name.as_str()
    };

    if type_params.contains_key(type_name) {
        if !type_ref.args.is_empty() {
            return Err(Diagnostic::at(
                type_ref.span,
                format!(
                    "type parameter `{}` does not take type arguments",
                    type_name
                ),
            ));
        }
        return Ok(Type::TypeParam(type_name.to_string()));
    }

    if type_name == "None" {
        if !type_ref.args.is_empty() {
            return Err(Diagnostic::at(
                type_ref.span,
                "`None` does not take generic arguments",
            ));
        }
        return Ok(Type::Unit);
    }

    let args = type_ref
        .args
        .iter()
        .map(|arg| lower_type(arg, type_names, type_arities, type_params))
        .collect::<Result<Vec<_>>>()?;

    if type_name == "Option" {
        if args.len() != 1 {
            return Err(Diagnostic::at(
                type_ref.span,
                "`Option` expects exactly one type argument",
            ));
        }
        return Ok(Type::Named(type_name.to_string(), args));
    }

    if type_name == "Result" {
        if args.len() != 2 {
            return Err(Diagnostic::at(
                type_ref.span,
                "`Result` expects exactly two type arguments",
            ));
        }
        return Ok(Type::Named(type_name.to_string(), args));
    }

    if type_name == "Channel" || type_name == "Task" || type_name == "SendError" {
        if args.len() != 1 {
            return Err(Diagnostic::at(
                type_ref.span,
                format!("`{}` expects exactly one type argument", type_name),
            ));
        }
        return Ok(Type::Named(type_name.to_string(), args));
    }

    if type_name == "TaskGroup" || type_name == "Duration" {
        if !args.is_empty() {
            return Err(Diagnostic::at(
                type_ref.span,
                format!("`{}` does not take type arguments", type_name),
            ));
        }
        return Ok(Type::Named(type_name.to_string(), args));
    }

    if let Some(expected_arity) = type_arities.get(type_name) {
        if args.len() != *expected_arity {
            return Err(Diagnostic::at(
                type_ref.span,
                format!(
                    "`{}` expects exactly {} type argument{}, found {}",
                    type_name,
                    expected_arity,
                    if *expected_arity == 1 { "" } else { "s" },
                    args.len()
                ),
            ));
        }
    } else if is_builtin_type(type_name) || type_names.contains_key(type_name) {
        if !args.is_empty() {
            return Err(Diagnostic::at(
                type_ref.span,
                format!("`{}` does not take type arguments", type_name),
            ));
        }
    }

    if is_builtin_type(type_name) || type_names.contains_key(type_name) {
        let canonical_name = type_name
            .rsplit_once('.')
            .map(|(_, leaf)| leaf)
            .unwrap_or(type_name);
        Ok(Type::Named(canonical_name.to_string(), args))
    } else {
        Err(Diagnostic::at(
            type_ref.span,
            format!("unknown type `{}`", type_ref.name),
        ))
    }
}

fn register_module_namespace_types(
    namespace: &ModuleNamespace,
    type_names: &mut BTreeMap<String, crate::diag::Span>,
    type_arities: &mut BTreeMap<String, usize>,
) {
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
        type_names.insert(qualified_name.clone(), trait_info.decl.span);
        type_arities.insert(qualified_name, trait_info.decl.type_params.len());
    }
    for child in namespace.modules.values() {
        register_module_namespace_types(child, type_names, type_arities);
    }
}

fn validate_type_params(
    type_params: &[String],
    span: crate::diag::Span,
    owner: &str,
) -> Result<()> {
    let mut seen = BTreeMap::new();
    for name in type_params {
        if seen.insert(name.clone(), ()).is_some() {
            return Err(Diagnostic::at(
                span,
                format!("duplicate type parameter `{}` on {}", name, owner),
            ));
        }
    }
    Ok(())
}

fn type_param_scope(type_params: &[String]) -> BTreeMap<String, ()> {
    type_params
        .iter()
        .cloned()
        .map(|name| (name, ()))
        .collect::<BTreeMap<_, _>>()
}

fn merged_type_param_scope(
    parent: &BTreeMap<String, ()>,
    added: &[String],
) -> BTreeMap<String, ()> {
    let mut merged = parent.clone();
    for name in added {
        merged.insert(name.clone(), ());
    }
    merged
}

fn default_argument_references_param(expr: &Expr, param_names: &[String]) -> Option<String> {
    match &expr.kind {
        ExprKind::Name(name) => param_names
            .iter()
            .find(|param_name| *param_name == name)
            .cloned(),
        ExprKind::Group(inner) | ExprKind::Try(inner) => {
            default_argument_references_param(inner, param_names)
        }
        ExprKind::Unary { expr, .. } | ExprKind::Cast { expr, .. } => {
            default_argument_references_param(expr, param_names)
        }
        ExprKind::Spawn { value, .. } => default_argument_references_param(value, param_names),
        ExprKind::Specialize { expr, .. } => default_argument_references_param(expr, param_names),
        ExprKind::Member { object, .. } => default_argument_references_param(object, param_names),
        ExprKind::Call { callee, args } => default_argument_references_param(callee, param_names)
            .or_else(|| {
                args.iter().find_map(|argument| {
                    default_argument_references_param(&argument.value, param_names)
                })
            }),
        ExprKind::FString(parts) => parts.iter().find_map(|part| match part {
            crate::ast::FormatPart::Literal(_) => None,
            crate::ast::FormatPart::Expr(expr) => {
                default_argument_references_param(expr, param_names)
            }
        }),
        ExprKind::Binary { left, right, .. } => {
            default_argument_references_param(left, param_names)
                .or_else(|| default_argument_references_param(right, param_names))
        }
        ExprKind::Int(_)
        | ExprKind::Float(_)
        | ExprKind::Bool(_)
        | ExprKind::String(_)
        | ExprKind::DurationMillis(_) => None,
    }
}

fn lower_trait_bounds(
    bounds: &BTreeMap<String, Vec<TypeRef>>,
    traits: &BTreeMap<String, TraitInfo>,
    _span: crate::diag::Span,
) -> Result<BTreeMap<String, Vec<String>>> {
    let mut lowered = BTreeMap::new();
    for (type_param, trait_bounds) in bounds {
        let mut names = Vec::new();
        for bound in trait_bounds {
            if !bound.args.is_empty() {
                return Err(Diagnostic::at(
                    bound.span,
                    "generic trait bounds are not implemented yet",
                ));
            }
            if !traits.contains_key(&bound.name) {
                return Err(Diagnostic::at(
                    bound.span,
                    format!("unknown trait `{}`", bound.name),
                ));
            }
            names.push(bound.name.clone());
        }
        lowered.insert(type_param.clone(), names);
    }
    Ok(lowered)
}

fn merge_trait_bounds(
    left: &BTreeMap<String, Vec<String>>,
    right: &BTreeMap<String, Vec<String>>,
) -> BTreeMap<String, Vec<String>> {
    let mut merged = left.clone();
    for (type_param, bounds) in right {
        merged
            .entry(type_param.clone())
            .or_default()
            .extend(bounds.iter().cloned());
    }
    merged
}

fn type_contains_named(ty: &Type, target: &str) -> bool {
    match ty {
        Type::Named(name, args) => {
            name == target || args.iter().any(|arg| type_contains_named(arg, target))
        }
        Type::TypeParam(_) | Type::Module(_) | Type::Unit => false,
    }
}

pub(crate) fn substitute_type(ty: &Type, substitutions: &HashMap<String, Type>) -> Type {
    match ty {
        Type::Unit => Type::Unit,
        Type::Module(name) => Type::Module(name.clone()),
        Type::TypeParam(name) => substitutions
            .get(name)
            .cloned()
            .unwrap_or_else(|| Type::TypeParam(name.clone())),
        Type::Named(name, args) => Type::Named(
            name.clone(),
            args.iter()
                .map(|arg| substitute_type(arg, substitutions))
                .collect(),
        ),
    }
}

fn has_unresolved_type_params(ty: &Type) -> bool {
    match ty {
        Type::Unit => false,
        Type::Module(_) => false,
        Type::TypeParam(_) => true,
        Type::Named(_, args) => args.iter().any(has_unresolved_type_params),
    }
}

pub(crate) fn substitutions_from_decl_type_args(
    type_params: &[String],
    actual_args: &[Type],
) -> HashMap<String, Type> {
    type_params
        .iter()
        .cloned()
        .zip(actual_args.iter().cloned())
        .collect()
}

fn unify_type_pattern(
    pattern: &Type,
    actual: &Type,
    substitutions: &mut HashMap<String, Type>,
) -> Result<()> {
    match pattern {
        Type::Unit => {
            if actual == &Type::Unit {
                Ok(())
            } else {
                Err(Diagnostic::new(format!(
                    "expected `None`, found `{}`",
                    actual
                )))
            }
        }
        Type::Module(name) => {
            if actual == &Type::Module(name.clone()) {
                Ok(())
            } else {
                Err(Diagnostic::new(format!(
                    "expected `module {}`, found `{}`",
                    name, actual
                )))
            }
        }
        Type::TypeParam(name) => {
            if let Some(existing) = substitutions.get(name) {
                if existing == actual {
                    Ok(())
                } else {
                    Err(Diagnostic::new(format!(
                        "conflicting inferred types for `{}`: `{}` and `{}`",
                        name, existing, actual
                    )))
                }
            } else {
                substitutions.insert(name.clone(), actual.clone());
                Ok(())
            }
        }
        Type::Named(name, args) => {
            let Type::Named(actual_name, actual_args) = actual else {
                return Err(Diagnostic::new(format!(
                    "expected `{}`, found `{}`",
                    pattern, actual
                )));
            };
            if name != actual_name || args.len() != actual_args.len() {
                return Err(Diagnostic::new(format!(
                    "expected `{}`, found `{}`",
                    pattern, actual
                )));
            }
            for (pattern_arg, actual_arg) in args.iter().zip(actual_args.iter()) {
                unify_type_pattern(pattern_arg, actual_arg, substitutions)?;
            }
            Ok(())
        }
    }
}

fn is_builtin_type(name: &str) -> bool {
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
            | "Channel"
            | "Task"
            | "SendError"
            | "TaskGroup"
            | "Duration"
    )
}

pub(crate) fn integer_type_bounds(ty: &Type) -> Option<IntegerBounds> {
    integer_type_bounds_impl(ty)
}

fn is_integer_type(ty: &Type) -> bool {
    integer_type_bounds(ty).is_some()
}

fn is_float_type(ty: &Type) -> bool {
    matches!(ty, Type::Named(name, args) if args.is_empty() && matches!(name.as_str(), "float32" | "float64"))
}

fn is_numeric_type(ty: &Type) -> bool {
    is_integer_type(ty) || is_float_type(ty)
}

#[derive(Clone)]
struct LocalBinding {
    ty: Type,
    assignable: bool,
    mutable_place: bool,
    passing: ReceiverKind,
    moved: bool,
}

struct FunctionChecker<'a> {
    module_name: &'a str,
    type_names: &'a BTreeMap<String, crate::diag::Span>,
    type_arities: &'a BTreeMap<String, usize>,
    classes: &'a BTreeMap<String, ClassInfo>,
    enums: &'a BTreeMap<String, EnumInfo>,
    functions: &'a BTreeMap<String, FunctionInfo>,
    traits: &'a BTreeMap<String, TraitInfo>,
    trait_impls: &'a [TraitImplInfo],
    imported_modules: &'a BTreeMap<String, ModuleNamespace>,
    module_registry: &'a BTreeMap<String, ModuleNamespace>,
    current_return_type: Option<Type>,
    type_params: BTreeMap<String, ()>,
    type_param_bounds: BTreeMap<String, Vec<String>>,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
enum BlockFlow {
    FallsThrough,
    AlwaysReturns,
}

impl<'a> FunctionChecker<'a> {
    fn is_copy_type(&self, ty: &Type) -> bool {
        type_is_copy_in_context(ty, self.classes, self.enums)
    }

    fn seed_imported_modules(&self, locals: &mut HashMap<String, LocalBinding>) {
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
                    passing: ReceiverKind::Value,
                    moved: false,
                },
            );
        }
    }

    fn new(
        module_name: &'a str,
        type_names: &'a BTreeMap<String, crate::diag::Span>,
        type_arities: &'a BTreeMap<String, usize>,
        classes: &'a BTreeMap<String, ClassInfo>,
        enums: &'a BTreeMap<String, EnumInfo>,
        functions: &'a BTreeMap<String, FunctionInfo>,
        traits: &'a BTreeMap<String, TraitInfo>,
        trait_impls: &'a [TraitImplInfo],
        imported_modules: &'a BTreeMap<String, ModuleNamespace>,
        module_registry: &'a BTreeMap<String, ModuleNamespace>,
    ) -> Self {
        Self {
            module_name,
            type_names,
            type_arities,
            classes,
            enums,
            functions,
            traits,
            trait_impls,
            imported_modules,
            module_registry,
            current_return_type: None,
            type_params: BTreeMap::new(),
            type_param_bounds: BTreeMap::new(),
        }
    }

    fn with_return_type(&self, return_type: Type) -> Self {
        Self {
            module_name: self.module_name,
            type_names: self.type_names,
            type_arities: self.type_arities,
            classes: self.classes,
            enums: self.enums,
            functions: self.functions,
            traits: self.traits,
            trait_impls: self.trait_impls,
            imported_modules: self.imported_modules,
            module_registry: self.module_registry,
            current_return_type: Some(return_type),
            type_params: self.type_params.clone(),
            type_param_bounds: self.type_param_bounds.clone(),
        }
    }

    fn with_type_params(
        &self,
        type_params: BTreeMap<String, ()>,
        type_param_bounds: BTreeMap<String, Vec<String>>,
    ) -> Self {
        Self {
            module_name: self.module_name,
            type_names: self.type_names,
            type_arities: self.type_arities,
            classes: self.classes,
            enums: self.enums,
            functions: self.functions,
            traits: self.traits,
            trait_impls: self.trait_impls,
            imported_modules: self.imported_modules,
            module_registry: self.module_registry,
            current_return_type: self.current_return_type.clone(),
            type_params,
            type_param_bounds,
        }
    }

    fn with_module_name(&self, module_name: &'a str) -> Self {
        Self {
            module_name,
            type_names: self.type_names,
            type_arities: self.type_arities,
            classes: self.classes,
            enums: self.enums,
            functions: self.functions,
            traits: self.traits,
            trait_impls: self.trait_impls,
            imported_modules: self.imported_modules,
            module_registry: self.module_registry,
            current_return_type: self.current_return_type.clone(),
            type_params: self.type_params.clone(),
            type_param_bounds: self.type_param_bounds.clone(),
        }
    }

    fn peel_specialization<'b>(&self, expr: &'b Expr) -> (&'b Expr, Option<&'b [TypeRef]>) {
        match &expr.kind {
            ExprKind::Specialize { expr, type_args } => (&**expr, Some(type_args.as_slice())),
            _ => (expr, None),
        }
    }

    fn lower_explicit_type_args(&self, type_args: &[TypeRef]) -> Result<Vec<Type>> {
        type_args
            .iter()
            .map(|type_arg| {
                lower_type(
                    type_arg,
                    self.type_names,
                    self.type_arities,
                    &self.type_params,
                )
            })
            .collect()
    }

    fn explicit_type_substitutions(
        &self,
        type_params: &[String],
        type_args: &[TypeRef],
        span: crate::diag::Span,
        callee_name: &str,
    ) -> Result<HashMap<String, Type>> {
        let lowered = self.lower_explicit_type_args(type_args)?;
        if lowered.len() != type_params.len() {
            return Err(Diagnostic::at(
                span,
                format!(
                    "{} expects {} type argument{}, found {}",
                    callee_name,
                    type_params.len(),
                    if type_params.len() == 1 { "" } else { "s" },
                    lowered.len()
                ),
            ));
        }
        Ok(substitutions_from_decl_type_args(type_params, &lowered))
    }

    fn validate_integer_literal(
        &self,
        value: u128,
        target_ty: &Type,
        span: crate::diag::Span,
    ) -> Result<()> {
        let Some(bounds) = integer_type_bounds(target_ty) else {
            return Ok(());
        };
        if !IntegerValue::from_literal(value).fits_bounds(bounds) {
            return Err(Diagnostic::at(
                span,
                format!(
                    "integer literal `{}` does not fit in `{}`",
                    value, target_ty
                ),
            ));
        }
        Ok(())
    }

    fn validate_negative_integer_literal(
        &self,
        value: u128,
        target_ty: &Type,
        span: crate::diag::Span,
    ) -> Result<()> {
        let Some(bounds) = integer_type_bounds(target_ty) else {
            return Ok(());
        };
        let Some(negative) = IntegerValue::from_literal(value).checked_neg() else {
            return Err(Diagnostic::at(
                span,
                format!(
                    "integer literal `-{}` does not fit in `{}`",
                    value, target_ty
                ),
            ));
        };
        if !negative.fits_bounds(bounds) {
            return Err(Diagnostic::at(
                span,
                format!(
                    "integer literal `-{}` does not fit in `{}`",
                    value, target_ty
                ),
            ));
        }
        Ok(())
    }

    fn consume_binding(
        &self,
        name: &str,
        span: crate::diag::Span,
        locals: &mut HashMap<String, LocalBinding>,
    ) -> Result<()> {
        let binding = locals
            .get_mut(name)
            .ok_or_else(|| Diagnostic::at(span, format!("unknown name `{}`", name)))?;
        if self.is_copy_type(&binding.ty) {
            return Ok(());
        }
        if binding.passing != ReceiverKind::Value {
            return Err(Diagnostic::at(
                span,
                format!("cannot move borrowed value `{}`", name),
            ));
        }
        if binding.moved {
            return Err(Diagnostic::at(
                span,
                format!("use of moved value `{}`", name),
            ));
        }
        binding.moved = true;
        Ok(())
    }

    fn consume_value_expr(
        &self,
        expr: &Expr,
        locals: &mut HashMap<String, LocalBinding>,
    ) -> Result<()> {
        match &expr.kind {
            ExprKind::Name(name) if name == "None" => Ok(()),
            ExprKind::Name(name) => self.consume_binding(name, expr.span, locals),
            ExprKind::Group(inner) => self.consume_value_expr(inner, locals),
            ExprKind::Cast { expr, .. } => self.consume_value_expr(expr, locals),
            ExprKind::Specialize { expr, .. } => self.consume_value_expr(expr, locals),
            ExprKind::Member { object, field } => {
                if let ExprKind::Name(enum_name) = &object.kind {
                    if enum_name == "Option" && field == "None" {
                        return Ok(());
                    }
                    if let Some(enum_info) = self.resolve_enum_info(enum_name) {
                        if enum_info
                            .variants
                            .get(field)
                            .is_some_and(|variant| variant.payload.is_none())
                        {
                            return Ok(());
                        }
                    }
                }
                if let Some((module_path, enum_name)) = self.qualified_module_item(object) {
                    if let Some(namespace) = self.module_namespace(&module_path) {
                        if namespace
                            .enums
                            .get(&enum_name)
                            .and_then(|enum_info| enum_info.variants.get(field))
                            .is_some_and(|variant| variant.payload.is_none())
                        {
                            return Ok(());
                        }
                    }
                }
                let object_ty = self.type_of_expr(object, locals)?;
                let member_ty = self.resolve_member_type(&object_ty, field, expr.span)?;
                if !self.is_copy_type(&member_ty) {
                    if let Some(name) = self.borrowed_root_binding_name(object, locals) {
                        return Err(Diagnostic::at(
                            expr.span,
                            format!(
                                "cannot move non-copy field `{}` out of borrowed value `{}`",
                                field, name
                            ),
                        ));
                    }
                }
                Ok(())
            }
            _ => Ok(()),
        }
    }

    fn merge_control_flow_moves(
        &self,
        locals: &mut HashMap<String, LocalBinding>,
        branch_states: &[&HashMap<String, LocalBinding>],
    ) {
        let binding_names = locals.keys().cloned().collect::<Vec<_>>();
        for name in binding_names {
            let moved = branch_states.iter().any(|state| {
                state
                    .get(&name)
                    .map(|binding| binding.moved)
                    .unwrap_or(false)
            });
            if let Some(binding) = locals.get_mut(&name) {
                binding.moved = moved;
            }
        }
    }

    fn reject_loop_carried_moves(
        &self,
        locals: &HashMap<String, LocalBinding>,
        body_locals: &HashMap<String, LocalBinding>,
        loop_kind: &str,
        span: crate::diag::Span,
    ) -> Result<()> {
        for (name, binding) in locals {
            if self.is_copy_type(&binding.ty) {
                continue;
            }
            let Some(body_binding) = body_locals.get(name) else {
                continue;
            };
            if !binding.moved && body_binding.moved {
                return Err(Diagnostic::at(
                    span,
                    format!(
                        "`{}` loop body moves `{}` and may execute more than once",
                        loop_kind, name
                    ),
                ));
            }
        }
        Ok(())
    }

    fn check_param_defaults(
        &self,
        params: &[Param],
        type_param_scope: &BTreeMap<String, ()>,
        allow_defaults: bool,
        owner: &str,
    ) -> Result<()> {
        let mut saw_default = false;
        let param_names = params
            .iter()
            .map(|param| param.name.clone())
            .collect::<Vec<_>>();

        for param in params {
            if param.passing != ReceiverKind::Value && param.default.is_some() {
                return Err(Diagnostic::at(
                    param.span,
                    format!(
                        "borrowed parameter `{}` may not have a default value",
                        param.name
                    ),
                ));
            }
            let lowered = lower_type(
                &param.ty,
                self.type_names,
                self.type_arities,
                type_param_scope,
            )?;
            match &param.default {
                Some(default) => {
                    if !allow_defaults {
                        return Err(Diagnostic::at(
                            param.span,
                            format!(
                                "default arguments are not allowed in {} declarations",
                                owner
                            ),
                        ));
                    }
                    saw_default = true;
                    if let Some(name) = default_argument_references_param(default, &param_names) {
                        return Err(Diagnostic::at(
                            default.span,
                            format!(
                                "default argument for parameter `{}` may not reference parameter `{}`",
                                param.name, name
                            ),
                        ));
                    }
                    let default_ty =
                        self.type_of_expr_hint(default, &mut HashMap::new(), Some(&lowered))?;
                    if default_ty != lowered {
                        return Err(Diagnostic::at(
                            default.span,
                            format!(
                                "default argument for parameter `{}` has type `{}`, expected `{}`",
                                param.name, default_ty, lowered
                            ),
                        ));
                    }
                }
                None if saw_default => {
                    return Err(Diagnostic::at(
                        param.span,
                        "parameters with default arguments must come after required parameters",
                    ));
                }
                None => {}
            }
        }

        Ok(())
    }

    fn check_function(&self, function: &FunctionDecl) -> Result<()> {
        let type_param_scope = type_param_scope(&function.type_params);
        let type_param_bounds =
            lower_trait_bounds(&function.type_param_bounds, self.traits, function.span)?;
        let return_type = lower_type(
            &function.return_type,
            self.type_names,
            self.type_arities,
            &type_param_scope,
        )?;
        let checker = self
            .with_type_params(type_param_scope.clone(), type_param_bounds)
            .with_return_type(return_type.clone());
        checker.check_param_defaults(&function.params, &type_param_scope, true, "function")?;
        let mut locals = HashMap::new();
        checker.seed_imported_modules(&mut locals);
        for param in &function.params {
            let ty = lower_type(
                &param.ty,
                self.type_names,
                self.type_arities,
                &type_param_scope,
            )?;
            locals.insert(
                param.name.clone(),
                LocalBinding {
                    ty,
                    assignable: false,
                    mutable_place: param.passing == ReceiverKind::BorrowMut,
                    passing: param.passing,
                    moved: false,
                },
            );
        }

        let flow = checker.check_block(&function.body, &mut locals, &return_type, 0, true)?;
        if return_type != Type::Unit && flow != BlockFlow::AlwaysReturns {
            return Err(Diagnostic::at(
                function.span,
                format!("function `{}` is missing a return", function.name),
            ));
        }

        Ok(())
    }

    fn check_method(&self, class_decl: &ClassDecl, method: &FunctionDecl) -> Result<()> {
        let class_type_param_scope = type_param_scope(&class_decl.type_params);
        let method_type_param_scope =
            merged_type_param_scope(&class_type_param_scope, &method.type_params);
        let class_type_param_bounds = self
            .classes
            .get(&class_decl.name)
            .map(|class_info| class_info.type_param_bounds.clone())
            .unwrap_or_default();
        let type_param_bounds = merge_trait_bounds(
            &class_type_param_bounds,
            &lower_trait_bounds(&method.type_param_bounds, self.traits, method.span)?,
        );
        let return_type = lower_type(
            &method.return_type,
            self.type_names,
            self.type_arities,
            &method_type_param_scope,
        )?;
        let checker = self
            .with_type_params(method_type_param_scope.clone(), type_param_bounds)
            .with_return_type(return_type.clone());
        checker.check_param_defaults(&method.params, &method_type_param_scope, true, "method")?;
        let mut locals = HashMap::new();
        checker.seed_imported_modules(&mut locals);
        if let Some(receiver_kind) = method.receiver {
            locals.insert(
                "self".to_string(),
                LocalBinding {
                    ty: Type::Named(
                        class_decl.name.clone(),
                        class_decl
                            .type_params
                            .iter()
                            .cloned()
                            .map(Type::TypeParam)
                            .collect(),
                    ),
                    assignable: false,
                    mutable_place: receiver_kind == ReceiverKind::BorrowMut,
                    passing: receiver_kind,
                    moved: false,
                },
            );
        }
        for param in &method.params {
            let ty = lower_type(
                &param.ty,
                self.type_names,
                self.type_arities,
                &method_type_param_scope,
            )?;
            locals.insert(
                param.name.clone(),
                LocalBinding {
                    ty,
                    assignable: false,
                    mutable_place: param.passing == ReceiverKind::BorrowMut,
                    passing: param.passing,
                    moved: false,
                },
            );
        }

        let flow = checker.check_block(&method.body, &mut locals, &return_type, 0, true)?;
        if return_type != Type::Unit && flow != BlockFlow::AlwaysReturns {
            return Err(Diagnostic::at(
                method.span,
                format!("method `{}` is missing a return", method.name),
            ));
        }

        Ok(())
    }

    fn check_trait_impl_method(&self, for_type: &Type, method: &FunctionDecl) -> Result<()> {
        let type_param_scope = type_param_scope(&method.type_params);
        let type_param_bounds =
            lower_trait_bounds(&method.type_param_bounds, self.traits, method.span)?;
        let return_type = lower_type(
            &method.return_type,
            self.type_names,
            self.type_arities,
            &type_param_scope,
        )?;
        let checker = self
            .with_type_params(type_param_scope.clone(), type_param_bounds)
            .with_return_type(return_type.clone());
        checker.check_param_defaults(&method.params, &type_param_scope, false, "impl method")?;
        let mut locals = HashMap::new();
        checker.seed_imported_modules(&mut locals);
        if let Some(receiver_kind) = method.receiver {
            locals.insert(
                "self".to_string(),
                LocalBinding {
                    ty: for_type.clone(),
                    assignable: false,
                    mutable_place: receiver_kind == ReceiverKind::BorrowMut,
                    passing: receiver_kind,
                    moved: false,
                },
            );
        }
        for param in &method.params {
            let ty = lower_type(
                &param.ty,
                self.type_names,
                self.type_arities,
                &type_param_scope,
            )?;
            locals.insert(
                param.name.clone(),
                LocalBinding {
                    ty,
                    assignable: false,
                    mutable_place: param.passing == ReceiverKind::BorrowMut,
                    passing: param.passing,
                    moved: false,
                },
            );
        }
        let flow = checker.check_block(&method.body, &mut locals, &return_type, 0, true)?;
        if return_type != Type::Unit && flow != BlockFlow::AlwaysReturns {
            return Err(Diagnostic::at(
                method.span,
                format!("method `{}` is missing a return", method.name),
            ));
        }
        Ok(())
    }

    fn check_top_level(&self, body: &[Stmt]) -> Result<()> {
        let mut locals = HashMap::new();
        self.seed_imported_modules(&mut locals);
        self.check_block(body, &mut locals, &Type::Unit, 0, false)?;
        Ok(())
    }

    fn check_block(
        &self,
        body: &[Stmt],
        locals: &mut HashMap<String, LocalBinding>,
        return_type: &Type,
        loop_depth: usize,
        allow_return: bool,
    ) -> Result<BlockFlow> {
        let mut flow = BlockFlow::FallsThrough;

        for stmt in body {
            match stmt {
                Stmt::Assign(assign) => self.check_assign(assign, locals)?,
                Stmt::Pass(_) => {}
                Stmt::Expr(expr_stmt) => {
                    self.type_of_expr(&expr_stmt.expr, locals)?;
                    self.consume_value_expr(&expr_stmt.expr, locals)?;
                }
                Stmt::Return(return_stmt) => {
                    if !allow_return {
                        return Err(Diagnostic::at(
                            return_stmt.span,
                            "`return` is only allowed inside a function body",
                        ));
                    }
                    let ty = if let Some(value) = &return_stmt.value {
                        self.type_of_expr_hint(value, locals, Some(return_type))?
                    } else {
                        Type::Unit
                    };
                    if &ty != return_type {
                        return Err(Diagnostic::at(
                            return_stmt.span,
                            format!(
                                "return type mismatch: expected `{}`, found `{}`",
                                return_type, ty
                            ),
                        ));
                    }
                    if let Some(value) = &return_stmt.value {
                        self.consume_value_expr(value, locals)?;
                    }
                    flow = BlockFlow::AlwaysReturns;
                    break;
                }
                Stmt::If(if_stmt) => {
                    for branch in &if_stmt.branches {
                        let condition_ty = self.type_of_expr(&branch.condition, locals)?;
                        if condition_ty != Type::named("bool") {
                            return Err(Diagnostic::at(
                                branch.span,
                                format!(
                                    "`if` condition must have type `bool`, found `{}`",
                                    condition_ty
                                ),
                            ));
                        }
                    }

                    let mut all_return = true;
                    let mut branch_states = Vec::new();
                    for branch in &if_stmt.branches {
                        let mut branch_locals = locals.clone();
                        let branch_flow = self.check_block(
                            &branch.body,
                            &mut branch_locals,
                            return_type,
                            loop_depth,
                            allow_return,
                        )?;
                        if branch_flow != BlockFlow::AlwaysReturns {
                            all_return = false;
                        }
                        branch_states.push(branch_locals);
                    }

                    let mut else_state = None;
                    if let Some(else_body) = &if_stmt.else_body {
                        let mut else_locals = locals.clone();
                        let else_flow = self.check_block(
                            else_body,
                            &mut else_locals,
                            return_type,
                            loop_depth,
                            allow_return,
                        )?;
                        if else_flow != BlockFlow::AlwaysReturns {
                            all_return = false;
                        }
                        else_state = Some(else_locals);
                    } else {
                        all_return = false;
                    }

                    if let Some(ref else_locals) = else_state {
                        let states = branch_states
                            .iter()
                            .map(|state| state as &HashMap<String, LocalBinding>)
                            .chain(std::iter::once(
                                else_locals as &HashMap<String, LocalBinding>,
                            ))
                            .collect::<Vec<_>>();
                        self.merge_control_flow_moves(locals, &states);
                    } else {
                        let baseline_locals = locals.clone();
                        let mut states = branch_states
                            .iter()
                            .map(|state| state as &HashMap<String, LocalBinding>)
                            .collect::<Vec<_>>();
                        states.push(&baseline_locals);
                        self.merge_control_flow_moves(locals, &states);
                    }

                    if all_return {
                        flow = BlockFlow::AlwaysReturns;
                        break;
                    }
                }
                Stmt::Match(match_stmt) => {
                    let match_flow = self.check_match(
                        match_stmt,
                        locals,
                        return_type,
                        loop_depth,
                        allow_return,
                    )?;
                    if match_flow == BlockFlow::AlwaysReturns {
                        flow = BlockFlow::AlwaysReturns;
                        break;
                    }
                }
                Stmt::For(for_stmt) => {
                    let iterable_ty = self.type_of_expr(&for_stmt.iterable, locals)?;
                    let (binding_ty, binding_passing) = match (&iterable_ty, for_stmt.borrow_mode) {
                        (Type::Named(name, _), _) if name == "Range" => {
                            (Type::named("int32"), ReceiverKind::Value)
                        }
                        (Type::Named(name, args), borrow_mode)
                            if name == "Channel" && args.len() == 1 =>
                        {
                            let element_ty = args[0].clone();
                            let passing =
                                if borrow_mode.is_some() && !self.is_copy_type(&element_ty) {
                                    borrow_mode.unwrap()
                                } else {
                                    ReceiverKind::Value
                                };
                            (element_ty, passing)
                        }
                        _ => {
                            return Err(Diagnostic::at(
                                for_stmt.span,
                                format!(
                                    "`for` currently requires a `Range` or `Channel[T]` iterable, found `{}`",
                                    iterable_ty
                                ),
                            ))
                        }
                    };
                    if for_stmt.borrow_mode.is_none() && !self.is_copy_type(&iterable_ty) {
                        self.consume_value_expr(&for_stmt.iterable, locals)?;
                    }
                    if locals.contains_key(&for_stmt.binding) {
                        return Err(Diagnostic::at(
                            for_stmt.span,
                            format!(
                                "loop binding `{}` would shadow an existing name",
                                for_stmt.binding
                            ),
                        ));
                    }
                    let mut body_locals = locals.clone();
                    body_locals.insert(
                        for_stmt.binding.clone(),
                        LocalBinding {
                            ty: binding_ty,
                            assignable: false,
                            mutable_place: false,
                            passing: binding_passing,
                            moved: false,
                        },
                    );
                    self.check_block(
                        &for_stmt.body,
                        &mut body_locals,
                        return_type,
                        loop_depth + 1,
                        allow_return,
                    )?;
                    self.reject_loop_carried_moves(locals, &body_locals, "for", for_stmt.span)?;
                    self.merge_control_flow_moves(locals, &[&body_locals]);
                }
                Stmt::With(with_stmt) => {
                    let with_flow =
                        self.check_with(with_stmt, locals, return_type, loop_depth, allow_return)?;
                    if with_flow == BlockFlow::AlwaysReturns {
                        flow = BlockFlow::AlwaysReturns;
                        break;
                    }
                }
                Stmt::Select(select_stmt) => {
                    let select_flow = self.check_select(
                        select_stmt,
                        locals,
                        return_type,
                        loop_depth,
                        allow_return,
                    )?;
                    if select_flow == BlockFlow::AlwaysReturns {
                        flow = BlockFlow::AlwaysReturns;
                        break;
                    }
                }
                Stmt::While(while_stmt) => {
                    let condition_ty = self.type_of_expr(&while_stmt.condition, locals)?;
                    if condition_ty != Type::named("bool") {
                        return Err(Diagnostic::at(
                            while_stmt.span,
                            format!(
                                "`while` condition must have type `bool`, found `{}`",
                                condition_ty
                            ),
                        ));
                    }
                    let mut body_locals = locals.clone();
                    self.check_block(
                        &while_stmt.body,
                        &mut body_locals,
                        return_type,
                        loop_depth + 1,
                        allow_return,
                    )?;
                    self.reject_loop_carried_moves(locals, &body_locals, "while", while_stmt.span)?;
                    self.merge_control_flow_moves(locals, &[&body_locals]);
                }
                Stmt::Break(break_stmt) => {
                    if loop_depth == 0 {
                        return Err(Diagnostic::at(
                            break_stmt.span,
                            "`break` is only allowed inside a loop",
                        ));
                    }
                }
                Stmt::Continue(continue_stmt) => {
                    if loop_depth == 0 {
                        return Err(Diagnostic::at(
                            continue_stmt.span,
                            "`continue` is only allowed inside a loop",
                        ));
                    }
                }
            }
        }

        Ok(flow)
    }

    fn check_with(
        &self,
        with_stmt: &WithStmt,
        locals: &mut HashMap<String, LocalBinding>,
        return_type: &Type,
        loop_depth: usize,
        allow_return: bool,
    ) -> Result<BlockFlow> {
        if locals.contains_key(&with_stmt.binding) {
            return Err(Diagnostic::at(
                with_stmt.span,
                format!(
                    "with binding `{}` would shadow an existing name",
                    with_stmt.binding
                ),
            ));
        }

        let value_ty = self.type_of_expr(&with_stmt.value, locals)?;
        self.require_with_resource(&value_ty, with_stmt.span)?;
        self.consume_value_expr(&with_stmt.value, locals)?;

        let mut body_locals = locals.clone();
        body_locals.insert(
            with_stmt.binding.clone(),
            LocalBinding {
                ty: value_ty,
                assignable: true,
                mutable_place: true,
                passing: ReceiverKind::Value,
                moved: false,
            },
        );
        self.check_block(
            &with_stmt.body,
            &mut body_locals,
            return_type,
            loop_depth,
            allow_return,
        )
    }

    fn check_select(
        &self,
        select_stmt: &SelectStmt,
        locals: &mut HashMap<String, LocalBinding>,
        return_type: &Type,
        loop_depth: usize,
        allow_return: bool,
    ) -> Result<BlockFlow> {
        if select_stmt.arms.is_empty() {
            return Err(Diagnostic::at(
                select_stmt.span,
                "`select` requires at least one `case` arm",
            ));
        }

        let mut all_return = true;
        for arm in &select_stmt.arms {
            let binding_ty = self.select_arm_binding_type(&arm.expr, locals)?;
            match (&arm.binding, binding_ty) {
                (Some(_), None) => {
                    return Err(Diagnostic::at(
                        arm.span,
                        "`after(...)` select arms cannot bind a value",
                    ));
                }
                (None, _) => {}
                (Some(binding), Some(ty)) => {
                    let mut arm_locals = locals.clone();
                    if arm_locals.contains_key(binding) {
                        return Err(Diagnostic::at(
                            arm.span,
                            format!("select binding `{}` would shadow an existing name", binding),
                        ));
                    }
                    arm_locals.insert(
                        binding.clone(),
                        LocalBinding {
                            ty,
                            assignable: false,
                            mutable_place: false,
                            passing: ReceiverKind::Value,
                            moved: false,
                        },
                    );
                    let arm_flow = self.check_block(
                        &arm.body,
                        &mut arm_locals,
                        return_type,
                        loop_depth,
                        allow_return,
                    )?;
                    if arm_flow != BlockFlow::AlwaysReturns {
                        all_return = false;
                    }
                    continue;
                }
            }

            let mut arm_locals = locals.clone();
            let arm_flow = self.check_block(
                &arm.body,
                &mut arm_locals,
                return_type,
                loop_depth,
                allow_return,
            )?;
            if arm_flow != BlockFlow::AlwaysReturns {
                all_return = false;
            }
        }

        if all_return {
            Ok(BlockFlow::AlwaysReturns)
        } else {
            Ok(BlockFlow::FallsThrough)
        }
    }

    fn select_arm_binding_type(
        &self,
        expr: &Expr,
        locals: &mut HashMap<String, LocalBinding>,
    ) -> Result<Option<Type>> {
        let ExprKind::Call { callee, args } = &expr.kind else {
            return Err(Diagnostic::at(
                expr.span,
                "`select` currently supports `recv()`, `send(...)`, and `after(...)` arms",
            ));
        };

        match &callee.kind {
            ExprKind::Name(name) if name == "after" => {
                let ordered_args = BuiltinFunction::After.bind_args(args, expr.span)?;
                let duration_arg = ordered_args[0].expect("`after` requires exactly one argument");
                let duration_ty = self.type_of_expr(&duration_arg.value, locals)?;
                if duration_ty != Type::named("Duration") {
                    return Err(Diagnostic::at(
                        duration_arg.span,
                        format!("`after(...)` expects a `Duration`, found `{}`", duration_ty),
                    ));
                }
                Ok(None)
            }
            ExprKind::Member { object, field } if field == "recv" => {
                BuiltinMember::ChannelRecv.bind_args(args, expr.span)?;
                let receiver_ty = self.type_of_expr(object, locals)?;
                let Type::Named(name, type_args) = receiver_ty else {
                    return Err(Diagnostic::at(
                        expr.span,
                        "`select` receive arms require `Channel[T].recv()`",
                    ));
                };
                if name != "Channel" || type_args.len() != 1 {
                    return Err(Diagnostic::at(
                        expr.span,
                        "`select` receive arms require `Channel[T].recv()`",
                    ));
                }
                Ok(Some(Type::Named(
                    "Option".to_string(),
                    vec![type_args[0].clone()],
                )))
            }
            ExprKind::Member { object, field } if field == "send" => {
                let ordered_args = BuiltinMember::ChannelSend.bind_args(args, expr.span)?;
                let send_arg = ordered_args[0].expect("`send` requires exactly one argument");
                let receiver_ty = self.type_of_expr(object, locals)?;
                let Type::Named(name, type_args) = receiver_ty else {
                    return Err(Diagnostic::at(
                        expr.span,
                        "`select` send arms require `Channel[T].send(value)`",
                    ));
                };
                if name != "Channel" || type_args.len() != 1 {
                    return Err(Diagnostic::at(
                        expr.span,
                        "`select` send arms require `Channel[T].send(value)`",
                    ));
                }
                let actual =
                    self.type_of_expr_hint(&send_arg.value, locals, Some(&type_args[0]))?;
                if actual != type_args[0] {
                    return Err(Diagnostic::at(
                        send_arg.span,
                        format!("`send()` expects `{}`, found `{}`", type_args[0], actual),
                    ));
                }
                Ok(Some(Type::Named(
                    "Result".to_string(),
                    vec![
                        Type::Unit,
                        Type::Named("SendError".to_string(), vec![type_args[0].clone()]),
                    ],
                )))
            }
            _ => Err(Diagnostic::at(
                expr.span,
                "`select` currently supports `recv()`, `send(...)`, and `after(...)` arms",
            )),
        }
    }

    fn check_assign(
        &self,
        assign: &AssignStmt,
        locals: &mut HashMap<String, LocalBinding>,
    ) -> Result<()> {
        if let AssignTarget::Member { object, field } = &assign.target {
            if assign.mutable {
                return Err(Diagnostic::at(
                    assign.span,
                    "`mut` can only be used when introducing a new binding",
                ));
            }

            if assign.annotation.is_some() {
                return Err(Diagnostic::at(
                    assign.span,
                    "member assignment cannot include a type annotation",
                ));
            }

            if !self.is_mutable_place(object, locals)? {
                return Err(Diagnostic::at(
                    assign.span,
                    format!(
                        "cannot assign through immutable place `{}`",
                        self.render_member_target(object, field)
                    ),
                ));
            }

            let target_ty = self.resolve_member_target_type(object, field, assign.span, locals)?;
            let value_ty = self.type_of_expr_hint(&assign.value, locals, Some(&target_ty))?;
            let final_value_ty = if let Some(op) = assign.op {
                self.type_of_binary(assign.span, op, target_ty.clone(), value_ty.clone())?
            } else {
                value_ty
            };

            if final_value_ty != target_ty {
                return Err(Diagnostic::at(
                    assign.span,
                    format!(
                        "cannot assign value of type `{}` to member `{}` of type `{}`",
                        final_value_ty,
                        self.render_member_target(object, field),
                        target_ty
                    ),
                ));
            }

            return Ok(());
        }

        let binding_name = match &assign.target {
            AssignTarget::Name(name) => name,
            AssignTarget::Member { .. } => unreachable!("handled above"),
        };
        let annotation_ty = assign
            .annotation
            .as_ref()
            .map(|annotation| {
                lower_type(
                    annotation,
                    self.type_names,
                    self.type_arities,
                    &self.type_params,
                )
            })
            .transpose()?;
        let existing_ty = locals.get(binding_name).map(|binding| binding.ty.clone());
        let value_ty = self.type_of_expr_hint(
            &assign.value,
            locals,
            existing_ty.as_ref().or(annotation_ty.as_ref()),
        )?;

        if let Some(existing) = locals.get(binding_name).cloned() {
            if assign.mutable {
                return Err(Diagnostic::at(
                    assign.span,
                    format!(
                        "`{}` is already declared; `mut` cannot redeclare an existing binding",
                        binding_name
                    ),
                ));
            }

            if assign.annotation.is_some() && assign.op.is_some() {
                return Err(Diagnostic::at(
                    assign.span,
                    format!(
                        "compound assignment to `{}` cannot include a type annotation",
                        binding_name
                    ),
                ));
            }

            if !existing.assignable {
                return Err(Diagnostic::at(
                    assign.span,
                    format!("cannot assign to immutable binding `{}`", binding_name),
                ));
            }

            if existing.moved && assign.op.is_some() {
                return Err(Diagnostic::at(
                    assign.span,
                    format!(
                        "cannot read moved value `{}` in compound assignment",
                        binding_name
                    ),
                ));
            }

            if let Some(annotation_ty) = annotation_ty {
                if annotation_ty != existing.ty {
                    return Err(Diagnostic::at(
                        assign.span,
                        format!(
                            "reassignment annotation for `{}` has type `{}`, expected `{}`",
                            binding_name, annotation_ty, existing.ty
                        ),
                    ));
                }
            }

            let final_value_ty = if let Some(op) = assign.op {
                self.type_of_binary(assign.span, op, existing.ty.clone(), value_ty.clone())?
            } else {
                value_ty.clone()
            };

            if final_value_ty != existing.ty {
                return Err(Diagnostic::at(
                    assign.span,
                    format!(
                        "cannot assign value of type `{}` to `{}` of type `{}`",
                        final_value_ty, binding_name, existing.ty
                    ),
                ));
            }

            self.consume_value_expr(&assign.value, locals)?;
            if let Some(existing) = locals.get_mut(binding_name) {
                existing.moved = false;
            }
            return Ok(());
        }

        if assign.op.is_some() {
            return Err(Diagnostic::at(
                assign.span,
                format!(
                    "compound assignment requires an existing mutable binding `{}`",
                    binding_name
                ),
            ));
        }

        let final_ty = annotation_ty.unwrap_or_else(|| value_ty.clone());
        if value_ty != final_ty {
            return Err(Diagnostic::at(
                assign.span,
                format!(
                    "binding `{}` has annotated type `{}`, but value has type `{}`",
                    binding_name, final_ty, value_ty
                ),
            ));
        }

        self.consume_value_expr(&assign.value, locals)?;
        locals.insert(
            binding_name.clone(),
            LocalBinding {
                ty: final_ty,
                assignable: assign.mutable,
                mutable_place: assign.mutable,
                passing: ReceiverKind::Value,
                moved: false,
            },
        );
        Ok(())
    }

    fn type_of_expr(
        &self,
        expr: &Expr,
        locals: &mut HashMap<String, LocalBinding>,
    ) -> Result<Type> {
        self.type_of_expr_hint(expr, locals, None)
    }

    fn type_of_expr_hint(
        &self,
        expr: &Expr,
        locals: &mut HashMap<String, LocalBinding>,
        expected: Option<&Type>,
    ) -> Result<Type> {
        match &expr.kind {
            ExprKind::Name(name) if name == "None" => {
                if let Some(expected_ty) = expected {
                    if matches!(expected_ty, Type::Named(enum_name, args) if enum_name == "Option" && args.len() == 1)
                    {
                        return Ok(expected_ty.clone());
                    }
                }
                Ok(Type::Unit)
            }
            ExprKind::Name(name) => {
                if let Some(binding) = locals.get(name) {
                    if binding.moved {
                        return Err(Diagnostic::at(
                            expr.span,
                            format!("use of moved value `{}`", name),
                        ));
                    }
                    return Ok(binding.ty.clone());
                }
                if let Some(function) = self.resolve_function_info(name) {
                    return Ok(function.signature.return_type.clone());
                }
                if let Some(class_info) = self.resolve_class_info(name) {
                    return Ok(Type::named(class_info.decl.name.clone()));
                }
                if let Some(enum_info) = self.resolve_enum_info(name) {
                    return Ok(Type::named(enum_info.decl.name.clone()));
                }
                Err(Diagnostic::at(
                    expr.span,
                    format!("unknown name `{}`", name),
                ))
            }
            ExprKind::Int(value) => {
                let target_ty = expected
                    .filter(|ty| is_integer_type(ty))
                    .cloned()
                    .unwrap_or_else(|| Type::named("int32"));
                self.validate_integer_literal(*value, &target_ty, expr.span)?;
                Ok(target_ty)
            }
            ExprKind::DurationMillis(_) => Ok(Type::named("Duration")),
            ExprKind::Float(_) => Ok(expected
                .filter(|ty| is_float_type(ty))
                .cloned()
                .unwrap_or_else(|| Type::named("float64"))),
            ExprKind::Bool(_) => Ok(Type::named("bool")),
            ExprKind::String(_) => Ok(Type::named("String")),
            ExprKind::FString(parts) => {
                for part in parts {
                    if let crate::ast::FormatPart::Expr(expr) = part {
                        self.type_of_expr(expr, locals)?;
                    }
                }
                Ok(Type::named("String"))
            }
            ExprKind::Group(inner) => self.type_of_expr_hint(inner, locals, expected),
            ExprKind::Specialize {
                expr: base,
                type_args,
            } => {
                let lowered = self.lower_explicit_type_args(type_args)?;
                match &base.kind {
                    ExprKind::Name(name) if self.resolve_class_info(name).is_some() => {
                        let class = self.resolve_class_info(name).unwrap();
                        if lowered.len() != class.decl.type_params.len() {
                            return Err(Diagnostic::at(
                                expr.span,
                                format!(
                                    "class `{}` expects {} type argument{}, found {}",
                                    name,
                                    class.decl.type_params.len(),
                                    if class.decl.type_params.len() == 1 {
                                        ""
                                    } else {
                                        "s"
                                    },
                                    lowered.len()
                                ),
                            ));
                        }
                        Ok(Type::Named(name.clone(), lowered))
                    }
                    ExprKind::Name(name) if self.resolve_enum_info(name).is_some() => {
                        let enum_info = self.resolve_enum_info(name).unwrap();
                        if lowered.len() != enum_info.decl.type_params.len() {
                            return Err(Diagnostic::at(
                                expr.span,
                                format!(
                                    "enum `{}` expects {} type argument{}, found {}",
                                    name,
                                    enum_info.decl.type_params.len(),
                                    if enum_info.decl.type_params.len() == 1 {
                                        ""
                                    } else {
                                        "s"
                                    },
                                    lowered.len()
                                ),
                            ));
                        }
                        Ok(Type::Named(name.clone(), lowered))
                    }
                    _ => self.type_of_expr_hint(base, locals, expected),
                }
            }
            ExprKind::Cast { expr: value, ty } => {
                let target_ty =
                    lower_type(ty, self.type_names, self.type_arities, &self.type_params)?;
                let source_ty = self.type_of_expr_hint(value, locals, Some(&target_ty))?;
                if !is_numeric_type(&source_ty) || !is_numeric_type(&target_ty) {
                    return Err(Diagnostic::at(
                        expr.span,
                        format!(
                            "casts are only supported between numeric types, found `{}` and `{}`",
                            source_ty, target_ty
                        ),
                    ));
                }
                Ok(target_ty)
            }
            ExprKind::Unary { op, expr: value } => match op {
                UnaryOp::Not => {
                    let value_ty = self.type_of_expr(value, locals)?;
                    if value_ty != Type::named("bool") {
                        return Err(Diagnostic::at(
                            expr.span,
                            format!("`not` expects `bool`, found `{}`", value_ty),
                        ));
                    }
                    Ok(Type::named("bool"))
                }
                UnaryOp::Neg => {
                    let value_ty = match &value.kind {
                        ExprKind::Int(inner) => {
                            let target_ty = expected
                                .filter(|ty| is_integer_type(ty))
                                .cloned()
                                .unwrap_or_else(|| Type::named("int32"));
                            self.validate_negative_integer_literal(*inner, &target_ty, expr.span)?;
                            target_ty
                        }
                        _ => self.type_of_expr_hint(value, locals, expected)?,
                    };
                    if is_integer_type(&value_ty) || is_float_type(&value_ty) {
                        Ok(value_ty)
                    } else {
                        Err(Diagnostic::at(
                            expr.span,
                            format!("unary `-` expects a numeric value, found `{}`", value_ty),
                        ))
                    }
                }
            },
            ExprKind::Spawn { detached, value } => {
                let ExprKind::Call { callee, args } = &value.kind else {
                    return Err(Diagnostic::at(
                        expr.span,
                        "`spawn` requires a function or method call expression",
                    ));
                };
                if let ExprKind::Name(function_name) = &callee.kind {
                    if let Some(function) = self.functions.get(function_name) {
                        self.require_spawnable_function(
                            function_name,
                            &function.decl.params,
                            callee.span,
                        )?;
                    }
                }
                let return_ty = self.type_of_call(callee, args, value.span, locals, None)?;
                if *detached {
                    Ok(Type::Unit)
                } else {
                    Ok(Type::Named("Task".to_string(), vec![return_ty]))
                }
            }
            ExprKind::Try(inner) => {
                let current_return_type = self.current_return_type.as_ref().ok_or_else(|| {
                    Diagnostic::at(expr.span, "`try` is only allowed inside a function body")
                })?;
                let inner_ty = self.type_of_expr(inner, locals)?;
                let Type::Named(inner_name, inner_args) = &inner_ty else {
                    return Err(Diagnostic::at(
                        expr.span,
                        format!("`try` requires a `Result[T, E]`, found `{}`", inner_ty),
                    ));
                };
                if inner_name != "Result" || inner_args.len() != 2 {
                    return Err(Diagnostic::at(
                        expr.span,
                        format!("`try` requires a `Result[T, E]`, found `{}`", inner_ty),
                    ));
                }

                let Type::Named(return_name, return_args) = current_return_type else {
                    return Err(Diagnostic::at(
                        expr.span,
                        format!(
                            "`try` requires the enclosing function to return `Result`, found `{}`",
                            current_return_type
                        ),
                    ));
                };
                if return_name != "Result" || return_args.len() != 2 {
                    return Err(Diagnostic::at(
                        expr.span,
                        format!(
                            "`try` requires the enclosing function to return `Result`, found `{}`",
                            current_return_type
                        ),
                    ));
                }

                if inner_args[1] != return_args[1] {
                    return Err(Diagnostic::at(
                        expr.span,
                        format!(
                            "`try` error type `{}` does not match enclosing `Result` error type `{}`",
                            inner_args[1], return_args[1]
                        ),
                    ));
                }

                Ok(inner_args[0].clone())
            }
            ExprKind::Binary { op, left, right } => {
                if matches!(op, BinaryOp::And | BinaryOp::Or) {
                    let left_ty = self.type_of_expr(left, locals)?;
                    let right_ty = self.type_of_expr(right, locals)?;
                    return self.type_of_binary(expr.span, *op, left_ty, right_ty);
                }
                let operand_expected = match op {
                    BinaryOp::Eq
                    | BinaryOp::NotEq
                    | BinaryOp::Less
                    | BinaryOp::LessEq
                    | BinaryOp::Greater
                    | BinaryOp::GreaterEq => None,
                    _ => expected,
                };
                let mut left_ty = self.type_of_expr_hint(left, locals, operand_expected)?;
                let mut right_ty = self.type_of_expr_hint(right, locals, Some(&left_ty))?;
                if left_ty != right_ty && matches!(left.kind, ExprKind::Int(_) | ExprKind::Float(_))
                {
                    left_ty = self.type_of_expr_hint(left, locals, Some(&right_ty))?;
                }
                if left_ty != right_ty
                    && matches!(right.kind, ExprKind::Int(_) | ExprKind::Float(_))
                {
                    right_ty = self.type_of_expr_hint(right, locals, Some(&left_ty))?;
                }
                self.type_of_binary(expr.span, *op, left_ty, right_ty)
            }
            ExprKind::Member { object, field } => {
                if let ExprKind::Specialize {
                    expr: inner,
                    type_args,
                } = &object.kind
                {
                    if let ExprKind::Name(enum_name) = &inner.kind {
                        if let Some(enum_info) = self.resolve_enum_info(enum_name) {
                            let explicit_args = self.lower_explicit_type_args(type_args)?;
                            if explicit_args.len() != enum_info.decl.type_params.len() {
                                return Err(Diagnostic::at(
                                    expr.span,
                                    format!(
                                        "enum `{}` expects {} type argument{}, found {}",
                                        enum_name,
                                        enum_info.decl.type_params.len(),
                                        if enum_info.decl.type_params.len() == 1 {
                                            ""
                                        } else {
                                            "s"
                                        },
                                        explicit_args.len()
                                    ),
                                ));
                            }
                            let variant = enum_info.variants.get(field).ok_or_else(|| {
                                Diagnostic::at(
                                    expr.span,
                                    format!("enum `{}` has no variant `{}`", enum_name, field),
                                )
                            })?;
                            if variant.payload.is_some() {
                                return Err(Diagnostic::at(
                                    expr.span,
                                    format!(
                                        "variant `{}` of enum `{}` requires a payload",
                                        field, enum_name
                                    ),
                                ));
                            }
                            return Ok(Type::Named(enum_info.decl.name.clone(), explicit_args));
                        }
                    }
                }
                if let Some((module_path, enum_name)) = self.qualified_module_item(object) {
                    if let Some(namespace) = self.module_namespace(&module_path) {
                        if let Some(enum_info) = namespace.enums.get(&enum_name) {
                            let variant = enum_info.variants.get(field).ok_or_else(|| {
                                Diagnostic::at(
                                    expr.span,
                                    format!("enum `{}` has no variant `{}`", enum_name, field),
                                )
                            })?;
                            if variant.payload.is_some() {
                                return Err(Diagnostic::at(
                                    expr.span,
                                    format!(
                                        "variant `{}` of enum `{}` requires a payload",
                                        field, enum_name
                                    ),
                                ));
                            }
                            return Ok(Type::named(enum_info.decl.name.clone()));
                        }
                    }
                }
                if let ExprKind::Name(enum_name) = &object.kind {
                    if let Some(expected_ty) = expected {
                        if let Some(payload_ty) =
                            self.builtin_enum_variant_payload(expected_ty, enum_name, field)
                        {
                            if payload_ty.is_some() {
                                return Err(Diagnostic::at(
                                    expr.span,
                                    format!(
                                        "variant `{}` of enum `{}` requires a payload",
                                        field, enum_name
                                    ),
                                ));
                            }
                            return Ok(expected_ty.clone());
                        }
                    }
                    if let Some(enum_info) = self.resolve_enum_info(enum_name) {
                        let variant = enum_info.variants.get(field).ok_or_else(|| {
                            Diagnostic::at(
                                expr.span,
                                format!("enum `{}` has no variant `{}`", enum_name, field),
                            )
                        })?;
                        if variant.payload.is_some() {
                            return Err(Diagnostic::at(
                                expr.span,
                                format!(
                                    "variant `{}` of enum `{}` requires a payload",
                                    field, enum_name
                                ),
                            ));
                        }
                        if let Some(Type::Named(expected_name, _)) = expected {
                            if expected_name == enum_name {
                                return Ok(expected.unwrap().clone());
                            }
                        }
                        if enum_info.decl.type_params.is_empty() {
                            return Ok(Type::named(enum_name));
                        }
                        let missing = enum_info
                            .decl
                            .type_params
                            .first()
                            .cloned()
                            .unwrap_or_else(|| "T".to_string());
                        return Err(Diagnostic::at(
                            expr.span,
                            format!(
                                "cannot infer type parameter `{}` for enum variant `{}.{}`",
                                missing, enum_name, field
                            ),
                        ));
                    }
                }
                let object_ty = self.type_of_expr(object, locals)?;
                let member_ty = self.resolve_member_type(&object_ty, field, expr.span)?;
                Ok(member_ty)
            }
            ExprKind::Call { callee, args } => {
                self.type_of_call(callee, args, expr.span, locals, expected)
            }
        }
    }

    fn type_of_binary(
        &self,
        span: crate::diag::Span,
        op: BinaryOp,
        left_ty: Type,
        right_ty: Type,
    ) -> Result<Type> {
        if left_ty != right_ty {
            return Err(Diagnostic::at(
                span,
                format!(
                    "binary operator operands must match, found `{}` and `{}`",
                    left_ty, right_ty
                ),
            ));
        }

        match (op, &left_ty) {
            (BinaryOp::And | BinaryOp::Or, Type::Named(name, args))
                if args.is_empty() && name == "bool" =>
            {
                Ok(Type::named("bool"))
            }
            (BinaryOp::Add, Type::Named(name, args))
                if args.is_empty()
                    && (is_integer_type(&left_ty)
                        || is_float_type(&left_ty)
                        || name == "String") =>
            {
                Ok(left_ty)
            }
            (BinaryOp::Sub | BinaryOp::Mul | BinaryOp::Div | BinaryOp::Mod, _)
                if is_integer_type(&left_ty) || is_float_type(&left_ty) =>
            {
                Ok(left_ty)
            }
            (BinaryOp::Eq | BinaryOp::NotEq, Type::Named(name, args))
                if (args.is_empty()
                    && (is_integer_type(&left_ty)
                        || is_float_type(&left_ty)
                        || matches!(name.as_str(), "bool" | "String")))
                    || self.enum_variants_for_type(&left_ty).is_some() =>
            {
                Ok(Type::named("bool"))
            }
            (BinaryOp::Less | BinaryOp::LessEq | BinaryOp::Greater | BinaryOp::GreaterEq, _)
                if is_integer_type(&left_ty) || is_float_type(&left_ty) =>
            {
                Ok(Type::named("bool"))
            }
            _ => Err(Diagnostic::at(
                span,
                format!("unsupported operands for binary expression: `{}`", left_ty),
            )),
        }
    }

    fn type_of_call(
        &self,
        callee: &Expr,
        args: &[Argument],
        span: crate::diag::Span,
        locals: &mut HashMap<String, LocalBinding>,
        expected: Option<&Type>,
    ) -> Result<Type> {
        let (base_callee, explicit_type_args) = self.peel_specialization(callee);

        if let (ExprKind::Name(name), Some(type_args)) = (&base_callee.kind, explicit_type_args) {
            if name == "Channel" {
                let explicit_args = self.lower_explicit_type_args(type_args)?;
                if explicit_args.len() != 1 {
                    return Err(Diagnostic::at(
                        span,
                        format!(
                            "class `{}` expects exactly one type argument, found {}",
                            name,
                            explicit_args.len()
                        ),
                    ));
                }
                let capacity_params = [crate::call::CallableParam::optional("capacity")];
                let ordered_args = bind_call_arguments(
                    "class `Channel`",
                    &capacity_params,
                    args,
                    span,
                    CallConvention::PositionalOrNamed,
                )?;
                if let Some(capacity_arg) = ordered_args[0] {
                    let actual = self.type_of_expr_hint(
                        &capacity_arg.value,
                        locals,
                        Some(&Type::named("int32")),
                    )?;
                    if actual != Type::named("int32") {
                        return Err(Diagnostic::at(
                            capacity_arg.span,
                            format!("field `capacity` expects `int32`, found `{}`", actual),
                        ));
                    }
                }
                return Ok(Type::Named("Channel".to_string(), explicit_args));
            }
        }

        match &base_callee.kind {
            ExprKind::Name(name) if BuiltinFunction::from_name(name).is_some() => {
                let builtin = BuiltinFunction::from_name(name).unwrap();
                let ordered_args = builtin.bind_args(args, span)?;
                match builtin {
                    BuiltinFunction::Print => {
                        self.type_of_expr(
                            &ordered_args[0]
                                .expect("`print` requires exactly one argument")
                                .value,
                            locals,
                        )?;
                        Ok(Type::Unit)
                    }
                    BuiltinFunction::Range => {
                        for argument in ordered_args.into_iter().flatten() {
                            let actual = self.type_of_expr(&argument.value, locals)?;
                            if actual != Type::named("int32") {
                                return Err(Diagnostic::at(
                                    argument.span,
                                    format!(
                                        "`range` arguments must have type `int32`, found `{}`",
                                        actual
                                    ),
                                ));
                            }
                        }
                        Ok(Type::named("Range"))
                    }
                    BuiltinFunction::Channel => {
                        let Some(expected_ty) = expected else {
                            return Err(Diagnostic::at(
                                span,
                                "`channel()` requires an expected `Channel[T]` type annotation in the bootstrap compiler",
                            ));
                        };
                        let Type::Named(name, args) = expected_ty else {
                            return Err(Diagnostic::at(
                                span,
                                "`channel()` requires an expected `Channel[T]` type annotation in the bootstrap compiler",
                            ));
                        };
                        if name != "Channel" || args.len() != 1 {
                            return Err(Diagnostic::at(
                                span,
                                "`channel()` requires an expected `Channel[T]` type annotation in the bootstrap compiler",
                            ));
                        }
                        Ok(expected_ty.clone())
                    }
                    BuiltinFunction::TaskGroup => Ok(Type::named("TaskGroup")),
                    BuiltinFunction::Cancelled => Ok(Type::named("bool")),
                    BuiltinFunction::After => {
                        let duration_arg =
                            ordered_args[0].expect("`after` requires exactly one argument");
                        let duration_ty = self.type_of_expr(&duration_arg.value, locals)?;
                        if duration_ty != Type::named("Duration") {
                            return Err(Diagnostic::at(
                                duration_arg.span,
                                format!(
                                    "`after(...)` expects a `Duration`, found `{}`",
                                    duration_ty
                                ),
                            ));
                        }
                        Ok(Type::named("Duration"))
                    }
                    BuiltinFunction::Sleep => {
                        let duration_arg =
                            ordered_args[0].expect("`sleep` requires exactly one argument");
                        let duration_ty = self.type_of_expr(&duration_arg.value, locals)?;
                        if duration_ty != Type::named("Duration") {
                            return Err(Diagnostic::at(
                                duration_arg.span,
                                format!(
                                    "`sleep(...)` expects a `Duration`, found `{}`",
                                    duration_ty
                                ),
                            ));
                        }
                        Ok(Type::Unit)
                    }
                }
            }
            ExprKind::Name(name) if self.resolve_function_info(name).is_some() => {
                let function = self.resolve_function_info(name).unwrap();
                let seed_substitutions = if let Some(type_args) = explicit_type_args {
                    self.explicit_type_substitutions(
                        &function.decl.type_params,
                        type_args,
                        span,
                        &format!("function `{}`", name),
                    )?
                } else {
                    HashMap::new()
                };
                self.type_check_callable_args(
                    &format!("function `{}`", name),
                    &function.decl.type_params,
                    &function.decl.params,
                    &function.signature.params,
                    &function.signature.return_type,
                    &function.type_param_bounds,
                    args,
                    span,
                    locals,
                    expected,
                    seed_substitutions,
                )
            }
            ExprKind::Name(name) if self.resolve_class_info(name).is_some() => {
                let class = self.resolve_class_info(name).unwrap();
                if args.iter().any(|argument| argument.name.is_none()) {
                    return Err(Diagnostic::at(
                        span,
                        format!(
                            "class constructor `{}` currently requires keyword arguments",
                            name
                        ),
                    ));
                }

                let mut provided = HashMap::new();
                let mut substitutions = if let Some(type_args) = explicit_type_args {
                    self.explicit_type_substitutions(
                        &class.decl.type_params,
                        type_args,
                        span,
                        &format!("class constructor `{}`", name),
                    )?
                } else {
                    match expected {
                        Some(Type::Named(expected_name, expected_args))
                            if expected_name == name
                                && expected_args.len() == class.decl.type_params.len() =>
                        {
                            substitutions_from_decl_type_args(
                                &class.decl.type_params,
                                expected_args,
                            )
                        }
                        _ => HashMap::new(),
                    }
                };
                for argument in args {
                    let field_name = argument.name.as_ref().unwrap();
                    let Some(field_info) = class.fields.get(field_name) else {
                        return Err(Diagnostic::at(
                            argument.span,
                            format!("class `{}` has no field named `{}`", name, field_name),
                        ));
                    };
                    if self.is_external_module(&class.module_name) && !field_info.public {
                        return Err(Diagnostic::at(
                            argument.span,
                            format!("field `{}` is private on `{}`", field_name, class.decl.name),
                        ));
                    }
                    if provided.insert(field_name.clone(), ()).is_some() {
                        return Err(Diagnostic::at(
                            argument.span,
                            format!("field `{}` was provided more than once", field_name),
                        ));
                    }

                    let hinted_field_ty = substitute_type(&field_info.ty, &substitutions);
                    let actual = if has_unresolved_type_params(&hinted_field_ty) {
                        self.type_of_expr(&argument.value, locals)?
                    } else {
                        self.type_of_expr_hint(&argument.value, locals, Some(&hinted_field_ty))?
                    };
                    if let Err(error) =
                        unify_type_pattern(&field_info.ty, &actual, &mut substitutions)
                    {
                        return Err(Diagnostic::at(
                            argument.span,
                            format!(
                                "field `{}` expects `{}`, found `{}` ({})",
                                field_name, hinted_field_ty, actual, error.message
                            ),
                        ));
                    }
                    if !self.is_copy_type(&actual) {
                        self.consume_value_expr(&argument.value, locals)?;
                    }
                }

                for field in &class.decl.fields {
                    if !provided.contains_key(&field.name) && field.default.is_none() {
                        if self.is_external_module(&class.module_name) && !field.public {
                            return Err(Diagnostic::at(
                                span,
                                format!(
                                    "class constructor `{}` cannot initialize private field `{}` from another module",
                                    class.decl.name, field.name
                                ),
                            ));
                        }
                        return Err(Diagnostic::at(
                            span,
                            format!(
                                "class constructor `{}` is missing required field `{}`",
                                name, field.name
                            ),
                        ));
                    }
                }

                let resolved_args = class
                    .decl
                    .type_params
                    .iter()
                    .map(|type_param| {
                        substitutions.get(type_param).cloned().ok_or_else(|| {
                            Diagnostic::at(
                                span,
                                format!(
                                    "cannot infer type parameter `{}` for class constructor `{}`",
                                    type_param, name
                                ),
                            )
                        })
                    })
                    .collect::<Result<Vec<_>>>()?;
                for (type_param, bounds) in &class.type_param_bounds {
                    let resolved_ty = resolved_args[class
                        .decl
                        .type_params
                        .iter()
                        .position(|name| name == type_param)
                        .expect("class type parameter should exist")]
                    .clone();
                    self.assert_type_satisfies_bounds(&resolved_ty, bounds, span)?;
                }

                Ok(Type::Named(name.clone(), resolved_args))
            }
            ExprKind::Member { object, field } => {
                let (base_object, object_type_args) = self.peel_specialization(object);
                if let Some((module_path, item_name)) = self.qualified_module_item(object) {
                    if let Some(namespace) = self.module_namespace(&module_path) {
                        if let Some(class_info) = namespace.classes.get(&item_name) {
                            if let Some(method) = class_info.methods.get(field) {
                                if method.decl.receiver.is_none() {
                                    return self.type_check_callable_args(
                                        &format!("method `{}`", field),
                                        &method.decl.type_params,
                                        &method.decl.params,
                                        &method.signature.params,
                                        &method.signature.return_type,
                                        &method.type_param_bounds,
                                        args,
                                        span,
                                        locals,
                                        expected,
                                        HashMap::new(),
                                    );
                                }
                            }
                        }
                        if let Some(enum_info) = namespace.enums.get(&item_name) {
                            let variant = enum_info.variants.get(field).ok_or_else(|| {
                                Diagnostic::at(
                                    span,
                                    format!("enum `{}` has no variant `{}`", item_name, field),
                                )
                            })?;
                            if args.iter().any(|argument| argument.name.is_some()) {
                                return Err(Diagnostic::at(
                                    span,
                                    "enum variant constructors do not take keyword arguments",
                                ));
                            }
                            match &variant.payload {
                                Some(payload_ty) => {
                                    if args.len() != 1 {
                                        return Err(Diagnostic::at(
                                            span,
                                            format!(
                                                "variant `{}` of enum `{}` expects exactly one payload argument",
                                                field, item_name
                                            ),
                                        ));
                                    }
                                    let actual = self.type_of_expr_hint(
                                        &args[0].value,
                                        locals,
                                        Some(payload_ty),
                                    )?;
                                    if actual != *payload_ty {
                                        return Err(Diagnostic::at(
                                            args[0].span,
                                            format!(
                                                "variant `{}` of enum `{}` expects `{}`, found `{}`",
                                                field, item_name, payload_ty, actual
                                            ),
                                        ));
                                    }
                                    if !self.is_copy_type(&actual) {
                                        self.consume_value_expr(&args[0].value, locals)?;
                                    }
                                }
                                None => {
                                    return Err(Diagnostic::at(
                                        span,
                                        format!(
                                            "variant `{}` of enum `{}` does not take a payload",
                                            field, item_name
                                        ),
                                    ));
                                }
                            }
                            return Ok(Type::named(enum_info.decl.name.clone()));
                        }
                    }
                }
                if let ExprKind::Name(class_name) = &base_object.kind {
                    if let Some(class_info) = self.resolve_class_info(class_name) {
                        if let Some(method) = class_info.methods.get(field) {
                            if method.decl.receiver.is_some() {
                                return Err(Diagnostic::at(
                                    span,
                                    format!(
                                        "method `{}` on class `{}` requires an instance receiver",
                                        field, class_name
                                    ),
                                ));
                            }
                            return self.type_check_callable_args(
                                &format!("method `{}`", field),
                                &method.decl.type_params,
                                &method.decl.params,
                                &method.signature.params,
                                &method.signature.return_type,
                                &method.type_param_bounds,
                                args,
                                span,
                                locals,
                                expected,
                                HashMap::new(),
                            );
                        }
                    }
                }

                if let ExprKind::Name(enum_name) = &base_object.kind {
                    if let Some(expected_ty) = expected {
                        if let Some(variant_payload) =
                            self.builtin_enum_variant_payload(expected_ty, enum_name, field)
                        {
                            if args.iter().any(|argument| argument.name.is_some()) {
                                return Err(Diagnostic::at(
                                    span,
                                    "enum variant constructors do not take keyword arguments",
                                ));
                            }
                            match variant_payload {
                                Some(payload_ty) => {
                                    if args.len() != 1 {
                                        return Err(Diagnostic::at(
                                            span,
                                            format!(
                                                "variant `{}` of enum `{}` expects exactly one payload argument",
                                                field, enum_name
                                            ),
                                        ));
                                    }
                                    let actual = self.type_of_expr_hint(
                                        &args[0].value,
                                        locals,
                                        Some(&payload_ty),
                                    )?;
                                    if actual != payload_ty {
                                        return Err(Diagnostic::at(
                                            args[0].span,
                                            format!(
                                                "variant `{}` of enum `{}` expects `{}`, found `{}`",
                                                field, enum_name, payload_ty, actual
                                            ),
                                        ));
                                    }
                                    if !self.is_copy_type(&payload_ty) {
                                        self.consume_value_expr(&args[0].value, locals)?;
                                    }
                                }
                                None => {
                                    return Err(Diagnostic::at(
                                        span,
                                        format!(
                                            "variant `{}` of enum `{}` does not take a payload",
                                            field, enum_name
                                        ),
                                    ));
                                }
                            }
                            return Ok(expected_ty.clone());
                        }
                    }
                    if let Some(enum_info) = self.resolve_enum_info(enum_name) {
                        let variant = enum_info.variants.get(field).ok_or_else(|| {
                            Diagnostic::at(
                                span,
                                format!("enum `{}` has no variant `{}`", enum_name, field),
                            )
                        })?;
                        if args.iter().any(|argument| argument.name.is_some()) {
                            return Err(Diagnostic::at(
                                span,
                                "enum variant constructors do not take keyword arguments",
                            ));
                        }
                        if variant.payload.is_none() && args.is_empty() {
                            if let Some(Type::Named(expected_name, _)) = expected {
                                if expected_name == enum_name {
                                    return Ok(expected.unwrap().clone());
                                }
                            }
                            if enum_info.decl.type_params.is_empty() {
                                return Ok(Type::named(enum_name));
                            }
                            let missing = enum_info
                                .decl
                                .type_params
                                .first()
                                .cloned()
                                .unwrap_or_else(|| "T".to_string());
                            return Err(Diagnostic::at(
                                span,
                                format!(
                                    "cannot infer type parameter `{}` for enum variant `{}.{}`",
                                    missing, enum_name, field
                                ),
                            ));
                        }
                        let mut substitutions = if let Some(type_args) = object_type_args {
                            self.explicit_type_substitutions(
                                &enum_info.decl.type_params,
                                type_args,
                                span,
                                &format!("enum `{}`", enum_name),
                            )?
                        } else {
                            match expected {
                                Some(Type::Named(expected_name, expected_args))
                                    if expected_name == enum_name
                                        && expected_args.len()
                                            == enum_info.decl.type_params.len() =>
                                {
                                    substitutions_from_decl_type_args(
                                        &enum_info.decl.type_params,
                                        expected_args,
                                    )
                                }
                                _ => HashMap::new(),
                            }
                        };
                        match &variant.payload {
                            Some(payload_ty) => {
                                if args.len() != 1 {
                                    return Err(Diagnostic::at(
                                        span,
                                        format!(
                                            "variant `{}` of enum `{}` expects exactly one payload argument",
                                            field, enum_name
                                        ),
                                    ));
                                }
                                let hinted_payload_ty = substitute_type(payload_ty, &substitutions);
                                let actual = if has_unresolved_type_params(&hinted_payload_ty) {
                                    self.type_of_expr(&args[0].value, locals)?
                                } else {
                                    self.type_of_expr_hint(
                                        &args[0].value,
                                        locals,
                                        Some(&hinted_payload_ty),
                                    )?
                                };
                                if let Err(error) =
                                    unify_type_pattern(payload_ty, &actual, &mut substitutions)
                                {
                                    return Err(Diagnostic::at(
                                        args[0].span,
                                        format!(
                                            "variant `{}` of enum `{}` expects `{}`, found `{}` ({})",
                                            field, enum_name, hinted_payload_ty, actual, error.message
                                        ),
                                    ));
                                }
                                if !self.is_copy_type(&actual) {
                                    self.consume_value_expr(&args[0].value, locals)?;
                                }
                            }
                            None => {
                                return Err(Diagnostic::at(
                                    span,
                                    format!(
                                        "variant `{}` of enum `{}` does not take a payload",
                                        field, enum_name
                                    ),
                                ));
                            }
                        }
                        let resolved_args = enum_info
                            .decl
                            .type_params
                            .iter()
                            .map(|type_param| {
                                substitutions.get(type_param).cloned().ok_or_else(|| {
                                    Diagnostic::at(
                                        span,
                                        format!(
                                            "cannot infer type parameter `{}` for enum variant `{}.{}`",
                                            type_param, enum_name, field
                                        ),
                                    )
                                })
                            })
                            .collect::<Result<Vec<_>>>()?;
                        for (type_param, bounds) in &enum_info.type_param_bounds {
                            let resolved_ty = resolved_args[enum_info
                                .decl
                                .type_params
                                .iter()
                                .position(|name| name == type_param)
                                .expect("enum type parameter should exist")]
                            .clone();
                            self.assert_type_satisfies_bounds(&resolved_ty, bounds, span)?;
                        }
                        return Ok(Type::Named(enum_name.clone(), resolved_args));
                    }
                }

                let receiver_ty = self.type_of_expr(object, locals)?;
                if let Type::Module(module_path) = &receiver_ty {
                    let namespace = self.module_namespace(module_path).ok_or_else(|| {
                        Diagnostic::at(span, format!("unknown module namespace `{}`", module_path))
                    })?;
                    if let Some(function) = namespace.functions.get(field) {
                        return self.type_check_callable_args(
                            &format!("function `{}`", function.decl.name),
                            &function.decl.type_params,
                            &function.decl.params,
                            &function.signature.params,
                            &function.signature.return_type,
                            &function.type_param_bounds,
                            args,
                            span,
                            locals,
                            expected,
                            HashMap::new(),
                        );
                    }
                    if let Some(class) = namespace.classes.get(field) {
                        if args.iter().any(|argument| argument.name.is_none()) {
                            return Err(Diagnostic::at(
                                span,
                                format!(
                                    "class constructor `{}` currently requires keyword arguments",
                                    class.decl.name
                                ),
                            ));
                        }

                        let mut provided = HashMap::new();
                        for argument in args {
                            let field_name = argument.name.as_ref().unwrap();
                            let Some(field_info) = class.fields.get(field_name) else {
                                return Err(Diagnostic::at(
                                    argument.span,
                                    format!(
                                        "class `{}` has no field named `{}`",
                                        class.decl.name, field_name
                                    ),
                                ));
                            };
                            if !field_info.public {
                                return Err(Diagnostic::at(
                                    argument.span,
                                    format!(
                                        "field `{}` is private on `{}`",
                                        field_name, class.decl.name
                                    ),
                                ));
                            }
                            if provided.insert(field_name.clone(), ()).is_some() {
                                return Err(Diagnostic::at(
                                    argument.span,
                                    format!("field `{}` was provided more than once", field_name),
                                ));
                            }
                            let actual = self.type_of_expr_hint(
                                &argument.value,
                                locals,
                                Some(&field_info.ty),
                            )?;
                            if actual != field_info.ty {
                                return Err(Diagnostic::at(
                                    argument.span,
                                    format!(
                                        "field `{}` expects `{}`, found `{}`",
                                        field_name, field_info.ty, actual
                                    ),
                                ));
                            }
                            if !self.is_copy_type(&actual) {
                                self.consume_value_expr(&argument.value, locals)?;
                            }
                        }

                        for field in &class.decl.fields {
                            if !provided.contains_key(&field.name) && field.default.is_none() {
                                if !field.public {
                                    return Err(Diagnostic::at(
                                        span,
                                        format!(
                                            "class constructor `{}` cannot initialize private field `{}` from another module",
                                            class.decl.name, field.name
                                        ),
                                    ));
                                }
                                return Err(Diagnostic::at(
                                    span,
                                    format!(
                                        "class constructor `{}` is missing required field `{}`",
                                        class.decl.name, field.name
                                    ),
                                ));
                            }
                        }

                        return Ok(Type::named(class.decl.name.clone()));
                    }
                    return Err(Diagnostic::at(
                        span,
                        format!(
                            "module `{}` has no callable member `{}`",
                            module_path, field
                        ),
                    ));
                }
                if let Type::Named(receiver_name, receiver_args) = &receiver_ty {
                    if receiver_name == "String" && receiver_args.is_empty() {
                        if let Some(builtin_member) = BuiltinMember::resolve(receiver_name, field) {
                            builtin_member.bind_args(args, span)?;
                            return match builtin_member {
                                BuiltinMember::StringClone => Ok(Type::named("String")),
                                _ => unreachable!("unexpected string builtin member"),
                            };
                        }
                    }

                    if receiver_name == "Channel" && receiver_args.len() == 1 {
                        if let Some(builtin_member) = BuiltinMember::resolve(receiver_name, field) {
                            let ordered_args = builtin_member.bind_args(args, span)?;
                            return match builtin_member {
                                BuiltinMember::ChannelClone => Ok(receiver_ty.clone()),
                                BuiltinMember::ChannelSend => {
                                    let send_arg = ordered_args[0]
                                        .expect("`send` requires exactly one argument");
                                    let actual = self.type_of_expr_hint(
                                        &send_arg.value,
                                        locals,
                                        Some(&receiver_args[0]),
                                    )?;
                                    if actual != receiver_args[0] {
                                        return Err(Diagnostic::at(
                                            send_arg.span,
                                            format!(
                                                "`send` expects `{}`, found `{}`",
                                                receiver_args[0], actual
                                            ),
                                        ));
                                    }
                                    if !self.is_copy_type(&receiver_args[0]) {
                                        self.consume_value_expr(&send_arg.value, locals)?;
                                    }
                                    Ok(Type::Named(
                                        "Result".to_string(),
                                        vec![
                                            Type::Unit,
                                            Type::Named(
                                                "SendError".to_string(),
                                                vec![receiver_args[0].clone()],
                                            ),
                                        ],
                                    ))
                                }
                                BuiltinMember::ChannelRecv => Ok(Type::Named(
                                    "Option".to_string(),
                                    vec![receiver_args[0].clone()],
                                )),
                                BuiltinMember::ChannelClose => Ok(Type::Unit),
                                _ => unreachable!("unexpected channel builtin member"),
                            };
                        }
                    }

                    if receiver_name == "Task" && receiver_args.len() == 1 {
                        if let Some(builtin_member) = BuiltinMember::resolve(receiver_name, field) {
                            builtin_member.bind_args(args, span)?;
                            return match builtin_member {
                                BuiltinMember::TaskClone => Ok(receiver_ty.clone()),
                                BuiltinMember::TaskJoin => Ok(receiver_args[0].clone()),
                                _ => unreachable!("unexpected task builtin member"),
                            };
                        }
                    }

                    if receiver_name == "TaskGroup" && receiver_args.is_empty() {
                        match field.as_str() {
                            "spawn" => {
                                if args.is_empty() {
                                    return Err(Diagnostic::at(
                                        span,
                                        "`spawn` expects a target function followed by its arguments",
                                    ));
                                }
                                if args[0].name.is_some() {
                                    return Err(Diagnostic::at(
                                        args[0].span,
                                        "`spawn` does not take keyword arguments",
                                    ));
                                }
                                let ExprKind::Name(function_name) = &args[0].value.kind else {
                                    return Err(Diagnostic::at(
                                        args[0].span,
                                        "`spawn` currently requires a named function target",
                                    ));
                                };
                                let function =
                                    self.functions.get(function_name).ok_or_else(|| {
                                        Diagnostic::at(
                                            args[0].span,
                                            format!("unknown function `{}`", function_name),
                                        )
                                    })?;
                                self.require_spawnable_function(
                                    function_name,
                                    &function.decl.params,
                                    args[0].span,
                                )?;
                                let spawn_args = &args[1..];
                                self.type_check_callable_args(
                                    &format!("function `{}`", function_name),
                                    &function.decl.type_params,
                                    &function.decl.params,
                                    &function.signature.params,
                                    &function.signature.return_type,
                                    &function.type_param_bounds,
                                    spawn_args,
                                    span,
                                    locals,
                                    None,
                                    HashMap::new(),
                                )?;
                                return Ok(Type::Named(
                                    "Task".to_string(),
                                    vec![function.signature.return_type.clone()],
                                ));
                            }
                            "cancel" | "close" => {
                                BuiltinMember::TaskGroupCancel.bind_args(args, span)?;
                                return Ok(Type::Unit);
                            }
                            _ => {}
                        }
                    }
                }

                if let Type::Named(class_name, type_args) = &receiver_ty {
                    if let Some(class_info) = self.resolve_class_info(class_name) {
                        if let Some(method) = class_info.methods.get(field) {
                            if self.is_external_module(&class_info.module_name)
                                && !method.decl.public
                            {
                                return Err(Diagnostic::at(
                                    span,
                                    format!(
                                        "method `{}` is private on `{}`",
                                        field, class_info.decl.name
                                    ),
                                ));
                            }
                            if method.decl.receiver.is_none() {
                                return Err(Diagnostic::at(
                                    span,
                                    format!(
                                        "associated method `{}` on class `{}` must be called through the class name",
                                        field, class_name
                                    ),
                                ));
                            }
                            if method.decl.receiver == Some(ReceiverKind::BorrowMut)
                                && !self.is_mutable_place(object, locals)?
                            {
                                return Err(Diagnostic::at(
                                    span,
                                    format!("method `{}` requires a mutable receiver", field),
                                ));
                            }
                            if method.decl.receiver == Some(ReceiverKind::Value) {
                                self.consume_value_expr(object, locals)?;
                            }
                            return self.type_check_callable_args(
                                &format!("method `{}`", field),
                                &method.decl.type_params,
                                &method.decl.params,
                                &method.signature.params,
                                &method.signature.return_type,
                                &method.type_param_bounds,
                                args,
                                span,
                                locals,
                                expected,
                                substitutions_from_decl_type_args(
                                    &class_info.decl.type_params,
                                    type_args,
                                ),
                            );
                        }
                    }
                }
                if let Type::TypeParam(type_param_name) = &receiver_ty {
                    if let Ok(method) = self.trait_method_from_type_param(type_param_name, field) {
                        return self.type_check_callable_args(
                            &format!("method `{}`", field),
                            &method.decl.type_params,
                            &method.decl.params,
                            &method.signature.params,
                            &method.signature.return_type,
                            &method.type_param_bounds,
                            args,
                            span,
                            locals,
                            expected,
                            HashMap::new(),
                        );
                    }
                }
                if let Some((_trait_impl, method)) =
                    self.trait_method_for_concrete_type(&receiver_ty, field)
                {
                    return self.type_check_callable_args(
                        &format!("method `{}`", field),
                        &method.decl.type_params,
                        &method.decl.params,
                        &method.signature.params,
                        &method.signature.return_type,
                        &method.type_param_bounds,
                        args,
                        span,
                        locals,
                        expected,
                        HashMap::new(),
                    );
                }
                match (&receiver_ty, field.as_str()) {
                    (Type::Named(name, type_args), "sqrt")
                        if type_args.is_empty() && name == "float64" =>
                    {
                        BuiltinMember::FloatSqrt.bind_args(args, span)?;
                        Ok(Type::named("float64"))
                    }
                    _ => Err(Diagnostic::at(
                        span,
                        format!("unsupported method call `{}` on `{}`", field, receiver_ty),
                    )),
                }
            }
            _ => Err(self.unsupported_call_target_diagnostic(callee, span)),
        }
    }

    fn unsupported_call_target_diagnostic(
        &self,
        callee: &Expr,
        span: crate::diag::Span,
    ) -> Diagnostic {
        let bare_name = match &callee.kind {
            ExprKind::Name(name) => Some(name.as_str()),
            ExprKind::Specialize { expr, .. } => match &expr.kind {
                ExprKind::Name(name) => Some(name.as_str()),
                _ => None,
            },
            _ => None,
        };

        match bare_name {
            Some("String") => {
                Diagnostic::at(span, "strings use quoted literals; `String(...)` is not a constructor")
            }
            Some("Some") => Diagnostic::at(
                span,
                "enum variants are not callable by bare name; use a qualified form such as `Option.Some(...)`",
            ),
            Some("None") => Diagnostic::at(
                span,
                "enum variants are not callable by bare name; use a qualified form such as `Option.None`",
            ),
            Some("Ok") => Diagnostic::at(
                span,
                "enum variants are not callable by bare name; use a qualified form such as `Result.Ok(...)`",
            ),
            Some("Err") => Diagnostic::at(
                span,
                "enum variants are not callable by bare name; use a qualified form such as `Result.Err(...)`",
            ),
            Some("Closed") => Diagnostic::at(
                span,
                "enum variants are not callable by bare name; use a qualified form such as `SendError.Closed(...)`",
            ),
            _ => Diagnostic::at(span, "unsupported call target"),
        }
    }

    fn check_match(
        &self,
        match_stmt: &MatchStmt,
        locals: &mut HashMap<String, LocalBinding>,
        return_type: &Type,
        loop_depth: usize,
        allow_return: bool,
    ) -> Result<BlockFlow> {
        let scrutinee_ty = self.type_of_expr(&match_stmt.scrutinee, locals)?;
        if match_stmt.borrow_mode.is_none() && !self.is_copy_type(&scrutinee_ty) {
            self.consume_value_expr(&match_stmt.scrutinee, locals)?;
        }
        let Type::Named(enum_name, _type_args) = &scrutinee_ty else {
            return Err(Diagnostic::at(
                match_stmt.span,
                format!(
                    "`match` currently requires an enum scrutinee, found `{}`",
                    scrutinee_ty
                ),
            ));
        };

        let Some(variants) = self.enum_variants_for_type(&scrutinee_ty) else {
            return Err(Diagnostic::at(
                match_stmt.span,
                format!(
                    "`match` currently requires an enum scrutinee, found `{}`",
                    scrutinee_ty
                ),
            ));
        };

        if match_stmt.arms.is_empty() {
            return Err(Diagnostic::at(
                match_stmt.span,
                "`match` requires at least one `case` arm",
            ));
        }

        let mut covered = BTreeMap::<String, crate::diag::Span>::new();
        let mut wildcard_span = None;
        let mut all_return = true;

        for (index, arm) in match_stmt.arms.iter().enumerate() {
            let mut arm_locals = locals.clone();
            match &arm.pattern {
                Pattern::Wildcard(span) => {
                    if wildcard_span.is_some() {
                        return Err(Diagnostic::at(*span, "duplicate wildcard match arm"));
                    }
                    if index + 1 != match_stmt.arms.len() {
                        return Err(Diagnostic::at(
                            *span,
                            "wildcard match arm must be the final `case`",
                        ));
                    }
                    wildcard_span = Some(*span);
                }
                Pattern::Variant(pattern) => {
                    let pattern_enum_name = if let Some(pattern_enum_name) = &pattern.enum_name {
                        if pattern_enum_name == enum_name {
                            pattern_enum_name.clone()
                        } else if let Some(pattern_enum_info) =
                            self.resolve_enum_info(pattern_enum_name)
                        {
                            pattern_enum_info.decl.name.clone()
                        } else {
                            return Err(Diagnostic::at(
                                pattern.span,
                                format!("unknown enum `{}` in match pattern", pattern_enum_name),
                            ));
                        }
                    } else {
                        enum_name.clone()
                    };
                    if pattern_enum_name != *enum_name {
                        return Err(Diagnostic::at(
                            pattern.span,
                            format!(
                                "match arm expects enum `{}`, found pattern for `{}`",
                                enum_name, pattern_enum_name
                            ),
                        ));
                    }

                    let Some(variant_payload) = variants
                        .iter()
                        .find(|(name, _)| name == &pattern.variant_name)
                        .map(|(_, payload)| payload.clone())
                    else {
                        return Err(Diagnostic::at(
                            pattern.span,
                            format!(
                                "enum `{}` has no variant `{}`",
                                enum_name, pattern.variant_name
                            ),
                        ));
                    };

                    if let Some(previous) =
                        covered.insert(pattern.variant_name.clone(), pattern.span)
                    {
                        return Err(Diagnostic::at(
                            pattern.span,
                            format!(
                                "duplicate match arm for `{}.{}` (previously matched at {})",
                                enum_name, pattern.variant_name, previous
                            ),
                        ));
                    }

                    match (&variant_payload, &pattern.binding) {
                        (Some(_), None) => {
                            return Err(Diagnostic::at(
                                pattern.span,
                                format!(
                                    "variant `{}.{}` carries a payload and must bind it",
                                    enum_name, pattern.variant_name
                                ),
                            ));
                        }
                        (None, Some(_)) => {
                            return Err(Diagnostic::at(
                                pattern.span,
                                format!(
                                    "variant `{}.{}` does not carry a payload",
                                    enum_name, pattern.variant_name
                                ),
                            ));
                        }
                        _ => {}
                    }

                    if let (Some(payload_ty), Some(binding)) = (&variant_payload, &pattern.binding)
                    {
                        if arm_locals.contains_key(binding) {
                            return Err(Diagnostic::at(
                                pattern.span,
                                format!(
                                    "pattern binding `{}` would shadow an existing name",
                                    binding
                                ),
                            ));
                        }
                        arm_locals.insert(
                            binding.clone(),
                            LocalBinding {
                                ty: payload_ty.clone(),
                                assignable: false,
                                mutable_place: false,
                                passing: if let Some(borrow_mode) = match_stmt.borrow_mode {
                                    if self.is_copy_type(payload_ty) {
                                        ReceiverKind::Value
                                    } else {
                                        borrow_mode
                                    }
                                } else {
                                    ReceiverKind::Value
                                },
                                moved: false,
                            },
                        );
                    }
                }
            }

            let arm_flow = self.check_block(
                &arm.body,
                &mut arm_locals,
                return_type,
                loop_depth,
                allow_return,
            )?;
            if arm_flow != BlockFlow::AlwaysReturns {
                all_return = false;
            }
        }

        let missing = variants
            .iter()
            .filter(|(name, _)| !covered.contains_key(name))
            .map(|(name, _)| name.clone())
            .collect::<Vec<_>>();
        if wildcard_span.is_none() && !missing.is_empty() {
            let rendered = missing
                .iter()
                .map(|name| format!("`{}`", name))
                .collect::<Vec<_>>()
                .join(", ");
            return Err(Diagnostic::at(
                match_stmt.span,
                format!(
                    "non-exhaustive match over `{}`: missing {}",
                    enum_name, rendered
                ),
            ));
        }

        if all_return {
            Ok(BlockFlow::AlwaysReturns)
        } else {
            Ok(BlockFlow::FallsThrough)
        }
    }

    fn resolve_member_type(
        &self,
        object_ty: &Type,
        field: &str,
        span: crate::diag::Span,
    ) -> Result<Type> {
        let (name, args) = match object_ty {
            Type::Module(path) => {
                let namespace = self.module_namespace(path).ok_or_else(|| {
                    Diagnostic::at(span, format!("unknown module namespace `{}`", path))
                })?;
                if let Some(child) = namespace.modules.get(field) {
                    return Ok(Type::Module(child.path.clone()));
                }
                if namespace.functions.contains_key(field) {
                    return Err(Diagnostic::at(
                        span,
                        format!(
                            "function `{}` from module `{}` must be called with `(...)`",
                            field, path
                        ),
                    ));
                }
                if namespace.classes.contains_key(field) {
                    return Err(Diagnostic::at(
                        span,
                        format!(
                            "class `{}` from module `{}` must be constructed with `(...)`",
                            field, path
                        ),
                    ));
                }
                if namespace.enums.contains_key(field) {
                    return Err(Diagnostic::at(
                        span,
                        format!(
                            "enum `{}` from module `{}` must be used via one of its variants",
                            field, path
                        ),
                    ));
                }
                return Err(Diagnostic::at(
                    span,
                    format!("module `{}` has no member `{}`", path, field),
                ));
            }
            Type::Named(name, args) => (name, args),
            Type::TypeParam(type_param_name) => {
                return self
                    .trait_method_from_type_param(type_param_name, field)
                    .map(|method| method.signature.return_type.clone())
                    .map_err(|_| {
                        Diagnostic::at(
                            span,
                            format!("cannot access field `{}` on `{}`", field, object_ty),
                        )
                    });
            }
            Type::Unit => {
                return Err(Diagnostic::at(
                    span,
                    format!("cannot access field `{}` on `{}`", field, object_ty),
                ));
            }
        };

        if BuiltinMember::resolve(name, field).is_some() {
            return Err(Diagnostic::at(
                span,
                format!(
                    "method `{}` on `{}` must be called with `(...)`",
                    field, object_ty
                ),
            ));
        }

        let Some(class_info) = self.resolve_class_info(name) else {
            return Err(Diagnostic::at(
                span,
                format!("type `{}` has no field `{}`", name, field),
            ));
        };
        let substitutions = substitutions_from_decl_type_args(&class_info.decl.type_params, args);
        if let Some(field_info) = class_info.fields.get(field) {
            if self.is_external_module(&class_info.module_name) && !field_info.public {
                return Err(Diagnostic::at(
                    span,
                    format!("field `{}` is private on `{}`", field, class_info.decl.name),
                ));
            }
            let field_ty = substitute_type(&field_info.ty, &substitutions);
            return Ok(field_ty);
        }
        if let Some(method) = class_info.methods.get(field) {
            if self.is_external_module(&class_info.module_name) && !method.decl.public {
                return Err(Diagnostic::at(
                    span,
                    format!(
                        "method `{}` is private on `{}`",
                        field, class_info.decl.name
                    ),
                ));
            }
        }
        if let Some((_trait_impl, method)) = self.trait_method_for_concrete_type(object_ty, field) {
            return Ok(method.signature.return_type.clone());
        }
        Err(Diagnostic::at(
            span,
            format!("class `{}` has no field `{}`", name, field),
        ))
    }

    fn resolve_member_target_type(
        &self,
        object: &Expr,
        field: &str,
        span: crate::diag::Span,
        locals: &mut HashMap<String, LocalBinding>,
    ) -> Result<Type> {
        let object_ty = self.type_of_expr(object, locals)?;
        self.resolve_member_type(&object_ty, field, span)
    }

    fn is_mutable_place(
        &self,
        expr: &Expr,
        locals: &mut HashMap<String, LocalBinding>,
    ) -> Result<bool> {
        match &expr.kind {
            ExprKind::Name(name) => Ok(locals
                .get(name)
                .map(|binding| binding.mutable_place)
                .unwrap_or(false)),
            ExprKind::Group(inner) => self.is_mutable_place(inner, locals),
            ExprKind::Member { object, field } => {
                self.resolve_member_target_type(object, field, expr.span, locals)?;
                self.is_mutable_place(object, locals)
            }
            _ => Ok(false),
        }
    }

    fn render_member_target(&self, object: &Expr, field: &str) -> String {
        format!("{}.{}", self.render_place_expr(object), field)
    }

    fn render_place_expr(&self, expr: &Expr) -> String {
        match &expr.kind {
            ExprKind::Name(name) => name.clone(),
            ExprKind::Group(inner) => self.render_place_expr(inner),
            ExprKind::Member { object, field } => {
                format!("{}.{}", self.render_place_expr(object), field)
            }
            _ => "<place>".to_string(),
        }
    }

    fn borrowed_root_binding_name(
        &self,
        expr: &Expr,
        locals: &HashMap<String, LocalBinding>,
    ) -> Option<String> {
        match &expr.kind {
            ExprKind::Name(name) => locals
                .get(name)
                .filter(|binding| binding.passing != ReceiverKind::Value && name != "self")
                .map(|_| name.clone()),
            ExprKind::Group(inner) => self.borrowed_root_binding_name(inner, locals),
            ExprKind::Member { object, .. } => self.borrowed_root_binding_name(object, locals),
            _ => None,
        }
    }

    fn module_namespace(&self, path: &str) -> Option<&ModuleNamespace> {
        if let Some(namespace) = self.module_registry.get(path) {
            return Some(namespace);
        }
        let mut segments = path.split('.');
        let first = segments.next()?;
        let mut namespace = self.imported_modules.get(first)?;
        for segment in segments {
            namespace = namespace.modules.get(segment)?;
        }
        Some(namespace)
    }

    fn current_module_namespace(&self) -> Option<&ModuleNamespace> {
        if self.module_name == "<main>" {
            None
        } else {
            self.module_registry.get(self.module_name)
        }
    }

    fn infer_module_path(&self, expr: &Expr) -> Option<String> {
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
            _ => None,
        }
    }

    fn qualified_module_item(&self, expr: &Expr) -> Option<(String, String)> {
        match &expr.kind {
            ExprKind::Specialize { expr, .. } => self.qualified_module_item(expr),
            ExprKind::Member { object, field } => self
                .infer_module_path(object)
                .map(|path| (path, field.clone())),
            ExprKind::Group(inner) => self.qualified_module_item(inner),
            _ => None,
        }
    }

    fn find_class_in_modules<'b>(
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

    fn find_enum_in_modules<'b>(
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

    fn imported_class_info(&self, name: &str) -> Option<&ClassInfo> {
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

    fn imported_enum_info(&self, name: &str) -> Option<&EnumInfo> {
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

    fn resolve_function_info(&self, name: &str) -> Option<&FunctionInfo> {
        self.current_module_namespace()
            .and_then(|namespace| namespace.all_functions.get(name))
            .or_else(|| self.functions.get(name))
    }

    fn resolve_class_info(&self, name: &str) -> Option<&ClassInfo> {
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

    fn resolve_enum_info(&self, name: &str) -> Option<&EnumInfo> {
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

    fn is_external_module(&self, owner_module: &str) -> bool {
        owner_module != self.module_name
    }

    fn trait_impls_in_scope(&self) -> impl Iterator<Item = &TraitImplInfo> + '_ {
        self.trait_impls.iter().chain(
            self.module_registry
                .values()
                .flat_map(|namespace| namespace.trait_impls.iter()),
        )
    }

    fn type_implements_trait(&self, ty: &Type, trait_name: &str) -> bool {
        self.trait_impls_in_scope()
            .any(|trait_impl| &trait_impl.for_type == ty && trait_impl.trait_name == trait_name)
    }

    fn assert_type_satisfies_bounds(
        &self,
        ty: &Type,
        bounds: &[String],
        span: crate::diag::Span,
    ) -> Result<()> {
        for bound in bounds {
            match ty {
                Type::TypeParam(name) => {
                    let current_bounds = self
                        .type_param_bounds
                        .get(name)
                        .cloned()
                        .unwrap_or_default();
                    if !current_bounds.iter().any(|current| current == bound) {
                        return Err(Diagnostic::at(
                            span,
                            format!(
                                "type parameter `{}` does not satisfy trait bound `{}`",
                                name, bound
                            ),
                        ));
                    }
                }
                _ => {
                    if !self.type_implements_trait(ty, bound) {
                        return Err(Diagnostic::at(
                            span,
                            format!("type `{}` does not implement trait `{}`", ty, bound),
                        ));
                    }
                }
            }
        }
        Ok(())
    }

    fn trait_method_from_type_param(
        &self,
        type_param_name: &str,
        method_name: &str,
    ) -> Result<&TraitMethodInfo> {
        let mut matches = Vec::new();
        for trait_name in self
            .type_param_bounds
            .get(type_param_name)
            .into_iter()
            .flatten()
        {
            if let Some(trait_info) = self.traits.get(trait_name) {
                if let Some(method) = trait_info.methods.get(method_name) {
                    matches.push(method);
                }
            }
        }
        match matches.len() {
            1 => Ok(matches[0]),
            0 => Err(Diagnostic::new(format!(
                "type parameter `{}` has no method `{}` in its trait bounds",
                type_param_name, method_name
            ))),
            _ => Err(Diagnostic::new(format!(
                "method `{}` is ambiguous for type parameter `{}`",
                method_name, type_param_name
            ))),
        }
    }

    fn trait_method_for_concrete_type(
        &self,
        ty: &Type,
        method_name: &str,
    ) -> Option<(&TraitImplInfo, &TraitImplMethodInfo)> {
        self.trait_impls_in_scope().find_map(|trait_impl| {
            if &trait_impl.for_type != ty {
                return None;
            }
            trait_impl
                .methods
                .get(method_name)
                .map(|method| (trait_impl, method))
        })
    }

    fn type_check_callable_args(
        &self,
        callee_name: &str,
        callee_type_params: &[String],
        param_decls: &[Param],
        param_types: &[Type],
        return_type: &Type,
        callee_type_param_bounds: &BTreeMap<String, Vec<String>>,
        args: &[Argument],
        span: crate::diag::Span,
        locals: &mut HashMap<String, LocalBinding>,
        expected_return: Option<&Type>,
        seed_substitutions: HashMap<String, Type>,
    ) -> Result<Type> {
        let ordered_args = bind_call_arguments(
            callee_name,
            &callable_params_from_decl(param_decls),
            args,
            span,
            CallConvention::PositionalOrNamed,
        )?;

        let mut substitutions = seed_substitutions;
        let mut resolved_args = Vec::new();
        for ((argument, expected), param_decl) in ordered_args
            .into_iter()
            .zip(param_types.iter())
            .zip(param_decls.iter())
        {
            let hinted_expected = substitute_type(expected, &substitutions);
            let actual = if let Some(argument) = argument {
                if has_unresolved_type_params(&hinted_expected) {
                    self.type_of_expr(&argument.value, locals)?
                } else {
                    self.type_of_expr_hint(&argument.value, locals, Some(&hinted_expected))?
                }
            } else {
                let default = param_decl
                    .default
                    .as_ref()
                    .expect("optional parameter should provide a default expression");
                if has_unresolved_type_params(&hinted_expected) {
                    self.type_of_expr(default, locals)?
                } else {
                    self.type_of_expr_hint(default, locals, Some(&hinted_expected))?
                }
            };
            if let Err(error) = unify_type_pattern(expected, &actual, &mut substitutions) {
                let span = argument
                    .map(|argument| argument.span)
                    .unwrap_or(param_decl.span);
                return Err(Diagnostic::at(
                    span,
                    format!(
                        "argument type mismatch for {}: {}",
                        callee_name, error.message
                    ),
                ));
            }
            resolved_args.push((argument, actual));
        }

        if let Some(expected_return) = expected_return {
            if let Err(error) = unify_type_pattern(return_type, expected_return, &mut substitutions)
            {
                return Err(Diagnostic::at(
                    span,
                    format!(
                        "result type mismatch for {}: {}",
                        callee_name, error.message
                    ),
                ));
            }
        }

        for type_param in callee_type_params {
            let Some(resolved) = substitutions.get(type_param) else {
                return Err(Diagnostic::at(
                    span,
                    format!(
                        "cannot infer type parameter `{}` for {}",
                        type_param, callee_name
                    ),
                ));
            };
            if matches!(
                resolved,
                Type::TypeParam(name)
                    if name == type_param && !self.type_params.contains_key(name)
            ) {
                return Err(Diagnostic::at(
                    span,
                    format!(
                        "cannot infer type parameter `{}` for {}",
                        type_param, callee_name
                    ),
                ));
            }
        }

        for (type_param, bounds) in callee_type_param_bounds {
            let Some(resolved_ty) = substitutions.get(type_param) else {
                continue;
            };
            self.assert_type_satisfies_bounds(resolved_ty, bounds, span)?;
        }

        for (((argument, actual), expected), param_decl) in resolved_args
            .into_iter()
            .zip(param_types.iter())
            .zip(param_decls.iter())
        {
            let expected = substitute_type(expected, &substitutions);
            if actual != expected {
                let span = argument
                    .map(|argument| argument.span)
                    .unwrap_or(param_decl.span);
                return Err(Diagnostic::at(
                    span,
                    format!(
                        "argument type mismatch for {}: expected `{}`, found `{}`",
                        callee_name, expected, actual
                    ),
                ));
            }
            if let Some(argument) = argument {
                match param_decl.passing {
                    ReceiverKind::Value => {
                        if !self.is_copy_type(&expected) {
                            self.consume_value_expr(&argument.value, locals)?;
                        }
                    }
                    ReceiverKind::Borrow => {}
                    ReceiverKind::BorrowMut => {
                        if !self.is_mutable_place(&argument.value, locals)? {
                            return Err(Diagnostic::at(
                                argument.span,
                                format!(
                                    "argument for parameter `{}` in {} must be a mutable place",
                                    param_decl.name, callee_name
                                ),
                            ));
                        }
                    }
                }
            }
        }

        Ok(substitute_type(return_type, &substitutions))
    }

    fn enum_variants_for_type(&self, ty: &Type) -> Option<Vec<(String, Option<Type>)>> {
        match ty {
            Type::Named(name, args) if name == "Option" && args.len() == 1 => Some(vec![
                ("Some".to_string(), Some(args[0].clone())),
                ("None".to_string(), None),
            ]),
            Type::Named(name, args) if name == "Result" && args.len() == 2 => Some(vec![
                ("Ok".to_string(), Some(args[0].clone())),
                ("Err".to_string(), Some(args[1].clone())),
            ]),
            Type::Named(name, args) if name == "SendError" && args.len() == 1 => {
                Some(vec![("Closed".to_string(), Some(args[0].clone()))])
            }
            Type::Named(name, args) => self.resolve_enum_info(name).map(|enum_info| {
                let substitutions =
                    substitutions_from_decl_type_args(&enum_info.decl.type_params, args);
                enum_info
                    .decl
                    .variants
                    .iter()
                    .map(|variant| {
                        (
                            variant.name.clone(),
                            enum_info.variants.get(&variant.name).and_then(|info| {
                                info.payload
                                    .as_ref()
                                    .map(|payload| substitute_type(payload, &substitutions))
                            }),
                        )
                    })
                    .collect::<Vec<_>>()
            }),
            _ => None,
        }
    }

    fn builtin_enum_variant_payload(
        &self,
        expected: &Type,
        enum_name: &str,
        variant_name: &str,
    ) -> Option<Option<Type>> {
        let Type::Named(expected_name, args) = expected else {
            return None;
        };
        if expected_name != enum_name {
            return None;
        }
        match (enum_name, variant_name, args.as_slice()) {
            ("Option", "Some", [inner]) => Some(Some(inner.clone())),
            ("Option", "None", [_]) => Some(None),
            ("Result", "Ok", [ok, _err]) => Some(Some(ok.clone())),
            ("Result", "Err", [_ok, err]) => Some(Some(err.clone())),
            ("SendError", "Closed", [value]) => Some(Some(value.clone())),
            _ => None,
        }
    }

    fn require_with_resource(&self, value_ty: &Type, span: crate::diag::Span) -> Result<()> {
        let Type::Named(name, args) = value_ty else {
            return Err(Diagnostic::at(
                span,
                format!("`with` requires a class resource, found `{}`", value_ty),
            ));
        };
        if name == "TaskGroup" && args.is_empty() {
            return Ok(());
        }
        if !args.is_empty() {
            return Err(Diagnostic::at(
                span,
                format!(
                    "`with` does not yet support generic resource types in the bootstrap compiler, found `{}`",
                    value_ty
                ),
            ));
        }

        let Some(class_info) = self.classes.get(name) else {
            return Err(Diagnostic::at(
                span,
                format!("`with` requires a class resource, found `{}`", value_ty),
            ));
        };

        let Some(method) = class_info.methods.get("close") else {
            return Err(Diagnostic::at(
                span,
                format!(
                    "class `{}` cannot be used with `with` because it does not define `close(borrow mut self)`",
                    name
                ),
            ));
        };

        if method.decl.receiver != Some(ReceiverKind::BorrowMut)
            || !method.signature.params.is_empty()
            || method.signature.return_type != Type::Unit
        {
            return Err(Diagnostic::at(
                method.decl.span,
                format!(
                    "`with` resources must define `close(borrow mut self)` returning `None`; `{}` does not",
                    name
                ),
            ));
        }

        Ok(())
    }

    fn require_spawnable_function(
        &self,
        function_name: &str,
        params: &[Param],
        span: crate::diag::Span,
    ) -> Result<()> {
        if let Some(param) = params
            .iter()
            .find(|param| param.passing != ReceiverKind::Value)
        {
            return Err(Diagnostic::at(
                span,
                format!(
                    "`spawn` does not yet support borrowed parameter `{}` on function `{}`",
                    param.name, function_name
                ),
            ));
        }
        Ok(())
    }
}
