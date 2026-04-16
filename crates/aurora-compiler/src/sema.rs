use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fmt;

use serde::{Deserialize, Serialize};

use crate::ast::{
    Argument, AssignStmt, AssignTarget, BinaryOp, ClassDecl, EnumDecl, Expr, ExprKind,
    FunctionDecl, ImplDecl, Item, LiteralPattern, LiteralPatternKind, MatchExprArm, MatchStmt,
    Module, Param, Pattern, ReceiverKind, SelectStmt, Stmt, TraitDecl, TypeRef, UnaryOp, WithStmt,
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
    pub source_path: Option<String>,
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
    pub type_param_bounds: BTreeMap<String, Vec<TraitBound>>,
    pub fields: BTreeMap<String, FieldInfo>,
    pub methods: BTreeMap<String, MethodInfo>,
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
pub struct MethodInfo {
    pub decl: FunctionDecl,
    pub signature: FunctionSignature,
    pub type_param_bounds: BTreeMap<String, Vec<TraitBound>>,
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

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct TraitBound {
    pub trait_name: String,
    pub trait_args: Vec<Type>,
}

impl fmt::Display for TraitBound {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.trait_args.is_empty() {
            write!(f, "{}", self.trait_name)
        } else {
            write!(
                f,
                "{}[{}]",
                self.trait_name,
                self.trait_args
                    .iter()
                    .map(Type::to_string)
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        }
    }
}

pub(crate) fn unary_operator_trait(op: UnaryOp) -> Option<(&'static str, &'static str)> {
    match op {
        UnaryOp::Neg => Some(("Neg", "neg")),
        UnaryOp::Not => Some(("Not", "not")),
    }
}

pub(crate) fn binary_operator_trait(op: BinaryOp) -> Option<(&'static str, &'static str)> {
    match op {
        BinaryOp::Add => Some(("Add", "add")),
        BinaryOp::Sub => Some(("Sub", "sub")),
        BinaryOp::Mul => Some(("Mul", "mul")),
        BinaryOp::Div => Some(("Div", "div")),
        BinaryOp::Mod => Some(("Mod", "mod")),
        BinaryOp::Less => Some(("Ord", "lt")),
        BinaryOp::LessEq => Some(("Ord", "le")),
        BinaryOp::Greater => Some(("Ord", "gt")),
        BinaryOp::GreaterEq => Some(("Ord", "ge")),
        BinaryOp::And | BinaryOp::Or | BinaryOp::Eq | BinaryOp::NotEq => None,
    }
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
    pub source_path: Option<String>,
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
    pub return_passing: ReceiverKind,
    pub return_borrow_source: Option<String>,
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

fn resolve_return_borrow_source(
    receiver: Option<ReceiverKind>,
    params: &[Param],
    return_passing: ReceiverKind,
    explicit_source: Option<&str>,
    span: crate::diag::Span,
) -> Result<Option<String>> {
    if return_passing == ReceiverKind::Value {
        return Ok(None);
    }

    let mut candidates = Vec::new();
    if let Some(receiver_kind) = receiver {
        if receiver_kind == ReceiverKind::BorrowMut || return_passing == ReceiverKind::Borrow {
            candidates.push(("self".to_string(), None, receiver_kind));
        }
    }
    for param in params {
        if param.passing == ReceiverKind::Value {
            continue;
        }
        if return_passing == ReceiverKind::BorrowMut && param.passing != ReceiverKind::BorrowMut {
            continue;
        }
        candidates.push((
            param.name.clone(),
            param.borrow_label.clone(),
            param.passing,
        ));
    }

    if let Some(source) = explicit_source {
        let Some((_name, _label, passing)) = candidates
            .iter()
            .find(|(name, label, _)| name == source || label.as_deref() == Some(source))
        else {
            return Err(Diagnostic::at(
                span,
                format!(
                    "borrow source `{}` must name a borrowed parameter, receiver, or lifetime label",
                    source
                ),
            ));
        };
        if return_passing == ReceiverKind::BorrowMut && *passing != ReceiverKind::BorrowMut {
            return Err(Diagnostic::at(
                span,
                format!(
                    "borrow source `{}` must be `borrow mut` for a `borrow mut` return",
                    source
                ),
            ));
        }
        return Ok(Some(source.to_string()));
    }

    match candidates.as_slice() {
        [] => Err(Diagnostic::at(
            span,
            "borrowed return types require a borrowed parameter or receiver",
        )),
        [(name, _, _)] => Ok(Some(name.clone())),
        _ => Err(Diagnostic::at(
            span,
            format!(
                "borrowed return type is ambiguous; write an explicit borrow source such as `-> {}[{}] T`",
                match return_passing {
                    ReceiverKind::BorrowMut => "borrow mut",
                    _ => "borrow",
                },
                candidates[0].1.as_deref().unwrap_or(&candidates[0].0)
            ),
        )),
    }
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
                        .payloads
                        .iter()
                        .all(|payload| type_is_copy_in_context(&payload.ty, classes, enums))
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
    let mut item_names = BTreeMap::<String, (&'static str, crate::diag::Span)>::new();
    let mut imported_modules = BTreeMap::new();

    let mut imported_functions = BTreeMap::new();
    let mut imported_classes = BTreeMap::new();
    let mut imported_enums = BTreeMap::new();
    let mut imported_traits = BTreeMap::new();

    for (name, binding) in &context.imported_bindings {
        match binding {
            ImportedBinding::Function(function) => {
                item_names.insert(name.clone(), ("function", function.decl.span));
                if let Some(namespace) = context.module_registry.get(&function.module_name) {
                    register_module_namespace_types(namespace, &mut type_names, &mut type_arities);
                }
                imported_functions.insert(name.clone(), function.clone());
            }
            ImportedBinding::Class(class_info) => {
                type_names.insert(name.clone(), class_info.decl.span);
                type_arities.insert(name.clone(), class_info.decl.type_params.len());
                item_names.insert(name.clone(), ("class", class_info.decl.span));
                if let Some(namespace) = context.module_registry.get(&class_info.module_name) {
                    register_module_namespace_types(namespace, &mut type_names, &mut type_arities);
                }
                imported_classes.insert(name.clone(), class_info.clone());
            }
            ImportedBinding::Enum(enum_info) => {
                type_names.insert(name.clone(), enum_info.decl.span);
                type_arities.insert(name.clone(), enum_info.decl.type_params.len());
                item_names.insert(name.clone(), ("enum", enum_info.decl.span));
                if let Some(namespace) = context.module_registry.get(&enum_info.module_name) {
                    register_module_namespace_types(namespace, &mut type_names, &mut type_arities);
                }
                imported_enums.insert(name.clone(), enum_info.clone());
            }
            ImportedBinding::Trait(trait_info) => {
                type_names.insert(name.clone(), trait_info.decl.span);
                type_arities.insert(name.clone(), trait_info.decl.type_params.len());
                item_names.insert(name.clone(), ("trait", trait_info.decl.span));
                if let Some(namespace) = context.module_registry.get(&trait_info.module_name) {
                    register_module_namespace_types(namespace, &mut type_names, &mut type_arities);
                }
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
            }
            Item::Trait(trait_decl) => {
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

    let mut traits = imported_traits.clone();
    for item in &module.items {
        let Item::Trait(trait_decl) = item else {
            continue;
        };
        validate_type_params(&trait_decl.type_params, trait_decl.span, "trait")?;
        let trait_type_param_scope = type_param_scope(&trait_decl.type_params);
        let self_placeholder = Type::TypeParam("Self".to_string());
        let mut methods = BTreeMap::new();
        for method in &trait_decl.methods {
            validate_type_params(&method.type_params, method.span, "trait method")?;
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
                        &method_type_param_scope,
                        Some(&self_placeholder),
                    )
                })
                .collect::<Result<Vec<_>>>()?;
            let return_type = lower_type_with_self(
                &method.return_type,
                &type_names,
                &type_arities,
                &method_type_param_scope,
                Some(&self_placeholder),
            )?;
            let return_borrow_source = resolve_return_borrow_source(
                method.receiver,
                &method.params,
                method.return_passing,
                method.return_borrow_source.as_deref(),
                method.return_type.span,
            )?;
            let type_param_bounds = lower_trait_bounds_with_self(
                &method.type_param_bounds,
                &traits,
                &type_names,
                &type_arities,
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
                            return_type,
                            return_passing: method.return_passing,
                            return_borrow_source,
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
        let enum_type_param_scope = type_param_scope(&enum_decl.type_params);
        let type_param_bounds = lower_trait_bounds(
            &enum_decl.type_param_bounds,
            &traits,
            &type_names,
            &type_arities,
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
            &class_type_param_scope,
        )?;
        let mut fields = BTreeMap::new();
        let mut methods = BTreeMap::new();
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
            let method_type_param_scope =
                merged_type_param_scope(&class_type_param_scope, &method.type_params);
            let type_param_bounds = merge_trait_bounds(
                &type_param_bounds,
                &lower_trait_bounds_with_self(
                    &method.type_param_bounds,
                    &traits,
                    &type_names,
                    &type_arities,
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
                        &method_type_param_scope,
                        Some(&class_self_type),
                    )
                })
                .collect::<Result<Vec<_>>>()?;
            let return_type = lower_type_with_self(
                &method.return_type,
                &type_names,
                &type_arities,
                &method_type_param_scope,
                Some(&class_self_type),
            )?;
            let return_borrow_source = resolve_return_borrow_source(
                method.receiver,
                &method.params,
                method.return_passing,
                method.return_borrow_source.as_deref(),
                method.return_type.span,
            )?;
            if methods
                .insert(
                    method.name.clone(),
                    MethodInfo {
                        decl: method.clone(),
                        signature: FunctionSignature {
                            params,
                            return_type,
                            return_passing: method.return_passing,
                            return_borrow_source,
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

    for item in &module.items {
        let Item::Class(class_decl) = item else {
            continue;
        };
        let class_info = classes
            .get(&class_decl.name)
            .expect("class should exist after collection");
        for field_decl in &class_decl.fields {
            if field_decl.ty.indirect {
                continue;
            }
            let field_ty = &class_info
                .fields
                .get(&field_decl.name)
                .expect("class field should have lowered type")
                .ty;
            if type_reaches_class_through_non_indirect_fields(
                field_ty,
                &class_decl.name,
                &classes,
                &mut BTreeSet::new(),
            ) {
                return Err(Diagnostic::at(
                    field_decl.span,
                    format!(
                        "recursive field `{}` on class `{}` requires `indirect`",
                        field_decl.name, class_decl.name
                    ),
                ));
            }
        }
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
            &type_names,
            &type_arities,
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
        let return_borrow_source = resolve_return_borrow_source(
            function_decl.receiver,
            &function_decl.params,
            function_decl.return_passing,
            function_decl.return_borrow_source.as_deref(),
            function_decl.return_type.span,
        )?;
        functions.insert(
            function_decl.name.clone(),
            FunctionInfo {
                module_name: module_name.clone(),
                decl: function_decl.clone(),
                signature: FunctionSignature {
                    params,
                    return_type,
                    return_passing: function_decl.return_passing,
                    return_borrow_source,
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
            .map(|arg| lower_type(arg, &type_names, &type_arities, &impl_type_param_scope))
            .collect::<Result<Vec<_>>>()?;
        let for_type = lower_type(
            &impl_decl.for_type,
            &type_names,
            &type_arities,
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
            let method_type_param_scope =
                merged_type_param_scope(&impl_type_param_scope, &method.type_params);
            let type_param_bounds = lower_trait_bounds_with_self(
                &method.type_param_bounds,
                &traits,
                &type_names,
                &type_arities,
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
                        &method_type_param_scope,
                        Some(&for_type),
                    )
                })
                .collect::<Result<Vec<_>>>()?;
            let return_type = lower_type_with_self(
                &method.return_type,
                &type_names,
                &type_arities,
                &method_type_param_scope,
                Some(&for_type),
            )?;
            let return_borrow_source = resolve_return_borrow_source(
                method.receiver,
                &method.params,
                method.return_passing,
                method.return_borrow_source.as_deref(),
                method.return_type.span,
            )?;
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
            if params != expected_params || return_type != expected_return_type {
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
                        return_passing: method.return_passing,
                        return_borrow_source,
                    },
                    type_param_bounds,
                },
            );
        }
        for trait_method_name in trait_info.methods.keys() {
            if methods.contains_key(trait_method_name) {
                continue;
            }
            let trait_method = trait_info
                .methods
                .get(trait_method_name)
                .expect("trait method should exist");
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
                        return_type: substitute_type(
                            &trait_method.signature.return_type,
                            &trait_substitutions,
                        ),
                        return_passing: trait_method.signature.return_passing,
                        return_borrow_source: trait_method.signature.return_borrow_source.clone(),
                    },
                    type_param_bounds: substitute_trait_bounds(
                        &trait_method.type_param_bounds,
                        &trait_substitutions,
                    ),
                },
            );
        }
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

    let program = Program {
        module: module.clone(),
        module_name,
        source_path: None,
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
            checker.check_trait_impl_method(
                &trait_impl.for_type,
                &trait_impl.type_params,
                &trait_impl.type_param_bounds,
                &method.decl,
            )?;
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
    lower_type_with_self(type_ref, type_names, type_arities, type_params, None)
}

fn lower_type_with_self(
    type_ref: &TypeRef,
    type_names: &BTreeMap<String, crate::diag::Span>,
    type_arities: &BTreeMap<String, usize>,
    type_params: &BTreeMap<String, ()>,
    self_type: Option<&Type>,
) -> Result<Type> {
    let type_name = if type_ref.name == "str" {
        "String"
    } else {
        type_ref.name.as_str()
    };

    if type_name == "Self" {
        if !type_ref.args.is_empty() {
            return Err(Diagnostic::at(
                type_ref.span,
                "`Self` does not take generic arguments",
            ));
        }
        let Some(self_type) = self_type else {
            return Err(Diagnostic::at(
                type_ref.span,
                "`Self` is only available inside class methods, trait methods, and impl methods",
            ));
        };
        return Ok(self_type.clone());
    }

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
        .map(|arg| lower_type_with_self(arg, type_names, type_arities, type_params, self_type))
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

    if type_name == "Channel"
        || type_name == "Task"
        || type_name == "SendError"
        || type_name == "Vec"
        || type_name == "Set"
    {
        if args.len() != 1 {
            return Err(Diagnostic::at(
                type_ref.span,
                format!("`{}` expects exactly one type argument", type_name),
            ));
        }
        return Ok(Type::Named(type_name.to_string(), args));
    }

    if type_name == "Map" {
        if args.len() != 2 {
            return Err(Diagnostic::at(
                type_ref.span,
                "`Map` expects exactly two type arguments",
            ));
        }
        return Ok(Type::Named(type_name.to_string(), args));
    }

    if type_name == "MapEntry" {
        if args.len() != 2 {
            return Err(Diagnostic::at(
                type_ref.span,
                "`MapEntry` expects exactly two type arguments",
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

pub(crate) fn self_type_substitutions(
    trait_decl: &TraitDecl,
    trait_args: &[Type],
    self_ty: Type,
) -> HashMap<String, Type> {
    let mut substitutions = trait_decl
        .type_params
        .iter()
        .cloned()
        .zip(trait_args.iter().cloned())
        .collect::<HashMap<_, _>>();
    substitutions.insert("Self".to_string(), self_ty);
    substitutions
}

fn reject_reserved_type_name(name: &str, span: crate::diag::Span) -> Result<()> {
    if is_builtin_type(name) {
        return Err(Diagnostic::at(
            span,
            format!("`{}` is a reserved built-in type name", name),
        ));
    }
    Ok(())
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
    for imported in namespace.imported_modules.values() {
        register_module_namespace_types(imported, type_names, type_arities);
    }
}

fn find_namespace_in_modules<'a>(
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

fn validate_type_params(
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

fn collect_type_ref_type_params(
    type_ref: &TypeRef,
    type_names: &BTreeMap<String, crate::diag::Span>,
    collected: &mut BTreeSet<String>,
    include_self: bool,
) {
    if include_self
        && type_ref.args.is_empty()
        && !type_ref.indirect
        && !is_builtin_type(&type_ref.name)
        && !type_names.contains_key(&type_ref.name)
    {
        collected.insert(type_ref.name.clone());
    }
    for arg in &type_ref.args {
        collect_type_ref_type_params(arg, type_names, collected, true);
    }
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
        ExprKind::Index { object, index } => default_argument_references_param(object, param_names)
            .or_else(|| default_argument_references_param(index, param_names)),
        ExprKind::Call { callee, args } => default_argument_references_param(callee, param_names)
            .or_else(|| {
                args.iter().find_map(|argument| {
                    default_argument_references_param(&argument.value, param_names)
                })
            }),
        ExprKind::List(elements) | ExprKind::Set(elements) => elements
            .iter()
            .find_map(|element| default_argument_references_param(element, param_names)),
        ExprKind::Map(entries) => entries.iter().find_map(|entry| {
            default_argument_references_param(&entry.key, param_names)
                .or_else(|| default_argument_references_param(&entry.value, param_names))
        }),
        ExprKind::FString(parts) => parts.iter().find_map(|part| match part {
            crate::ast::FormatPart::Literal(_) => None,
            crate::ast::FormatPart::Expr(expr) => {
                default_argument_references_param(expr, param_names)
            }
        }),
        ExprKind::Match {
            scrutinee, arms, ..
        } => default_argument_references_param(scrutinee, param_names).or_else(|| {
            arms.iter()
                .find_map(|arm| default_argument_references_param(&arm.value, param_names))
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
    type_names: &BTreeMap<String, crate::diag::Span>,
    type_arities: &BTreeMap<String, usize>,
    type_param_scope: &BTreeMap<String, ()>,
) -> Result<BTreeMap<String, Vec<TraitBound>>> {
    lower_trait_bounds_with_self(
        bounds,
        traits,
        type_names,
        type_arities,
        type_param_scope,
        None,
    )
}

fn lower_trait_bounds_with_self(
    bounds: &BTreeMap<String, Vec<TypeRef>>,
    traits: &BTreeMap<String, TraitInfo>,
    type_names: &BTreeMap<String, crate::diag::Span>,
    type_arities: &BTreeMap<String, usize>,
    type_param_scope: &BTreeMap<String, ()>,
    self_type: Option<&Type>,
) -> Result<BTreeMap<String, Vec<TraitBound>>> {
    let mut lowered = BTreeMap::new();
    for (type_param, trait_bounds) in bounds {
        let mut names = Vec::new();
        for bound in trait_bounds {
            let Some(trait_info) = traits.get(&bound.name) else {
                return Err(Diagnostic::at(
                    bound.span,
                    format!("unknown trait `{}`", bound.name),
                ));
            };
            if bound.args.len() != trait_info.decl.type_params.len() {
                return Err(Diagnostic::at(
                    bound.span,
                    format!(
                        "trait `{}` expects {} type arguments, found {}",
                        bound.name,
                        trait_info.decl.type_params.len(),
                        bound.args.len()
                    ),
                ));
            }
            let trait_args = bound
                .args
                .iter()
                .map(|arg| {
                    lower_type_with_self(arg, type_names, type_arities, type_param_scope, self_type)
                })
                .collect::<Result<Vec<_>>>()?;
            names.push(TraitBound {
                trait_name: bound.name.clone(),
                trait_args,
            });
        }
        lowered.insert(type_param.clone(), names);
    }
    Ok(lowered)
}

pub(crate) fn merge_trait_bounds(
    left: &BTreeMap<String, Vec<TraitBound>>,
    right: &BTreeMap<String, Vec<TraitBound>>,
) -> BTreeMap<String, Vec<TraitBound>> {
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

fn type_reaches_class_through_non_indirect_fields(
    ty: &Type,
    target: &str,
    classes: &BTreeMap<String, ClassInfo>,
    visiting: &mut BTreeSet<String>,
) -> bool {
    match ty {
        Type::Named(name, args) => {
            if name == target {
                return true;
            }
            if args.iter().any(|arg| {
                type_reaches_class_through_non_indirect_fields(arg, target, classes, visiting)
            }) {
                return true;
            }
            let Some(class_info) = classes.get(name) else {
                return false;
            };
            if !visiting.insert(name.clone()) {
                return false;
            }
            let reaches_target = class_info.decl.fields.iter().any(|field_decl| {
                if field_decl.ty.indirect {
                    return false;
                }
                let field_ty = &class_info
                    .fields
                    .get(&field_decl.name)
                    .expect("class field should have lowered type")
                    .ty;
                type_reaches_class_through_non_indirect_fields(field_ty, target, classes, visiting)
            });
            visiting.remove(name);
            reaches_target
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

pub(crate) fn substitute_trait_bound(
    bound: &TraitBound,
    substitutions: &HashMap<String, Type>,
) -> TraitBound {
    TraitBound {
        trait_name: bound.trait_name.clone(),
        trait_args: bound
            .trait_args
            .iter()
            .map(|arg| substitute_type(arg, substitutions))
            .collect(),
    }
}

fn substitute_trait_bounds(
    bounds: &BTreeMap<String, Vec<TraitBound>>,
    substitutions: &HashMap<String, Type>,
) -> BTreeMap<String, Vec<TraitBound>> {
    bounds
        .iter()
        .map(|(type_param, type_bounds)| {
            (
                type_param.clone(),
                type_bounds
                    .iter()
                    .map(|bound| substitute_trait_bound(bound, substitutions))
                    .collect(),
            )
        })
        .collect()
}

fn collect_type_params_from_type(ty: &Type, collected: &mut BTreeSet<String>) {
    match ty {
        Type::TypeParam(name) => {
            collected.insert(name.clone());
        }
        Type::Named(_, args) => {
            for arg in args {
                collect_type_params_from_type(arg, collected);
            }
        }
        Type::Unit | Type::Module(_) => {}
    }
}

pub(crate) fn type_pattern_matches(
    pattern: &Type,
    actual: &Type,
    type_params: &BTreeSet<String>,
    substitutions: &mut HashMap<String, Type>,
) -> bool {
    match pattern {
        Type::TypeParam(name) if type_params.contains(name) => {
            if let Some(existing) = substitutions.get(name) {
                existing == actual
            } else {
                substitutions.insert(name.clone(), actual.clone());
                true
            }
        }
        Type::TypeParam(_) => pattern == actual,
        Type::Named(name, pattern_args) => {
            let Type::Named(actual_name, actual_args) = actual else {
                return false;
            };
            if name != actual_name || pattern_args.len() != actual_args.len() {
                return false;
            }
            pattern_args
                .iter()
                .zip(actual_args.iter())
                .all(|(pattern_arg, actual_arg)| {
                    type_pattern_matches(pattern_arg, actual_arg, type_params, substitutions)
                })
        }
        Type::Module(path) => matches!(actual, Type::Module(actual_path) if actual_path == path),
        Type::Unit => *actual == Type::Unit,
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
            | "Vec"
            | "Set"
            | "Map"
            | "MapEntry"
            | "Range"
            | "Channel"
            | "Task"
            | "Option"
            | "Result"
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

fn is_string_type(ty: &Type) -> bool {
    matches!(ty, Type::Named(name, args) if name == "String" && args.is_empty())
}

fn is_numeric_type(ty: &Type) -> bool {
    is_integer_type(ty) || is_float_type(ty)
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
enum LiteralPatternKey {
    Int(IntegerValue),
    Float(u64),
    Bool(bool),
    String(String),
}

fn render_literal_pattern_key(key: &LiteralPatternKey) -> String {
    match key {
        LiteralPatternKey::Int(value) => value.to_string(),
        LiteralPatternKey::Float(bits) => f64::from_bits(*bits).to_string(),
        LiteralPatternKey::Bool(value) => value.to_string(),
        LiteralPatternKey::String(value) => format!("{:?}", value),
    }
}

fn vec_element_type(ty: &Type) -> Option<&Type> {
    match ty {
        Type::Named(name, args) if name == "Vec" && args.len() == 1 => Some(&args[0]),
        _ => None,
    }
}

fn set_element_type(ty: &Type) -> Option<&Type> {
    match ty {
        Type::Named(name, args) if name == "Set" && args.len() == 1 => Some(&args[0]),
        _ => None,
    }
}

fn map_key_value_types(ty: &Type) -> Option<(&Type, &Type)> {
    match ty {
        Type::Named(name, args) if name == "Map" && args.len() == 2 => Some((&args[0], &args[1])),
        _ => None,
    }
}

fn enum_variant_single_payload(variant: &EnumVariantInfo) -> Option<&Type> {
    match variant.payloads.as_slice() {
        [payload] => Some(&payload.ty),
        _ => None,
    }
}

fn pattern_single_binding_name(pattern: &Pattern) -> Option<&str> {
    match pattern {
        Pattern::Binding(binding) => Some(binding.name.as_str()),
        _ => None,
    }
}

fn borrow_places_overlap(left: &str, right: &str) -> bool {
    let left_segments = left.split('.').collect::<Vec<_>>();
    let right_segments = right.split('.').collect::<Vec<_>>();
    if left_segments.first() != right_segments.first() {
        return false;
    }
    let shared = left_segments
        .iter()
        .zip(right_segments.iter())
        .take_while(|(lhs, rhs)| lhs == rhs)
        .count();
    shared == left_segments.len() || shared == right_segments.len()
}

#[derive(Clone)]
struct LocalBinding {
    ty: Type,
    assignable: bool,
    mutable_place: bool,
    passing: ReceiverKind,
    borrow_origin: Option<String>,
    borrow_label: Option<String>,
    moved: bool,
    moved_fields: BTreeSet<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct BorrowSourceInfo {
    origin: String,
    borrow_label: Option<String>,
    passing: ReceiverKind,
}

#[derive(Clone)]
struct BorrowedCallPlace {
    path: String,
    passing: ReceiverKind,
    param_name: String,
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
    current_return_passing: ReceiverKind,
    current_return_borrow_source: Option<String>,
    type_params: BTreeMap<String, ()>,
    type_param_bounds: BTreeMap<String, Vec<TraitBound>>,
}

#[derive(Clone)]
struct ResolvedTraitMethodInfo {
    decl: FunctionDecl,
    signature: FunctionSignature,
    type_param_bounds: BTreeMap<String, Vec<TraitBound>>,
}

#[derive(Clone)]
struct ResolvedCallableInfo {
    display_name: String,
    decl: FunctionDecl,
    signature: FunctionSignature,
    type_param_bounds: BTreeMap<String, Vec<TraitBound>>,
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
                    borrow_origin: None,
                    borrow_label: None,
                    moved: false,
                    moved_fields: BTreeSet::new(),
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
            current_return_passing: ReceiverKind::Value,
            current_return_borrow_source: None,
            type_params: BTreeMap::new(),
            type_param_bounds: BTreeMap::new(),
        }
    }

    fn with_return_type(
        &self,
        return_type: Type,
        return_passing: ReceiverKind,
        return_borrow_source: Option<String>,
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
            current_return_type: Some(return_type),
            current_return_passing: return_passing,
            current_return_borrow_source: return_borrow_source,
            type_params: self.type_params.clone(),
            type_param_bounds: self.type_param_bounds.clone(),
        }
    }

    fn with_type_params(
        &self,
        type_params: BTreeMap<String, ()>,
        type_param_bounds: BTreeMap<String, Vec<TraitBound>>,
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
            current_return_passing: self.current_return_passing,
            current_return_borrow_source: self.current_return_borrow_source.clone(),
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
            current_return_passing: self.current_return_passing,
            current_return_borrow_source: self.current_return_borrow_source.clone(),
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
            ExprKind::List(elements) => {
                for element in elements {
                    self.consume_value_expr(element, locals)?;
                }
                Ok(())
            }
            ExprKind::Set(elements) => {
                for element in elements {
                    self.consume_value_expr(element, locals)?;
                }
                Ok(())
            }
            ExprKind::Map(entries) => {
                for entry in entries {
                    self.consume_value_expr(&entry.key, locals)?;
                    self.consume_value_expr(&entry.value, locals)?;
                }
                Ok(())
            }
            ExprKind::Member { object, field } => {
                let base_object = match &object.kind {
                    ExprKind::Specialize { expr, .. } => &**expr,
                    _ => object,
                };
                if let ExprKind::Name(enum_name) = &base_object.kind {
                    if enum_name == "Option" && field == "None" {
                        return Ok(());
                    }
                    if let Some(enum_info) = self.resolve_enum_info(enum_name) {
                        if enum_info
                            .variants
                            .get(field)
                            .is_some_and(|variant| variant.payloads.is_empty())
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
                            .is_some_and(|variant| variant.payloads.is_empty())
                        {
                            return Ok(());
                        }
                    }
                }
                let object_ty = self.type_of_member_object_expr(object, locals)?;
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
                    if let Some((binding_name, path)) = self.member_access_path(expr) {
                        if let Some(binding) = locals.get_mut(&binding_name) {
                            binding.moved_fields.insert(path);
                        }
                    }
                }
                Ok(())
            }
            ExprKind::Index { .. } => self.type_of_expr(expr, locals).map(|_| ()),
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
                binding.moved_fields = branch_states
                    .iter()
                    .filter_map(|state| state.get(&name))
                    .flat_map(|binding| binding.moved_fields.iter().cloned())
                    .collect();
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
            if body_binding
                .moved_fields
                .iter()
                .any(|field| !binding.moved_fields.contains(field))
            {
                return Err(Diagnostic::at(
                    span,
                    format!(
                        "`{}` loop body partially moves `{}` and may execute more than once",
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
        let type_param_bounds = lower_trait_bounds(
            &function.type_param_bounds,
            self.traits,
            self.type_names,
            self.type_arities,
            &type_param_scope,
        )?;
        let return_type = lower_type(
            &function.return_type,
            self.type_names,
            self.type_arities,
            &type_param_scope,
        )?;
        let return_borrow_source = resolve_return_borrow_source(
            function.receiver,
            &function.params,
            function.return_passing,
            function.return_borrow_source.as_deref(),
            function.return_type.span,
        )?;
        let checker = self
            .with_type_params(type_param_scope.clone(), type_param_bounds)
            .with_return_type(
                return_type.clone(),
                function.return_passing,
                return_borrow_source,
            );
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
                    borrow_origin: (param.passing != ReceiverKind::Value)
                        .then(|| param.name.clone()),
                    borrow_label: param.borrow_label.clone(),
                    moved: false,
                    moved_fields: BTreeSet::new(),
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
        let class_self_type = Type::Named(
            class_decl.name.clone(),
            class_decl
                .type_params
                .iter()
                .cloned()
                .map(Type::TypeParam)
                .collect(),
        );
        let method_type_param_scope =
            merged_type_param_scope(&class_type_param_scope, &method.type_params);
        let class_type_param_bounds = self
            .classes
            .get(&class_decl.name)
            .map(|class_info| class_info.type_param_bounds.clone())
            .unwrap_or_default();
        let type_param_bounds = merge_trait_bounds(
            &class_type_param_bounds,
            &lower_trait_bounds_with_self(
                &method.type_param_bounds,
                self.traits,
                self.type_names,
                self.type_arities,
                &method_type_param_scope,
                Some(&class_self_type),
            )?,
        );
        let return_type = lower_type_with_self(
            &method.return_type,
            self.type_names,
            self.type_arities,
            &method_type_param_scope,
            Some(&class_self_type),
        )?;
        let return_borrow_source = resolve_return_borrow_source(
            method.receiver,
            &method.params,
            method.return_passing,
            method.return_borrow_source.as_deref(),
            method.return_type.span,
        )?;
        let checker = self
            .with_type_params(method_type_param_scope.clone(), type_param_bounds)
            .with_return_type(
                return_type.clone(),
                method.return_passing,
                return_borrow_source,
            );
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
                    borrow_origin: (receiver_kind != ReceiverKind::Value)
                        .then(|| "self".to_string()),
                    borrow_label: None,
                    moved: false,
                    moved_fields: BTreeSet::new(),
                },
            );
        }
        for param in &method.params {
            let ty = lower_type_with_self(
                &param.ty,
                self.type_names,
                self.type_arities,
                &method_type_param_scope,
                Some(&class_self_type),
            )?;
            locals.insert(
                param.name.clone(),
                LocalBinding {
                    ty,
                    assignable: false,
                    mutable_place: param.passing == ReceiverKind::BorrowMut,
                    passing: param.passing,
                    borrow_origin: (param.passing != ReceiverKind::Value)
                        .then(|| param.name.clone()),
                    borrow_label: param.borrow_label.clone(),
                    moved: false,
                    moved_fields: BTreeSet::new(),
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

    fn check_trait_impl_method(
        &self,
        for_type: &Type,
        impl_type_params: &[String],
        impl_type_param_bounds: &BTreeMap<String, Vec<TraitBound>>,
        method: &FunctionDecl,
    ) -> Result<()> {
        let impl_type_param_scope = type_param_scope(impl_type_params);
        let type_param_scope = merged_type_param_scope(&impl_type_param_scope, &method.type_params);
        let type_param_bounds = merge_trait_bounds(
            impl_type_param_bounds,
            &lower_trait_bounds_with_self(
                &method.type_param_bounds,
                self.traits,
                self.type_names,
                self.type_arities,
                &type_param_scope,
                Some(for_type),
            )?,
        );
        let return_type = lower_type_with_self(
            &method.return_type,
            self.type_names,
            self.type_arities,
            &type_param_scope,
            Some(for_type),
        )?;
        let return_borrow_source = resolve_return_borrow_source(
            method.receiver,
            &method.params,
            method.return_passing,
            method.return_borrow_source.as_deref(),
            method.return_type.span,
        )?;
        let checker = self
            .with_type_params(type_param_scope.clone(), type_param_bounds)
            .with_return_type(
                return_type.clone(),
                method.return_passing,
                return_borrow_source,
            );
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
                    borrow_origin: (receiver_kind != ReceiverKind::Value)
                        .then(|| "self".to_string()),
                    borrow_label: None,
                    moved: false,
                    moved_fields: BTreeSet::new(),
                },
            );
        }
        for param in &method.params {
            let ty = lower_type_with_self(
                &param.ty,
                self.type_names,
                self.type_arities,
                &type_param_scope,
                Some(for_type),
            )?;
            locals.insert(
                param.name.clone(),
                LocalBinding {
                    ty,
                    assignable: false,
                    mutable_place: param.passing == ReceiverKind::BorrowMut,
                    passing: param.passing,
                    borrow_origin: (param.passing != ReceiverKind::Value)
                        .then(|| param.name.clone()),
                    borrow_label: param.borrow_label.clone(),
                    moved: false,
                    moved_fields: BTreeSet::new(),
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
                        if self.current_return_passing != ReceiverKind::Value {
                            let actual_source = self.expr_borrow_info(value, locals)?.ok_or_else(|| {
                                    Diagnostic::at(
                                        value.span,
                                        "borrowed return expression must come from a borrowed parameter or receiver",
                                    )
                                })?;
                            let expected_source = self
                                .current_return_borrow_source
                                .as_deref()
                                .expect("borrowed return source should be resolved");
                            if !self.borrow_source_matches(expected_source, &actual_source) {
                                return Err(Diagnostic::at(
                                    value.span,
                                    format!(
                                        "borrowed return expression must come from `{}`",
                                        expected_source
                                    ),
                                ));
                            }
                        } else {
                            self.consume_value_expr(value, locals)?;
                        }
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
                    let (binding_ty, binding_passing, binding_mutable_place) =
                        match (&iterable_ty, for_stmt.borrow_mode) {
                        (Type::Named(name, _), _) if name == "Range" => {
                            (Type::named("int32"), ReceiverKind::Value, false)
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
                            (element_ty, passing, false)
                        }
                        (Type::Named(name, args), borrow_mode) if name == "Vec" && args.len() == 1 => {
                            if borrow_mode == Some(ReceiverKind::BorrowMut)
                                && !self.is_mutable_place(&for_stmt.iterable, locals)?
                            {
                                return Err(Diagnostic::at(
                                    for_stmt.iterable.span,
                                    "`for value in borrow mut ...:` requires a mutable `Vec[T]` place",
                                ));
                            }
                            let element_ty = args[0].clone();
                            let passing = match borrow_mode {
                                Some(ReceiverKind::BorrowMut) => ReceiverKind::BorrowMut,
                                Some(ReceiverKind::Borrow) if !self.is_copy_type(&element_ty) => {
                                    ReceiverKind::Borrow
                                }
                                _ => ReceiverKind::Value,
                            };
                            (
                                element_ty,
                                passing,
                                passing == ReceiverKind::BorrowMut,
                            )
                        }
                        (Type::Named(name, args), Some(ReceiverKind::BorrowMut))
                            if name == "Set" && args.len() == 1 =>
                        {
                            return Err(Diagnostic::at(
                                for_stmt.iterable.span,
                                "`for value in borrow mut ...:` is not supported for `Set[T]`; use `insert`/`remove` on the set directly",
                            ))
                        }
                        (Type::Named(name, args), borrow_mode) if name == "Set" && args.len() == 1 => {
                            let element_ty = args[0].clone();
                            let passing = match borrow_mode {
                                Some(ReceiverKind::Borrow) if !self.is_copy_type(&element_ty) => {
                                    ReceiverKind::Borrow
                                }
                                _ => ReceiverKind::Value,
                            };
                            (element_ty, passing, false)
                        }
                        _ => {
                            return Err(Diagnostic::at(
                                for_stmt.span,
                                format!(
                                    "`for` currently requires a `Range`, `Channel[T]`, `Vec[T]`, or `Set[T]` iterable, found `{}`",
                                    iterable_ty
                                ),
                            ))
                        }
                    };
                    if matches!(
                        (&iterable_ty, for_stmt.borrow_mode),
                        (Type::Named(name, args), Some(ReceiverKind::BorrowMut))
                            if name == "Vec" && args.len() == 1
                    ) && !self.is_mutable_place(&for_stmt.iterable, locals)?
                    {
                        return Err(Diagnostic::at(
                            for_stmt.iterable.span,
                            "`for` with `borrow mut` requires a mutable iterable place",
                        ));
                    }
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
                            mutable_place: binding_mutable_place,
                            passing: binding_passing,
                            borrow_origin: None,
                            borrow_label: None,
                            moved: false,
                            moved_fields: BTreeSet::new(),
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
                borrow_origin: None,
                borrow_label: None,
                moved: false,
                moved_fields: BTreeSet::new(),
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
                            borrow_origin: None,
                            borrow_label: None,
                            moved: false,
                            moved_fields: BTreeSet::new(),
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
        if let AssignTarget::Index { object, index } = &assign.target {
            if assign.mutable {
                return Err(Diagnostic::at(
                    assign.span,
                    "`mut` can only be used when introducing a new binding",
                ));
            }

            if assign.annotation.is_some() {
                return Err(Diagnostic::at(
                    assign.span,
                    "index assignment cannot include a type annotation",
                ));
            }

            if !self.is_mutable_place(object, locals)? {
                return Err(Diagnostic::at(
                    assign.span,
                    format!(
                        "cannot assign through immutable place `{}`",
                        self.render_index_target(object)
                    ),
                ));
            }

            let object_ty = self.type_of_expr(object, locals)?;
            let target_ty = if let Some(target_ty) = vec_element_type(&object_ty).cloned() {
                let index_ty = self.type_of_expr(index, locals)?;
                if !is_integer_type(&index_ty) {
                    return Err(Diagnostic::at(
                        index.span,
                        format!("vector indices must be integers, found `{}`", index_ty),
                    ));
                }
                target_ty
            } else if let Some((key_ty, value_ty)) = map_key_value_types(&object_ty) {
                let index_ty = self.type_of_expr_hint(index, locals, Some(key_ty))?;
                if index_ty != *key_ty {
                    return Err(Diagnostic::at(
                        index.span,
                        format!("map keys must have type `{}`, found `{}`", key_ty, index_ty),
                    ));
                }
                value_ty.clone()
            } else {
                return Err(Diagnostic::at(
                    assign.span,
                    format!("cannot index non-vector-or-map value `{}`", object_ty),
                ));
            };

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
                        "cannot assign value of type `{}` to indexed element of type `{}`",
                        final_value_ty, target_ty
                    ),
                ));
            }

            self.consume_value_expr(&assign.value, locals)?;
            return Ok(());
        }

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

            if let Some((binding_name, path)) = self.member_target_path(object, field) {
                if let Some(binding) = locals.get(&binding_name) {
                    if assign.op.is_some() && Self::field_path_is_moved(binding, &path) {
                        return Err(Diagnostic::at(
                            assign.span,
                            format!(
                                "cannot read moved field `{}` from `{}` in compound assignment",
                                path, binding_name
                            ),
                        ));
                    }
                }
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

            self.consume_value_expr(&assign.value, locals)?;
            if let Some((binding_name, path)) = self.member_target_path(object, field) {
                if let Some(binding) = locals.get_mut(&binding_name) {
                    Self::clear_moved_field_path(binding, &path);
                }
            }
            return Ok(());
        }

        let binding_name = match &assign.target {
            AssignTarget::Name(name) => name,
            AssignTarget::Member { .. } => unreachable!("handled above"),
            AssignTarget::Index { .. } => unreachable!("handled above"),
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

            if !existing.assignable && !existing.mutable_place {
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
                existing.moved_fields.clear();
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

        if let Some(borrowed) = self.expr_borrow_info(&assign.value, locals)? {
            if borrowed.passing == ReceiverKind::Borrow && assign.mutable {
                return Err(Diagnostic::at(
                    assign.value.span,
                    "shared borrowed values cannot be bound with `mut`",
                ));
            }
            locals.insert(
                binding_name.clone(),
                LocalBinding {
                    ty: final_ty,
                    assignable: assign.mutable,
                    mutable_place: borrowed.passing == ReceiverKind::BorrowMut,
                    passing: borrowed.passing,
                    borrow_origin: Some(borrowed.origin),
                    borrow_label: borrowed.borrow_label,
                    moved: false,
                    moved_fields: BTreeSet::new(),
                },
            );
            return Ok(());
        }

        self.consume_value_expr(&assign.value, locals)?;
        locals.insert(
            binding_name.clone(),
            LocalBinding {
                ty: final_ty,
                assignable: assign.mutable,
                mutable_place: assign.mutable,
                passing: ReceiverKind::Value,
                borrow_origin: None,
                borrow_label: None,
                moved: false,
                moved_fields: BTreeSet::new(),
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
                    if !binding.moved_fields.is_empty() {
                        return Err(Diagnostic::at(
                            expr.span,
                            format!("use of partially moved value `{}`", name),
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
            ExprKind::List(elements) => {
                let mut element_ty = expected.and_then(vec_element_type).cloned();
                for element in elements {
                    let actual = if let Some(expected_element_ty) = element_ty.as_ref() {
                        self.type_of_expr_hint(element, locals, Some(expected_element_ty))?
                    } else {
                        self.type_of_expr(element, locals)?
                    };
                    if let Some(expected_element_ty) = element_ty.as_ref() {
                        if actual != *expected_element_ty {
                            return Err(Diagnostic::at(
                                element.span,
                                format!(
                                    "list literal elements must all have type `{}`, found `{}`",
                                    expected_element_ty, actual
                                ),
                            ));
                        }
                    } else {
                        element_ty = Some(actual);
                    }
                }
                let Some(element_ty) = element_ty else {
                    return Err(Diagnostic::at(
                        expr.span,
                        "empty list literals require an expected `Vec[T]` type annotation in the bootstrap compiler",
                    ));
                };
                for element in elements {
                    if !self.is_copy_type(&element_ty) {
                        self.consume_value_expr(element, locals)?;
                    }
                }
                Ok(Type::Named("Vec".to_string(), vec![element_ty]))
            }
            ExprKind::Set(elements) => {
                let mut element_ty = expected.and_then(set_element_type).cloned();
                for element in elements {
                    let actual = if let Some(expected_element_ty) = element_ty.as_ref() {
                        self.type_of_expr_hint(element, locals, Some(expected_element_ty))?
                    } else {
                        self.type_of_expr(element, locals)?
                    };
                    if let Some(expected_element_ty) = element_ty.as_ref() {
                        if actual != *expected_element_ty {
                            return Err(Diagnostic::at(
                                element.span,
                                format!(
                                    "set literal elements must all have type `{}`, found `{}`",
                                    expected_element_ty, actual
                                ),
                            ));
                        }
                    } else {
                        element_ty = Some(actual);
                    }
                }
                let Some(element_ty) = element_ty else {
                    return Err(Diagnostic::at(
                        expr.span,
                        "empty set literals require an expected `Set[T]` type annotation in the bootstrap compiler",
                    ));
                };
                for element in elements {
                    if !self.is_copy_type(&element_ty) {
                        self.consume_value_expr(element, locals)?;
                    }
                }
                Ok(Type::Named("Set".to_string(), vec![element_ty]))
            }
            ExprKind::Map(entries) => {
                let mut key_ty = expected
                    .and_then(map_key_value_types)
                    .map(|(key_ty, _)| key_ty.clone());
                let mut value_ty = expected
                    .and_then(map_key_value_types)
                    .map(|(_, value_ty)| value_ty.clone());
                for entry in entries {
                    let actual_key = if let Some(expected_key_ty) = key_ty.as_ref() {
                        self.type_of_expr_hint(&entry.key, locals, Some(expected_key_ty))?
                    } else {
                        self.type_of_expr(&entry.key, locals)?
                    };
                    if let Some(expected_key_ty) = key_ty.as_ref() {
                        if actual_key != *expected_key_ty {
                            return Err(Diagnostic::at(
                                entry.key.span,
                                format!(
                                    "map literal keys must all have type `{}`, found `{}`",
                                    expected_key_ty, actual_key
                                ),
                            ));
                        }
                    } else {
                        key_ty = Some(actual_key);
                    }

                    let actual_value = if let Some(expected_value_ty) = value_ty.as_ref() {
                        self.type_of_expr_hint(&entry.value, locals, Some(expected_value_ty))?
                    } else {
                        self.type_of_expr(&entry.value, locals)?
                    };
                    if let Some(expected_value_ty) = value_ty.as_ref() {
                        if actual_value != *expected_value_ty {
                            return Err(Diagnostic::at(
                                entry.value.span,
                                format!(
                                    "map literal values must all have type `{}`, found `{}`",
                                    expected_value_ty, actual_value
                                ),
                            ));
                        }
                    } else {
                        value_ty = Some(actual_value);
                    }
                }
                let (Some(key_ty), Some(value_ty)) = (key_ty, value_ty) else {
                    return Err(Diagnostic::at(
                        expr.span,
                        "empty map literals require an expected `Map[K, V]` type annotation in the bootstrap compiler",
                    ));
                };
                for entry in entries {
                    if !self.is_copy_type(&key_ty) {
                        self.consume_value_expr(&entry.key, locals)?;
                    }
                    if !self.is_copy_type(&value_ty) {
                        self.consume_value_expr(&entry.value, locals)?;
                    }
                }
                Ok(Type::Named("Map".to_string(), vec![key_ty, value_ty]))
            }
            ExprKind::Match {
                scrutinee,
                borrow_mode,
                arms,
            } => {
                self.type_of_match_expr(scrutinee, *borrow_mode, arms, expr.span, locals, expected)
            }
            ExprKind::Group(inner) => self.type_of_expr_hint(inner, locals, expected),
            ExprKind::Specialize {
                expr: base,
                type_args,
            } => {
                let lowered = self.lower_explicit_type_args(type_args)?;
                match &base.kind {
                    ExprKind::Name(name)
                        if matches!(name.as_str(), "Option" | "Result" | "SendError") =>
                    {
                        self.explicit_builtin_type(name, &lowered, expr.span)
                    }
                    ExprKind::Name(name) if name == "Set" => {
                        if lowered.len() != 1 {
                            return Err(Diagnostic::at(
                                expr.span,
                                format!(
                                    "type `Set` expects exactly one type argument, found {}",
                                    lowered.len()
                                ),
                            ));
                        }
                        Ok(Type::Named("Set".to_string(), lowered))
                    }
                    ExprKind::Name(name) if name == "Map" => {
                        if lowered.len() != 2 {
                            return Err(Diagnostic::at(
                                expr.span,
                                format!(
                                    "type `Map` expects exactly two type arguments, found {}",
                                    lowered.len()
                                ),
                            ));
                        }
                        Ok(Type::Named("Map".to_string(), lowered))
                    }
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
                    if value_ty == Type::named("bool") {
                        Ok(Type::named("bool"))
                    } else if let Some(return_ty) =
                        self.type_of_unary_operator_via_trait(expr.span, *op, &value_ty)?
                    {
                        Ok(return_ty)
                    } else {
                        Err(Diagnostic::at(
                            expr.span,
                            format!("`not` expects `bool`, found `{}`", value_ty),
                        ))
                    }
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
                    } else if let Some(return_ty) =
                        self.type_of_unary_operator_via_trait(expr.span, *op, &value_ty)?
                    {
                        Ok(return_ty)
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
                let callable = self.resolve_spawn_callable(callee)?;
                self.require_spawnable_function(
                    &callable.display_name,
                    &callable.decl.params,
                    callee.span,
                )?;
                let return_ty = self.type_check_callable_args(
                    &callable.display_name,
                    &callable.decl.type_params,
                    &callable.decl.params,
                    &callable.signature.params,
                    &callable.signature.return_type,
                    &callable.type_param_bounds,
                    args,
                    value.span,
                    locals,
                    None,
                    HashMap::new(),
                )?;
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

                if inner_args[1] != return_args[1]
                    && !self.has_from_conversion(&inner_args[1], &return_args[1])
                {
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
                if let Some((binding_name, path)) = self.member_access_path(expr) {
                    if let Some(binding) = locals.get(&binding_name) {
                        if Self::field_path_is_moved(binding, &path) {
                            return Err(Diagnostic::at(
                                expr.span,
                                format!("use of moved field `{}` from `{}`", path, binding_name),
                            ));
                        }
                    }
                }
                if let ExprKind::Specialize {
                    expr: inner,
                    type_args,
                } = &object.kind
                {
                    if let ExprKind::Name(enum_name) = &inner.kind {
                        let explicit_args = self.lower_explicit_type_args(type_args)?;
                        if let Ok(explicit_ty) =
                            self.explicit_builtin_type(enum_name, &explicit_args, expr.span)
                        {
                            if let Some(payload_ty) =
                                self.builtin_enum_variant_payload(&explicit_ty, enum_name, field)
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
                                return Ok(explicit_ty);
                            }
                        }
                        if let Some(enum_info) = self.resolve_enum_info(enum_name) {
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
                            if !variant.payloads.is_empty() {
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
                            if !variant.payloads.is_empty() {
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
                        if !variant.payloads.is_empty() {
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
                let object_ty = self.type_of_member_object_expr(object, locals)?;
                let member_ty = self.resolve_member_type(&object_ty, field, expr.span)?;
                Ok(member_ty)
            }
            ExprKind::Index { object, index } => {
                let object_ty = self.type_of_expr(object, locals)?;
                if let Some(element_ty) = vec_element_type(&object_ty).cloned() {
                    let index_ty = self.type_of_expr(index, locals)?;
                    if !is_integer_type(&index_ty) {
                        return Err(Diagnostic::at(
                            index.span,
                            format!("vector indices must be integers, found `{}`", index_ty),
                        ));
                    }
                    return Ok(element_ty);
                }
                if let Some((key_ty, value_ty)) = map_key_value_types(&object_ty) {
                    let index_ty = self.type_of_expr_hint(index, locals, Some(key_ty))?;
                    if index_ty != *key_ty {
                        return Err(Diagnostic::at(
                            index.span,
                            format!("map keys must have type `{}`, found `{}`", key_ty, index_ty),
                        ));
                    }
                    return Ok(value_ty.clone());
                }
                Err(Diagnostic::at(
                    expr.span,
                    format!("cannot index non-vector-or-map value `{}`", object_ty),
                ))
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
        match (op, &left_ty, &right_ty) {
            (BinaryOp::And | BinaryOp::Or, Type::Named(name, args), _)
                if args.is_empty() && name == "bool" && left_ty == right_ty =>
            {
                Ok(Type::named("bool"))
            }
            (BinaryOp::Add, Type::Named(name, args), _)
                if args.is_empty()
                    && left_ty == right_ty
                    && (is_integer_type(&left_ty)
                        || is_float_type(&left_ty)
                        || name == "String") =>
            {
                Ok(left_ty)
            }
            (BinaryOp::Sub | BinaryOp::Mul | BinaryOp::Div | BinaryOp::Mod, _, _)
                if left_ty == right_ty
                    && (is_integer_type(&left_ty) || is_float_type(&left_ty)) =>
            {
                Ok(left_ty)
            }
            (BinaryOp::Eq | BinaryOp::NotEq, _, _) if left_ty == right_ty => {
                Ok(Type::named("bool"))
            }
            (BinaryOp::Less | BinaryOp::LessEq | BinaryOp::Greater | BinaryOp::GreaterEq, _, _)
                if left_ty == right_ty
                    && (is_integer_type(&left_ty) || is_float_type(&left_ty)) =>
            {
                Ok(Type::named("bool"))
            }
            _ => {
                if let Some(return_ty) =
                    self.type_of_binary_operator_via_trait(span, op, &left_ty, &right_ty)?
                {
                    Ok(return_ty)
                } else if left_ty != right_ty {
                    Err(Diagnostic::at(
                        span,
                        format!(
                            "binary operator operands must match, found `{}` and `{}`",
                            left_ty, right_ty
                        ),
                    ))
                } else {
                    Err(Diagnostic::at(
                        span,
                        format!("unsupported operands for binary expression: `{}`", left_ty),
                    ))
                }
            }
        }
    }

    fn type_of_unary_operator_via_trait(
        &self,
        span: crate::diag::Span,
        op: UnaryOp,
        value_ty: &Type,
    ) -> Result<Option<Type>> {
        let Some((trait_name, method_name)) = unary_operator_trait(op) else {
            return Ok(None);
        };
        if let Type::TypeParam(type_param_name) = value_ty {
            return self
                .operator_method_from_type_param(type_param_name, trait_name, method_name, None)
                .map(|method| method.map(|method| method.signature.return_type));
        }
        self.operator_method_for_concrete_type(span, value_ty, trait_name, method_name, None)
            .map(|method| {
                method.map(|(method, substitutions)| {
                    substitute_type(&method.signature.return_type, &substitutions)
                })
            })
    }

    fn type_of_binary_operator_via_trait(
        &self,
        span: crate::diag::Span,
        op: BinaryOp,
        left_ty: &Type,
        right_ty: &Type,
    ) -> Result<Option<Type>> {
        let Some((trait_name, method_name)) = binary_operator_trait(op) else {
            return Ok(None);
        };
        if let Type::TypeParam(type_param_name) = left_ty {
            return self
                .operator_method_from_type_param(
                    type_param_name,
                    trait_name,
                    method_name,
                    Some(right_ty),
                )
                .map(|method| method.map(|method| method.signature.return_type));
        }
        self.operator_method_for_concrete_type(
            span,
            left_ty,
            trait_name,
            method_name,
            Some(right_ty),
        )
        .and_then(|method| {
            method
                .map(|(method, substitutions)| {
                    substitute_type(&method.signature.return_type, &substitutions)
                })
                .map(|return_ty| {
                    if matches!(
                        op,
                        BinaryOp::Less | BinaryOp::LessEq | BinaryOp::Greater | BinaryOp::GreaterEq
                    ) && return_ty != Type::named("bool")
                    {
                        Err(Diagnostic::at(
                            span,
                            format!(
                                "operator trait `{}` for `{}` must return `bool`",
                                trait_name, method_name
                            ),
                        ))
                    } else {
                        Ok(return_ty)
                    }
                })
                .transpose()
        })
    }

    fn operator_method_from_type_param(
        &self,
        type_param_name: &str,
        trait_name: &str,
        method_name: &str,
        rhs: Option<&Type>,
    ) -> Result<Option<ResolvedTraitMethodInfo>> {
        let Some(trait_info) = self.traits.get(trait_name) else {
            return Ok(None);
        };
        let Some(method) = trait_info.methods.get(method_name) else {
            return Err(Diagnostic::new(format!(
                "operator trait `{}` must define method `{}`",
                trait_name, method_name
            )));
        };
        let mut matches = Vec::new();
        for bound in self
            .type_param_bounds
            .get(type_param_name)
            .into_iter()
            .flatten()
            .filter(|bound| bound.trait_name == trait_name)
        {
            match rhs {
                Some(rhs_ty) if !bound.trait_args.is_empty() && &bound.trait_args[0] == rhs_ty => {}
                None if bound.trait_args.len() == 1 => {}
                _ => continue,
            }
            let trait_substitutions = self_type_substitutions(
                &trait_info.decl,
                &bound.trait_args,
                Type::TypeParam(type_param_name.to_string()),
            );
            matches.push(ResolvedTraitMethodInfo {
                decl: method.decl.clone(),
                signature: FunctionSignature {
                    params: method
                        .signature
                        .params
                        .iter()
                        .map(|param| substitute_type(param, &trait_substitutions))
                        .collect(),
                    return_type: substitute_type(
                        &method.signature.return_type,
                        &trait_substitutions,
                    ),
                    return_passing: method.signature.return_passing,
                    return_borrow_source: method.signature.return_borrow_source.clone(),
                },
                type_param_bounds: substitute_trait_bounds(
                    &method.type_param_bounds,
                    &trait_substitutions,
                ),
            });
        }
        match matches.len() {
            0 => Ok(None),
            1 => Ok(matches.pop()),
            _ => Err(Diagnostic::new(format!(
                "operator trait `{}` is ambiguous for type parameter `{}`",
                trait_name, type_param_name
            ))),
        }
    }

    fn operator_method_for_concrete_type(
        &self,
        span: crate::diag::Span,
        receiver_ty: &Type,
        trait_name: &str,
        method_name: &str,
        rhs: Option<&Type>,
    ) -> Result<Option<(ResolvedTraitMethodInfo, HashMap<String, Type>)>> {
        let mut matches = Vec::new();
        for trait_impl in self
            .trait_impls_in_scope()
            .filter(|trait_impl| trait_impl.trait_name == trait_name)
        {
            let Some(method) = trait_impl.methods.get(method_name) else {
                continue;
            };
            let mut type_params = BTreeSet::new();
            collect_type_params_from_type(&trait_impl.for_type, &mut type_params);
            for trait_arg in &trait_impl.trait_args {
                collect_type_params_from_type(trait_arg, &mut type_params);
            }
            let mut substitutions = HashMap::new();
            if !type_pattern_matches(
                &trait_impl.for_type,
                receiver_ty,
                &type_params,
                &mut substitutions,
            ) {
                continue;
            }
            match rhs {
                Some(rhs_ty) if !trait_impl.trait_args.is_empty() => {
                    if !type_pattern_matches(
                        &trait_impl.trait_args[0],
                        rhs_ty,
                        &type_params,
                        &mut substitutions,
                    ) {
                        continue;
                    }
                }
                None if trait_impl.trait_args.len() == 1 => {}
                _ => continue,
            }
            let mut valid = true;
            for (type_param, bounds) in &trait_impl.type_param_bounds {
                let Some(actual_ty) = substitutions.get(type_param) else {
                    valid = false;
                    break;
                };
                for impl_bound in bounds {
                    let resolved_bound = substitute_trait_bound(impl_bound, &substitutions);
                    if !self.type_implements_trait_bound(actual_ty, &resolved_bound) {
                        valid = false;
                        break;
                    }
                }
                if !valid {
                    break;
                }
            }
            if !valid {
                continue;
            }
            matches.push((
                ResolvedTraitMethodInfo {
                    decl: method.decl.clone(),
                    signature: FunctionSignature {
                        params: method
                            .signature
                            .params
                            .iter()
                            .map(|param| substitute_type(param, &substitutions))
                            .collect(),
                        return_type: substitute_type(&method.signature.return_type, &substitutions),
                        return_passing: method.signature.return_passing,
                        return_borrow_source: method.signature.return_borrow_source.clone(),
                    },
                    type_param_bounds: substitute_trait_bounds(
                        &method.type_param_bounds,
                        &substitutions,
                    ),
                },
                substitutions,
            ));
        }
        match matches.len() {
            0 => Ok(None),
            1 => Ok(matches.pop()),
            _ => Err(Diagnostic::at(
                span,
                format!(
                    "operator trait `{}` is ambiguous for type `{}`",
                    trait_name, receiver_ty
                ),
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
            if name == "Vec" {
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
                if !args.is_empty() {
                    return Err(Diagnostic::at(
                        span,
                        "class `Vec` does not take constructor arguments; use a list literal or `push(...)`",
                    ));
                }
                return Ok(Type::Named("Vec".to_string(), explicit_args));
            }
            if name == "Set" {
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
                if !args.is_empty() {
                    return Err(Diagnostic::at(
                        span,
                        "class `Set` does not take constructor arguments; use a set literal or `insert(...)`",
                    ));
                }
                return Ok(Type::Named("Set".to_string(), explicit_args));
            }
            if name == "Map" {
                let explicit_args = self.lower_explicit_type_args(type_args)?;
                if explicit_args.len() != 2 {
                    return Err(Diagnostic::at(
                        span,
                        format!(
                            "class `{}` expects exactly two type arguments, found {}",
                            name,
                            explicit_args.len()
                        ),
                    ));
                }
                if !args.is_empty() {
                    return Err(Diagnostic::at(
                        span,
                        "class `Map` does not take constructor arguments; use a map literal or `set(...)`",
                    ));
                }
                return Ok(Type::Named("Map".to_string(), explicit_args));
            }
        }

        match &base_callee.kind {
            ExprKind::Name(name) if matches!(name.as_str(), "Some" | "Ok" | "Err" | "Closed") => {
                let enum_name = match name.as_str() {
                    "Some" => "Option",
                    "Ok" | "Err" => "Result",
                    "Closed" => "SendError",
                    _ => unreachable!(),
                };
                let Some(expected_ty) = expected else {
                    return Err(Diagnostic::at(
                        span,
                        "bare enum variants require an expected enum type or a qualified form such as `Result.Ok(...)`",
                    ));
                };
                self.type_check_builtin_enum_variant_constructor(
                    enum_name,
                    name,
                    expected_ty,
                    args,
                    span,
                    locals,
                )
            }
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
                        if let Some(type_args) = explicit_type_args {
                            let explicit_args = self.lower_explicit_type_args(type_args)?;
                            if explicit_args.len() != 1 {
                                return Err(Diagnostic::at(
                                    span,
                                    format!(
                                        "`channel[...]()` expects exactly one type argument, found {}",
                                        explicit_args.len()
                                    ),
                                ));
                            }
                            Ok(Type::Named("Channel".to_string(), explicit_args))
                        } else {
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
                    BuiltinFunction::Abs => {
                        let value_arg =
                            ordered_args[0].expect("`abs` requires exactly one argument");
                        let value_ty = self.type_of_expr(&value_arg.value, locals)?;
                        if !is_numeric_type(&value_ty) {
                            return Err(Diagnostic::at(
                                value_arg.span,
                                format!(
                                    "`abs(...)` expects an integer or float value, found `{}`",
                                    value_ty
                                ),
                            ));
                        }
                        Ok(value_ty)
                    }
                    BuiltinFunction::Min | BuiltinFunction::Max => {
                        let left_arg =
                            ordered_args[0].expect("`min`/`max` requires a left argument");
                        let left_ty = self.type_of_expr(&left_arg.value, locals)?;
                        if !is_numeric_type(&left_ty) {
                            return Err(Diagnostic::at(
                                left_arg.span,
                                format!(
                                    "`{}` expects numeric arguments, found `{}`",
                                    builtin.name(),
                                    left_ty
                                ),
                            ));
                        }
                        let right_arg =
                            ordered_args[1].expect("`min`/`max` requires a right argument");
                        let right_ty =
                            self.type_of_expr_hint(&right_arg.value, locals, Some(&left_ty))?;
                        if right_ty != left_ty {
                            return Err(Diagnostic::at(
                                right_arg.span,
                                format!(
                                    "`{}` arguments must match, found `{}` and `{}`",
                                    builtin.name(),
                                    left_ty,
                                    right_ty
                                ),
                            ));
                        }
                        if !is_numeric_type(&right_ty) {
                            return Err(Diagnostic::at(
                                right_arg.span,
                                format!(
                                    "`{}` expects numeric arguments, found `{}`",
                                    builtin.name(),
                                    right_ty
                                ),
                            ));
                        }
                        Ok(left_ty)
                    }
                    BuiltinFunction::Sqrt => {
                        let value_arg =
                            ordered_args[0].expect("`sqrt` requires exactly one argument");
                        let value_ty = self.type_of_expr(&value_arg.value, locals)?;
                        if !matches!(
                            value_ty,
                            Type::Named(ref name, ref args)
                                if args.is_empty()
                                    && matches!(name.as_str(), "float32" | "float64")
                        ) {
                            return Err(Diagnostic::at(
                                value_arg.span,
                                format!(
                                    "`sqrt(...)` expects `float32` or `float64`, found `{}`",
                                    value_ty
                                ),
                            ));
                        }
                        Ok(value_ty)
                    }
                    BuiltinFunction::ParseInt32 => {
                        let text_arg =
                            ordered_args[0].expect("`parse_int32` requires exactly one argument");
                        let text_ty = self.type_of_expr_hint(
                            &text_arg.value,
                            locals,
                            Some(&Type::named("String")),
                        )?;
                        if text_ty != Type::named("String") {
                            return Err(Diagnostic::at(
                                text_arg.span,
                                format!("`parse_int32(...)` expects `String`, found `{}`", text_ty),
                            ));
                        }
                        Ok(Type::Named(
                            "Result".to_string(),
                            vec![Type::named("int32"), Type::named("String")],
                        ))
                    }
                    BuiltinFunction::ParseInt64 => {
                        let text_arg =
                            ordered_args[0].expect("`parse_int64` requires exactly one argument");
                        let text_ty = self.type_of_expr_hint(
                            &text_arg.value,
                            locals,
                            Some(&Type::named("String")),
                        )?;
                        if text_ty != Type::named("String") {
                            return Err(Diagnostic::at(
                                text_arg.span,
                                format!("`parse_int64(...)` expects `String`, found `{}`", text_ty),
                            ));
                        }
                        Ok(Type::Named(
                            "Result".to_string(),
                            vec![Type::named("int64"), Type::named("String")],
                        ))
                    }
                    BuiltinFunction::ParseFloat64 => {
                        let text_arg =
                            ordered_args[0].expect("`parse_float64` requires exactly one argument");
                        let text_ty = self.type_of_expr_hint(
                            &text_arg.value,
                            locals,
                            Some(&Type::named("String")),
                        )?;
                        if text_ty != Type::named("String") {
                            return Err(Diagnostic::at(
                                text_arg.span,
                                format!(
                                    "`parse_float64(...)` expects `String`, found `{}`",
                                    text_ty
                                ),
                            ));
                        }
                        Ok(Type::Named(
                            "Result".to_string(),
                            vec![Type::named("float64"), Type::named("String")],
                        ))
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
                let mut next_positional_field = 0usize;
                let mut saw_named = false;
                for argument in args {
                    let field_name = if let Some(field_name) = argument.name.as_ref() {
                        saw_named = true;
                        field_name
                    } else {
                        if saw_named {
                            return Err(Diagnostic::at(
                                argument.span,
                                "positional class constructor arguments must come before named arguments",
                            ));
                        }
                        let Some(field_decl) = class.decl.fields.get(next_positional_field) else {
                            return Err(Diagnostic::at(
                                argument.span,
                                format!(
                                    "class constructor `{}` received too many positional arguments",
                                    name
                                ),
                            ));
                        };
                        next_positional_field += 1;
                        &field_decl.name
                    };
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
                    let actual = match self.type_of_expr_hint(
                        &argument.value,
                        locals,
                        Some(&hinted_field_ty),
                    ) {
                        Ok(actual) => actual,
                        Err(error)
                            if has_unresolved_type_params(&hinted_field_ty)
                                && !self.expr_can_use_partial_expected_hint(&argument.value) =>
                        {
                            match self.type_of_expr(&argument.value, locals) {
                                Ok(actual) => actual,
                                Err(_) => return Err(error),
                            }
                        }
                        Err(error) => return Err(error),
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
                    let substitutions =
                        substitutions_from_decl_type_args(&class.decl.type_params, &resolved_args);
                    let resolved_bounds = bounds
                        .iter()
                        .map(|bound| substitute_trait_bound(bound, &substitutions))
                        .collect::<Vec<_>>();
                    self.assert_type_satisfies_bounds(&resolved_ty, &resolved_bounds, span)?;
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
                            let ordered_args = self.variant_payload_arguments(
                                args,
                                span,
                                field,
                                &item_name,
                                &variant.payloads,
                                variant.named_payloads,
                            )?;
                            for (argument, payload) in
                                ordered_args.iter().zip(variant.payloads.iter())
                            {
                                let actual = self.type_of_expr_hint(
                                    &argument.value,
                                    locals,
                                    Some(&payload.ty),
                                )?;
                                if actual != payload.ty {
                                    return Err(Diagnostic::at(
                                        argument.span,
                                        format!(
                                            "variant `{}` of enum `{}` expects `{}`, found `{}`",
                                            field, item_name, payload.ty, actual
                                        ),
                                    ));
                                }
                                if !self.is_copy_type(&actual) {
                                    self.consume_value_expr(&argument.value, locals)?;
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
                    if let Some(type_args) = object_type_args {
                        let explicit_args = self.lower_explicit_type_args(type_args)?;
                        if let Ok(explicit_ty) =
                            self.explicit_builtin_type(enum_name, &explicit_args, span)
                        {
                            return self.type_check_builtin_enum_variant_constructor(
                                enum_name,
                                field,
                                &explicit_ty,
                                args,
                                span,
                                locals,
                            );
                        }
                    }
                    if let Some(expected_ty) = expected {
                        if let Some(variant_payload) =
                            self.builtin_enum_variant_payload(expected_ty, enum_name, field)
                        {
                            match variant_payload {
                                Some(payload_ty) => {
                                    let argument = self
                                        .variant_payload_argument(args, span, field, enum_name)?;
                                    let actual = self.type_of_expr_hint(
                                        &argument.value,
                                        locals,
                                        Some(&payload_ty),
                                    )?;
                                    if actual != payload_ty {
                                        return Err(Diagnostic::at(
                                            argument.span,
                                            format!(
                                                "variant `{}` of enum `{}` expects `{}`, found `{}`",
                                                field, enum_name, payload_ty, actual
                                            ),
                                        ));
                                    }
                                    if !self.is_copy_type(&payload_ty) {
                                        self.consume_value_expr(&argument.value, locals)?;
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
                        if variant.payloads.is_empty() && args.is_empty() {
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
                        let ordered_args = self.variant_payload_arguments(
                            args,
                            span,
                            field,
                            enum_name,
                            &variant.payloads,
                            variant.named_payloads,
                        )?;
                        for (argument, payload) in ordered_args.iter().zip(variant.payloads.iter())
                        {
                            let hinted_payload_ty = substitute_type(&payload.ty, &substitutions);
                            let actual = if has_unresolved_type_params(&hinted_payload_ty) {
                                self.type_of_expr(&argument.value, locals)?
                            } else {
                                self.type_of_expr_hint(
                                    &argument.value,
                                    locals,
                                    Some(&hinted_payload_ty),
                                )?
                            };
                            if let Err(error) =
                                unify_type_pattern(&payload.ty, &actual, &mut substitutions)
                            {
                                return Err(Diagnostic::at(
                                    argument.span,
                                    format!(
                                        "variant `{}` of enum `{}` expects `{}`, found `{}` ({})",
                                        field, enum_name, hinted_payload_ty, actual, error.message
                                    ),
                                ));
                            }
                            if !self.is_copy_type(&actual) {
                                self.consume_value_expr(&argument.value, locals)?;
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
                            let substitutions = substitutions_from_decl_type_args(
                                &enum_info.decl.type_params,
                                &resolved_args,
                            );
                            let resolved_bounds = bounds
                                .iter()
                                .map(|bound| substitute_trait_bound(bound, &substitutions))
                                .collect::<Vec<_>>();
                            self.assert_type_satisfies_bounds(
                                &resolved_ty,
                                &resolved_bounds,
                                span,
                            )?;
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
                        let mut provided = HashMap::new();
                        let mut next_positional_field = 0usize;
                        let mut saw_named = false;
                        for argument in args {
                            let field_name = if let Some(field_name) = argument.name.as_ref() {
                                saw_named = true;
                                field_name
                            } else {
                                if saw_named {
                                    return Err(Diagnostic::at(
                                        argument.span,
                                        "positional class constructor arguments must come before named arguments",
                                    ));
                                }
                                let Some(field_decl) = class.decl.fields.get(next_positional_field)
                                else {
                                    return Err(Diagnostic::at(
                                        argument.span,
                                        format!(
                                            "class constructor `{}` received too many positional arguments",
                                            class.decl.name
                                        ),
                                    ));
                                };
                                next_positional_field += 1;
                                &field_decl.name
                            };
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
                    if receiver_name == "Vec" && receiver_args.len() == 1 {
                        if let Some(builtin_member) = BuiltinMember::resolve(receiver_name, field) {
                            let ordered_args = builtin_member.bind_args(args, span)?;
                            return match builtin_member {
                                BuiltinMember::VecLen => Ok(Type::named("int32")),
                                BuiltinMember::VecIsEmpty => Ok(Type::named("bool")),
                                BuiltinMember::VecClone => Ok(receiver_ty.clone()),
                                BuiltinMember::VecPush => {
                                    if !self.is_mutable_place(object, locals)? {
                                        return Err(Diagnostic::at(
                                            span,
                                            format!(
                                                "method `{}` requires a mutable receiver",
                                                field
                                            ),
                                        ));
                                    }
                                    let push_arg = ordered_args[0]
                                        .expect("`push` requires exactly one argument");
                                    let actual = self.type_of_expr_hint(
                                        &push_arg.value,
                                        locals,
                                        Some(&receiver_args[0]),
                                    )?;
                                    if actual != receiver_args[0] {
                                        return Err(Diagnostic::at(
                                            push_arg.span,
                                            format!(
                                                "`push` expects `{}`, found `{}`",
                                                receiver_args[0], actual
                                            ),
                                        ));
                                    }
                                    if !self.is_copy_type(&receiver_args[0]) {
                                        self.consume_value_expr(&push_arg.value, locals)?;
                                    }
                                    Ok(Type::Unit)
                                }
                                BuiltinMember::VecPop => {
                                    if !self.is_mutable_place(object, locals)? {
                                        return Err(Diagnostic::at(
                                            span,
                                            format!(
                                                "method `{}` requires a mutable receiver",
                                                field
                                            ),
                                        ));
                                    }
                                    Ok(Type::Named(
                                        "Option".to_string(),
                                        vec![receiver_args[0].clone()],
                                    ))
                                }
                                BuiltinMember::VecGet => {
                                    let index_arg = ordered_args[0]
                                        .expect("`get` requires exactly one argument");
                                    let index_ty = self.type_of_expr_hint(
                                        &index_arg.value,
                                        locals,
                                        Some(&Type::named("int32")),
                                    )?;
                                    if !is_integer_type(&index_ty) {
                                        return Err(Diagnostic::at(
                                            index_arg.span,
                                            format!(
                                                "vector indices must be integers, found `{}`",
                                                index_ty
                                            ),
                                        ));
                                    }
                                    Ok(Type::Named(
                                        "Option".to_string(),
                                        vec![receiver_args[0].clone()],
                                    ))
                                }
                                BuiltinMember::VecSet => {
                                    if !self.is_mutable_place(object, locals)? {
                                        return Err(Diagnostic::at(
                                            span,
                                            format!(
                                                "method `{}` requires a mutable receiver",
                                                field
                                            ),
                                        ));
                                    }
                                    let index_arg = ordered_args[0]
                                        .expect("`set` requires an `index` argument");
                                    let index_ty = self.type_of_expr_hint(
                                        &index_arg.value,
                                        locals,
                                        Some(&Type::named("int32")),
                                    )?;
                                    if !is_integer_type(&index_ty) {
                                        return Err(Diagnostic::at(
                                            index_arg.span,
                                            format!(
                                                "vector indices must be integers, found `{}`",
                                                index_ty
                                            ),
                                        ));
                                    }
                                    let value_arg =
                                        ordered_args[1].expect("`set` requires a `value` argument");
                                    let actual = self.type_of_expr_hint(
                                        &value_arg.value,
                                        locals,
                                        Some(&receiver_args[0]),
                                    )?;
                                    if actual != receiver_args[0] {
                                        return Err(Diagnostic::at(
                                            value_arg.span,
                                            format!(
                                                "`set` expects `{}`, found `{}`",
                                                receiver_args[0], actual
                                            ),
                                        ));
                                    }
                                    if !self.is_copy_type(&receiver_args[0]) {
                                        self.consume_value_expr(&value_arg.value, locals)?;
                                    }
                                    Ok(Type::Named(
                                        "Option".to_string(),
                                        vec![receiver_args[0].clone()],
                                    ))
                                }
                                BuiltinMember::VecRemove => {
                                    if !self.is_mutable_place(object, locals)? {
                                        return Err(Diagnostic::at(
                                            span,
                                            format!(
                                                "method `{}` requires a mutable receiver",
                                                field
                                            ),
                                        ));
                                    }
                                    let index_arg = ordered_args[0]
                                        .expect("`remove` requires exactly one argument");
                                    let index_ty = self.type_of_expr_hint(
                                        &index_arg.value,
                                        locals,
                                        Some(&Type::named("int32")),
                                    )?;
                                    if !is_integer_type(&index_ty) {
                                        return Err(Diagnostic::at(
                                            index_arg.span,
                                            format!(
                                                "vector indices must be integers, found `{}`",
                                                index_ty
                                            ),
                                        ));
                                    }
                                    Ok(Type::Named(
                                        "Option".to_string(),
                                        vec![receiver_args[0].clone()],
                                    ))
                                }
                                BuiltinMember::VecSwap => {
                                    if !self.is_mutable_place(object, locals)? {
                                        return Err(Diagnostic::at(
                                            span,
                                            format!(
                                                "method `{}` requires a mutable receiver",
                                                field
                                            ),
                                        ));
                                    }
                                    let first_arg = ordered_args[0]
                                        .expect("`swap` requires a `first` argument");
                                    let first_ty = self.type_of_expr_hint(
                                        &first_arg.value,
                                        locals,
                                        Some(&Type::named("int32")),
                                    )?;
                                    if !is_integer_type(&first_ty) {
                                        return Err(Diagnostic::at(
                                            first_arg.span,
                                            format!(
                                                "vector indices must be integers, found `{}`",
                                                first_ty
                                            ),
                                        ));
                                    }
                                    let second_arg = ordered_args[1]
                                        .expect("`swap` requires a `second` argument");
                                    let second_ty = self.type_of_expr_hint(
                                        &second_arg.value,
                                        locals,
                                        Some(&Type::named("int32")),
                                    )?;
                                    if !is_integer_type(&second_ty) {
                                        return Err(Diagnostic::at(
                                            second_arg.span,
                                            format!(
                                                "vector indices must be integers, found `{}`",
                                                second_ty
                                            ),
                                        ));
                                    }
                                    Ok(Type::named("bool"))
                                }
                                BuiltinMember::VecContains => {
                                    let value_arg = ordered_args[0]
                                        .expect("`contains` requires a `value` argument");
                                    let actual = self.type_of_expr_hint(
                                        &value_arg.value,
                                        locals,
                                        Some(&receiver_args[0]),
                                    )?;
                                    if actual != receiver_args[0] {
                                        return Err(Diagnostic::at(
                                            value_arg.span,
                                            format!(
                                                "`contains` expects `{}`, found `{}`",
                                                receiver_args[0], actual
                                            ),
                                        ));
                                    }
                                    Ok(Type::named("bool"))
                                }
                                BuiltinMember::VecExtend => {
                                    if !self.is_mutable_place(object, locals)? {
                                        return Err(Diagnostic::at(
                                            span,
                                            format!(
                                                "method `{}` requires a mutable receiver",
                                                field
                                            ),
                                        ));
                                    }
                                    let other_arg = ordered_args[0]
                                        .expect("`extend` requires an `other` argument");
                                    let actual = self.type_of_expr_hint(
                                        &other_arg.value,
                                        locals,
                                        Some(&receiver_ty),
                                    )?;
                                    if actual != receiver_ty {
                                        return Err(Diagnostic::at(
                                            other_arg.span,
                                            format!(
                                                "`extend` expects `{}`, found `{}`",
                                                receiver_ty, actual
                                            ),
                                        ));
                                    }
                                    self.consume_value_expr(&other_arg.value, locals)?;
                                    Ok(Type::Unit)
                                }
                                BuiltinMember::VecInsert => {
                                    if !self.is_mutable_place(object, locals)? {
                                        return Err(Diagnostic::at(
                                            span,
                                            format!(
                                                "method `{}` requires a mutable receiver",
                                                field
                                            ),
                                        ));
                                    }
                                    let index_arg = ordered_args[0]
                                        .expect("`insert` requires an `index` argument");
                                    let index_ty = self.type_of_expr_hint(
                                        &index_arg.value,
                                        locals,
                                        Some(&Type::named("int32")),
                                    )?;
                                    if !is_integer_type(&index_ty) {
                                        return Err(Diagnostic::at(
                                            index_arg.span,
                                            format!(
                                                "vector indices must be integers, found `{}`",
                                                index_ty
                                            ),
                                        ));
                                    }
                                    let value_arg = ordered_args[1]
                                        .expect("`insert` requires a `value` argument");
                                    let actual = self.type_of_expr_hint(
                                        &value_arg.value,
                                        locals,
                                        Some(&receiver_args[0]),
                                    )?;
                                    if actual != receiver_args[0] {
                                        return Err(Diagnostic::at(
                                            value_arg.span,
                                            format!(
                                                "`insert` expects `{}`, found `{}`",
                                                receiver_args[0], actual
                                            ),
                                        ));
                                    }
                                    if !self.is_copy_type(&receiver_args[0]) {
                                        self.consume_value_expr(&value_arg.value, locals)?;
                                    }
                                    Ok(Type::named("bool"))
                                }
                                BuiltinMember::VecClear | BuiltinMember::VecReverse => {
                                    if !self.is_mutable_place(object, locals)? {
                                        return Err(Diagnostic::at(
                                            span,
                                            format!(
                                                "method `{}` requires a mutable receiver",
                                                field
                                            ),
                                        ));
                                    }
                                    Ok(Type::Unit)
                                }
                                _ => unreachable!("unexpected vector builtin member"),
                            };
                        }
                    }

                    if receiver_name == "String" && receiver_args.is_empty() {
                        if let Some(builtin_member) = BuiltinMember::resolve(receiver_name, field) {
                            let ordered_args = builtin_member.bind_args(args, span)?;
                            return match builtin_member {
                                BuiltinMember::StringLen => Ok(Type::named("int32")),
                                BuiltinMember::StringContains
                                | BuiltinMember::StringStartsWith
                                | BuiltinMember::StringEndsWith => {
                                    let text_arg = ordered_args[0]
                                        .expect("string predicate methods require one argument");
                                    let actual = self.type_of_expr_hint(
                                        &text_arg.value,
                                        locals,
                                        Some(&Type::named("String")),
                                    )?;
                                    if actual != Type::named("String") {
                                        return Err(Diagnostic::at(
                                            text_arg.span,
                                            format!(
                                                "`{}` expects `String`, found `{}`",
                                                field, actual
                                            ),
                                        ));
                                    }
                                    Ok(Type::named("bool"))
                                }
                                BuiltinMember::StringSplit => {
                                    let text_arg = ordered_args[0]
                                        .expect("`split` requires a `text` argument");
                                    let actual = self.type_of_expr_hint(
                                        &text_arg.value,
                                        locals,
                                        Some(&Type::named("String")),
                                    )?;
                                    if actual != Type::named("String") {
                                        return Err(Diagnostic::at(
                                            text_arg.span,
                                            format!("`split` expects `String`, found `{}`", actual),
                                        ));
                                    }
                                    Ok(Type::Named("Vec".to_string(), vec![Type::named("String")]))
                                }
                                BuiltinMember::StringReplace => {
                                    let from_arg = ordered_args[0]
                                        .expect("`replace` requires a `from` argument");
                                    let from_actual = self.type_of_expr_hint(
                                        &from_arg.value,
                                        locals,
                                        Some(&Type::named("String")),
                                    )?;
                                    if from_actual != Type::named("String") {
                                        return Err(Diagnostic::at(
                                            from_arg.span,
                                            format!(
                                                "`replace` expects `String` for `from`, found `{}`",
                                                from_actual
                                            ),
                                        ));
                                    }
                                    let to_arg = ordered_args[1]
                                        .expect("`replace` requires a `to` argument");
                                    let to_actual = self.type_of_expr_hint(
                                        &to_arg.value,
                                        locals,
                                        Some(&Type::named("String")),
                                    )?;
                                    if to_actual != Type::named("String") {
                                        return Err(Diagnostic::at(
                                            to_arg.span,
                                            format!(
                                                "`replace` expects `String` for `to`, found `{}`",
                                                to_actual
                                            ),
                                        ));
                                    }
                                    Ok(Type::named("String"))
                                }
                                BuiltinMember::StringToLower
                                | BuiltinMember::StringToUpper
                                | BuiltinMember::StringTrim
                                | BuiltinMember::StringClone => Ok(Type::named("String")),
                                BuiltinMember::StringJoin => {
                                    let parts_arg = ordered_args[0]
                                        .expect("`join` requires a `parts` argument");
                                    let expected_parts =
                                        Type::Named("Vec".to_string(), vec![Type::named("String")]);
                                    let actual = self.type_of_expr_hint(
                                        &parts_arg.value,
                                        locals,
                                        Some(&expected_parts),
                                    )?;
                                    if actual != expected_parts {
                                        return Err(Diagnostic::at(
                                            parts_arg.span,
                                            format!(
                                                "`join` expects `Vec[String]`, found `{}`",
                                                actual
                                            ),
                                        ));
                                    }
                                    Ok(Type::named("String"))
                                }
                                BuiltinMember::StringStripPrefix
                                | BuiltinMember::StringStripSuffix => {
                                    let text_arg = ordered_args[0]
                                        .expect("string strip methods require one `text` argument");
                                    let actual = self.type_of_expr_hint(
                                        &text_arg.value,
                                        locals,
                                        Some(&Type::named("String")),
                                    )?;
                                    if actual != Type::named("String") {
                                        return Err(Diagnostic::at(
                                            text_arg.span,
                                            format!(
                                                "`{}` expects `String`, found `{}`",
                                                field, actual
                                            ),
                                        ));
                                    }
                                    Ok(Type::Named(
                                        "Option".to_string(),
                                        vec![Type::named("String")],
                                    ))
                                }
                                _ => unreachable!("unexpected string builtin member"),
                            };
                        }
                    }

                    if receiver_name == "Map" && receiver_args.len() == 2 {
                        if let Some(builtin_member) = BuiltinMember::resolve(receiver_name, field) {
                            let ordered_args = builtin_member.bind_args(args, span)?;
                            return match builtin_member {
                                BuiltinMember::MapLen => Ok(Type::named("int32")),
                                BuiltinMember::MapIsEmpty => Ok(Type::named("bool")),
                                BuiltinMember::MapClone => Ok(receiver_ty.clone()),
                                BuiltinMember::MapGet => {
                                    let key_arg = ordered_args[0]
                                        .expect("`get` requires exactly one key argument");
                                    let actual = self.type_of_expr_hint(
                                        &key_arg.value,
                                        locals,
                                        Some(&receiver_args[0]),
                                    )?;
                                    if actual != receiver_args[0] {
                                        return Err(Diagnostic::at(
                                            key_arg.span,
                                            format!(
                                                "`get` expects `{}`, found `{}`",
                                                receiver_args[0], actual
                                            ),
                                        ));
                                    }
                                    Ok(Type::Named(
                                        "Option".to_string(),
                                        vec![receiver_args[1].clone()],
                                    ))
                                }
                                BuiltinMember::MapSet => {
                                    if !self.is_mutable_place(object, locals)? {
                                        return Err(Diagnostic::at(
                                            span,
                                            format!(
                                                "method `{}` requires a mutable receiver",
                                                field
                                            ),
                                        ));
                                    }
                                    let key_arg =
                                        ordered_args[0].expect("`set` requires a `key` argument");
                                    let key_actual = self.type_of_expr_hint(
                                        &key_arg.value,
                                        locals,
                                        Some(&receiver_args[0]),
                                    )?;
                                    if key_actual != receiver_args[0] {
                                        return Err(Diagnostic::at(
                                            key_arg.span,
                                            format!(
                                                "`set` expects key type `{}`, found `{}`",
                                                receiver_args[0], key_actual
                                            ),
                                        ));
                                    }
                                    let value_arg =
                                        ordered_args[1].expect("`set` requires a `value` argument");
                                    let value_actual = self.type_of_expr_hint(
                                        &value_arg.value,
                                        locals,
                                        Some(&receiver_args[1]),
                                    )?;
                                    if value_actual != receiver_args[1] {
                                        return Err(Diagnostic::at(
                                            value_arg.span,
                                            format!(
                                                "`set` expects value type `{}`, found `{}`",
                                                receiver_args[1], value_actual
                                            ),
                                        ));
                                    }
                                    if !self.is_copy_type(&receiver_args[0]) {
                                        self.consume_value_expr(&key_arg.value, locals)?;
                                    }
                                    if !self.is_copy_type(&receiver_args[1]) {
                                        self.consume_value_expr(&value_arg.value, locals)?;
                                    }
                                    Ok(Type::Named(
                                        "Option".to_string(),
                                        vec![receiver_args[1].clone()],
                                    ))
                                }
                                BuiltinMember::MapRemove => {
                                    if !self.is_mutable_place(object, locals)? {
                                        return Err(Diagnostic::at(
                                            span,
                                            format!(
                                                "method `{}` requires a mutable receiver",
                                                field
                                            ),
                                        ));
                                    }
                                    let key_arg = ordered_args[0]
                                        .expect("`remove` requires exactly one key argument");
                                    let actual = self.type_of_expr_hint(
                                        &key_arg.value,
                                        locals,
                                        Some(&receiver_args[0]),
                                    )?;
                                    if actual != receiver_args[0] {
                                        return Err(Diagnostic::at(
                                            key_arg.span,
                                            format!(
                                                "`remove` expects `{}`, found `{}`",
                                                receiver_args[0], actual
                                            ),
                                        ));
                                    }
                                    if !self.is_copy_type(&receiver_args[0]) {
                                        self.consume_value_expr(&key_arg.value, locals)?;
                                    }
                                    Ok(Type::Named(
                                        "Option".to_string(),
                                        vec![receiver_args[1].clone()],
                                    ))
                                }
                                BuiltinMember::MapContainsKey => {
                                    let key_arg = ordered_args[0]
                                        .expect("`contains_key` requires exactly one key argument");
                                    let actual = self.type_of_expr_hint(
                                        &key_arg.value,
                                        locals,
                                        Some(&receiver_args[0]),
                                    )?;
                                    if actual != receiver_args[0] {
                                        return Err(Diagnostic::at(
                                            key_arg.span,
                                            format!(
                                                "`contains_key` expects `{}`, found `{}`",
                                                receiver_args[0], actual
                                            ),
                                        ));
                                    }
                                    Ok(Type::named("bool"))
                                }
                                BuiltinMember::MapKeys => Ok(Type::Named(
                                    "Vec".to_string(),
                                    vec![receiver_args[0].clone()],
                                )),
                                BuiltinMember::MapValues => Ok(Type::Named(
                                    "Vec".to_string(),
                                    vec![receiver_args[1].clone()],
                                )),
                                BuiltinMember::MapItems | BuiltinMember::MapEntries => {
                                    Ok(Type::Named(
                                        "Vec".to_string(),
                                        vec![Type::Named(
                                            "MapEntry".to_string(),
                                            vec![
                                                receiver_args[0].clone(),
                                                receiver_args[1].clone(),
                                            ],
                                        )],
                                    ))
                                }
                                BuiltinMember::MapClear => {
                                    if !self.is_mutable_place(object, locals)? {
                                        return Err(Diagnostic::at(
                                            span,
                                            format!(
                                                "method `{}` requires a mutable receiver",
                                                field
                                            ),
                                        ));
                                    }
                                    Ok(Type::Unit)
                                }
                                BuiltinMember::MapExtend => {
                                    if !self.is_mutable_place(object, locals)? {
                                        return Err(Diagnostic::at(
                                            span,
                                            format!(
                                                "method `{}` requires a mutable receiver",
                                                field
                                            ),
                                        ));
                                    }
                                    let other_arg = ordered_args[0]
                                        .expect("`extend` requires an `other` argument");
                                    let actual = self.type_of_expr_hint(
                                        &other_arg.value,
                                        locals,
                                        Some(&receiver_ty),
                                    )?;
                                    if actual != receiver_ty {
                                        return Err(Diagnostic::at(
                                            other_arg.span,
                                            format!(
                                                "`extend` expects `{}`, found `{}`",
                                                receiver_ty, actual
                                            ),
                                        ));
                                    }
                                    self.consume_value_expr(&other_arg.value, locals)?;
                                    Ok(Type::Unit)
                                }
                                _ => unreachable!("unexpected map builtin member"),
                            };
                        }
                    }

                    if receiver_name == "Set" && receiver_args.len() == 1 {
                        if let Some(builtin_member) = BuiltinMember::resolve(receiver_name, field) {
                            let ordered_args = builtin_member.bind_args(args, span)?;
                            return match builtin_member {
                                BuiltinMember::SetLen => Ok(Type::named("int32")),
                                BuiltinMember::SetIsEmpty => Ok(Type::named("bool")),
                                BuiltinMember::SetClone => Ok(receiver_ty.clone()),
                                BuiltinMember::SetContains => {
                                    let value_arg = ordered_args[0]
                                        .expect("`contains` requires a `value` argument");
                                    let actual = self.type_of_expr_hint(
                                        &value_arg.value,
                                        locals,
                                        Some(&receiver_args[0]),
                                    )?;
                                    if actual != receiver_args[0] {
                                        return Err(Diagnostic::at(
                                            value_arg.span,
                                            format!(
                                                "`contains` expects `{}`, found `{}`",
                                                receiver_args[0], actual
                                            ),
                                        ));
                                    }
                                    Ok(Type::named("bool"))
                                }
                                BuiltinMember::SetInsert | BuiltinMember::SetRemove => {
                                    if !self.is_mutable_place(object, locals)? {
                                        return Err(Diagnostic::at(
                                            span,
                                            format!(
                                                "method `{}` requires a mutable receiver",
                                                field
                                            ),
                                        ));
                                    }
                                    let value_arg = ordered_args[0]
                                        .expect("set mutation requires a `value` argument");
                                    let actual = self.type_of_expr_hint(
                                        &value_arg.value,
                                        locals,
                                        Some(&receiver_args[0]),
                                    )?;
                                    if actual != receiver_args[0] {
                                        return Err(Diagnostic::at(
                                            value_arg.span,
                                            format!(
                                                "`{}` expects `{}`, found `{}`",
                                                field, receiver_args[0], actual
                                            ),
                                        ));
                                    }
                                    if !self.is_copy_type(&receiver_args[0]) {
                                        self.consume_value_expr(&value_arg.value, locals)?;
                                    }
                                    Ok(Type::named("bool"))
                                }
                                _ => unreachable!("unexpected set builtin member"),
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
                                let callable = self.resolve_spawn_callable(&args[0].value)?;
                                self.require_spawnable_function(
                                    &callable.display_name,
                                    &callable.decl.params,
                                    args[0].span,
                                )?;
                                let spawn_args = &args[1..];
                                self.type_check_callable_args(
                                    &callable.display_name,
                                    &callable.decl.type_params,
                                    &callable.decl.params,
                                    &callable.signature.params,
                                    &callable.signature.return_type,
                                    &callable.type_param_bounds,
                                    spawn_args,
                                    span,
                                    locals,
                                    None,
                                    HashMap::new(),
                                )?;
                                return Ok(Type::Named(
                                    "Task".to_string(),
                                    vec![callable.signature.return_type.clone()],
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
                            let receiver_borrows = self.prepare_method_receiver_borrows(
                                field,
                                method.decl.receiver,
                                object,
                                span,
                                locals,
                            )?;
                            return self.type_check_callable_args_seeded(
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
                                receiver_borrows,
                            );
                        }
                    }
                }
                if let Type::TypeParam(type_param_name) = &receiver_ty {
                    if let Ok(method) = self.trait_method_from_type_param(type_param_name, field) {
                        let receiver_borrows = self.prepare_method_receiver_borrows(
                            field,
                            method.decl.receiver,
                            object,
                            span,
                            locals,
                        )?;
                        return self.type_check_callable_args_seeded(
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
                            receiver_borrows,
                        );
                    }
                }
                if let Some((_trait_impl, method, impl_substitutions)) =
                    self.trait_method_for_concrete_type(&receiver_ty, field)
                {
                    let substituted_params = method
                        .signature
                        .params
                        .iter()
                        .map(|param| substitute_type(param, &impl_substitutions))
                        .collect::<Vec<_>>();
                    let substituted_return_type =
                        substitute_type(&method.signature.return_type, &impl_substitutions);
                    let receiver_borrows = self.prepare_method_receiver_borrows(
                        field,
                        method.decl.receiver,
                        object,
                        span,
                        locals,
                    )?;
                    return self.type_check_callable_args_seeded(
                        &format!("method `{}`", field),
                        &method.decl.type_params,
                        &method.decl.params,
                        &substituted_params,
                        &substituted_return_type,
                        &method.type_param_bounds,
                        args,
                        span,
                        locals,
                        expected,
                        HashMap::new(),
                        receiver_borrows,
                    );
                }
                match (&receiver_ty, field.as_str()) {
                    (Type::Named(name, type_args), "to_string")
                        if type_args.is_empty()
                            && (is_numeric_type(&receiver_ty) || name == "bool") =>
                    {
                        BuiltinMember::ScalarToString.bind_args(args, span)?;
                        Ok(Type::named("String"))
                    }
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
            Some("Some" | "None" | "Ok" | "Err" | "Closed") => Diagnostic::at(
                span,
                "bare enum variants require an expected enum type or a qualified form such as `Result.Ok(...)`",
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

        if match_stmt.arms.is_empty() {
            return Err(Diagnostic::at(
                match_stmt.span,
                "`match` requires at least one `case` arm",
            ));
        }

        if let Some(variants) = self.enum_variants_for_type(&scrutinee_ty) {
            let Type::Named(enum_name, _type_args) = &scrutinee_ty else {
                unreachable!("enum scrutinee types should be named");
            };
            let scrutinee_enum_name = self.canonical_enum_name(enum_name);
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
                    Pattern::Literal(pattern) => {
                        return Err(Diagnostic::at(
                            pattern.span,
                            format!(
                                "match over `{}` expects enum variant patterns, not literal `{}`",
                                enum_name,
                                self.render_literal_pattern(pattern)
                            ),
                        ));
                    }
                    Pattern::Binding(binding) => {
                        return Err(Diagnostic::at(
                            binding.span,
                            "top-level binding patterns are not yet supported; use `_` or an explicit enum variant pattern",
                        ));
                    }
                    Pattern::Variant(pattern) => {
                        let pattern_enum_name = if let Some(pattern_enum_name) = &pattern.enum_name
                        {
                            if pattern_enum_name == enum_name {
                                pattern_enum_name.clone()
                            } else if let Some(pattern_enum_info) =
                                self.resolve_enum_info(pattern_enum_name)
                            {
                                pattern_enum_info.decl.name.clone()
                            } else {
                                return Err(Diagnostic::at(
                                    pattern.span,
                                    format!(
                                        "unknown enum `{}` in match pattern",
                                        pattern_enum_name
                                    ),
                                ));
                            }
                        } else {
                            scrutinee_enum_name.clone()
                        };
                        if pattern_enum_name != scrutinee_enum_name {
                            return Err(Diagnostic::at(
                                pattern.span,
                                format!(
                                    "match arm expects enum `{}`, found pattern for `{}`",
                                    scrutinee_enum_name, pattern_enum_name
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
                                    scrutinee_enum_name, pattern.variant_name
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
                                    scrutinee_enum_name, pattern.variant_name, previous
                                ),
                            ));
                        }

                        if pattern.subpatterns.is_empty() && !variant_payload.is_empty() {
                            return Err(Diagnostic::at(
                                pattern.span,
                                format!(
                                    "variant `{}.{}` carries a payload and must bind it",
                                    scrutinee_enum_name, pattern.variant_name
                                ),
                            ));
                        }
                        if variant_payload.is_empty() && !pattern.subpatterns.is_empty() {
                            return Err(Diagnostic::at(
                                pattern.span,
                                format!(
                                    "variant `{}.{}` does not carry a payload",
                                    scrutinee_enum_name, pattern.variant_name
                                ),
                            ));
                        }
                        if pattern.subpatterns.len() != variant_payload.len() {
                            return Err(Diagnostic::at(
                                pattern.span,
                                format!(
                                    "variant `{}.{}` expects {} pattern payload{}, found {}",
                                    scrutinee_enum_name,
                                    pattern.variant_name,
                                    variant_payload.len(),
                                    if variant_payload.len() == 1 { "" } else { "s" },
                                    pattern.subpatterns.len()
                                ),
                            ));
                        }
                        self.bind_pattern_locals(
                            &arm.pattern,
                            &scrutinee_ty,
                            &mut arm_locals,
                            match_stmt.borrow_mode,
                        )?;
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

            return if all_return {
                Ok(BlockFlow::AlwaysReturns)
            } else {
                Ok(BlockFlow::FallsThrough)
            };
        }

        if !matches!(scrutinee_ty, Type::Named(_, _))
            || !(is_integer_type(&scrutinee_ty)
                || is_float_type(&scrutinee_ty)
                || matches!(scrutinee_ty, Type::Named(ref name, ref args) if name == "bool" && args.is_empty())
                || is_string_type(&scrutinee_ty))
        {
            return Err(Diagnostic::at(
                match_stmt.span,
                format!(
                    "`match` currently requires an enum, bool, integer, float, or String scrutinee, found `{}`",
                    scrutinee_ty
                ),
            ));
        }

        let mut wildcard_span = None;
        let mut all_return = true;
        let mut covered_literals = BTreeMap::<LiteralPatternKey, crate::diag::Span>::new();
        let mut covered_bools = BTreeSet::<bool>::new();

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
                Pattern::Literal(pattern) => {
                    let key = self.literal_pattern_key(pattern, &scrutinee_ty)?;
                    if let Some(previous) = covered_literals.insert(key.clone(), pattern.span) {
                        return Err(Diagnostic::at(
                            pattern.span,
                            format!(
                                "duplicate match arm for literal `{}` (previously matched at {})",
                                render_literal_pattern_key(&key),
                                previous
                            ),
                        ));
                    }
                    if let LiteralPatternKey::Bool(value) = key {
                        covered_bools.insert(value);
                    }
                }
                Pattern::Variant(pattern) => {
                    return Err(Diagnostic::at(
                        pattern.span,
                        format!(
                            "match over `{}` only supports literal patterns and `_`",
                            scrutinee_ty
                        ),
                    ));
                }
                Pattern::Binding(binding) => {
                    return Err(Diagnostic::at(
                        binding.span,
                        "top-level binding patterns are not yet supported; use `_` or a literal pattern",
                    ));
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

        if wildcard_span.is_none() {
            if matches!(scrutinee_ty, Type::Named(ref name, ref args) if name == "bool" && args.is_empty())
            {
                let missing = [true, false]
                    .into_iter()
                    .filter(|value| !covered_bools.contains(value))
                    .map(|value| format!("`{}`", value))
                    .collect::<Vec<_>>();
                if !missing.is_empty() {
                    return Err(Diagnostic::at(
                        match_stmt.span,
                        format!(
                            "non-exhaustive match over `bool`: missing {}",
                            missing.join(", ")
                        ),
                    ));
                }
            } else {
                return Err(Diagnostic::at(
                    match_stmt.span,
                    format!(
                        "`match` over `{}` with literal patterns requires a final `case _:` arm",
                        scrutinee_ty
                    ),
                ));
            }
        }

        if all_return {
            Ok(BlockFlow::AlwaysReturns)
        } else {
            Ok(BlockFlow::FallsThrough)
        }
    }

    fn bind_pattern_locals(
        &self,
        pattern: &Pattern,
        expected_ty: &Type,
        locals: &mut HashMap<String, LocalBinding>,
        borrow_mode: Option<ReceiverKind>,
    ) -> Result<()> {
        match pattern {
            Pattern::Wildcard(_) => Ok(()),
            Pattern::Literal(pattern) => {
                let _ = self.literal_pattern_key(pattern, expected_ty)?;
                Ok(())
            }
            Pattern::Binding(binding) => {
                if locals.contains_key(&binding.name) {
                    return Err(Diagnostic::at(
                        binding.span,
                        format!(
                            "pattern binding `{}` would shadow an existing name",
                            binding.name
                        ),
                    ));
                }
                locals.insert(
                    binding.name.clone(),
                    LocalBinding {
                        ty: expected_ty.clone(),
                        assignable: false,
                        mutable_place: false,
                        passing: if let Some(borrow_mode) = borrow_mode {
                            if self.is_copy_type(expected_ty) {
                                ReceiverKind::Value
                            } else {
                                borrow_mode
                            }
                        } else {
                            ReceiverKind::Value
                        },
                        borrow_origin: None,
                        borrow_label: None,
                        moved: false,
                        moved_fields: BTreeSet::new(),
                    },
                );
                Ok(())
            }
            Pattern::Variant(variant_pattern) => {
                let Some(variants) = self.enum_variants_for_type(expected_ty) else {
                    return Err(Diagnostic::at(
                        variant_pattern.span,
                        format!(
                            "pattern `{}` expects an enum scrutinee, found `{}`",
                            variant_pattern.variant_name, expected_ty
                        ),
                    ));
                };
                let Type::Named(enum_name, _) = expected_ty else {
                    unreachable!("enum pattern scrutinee types should be named");
                };
                let expected_enum_name = self.canonical_enum_name(enum_name);
                let pattern_enum_name = variant_pattern
                    .enum_name
                    .as_deref()
                    .map(|name| self.canonical_enum_name(name))
                    .unwrap_or_else(|| expected_enum_name.clone());
                if pattern_enum_name != expected_enum_name {
                    return Err(Diagnostic::at(
                        variant_pattern.span,
                        format!(
                            "match arm expects enum `{}`, found pattern for `{}`",
                            expected_enum_name, pattern_enum_name
                        ),
                    ));
                }
                let Some((_, payloads)) = variants
                    .iter()
                    .find(|(name, _)| name == &variant_pattern.variant_name)
                else {
                    return Err(Diagnostic::at(
                        variant_pattern.span,
                        format!(
                            "enum `{}` has no variant `{}`",
                            enum_name, variant_pattern.variant_name
                        ),
                    ));
                };
                if variant_pattern.subpatterns.is_empty() && !payloads.is_empty() {
                    return Err(Diagnostic::at(
                        variant_pattern.span,
                        format!(
                            "variant `{}.{}` carries a payload and must bind it",
                            expected_enum_name, variant_pattern.variant_name
                        ),
                    ));
                }
                if payloads.is_empty() && !variant_pattern.subpatterns.is_empty() {
                    return Err(Diagnostic::at(
                        variant_pattern.span,
                        format!(
                            "variant `{}.{}` does not carry a payload",
                            expected_enum_name, variant_pattern.variant_name
                        ),
                    ));
                }
                if payloads.len() != variant_pattern.subpatterns.len() {
                    return Err(Diagnostic::at(
                        variant_pattern.span,
                        format!(
                            "variant `{}.{}` expects {} pattern payload{}, found {}",
                            expected_enum_name,
                            variant_pattern.variant_name,
                            payloads.len(),
                            if payloads.len() == 1 { "" } else { "s" },
                            variant_pattern.subpatterns.len()
                        ),
                    ));
                }
                for (subpattern, payload_ty) in
                    variant_pattern.subpatterns.iter().zip(payloads.iter())
                {
                    self.bind_pattern_locals(subpattern, payload_ty, locals, borrow_mode)?;
                }
                Ok(())
            }
        }
    }

    fn type_of_match_expr(
        &self,
        scrutinee: &Expr,
        borrow_mode: Option<ReceiverKind>,
        arms: &[MatchExprArm],
        span: crate::diag::Span,
        locals: &mut HashMap<String, LocalBinding>,
        expected: Option<&Type>,
    ) -> Result<Type> {
        let scrutinee_ty = self.type_of_expr(scrutinee, locals)?;
        if borrow_mode.is_none() && !self.is_copy_type(&scrutinee_ty) {
            self.consume_value_expr(scrutinee, locals)?;
        }
        if arms.is_empty() {
            return Err(Diagnostic::at(
                span,
                "`match` requires at least one `case` arm",
            ));
        }

        let mut result_ty = expected.cloned();

        if let Some(variants) = self.enum_variants_for_type(&scrutinee_ty) {
            let Type::Named(enum_name, _) = &scrutinee_ty else {
                unreachable!("enum scrutinee types should be named");
            };
            let scrutinee_enum_name = self.canonical_enum_name(enum_name);
            let mut covered = BTreeSet::<String>::new();
            let mut wildcard_seen = false;

            for (index, arm) in arms.iter().enumerate() {
                let mut arm_locals = locals.clone();
                match &arm.pattern {
                    Pattern::Wildcard(wildcard_span) => {
                        if wildcard_seen {
                            return Err(Diagnostic::at(
                                *wildcard_span,
                                "duplicate wildcard match arm",
                            ));
                        }
                        if index + 1 != arms.len() {
                            return Err(Diagnostic::at(
                                *wildcard_span,
                                "wildcard match arm must be the final `case`",
                            ));
                        }
                        wildcard_seen = true;
                    }
                    Pattern::Literal(pattern) => {
                        return Err(Diagnostic::at(
                            pattern.span,
                            format!(
                                "match over `{}` expects enum variant patterns, not literal `{}`",
                                enum_name,
                                self.render_literal_pattern(pattern)
                            ),
                        ));
                    }
                    Pattern::Binding(binding) => {
                        return Err(Diagnostic::at(
                            binding.span,
                            "top-level binding patterns are not yet supported; use `_` or an explicit enum variant pattern",
                        ));
                    }
                    Pattern::Variant(pattern) => {
                        let pattern_enum_name = if let Some(pattern_enum_name) = &pattern.enum_name
                        {
                            if pattern_enum_name == enum_name {
                                pattern_enum_name.clone()
                            } else if let Some(pattern_enum_info) =
                                self.resolve_enum_info(pattern_enum_name)
                            {
                                pattern_enum_info.decl.name.clone()
                            } else {
                                return Err(Diagnostic::at(
                                    pattern.span,
                                    format!(
                                        "unknown enum `{}` in match pattern",
                                        pattern_enum_name
                                    ),
                                ));
                            }
                        } else {
                            scrutinee_enum_name.clone()
                        };
                        if pattern_enum_name != scrutinee_enum_name {
                            return Err(Diagnostic::at(
                                pattern.span,
                                format!(
                                    "match arm expects enum `{}`, found pattern for `{}`",
                                    scrutinee_enum_name, pattern_enum_name
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
                                    scrutinee_enum_name, pattern.variant_name
                                ),
                            ));
                        };
                        covered.insert(pattern.variant_name.clone());

                        if pattern.subpatterns.is_empty() && !variant_payload.is_empty() {
                            return Err(Diagnostic::at(
                                pattern.span,
                                format!(
                                    "variant `{}.{}` carries a payload and must bind it",
                                    scrutinee_enum_name, pattern.variant_name
                                ),
                            ));
                        }
                        if variant_payload.is_empty() && !pattern.subpatterns.is_empty() {
                            return Err(Diagnostic::at(
                                pattern.span,
                                format!(
                                    "variant `{}.{}` does not carry a payload",
                                    scrutinee_enum_name, pattern.variant_name
                                ),
                            ));
                        }
                        if pattern.subpatterns.len() != variant_payload.len() {
                            return Err(Diagnostic::at(
                                pattern.span,
                                format!(
                                    "variant `{}.{}` expects {} pattern payload{}, found {}",
                                    scrutinee_enum_name,
                                    pattern.variant_name,
                                    variant_payload.len(),
                                    if variant_payload.len() == 1 { "" } else { "s" },
                                    pattern.subpatterns.len()
                                ),
                            ));
                        }
                        self.bind_pattern_locals(
                            &arm.pattern,
                            &scrutinee_ty,
                            &mut arm_locals,
                            borrow_mode,
                        )?;
                    }
                }

                let arm_ty = if let Some(expected_ty) = result_ty.as_ref() {
                    self.type_of_expr_hint(&arm.value, &mut arm_locals, Some(expected_ty))?
                } else {
                    self.type_of_expr(&arm.value, &mut arm_locals)?
                };
                if let Some(expected_ty) = result_ty.as_ref() {
                    if arm_ty != *expected_ty {
                        return Err(Diagnostic::at(
                            arm.value.span,
                            format!(
                                "match arm expression expects `{}`, found `{}`",
                                expected_ty, arm_ty
                            ),
                        ));
                    }
                } else {
                    result_ty = Some(arm_ty);
                }
            }

            let missing = variants
                .iter()
                .filter(|(name, _)| !covered.contains(name))
                .map(|(name, _)| format!("`{}`", name))
                .collect::<Vec<_>>();
            if !wildcard_seen && !missing.is_empty() {
                return Err(Diagnostic::at(
                    span,
                    format!(
                        "non-exhaustive match over `{}`: missing {}",
                        enum_name,
                        missing.join(", ")
                    ),
                ));
            }

            return Ok(result_ty.unwrap_or(Type::Unit));
        }

        if !matches!(scrutinee_ty, Type::Named(_, _))
            || !(is_integer_type(&scrutinee_ty)
                || is_float_type(&scrutinee_ty)
                || matches!(scrutinee_ty, Type::Named(ref name, ref args) if name == "bool" && args.is_empty())
                || is_string_type(&scrutinee_ty))
        {
            return Err(Diagnostic::at(
                span,
                format!(
                    "`match` currently requires an enum, bool, integer, float, or String scrutinee, found `{}`",
                    scrutinee_ty
                ),
            ));
        }

        let mut wildcard_seen = false;
        let mut covered_literals = BTreeSet::<LiteralPatternKey>::new();
        let mut covered_bools = BTreeSet::<bool>::new();

        for (index, arm) in arms.iter().enumerate() {
            let mut arm_locals = locals.clone();
            match &arm.pattern {
                Pattern::Wildcard(wildcard_span) => {
                    if wildcard_seen {
                        return Err(Diagnostic::at(
                            *wildcard_span,
                            "duplicate wildcard match arm",
                        ));
                    }
                    if index + 1 != arms.len() {
                        return Err(Diagnostic::at(
                            *wildcard_span,
                            "wildcard match arm must be the final `case`",
                        ));
                    }
                    wildcard_seen = true;
                }
                Pattern::Literal(pattern) => {
                    let key = self.literal_pattern_key(pattern, &scrutinee_ty)?;
                    covered_literals.insert(key.clone());
                    if let LiteralPatternKey::Bool(value) = key {
                        covered_bools.insert(value);
                    }
                }
                Pattern::Variant(pattern) => {
                    return Err(Diagnostic::at(
                        pattern.span,
                        format!(
                            "match over `{}` only supports literal patterns and `_`",
                            scrutinee_ty
                        ),
                    ));
                }
                Pattern::Binding(binding) => {
                    return Err(Diagnostic::at(
                        binding.span,
                        "top-level binding patterns are not yet supported; use `_` or a literal pattern",
                    ));
                }
            }

            let arm_ty = if let Some(expected_ty) = result_ty.as_ref() {
                self.type_of_expr_hint(&arm.value, &mut arm_locals, Some(expected_ty))?
            } else {
                self.type_of_expr(&arm.value, &mut arm_locals)?
            };
            if let Some(expected_ty) = result_ty.as_ref() {
                if arm_ty != *expected_ty {
                    return Err(Diagnostic::at(
                        arm.value.span,
                        format!(
                            "match arm expression expects `{}`, found `{}`",
                            expected_ty, arm_ty
                        ),
                    ));
                }
            } else {
                result_ty = Some(arm_ty);
            }
        }

        if matches!(scrutinee_ty, Type::Named(ref name, ref args) if name == "bool" && args.is_empty())
            && !wildcard_seen
            && covered_bools.len() < 2
        {
            let missing = [false, true]
                .into_iter()
                .filter(|value| !covered_bools.contains(value))
                .map(|value| format!("`{}`", value))
                .collect::<Vec<_>>();
            return Err(Diagnostic::at(
                span,
                format!("non-exhaustive bool match: missing {}", missing.join(", ")),
            ));
        }
        if !wildcard_seen
            && (is_integer_type(&scrutinee_ty)
                || is_float_type(&scrutinee_ty)
                || is_string_type(&scrutinee_ty))
        {
            return Err(Diagnostic::at(
                span,
                format!(
                    "match over `{}` requires a final wildcard arm because the domain is open-ended",
                    scrutinee_ty
                ),
            ));
        }

        Ok(result_ty.unwrap_or(Type::Unit))
    }

    fn render_literal_pattern(&self, pattern: &LiteralPattern) -> String {
        match &pattern.kind {
            LiteralPatternKind::Int(value) => value.to_string(),
            LiteralPatternKind::Float(value) => value.to_string(),
            LiteralPatternKind::Bool(value) => value.to_string(),
            LiteralPatternKind::String(value) => format!("{:?}", value),
        }
    }

    fn literal_pattern_key(
        &self,
        pattern: &LiteralPattern,
        scrutinee_ty: &Type,
    ) -> Result<LiteralPatternKey> {
        match &pattern.kind {
            LiteralPatternKind::Int(value) => {
                let Some(bounds) = integer_type_bounds(scrutinee_ty) else {
                    return Err(Diagnostic::at(
                        pattern.span,
                        format!(
                            "literal pattern `{}` does not match scrutinee type `{}`",
                            value, scrutinee_ty
                        ),
                    ));
                };
                if !value.fits_bounds(bounds) {
                    return Err(Diagnostic::at(
                        pattern.span,
                        format!(
                            "literal pattern `{}` does not fit scrutinee type `{}`",
                            value, scrutinee_ty
                        ),
                    ));
                }
                Ok(LiteralPatternKey::Int(*value))
            }
            LiteralPatternKind::Float(value) => {
                if !is_float_type(scrutinee_ty) {
                    return Err(Diagnostic::at(
                        pattern.span,
                        format!(
                            "literal pattern `{}` does not match scrutinee type `{}`",
                            value, scrutinee_ty
                        ),
                    ));
                }
                Ok(LiteralPatternKey::Float(value.to_bits()))
            }
            LiteralPatternKind::Bool(value) => {
                if !matches!(scrutinee_ty, Type::Named(name, args) if name == "bool" && args.is_empty())
                {
                    return Err(Diagnostic::at(
                        pattern.span,
                        format!(
                            "literal pattern `{}` does not match scrutinee type `{}`",
                            value, scrutinee_ty
                        ),
                    ));
                }
                Ok(LiteralPatternKey::Bool(*value))
            }
            LiteralPatternKind::String(value) => {
                if !is_string_type(scrutinee_ty) {
                    return Err(Diagnostic::at(
                        pattern.span,
                        format!(
                            "literal pattern {:?} does not match scrutinee type `{}`",
                            value, scrutinee_ty
                        ),
                    ));
                }
                Ok(LiteralPatternKey::String(value.clone()))
            }
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

        if let Some(variant_payload) = self.builtin_enum_variant_payload(object_ty, name, field) {
            return match variant_payload {
                Some(_) => Err(Diagnostic::at(
                    span,
                    format!("variant `{}` of enum `{}` requires a payload", field, name),
                )),
                None => Ok(object_ty.clone()),
            };
        }

        if BuiltinMember::resolve(name, field).is_some() {
            return Err(Diagnostic::at(
                span,
                format!(
                    "method `{}` on `{}` must be called with `(...)`",
                    field, object_ty
                ),
            ));
        }

        if name == "MapEntry" && args.len() == 2 {
            return match field {
                "key" => Ok(args[0].clone()),
                "value" => Ok(args[1].clone()),
                _ => Err(Diagnostic::at(
                    span,
                    format!("type `{}` has no field `{}`", name, field),
                )),
            };
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
        if let Some((_trait_impl, method, substitutions)) =
            self.trait_method_for_concrete_type(object_ty, field)
        {
            return Ok(substitute_type(
                &method.signature.return_type,
                &substitutions,
            ));
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
        let object_ty = self.type_of_member_object_expr(object, locals)?;
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

    fn borrow_call_place(&self, expr: &Expr) -> Option<String> {
        match &expr.kind {
            ExprKind::Name(name) => Some(name.clone()),
            ExprKind::Group(inner) => self.borrow_call_place(inner),
            ExprKind::Member { object, field } => {
                let parent = self.borrow_call_place(object)?;
                Some(format!("{}.{}", parent, field))
            }
            _ => None,
        }
    }

    fn borrow_info_for_place(
        &self,
        expr: &Expr,
        locals: &HashMap<String, LocalBinding>,
    ) -> Option<BorrowSourceInfo> {
        let place = self.borrow_call_place(expr)?;
        let root = place.split('.').next()?;
        let binding = locals.get(root)?;
        Some(BorrowSourceInfo {
            origin: binding
                .borrow_origin
                .clone()
                .unwrap_or_else(|| root.to_string()),
            borrow_label: binding.borrow_label.clone(),
            passing: binding.passing,
        })
    }

    fn expr_borrow_info(
        &self,
        expr: &Expr,
        locals: &mut HashMap<String, LocalBinding>,
    ) -> Result<Option<BorrowSourceInfo>> {
        match &expr.kind {
            ExprKind::Name(name) => Ok(locals.get(name).and_then(|binding| {
                (binding.passing != ReceiverKind::Value).then(|| BorrowSourceInfo {
                    origin: binding
                        .borrow_origin
                        .clone()
                        .unwrap_or_else(|| name.clone()),
                    borrow_label: binding.borrow_label.clone(),
                    passing: binding.passing,
                })
            })),
            ExprKind::Group(inner)
            | ExprKind::Cast { expr: inner, .. }
            | ExprKind::Specialize { expr: inner, .. } => self.expr_borrow_info(inner, locals),
            ExprKind::Member { object, .. } | ExprKind::Index { object, .. } => {
                self.expr_borrow_info(object, locals)
            }
            ExprKind::Call { callee, args } => self.call_expr_borrow_info(callee, args, locals),
            ExprKind::Match { arms, .. } => {
                let mut source = None;
                for arm in arms {
                    let arm_source = self.expr_borrow_info(&arm.value, locals)?;
                    match (&source, arm_source) {
                        (None, current) => source = current,
                        (Some(existing), Some(current))
                            if self.borrow_sources_compatible(existing, &current) => {}
                        _ => return Ok(None),
                    }
                }
                Ok(source)
            }
            _ => Ok(None),
        }
    }

    fn call_expr_borrow_info(
        &self,
        callee: &Expr,
        args: &[Argument],
        locals: &mut HashMap<String, LocalBinding>,
    ) -> Result<Option<BorrowSourceInfo>> {
        let (base_callee, _) = self.peel_specialization(callee);
        match &base_callee.kind {
            ExprKind::Name(name) => {
                let Some(function) = self.functions.get(name).or_else(|| {
                    self.current_module_namespace()
                        .and_then(|namespace| namespace.all_functions.get(name))
                }) else {
                    return Ok(None);
                };
                if function.signature.return_passing == ReceiverKind::Value {
                    return Ok(None);
                }
                let Some(source_param) = function.signature.return_borrow_source.as_deref() else {
                    return Ok(None);
                };
                let callable_params = callable_params_from_decl(&function.decl.params);
                let ordered_args = bind_call_arguments(
                    &format!("function `{}`", function.decl.name),
                    &callable_params,
                    args,
                    callee.span,
                    CallConvention::PositionalOrNamed,
                )?;
                let source_indexes = function
                    .decl
                    .params
                    .iter()
                    .enumerate()
                    .filter_map(|(index, param)| {
                        (param.name == source_param
                            || param.borrow_label.as_deref() == Some(source_param))
                        .then_some(index)
                    })
                    .collect::<Vec<_>>();
                if source_indexes.is_empty() {
                    return Ok(None);
                }
                self.bound_arguments_borrow_info(
                    &function.decl.params,
                    &ordered_args,
                    &source_indexes,
                    source_param,
                    function.signature.return_passing,
                    locals,
                )
            }
            ExprKind::Member { object, field } => {
                if let ExprKind::Name(enum_name) = &object.kind {
                    if matches!(enum_name.as_str(), "Option" | "Result" | "SendError")
                        || self.resolve_enum_info(enum_name).is_some()
                    {
                        return Ok(None);
                    }
                }
                if let Some((module_path, enum_name)) = self.qualified_module_item(object) {
                    if self
                        .module_namespace(&module_path)
                        .and_then(|namespace| namespace.enums.get(&enum_name))
                        .is_some()
                    {
                        return Ok(None);
                    }
                }
                let receiver_ty = self.type_of_expr(object, locals)?;
                let Type::Named(receiver_name, _) = &receiver_ty else {
                    return Ok(None);
                };
                let Some(class_info) = self.resolve_class_info(receiver_name) else {
                    return Ok(None);
                };
                let Some(method) = class_info.methods.get(field) else {
                    return Ok(None);
                };
                if method.signature.return_passing == ReceiverKind::Value {
                    return Ok(None);
                }
                match method.signature.return_borrow_source.as_deref() {
                    Some("self") => Ok(self
                        .expr_borrow_info(object, locals)?
                        .or_else(|| self.borrow_info_for_place(object, locals))
                        .map(|mut borrowed| {
                            borrowed.passing = method.signature.return_passing;
                            borrowed
                        })),
                    Some(source_param) => {
                        let callable_params = callable_params_from_decl(&method.decl.params);
                        let ordered_args = bind_call_arguments(
                            &format!("method `{}`", method.decl.name),
                            &callable_params,
                            args,
                            callee.span,
                            CallConvention::PositionalOrNamed,
                        )?;
                        let source_indexes = method
                            .decl
                            .params
                            .iter()
                            .enumerate()
                            .filter_map(|(index, param)| {
                                (param.name == source_param
                                    || param.borrow_label.as_deref() == Some(source_param))
                                .then_some(index)
                            })
                            .collect::<Vec<_>>();
                        if source_indexes.is_empty() {
                            return Ok(None);
                        }
                        self.bound_arguments_borrow_info(
                            &method.decl.params,
                            &ordered_args,
                            &source_indexes,
                            source_param,
                            method.signature.return_passing,
                            locals,
                        )
                    }
                    None => Ok(None),
                }
            }
            _ => Ok(None),
        }
    }

    fn bound_arguments_borrow_info(
        &self,
        params: &[Param],
        ordered_args: &[Option<&Argument>],
        source_indexes: &[usize],
        source_name: &str,
        return_passing: ReceiverKind,
        locals: &mut HashMap<String, LocalBinding>,
    ) -> Result<Option<BorrowSourceInfo>> {
        let mut combined = None;
        for index in source_indexes {
            let Some(argument) = ordered_args[*index] else {
                return Ok(None);
            };
            let param = &params[*index];
            let current = self
                .expr_borrow_info(&argument.value, locals)?
                .or_else(|| self.borrow_info_for_place(&argument.value, locals))
                .map(|mut borrowed| {
                    borrowed.passing = return_passing;
                    if param.borrow_label.as_deref() == Some(source_name) {
                        borrowed.borrow_label = Some(source_name.to_string());
                    }
                    borrowed
                });
            match (&combined, current) {
                (None, next) => combined = next,
                (Some(existing), Some(current))
                    if self.borrow_sources_compatible(existing, &current) => {}
                _ => return Ok(None),
            }
        }
        Ok(combined)
    }

    fn borrow_source_matches(&self, expected: &str, actual: &BorrowSourceInfo) -> bool {
        actual.origin == expected || actual.borrow_label.as_deref() == Some(expected)
    }

    fn borrow_sources_compatible(&self, left: &BorrowSourceInfo, right: &BorrowSourceInfo) -> bool {
        left == right
            || (left.passing == right.passing
                && left.borrow_label.is_some()
                && left.borrow_label == right.borrow_label)
    }

    fn prepare_method_receiver_borrows(
        &self,
        method_name: &str,
        receiver_kind: Option<ReceiverKind>,
        object: &Expr,
        span: crate::diag::Span,
        locals: &mut HashMap<String, LocalBinding>,
    ) -> Result<Vec<BorrowedCallPlace>> {
        let Some(receiver_kind) = receiver_kind else {
            return Ok(Vec::new());
        };

        if receiver_kind == ReceiverKind::BorrowMut && !self.is_mutable_place(object, locals)? {
            return Err(Diagnostic::at(
                span,
                format!("method `{}` requires a mutable receiver", method_name),
            ));
        }

        if receiver_kind == ReceiverKind::Value {
            self.consume_value_expr(object, locals)?;
            return Ok(Vec::new());
        }

        let mut borrowed_places = Vec::new();
        if let Some(place) = self.borrow_call_place(object) {
            borrowed_places.push(BorrowedCallPlace {
                path: place,
                passing: receiver_kind,
                param_name: "self".to_string(),
            });
        }
        Ok(borrowed_places)
    }

    fn reject_overlapping_borrow(
        &self,
        borrowed_places: &[BorrowedCallPlace],
        current_path: &str,
        current_passing: ReceiverKind,
        current_param_name: &str,
        callee_name: &str,
        span: crate::diag::Span,
    ) -> Result<()> {
        for prior in borrowed_places {
            if !borrow_places_overlap(&prior.path, current_path) {
                continue;
            }
            match (prior.passing, current_passing) {
                (ReceiverKind::Borrow, ReceiverKind::Borrow) => continue,
                (ReceiverKind::BorrowMut, ReceiverKind::BorrowMut) => {
                    return Err(Diagnostic::at(
                        span,
                        format!(
                            "argument for parameter `{}` in {} overlaps mutable borrow for parameter `{}`",
                            current_param_name, callee_name, prior.param_name
                        ),
                    ));
                }
                (ReceiverKind::Borrow, ReceiverKind::BorrowMut) => {
                    return Err(Diagnostic::at(
                        span,
                        format!(
                            "argument for parameter `{}` in {} overlaps borrow for parameter `{}`; mutable borrows must be exclusive",
                            current_param_name, callee_name, prior.param_name
                        ),
                    ));
                }
                (ReceiverKind::BorrowMut, ReceiverKind::Borrow) => {
                    return Err(Diagnostic::at(
                        span,
                        format!(
                            "argument for parameter `{}` in {} overlaps mutable borrow for parameter `{}`; mutable borrows must be exclusive",
                            current_param_name, callee_name, prior.param_name
                        ),
                    ));
                }
                _ => {}
            }
        }
        Ok(())
    }

    fn render_member_target(&self, object: &Expr, field: &str) -> String {
        format!("{}.{}", self.render_place_expr(object), field)
    }

    fn render_index_target(&self, object: &Expr) -> String {
        format!("{}[..]", self.render_place_expr(object))
    }

    fn render_place_expr(&self, expr: &Expr) -> String {
        match &expr.kind {
            ExprKind::Name(name) => name.clone(),
            ExprKind::Group(inner) => self.render_place_expr(inner),
            ExprKind::Member { object, field } => {
                format!("{}.{}", self.render_place_expr(object), field)
            }
            ExprKind::Index { object, .. } => {
                format!("{}[..]", self.render_place_expr(object))
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
            ExprKind::Index { object, .. } => self.borrowed_root_binding_name(object, locals),
            _ => None,
        }
    }

    fn member_access_path(&self, expr: &Expr) -> Option<(String, String)> {
        match &expr.kind {
            ExprKind::Name(name) => Some((name.clone(), String::new())),
            ExprKind::Group(inner)
            | ExprKind::Cast { expr: inner, .. }
            | ExprKind::Specialize { expr: inner, .. } => self.member_access_path(inner),
            ExprKind::Member { object, field } => {
                let (root, prefix) = self.member_access_path(object)?;
                let path = if prefix.is_empty() {
                    field.clone()
                } else {
                    format!("{}.{}", prefix, field)
                };
                Some((root, path))
            }
            _ => None,
        }
    }

    fn member_target_path(&self, object: &Expr, field: &str) -> Option<(String, String)> {
        let (root, prefix) = self.member_access_path(object)?;
        let path = if prefix.is_empty() {
            field.to_string()
        } else {
            format!("{}.{}", prefix, field)
        };
        Some((root, path))
    }

    fn field_path_is_moved(binding: &LocalBinding, path: &str) -> bool {
        binding.moved_fields.iter().any(|moved_path| {
            moved_path == path
                || moved_path.starts_with(&format!("{}.", path))
                || path.starts_with(&format!("{}.", moved_path))
        })
    }

    fn clear_moved_field_path(binding: &mut LocalBinding, path: &str) {
        binding.moved_fields.retain(|moved_path| {
            moved_path != path && !moved_path.starts_with(&format!("{}.", path))
        });
    }

    fn type_of_member_object_expr(
        &self,
        expr: &Expr,
        locals: &mut HashMap<String, LocalBinding>,
    ) -> Result<Type> {
        match &expr.kind {
            ExprKind::Name(name) => {
                let binding = locals
                    .get(name)
                    .ok_or_else(|| Diagnostic::at(expr.span, format!("unknown name `{}`", name)))?;
                if binding.moved {
                    return Err(Diagnostic::at(
                        expr.span,
                        format!("use of moved value `{}`", name),
                    ));
                }
                Ok(binding.ty.clone())
            }
            ExprKind::Group(inner)
            | ExprKind::Cast { expr: inner, .. }
            | ExprKind::Specialize { expr: inner, .. } => {
                self.type_of_member_object_expr(inner, locals)
            }
            ExprKind::Member { object, field } => {
                let object_ty = self.type_of_member_object_expr(object, locals)?;
                self.resolve_member_type(&object_ty, field, expr.span)
            }
            ExprKind::Index { .. } => self.type_of_expr(expr, locals),
            _ => self.type_of_expr(expr, locals),
        }
    }

    fn module_namespace(&self, path: &str) -> Option<&ModuleNamespace> {
        if let Some(namespace) = self.module_registry.get(path) {
            return Some(namespace);
        }
        self.current_module_namespace()
            .and_then(|current| find_namespace_in_modules(&current.imported_modules, path))
            .or_else(|| find_namespace_in_modules(&self.imported_modules, path))
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
            ExprKind::Index { object, .. } => self.infer_module_path(object),
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
            ExprKind::Index { object, .. } => self.qualified_module_item(object),
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

    fn canonical_enum_name(&self, name: &str) -> String {
        self.resolve_enum_info(name)
            .map(|enum_info| enum_info.decl.name.clone())
            .unwrap_or_else(|| {
                name.rsplit_once('.')
                    .map(|(_, leaf)| leaf.to_string())
                    .unwrap_or_else(|| name.to_string())
            })
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

    fn trait_impl_substitutions(
        &self,
        trait_impl: &TraitImplInfo,
        actual: &Type,
    ) -> Option<HashMap<String, Type>> {
        let type_params = trait_impl
            .type_params
            .iter()
            .cloned()
            .collect::<BTreeSet<_>>();
        let mut substitutions = HashMap::new();
        if !type_pattern_matches(
            &trait_impl.for_type,
            actual,
            &type_params,
            &mut substitutions,
        ) {
            return None;
        }
        for (type_param, bounds) in &trait_impl.type_param_bounds {
            let actual_ty = substitutions.get(type_param)?;
            for bound in bounds {
                let resolved_bound = substitute_trait_bound(bound, &substitutions);
                if !self.type_implements_trait_bound(actual_ty, &resolved_bound) {
                    return None;
                }
            }
        }
        Some(substitutions)
    }

    fn trait_impl_substitutions_for_bound(
        &self,
        trait_impl: &TraitImplInfo,
        actual: &Type,
        bound: &TraitBound,
    ) -> Option<HashMap<String, Type>> {
        if trait_impl.trait_name != bound.trait_name
            || trait_impl.trait_args.len() != bound.trait_args.len()
        {
            return None;
        }
        let mut type_params = BTreeSet::new();
        collect_type_params_from_type(&trait_impl.for_type, &mut type_params);
        for trait_arg in &trait_impl.trait_args {
            collect_type_params_from_type(trait_arg, &mut type_params);
        }
        let mut substitutions = HashMap::new();
        if !type_pattern_matches(
            &trait_impl.for_type,
            actual,
            &type_params,
            &mut substitutions,
        ) {
            return None;
        }
        for (pattern, actual_arg) in trait_impl.trait_args.iter().zip(&bound.trait_args) {
            if !type_pattern_matches(pattern, actual_arg, &type_params, &mut substitutions) {
                return None;
            }
        }
        for (type_param, bounds) in &trait_impl.type_param_bounds {
            let actual_ty = substitutions.get(type_param)?;
            for impl_bound in bounds {
                let resolved_bound = substitute_trait_bound(impl_bound, &substitutions);
                if !self.type_implements_trait_bound(actual_ty, &resolved_bound) {
                    return None;
                }
            }
        }
        Some(substitutions)
    }

    fn type_implements_trait_bound(&self, ty: &Type, bound: &TraitBound) -> bool {
        self.trait_impls_in_scope().any(|trait_impl| {
            self.trait_impl_substitutions_for_bound(trait_impl, ty, bound)
                .or_else(|| {
                    if bound.trait_args.is_empty() && trait_impl.trait_name == bound.trait_name {
                        self.trait_impl_substitutions(trait_impl, ty)
                    } else {
                        None
                    }
                })
                .is_some()
        })
    }

    fn assert_type_satisfies_bounds(
        &self,
        ty: &Type,
        bounds: &[TraitBound],
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
                    if !self.type_implements_trait_bound(ty, bound) {
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
    ) -> Result<ResolvedTraitMethodInfo> {
        let mut matches = Vec::new();
        for bound in self
            .type_param_bounds
            .get(type_param_name)
            .into_iter()
            .flatten()
        {
            if let Some(trait_info) = self.traits.get(&bound.trait_name) {
                if let Some(method) = trait_info.methods.get(method_name) {
                    let trait_substitutions = self_type_substitutions(
                        &trait_info.decl,
                        &bound.trait_args,
                        Type::TypeParam(type_param_name.to_string()),
                    );
                    matches.push(ResolvedTraitMethodInfo {
                        decl: method.decl.clone(),
                        signature: FunctionSignature {
                            params: method
                                .signature
                                .params
                                .iter()
                                .map(|param| substitute_type(param, &trait_substitutions))
                                .collect(),
                            return_type: substitute_type(
                                &method.signature.return_type,
                                &trait_substitutions,
                            ),
                            return_passing: method.signature.return_passing,
                            return_borrow_source: method.signature.return_borrow_source.clone(),
                        },
                        type_param_bounds: substitute_trait_bounds(
                            &method.type_param_bounds,
                            &trait_substitutions,
                        ),
                    });
                }
            }
        }
        match matches.len() {
            1 => Ok(matches.remove(0)),
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
    ) -> Option<(&TraitImplInfo, &TraitImplMethodInfo, HashMap<String, Type>)> {
        self.trait_impls_in_scope()
            .filter_map(|trait_impl| {
                self.trait_impl_substitutions(trait_impl, ty)
                    .map(|substitutions| (trait_impl, substitutions))
            })
            .find_map(|(trait_impl, substitutions)| {
                trait_impl
                    .methods
                    .get(method_name)
                    .map(|method| (trait_impl, method, substitutions))
            })
    }

    fn has_from_conversion(&self, source_ty: &Type, target_ty: &Type) -> bool {
        self.trait_impls_in_scope().any(|trait_impl| {
            if trait_impl.trait_name != "From" || trait_impl.trait_args.len() != 1 {
                return false;
            }
            if !trait_impl.methods.contains_key("from") {
                return false;
            }
            let Some(substitutions) = self.trait_impl_substitutions(trait_impl, target_ty) else {
                return false;
            };
            substitute_type(&trait_impl.trait_args[0], &substitutions) == *source_ty
        })
    }

    fn resolve_spawn_callable(&self, callee: &Expr) -> Result<ResolvedCallableInfo> {
        let base_callee = match &callee.kind {
            ExprKind::Specialize { expr, .. } => &**expr,
            _ => callee,
        };

        match &base_callee.kind {
            ExprKind::Name(function_name) => {
                let function = self.functions.get(function_name).ok_or_else(|| {
                    Diagnostic::at(
                        callee.span,
                        format!(
                            "spawn target must be a callable function, found `{}`",
                            function_name
                        ),
                    )
                })?;
                Ok(ResolvedCallableInfo {
                    display_name: function_name.clone(),
                    decl: function.decl.clone(),
                    signature: function.signature.clone(),
                    type_param_bounds: function.type_param_bounds.clone(),
                })
            }
            ExprKind::Member { object, field } => {
                if let Some((module_path, item_name)) = self.qualified_module_item(object) {
                    if let Some(namespace) = self.module_namespace(&module_path) {
                        if let Some(class_info) = namespace.classes.get(&item_name) {
                            if let Some(method) = class_info.methods.get(field) {
                                if method.decl.receiver.is_none() {
                                    return Ok(ResolvedCallableInfo {
                                        display_name: format!("{}.{}", item_name, field),
                                        decl: method.decl.clone(),
                                        signature: method.signature.clone(),
                                        type_param_bounds: method.type_param_bounds.clone(),
                                    });
                                }
                            }
                        }
                    }
                }

                if let Some((module_path, function_name)) = self.qualified_module_item(callee) {
                    if let Some(namespace) = self.module_namespace(&module_path) {
                        if let Some(function) = namespace
                            .functions
                            .get(&function_name)
                            .or_else(|| namespace.all_functions.get(&function_name))
                        {
                            return Ok(ResolvedCallableInfo {
                                display_name: format!("{}.{}", module_path, function_name),
                                decl: function.decl.clone(),
                                signature: function.signature.clone(),
                                type_param_bounds: function.type_param_bounds.clone(),
                            });
                        }
                    }
                }

                let base_object = match &object.kind {
                    ExprKind::Specialize { expr, .. } => &**expr,
                    _ => &**object,
                };
                if let ExprKind::Name(class_name) = &base_object.kind {
                    if let Some(class_info) = self.resolve_class_info(class_name) {
                        if let Some(method) = class_info.methods.get(field) {
                            if method.decl.receiver.is_none() {
                                return Ok(ResolvedCallableInfo {
                                    display_name: format!("{}.{}", class_name, field),
                                    decl: method.decl.clone(),
                                    signature: method.signature.clone(),
                                    type_param_bounds: method.type_param_bounds.clone(),
                                });
                            }
                        }
                    }
                }

                Err(Diagnostic::at(
                    callee.span,
                    "`spawn` currently supports named functions and associated methods without `self`",
                ))
            }
            _ => Err(Diagnostic::at(
                callee.span,
                "`spawn` currently supports named functions and associated methods without `self`",
            )),
        }
    }

    fn type_check_callable_args(
        &self,
        callee_name: &str,
        callee_type_params: &[String],
        param_decls: &[Param],
        param_types: &[Type],
        return_type: &Type,
        callee_type_param_bounds: &BTreeMap<String, Vec<TraitBound>>,
        args: &[Argument],
        span: crate::diag::Span,
        locals: &mut HashMap<String, LocalBinding>,
        expected_return: Option<&Type>,
        seed_substitutions: HashMap<String, Type>,
    ) -> Result<Type> {
        self.type_check_callable_args_seeded(
            callee_name,
            callee_type_params,
            param_decls,
            param_types,
            return_type,
            callee_type_param_bounds,
            args,
            span,
            locals,
            expected_return,
            seed_substitutions,
            Vec::new(),
        )
    }

    fn type_check_callable_args_seeded(
        &self,
        callee_name: &str,
        callee_type_params: &[String],
        param_decls: &[Param],
        param_types: &[Type],
        return_type: &Type,
        callee_type_param_bounds: &BTreeMap<String, Vec<TraitBound>>,
        args: &[Argument],
        span: crate::diag::Span,
        locals: &mut HashMap<String, LocalBinding>,
        expected_return: Option<&Type>,
        seed_substitutions: HashMap<String, Type>,
        seeded_borrowed_places: Vec<BorrowedCallPlace>,
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
                match self.type_of_expr_hint(&argument.value, locals, Some(&hinted_expected)) {
                    Ok(actual) => actual,
                    Err(error) if has_unresolved_type_params(&hinted_expected) => {
                        match self.type_of_expr(&argument.value, locals) {
                            Ok(actual) => actual,
                            Err(_) => return Err(error),
                        }
                    }
                    Err(error) => return Err(error),
                }
            } else {
                let default = param_decl
                    .default
                    .as_ref()
                    .expect("optional parameter should provide a default expression");
                match self.type_of_expr_hint(default, locals, Some(&hinted_expected)) {
                    Ok(actual) => actual,
                    Err(error) if has_unresolved_type_params(&hinted_expected) => {
                        match self.type_of_expr(default, locals) {
                            Ok(actual) => actual,
                            Err(_) => return Err(error),
                        }
                    }
                    Err(error) => return Err(error),
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
            let resolved_bounds = bounds
                .iter()
                .map(|bound| substitute_trait_bound(bound, &substitutions))
                .collect::<Vec<_>>();
            self.assert_type_satisfies_bounds(resolved_ty, &resolved_bounds, span)?;
        }

        let mut borrowed_places = seeded_borrowed_places;
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
                    ReceiverKind::Borrow => {
                        if let Some(place) = self.borrow_call_place(&argument.value) {
                            self.reject_overlapping_borrow(
                                &borrowed_places,
                                &place,
                                ReceiverKind::Borrow,
                                &param_decl.name,
                                callee_name,
                                argument.span,
                            )?;
                            borrowed_places.push(BorrowedCallPlace {
                                path: place,
                                passing: ReceiverKind::Borrow,
                                param_name: param_decl.name.clone(),
                            });
                        }
                    }
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
                        if let Some(place) = self.borrow_call_place(&argument.value) {
                            self.reject_overlapping_borrow(
                                &borrowed_places,
                                &place,
                                ReceiverKind::BorrowMut,
                                &param_decl.name,
                                callee_name,
                                argument.span,
                            )?;
                            borrowed_places.push(BorrowedCallPlace {
                                path: place,
                                passing: ReceiverKind::BorrowMut,
                                param_name: param_decl.name.clone(),
                            });
                        }
                    }
                }
            }
        }

        Ok(substitute_type(return_type, &substitutions))
    }

    fn enum_variants_for_type(&self, ty: &Type) -> Option<Vec<(String, Vec<Type>)>> {
        match ty {
            Type::Named(name, args) if name == "Option" && args.len() == 1 => Some(vec![
                ("Some".to_string(), vec![args[0].clone()]),
                ("None".to_string(), Vec::new()),
            ]),
            Type::Named(name, args) if name == "Result" && args.len() == 2 => Some(vec![
                ("Ok".to_string(), vec![args[0].clone()]),
                ("Err".to_string(), vec![args[1].clone()]),
            ]),
            Type::Named(name, args) if name == "SendError" && args.len() == 1 => {
                Some(vec![("Closed".to_string(), vec![args[0].clone()])])
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
                            enum_info
                                .variants
                                .get(&variant.name)
                                .map(|info| {
                                    info.payloads
                                        .iter()
                                        .map(|payload| substitute_type(&payload.ty, &substitutions))
                                        .collect::<Vec<_>>()
                                })
                                .unwrap_or_default(),
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

    fn explicit_builtin_type(
        &self,
        name: &str,
        explicit_args: &[Type],
        span: crate::diag::Span,
    ) -> Result<Type> {
        let expected_len = match name {
            "Option" => 1,
            "Result" => 2,
            "SendError" => 1,
            _ => return Err(Diagnostic::at(span, format!("unknown name `{}`", name))),
        };
        if explicit_args.len() != expected_len {
            return Err(Diagnostic::at(
                span,
                format!(
                    "enum `{}` expects {} type argument{}, found {}",
                    name,
                    expected_len,
                    if expected_len == 1 { "" } else { "s" },
                    explicit_args.len()
                ),
            ));
        }
        Ok(Type::Named(name.to_string(), explicit_args.to_vec()))
    }

    fn expr_can_use_partial_expected_hint(&self, expr: &Expr) -> bool {
        match &expr.kind {
            ExprKind::Member { object, .. } => self.is_builtin_enum_constructor_expr(object),
            ExprKind::Call { callee, .. } => match &callee.kind {
                ExprKind::Member { object, .. } => self.is_builtin_enum_constructor_expr(object),
                _ => false,
            },
            ExprKind::Group(inner) => self.expr_can_use_partial_expected_hint(inner),
            _ => false,
        }
    }

    fn is_builtin_enum_constructor_expr(&self, expr: &Expr) -> bool {
        match &expr.kind {
            ExprKind::Name(name) => matches!(name.as_str(), "Option" | "Result" | "SendError"),
            ExprKind::Specialize { expr, .. } => self.is_builtin_enum_constructor_expr(expr),
            ExprKind::Group(inner) => self.is_builtin_enum_constructor_expr(inner),
            _ => false,
        }
    }

    fn type_check_builtin_enum_variant_constructor(
        &self,
        enum_name: &str,
        variant_name: &str,
        enum_ty: &Type,
        args: &[Argument],
        span: crate::diag::Span,
        locals: &mut HashMap<String, LocalBinding>,
    ) -> Result<Type> {
        let Some(variant_payload) =
            self.builtin_enum_variant_payload(enum_ty, enum_name, variant_name)
        else {
            return Err(Diagnostic::at(
                span,
                format!("enum `{}` has no variant `{}`", enum_name, variant_name),
            ));
        };
        match variant_payload {
            Some(payload_ty) => {
                let argument =
                    self.variant_payload_argument(args, span, variant_name, enum_name)?;
                let actual = self.type_of_expr_hint(&argument.value, locals, Some(&payload_ty))?;
                if actual != payload_ty {
                    return Err(Diagnostic::at(
                        argument.span,
                        format!(
                            "variant `{}` of enum `{}` expects `{}`, found `{}`",
                            variant_name, enum_name, payload_ty, actual
                        ),
                    ));
                }
                if !self.is_copy_type(&payload_ty) {
                    self.consume_value_expr(&argument.value, locals)?;
                }
            }
            None => {
                if !args.is_empty() {
                    return Err(Diagnostic::at(
                        span,
                        format!(
                            "variant `{}` of enum `{}` does not take a payload",
                            variant_name, enum_name
                        ),
                    ));
                }
            }
        }
        Ok(enum_ty.clone())
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

    fn variant_payload_argument<'b>(
        &self,
        args: &'b [Argument],
        span: crate::diag::Span,
        variant_name: &str,
        enum_name: &str,
    ) -> Result<&'b Argument> {
        if args.len() != 1 {
            return Err(Diagnostic::at(
                span,
                format!(
                    "variant `{}` of enum `{}` expects exactly one payload argument",
                    variant_name, enum_name
                ),
            ));
        }
        if let Some(name) = args[0].name.as_deref() {
            if name != "value" {
                return Err(Diagnostic::at(
                    args[0].span,
                    format!(
                        "variant `{}` of enum `{}` only accepts the keyword `value=`",
                        variant_name, enum_name
                    ),
                ));
            }
        }
        Ok(&args[0])
    }

    fn variant_payload_arguments<'b>(
        &self,
        args: &'b [Argument],
        span: crate::diag::Span,
        variant_name: &str,
        enum_name: &str,
        payloads: &[EnumPayloadFieldInfo],
        named_payloads: bool,
    ) -> Result<Vec<&'b Argument>> {
        if payloads.is_empty() {
            if !args.is_empty() {
                return Err(Diagnostic::at(
                    span,
                    format!(
                        "variant `{}` of enum `{}` does not take a payload",
                        variant_name, enum_name
                    ),
                ));
            }
            return Ok(Vec::new());
        }

        let uses_named_args = args.iter().any(|argument| argument.name.is_some());
        if uses_named_args {
            if payloads.len() == 1 && !named_payloads {
                let argument =
                    self.variant_payload_argument(args, span, variant_name, enum_name)?;
                if let Some(name) = argument.name.as_deref() {
                    if name != "value" {
                        return Err(Diagnostic::at(
                            argument.span,
                            format!(
                                "single-payload variant `{}` of enum `{}` only accepts named argument `value=`",
                                variant_name, enum_name
                            ),
                        ));
                    }
                }
                return Ok(vec![argument]);
            }
            if !named_payloads {
                return Err(Diagnostic::at(
                    span,
                    format!(
                        "variant `{}` of enum `{}` uses positional payloads and cannot be constructed with named arguments",
                        variant_name, enum_name
                    ),
                ));
            }
            if args.len() != payloads.len() {
                return Err(Diagnostic::at(
                    span,
                    format!(
                        "variant `{}` of enum `{}` expects {} payload argument{}, found {}",
                        variant_name,
                        enum_name,
                        payloads.len(),
                        if payloads.len() == 1 { "" } else { "s" },
                        args.len()
                    ),
                ));
            }
            let mut ordered = Vec::with_capacity(payloads.len());
            let mut used = BTreeSet::new();
            for payload in payloads {
                let payload_name = payload
                    .name
                    .as_deref()
                    .expect("named enum payloads should have names");
                let argument = args
                    .iter()
                    .find(|argument| argument.name.as_deref() == Some(payload_name))
                    .ok_or_else(|| {
                        Diagnostic::at(
                            span,
                            format!(
                                "variant `{}` of enum `{}` is missing payload argument `{}`",
                                variant_name, enum_name, payload_name
                            ),
                        )
                    })?;
                used.insert(payload_name.to_string());
                ordered.push(argument);
            }
            if let Some(extra) = args
                .iter()
                .filter_map(|argument| argument.name.as_deref())
                .find(|name| !used.contains(*name))
            {
                return Err(Diagnostic::at(
                    span,
                    format!(
                        "variant `{}` of enum `{}` has no payload named `{}`",
                        variant_name, enum_name, extra
                    ),
                ));
            }
            return Ok(ordered);
        }

        if args.len() != payloads.len() {
            return Err(Diagnostic::at(
                span,
                format!(
                    "variant `{}` of enum `{}` expects {} payload argument{}, found {}",
                    variant_name,
                    enum_name,
                    payloads.len(),
                    if payloads.len() == 1 { "" } else { "s" },
                    args.len()
                ),
            ));
        }
        Ok(args.iter().collect())
    }
}

#[cfg(test)]
#[path = "sema_tests.rs"]
mod tests;
