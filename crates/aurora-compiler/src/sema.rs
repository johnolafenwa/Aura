use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fmt;
use std::rc::Rc;

use serde::{Deserialize, Serialize};

use crate::ast::{
    Argument, AssignStmt, AssignTarget, BinaryOp, ClassDecl, CompareLink, EnumDecl, Expr, ExprKind,
    FunctionDecl, ImplDecl, Item, LiteralPattern, LiteralPatternKind, MatchExprArm, MatchStmt,
    Module, Param, ParamMode, Pattern, ReceiverKind, Stmt, TraitDecl, TypeRef, UnaryOp,
    VariantPattern, WithStmt,
};
use crate::call::{
    bind_call_arguments, callable_params_from_decl, BuiltinAssociatedFunction,
    BuiltinClassConstructor, BuiltinFunction, BuiltinMember, CallConvention,
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
    /// Canonical nominal identities for names visible in the checked module.
    /// Imported aliases map to their defining module, while local names map
    /// to themselves. Tooling uses this to mirror checker type identity.
    pub canonical_type_names: BTreeMap<String, String>,
    pub top_level_stmts: Vec<Stmt>,
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

type TraitMethodMatch<'a> = (
    &'a TraitImplInfo,
    &'a TraitImplMethodInfo,
    HashMap<String, Type>,
);

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
        BinaryOp::FloorDiv => Some(("FloorDiv", "floor_div")),
        BinaryOp::Mod => Some(("Mod", "mod")),
        BinaryOp::Less => Some(("Ord", "lt")),
        BinaryOp::LessEq => Some(("Ord", "le")),
        BinaryOp::Greater => Some(("Ord", "gt")),
        BinaryOp::GreaterEq => Some(("Ord", "ge")),
        BinaryOp::And | BinaryOp::Or | BinaryOp::Eq | BinaryOp::NotEq => None,
    }
}

pub(crate) fn is_duration_type(ty: &Type) -> bool {
    *ty == Type::named("Duration")
}

pub(crate) fn builtin_duration_binary_result(
    op: BinaryOp,
    left_ty: &Type,
    right_ty: &Type,
) -> Option<Type> {
    let duration = Type::named("Duration");
    let int64 = Type::named("int64");
    match op {
        BinaryOp::Add | BinaryOp::Sub if left_ty == &duration && right_ty == &duration => {
            Some(duration)
        }
        BinaryOp::Mul
            if (left_ty == &duration && right_ty == &int64)
                || (left_ty == &int64 && right_ty == &duration) =>
        {
            Some(duration)
        }
        BinaryOp::FloorDiv if left_ty == &duration && right_ty == &int64 => Some(duration),
        BinaryOp::Eq
        | BinaryOp::NotEq
        | BinaryOp::Less
        | BinaryOp::LessEq
        | BinaryOp::Greater
        | BinaryOp::GreaterEq
            if left_ty == &duration && right_ty == &duration =>
        {
            Some(Type::named("bool"))
        }
        _ => None,
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
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum Type {
    Named(String, Vec<Type>),
    Tuple(Vec<Type>),
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
            Type::Tuple(elements) => elements.iter().all(Type::is_copy),
            Type::Named(name, args) => is_builtin_copy_named_type(name, args),
        }
    }
}

fn is_builtin_copy_named_type(name: &str, args: &[Type]) -> bool {
    match name {
        "Queue" | "Task" => args.len() == 1,
        _ => {
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
    }
}

/// Maps a written parameter capability to its passing convention.
///
/// ADR-0022 Q1 ratifies universal logical sharing: bare means shared access
/// for every type, including declaration-known copy types. The ABI may still
/// pass copied bits, but the source-level shared-loan and ADR-0016 sequencing
/// rules apply uniformly. That is what keeps this mapping declaration-stable,
/// which generic trait specializations and builtin signatures depend on.
pub(crate) fn resolve_param_passing(mode: ParamMode) -> ReceiverKind {
    match mode {
        ParamMode::Default => ReceiverKind::Borrow,
        ParamMode::Own => ReceiverKind::Value,
        ParamMode::BorrowMut => ReceiverKind::BorrowMut,
    }
}

fn resolve_param_passings(params: &[Param]) -> Vec<ReceiverKind> {
    params
        .iter()
        .map(|param| resolve_param_passing(param.mode))
        .collect()
}

#[cfg(test)]
fn type_is_copy_in_context(
    ty: &Type,
    classes: &BTreeMap<String, ClassInfo>,
    enums: &BTreeMap<String, EnumInfo>,
) -> bool {
    type_is_copy_in_context_inner(ty, classes, enums, None, None, &mut BTreeSet::new())
}

fn type_is_copy_in_context_with_modules(
    ty: &Type,
    classes: &BTreeMap<String, ClassInfo>,
    enums: &BTreeMap<String, EnumInfo>,
    imported_modules: &BTreeMap<String, ModuleNamespace>,
    module_registry: &BTreeMap<String, ModuleNamespace>,
) -> bool {
    type_is_copy_in_context_inner(
        ty,
        classes,
        enums,
        Some(imported_modules),
        Some(module_registry),
        &mut BTreeSet::new(),
    )
}

pub(crate) fn type_is_copy_in_program(ty: &Type, program: &Program) -> bool {
    type_is_copy_in_context_with_modules(
        ty,
        &program.classes,
        &program.enums,
        &program.imported_modules,
        &program.module_registry,
    )
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
enum RngCloneSafety {
    Safe,
    ContainsRng,
    Unknown,
}

impl RngCloneSafety {
    fn combine(self, other: Self) -> Self {
        match (self, other) {
            (Self::ContainsRng, _) | (_, Self::ContainsRng) => Self::ContainsRng,
            (Self::Unknown, _) | (_, Self::Unknown) => Self::Unknown,
            _ => Self::Safe,
        }
    }
}

fn rng_clone_safety_in_context_with_modules(
    ty: &Type,
    classes: &BTreeMap<String, ClassInfo>,
    enums: &BTreeMap<String, EnumInfo>,
    imported_modules: &BTreeMap<String, ModuleNamespace>,
    module_registry: &BTreeMap<String, ModuleNamespace>,
) -> RngCloneSafety {
    rng_clone_safety_in_context_inner(
        ty,
        classes,
        enums,
        imported_modules,
        module_registry,
        &mut BTreeSet::new(),
    )
}

fn rng_clone_safety_in_context_inner(
    ty: &Type,
    classes: &BTreeMap<String, ClassInfo>,
    enums: &BTreeMap<String, EnumInfo>,
    imported_modules: &BTreeMap<String, ModuleNamespace>,
    module_registry: &BTreeMap<String, ModuleNamespace>,
    visiting: &mut BTreeSet<String>,
) -> RngCloneSafety {
    let Type::Named(name, args) = ty else {
        return match ty {
            Type::TypeParam(_) => RngCloneSafety::Unknown,
            Type::Unit | Type::Module(_) => RngCloneSafety::Safe,
            Type::Tuple(elements) => {
                elements
                    .iter()
                    .fold(RngCloneSafety::Safe, |safety, element| {
                        safety.combine(rng_clone_safety_in_context_inner(
                            element,
                            classes,
                            enums,
                            imported_modules,
                            module_registry,
                            visiting,
                        ))
                    })
            }
            Type::Named(_, _) => unreachable!(),
        };
    };
    if matches!(name.as_str(), "Queue" | "Task") && args.len() == 1 {
        return RngCloneSafety::Safe;
    }

    if let Some(class_info) = classes
        .get(name)
        .or_else(|| copy_class_info_from_modules(name, imported_modules, module_registry))
    {
        if class_info.is_builtin
            && class_info.module_name == "random"
            && class_info.decl.name == "Rng"
            && args.is_empty()
        {
            return RngCloneSafety::ContainsRng;
        }
        if args.len() != class_info.decl.type_params.len() {
            return RngCloneSafety::Unknown;
        }
        let key = format!(
            "class:{}:{}:{}",
            class_info.is_builtin, class_info.module_name, class_info.decl.name
        );
        if !visiting.insert(key.clone()) {
            return args.iter().fold(RngCloneSafety::Safe, |safety, arg| {
                safety.combine(rng_clone_safety_in_context_inner(
                    arg,
                    classes,
                    enums,
                    imported_modules,
                    module_registry,
                    visiting,
                ))
            });
        }
        let substitutions = substitutions_from_decl_type_args(&class_info.decl.type_params, args);
        let safety = class_info
            .fields
            .values()
            .map(|field| substitute_type(&field.ty, &substitutions))
            .fold(RngCloneSafety::Safe, |safety, field_ty| {
                safety.combine(rng_clone_safety_in_context_inner(
                    &field_ty,
                    classes,
                    enums,
                    imported_modules,
                    module_registry,
                    visiting,
                ))
            });
        visiting.remove(&key);
        safety
    } else if name == "random.Rng" && args.is_empty() {
        // A canonical builtin type can reach clone-safety checking without
        // its namespace in reduced checker contexts. Resolved user classes
        // with the same nominal spelling took the class branch above.
        RngCloneSafety::ContainsRng
    } else if let Some(enum_info) = enums
        .get(name)
        .or_else(|| copy_enum_info_from_modules(name, imported_modules, module_registry))
    {
        if args.len() != enum_info.decl.type_params.len() {
            return RngCloneSafety::Unknown;
        }
        let key = format!("enum:{}:{}", enum_info.module_name, enum_info.decl.name);
        if !visiting.insert(key.clone()) {
            return args.iter().fold(RngCloneSafety::Safe, |safety, arg| {
                safety.combine(rng_clone_safety_in_context_inner(
                    arg,
                    classes,
                    enums,
                    imported_modules,
                    module_registry,
                    visiting,
                ))
            });
        }
        let substitutions = substitutions_from_decl_type_args(&enum_info.decl.type_params, args);
        let safety = enum_info
            .variants
            .values()
            .flat_map(|variant| variant.payloads.iter())
            .map(|payload| substitute_type(&payload.ty, &substitutions))
            .fold(RngCloneSafety::Safe, |safety, payload_ty| {
                safety.combine(rng_clone_safety_in_context_inner(
                    &payload_ty,
                    classes,
                    enums,
                    imported_modules,
                    module_registry,
                    visiting,
                ))
            });
        visiting.remove(&key);
        safety
    } else {
        args.iter().fold(RngCloneSafety::Safe, |safety, arg| {
            safety.combine(rng_clone_safety_in_context_inner(
                arg,
                classes,
                enums,
                imported_modules,
                module_registry,
                visiting,
            ))
        })
    }
}

fn rng_clone_obligation_params_in_context_with_modules(
    ty: &Type,
    classes: &BTreeMap<String, ClassInfo>,
    enums: &BTreeMap<String, EnumInfo>,
    imported_modules: &BTreeMap<String, ModuleNamespace>,
    module_registry: &BTreeMap<String, ModuleNamespace>,
) -> BTreeSet<String> {
    let mut params = BTreeSet::new();
    collect_rng_clone_obligation_params_in_context_inner(
        ty,
        classes,
        enums,
        imported_modules,
        module_registry,
        &mut BTreeSet::new(),
        &mut params,
    );
    params
}

fn collect_rng_clone_obligation_params_from_args(
    args: &[Type],
    classes: &BTreeMap<String, ClassInfo>,
    enums: &BTreeMap<String, EnumInfo>,
    imported_modules: &BTreeMap<String, ModuleNamespace>,
    module_registry: &BTreeMap<String, ModuleNamespace>,
    visiting: &mut BTreeSet<String>,
    params: &mut BTreeSet<String>,
) {
    for arg in args {
        collect_rng_clone_obligation_params_in_context_inner(
            arg,
            classes,
            enums,
            imported_modules,
            module_registry,
            visiting,
            params,
        );
    }
}

fn collect_rng_clone_obligation_params_in_context_inner(
    ty: &Type,
    classes: &BTreeMap<String, ClassInfo>,
    enums: &BTreeMap<String, EnumInfo>,
    imported_modules: &BTreeMap<String, ModuleNamespace>,
    module_registry: &BTreeMap<String, ModuleNamespace>,
    visiting: &mut BTreeSet<String>,
    params: &mut BTreeSet<String>,
) {
    match ty {
        Type::TypeParam(name) => {
            params.insert(name.clone());
        }
        Type::Unit | Type::Module(_) => {}
        Type::Tuple(elements) => collect_rng_clone_obligation_params_from_args(
            elements,
            classes,
            enums,
            imported_modules,
            module_registry,
            visiting,
            params,
        ),
        Type::Named(name, args) if matches!(name.as_str(), "Queue" | "Task") && args.len() == 1 => {
            // Cloning a Queue or Task copies only its shared handle, not its
            // contained or eventual value.
        }
        Type::Named(name, args) => {
            if let Some(class_info) = classes
                .get(name)
                .or_else(|| copy_class_info_from_modules(name, imported_modules, module_registry))
            {
                let key = format!("class:{}:{}", class_info.module_name, class_info.decl.name);
                if !visiting.insert(key.clone()) {
                    collect_rng_clone_obligation_params_from_args(
                        args,
                        classes,
                        enums,
                        imported_modules,
                        module_registry,
                        visiting,
                        params,
                    );
                    return;
                }
                if args.len() == class_info.decl.type_params.len() {
                    let substitutions =
                        substitutions_from_decl_type_args(&class_info.decl.type_params, args);
                    for field in class_info.fields.values() {
                        collect_rng_clone_obligation_params_in_context_inner(
                            &substitute_type(&field.ty, &substitutions),
                            classes,
                            enums,
                            imported_modules,
                            module_registry,
                            visiting,
                            params,
                        );
                    }
                } else {
                    collect_rng_clone_obligation_params_from_args(
                        args,
                        classes,
                        enums,
                        imported_modules,
                        module_registry,
                        visiting,
                        params,
                    );
                }
                visiting.remove(&key);
            } else if let Some(enum_info) = enums
                .get(name)
                .or_else(|| copy_enum_info_from_modules(name, imported_modules, module_registry))
            {
                let key = format!("enum:{}:{}", enum_info.module_name, enum_info.decl.name);
                if !visiting.insert(key.clone()) {
                    collect_rng_clone_obligation_params_from_args(
                        args,
                        classes,
                        enums,
                        imported_modules,
                        module_registry,
                        visiting,
                        params,
                    );
                    return;
                }
                if args.len() == enum_info.decl.type_params.len() {
                    let substitutions =
                        substitutions_from_decl_type_args(&enum_info.decl.type_params, args);
                    for payload in enum_info
                        .variants
                        .values()
                        .flat_map(|variant| variant.payloads.iter())
                    {
                        collect_rng_clone_obligation_params_in_context_inner(
                            &substitute_type(&payload.ty, &substitutions),
                            classes,
                            enums,
                            imported_modules,
                            module_registry,
                            visiting,
                            params,
                        );
                    }
                } else {
                    collect_rng_clone_obligation_params_from_args(
                        args,
                        classes,
                        enums,
                        imported_modules,
                        module_registry,
                        visiting,
                        params,
                    );
                }
                visiting.remove(&key);
            } else {
                collect_rng_clone_obligation_params_from_args(
                    args,
                    classes,
                    enums,
                    imported_modules,
                    module_registry,
                    visiting,
                    params,
                );
            }
        }
    }
}

fn copy_class_info_from_modules<'a>(
    name: &str,
    imported_modules: &'a BTreeMap<String, ModuleNamespace>,
    module_registry: &'a BTreeMap<String, ModuleNamespace>,
) -> Option<&'a ClassInfo> {
    if let Some((module_path, item_name)) = name.rsplit_once('.') {
        let namespace = module_registry
            .get(module_path)
            .or_else(|| find_namespace_in_modules(imported_modules, module_path))?;
        return namespace
            .classes
            .get(item_name)
            .or_else(|| namespace.all_classes.get(item_name));
    }

    let mut found = None;
    let mut ambiguous = false;
    find_copy_class_in_modules(imported_modules, name, &mut found, &mut ambiguous);
    (!ambiguous).then_some(found).flatten()
}

fn find_copy_class_in_modules<'a>(
    modules: &'a BTreeMap<String, ModuleNamespace>,
    name: &str,
    found: &mut Option<&'a ClassInfo>,
    ambiguous: &mut bool,
) {
    for namespace in modules.values() {
        if let Some(candidate) = namespace
            .classes
            .get(name)
            .or_else(|| namespace.all_classes.get(name))
        {
            match found {
                Some(existing)
                    if existing.module_name != candidate.module_name
                        || existing.decl.name != candidate.decl.name =>
                {
                    *ambiguous = true;
                }
                None => *found = Some(candidate),
                Some(_) => {}
            }
        }
        find_copy_class_in_modules(&namespace.modules, name, found, ambiguous);
        find_copy_class_in_modules(&namespace.imported_modules, name, found, ambiguous);
    }
}

fn copy_enum_info_from_modules<'a>(
    name: &str,
    imported_modules: &'a BTreeMap<String, ModuleNamespace>,
    module_registry: &'a BTreeMap<String, ModuleNamespace>,
) -> Option<&'a EnumInfo> {
    if let Some((module_path, item_name)) = name.rsplit_once('.') {
        let namespace = module_registry
            .get(module_path)
            .or_else(|| find_namespace_in_modules(imported_modules, module_path))?;
        return namespace
            .enums
            .get(item_name)
            .or_else(|| namespace.all_enums.get(item_name));
    }

    let mut found = None;
    let mut ambiguous = false;
    find_copy_enum_in_modules(imported_modules, name, &mut found, &mut ambiguous);
    (!ambiguous).then_some(found).flatten()
}

fn find_copy_enum_in_modules<'a>(
    modules: &'a BTreeMap<String, ModuleNamespace>,
    name: &str,
    found: &mut Option<&'a EnumInfo>,
    ambiguous: &mut bool,
) {
    for namespace in modules.values() {
        if let Some(candidate) = namespace
            .enums
            .get(name)
            .or_else(|| namespace.all_enums.get(name))
        {
            match found {
                Some(existing)
                    if existing.module_name != candidate.module_name
                        || existing.decl.name != candidate.decl.name =>
                {
                    *ambiguous = true;
                }
                None => *found = Some(candidate),
                Some(_) => {}
            }
        }
        find_copy_enum_in_modules(&namespace.modules, name, found, ambiguous);
        find_copy_enum_in_modules(&namespace.imported_modules, name, found, ambiguous);
    }
}

fn type_is_copy_in_context_inner(
    ty: &Type,
    classes: &BTreeMap<String, ClassInfo>,
    enums: &BTreeMap<String, EnumInfo>,
    imported_modules: Option<&BTreeMap<String, ModuleNamespace>>,
    module_registry: Option<&BTreeMap<String, ModuleNamespace>>,
    visiting: &mut BTreeSet<String>,
) -> bool {
    match ty {
        Type::Unit => true,
        Type::Module(_) => false,
        Type::TypeParam(_) => false,
        Type::Tuple(elements) => elements.iter().all(|element| {
            type_is_copy_in_context_inner(
                element,
                classes,
                enums,
                imported_modules,
                module_registry,
                visiting,
            )
        }),
        Type::Named(name, args) if is_builtin_copy_named_type(name, args) => true,
        Type::Named(name, args) if name == "Option" && args.len() == 1 => {
            type_is_copy_in_context_inner(
                &args[0],
                classes,
                enums,
                imported_modules,
                module_registry,
                visiting,
            )
        }
        Type::Named(name, args) if name == "Result" && args.len() == 2 => args.iter().all(|arg| {
            type_is_copy_in_context_inner(
                arg,
                classes,
                enums,
                imported_modules,
                module_registry,
                visiting,
            )
        }),
        Type::Named(name, args) if name == "SendError" && args.len() == 1 => {
            type_is_copy_in_context_inner(
                &args[0],
                classes,
                enums,
                imported_modules,
                module_registry,
                visiting,
            )
        }
        Type::Named(name, args) if name == "QueueReceive" && args.len() == 1 => {
            type_is_copy_in_context_inner(
                &args[0],
                classes,
                enums,
                imported_modules,
                module_registry,
                visiting,
            )
        }
        Type::Named(name, args)
            if matches!(name.as_str(), "TaskResult" | "WaitAny" | "WaitAll") && args.len() == 1 =>
        {
            false
        }
        Type::Named(name, args) => {
            let key = ty.to_string();
            if !visiting.insert(key.clone()) {
                return false;
            }
            if let Some(class_info) = classes
                .get(name)
                .or_else(|| copy_class_info_from_modules(name, imported_modules?, module_registry?))
            {
                let result = class_info.decl.copy
                    && args.iter().all(|arg| {
                        type_is_copy_in_context_inner(
                            arg,
                            classes,
                            enums,
                            imported_modules,
                            module_registry,
                            visiting,
                        )
                    });
                visiting.remove(&key);
                return result;
            }
            if let Some(enum_info) = enums
                .get(name)
                .or_else(|| copy_enum_info_from_modules(name, imported_modules?, module_registry?))
            {
                if args.len() != enum_info.decl.type_params.len() {
                    visiting.remove(&key);
                    return false;
                }
                let substitutions =
                    substitutions_from_decl_type_args(&enum_info.decl.type_params, args);
                let result = enum_info.variants.values().all(|variant| {
                    variant.payloads.iter().all(|payload| {
                        let payload_ty = substitute_type(&payload.ty, &substitutions);
                        type_is_copy_in_context_inner(
                            &payload_ty,
                            classes,
                            enums,
                            imported_modules,
                            module_registry,
                            visiting,
                        )
                    })
                });
                visiting.remove(&key);
                return result;
            }
            visiting.remove(&key);
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
            Type::Tuple(elements) => {
                write!(f, "(")?;
                for (index, element) in elements.iter().enumerate() {
                    if index > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{}", element)?;
                }
                if elements.len() == 1 {
                    write!(f, ",")?;
                }
                write!(f, ")")
            }
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
    let mut canonical_type_names = BTreeMap::<String, String>::new();
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
        let class_info = classes.get(&class_decl.name).ok_or_else(|| {
            Diagnostic::at(
                class_decl.span,
                format!(
                    "internal error: class `{}` disappeared after collection",
                    class_decl.name
                ),
            )
        })?;
        for field_decl in &class_decl.fields {
            if field_decl.ty.indirect {
                continue;
            }
            let field_ty = &class_info
                .fields
                .get(&field_decl.name)
                .ok_or_else(|| {
                    Diagnostic::at(
                        field_decl.span,
                        format!(
                            "internal error: class field `{}` on `{}` lost its lowered type",
                            field_decl.name, class_decl.name
                        ),
                    )
                })?
                .ty;
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
            let field_ty = &class
                .fields
                .get(&field_decl.name)
                .ok_or_else(|| {
                    Diagnostic::at(
                        field_decl.span,
                        format!(
                            "internal error: class field `{}` on `{}` lost its lowered type",
                            field_decl.name, class.decl.name
                        ),
                    )
                })?
                .ty;
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
            let lowered = class
                .fields
                .get(&field.name)
                .ok_or_else(|| {
                    Diagnostic::at(
                        field.span,
                        format!(
                            "internal error: class field `{}` on `{}` lost its lowered type",
                            field.name, class.decl.name
                        ),
                    )
                })?
                .ty
                .clone();
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
                    },
                    type_param_bounds,
                },
            );
        }
        for trait_method_name in trait_info.methods.keys() {
            if methods.contains_key(trait_method_name) {
                continue;
            }
            let trait_method = trait_info.methods.get(trait_method_name).ok_or_else(|| {
                Diagnostic::at(
                    impl_decl.span,
                    format!(
                        "internal error: trait method `{}` disappeared during impl checking",
                        trait_method_name
                    ),
                )
            })?;
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

    let mut program = Program {
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
        canonical_type_names: canonical_type_names.clone(),
        top_level_stmts: module.top_level_stmts.clone(),
    };

    let local_main = program
        .functions
        .get("main")
        .filter(|function| function.module_name == program.module_name);
    if context.is_entry_module && !program.top_level_stmts.is_empty() && local_main.is_some() {
        let main = local_main.ok_or_else(|| {
            Diagnostic::new(
                "internal error: local `main` disappeared while validating top-level statements",
            )
        })?;
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
            class_method_obligations,
            trait_method_obligations,
            impl_method_obligations,
        ) = {
            let checker = FunctionChecker::new(
                &program.module_name,
                &type_names,
                &type_arities,
                &canonical_type_names,
                &program.classes,
                &program.enums,
                &program.functions,
                &program.traits,
                &program.trait_impls,
                &program.imported_modules,
                &program.module_registry,
            );
            let mut function_obligations = BTreeMap::<String, BTreeSet<String>>::new();
            let mut class_method_obligations = BTreeMap::<CallableKey, BTreeSet<String>>::new();
            let mut trait_method_obligations = BTreeMap::<CallableKey, BTreeSet<String>>::new();
            let mut impl_method_obligations = BTreeMap::<(usize, String), BTreeSet<String>>::new();

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
                    checker
                        .with_module_name(&trait_info.module_name)
                        .with_rng_clone_obligation_sink(sink.clone())
                        .check_trait_method(trait_info, method)?;
                    trait_method_obligations.insert(
                        (trait_name.clone(), method_name.clone()),
                        sink.borrow().clone(),
                    );
                }
            }
            for (function_name, function) in &program.functions {
                if function.module_name != program.module_name {
                    continue;
                }
                let sink = Rc::new(RefCell::new(BTreeSet::new()));
                checker
                    .with_module_name(&function.module_name)
                    .with_rng_clone_obligation_sink(sink.clone())
                    .check_function(function)?;
                function_obligations.insert(function_name.clone(), sink.borrow().clone());
            }

            for (class_name, class) in &program.classes {
                for (method_name, method) in &class.methods {
                    let sink = Rc::new(RefCell::new(BTreeSet::new()));
                    checker
                        .with_module_name(&class.module_name)
                        .with_rng_clone_obligation_sink(sink.clone())
                        .check_method(&class.decl, method)?;
                    class_method_obligations.insert(
                        (class_name.clone(), method_name.clone()),
                        sink.borrow().clone(),
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
                    checker
                        .with_module_name(&trait_impl.module_name)
                        .with_rng_clone_obligation_sink(sink.clone())
                        .check_trait_impl_method(
                            &trait_impl.for_type,
                            &trait_impl.type_params,
                            &trait_impl.type_param_bounds,
                            method,
                        )?;
                    impl_method_obligations
                        .insert((impl_index, method_name.clone()), sink.borrow().clone());
                }
            }

            (
                function_obligations,
                class_method_obligations,
                trait_method_obligations,
                impl_method_obligations,
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
        let body_impl_obligations = impl_method_obligations.clone();
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

        // A trait method's inferred requirements are part of its callable
        // contract. Map them through each impl header so direct concrete
        // dispatch observes the same contract as dispatch through a bound.
        let mut mapped_impl_contracts = BTreeMap::<(usize, String), BTreeSet<String>>::new();
        for (impl_index, trait_impl) in program.trait_impls.iter().enumerate() {
            let trait_info = program.traits.get(&trait_impl.trait_name).ok_or_else(|| {
                Diagnostic::at(
                    trait_impl.decl.span,
                    format!(
                        "internal error: trait `{}` disappeared during clone-safety contract checking",
                        trait_impl.trait_name
                    ),
                )
            })?;
            let substitutions = self_type_substitutions(
                &trait_info.decl,
                &trait_impl.trait_args,
                trait_impl.for_type.clone(),
            );
            for (method_name, impl_method) in &trait_impl.methods {
                let Some(trait_method) = trait_info.methods.get(method_name) else {
                    continue;
                };
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
        if !changed {
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
            break;
        }
    }

    FunctionChecker::new(
        &program.module_name,
        &type_names,
        &type_arities,
        &canonical_type_names,
        &program.classes,
        &program.enums,
        &program.functions,
        &program.traits,
        &program.trait_impls,
        &program.imported_modules,
        &program.module_registry,
    )
    .check_top_level(&program.top_level_stmts)?;

    Ok(program)
}

/// Rejects a trait implementation that would shadow a builtin method of its
/// target. The rule covers every builtin target, not only the runtime handles:
/// a shadowed builtin name is silently ignored at every call site, so the
/// program does something other than what its source says.
fn reject_builtin_trait_method_collisions(
    impl_decl: &ImplDecl,
    for_type: &Type,
    methods: &BTreeMap<String, TraitImplMethodInfo>,
) -> Result<()> {
    let Type::Named(target_name, _) = for_type else {
        return Ok(());
    };
    if !is_builtin_type(target_name) && !preserves_qualified_builtin_type_name(target_name) {
        return Ok(());
    }

    for (method_name, method) in methods {
        if BuiltinMember::resolve(target_name, method_name).is_none() {
            continue;
        }

        let explicit_method = impl_decl
            .methods
            .iter()
            .find(|candidate| candidate.name.as_str() == method_name.as_str());
        let primary_span = explicit_method.map_or(impl_decl.span, |method| method.span);
        let mut diagnostic = Diagnostic::coded_at(
            "AU2006",
            primary_span,
            format!(
                "trait method `{method_name}` collides with builtin method \
                 `{target_name}.{method_name}`"
            ),
        )
        .with_help(
            "rename the trait method; builtin methods cannot be shadowed by trait implementations",
        );
        if explicit_method.is_none() {
            diagnostic = diagnostic.with_secondary(
                method.decl.span,
                "colliding default trait method is declared here",
            );
        }
        return Err(diagnostic);
    }

    Ok(())
}

fn lower_type(
    type_ref: &TypeRef,
    type_names: &BTreeMap<String, crate::diag::Span>,
    type_arities: &BTreeMap<String, usize>,
    canonical_type_names: &BTreeMap<String, String>,
    type_params: &BTreeMap<String, ()>,
) -> Result<Type> {
    lower_type_with_self(
        type_ref,
        type_names,
        type_arities,
        canonical_type_names,
        type_params,
        None,
    )
}

fn lower_type_with_self(
    type_ref: &TypeRef,
    type_names: &BTreeMap<String, crate::diag::Span>,
    type_arities: &BTreeMap<String, usize>,
    canonical_type_names: &BTreeMap<String, String>,
    type_params: &BTreeMap<String, ()>,
    self_type: Option<&Type>,
) -> Result<Type> {
    let (name, type_args) = match &type_ref.kind {
        crate::ast::TypeRefKind::Tuple(elements) => {
            return elements
                .iter()
                .map(|element| {
                    lower_type_with_self(
                        element,
                        type_names,
                        type_arities,
                        canonical_type_names,
                        type_params,
                        self_type,
                    )
                })
                .collect::<Result<Vec<_>>>()
                .map(Type::Tuple);
        }
        crate::ast::TypeRefKind::Named { name, args } => (name, args),
    };
    let type_name = match name.as_str() {
        "str" => "String",
        "int" => "int64",
        name => name,
    };

    if type_name == "Self" {
        if !type_args.is_empty() {
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
        if !type_args.is_empty() {
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
        if !type_args.is_empty() {
            return Err(Diagnostic::at(
                type_ref.span,
                "`None` does not take generic arguments",
            ));
        }
        return Ok(Type::Unit);
    }

    let args = type_args
        .iter()
        .map(|arg| {
            lower_type_with_self(
                arg,
                type_names,
                type_arities,
                canonical_type_names,
                type_params,
                self_type,
            )
        })
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

    if type_name == "Queue"
        || type_name == "Task"
        || type_name == "SendError"
        || type_name == "QueueReceive"
        || type_name == "TaskResult"
        || type_name == "WaitAny"
        || type_name == "WaitAll"
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
    } else if (is_builtin_type(type_name) || type_names.contains_key(type_name)) && !args.is_empty()
    {
        return Err(Diagnostic::at(
            type_ref.span,
            format!("`{}` does not take type arguments", type_name),
        ));
    }

    if is_builtin_type(type_name) || type_names.contains_key(type_name) {
        let canonical_name =
            if preserves_qualified_builtin_type_name(type_name) || type_name.contains('.') {
                type_name.to_string()
            } else if let Some(canonical_name) = canonical_type_names.get(type_name) {
                canonical_name.clone()
            } else {
                type_name.to_string()
            };
        Ok(Type::Named(canonical_name, args))
    } else {
        Err(Diagnostic::at(
            type_ref.span,
            format!("unknown type `{}`", name),
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

fn validate_params(receiver: Option<ReceiverKind>, params: &[Param], owner: &str) -> Result<()> {
    let mut seen = BTreeSet::new();
    for param in params {
        if receiver.is_some() && param.name == "self" {
            return Err(Diagnostic::at(
                param.span,
                format!("parameter `self` conflicts with the receiver on {}", owner),
            ));
        }
        if !seen.insert(&param.name) {
            return Err(Diagnostic::at(
                param.span,
                format!("duplicate parameter `{}` on {}", param.name, owner),
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
    match &type_ref.kind {
        crate::ast::TypeRefKind::Tuple(elements) => {
            for element in elements {
                collect_type_ref_type_params(element, type_names, collected, true);
            }
        }
        crate::ast::TypeRefKind::Named { name, args } => {
            if include_self
                && args.is_empty()
                && !type_ref.indirect
                && !is_builtin_type(name)
                && !type_names.contains_key(name)
            {
                collected.insert(name.clone());
            }
            for arg in args {
                collect_type_ref_type_params(arg, type_names, collected, true);
            }
        }
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
        ExprKind::Tuple(elements) | ExprKind::List(elements) | ExprKind::Set(elements) => elements
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
        ExprKind::Membership {
            value, container, ..
        } => default_argument_references_param(value, param_names)
            .or_else(|| default_argument_references_param(container, param_names)),
        ExprKind::CompareChain { first, links } => {
            default_argument_references_param(first, param_names).or_else(|| {
                links
                    .iter()
                    .find_map(|link| default_argument_references_param(&link.operand, param_names))
            })
        }
        ExprKind::Binary { left, right, .. } => {
            default_argument_references_param(left, param_names)
                .or_else(|| default_argument_references_param(right, param_names))
        }
        ExprKind::Conditional {
            then_expr,
            condition,
            else_expr,
        } => default_argument_references_param(condition, param_names)
            .or_else(|| default_argument_references_param(then_expr, param_names))
            .or_else(|| default_argument_references_param(else_expr, param_names)),
        ExprKind::Int(_)
        | ExprKind::Float(_)
        | ExprKind::Bool(_)
        | ExprKind::String(_)
        | ExprKind::DurationNanos(_)
        | ExprKind::BuiltinOmitted => None,
    }
}

fn lower_trait_bounds(
    bounds: &BTreeMap<String, Vec<TypeRef>>,
    traits: &BTreeMap<String, TraitInfo>,
    type_names: &BTreeMap<String, crate::diag::Span>,
    type_arities: &BTreeMap<String, usize>,
    canonical_type_names: &BTreeMap<String, String>,
    type_param_scope: &BTreeMap<String, ()>,
) -> Result<BTreeMap<String, Vec<TraitBound>>> {
    lower_trait_bounds_with_self(
        bounds,
        traits,
        type_names,
        type_arities,
        canonical_type_names,
        type_param_scope,
        None,
    )
}

fn lower_supertraits(
    supertraits: &[TypeRef],
    traits: &BTreeMap<String, TraitInfo>,
    type_names: &BTreeMap<String, crate::diag::Span>,
    type_arities: &BTreeMap<String, usize>,
    canonical_type_names: &BTreeMap<String, String>,
    type_param_scope: &BTreeMap<String, ()>,
    self_type: Option<&Type>,
) -> Result<Vec<TraitBound>> {
    let mut lowered = Vec::new();
    for supertrait in supertraits {
        let Some((trait_name, trait_type_args)) = supertrait.named_parts() else {
            return Err(Diagnostic::at(
                supertrait.span,
                "a supertrait must be a named trait type",
            ));
        };
        let Some(trait_info) = traits.get(trait_name) else {
            return Err(Diagnostic::at(
                supertrait.span,
                format!("unknown trait `{}`", trait_name),
            ));
        };
        if trait_type_args.len() != trait_info.decl.type_params.len() {
            return Err(Diagnostic::at(
                supertrait.span,
                format!(
                    "trait `{}` expects {} type arguments, found {}",
                    trait_name,
                    trait_info.decl.type_params.len(),
                    trait_type_args.len()
                ),
            ));
        }
        let trait_args = trait_type_args
            .iter()
            .map(|arg| {
                lower_type_with_self(
                    arg,
                    type_names,
                    type_arities,
                    canonical_type_names,
                    type_param_scope,
                    self_type,
                )
            })
            .collect::<Result<Vec<_>>>()?;
        lowered.push(TraitBound {
            trait_name: trait_name.to_string(),
            trait_args,
        });
    }
    Ok(lowered)
}

fn lower_trait_bounds_with_self(
    bounds: &BTreeMap<String, Vec<TypeRef>>,
    traits: &BTreeMap<String, TraitInfo>,
    type_names: &BTreeMap<String, crate::diag::Span>,
    type_arities: &BTreeMap<String, usize>,
    canonical_type_names: &BTreeMap<String, String>,
    type_param_scope: &BTreeMap<String, ()>,
    self_type: Option<&Type>,
) -> Result<BTreeMap<String, Vec<TraitBound>>> {
    let mut lowered = BTreeMap::new();
    for (type_param, trait_bounds) in bounds {
        let mut names = Vec::new();
        for bound in trait_bounds {
            let Some((trait_name, trait_type_args)) = bound.named_parts() else {
                return Err(Diagnostic::at(
                    bound.span,
                    "a type parameter bound must be a named trait type",
                ));
            };
            let Some(trait_info) = traits.get(trait_name) else {
                return Err(Diagnostic::at(
                    bound.span,
                    format!("unknown trait `{}`", trait_name),
                ));
            };
            if trait_type_args.len() != trait_info.decl.type_params.len() {
                return Err(Diagnostic::at(
                    bound.span,
                    format!(
                        "trait `{}` expects {} type arguments, found {}",
                        trait_name,
                        trait_info.decl.type_params.len(),
                        trait_type_args.len()
                    ),
                ));
            }
            let trait_args = trait_type_args
                .iter()
                .map(|arg| {
                    lower_type_with_self(
                        arg,
                        type_names,
                        type_arities,
                        canonical_type_names,
                        type_param_scope,
                        self_type,
                    )
                })
                .collect::<Result<Vec<_>>>()?;
            names.push(TraitBound {
                trait_name: trait_name.to_string(),
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
        Type::Tuple(elements) => elements
            .iter()
            .any(|element| type_contains_named(element, target)),
        Type::Named(name, args) => {
            name == target || args.iter().any(|arg| type_contains_named(arg, target))
        }
        Type::TypeParam(_) | Type::Module(_) | Type::Unit => false,
    }
}

fn recursive_field_message(class_name: &str, field_name: &str, field_type: &TypeRef) -> String {
    if matches!(&field_type.kind, crate::ast::TypeRefKind::Tuple(_)) {
        return format!(
            "recursive field `{field_name}` on class `{class_name}` contains tuple storage; tuple types cannot be `indirect`, so move the recursive link into an `indirect` named field"
        );
    }
    format!("recursive field `{field_name}` on class `{class_name}` requires `indirect`")
}

fn type_reaches_class_through_non_indirect_fields(
    ty: &Type,
    target: &str,
    classes: &BTreeMap<String, ClassInfo>,
    visiting: &mut BTreeSet<String>,
) -> bool {
    match ty {
        Type::Tuple(elements) => elements.iter().any(|element| {
            type_reaches_class_through_non_indirect_fields(element, target, classes, visiting)
        }),
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
                let Some(field_ty) = class_info
                    .fields
                    .get(&field_decl.name)
                    .map(|field| &field.ty)
                else {
                    return false;
                };
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
        Type::Tuple(elements) => Type::Tuple(
            elements
                .iter()
                .map(|element| substitute_type(element, substitutions))
                .collect(),
        ),
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
        Type::Tuple(elements) => {
            for element in elements {
                collect_type_params_from_type(element, collected);
            }
        }
        Type::Unit | Type::Module(_) => {}
    }
}

pub(crate) fn type_pattern_specificity(ty: &Type) -> usize {
    match ty {
        Type::TypeParam(_) => 0,
        Type::Named(_, args) => 1 + args.iter().map(type_pattern_specificity).sum::<usize>(),
        Type::Tuple(elements) => 1 + elements.iter().map(type_pattern_specificity).sum::<usize>(),
        Type::Module(_) | Type::Unit => 1,
    }
}

pub(crate) fn trait_impl_specificity_parts(for_type: &Type, trait_args: &[Type]) -> usize {
    type_pattern_specificity(for_type)
        + trait_args
            .iter()
            .map(type_pattern_specificity)
            .sum::<usize>()
}

pub(crate) fn trait_impl_specificity(trait_impl: &TraitImplInfo) -> usize {
    trait_impl_specificity_parts(&trait_impl.for_type, &trait_impl.trait_args)
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
        Type::Tuple(pattern_elements) => {
            let Type::Tuple(actual_elements) = actual else {
                return false;
            };
            pattern_elements.len() == actual_elements.len()
                && pattern_elements
                    .iter()
                    .zip(actual_elements)
                    .all(|(pattern, actual)| {
                        type_pattern_matches(pattern, actual, type_params, substitutions)
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
        Type::Tuple(elements) => elements.iter().any(has_unresolved_type_params),
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
        Type::Tuple(elements) => {
            let Type::Tuple(actual_elements) = actual else {
                return Err(Diagnostic::new(format!(
                    "expected `{}`, found `{}`",
                    pattern, actual
                )));
            };
            if elements.len() != actual_elements.len() {
                return Err(Diagnostic::new(format!(
                    "expected `{}`, found `{}`",
                    pattern, actual
                )));
            }
            for (element, actual_element) in elements.iter().zip(actual_elements) {
                unify_type_pattern(element, actual_element, substitutions)?;
            }
            Ok(())
        }
    }
}

/// The element, key, or substring type an `in` container compares against.
pub(crate) fn membership_needle_type(container_ty: &Type) -> Option<Type> {
    match container_ty {
        Type::Named(name, args) if (name == "Vec" || name == "Set") && args.len() == 1 => {
            Some(args[0].clone())
        }
        Type::Named(name, args) if name == "Map" && args.len() == 2 => Some(args[0].clone()),
        Type::Named(name, args) if name == "String" && args.is_empty() => {
            Some(Type::named("String"))
        }
        _ => None,
    }
}

/// The builtin member that `in` delegates to for a supported container.
pub(crate) fn membership_member_name(container_ty: &Type) -> Option<&'static str> {
    match container_ty {
        Type::Named(name, args) if name == "Map" && args.len() == 2 => Some("contains_key"),
        Type::Named(name, args) if (name == "Vec" || name == "Set") && args.len() == 1 => {
            Some("contains")
        }
        Type::Named(name, args) if name == "String" && args.is_empty() => Some("contains"),
        _ => None,
    }
}

fn is_builtin_type(name: &str) -> bool {
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
            | "Vec"
            | "Set"
            | "Map"
            | "MapEntry"
            | "Range"
            | "Queue"
            | "Task"
            | "Option"
            | "Result"
            | "SendError"
            | "QueueReceive"
            | "TaskResult"
            | "WaitAny"
            | "WaitAll"
            | "TaskGroup"
            | "Duration"
    )
}

fn preserves_qualified_builtin_type_name(type_name: &str) -> bool {
    matches!(
        type_name,
        "fs.File"
            | "process.Child"
            | "process.Pipe"
            | "process.Completed"
            | "process.Supervisor"
            | "process.ExitStatus"
            | "process.Wait"
            | "process.RestartPolicy"
            | "process.SupervisorEvent"
            | "process.SupervisorWait"
            | "process.Stdio"
            | "process.Error"
            | "net.TcpStream"
            | "net.TcpListener"
            | "net.UdpSocket"
            | "net.UdpDatagram"
            | "net.HttpListener"
            | "net.HttpExchange"
            | "net.HttpResponse"
            | "net.WebSocketListener"
            | "net.WebSocket"
            | "net.UnixListener"
            | "net.UnixStream"
            | "net.TlsListener"
            | "net.TlsStream"
            | "io.Error"
            | "random.Rng"
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

fn is_option_type(ty: &Type) -> bool {
    matches!(ty, Type::Named(name, args) if name == "Option" && args.len() == 1)
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

fn required_ordered_arg<'a>(
    ordered_args: &[Option<&'a Argument>],
    index: usize,
    span: crate::diag::Span,
    message: impl Into<String>,
) -> Result<&'a Argument> {
    ordered_args
        .get(index)
        .and_then(|argument| *argument)
        .ok_or_else(|| Diagnostic::at(span, message.into()))
}

fn is_builtin_io_resource_type(name: &str, args: &[Type]) -> bool {
    args.is_empty()
        && matches!(
            name,
            "TaskGroup"
                | "process.Child"
                | "process.Pipe"
                | "process.Supervisor"
                | "fs.File"
                | "net.TcpStream"
                | "net.TcpListener"
                | "net.UdpSocket"
                | "net.UdpDatagram"
                | "net.HttpListener"
                | "net.HttpExchange"
                | "net.HttpResponse"
                | "net.WebSocketListener"
                | "net.WebSocket"
                | "net.UnixListener"
                | "net.UnixStream"
                | "net.TlsListener"
                | "net.TlsStream"
        )
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
enum PlaceProjection {
    Field(String),
}

impl fmt::Display for PlaceProjection {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Field(field) => write!(f, "{}", field),
        }
    }
}

#[derive(Clone, Debug, Default, Eq, Ord, PartialEq, PartialOrd)]
struct ProjectionPath(Vec<PlaceProjection>);

impl ProjectionPath {
    fn with_field(&self, field: impl Into<String>) -> Self {
        let mut projections = self.0.clone();
        projections.push(PlaceProjection::Field(field.into()));
        Self(projections)
    }

    fn overlaps(&self, other: &Self) -> bool {
        self.0.starts_with(&other.0) || other.0.starts_with(&self.0)
    }

    fn is_descendant_of_or_equal(&self, other: &Self) -> bool {
        self.0.starts_with(&other.0)
    }
}

impl fmt::Display for ProjectionPath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (index, projection) in self.0.iter().enumerate() {
            if index > 0 {
                write!(f, ".")?;
            }
            write!(f, "{}", projection)?;
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct PlacePath {
    root: String,
    projections: ProjectionPath,
}

impl PlacePath {
    fn root(root: impl Into<String>) -> Self {
        Self {
            root: root.into(),
            projections: ProjectionPath::default(),
        }
    }

    fn with_field(&self, field: impl Into<String>) -> Self {
        Self {
            root: self.root.clone(),
            projections: self.projections.with_field(field),
        }
    }

    fn overlaps(&self, other: &Self) -> bool {
        self.root == other.root && self.projections.overlaps(&other.projections)
    }

    fn is_root(&self) -> bool {
        self.projections.0.is_empty()
    }
}

impl fmt::Display for PlacePath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.root)?;
        if !self.projections.0.is_empty() {
            write!(f, ".{}", self.projections)?;
        }
        Ok(())
    }
}

#[derive(Clone)]
struct LocalBinding {
    ty: Type,
    assignable: bool,
    mutable_place: bool,
    managed_resource: bool,
    passing: ReceiverKind,
    borrow_origin: Option<String>,
    borrowed_at: Option<crate::diag::Span>,
    match_borrow_mut_place: Option<PlacePath>,
    stale_match_borrow_mut_place: Option<PlacePath>,
    /// Set on a payload bound by a bare (shared) `match` over a named place.
    /// ADR-0022 Q2 requires moving such a payload out to name `match own` as
    /// the replacement instead of the generic borrowed-move wording.
    shared_match_scrutinee: Option<String>,
    moved: bool,
    moved_at: Option<crate::diag::Span>,
    moved_fields: BTreeMap<ProjectionPath, crate::diag::Span>,
    frozen_places: BTreeMap<PlacePath, crate::diag::Span>,
}

#[derive(Clone)]
struct ExprResultEntry {
    locals: HashMap<String, LocalBinding>,
    expected: Option<Type>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct BorrowSourceInfo {
    origin: String,
    passing: ReceiverKind,
}

#[derive(Clone)]
struct BorrowedCallPlace {
    path: PlacePath,
    passing: ReceiverKind,
    param_name: String,
    origin_span: crate::diag::Span,
}

/// A compiler-known `for` iterable form.
#[derive(Clone, Copy, Eq, PartialEq)]
enum LoopFormKind {
    Enumerate,
    Zip,
}

impl LoopFormKind {
    fn name(self) -> &'static str {
        match self {
            Self::Enumerate => "enumerate",
            Self::Zip => "zip",
        }
    }

    fn arity(self) -> usize {
        match self {
            Self::Enumerate => 1,
            Self::Zip => 2,
        }
    }
}

struct LoopForm<'a> {
    kind: LoopFormKind,
    name: &'static str,
    #[allow(dead_code)]
    span: crate::diag::Span,
    iterables: Vec<&'a Expr>,
}

/// The element type an index-addressable collection yields in a lockstep loop.
pub(crate) fn lockstep_element_type(iterable_ty: &Type) -> Option<Type> {
    match iterable_ty {
        Type::Named(name, args) if (name == "Vec" || name == "Set") && args.len() == 1 => {
            Some(args[0].clone())
        }
        _ => None,
    }
}

/// One named field read out of a surrounding expression's result.
#[derive(Clone, Copy)]
struct ProjectedField<'a> {
    name: &'a str,
    span: crate::diag::Span,
}

/// The parts of an expression-form `match` that its typing needs.
#[derive(Clone, Copy)]
struct MatchExprParts<'a> {
    scrutinee: &'a Expr,
    borrow_mode: ReceiverKind,
    arms: &'a [MatchExprArm],
    span: crate::diag::Span,
}

/// How the surrounding expression uses a branching expression's result.
#[derive(Clone, Copy)]
enum BranchResultUse<'a> {
    /// The result is produced without transferring ownership at this point.
    Inspected,
    /// The result itself is consumed by the surrounding expression.
    Consumed,
    /// One field of the result is consumed by the surrounding expression.
    ProjectedField(ProjectedField<'a>),
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct ResolvedUnaryOperatorAccess {
    return_type: Type,
    receiver_passing: ReceiverKind,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct ResolvedBinaryOperatorAccess {
    return_type: Type,
    receiver_passing: ReceiverKind,
    rhs_passing: ReceiverKind,
}

struct FunctionChecker<'a> {
    root_module_name: &'a str,
    module_name: &'a str,
    type_names: &'a BTreeMap<String, crate::diag::Span>,
    type_arities: &'a BTreeMap<String, usize>,
    canonical_type_names: &'a BTreeMap<String, String>,
    classes: &'a BTreeMap<String, ClassInfo>,
    enums: &'a BTreeMap<String, EnumInfo>,
    functions: &'a BTreeMap<String, FunctionInfo>,
    traits: &'a BTreeMap<String, TraitInfo>,
    trait_impls: &'a [TraitImplInfo],
    imported_modules: &'a BTreeMap<String, ModuleNamespace>,
    module_registry: &'a BTreeMap<String, ModuleNamespace>,
    current_return_type: Option<Type>,
    type_params: BTreeMap<String, ()>,
    type_param_bounds: BTreeMap<String, Vec<TraitBound>>,
    implicit_borrowed_params: BTreeMap<String, Type>,
    active_match_borrow_mut_places: Rc<RefCell<Vec<PlacePath>>>,
    rng_clone_obligations: Rc<RefCell<BTreeSet<String>>>,
    expr_result_entries: Rc<RefCell<HashMap<usize, ExprResultEntry>>>,
}

#[derive(Clone)]
struct ResolvedTraitMethodInfo {
    decl: FunctionDecl,
    signature: FunctionSignature,
    type_param_bounds: BTreeMap<String, Vec<TraitBound>>,
    rng_clone_safe_types: Vec<Type>,
}

#[derive(Clone)]
struct ResolvedCallableInfo {
    display_name: String,
    decl: FunctionDecl,
    signature: FunctionSignature,
    type_param_bounds: BTreeMap<String, Vec<TraitBound>>,
    seed_substitutions: HashMap<String, Type>,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
enum BlockFlow {
    FallsThrough,
    AlwaysReturns,
}

impl<'a> FunctionChecker<'a> {
    fn check_vec_index_type(
        &self,
        index: &Expr,
        span: crate::diag::Span,
        locals: &mut HashMap<String, LocalBinding>,
    ) -> Result<()> {
        let expected = Type::named("int32");
        let actual = self.type_of_expr_hint(index, locals, Some(&expected))?;
        if actual != expected {
            return Err(Diagnostic::at(
                span,
                format!("vector indices must have type `int32`, found `{}`", actual),
            ));
        }
        Ok(())
    }

    fn bound_argument<'b>(
        &self,
        ordered_args: &'b [Option<&'b Argument>],
        index: usize,
        span: crate::diag::Span,
        message: impl Into<String>,
    ) -> Result<&'b Argument> {
        ordered_args
            .get(index)
            .copied()
            .flatten()
            .ok_or_else(|| Diagnostic::at(span, format!("internal error: {}", message.into())))
    }

    fn is_copy_type(&self, ty: &Type) -> bool {
        type_is_copy_in_context_with_modules(
            ty,
            self.classes,
            self.enums,
            self.imported_modules,
            self.module_registry,
        )
    }

    /// Builds the `AU3005` message for a rejected non-copy indexed read.
    ///
    /// The recommended recovery depends on whether the selected value can be
    /// cloned at all. Recommending `get(...)` unconditionally sends a caller
    /// holding non-cloneable `random.Rng` state to an `AU3007` dead end, so the
    /// guidance follows the same tri-state classification that rejection uses.
    fn indexed_read_guidance(&self, container: &str, selector: &str, ty: &Type) -> String {
        match self.rng_clone_safety(ty) {
            RngCloneSafety::Safe if container == "map" => format!(
                "cannot implicitly copy `{ty}` out of a map index; use `get(key)` for an explicit cloned optional read, or `remove(key)` to transfer ownership"
            ),
            RngCloneSafety::Safe => format!(
                "cannot implicitly copy `{ty}` out of a vector index; use `get(index)` for an explicit cloned read instead"
            ),
            RngCloneSafety::ContainsRng => format!(
                "cannot implicitly copy `{ty}` out of a {container} index; `get({selector})` cannot clone it because `{ty}` contains non-cloneable `random.Rng` state, so use `remove({selector})` to transfer ownership instead"
            ),
            RngCloneSafety::Unknown => format!(
                "cannot implicitly copy `{ty}` out of a {container} index; `get({selector})` requires a clone-safe `{ty}`, or use `remove({selector})` to transfer ownership"
            ),
        }
    }

    fn rng_clone_safety(&self, ty: &Type) -> RngCloneSafety {
        rng_clone_safety_in_context_with_modules(
            ty,
            self.classes,
            self.enums,
            self.imported_modules,
            self.module_registry,
        )
    }

    fn reject_rng_duplication(
        &self,
        operation: &str,
        ty: &Type,
        span: crate::diag::Span,
    ) -> Result<()> {
        let operation = if operation.contains('`') {
            operation.to_string()
        } else {
            format!("`{operation}`")
        };
        let qualifier = match self.rng_clone_safety(ty) {
            RngCloneSafety::Safe => return Ok(()),
            RngCloneSafety::ContainsRng => "contains",
            RngCloneSafety::Unknown => {
                let params = rng_clone_obligation_params_in_context_with_modules(
                    ty,
                    self.classes,
                    self.enums,
                    self.imported_modules,
                    self.module_registry,
                );
                if !params.is_empty() {
                    let foreign = params
                        .iter()
                        .filter(|name| {
                            !self.type_params.contains_key(*name) && name.as_str() != "Self"
                        })
                        .cloned()
                        .collect::<Vec<_>>();
                    if !foreign.is_empty() {
                        return Err(Diagnostic::coded_at(
                            "AU3007",
                            span,
                            format!(
                                "cannot prove clone safety for unresolved type parameter{} {} while checking {operation}",
                                if foreign.len() == 1 { "" } else { "s" },
                                foreign
                                    .iter()
                                    .map(|name| format!("`{}`", name))
                                    .collect::<Vec<_>>()
                                    .join(", ")
                            ),
                        ));
                    }
                    self.rng_clone_obligations.borrow_mut().extend(params);
                    return Ok(());
                }
                "may contain"
            }
        };
        Err(
            Diagnostic::coded_at(
                "AU3007",
                span,
                format!(
                    "cannot use {operation} because `{ty}` {qualifier} non-cloneable `random.Rng` state"
                ),
            )
            .with_help(
                "move or remove the value so it has one owner, or construct an independent generator with an explicit seed",
            ),
        )
    }

    fn enforce_rng_clone_obligations(
        &self,
        operation: &str,
        obligations: &BTreeSet<String>,
        substitutions: &HashMap<String, Type>,
        span: crate::diag::Span,
    ) -> Result<()> {
        for type_param in obligations {
            let resolved = substitutions
                .get(type_param)
                .cloned()
                .unwrap_or_else(|| Type::TypeParam(type_param.clone()));
            self.reject_rng_duplication(operation, &resolved, span)?;
        }
        Ok(())
    }

    fn enforce_rng_clone_obligations_before_method_inference(
        &self,
        operation: &str,
        obligations: &BTreeSet<String>,
        substitutions: &HashMap<String, Type>,
        method_type_params: &[String],
        span: crate::diag::Span,
    ) -> Result<()> {
        for type_param in obligations {
            if !substitutions.contains_key(type_param)
                && method_type_params
                    .iter()
                    .any(|candidate| candidate == type_param)
            {
                continue;
            }
            let resolved = substitutions
                .get(type_param)
                .cloned()
                .unwrap_or_else(|| Type::TypeParam(type_param.clone()));
            self.reject_rng_duplication(operation, &resolved, span)?;
        }
        Ok(())
    }

    fn enforce_resolved_rng_clone_obligations_before_method_inference(
        &self,
        operation: &str,
        obligations: &[Type],
        method_type_params: &[String],
        span: crate::diag::Span,
    ) -> Result<()> {
        for ty in obligations {
            if matches!(
                ty,
                Type::TypeParam(name)
                    if method_type_params.iter().any(|candidate| candidate == name)
            ) {
                continue;
            }
            self.reject_rng_duplication(operation, ty, span)?;
        }
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    fn infer_method_type_substitutions(
        &self,
        operation: &str,
        method_type_params: &[String],
        param_types: &[Type],
        type_param_bounds: &BTreeMap<String, Vec<TraitBound>>,
        rng_clone_safe_type_params: &BTreeSet<String>,
        actual_types: &[Type],
        mut substitutions: HashMap<String, Type>,
        span: crate::diag::Span,
    ) -> Result<HashMap<String, Type>> {
        for (expected, actual) in param_types.iter().zip(actual_types) {
            if let Err(error) = unify_type_pattern(expected, actual, &mut substitutions) {
                return Err(Diagnostic::at(
                    span,
                    format!("argument type mismatch for {operation}: {}", error.message),
                ));
            }
        }

        for type_param in method_type_params {
            if !substitutions.contains_key(type_param) {
                return Err(Diagnostic::at(
                    span,
                    format!("cannot infer type parameter `{type_param}` for {operation}"),
                ));
            }
        }

        for (type_param, bounds) in type_param_bounds {
            let Some(resolved_ty) = substitutions.get(type_param) else {
                continue;
            };
            let resolved_bounds = bounds
                .iter()
                .map(|bound| substitute_trait_bound(bound, &substitutions))
                .collect::<Vec<_>>();
            self.assert_type_satisfies_bounds(resolved_ty, &resolved_bounds, span)?;
        }

        self.enforce_rng_clone_obligations(
            operation,
            rng_clone_safe_type_params,
            &substitutions,
            span,
        )?;
        Ok(substitutions)
    }

    fn check_builtin_argument_type(
        &self,
        argument: &Argument,
        expected: &Type,
        locals: &mut HashMap<String, LocalBinding>,
        label: &str,
    ) -> Result<()> {
        let actual = self.type_of_expr_hint(&argument.value, locals, Some(expected))?;
        if actual != *expected {
            return Err(Diagnostic::at(
                argument.span,
                format!("`{}` expects `{}`, found `{}`", label, expected, actual),
            ));
        }
        Ok(())
    }

    fn apply_builtin_argument_passing(
        &self,
        member: BuiltinMember,
        index: usize,
        argument: &Argument,
        locals: &mut HashMap<String, LocalBinding>,
    ) -> Result<()> {
        let passing = member
            .argument_passing(index)
            .expect("type-checked builtin argument must have passing metadata");
        self.apply_operator_operand_passing(
            &argument.value,
            passing,
            &format!("builtin method `{}` argument", member.name()),
            locals,
        )
    }

    fn apply_operator_operand_passing(
        &self,
        expr: &Expr,
        passing: ReceiverKind,
        label: &str,
        locals: &mut HashMap<String, LocalBinding>,
    ) -> Result<()> {
        match passing {
            ReceiverKind::Value => self.consume_value_expr(expr, locals),
            ReceiverKind::Borrow => Ok(()),
            ReceiverKind::BorrowMut => {
                if !self.is_mutable_place(expr, locals)? {
                    return Err(Diagnostic::coded_at(
                        "AU3002",
                        expr.span,
                        format!(
                            "{} is declared `borrow mut` and requires a mutable place",
                            label
                        ),
                    ));
                }
                if let Some(place) = self.borrow_call_place(expr) {
                    self.ensure_place_not_frozen(&place, expr.span, locals)?;
                }
                Ok(())
            }
        }
    }

    fn check_optional_builtin_timeout_argument(
        &self,
        ordered_args: &[Option<&Argument>],
        index: usize,
        locals: &mut HashMap<String, LocalBinding>,
        label: &str,
    ) -> Result<()> {
        if let Some(argument) = ordered_args.get(index).copied().flatten() {
            self.check_builtin_argument_type(argument, &Type::named("Duration"), locals, label)?;
        }
        Ok(())
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
                    managed_resource: false,
                    passing: ReceiverKind::Value,
                    borrow_origin: None,
                    borrowed_at: None,
                    match_borrow_mut_place: None,
                    stale_match_borrow_mut_place: None,
                    shared_match_scrutinee: None,
                    moved: false,
                    moved_at: None,
                    moved_fields: BTreeMap::new(),
                    frozen_places: BTreeMap::new(),
                },
            );
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn new(
        module_name: &'a str,
        type_names: &'a BTreeMap<String, crate::diag::Span>,
        type_arities: &'a BTreeMap<String, usize>,
        canonical_type_names: &'a BTreeMap<String, String>,
        classes: &'a BTreeMap<String, ClassInfo>,
        enums: &'a BTreeMap<String, EnumInfo>,
        functions: &'a BTreeMap<String, FunctionInfo>,
        traits: &'a BTreeMap<String, TraitInfo>,
        trait_impls: &'a [TraitImplInfo],
        imported_modules: &'a BTreeMap<String, ModuleNamespace>,
        module_registry: &'a BTreeMap<String, ModuleNamespace>,
    ) -> Self {
        Self {
            root_module_name: module_name,
            module_name,
            type_names,
            type_arities,
            canonical_type_names,
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
            implicit_borrowed_params: BTreeMap::new(),
            active_match_borrow_mut_places: Rc::new(RefCell::new(Vec::new())),
            rng_clone_obligations: Rc::new(RefCell::new(BTreeSet::new())),
            expr_result_entries: Rc::new(RefCell::new(HashMap::new())),
        }
    }

    fn with_return_type(&self, return_type: Type) -> Self {
        Self {
            root_module_name: self.root_module_name,
            module_name: self.module_name,
            type_names: self.type_names,
            type_arities: self.type_arities,
            canonical_type_names: self.canonical_type_names,
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
            implicit_borrowed_params: self.implicit_borrowed_params.clone(),
            active_match_borrow_mut_places: self.active_match_borrow_mut_places.clone(),
            rng_clone_obligations: self.rng_clone_obligations.clone(),
            expr_result_entries: self.expr_result_entries.clone(),
        }
    }

    fn with_type_params(
        &self,
        type_params: BTreeMap<String, ()>,
        type_param_bounds: BTreeMap<String, Vec<TraitBound>>,
    ) -> Self {
        Self {
            root_module_name: self.root_module_name,
            module_name: self.module_name,
            type_names: self.type_names,
            type_arities: self.type_arities,
            canonical_type_names: self.canonical_type_names,
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
            implicit_borrowed_params: self.implicit_borrowed_params.clone(),
            active_match_borrow_mut_places: self.active_match_borrow_mut_places.clone(),
            rng_clone_obligations: self.rng_clone_obligations.clone(),
            expr_result_entries: self.expr_result_entries.clone(),
        }
    }

    fn with_module_name(&self, module_name: &'a str) -> Self {
        Self {
            root_module_name: self.root_module_name,
            module_name,
            type_names: self.type_names,
            type_arities: self.type_arities,
            canonical_type_names: self.canonical_type_names,
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
            implicit_borrowed_params: self.implicit_borrowed_params.clone(),
            active_match_borrow_mut_places: self.active_match_borrow_mut_places.clone(),
            rng_clone_obligations: self.rng_clone_obligations.clone(),
            expr_result_entries: self.expr_result_entries.clone(),
        }
    }

    fn with_rng_clone_obligation_sink(&self, sink: Rc<RefCell<BTreeSet<String>>>) -> Self {
        Self {
            root_module_name: self.root_module_name,
            module_name: self.module_name,
            type_names: self.type_names,
            type_arities: self.type_arities,
            canonical_type_names: self.canonical_type_names,
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
            implicit_borrowed_params: self.implicit_borrowed_params.clone(),
            active_match_borrow_mut_places: self.active_match_borrow_mut_places.clone(),
            rng_clone_obligations: sink,
            expr_result_entries: self.expr_result_entries.clone(),
        }
    }

    fn with_implicit_param_borrows(
        mut self,
        params: &[Param],
        param_types: &[Type],
        param_passings: &[ReceiverKind],
    ) -> Self {
        self.implicit_borrowed_params = params
            .iter()
            .zip(param_types)
            .zip(param_passings)
            .filter(|((param, _), passing)| {
                param.mode == ParamMode::Default && **passing == ReceiverKind::Borrow
            })
            .map(|((param, ty), _)| (param.name.clone(), ty.clone()))
            .collect();
        self
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
                    self.canonical_type_names,
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

    fn validate_float_context_integer_literal(
        &self,
        value: u128,
        negative: bool,
        target_ty: &Type,
        span: crate::diag::Span,
    ) -> Result<()> {
        let integer = IntegerValue::from_literal(value);
        let exactly_representable = match target_ty {
            Type::Named(name, args) if args.is_empty() && name == "float32" => {
                integer.to_exact_f32().is_some()
            }
            Type::Named(name, args) if args.is_empty() && name == "float64" => {
                integer.to_exact_f64().is_some()
            }
            _ => return Ok(()),
        };
        if exactly_representable {
            return Ok(());
        }

        let rendered_value = if negative {
            format!("-{}", value)
        } else {
            value.to_string()
        };
        Err(Diagnostic::coded_at(
            "AU2002",
            span,
            format!(
                "integer literal `{}` cannot be represented exactly as `{}`; write an explicit float spelling such as `{}.0` or use `.to_float()` when rounding is intended",
                rendered_value, target_ty, rendered_value
            ),
        ))
    }

    fn consume_binding(
        &self,
        name: &str,
        span: crate::diag::Span,
        locals: &mut HashMap<String, LocalBinding>,
    ) -> Result<()> {
        self.ensure_place_not_frozen_for_move(&PlacePath::root(name.to_string()), span, locals)?;
        let binding = locals
            .get_mut(name)
            .ok_or_else(|| Diagnostic::at(span, format!("unknown name `{}`", name)))?;
        if self.is_copy_type(&binding.ty) {
            return Ok(());
        }
        let clone_supported = self.type_supports_builtin_clone(&binding.ty);
        if binding.passing != ReceiverKind::Value {
            if let Some(ty) = self.implicit_borrowed_params.get(name) {
                let mut diagnostic = Diagnostic::at(
                    span,
                    if clone_supported {
                        format!(
                            "parameter `{}` is borrowed; declare it as `own {}` to take ownership, or clone the value before consuming it",
                            name, ty
                        )
                    } else {
                        format!(
                            "parameter `{}` is borrowed; declare it as `own {}` to take ownership",
                            name, ty
                        )
                    },
                );
                if let Some(origin) = binding.borrowed_at {
                    diagnostic = diagnostic
                        .with_secondary(origin, format!("parameter `{}` is borrowed here", name));
                }
                diagnostic = diagnostic.with_help(if clone_supported {
                    format!(
                        "declare the parameter as `own {}` when the function should consume it, or call `.clone()` to consume an independent copy",
                        ty
                    )
                } else {
                    format!(
                        "declare the parameter as `own {}` when the function should consume this non-cloneable value",
                        ty
                    )
                });
                if clone_supported {
                    let insertion = crate::diag::Span::new(
                        span.line,
                        span.column.saturating_add(name.chars().count()),
                    );
                    diagnostic = diagnostic.with_edit(insertion, insertion, ".clone()");
                }
                return Err(diagnostic);
            }
            // ADR-0022 Q2: a payload bound by a bare match has exactly one
            // replacement, so name it instead of the generic borrowed-move
            // guidance that would send the caller looking for a parameter.
            if let Some(scrutinee) = &binding.shared_match_scrutinee {
                let mut diagnostic = Diagnostic::coded_at(
                    "AU3002",
                    span,
                    format!("cannot move `{name}` out of a shared match on `{scrutinee}`"),
                );
                if let Some(origin) = binding.borrowed_at {
                    diagnostic = diagnostic.with_secondary(origin, "value is borrowed here");
                }
                diagnostic = diagnostic.with_help(if clone_supported {
                    format!(
                        "write `match own {scrutinee}` to consume the scrutinee, or call `.clone()` to consume an independent copy"
                    )
                } else {
                    format!(
                        "write `match own {scrutinee}` to consume the scrutinee; `{}` cannot be cloned",
                        binding.ty
                    )
                });
                if clone_supported {
                    let insertion = crate::diag::Span::new(
                        span.line,
                        span.column.saturating_add(name.chars().count()),
                    );
                    diagnostic = diagnostic.with_edit(insertion, insertion, ".clone()");
                }
                return Err(diagnostic);
            }
            let mut diagnostic =
                Diagnostic::at(span, format!("cannot move borrowed value `{}`", name));
            if let Some(origin) = binding.borrowed_at {
                diagnostic = diagnostic.with_secondary(origin, "value is borrowed here");
            }
            diagnostic = diagnostic.with_help(if clone_supported {
                format!(
                    "take `{}` as `own {}` when ownership is required, or call `.clone()` to consume an independent copy",
                    name, binding.ty
                )
            } else {
                format!(
                    "take `{}` as `own {}` when ownership of this non-cloneable value is required",
                    name, binding.ty
                )
            });
            if clone_supported {
                let insertion = crate::diag::Span::new(
                    span.line,
                    span.column.saturating_add(name.chars().count()),
                );
                diagnostic = diagnostic.with_edit(insertion, insertion, ".clone()");
            }
            return Err(diagnostic);
        }
        if binding.managed_resource {
            return Err(Diagnostic::at(
                span,
                format!("cannot move managed `with` resource `{}`", name),
            ));
        }
        if binding.moved {
            return Err(self.moved_value_diagnostic(name, span, binding));
        }
        binding.moved = true;
        binding.moved_at = Some(span);
        Ok(())
    }

    fn type_supports_builtin_clone(&self, ty: &Type) -> bool {
        matches!(ty, Type::Named(name, _) if BuiltinMember::resolve(name, "clone").is_some())
            && self.rng_clone_safety(ty) == RngCloneSafety::Safe
    }

    fn moved_value_diagnostic(
        &self,
        name: &str,
        span: crate::diag::Span,
        binding: &LocalBinding,
    ) -> Diagnostic {
        let mut diagnostic = Diagnostic::at(span, format!("use of moved value `{}`", name));
        if let Some(origin) = binding.moved_at {
            diagnostic = diagnostic.with_secondary(origin, "value moved here");
            if self.type_supports_builtin_clone(&binding.ty) {
                diagnostic = diagnostic.with_help(
                    "pass a shared borrow when ownership is not needed, or call `.clone()` at the move site when an independent value is required",
                );
                let insertion = crate::diag::Span::new(
                    origin.line,
                    origin.column.saturating_add(name.chars().count()),
                );
                diagnostic = diagnostic.with_edit(insertion, insertion, ".clone()");
            } else {
                diagnostic = diagnostic.with_help(
                    "pass a shared borrow when ownership is not needed, or transfer this non-cloneable value only once",
                );
            }
        }
        diagnostic
    }

    fn consume_value_expr(
        &self,
        expr: &Expr,
        locals: &mut HashMap<String, LocalBinding>,
    ) -> Result<()> {
        let key = expr as *const Expr as usize;
        let Some(entry) = self.expr_result_entries.borrow_mut().remove(&key) else {
            return self.consume_value_expr_raw(expr, locals);
        };

        // Type checking and ownership transfer are separate at most call
        // sites. Replaying from the state immediately before this expression
        // lets owned result transfer happen in source order with moves caused
        // while evaluating the expression itself. Isolate entries created by
        // replay so they can serve nested owned arguments without reusing or
        // leaking entries from the original type-check pass.
        let post_typecheck = locals.clone();
        let mut replay_locals = entry.locals;
        let saved_entries = std::mem::take(&mut *self.expr_result_entries.borrow_mut());
        let replay_result =
            self.type_expr_consuming_result(expr, &mut replay_locals, entry.expected.as_ref());
        *self.expr_result_entries.borrow_mut() = saved_entries;
        replay_result?;
        self.merge_control_flow_moves(locals, &[&post_typecheck, &replay_locals]);
        Ok(())
    }

    fn consume_value_expr_raw(
        &self,
        expr: &Expr,
        locals: &mut HashMap<String, LocalBinding>,
    ) -> Result<()> {
        match &expr.kind {
            ExprKind::Name(name) if name == "None" => Ok(()),
            ExprKind::Name(name) => self.consume_binding(name, expr.span, locals),
            ExprKind::Group(inner) => self.consume_value_expr_raw(inner, locals),
            ExprKind::Specialize { expr, .. } => self.consume_value_expr_raw(expr, locals),
            ExprKind::Member { object, field } => {
                if self.is_payload_free_variant_expr(expr) {
                    return Ok(());
                }
                let object_ty = self.type_of_member_object_expr(object, locals)?;
                let member_ty = self.resolve_member_type(&object_ty, field, expr.span)?;
                self.consume_typed_member_value_expr(
                    expr, object, field, &object_ty, &member_ty, locals,
                )
            }
            ExprKind::Index { .. } => self.type_of_expr(expr, locals).map(|_| ()),
            // Composite, branching, and fallible results have one branch-aware
            // walk. Reaching it from here means the expression is consumed
            // without a recorded pre-expression state, so the walk starts from
            // the current state instead of a replayed one.
            _ if Self::result_consumption_needs_replay(expr) => self
                .type_expr_consuming_result(expr, locals, None)
                .map(|_| ()),
            _ => Ok(()),
        }
    }

    fn consume_typed_member_value_expr(
        &self,
        expr: &Expr,
        object: &Expr,
        field: &str,
        object_ty: &Type,
        member_ty: &Type,
        locals: &mut HashMap<String, LocalBinding>,
    ) -> Result<()> {
        if self.is_copy_type(member_ty) {
            return Ok(());
        }
        if let Some(name) = self.borrowed_root_binding_name(object, locals) {
            let mut diagnostic = Diagnostic::at(
                expr.span,
                format!(
                    "cannot move non-copy field `{}` out of borrowed value `{}`",
                    field, name
                ),
            );
            if let Some(origin) = locals.get(&name).and_then(|binding| binding.borrowed_at) {
                diagnostic =
                    diagnostic.with_secondary(origin, format!("`{}` is borrowed here", name));
            }
            let clone_supported = self.type_supports_builtin_clone(member_ty);
            diagnostic = diagnostic.with_help(if clone_supported {
                format!(
                    "take `{}` as `own {}` when the field should be moved, or call `.clone()` on the field to return an independent value",
                    name, object_ty
                )
            } else {
                format!(
                    "take `{}` as `own {}` when this non-cloneable field should be moved",
                    name, object_ty
                )
            });
            if clone_supported {
                let insertion = crate::diag::Span::new(
                    expr.span.line,
                    expr.span.column.saturating_add(field.chars().count()),
                );
                diagnostic = diagnostic.with_edit(insertion, insertion, ".clone()");
            }
            return Err(diagnostic);
        }
        if let Some(path) = self.member_access_path(expr) {
            self.ensure_place_not_frozen_for_move(&path, expr.span, locals)?;
            if let Some(binding) = locals.get_mut(&path.root) {
                if binding.managed_resource {
                    return Err(Diagnostic::at(
                        expr.span,
                        format!(
                            "cannot move non-copy field `{}` out of managed `with` resource `{}`",
                            field, path.root
                        ),
                    ));
                }
                binding.moved_fields.insert(path.projections, expr.span);
            }
        }
        Ok(())
    }

    fn consume_match_scrutinee_expr(
        &self,
        expr: &Expr,
        locals: &mut HashMap<String, LocalBinding>,
    ) -> Result<()> {
        let ungrouped = match &expr.kind {
            ExprKind::Group(inner) => inner.as_ref(),
            _ => expr,
        };
        if let ExprKind::Member { object, field } = &ungrouped.kind {
            let object_ty = self.type_of_member_object_expr(object, locals)?;
            let member_ty = self.resolve_member_type(&object_ty, field, ungrouped.span)?;
            if !self.is_copy_type(&member_ty) {
                if let Some(root) = self.borrowed_root_binding_name(object, locals) {
                    let rendered_place = self.render_place_expr(ungrouped);
                    let mut diagnostic = Diagnostic::coded_at(
                        "AU3002",
                        ungrouped.span,
                        format!(
                            "cannot move non-copy field `{}` out of borrowed value `{}` in match scrutinee; use `match {}:` to inspect it by shared access",
                            field,
                            root,
                            rendered_place
                        ),
                    );
                    if let Some(origin) = locals.get(&root).and_then(|binding| binding.borrowed_at)
                    {
                        diagnostic = diagnostic
                            .with_secondary(origin, format!("`{}` is borrowed here", root));
                    }
                    diagnostic = diagnostic.with_help(format!(
                        "use `match {}:` to inspect the field without moving it",
                        rendered_place
                    ));
                    // The old fix inserted `borrow ` before the scrutinee. The
                    // new fix is to delete the `own` keyword, whose span this
                    // check does not carry, so the precise help text stands
                    // alone rather than offering an edit that would write a
                    // retired spelling.
                    return Err(diagnostic);
                }
            }
        }
        self.consume_value_expr(expr, locals)
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
                binding.moved_at = branch_states.iter().find_map(|state| {
                    state
                        .get(&name)
                        .filter(|binding| binding.moved)
                        .and_then(|binding| binding.moved_at)
                });
                binding.moved_fields = branch_states
                    .iter()
                    .filter_map(|state| state.get(&name))
                    .flat_map(|binding| {
                        binding
                            .moved_fields
                            .iter()
                            .map(|(path, span)| (path.clone(), *span))
                    })
                    .collect();
                binding.stale_match_borrow_mut_place = branch_states.iter().find_map(|state| {
                    state
                        .get(&name)
                        .and_then(|binding| binding.stale_match_borrow_mut_place.clone())
                });
            }
        }
    }

    fn const_bool_value(&self, expr: &Expr) -> Option<bool> {
        match &expr.kind {
            ExprKind::Bool(value) => Some(*value),
            ExprKind::Group(inner) => self.const_bool_value(inner),
            ExprKind::Unary {
                op: UnaryOp::Not,
                expr: inner,
            } => self.const_bool_value(inner).map(|value| !value),
            _ => None,
        }
    }

    fn ensure_pattern_binding_not_stale(
        &self,
        name: &str,
        span: crate::diag::Span,
        binding: &LocalBinding,
    ) -> Result<()> {
        if let Some(place) = &binding.stale_match_borrow_mut_place {
            return Err(Diagnostic::at(
                span,
                format!(
                    "cannot use pattern binding `{}` after reassigning match scrutinee `{}`",
                    name, place
                ),
            ));
        }
        Ok(())
    }

    fn invalidate_match_borrow_mut_bindings_for_place(
        &self,
        place: &PlacePath,
        locals: &mut HashMap<String, LocalBinding>,
    ) {
        for binding in locals.values_mut() {
            if binding
                .match_borrow_mut_place
                .as_ref()
                .is_some_and(|binding_place| binding_place.overlaps(place))
            {
                binding.stale_match_borrow_mut_place = binding.match_borrow_mut_place.clone();
            }
        }
    }

    fn invalidate_match_borrow_mut_bindings_for_borrowed_places(
        &self,
        places: &[BorrowedCallPlace],
        locals: &mut HashMap<String, LocalBinding>,
    ) {
        for place in places {
            if place.passing == ReceiverKind::BorrowMut {
                self.invalidate_match_borrow_mut_bindings_for_place(&place.path, locals);
            }
        }
    }

    fn module_enum_type_name(&self, module_path: &str, enum_info: &EnumInfo) -> String {
        format!("{}.{}", module_path, enum_info.decl.name)
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
                .keys()
                .any(|field| !binding.moved_fields.contains_key(field))
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
        self_type: Option<&Type>,
        allow_defaults: bool,
        owner: &str,
    ) -> Result<()> {
        let mut saw_default = false;
        let param_names = params
            .iter()
            .map(|param| param.name.clone())
            .collect::<Vec<_>>();

        for param in params {
            if param.mode == ParamMode::BorrowMut && param.default.is_some() {
                return Err(Diagnostic::coded_at(
                    "AU3002",
                    param.span,
                    format!(
                        "`mut` parameter `{}` cannot have a default: the default creates a caller-invisible temporary, so mutations through it would be silently lost; require the caller to pass a value, or take the parameter as `own T` and return the result",
                        param.name
                    ),
                ));
            }
            let lowered = lower_type_with_self(
                &param.ty,
                self.type_names,
                self.type_arities,
                self.canonical_type_names,
                type_param_scope,
                self_type,
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
                    if matches!(default.kind, ExprKind::BuiltinOmitted) {
                        continue;
                    }
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

    fn check_trait_method(
        &self,
        trait_info: &TraitInfo,
        method_info: &TraitMethodInfo,
    ) -> Result<()> {
        let method = &method_info.decl;
        if method.body.is_empty() {
            return Ok(());
        }

        let trait_type_param_scope = type_param_scope(&trait_info.decl.type_params);
        let self_placeholder = Type::TypeParam("Self".to_string());
        let method_type_param_scope =
            merged_type_param_scope(&trait_type_param_scope, &method.type_params);
        let mut type_param_bounds = method_info.type_param_bounds.clone();
        let self_bounds = type_param_bounds.entry("Self".to_string()).or_default();
        self_bounds.push(TraitBound {
            trait_name: trait_info.decl.name.clone(),
            trait_args: trait_info
                .decl
                .type_params
                .iter()
                .cloned()
                .map(Type::TypeParam)
                .collect(),
        });
        let return_type = method_info.signature.return_type.clone();
        let checker = self
            .with_type_params(method_type_param_scope.clone(), type_param_bounds)
            .with_return_type(return_type.clone())
            .with_implicit_param_borrows(
                &method.params,
                &method_info.signature.params,
                &method_info.signature.param_passings,
            );
        let mut locals = HashMap::new();
        checker.seed_imported_modules(&mut locals);
        if let Some(receiver_kind) = method.receiver {
            locals.insert(
                "self".to_string(),
                LocalBinding {
                    ty: self_placeholder,
                    assignable: false,
                    mutable_place: receiver_kind == ReceiverKind::BorrowMut,
                    managed_resource: false,
                    passing: receiver_kind,
                    borrow_origin: (receiver_kind != ReceiverKind::Value)
                        .then(|| "self".to_string()),
                    borrowed_at: (receiver_kind != ReceiverKind::Value).then_some(method.span),
                    match_borrow_mut_place: None,
                    stale_match_borrow_mut_place: None,
                    shared_match_scrutinee: None,
                    moved: false,
                    moved_at: None,
                    moved_fields: BTreeMap::new(),
                    frozen_places: BTreeMap::new(),
                },
            );
        }
        for ((param, ty), passing) in method
            .params
            .iter()
            .zip(method_info.signature.params.iter())
            .zip(method_info.signature.param_passings.iter().copied())
        {
            locals.insert(
                param.name.clone(),
                LocalBinding {
                    ty: ty.clone(),
                    assignable: false,
                    mutable_place: passing == ReceiverKind::BorrowMut,
                    managed_resource: false,
                    passing,
                    borrow_origin: (passing != ReceiverKind::Value).then(|| param.name.clone()),
                    borrowed_at: (passing != ReceiverKind::Value).then_some(param.span),
                    match_borrow_mut_place: None,
                    stale_match_borrow_mut_place: None,
                    shared_match_scrutinee: None,
                    moved: false,
                    moved_at: None,
                    moved_fields: BTreeMap::new(),
                    frozen_places: BTreeMap::new(),
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

    fn check_function(&self, function_info: &FunctionInfo) -> Result<()> {
        let function = &function_info.decl;
        let type_param_scope = type_param_scope(&function.type_params);
        let type_param_bounds = lower_trait_bounds(
            &function.type_param_bounds,
            self.traits,
            self.type_names,
            self.type_arities,
            self.canonical_type_names,
            &type_param_scope,
        )?;
        let return_type = lower_type(
            &function.return_type,
            self.type_names,
            self.type_arities,
            self.canonical_type_names,
            &type_param_scope,
        )?;
        let checker = self
            .with_type_params(type_param_scope.clone(), type_param_bounds)
            .with_return_type(return_type.clone())
            .with_implicit_param_borrows(
                &function.params,
                &function_info.signature.params,
                &function_info.signature.param_passings,
            );
        checker.check_param_defaults(
            &function.params,
            &type_param_scope,
            None,
            true,
            "function",
        )?;
        let mut locals = HashMap::new();
        checker.seed_imported_modules(&mut locals);
        for ((param, ty), passing) in function
            .params
            .iter()
            .zip(function_info.signature.params.iter())
            .zip(function_info.signature.param_passings.iter().copied())
        {
            locals.insert(
                param.name.clone(),
                LocalBinding {
                    ty: ty.clone(),
                    assignable: false,
                    mutable_place: passing == ReceiverKind::BorrowMut,
                    managed_resource: false,
                    passing,
                    borrow_origin: (passing != ReceiverKind::Value).then(|| param.name.clone()),
                    borrowed_at: (passing != ReceiverKind::Value).then_some(param.span),
                    match_borrow_mut_place: None,
                    stale_match_borrow_mut_place: None,
                    shared_match_scrutinee: None,
                    moved: false,
                    moved_at: None,
                    moved_fields: BTreeMap::new(),
                    frozen_places: BTreeMap::new(),
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

    fn check_method(&self, class_decl: &ClassDecl, method_info: &MethodInfo) -> Result<()> {
        let method = &method_info.decl;
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
                self.canonical_type_names,
                &method_type_param_scope,
                Some(&class_self_type),
            )?,
        );
        let return_type = lower_type_with_self(
            &method.return_type,
            self.type_names,
            self.type_arities,
            self.canonical_type_names,
            &method_type_param_scope,
            Some(&class_self_type),
        )?;
        let checker = self
            .with_type_params(method_type_param_scope.clone(), type_param_bounds)
            .with_return_type(return_type.clone())
            .with_implicit_param_borrows(
                &method.params,
                &method_info.signature.params,
                &method_info.signature.param_passings,
            );
        checker.check_param_defaults(
            &method.params,
            &method_type_param_scope,
            Some(&class_self_type),
            true,
            "method",
        )?;
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
                    managed_resource: false,
                    passing: receiver_kind,
                    borrow_origin: (receiver_kind != ReceiverKind::Value)
                        .then(|| "self".to_string()),
                    borrowed_at: (receiver_kind != ReceiverKind::Value).then_some(method.span),
                    match_borrow_mut_place: None,
                    stale_match_borrow_mut_place: None,
                    shared_match_scrutinee: None,
                    moved: false,
                    moved_at: None,
                    moved_fields: BTreeMap::new(),
                    frozen_places: BTreeMap::new(),
                },
            );
        }
        for ((param, ty), passing) in method
            .params
            .iter()
            .zip(method_info.signature.params.iter())
            .zip(method_info.signature.param_passings.iter().copied())
        {
            locals.insert(
                param.name.clone(),
                LocalBinding {
                    ty: ty.clone(),
                    assignable: false,
                    mutable_place: passing == ReceiverKind::BorrowMut,
                    managed_resource: false,
                    passing,
                    borrow_origin: (passing != ReceiverKind::Value).then(|| param.name.clone()),
                    borrowed_at: (passing != ReceiverKind::Value).then_some(param.span),
                    match_borrow_mut_place: None,
                    stale_match_borrow_mut_place: None,
                    shared_match_scrutinee: None,
                    moved: false,
                    moved_at: None,
                    moved_fields: BTreeMap::new(),
                    frozen_places: BTreeMap::new(),
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

    fn check_trait_impl_supertraits(&self, trait_impl: &TraitImplInfo) -> Result<()> {
        let Some(trait_info) = self.traits.get(&trait_impl.trait_name) else {
            return Ok(());
        };
        let substitutions = self_type_substitutions(
            &trait_info.decl,
            &trait_impl.trait_args,
            trait_impl.for_type.clone(),
        );
        for supertrait in trait_info
            .supertraits
            .iter()
            .map(|supertrait| substitute_trait_bound(supertrait, &substitutions))
        {
            let implemented_elsewhere = self
                .trait_impls_in_scope()
                .filter(|candidate| {
                    !(candidate.trait_name == trait_impl.trait_name
                        && candidate.trait_args == trait_impl.trait_args
                        && candidate.for_type == trait_impl.for_type
                        && candidate.module_name == trait_impl.module_name)
                })
                .any(|candidate| {
                    let Some(substitutions) =
                        self.trait_impl_substitutions(candidate, &trait_impl.for_type)
                    else {
                        return false;
                    };
                    let implemented = self.resolved_trait_bound_for_impl(candidate, &substitutions);
                    self.trait_bound_closure(&implemented, &trait_impl.for_type)
                        .into_iter()
                        .any(|candidate| candidate == supertrait)
                });
            if !implemented_elsewhere {
                return Err(Diagnostic::at(
                    trait_impl.decl.span,
                    format!(
                        "impl of `{}` for `{}` requires supertrait `{}`",
                        trait_impl.trait_name, trait_impl.for_type, supertrait
                    ),
                ));
            }
        }
        Ok(())
    }

    fn check_trait_impl_method(
        &self,
        for_type: &Type,
        impl_type_params: &[String],
        impl_type_param_bounds: &BTreeMap<String, Vec<TraitBound>>,
        method_info: &TraitImplMethodInfo,
    ) -> Result<()> {
        let method = &method_info.decl;
        let impl_type_param_scope = type_param_scope(impl_type_params);
        let type_param_scope = merged_type_param_scope(&impl_type_param_scope, &method.type_params);
        let type_param_bounds = merge_trait_bounds(
            impl_type_param_bounds,
            &lower_trait_bounds_with_self(
                &method.type_param_bounds,
                self.traits,
                self.type_names,
                self.type_arities,
                self.canonical_type_names,
                &type_param_scope,
                Some(for_type),
            )?,
        );
        let return_type = lower_type_with_self(
            &method.return_type,
            self.type_names,
            self.type_arities,
            self.canonical_type_names,
            &type_param_scope,
            Some(for_type),
        )?;
        let checker = self
            .with_type_params(type_param_scope.clone(), type_param_bounds)
            .with_return_type(return_type.clone())
            .with_implicit_param_borrows(
                &method.params,
                &method_info.signature.params,
                &method_info.signature.param_passings,
            );
        checker.check_param_defaults(
            &method.params,
            &type_param_scope,
            Some(for_type),
            false,
            "impl method",
        )?;
        let mut locals = HashMap::new();
        checker.seed_imported_modules(&mut locals);
        if let Some(receiver_kind) = method.receiver {
            locals.insert(
                "self".to_string(),
                LocalBinding {
                    ty: for_type.clone(),
                    assignable: false,
                    mutable_place: receiver_kind == ReceiverKind::BorrowMut,
                    managed_resource: false,
                    passing: receiver_kind,
                    borrow_origin: (receiver_kind != ReceiverKind::Value)
                        .then(|| "self".to_string()),
                    borrowed_at: (receiver_kind != ReceiverKind::Value).then_some(method.span),
                    match_borrow_mut_place: None,
                    stale_match_borrow_mut_place: None,
                    shared_match_scrutinee: None,
                    moved: false,
                    moved_at: None,
                    moved_fields: BTreeMap::new(),
                    frozen_places: BTreeMap::new(),
                },
            );
        }
        for ((param, ty), passing) in method
            .params
            .iter()
            .zip(method_info.signature.params.iter())
            .zip(method_info.signature.param_passings.iter().copied())
        {
            locals.insert(
                param.name.clone(),
                LocalBinding {
                    ty: ty.clone(),
                    assignable: false,
                    mutable_place: passing == ReceiverKind::BorrowMut,
                    managed_resource: false,
                    passing,
                    borrow_origin: (passing != ReceiverKind::Value).then(|| param.name.clone()),
                    borrowed_at: (passing != ReceiverKind::Value).then_some(param.span),
                    match_borrow_mut_place: None,
                    stale_match_borrow_mut_place: None,
                    shared_match_scrutinee: None,
                    moved: false,
                    moved_at: None,
                    moved_fields: BTreeMap::new(),
                    frozen_places: BTreeMap::new(),
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

    /// Recognizes the compiler-known `for` iterable forms `enumerate(xs)` and
    /// `zip(xs, ys)`. A user definition of either name shadows the loop form.
    fn loop_form<'e>(&self, iterable: &'e Expr) -> Result<Option<LoopForm<'e>>> {
        let ExprKind::Call { callee, args } = &iterable.kind else {
            return Ok(None);
        };
        let ExprKind::Name(name) = &callee.kind else {
            return Ok(None);
        };
        let kind = match name.as_str() {
            "enumerate" => LoopFormKind::Enumerate,
            "zip" => LoopFormKind::Zip,
            _ => return Ok(None),
        };
        if self.functions.contains_key(name)
            || self.resolve_class_info(name).is_some()
            || self.resolve_enum_info(name).is_some()
        {
            return Ok(None);
        }
        let arity = kind.arity();
        if args.len() != arity {
            return Err(Diagnostic::coded_at(
                "AU2004",
                iterable.span,
                format!(
                    "`{}` takes {} iterable{}, found {}",
                    name,
                    arity,
                    if arity == 1 { "" } else { "s" },
                    args.len()
                ),
            ));
        }
        if let Some(named) = args.iter().find(|argument| argument.name.is_some()) {
            return Err(Diagnostic::coded_at(
                "AU2004",
                named.span,
                format!("`{}` takes positional iterables only", name),
            ));
        }
        Ok(Some(LoopForm {
            kind,
            name: kind.name(),
            span: iterable.span,
            iterables: args.iter().map(|argument| &argument.value).collect(),
        }))
    }

    /// Checks `for ... in enumerate(xs):` and `for ... in zip(xs, ys):`. Both
    /// iterate index-addressable collections in lockstep over the bare-loop
    /// borrow default, so neither accepts an ownership modifier.
    fn check_lockstep_for(
        &self,
        for_stmt: &crate::ast::ForStmt,
        form: LoopForm<'_>,
        locals: &mut HashMap<String, LocalBinding>,
        return_type: &Type,
        loop_depth: usize,
        allow_return: bool,
    ) -> Result<()> {
        if for_stmt.borrow_mode.is_some() {
            return Err(Diagnostic::coded_at(
                "AU3002",
                for_stmt.span,
                format!(
                    "`{}` iterates over the bare-loop shared default; write `for ... in {}(...):` without an ownership modifier",
                    form.name, form.name
                ),
            ));
        }

        let mut element_types = Vec::with_capacity(form.iterables.len() + 1);
        if form.kind == LoopFormKind::Enumerate {
            element_types.push(Type::named("int64"));
        }
        let mut any_non_copy = false;
        for iterable in &form.iterables {
            let iterable_ty = self.type_of_expr(iterable, locals)?;
            let Some(element_ty) = lockstep_element_type(&iterable_ty) else {
                return Err(Diagnostic::coded_at(
                    "AU2002",
                    iterable.span,
                    format!(
                        "`{}` requires a `Vec[T]` or `Set[T]` iterable, found `{}`",
                        form.name, iterable_ty
                    ),
                )
                .with_help(
                    "these loop forms read collections by position; iterate a `Range` or `Queue[T]` with the bare `for` form",
                ));
            };
            any_non_copy = any_non_copy || !self.is_copy_type(&element_ty);
            element_types.push(element_ty);
        }

        let binding_ty = Type::Tuple(element_types);
        let binding_passing = if any_non_copy {
            ReceiverKind::Borrow
        } else {
            ReceiverKind::Value
        };

        let mut body_locals = locals.clone();
        for iterable in &form.iterables {
            if let Some(place) = self.borrow_call_place(iterable) {
                let root = place.root.clone();
                if let Some(binding) = body_locals.get_mut(&root) {
                    if place.is_root() {
                        binding.assignable = false;
                        binding.mutable_place = false;
                        if binding.passing == ReceiverKind::Value {
                            binding.passing = ReceiverKind::Borrow;
                            binding.borrow_origin = Some(root);
                        }
                    }
                    binding.frozen_places.insert(place.clone(), iterable.span);
                }
            }
        }

        self.bind_target(
            &for_stmt.target,
            &binding_ty,
            binding_passing,
            false,
            &mut body_locals,
            "loop",
        )?;
        self.check_block(
            &for_stmt.body,
            &mut body_locals,
            return_type,
            loop_depth + 1,
            allow_return,
        )?;
        self.reject_loop_carried_moves(locals, &body_locals, "for", for_stmt.span)?;
        self.merge_control_flow_moves(locals, &[&body_locals]);
        Ok(())
    }

    fn bind_target(
        &self,
        target: &crate::ast::BindingTarget,
        ty: &Type,
        passing: ReceiverKind,
        mutable_place: bool,
        locals: &mut HashMap<String, LocalBinding>,
        context: &str,
    ) -> Result<()> {
        match target {
            crate::ast::BindingTarget::Name { name, span } => {
                if locals.contains_key(name) {
                    return Err(Diagnostic::at(
                        *span,
                        format!(
                            "{} binding `{}` would shadow an existing name",
                            context, name
                        ),
                    ));
                }
                let leaf_passing = if passing == ReceiverKind::BorrowMut {
                    ReceiverKind::BorrowMut
                } else if self.is_copy_type(ty) {
                    ReceiverKind::Value
                } else {
                    passing
                };
                locals.insert(
                    name.clone(),
                    LocalBinding {
                        ty: ty.clone(),
                        assignable: false,
                        mutable_place: mutable_place && leaf_passing == ReceiverKind::BorrowMut,
                        managed_resource: false,
                        passing: leaf_passing,
                        borrow_origin: None,
                        borrowed_at: (leaf_passing != ReceiverKind::Value).then_some(*span),
                        match_borrow_mut_place: None,
                        stale_match_borrow_mut_place: None,
                        shared_match_scrutinee: None,
                        moved: false,
                        moved_at: None,
                        moved_fields: BTreeMap::new(),
                        frozen_places: BTreeMap::new(),
                    },
                );
                Ok(())
            }
            crate::ast::BindingTarget::Tuple { elements, span } => {
                if passing == ReceiverKind::BorrowMut {
                    return Err(Diagnostic::coded_at(
                        "AU3002",
                        *span,
                        "`mut` tuple targets are not supported; bind the tuple to one mutable name and update its elements explicitly",
                    ));
                }
                let Type::Tuple(element_types) = ty else {
                    return Err(Diagnostic::at(
                        *span,
                        format!("tuple binding requires a tuple value, found `{}`", ty),
                    ));
                };
                if elements.len() != element_types.len() {
                    return Err(Diagnostic::at(
                        *span,
                        format!(
                            "tuple binding has {} elements but the value has {}",
                            elements.len(),
                            element_types.len()
                        ),
                    ));
                }
                for (element, element_ty) in elements.iter().zip(element_types) {
                    self.bind_target(element, element_ty, passing, false, locals, context)?;
                }
                Ok(())
            }
        }
    }

    fn check_destructure(
        &self,
        destructure: &crate::ast::DestructureStmt,
        locals: &mut HashMap<String, LocalBinding>,
    ) -> Result<()> {
        let value_ty = self.type_of_expr(&destructure.value, locals)?;
        let mut candidate = locals.clone();
        self.bind_target(
            &destructure.target,
            &value_ty,
            ReceiverKind::Value,
            false,
            &mut candidate,
            "tuple",
        )?;
        self.consume_value_expr(&destructure.value, locals)?;
        self.bind_target(
            &destructure.target,
            &value_ty,
            ReceiverKind::Value,
            false,
            locals,
            "tuple",
        )
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
                Stmt::Destructure(destructure) => self.check_destructure(destructure, locals)?,
                Stmt::Pass(_) => {}
                Stmt::Assert(assert_stmt) => {
                    let condition_ty = self.type_of_expr(&assert_stmt.condition, locals)?;
                    if condition_ty != Type::named("bool") {
                        return Err(Diagnostic::at(
                            assert_stmt.span,
                            format!(
                                "`assert` condition must have type `bool`, found `{}`",
                                condition_ty
                            ),
                        )
                        .with_help(
                            "Aurora has no implicit truthiness; compare the value explicitly, for example `value != 0`",
                        ));
                    }
                    self.consume_value_expr(&assert_stmt.condition, locals)?;

                    if let Some(message) = &assert_stmt.message {
                        let mut message_locals = locals.clone();
                        let message_ty = self.type_of_expr(message, &mut message_locals)?;
                        if message_ty != Type::named("String") {
                            return Err(Diagnostic::at(
                                assert_stmt.span,
                                format!(
                                    "`assert` message must have type `String`, found `{}`",
                                    message_ty
                                ),
                            ));
                        }
                    }
                }
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
                    let mut later_branches_reachable = true;
                    for branch in &if_stmt.branches {
                        let mut branch_locals = locals.clone();
                        let branch_flow = self.check_block(
                            &branch.body,
                            &mut branch_locals,
                            return_type,
                            loop_depth,
                            allow_return,
                        )?;
                        let branch_reachable = later_branches_reachable
                            && self.const_bool_value(&branch.condition) != Some(false);
                        if branch_reachable && branch_flow != BlockFlow::AlwaysReturns {
                            all_return = false;
                            branch_states.push(branch_locals);
                        }
                        if later_branches_reachable
                            && self.const_bool_value(&branch.condition) == Some(true)
                        {
                            later_branches_reachable = false;
                        }
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
                        if later_branches_reachable && else_flow != BlockFlow::AlwaysReturns {
                            all_return = false;
                            else_state = Some(else_locals);
                        }
                    } else if later_branches_reachable {
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
                        if later_branches_reachable {
                            states.push(&baseline_locals);
                        }
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
                    if let Some(form) = self.loop_form(&for_stmt.iterable)? {
                        self.check_lockstep_for(
                            for_stmt,
                            form,
                            locals,
                            return_type,
                            loop_depth,
                            allow_return,
                        )?;
                        continue;
                    }
                    let iterable_ty = self.type_of_expr(&for_stmt.iterable, locals)?;
                    if matches!(&iterable_ty, Type::Named(name, args) if name == "Queue" && args.len() == 1)
                        && for_stmt.borrow_mode.is_some()
                    {
                        return Err(Diagnostic::at(
                            for_stmt.span,
                            "Queue iteration receives values; each received item is already owned by the loop binding, and the Queue handle is a copy value, so ownership modifiers have nothing to modify; use the bare form `for item in queue:`",
                        ));
                    }
                    let (binding_ty, binding_passing, binding_mutable_place) =
                        match (&iterable_ty, for_stmt.borrow_mode) {
                        (Type::Named(name, _), _) if name == "Range" => {
                            (Type::named("int32"), ReceiverKind::Value, false)
                        }
                        (Type::Named(name, args), borrow_mode)
                            if name == "Queue" && args.len() == 1 =>
                        {
                            let element_ty = args[0].clone();
                            debug_assert!(borrow_mode.is_none());
                            (element_ty, ReceiverKind::Value, false)
                        }
                        (Type::Named(name, args), borrow_mode) if name == "Vec" && args.len() == 1 => {
                            if borrow_mode == Some(ReceiverKind::BorrowMut)
                                && !self.is_mutable_place(&for_stmt.iterable, locals)?
                            {
                                return Err(Diagnostic::coded_at(
                                    "AU3002",
                                    for_stmt.iterable.span,
                                    "`for value in mut ...:` requires a mutable `Vec[T]` place",
                                ));
                            }
                            let element_ty = args[0].clone();
                            let passing = match borrow_mode {
                                Some(ReceiverKind::BorrowMut) => ReceiverKind::BorrowMut,
                                None | Some(ReceiverKind::Borrow)
                                    if !self.is_copy_type(&element_ty) =>
                                {
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
                                "`for value in mut ...:` is not supported for `Set[T]`; use `insert`/`remove` on the set directly",
                            ))
                        }
                        (Type::Named(name, args), borrow_mode) if name == "Set" && args.len() == 1 => {
                            let element_ty = args[0].clone();
                            let passing = match borrow_mode {
                                None | Some(ReceiverKind::Borrow)
                                    if !self.is_copy_type(&element_ty) =>
                                {
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
                                    "`for` currently requires a `Range`, `Queue[T]`, `Vec[T]`, or `Set[T]` iterable, found `{}`",
                                    iterable_ty
                                ),
                            ))
                        }
                    };
                    if for_stmt.borrow_mode == Some(ReceiverKind::BorrowMut) {
                        if let Some(place) = self.borrow_call_place(&for_stmt.iterable) {
                            self.ensure_place_not_frozen(&place, for_stmt.iterable.span, locals)?;
                        }
                    }
                    if matches!(
                        (&iterable_ty, for_stmt.borrow_mode),
                        (Type::Named(name, args), Some(ReceiverKind::BorrowMut))
                            if name == "Vec" && args.len() == 1
                    ) && !self.is_mutable_place(&for_stmt.iterable, locals)?
                    {
                        return Err(Diagnostic::coded_at(
                            "AU3002",
                            for_stmt.iterable.span,
                            "`for ... in mut ...` requires a mutable iterable place",
                        ));
                    }
                    if for_stmt.borrow_mode == Some(ReceiverKind::Value)
                        && !self.is_copy_type(&iterable_ty)
                    {
                        self.consume_value_expr(&for_stmt.iterable, locals)?;
                    }
                    let mut body_locals = locals.clone();
                    let effective_borrow_mode = match &iterable_ty {
                        Type::Named(name, args) if name == "Queue" && args.len() == 1 => None,
                        _ => match for_stmt.borrow_mode {
                            Some(ReceiverKind::Value) => None,
                            Some(mode) => Some(mode),
                            None => Some(ReceiverKind::Borrow),
                        },
                    };
                    if let Some(borrow_mode) = effective_borrow_mode {
                        if let Some(place) = self.borrow_call_place(&for_stmt.iterable) {
                            let root = place.root.clone();
                            if let Some(binding) = body_locals.get_mut(&root) {
                                if place.is_root() {
                                    binding.assignable = false;
                                    binding.mutable_place = false;
                                    if binding.passing == ReceiverKind::Value {
                                        binding.passing = borrow_mode;
                                        binding.borrow_origin = Some(root);
                                    }
                                }
                                binding
                                    .frozen_places
                                    .insert(place.clone(), for_stmt.iterable.span);
                            }
                        }
                    }
                    self.bind_target(
                        &for_stmt.target,
                        &binding_ty,
                        binding_passing,
                        binding_mutable_place,
                        &mut body_locals,
                        "loop",
                    )?;
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
                    if self.const_bool_value(&while_stmt.condition) != Some(false) {
                        self.reject_loop_carried_moves(
                            locals,
                            &body_locals,
                            "while",
                            while_stmt.span,
                        )?;
                        self.merge_control_flow_moves(locals, &[&body_locals]);
                    }
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
                managed_resource: true,
                passing: ReceiverKind::Value,
                borrow_origin: None,
                borrowed_at: None,
                match_borrow_mut_place: None,
                stale_match_borrow_mut_place: None,
                shared_match_scrutinee: None,
                moved: false,
                moved_at: None,
                moved_fields: BTreeMap::new(),
                frozen_places: BTreeMap::new(),
            },
        );
        self.check_block(
            &with_stmt.body,
            &mut body_locals,
            return_type,
            loop_depth,
            allow_return,
        )
        .inspect(|&flow| {
            if flow != BlockFlow::AlwaysReturns {
                self.merge_control_flow_moves(locals, &[&body_locals]);
            }
        })
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

            if let Some(place) = self.borrow_call_place(object) {
                self.ensure_place_not_frozen(&place, assign.span, locals)?;
            }
            if !self.is_mutable_place(object, locals)? {
                if self.is_shared_self_place(object, locals) {
                    return Err(self.shared_self_mutation_diagnostic(assign.span, locals));
                }
                return Err(Diagnostic::at(
                    assign.span,
                    format!(
                        "cannot assign through immutable place `{}`",
                        self.render_index_target(object)
                    ),
                ));
            }

            let object_ty = self.type_of_expr(object, locals)?;
            let locals_before_index = locals.clone();
            let target_ty = if let Some(target_ty) = vec_element_type(&object_ty).cloned() {
                self.check_vec_index_type(index, index.span, locals)?;
                if assign.op.is_some() && !self.is_copy_type(&target_ty) {
                    return Err(Diagnostic::coded_at(
                        "AU3006",
                        assign.span,
                        format!(
                            "cannot implicitly copy `{}` out of a vector index for compound assignment; use `get(index)` for an explicit cloned optional read, or `remove(index)` to transfer ownership and assign the result explicitly",
                            target_ty
                        ),
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
                if assign.op.is_some() && !self.is_copy_type(value_ty) {
                    return Err(Diagnostic::coded_at(
                        "AU3006",
                        assign.span,
                        format!(
                            "cannot implicitly copy `{}` out of a map index for compound assignment; use `get(key)` for an explicit cloned optional read, or `remove(key)` to transfer ownership and assign the result explicitly",
                            value_ty
                        ),
                    ));
                }
                value_ty.clone()
            } else {
                return Err(Diagnostic::at(
                    assign.span,
                    format!("cannot index non-vector-or-map value `{}`", object_ty),
                ));
            };

            let retained_target = self
                .retained_place_access(
                    object,
                    &object_ty,
                    ReceiverKind::BorrowMut,
                    "indexed assignment target",
                )
                .into_iter()
                .collect::<Vec<_>>();
            let mut index_borrowed_places = Vec::new();
            self.collect_expr_borrowed_places(
                index,
                &locals_before_index,
                &mut index_borrowed_places,
            )?;
            self.reject_retained_access_overlap(&retained_target, &index_borrowed_places)?;
            self.consume_value_expr(index, locals)?;
            let index_moved_accesses = self.newly_moved_place_accesses(
                &locals_before_index,
                locals,
                "index expression",
                index.span,
            );
            self.reject_retained_access_overlap(&retained_target, &index_moved_accesses)?;
            let locals_before_value = locals.clone();
            let value_ty = self.type_of_expr_hint(&assign.value, locals, Some(&target_ty))?;
            let operator_access = if let Some(op) = assign.op {
                if Self::binary_uses_builtin_value_semantics(op, &target_ty, &value_ty) {
                    None
                } else {
                    self.type_of_binary_operator_via_trait(assign.span, op, &target_ty, &value_ty)?
                }
            } else {
                None
            };
            let rhs_passing = if assign.op.is_some() {
                operator_access
                    .as_ref()
                    .map(|operator| operator.rhs_passing)
                    .unwrap_or(ReceiverKind::Borrow)
            } else {
                ReceiverKind::Value
            };
            self.reject_retained_expr_overlap(
                &retained_target,
                &assign.value,
                &value_ty,
                Some(rhs_passing),
                &locals_before_value,
                locals,
                "indexed assignment value",
            )?;
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

            if assign.op.is_none() {
                self.consume_value_expr(&assign.value, locals)?;
            } else if let Some(operator) = operator_access {
                self.apply_operator_operand_passing(
                    &assign.value,
                    operator.rhs_passing,
                    "operator right operand",
                    locals,
                )?;
            }
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

            if let Some(path) = self.member_target_path(object, field) {
                self.ensure_place_not_frozen(&path, assign.span, locals)?;
            }
            if !self.is_mutable_place(object, locals)? {
                if self.is_shared_self_place(object, locals) {
                    return Err(self.shared_self_mutation_diagnostic(assign.span, locals));
                }
                return Err(Diagnostic::at(
                    assign.span,
                    format!(
                        "cannot assign through immutable place `{}`",
                        self.render_member_target(object, field)
                    ),
                ));
            }

            if let Some(path) = self.member_target_path(object, field) {
                if let Some(binding) = locals.get(&path.root) {
                    if assign.op.is_some() && Self::field_path_is_moved(binding, &path.projections)
                    {
                        return Err(Diagnostic::at(
                            assign.span,
                            format!(
                                "cannot read moved field `{}` from `{}` in compound assignment",
                                path.projections, path.root
                            ),
                        ));
                    }
                }
            }

            let target_ty = self.resolve_member_target_type(object, field, assign.span, locals)?;
            let locals_before_value = locals.clone();
            let value_ty = self.type_of_expr_hint(&assign.value, locals, Some(&target_ty))?;
            let operator_access = if let Some(op) = assign.op {
                if Self::binary_uses_builtin_value_semantics(op, &target_ty, &value_ty) {
                    None
                } else {
                    self.type_of_binary_operator_via_trait(assign.span, op, &target_ty, &value_ty)?
                }
            } else {
                None
            };
            if assign.op.is_some() {
                let retained_target = self
                    .member_target_path(object, field)
                    .and_then(|path| match &operator_access {
                        Some(operator) => self.retained_path_access(
                            path,
                            &target_ty,
                            operator.receiver_passing,
                            "compound assignment target",
                            assign.span,
                        ),
                        None if !self.is_copy_type(&target_ty) => Some(BorrowedCallPlace {
                            path,
                            passing: ReceiverKind::Borrow,
                            param_name: "compound assignment target".to_string(),
                            origin_span: assign.span,
                        }),
                        None => None,
                    })
                    .into_iter()
                    .collect::<Vec<_>>();
                let rhs_passing = operator_access
                    .as_ref()
                    .map(|operator| operator.rhs_passing)
                    .unwrap_or(ReceiverKind::Borrow);
                self.reject_retained_expr_overlap(
                    &retained_target,
                    &assign.value,
                    &value_ty,
                    Some(rhs_passing),
                    &locals_before_value,
                    locals,
                    "compound assignment value",
                )?;
            }
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

            if assign.op.is_none() {
                self.consume_value_expr(&assign.value, locals)?;
            } else if let Some(operator) = operator_access {
                self.apply_operator_operand_passing(
                    &assign.value,
                    operator.rhs_passing,
                    "operator right operand",
                    locals,
                )?;
            }
            if let Some(path) = self.member_target_path(object, field) {
                if let Some(binding) = locals.get_mut(&path.root) {
                    Self::clear_moved_field_path(binding, &path.projections);
                }
                self.invalidate_match_borrow_mut_bindings_for_place(&path, locals);
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
                    self.canonical_type_names,
                    &self.type_params,
                )
            })
            .transpose()?;
        let existing_binding = locals.get(binding_name).cloned();
        if let Some(existing) = &existing_binding {
            self.ensure_pattern_binding_not_stale(binding_name, assign.span, existing)?;
        }
        let existing_ty = existing_binding.as_ref().map(|binding| binding.ty.clone());
        let mut borrow_info_locals = locals.clone();
        let locals_before_value = locals.clone();
        let value_ty = self.type_of_expr_hint(
            &assign.value,
            locals,
            existing_ty.as_ref().or(annotation_ty.as_ref()),
        )?;

        if let Some(existing) = existing_binding {
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

            self.ensure_place_not_frozen(
                &PlacePath::root(binding_name.clone()),
                assign.span,
                locals,
            )?;
            if !existing.assignable && !existing.mutable_place {
                return Err(Diagnostic::coded_at(
                    "AU3003",
                    assign.span,
                    format!(
                        "cannot assign to immutable binding `{}`; declare it with `mut` to rebind it",
                        binding_name
                    ),
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

            let operator_access = if let Some(op) = assign.op {
                if Self::binary_uses_builtin_value_semantics(op, &existing.ty, &value_ty) {
                    None
                } else {
                    self.type_of_binary_operator_via_trait(
                        assign.span,
                        op,
                        &existing.ty,
                        &value_ty,
                    )?
                }
            } else {
                None
            };
            if assign.op.is_some() {
                let retained_target = match &operator_access {
                    Some(operator) => self
                        .retained_path_access(
                            PlacePath::root(binding_name.clone()),
                            &existing.ty,
                            operator.receiver_passing,
                            "compound assignment target",
                            assign.span,
                        )
                        .into_iter()
                        .collect::<Vec<_>>(),
                    None if !self.is_copy_type(&existing.ty) => vec![BorrowedCallPlace {
                        path: PlacePath::root(binding_name.clone()),
                        passing: ReceiverKind::Borrow,
                        param_name: "compound assignment target".to_string(),
                        origin_span: assign.span,
                    }],
                    None => Vec::new(),
                };
                let rhs_passing = operator_access
                    .as_ref()
                    .map(|operator| operator.rhs_passing)
                    .unwrap_or(ReceiverKind::Borrow);
                self.reject_retained_expr_overlap(
                    &retained_target,
                    &assign.value,
                    &value_ty,
                    Some(rhs_passing),
                    &locals_before_value,
                    locals,
                    "compound assignment value",
                )?;
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

            if assign.op.is_none() {
                self.consume_value_expr(&assign.value, locals)?;
            } else if let Some(operator) = operator_access {
                self.apply_operator_operand_passing(
                    &assign.value,
                    operator.rhs_passing,
                    "operator right operand",
                    locals,
                )?;
            }
            if let Some(existing) = locals.get_mut(binding_name) {
                existing.moved = false;
                existing.moved_at = None;
                existing.moved_fields.clear();
            }
            self.invalidate_match_borrow_mut_bindings_for_place(
                &PlacePath::root(binding_name.clone()),
                locals,
            );
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

        if let Some(borrowed) = self.expr_borrow_info(&assign.value, &mut borrow_info_locals)? {
            if self.is_copy_type(&final_ty) {
                self.consume_value_expr(&assign.value, locals)?;
                locals.insert(
                    binding_name.clone(),
                    LocalBinding {
                        ty: final_ty,
                        assignable: assign.mutable,
                        mutable_place: assign.mutable,
                        managed_resource: false,
                        passing: ReceiverKind::Value,
                        borrow_origin: None,
                        borrowed_at: None,
                        match_borrow_mut_place: None,
                        stale_match_borrow_mut_place: None,
                        shared_match_scrutinee: None,
                        moved: false,
                        moved_at: None,
                        moved_fields: BTreeMap::new(),
                        frozen_places: BTreeMap::new(),
                    },
                );
                return Ok(());
            }
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
                    managed_resource: false,
                    passing: borrowed.passing,
                    borrow_origin: Some(borrowed.origin),
                    borrowed_at: Some(assign.value.span),
                    match_borrow_mut_place: None,
                    stale_match_borrow_mut_place: None,
                    shared_match_scrutinee: None,
                    moved: false,
                    moved_at: None,
                    moved_fields: BTreeMap::new(),
                    frozen_places: BTreeMap::new(),
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
                managed_resource: false,
                passing: ReceiverKind::Value,
                borrow_origin: None,
                borrowed_at: None,
                match_borrow_mut_place: None,
                stale_match_borrow_mut_place: None,
                shared_match_scrutinee: None,
                moved: false,
                moved_at: None,
                moved_fields: BTreeMap::new(),
                frozen_places: BTreeMap::new(),
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

    /// Replays an expression from the ownership state that preceded it so the
    /// transfer of its owned result happens in source order with the moves the
    /// expression itself performs.
    ///
    /// A replay only ever runs after `type_of_expr_hint` accepted the same
    /// expression under the same expected type, and typing does not depend on
    /// move state, so every type rule this walk would restate is already
    /// proven. The walk therefore reproduces the accepted result type and
    /// reports ownership diagnostics only; restating the type rules here would
    /// add unreachable branches and a second place for them to drift.
    fn type_expr_consuming_result(
        &self,
        expr: &Expr,
        locals: &mut HashMap<String, LocalBinding>,
        expected: Option<&Type>,
    ) -> Result<Type> {
        match &expr.kind {
            ExprKind::Group(inner) => self.type_expr_consuming_result(inner, locals, expected),
            ExprKind::Cast { expr: inner, .. } => {
                let ty = self.type_of_expr_hint(expr, locals, expected)?;
                self.consume_value_expr(inner, locals)?;
                Ok(ty)
            }
            ExprKind::Try(inner) => {
                let ty = self.type_of_expr_hint(expr, locals, expected)?;
                self.consume_value_expr(inner, locals)?;
                Ok(ty)
            }
            ExprKind::Tuple(elements) => {
                let expected_elements = match expected {
                    Some(Type::Tuple(expected_elements))
                        if expected_elements.len() == elements.len() =>
                    {
                        Some(expected_elements.as_slice())
                    }
                    _ => None,
                };
                let element_types = elements
                    .iter()
                    .enumerate()
                    .map(|(index, element)| {
                        self.type_expr_consuming_result(
                            element,
                            locals,
                            expected_elements.and_then(|types| types.get(index)),
                        )
                    })
                    .collect::<Result<Vec<_>>>()?;
                Ok(Type::Tuple(element_types))
            }
            ExprKind::List(elements) => {
                let mut element_ty = expected.and_then(vec_element_type).cloned();
                for element in elements {
                    let actual =
                        self.type_expr_consuming_result(element, locals, element_ty.as_ref())?;
                    element_ty.get_or_insert(actual);
                }
                Ok(Type::Named(
                    "Vec".to_string(),
                    vec![element_ty.unwrap_or(Type::Unit)],
                ))
            }
            ExprKind::Set(elements) => {
                let mut element_ty = expected.and_then(set_element_type).cloned();
                for element in elements {
                    let actual =
                        self.type_expr_consuming_result(element, locals, element_ty.as_ref())?;
                    element_ty.get_or_insert(actual);
                }
                Ok(Type::Named(
                    "Set".to_string(),
                    vec![element_ty.unwrap_or(Type::Unit)],
                ))
            }
            ExprKind::Map(entries) => {
                if entries.is_empty() {
                    if let Some(Type::Named(name, args)) = expected {
                        if name == "Set" && args.len() == 1 {
                            return Ok(Type::Named("Set".to_string(), vec![args[0].clone()]));
                        }
                    }
                }
                let mut key_ty = expected
                    .and_then(map_key_value_types)
                    .map(|(key_ty, _)| key_ty.clone());
                let mut value_ty = expected
                    .and_then(map_key_value_types)
                    .map(|(_, value_ty)| value_ty.clone());
                for entry in entries {
                    let actual_key =
                        self.type_expr_consuming_result(&entry.key, locals, key_ty.as_ref())?;
                    key_ty.get_or_insert(actual_key);
                    let actual_value =
                        self.type_expr_consuming_result(&entry.value, locals, value_ty.as_ref())?;
                    value_ty.get_or_insert(actual_value);
                }
                Ok(Type::Named(
                    "Map".to_string(),
                    vec![key_ty.unwrap_or(Type::Unit), value_ty.unwrap_or(Type::Unit)],
                ))
            }
            ExprKind::Conditional {
                then_expr,
                condition,
                else_expr,
            } => {
                self.type_of_expr(condition, locals)?;
                let result_ty =
                    self.conditional_result_hint(then_expr, else_expr, locals, expected)?;
                let mut then_locals = locals.clone();
                self.type_expr_consuming_result(then_expr, &mut then_locals, Some(&result_ty))?;
                let mut else_locals = locals.clone();
                self.type_expr_consuming_result(else_expr, &mut else_locals, Some(&result_ty))?;
                self.merge_control_flow_moves(locals, &[&then_locals, &else_locals]);
                Ok(result_ty)
            }
            ExprKind::Member { object, field } if Self::member_projects_branch_result(object) => {
                let (_, member_ty) =
                    self.type_member_result_consuming(object, field, expr.span, locals, None)?;
                Ok(member_ty)
            }
            ExprKind::Match {
                scrutinee,
                capability,
                arms,
            } => self.type_of_match_expr(
                MatchExprParts {
                    scrutinee,
                    borrow_mode: *capability,
                    arms,
                    span: expr.span,
                },
                locals,
                expected,
                BranchResultUse::Consumed,
            ),
            _ => {
                let ty = self.type_of_expr_hint(expr, locals, expected)?;
                self.consume_value_expr_raw(expr, locals)?;
                Ok(ty)
            }
        }
    }

    fn type_member_result_consuming(
        &self,
        object: &Expr,
        field: &str,
        member_span: crate::diag::Span,
        locals: &mut HashMap<String, LocalBinding>,
        expected_object: Option<&Type>,
    ) -> Result<(Type, Type)> {
        match &object.kind {
            ExprKind::Group(inner) => self.type_member_result_consuming(
                inner,
                field,
                member_span,
                locals,
                expected_object,
            ),
            ExprKind::Conditional {
                then_expr,
                condition,
                else_expr,
            } => {
                self.type_of_expr(condition, locals)?;
                let object_ty =
                    self.conditional_result_hint(then_expr, else_expr, locals, expected_object)?;

                let mut then_locals = locals.clone();
                let (_, then_member_ty) = self.type_member_result_consuming(
                    then_expr,
                    field,
                    member_span,
                    &mut then_locals,
                    Some(&object_ty),
                )?;

                let mut else_locals = locals.clone();
                self.type_member_result_consuming(
                    else_expr,
                    field,
                    member_span,
                    &mut else_locals,
                    Some(&object_ty),
                )?;
                self.merge_control_flow_moves(locals, &[&then_locals, &else_locals]);
                Ok((object_ty, then_member_ty))
            }
            ExprKind::Match {
                scrutinee,
                capability,
                arms,
            } => {
                let object_ty = self.type_of_match_expr(
                    MatchExprParts {
                        scrutinee,
                        borrow_mode: *capability,
                        arms,
                        span: object.span,
                    },
                    locals,
                    expected_object,
                    BranchResultUse::ProjectedField(ProjectedField {
                        name: field,
                        span: member_span,
                    }),
                )?;
                let member_ty = self.resolve_member_type(&object_ty, field, member_span)?;
                Ok((object_ty, member_ty))
            }
            _ => {
                let object_ty = self.type_of_member_object_expr(object, locals)?;
                if let Some(expected_object) = expected_object {
                    if object_ty != *expected_object {
                        return Ok((
                            object_ty.clone(),
                            self.resolve_member_type(&object_ty, field, member_span)?,
                        ));
                    }
                }
                let member_ty = self.resolve_member_type(&object_ty, field, member_span)?;
                let member_expr = Expr {
                    kind: ExprKind::Member {
                        object: Box::new(object.clone()),
                        field: field.to_string(),
                    },
                    span: member_span,
                };
                if !self.is_payload_free_variant_expr(&member_expr) {
                    self.consume_typed_member_value_expr(
                        &member_expr,
                        object,
                        field,
                        &object_ty,
                        &member_ty,
                        locals,
                    )?;
                }
                Ok((object_ty, member_ty))
            }
        }
    }

    fn type_of_expr_without_move_state(
        &self,
        expr: &Expr,
        locals: &HashMap<String, LocalBinding>,
        expected: Option<&Type>,
    ) -> Result<Type> {
        let mut snapshot = locals.clone();
        for binding in snapshot.values_mut() {
            binding.moved = false;
            binding.moved_at = None;
            binding.moved_fields.clear();
            binding.stale_match_borrow_mut_place = None;
        }
        let saved_entries = std::mem::take(&mut *self.expr_result_entries.borrow_mut());
        let result = self.type_of_expr_hint(expr, &mut snapshot, expected);
        *self.expr_result_entries.borrow_mut() = saved_entries;
        result
    }

    fn is_contextual_none_expr(expr: &Expr) -> bool {
        match &expr.kind {
            ExprKind::Name(name) => name == "None",
            ExprKind::Group(inner) => Self::is_contextual_none_expr(inner),
            _ => false,
        }
    }

    fn is_integer_literal_expr(expr: &Expr) -> bool {
        match &expr.kind {
            ExprKind::Int(_) => true,
            ExprKind::Group(inner) => Self::is_integer_literal_expr(inner),
            ExprKind::Unary {
                op: UnaryOp::Neg,
                expr: inner,
            } => matches!(inner.kind, ExprKind::Int(_)),
            _ => false,
        }
    }

    fn is_float_literal_expr(expr: &Expr) -> bool {
        match &expr.kind {
            ExprKind::Float(_) => true,
            ExprKind::Group(inner) => Self::is_float_literal_expr(inner),
            _ => false,
        }
    }

    fn conditional_result_hint(
        &self,
        then_expr: &Expr,
        else_expr: &Expr,
        locals: &HashMap<String, LocalBinding>,
        expected: Option<&Type>,
    ) -> Result<Type> {
        if let Some(expected) = expected {
            return Ok(expected.clone());
        }

        if let ExprKind::Group(inner) = &then_expr.kind {
            return self.conditional_result_hint(inner, else_expr, locals, None);
        }
        if let ExprKind::Group(inner) = &else_expr.kind {
            return self.conditional_result_hint(then_expr, inner, locals, None);
        }
        if let (ExprKind::Tuple(then_elements), ExprKind::Tuple(else_elements)) =
            (&then_expr.kind, &else_expr.kind)
        {
            if then_elements.len() == else_elements.len() {
                let element_types = then_elements
                    .iter()
                    .zip(else_elements)
                    .map(|(then_element, else_element)| {
                        self.conditional_result_hint(then_element, else_element, locals, None)
                    })
                    .collect::<Result<Vec<_>>>()?;
                return Ok(Type::Tuple(element_types));
            }
        }
        if let (ExprKind::List(then_elements), ExprKind::List(else_elements)) =
            (&then_expr.kind, &else_expr.kind)
        {
            if !then_elements.is_empty() && then_elements.len() == else_elements.len() {
                let element_types = then_elements
                    .iter()
                    .zip(else_elements)
                    .map(|(then_element, else_element)| {
                        self.conditional_result_hint(then_element, else_element, locals, None)
                    })
                    .collect::<Result<Vec<_>>>()?;
                if let Some(element_ty) = element_types.first() {
                    if element_types.iter().all(|actual| actual == element_ty) {
                        return Ok(Type::Named("Vec".to_string(), vec![element_ty.clone()]));
                    }
                }
            }
        }
        if let (ExprKind::Set(then_elements), ExprKind::Set(else_elements)) =
            (&then_expr.kind, &else_expr.kind)
        {
            if !then_elements.is_empty() && then_elements.len() == else_elements.len() {
                let element_types = then_elements
                    .iter()
                    .zip(else_elements)
                    .map(|(then_element, else_element)| {
                        self.conditional_result_hint(then_element, else_element, locals, None)
                    })
                    .collect::<Result<Vec<_>>>()?;
                if let Some(element_ty) = element_types.first() {
                    if element_types.iter().all(|actual| actual == element_ty) {
                        return Ok(Type::Named("Set".to_string(), vec![element_ty.clone()]));
                    }
                }
            }
        }
        if let (ExprKind::Map(then_entries), ExprKind::Map(else_entries)) =
            (&then_expr.kind, &else_expr.kind)
        {
            if !then_entries.is_empty() && then_entries.len() == else_entries.len() {
                let entry_types = then_entries
                    .iter()
                    .zip(else_entries)
                    .map(|(then_entry, else_entry)| {
                        Ok((
                            self.conditional_result_hint(
                                &then_entry.key,
                                &else_entry.key,
                                locals,
                                None,
                            )?,
                            self.conditional_result_hint(
                                &then_entry.value,
                                &else_entry.value,
                                locals,
                                None,
                            )?,
                        ))
                    })
                    .collect::<Result<Vec<_>>>()?;
                if let Some((key_ty, value_ty)) = entry_types.first() {
                    if entry_types.iter().all(|(actual_key, actual_value)| {
                        actual_key == key_ty && actual_value == value_ty
                    }) {
                        return Ok(Type::Named(
                            "Map".to_string(),
                            vec![key_ty.clone(), value_ty.clone()],
                        ));
                    }
                }
            }
        }

        let then_guess = self.type_of_expr_without_move_state(then_expr, locals, None);
        let else_guess = self.type_of_expr_without_move_state(else_expr, locals, None);
        let (then_ty, else_ty) = match (then_guess, else_guess) {
            (Ok(then_ty), Ok(else_ty)) => (then_ty, else_ty),
            (Err(_), Ok(else_ty)) => return Ok(else_ty),
            (Ok(then_ty), Err(_)) => return Ok(then_ty),
            (Err(error), Err(_)) => return Err(error),
        };

        if then_ty == else_ty {
            return Ok(then_ty);
        }
        let then_adopts_else = self
            .type_of_expr_without_move_state(then_expr, locals, Some(&else_ty))
            .is_ok_and(|actual| actual == else_ty);
        let else_adopts_then = self
            .type_of_expr_without_move_state(else_expr, locals, Some(&then_ty))
            .is_ok_and(|actual| actual == then_ty);
        match (then_adopts_else, else_adopts_then) {
            (true, false) => return Ok(else_ty),
            (false, true) => return Ok(then_ty),
            _ => {}
        }
        if Self::is_contextual_none_expr(then_expr) {
            return Ok(else_ty);
        }
        if Self::is_contextual_none_expr(else_expr) {
            return Ok(then_ty);
        }
        if Self::is_integer_literal_expr(then_expr)
            && (is_float_type(&else_ty) || is_integer_type(&else_ty))
        {
            return Ok(else_ty);
        }
        if Self::is_integer_literal_expr(else_expr)
            && (is_float_type(&then_ty) || is_integer_type(&then_ty))
        {
            return Ok(then_ty);
        }
        if Self::is_float_literal_expr(then_expr)
            && !Self::is_float_literal_expr(else_expr)
            && is_float_type(&else_ty)
        {
            return Ok(else_ty);
        }
        if Self::is_float_literal_expr(else_expr)
            && !Self::is_float_literal_expr(then_expr)
            && is_float_type(&then_ty)
        {
            return Ok(then_ty);
        }
        Ok(then_ty)
    }

    fn equality_operand_hint(
        &self,
        left: &Expr,
        right: &Expr,
        locals: &HashMap<String, LocalBinding>,
    ) -> Option<Type> {
        self.conditional_result_hint(left, right, locals, None).ok()
    }

    fn result_consumption_needs_replay(expr: &Expr) -> bool {
        match &expr.kind {
            ExprKind::Member { object, .. } => Self::member_projects_branch_result(object),
            kind => matches!(
                kind,
                ExprKind::Group(_)
                    | ExprKind::Cast { .. }
                    | ExprKind::Tuple(_)
                    | ExprKind::List(_)
                    | ExprKind::Set(_)
                    | ExprKind::Map(_)
                    | ExprKind::Conditional { .. }
                    | ExprKind::Match { .. }
                    | ExprKind::Try(_)
            ),
        }
    }

    /// A member access only needs branch-aware result consumption when its
    /// object is a branching expression whose owned result is produced by more
    /// than one arm. Ordinary member paths — including module-qualified and
    /// enum-variant paths, which are not value objects at all — keep using the
    /// direct consumption path.
    fn member_projects_branch_result(object: &Expr) -> bool {
        match &object.kind {
            ExprKind::Group(inner) => Self::member_projects_branch_result(inner),
            ExprKind::Conditional { .. } | ExprKind::Match { .. } => true,
            _ => false,
        }
    }

    fn type_of_expr_hint(
        &self,
        expr: &Expr,
        locals: &mut HashMap<String, LocalBinding>,
        expected: Option<&Type>,
    ) -> Result<Type> {
        if Self::result_consumption_needs_replay(expr) {
            self.expr_result_entries.borrow_mut().insert(
                expr as *const Expr as usize,
                ExprResultEntry {
                    locals: locals.clone(),
                    expected: expected.cloned(),
                },
            );
        }
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
                    self.ensure_pattern_binding_not_stale(name, expr.span, binding)?;
                    if binding.moved {
                        return Err(self.moved_value_diagnostic(name, expr.span, binding));
                    }
                    if !binding.moved_fields.is_empty() {
                        let mut diagnostic = Diagnostic::at(
                            expr.span,
                            format!("use of partially moved value `{}`", name),
                        );
                        if let Some(origin) = binding.moved_fields.values().next().copied() {
                            diagnostic = diagnostic
                                .with_secondary(origin, "field moved here")
                                .with_help(
                                    "borrow the field when ownership is not needed, or call `.clone()` before moving it when an independent value is required",
                                );
                        }
                        return Err(diagnostic);
                    }
                    return Ok(binding.ty.clone());
                }
                if let Some(function) = self.resolve_function_info(name) {
                    return Ok(function.signature.return_type.clone());
                }
                if let Some(class_info) = self.resolve_class_info(name) {
                    return Ok(Type::named(self.canonical_class_name(name, class_info)));
                }
                if let Some(enum_info) = self.resolve_enum_info(name) {
                    return Ok(Type::named(self.canonical_enum_info_name(name, enum_info)));
                }
                match name.as_str() {
                    "True" => Err(Diagnostic::coded_at(
                        "AU2005",
                        expr.span,
                        "unknown name `True`; did you mean `true`?",
                    )),
                    "False" => Err(Diagnostic::coded_at(
                        "AU2005",
                        expr.span,
                        "unknown name `False`; did you mean `false`?",
                    )),
                    _ => Err(Diagnostic::at(
                        expr.span,
                        format!("unknown name `{}`", name),
                    )),
                }
            }
            ExprKind::Int(value) => {
                if let Some(target_ty) = expected.filter(|ty| is_float_type(ty)) {
                    self.validate_float_context_integer_literal(
                        *value, false, target_ty, expr.span,
                    )?;
                    Ok(target_ty.clone())
                } else {
                    let target_ty = expected
                        .filter(|ty| is_integer_type(ty))
                        .cloned()
                        .unwrap_or_else(|| Type::named("int64"));
                    self.validate_integer_literal(*value, &target_ty, expr.span)?;
                    Ok(target_ty)
                }
            }
            ExprKind::DurationNanos(_) => Ok(Type::named("Duration")),
            ExprKind::BuiltinOmitted => Err(Diagnostic::at(
                expr.span,
                "internal builtin omitted-default marker cannot be used as a source expression",
            )),
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
            ExprKind::Tuple(elements) => {
                let expected_elements = match expected {
                    Some(Type::Tuple(expected_elements))
                        if expected_elements.len() == elements.len() =>
                    {
                        Some(expected_elements.as_slice())
                    }
                    _ => None,
                };
                let element_types = elements
                    .iter()
                    .enumerate()
                    .map(|(index, element)| {
                        self.type_of_expr_hint(
                            element,
                            locals,
                            expected_elements.and_then(|types| types.get(index)),
                        )
                    })
                    .collect::<Result<Vec<_>>>()?;
                Ok(Type::Tuple(element_types))
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
                Ok(Type::Named("Set".to_string(), vec![element_ty]))
            }
            ExprKind::Map(entries) => {
                if entries.is_empty() {
                    if let Some(Type::Named(name, args)) = expected {
                        if name == "Set" && args.len() == 1 {
                            return Ok(Type::Named("Set".to_string(), vec![args[0].clone()]));
                        }
                    }
                }
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
                Ok(Type::Named("Map".to_string(), vec![key_ty, value_ty]))
            }
            ExprKind::Conditional {
                then_expr,
                condition,
                else_expr,
            } => {
                let condition_ty = self.type_of_expr(condition, locals)?;
                if condition_ty != Type::named("bool") {
                    return Err(Diagnostic::coded_at(
                        "AU2002",
                        condition.span,
                        format!(
                            "conditional expression condition must have type `bool`, found `{}`",
                            condition_ty
                        ),
                    )
                    .with_help("Aurora has no implicit truthiness; compare the value explicitly"));
                }

                let result_ty =
                    self.conditional_result_hint(then_expr, else_expr, locals, expected)?;
                let mut then_locals = locals.clone();
                let then_ty =
                    self.type_of_expr_hint(then_expr, &mut then_locals, Some(&result_ty))?;
                if then_ty != result_ty {
                    return Err(Diagnostic::coded_at(
                        "AU2002",
                        then_expr.span,
                        format!(
                            "conditional expression arm expects `{}`, found `{}`",
                            result_ty, then_ty
                        ),
                    ));
                }

                let mut else_locals = locals.clone();
                let else_ty =
                    self.type_of_expr_hint(else_expr, &mut else_locals, Some(&result_ty))?;
                if else_ty != result_ty {
                    return Err(Diagnostic::coded_at(
                        "AU2002",
                        else_expr.span,
                        format!(
                            "conditional expression arms must have one type; expected `{}`, found `{}`",
                            result_ty, else_ty
                        ),
                    ));
                }
                self.merge_control_flow_moves(locals, &[&then_locals, &else_locals]);
                Ok(result_ty)
            }
            ExprKind::Match {
                scrutinee,
                capability,
                arms,
            } => self.type_of_match_expr(
                MatchExprParts {
                    scrutinee,
                    borrow_mode: *capability,
                    arms,
                    span: expr.span,
                },
                locals,
                expected,
                BranchResultUse::Inspected,
            ),
            ExprKind::Group(inner) => self.type_of_expr_hint(inner, locals, expected),
            ExprKind::Specialize {
                expr: base,
                type_args,
            } => {
                let lowered = self.lower_explicit_type_args(type_args)?;
                match &base.kind {
                    ExprKind::Name(name)
                        if matches!(
                            name.as_str(),
                            "Option"
                                | "Result"
                                | "SendError"
                                | "QueueReceive"
                                | "TaskResult"
                                | "WaitAny"
                                | "WaitAll"
                        ) =>
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
                        let Some(class) = self.resolve_class_info(name) else {
                            unreachable!(
                                "class lookup is stable during explicit type argument checking"
                            );
                        };
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
                        Ok(Type::Named(self.canonical_class_name(name, class), lowered))
                    }
                    ExprKind::Name(name) if self.resolve_enum_info(name).is_some() => {
                        let Some(enum_info) = self.resolve_enum_info(name) else {
                            unreachable!(
                                "enum lookup is stable during explicit type argument checking"
                            );
                        };
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
                        Ok(Type::Named(
                            self.canonical_enum_info_name(name, enum_info),
                            lowered,
                        ))
                    }
                    _ => self.type_of_expr_hint(base, locals, expected),
                }
            }
            ExprKind::Cast { expr: value, ty } => {
                let target_ty = lower_type(
                    ty,
                    self.type_names,
                    self.type_arities,
                    self.canonical_type_names,
                    &self.type_params,
                )?;
                let source_ty = if is_float_type(&target_ty) && Self::is_integer_literal_expr(value)
                {
                    // An explicit integer-to-float cast retains its exactness-
                    // checked cast semantics; it is not contextual literal typing.
                    self.type_of_expr(value, locals)?
                } else {
                    self.type_of_expr_hint(value, locals, Some(&target_ty))?
                };
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
                    } else if let Some(operator) =
                        self.type_of_unary_operator_via_trait(expr.span, *op, &value_ty)?
                    {
                        self.apply_operator_operand_passing(
                            value,
                            operator.receiver_passing,
                            "operator receiver",
                            locals,
                        )?;
                        Ok(operator.return_type)
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
                            if let Some(target_ty) = expected.filter(|ty| is_float_type(ty)) {
                                self.validate_float_context_integer_literal(
                                    *inner, true, target_ty, expr.span,
                                )?;
                                target_ty.clone()
                            } else {
                                let target_ty = expected
                                    .filter(|ty| is_integer_type(ty))
                                    .cloned()
                                    .unwrap_or_else(|| Type::named("int64"));
                                self.validate_negative_integer_literal(
                                    *inner, &target_ty, expr.span,
                                )?;
                                target_ty
                            }
                        }
                        _ => self.type_of_expr_hint(value, locals, expected)?,
                    };
                    if is_integer_type(&value_ty) || is_float_type(&value_ty) {
                        Ok(value_ty)
                    } else if let Some(operator) =
                        self.type_of_unary_operator_via_trait(expr.span, *op, &value_ty)?
                    {
                        self.apply_operator_operand_passing(
                            value,
                            operator.receiver_passing,
                            "operator receiver",
                            locals,
                        )?;
                        Ok(operator.return_type)
                    } else {
                        Err(Diagnostic::coded_at(
                            "AU2003",
                            expr.span,
                            format!("unary `-` expects a numeric value, found `{}`", value_ty),
                        ))
                    }
                }
            },
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
                    && !self.has_from_conversion(&inner_args[1], &return_args[1], expr.span)?
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
            ExprKind::Membership {
                value,
                container,
                negated: _,
                operator_span,
            } => {
                // The container decides the element type the value must have,
                // but the value is written and evaluated first, so the hint is
                // taken from a speculative pass that leaves move state alone.
                let needle_hint = self
                    .type_of_expr_without_move_state(container, locals, None)
                    .ok()
                    .and_then(|ty| membership_needle_type(&ty));
                let value_ty = self.type_of_expr_hint(value, locals, needle_hint.as_ref())?;
                let container_ty = self.type_of_expr(container, locals)?;
                self.check_membership_operands(
                    &value_ty,
                    &container_ty,
                    value.span,
                    *operator_span,
                )?;
                Ok(Type::named("bool"))
            }
            ExprKind::CompareChain { first, links } => {
                // Each operand is typed once, in source order, and a numeric
                // literal still adopts the type its neighbour establishes, the
                // same way a single comparison does.
                let first_hint = links
                    .first()
                    .and_then(|link| self.chain_operand_hint(first, link, locals));
                let mut left_expr: &Expr = first;
                let mut left_ty = self.type_of_expr_hint(first, locals, first_hint.as_ref())?;
                for link in links {
                    match link.op.as_binary_op() {
                        Some(op) => {
                            // The right operand is typed under the left
                            // operand's type, so only the left operand can
                            // still need to adopt its neighbour's. Every
                            // comparison operator produces `bool`, builtin or
                            // through an operator trait whose declaration
                            // already fixes that return type, so the link needs
                            // no further result check.
                            let locals_before_right = locals.clone();
                            let right_ty =
                                self.type_of_expr_hint(&link.operand, locals, Some(&left_ty))?;
                            if left_ty != right_ty && Self::is_numeric_literal_expr(left_expr) {
                                left_ty =
                                    self.type_of_expr_hint(left_expr, locals, Some(&right_ty))?;
                            }
                            if Self::binary_uses_builtin_value_semantics(op, &left_ty, &right_ty) {
                                let retained_left = self
                                    .retained_place_access(
                                        left_expr,
                                        &left_ty,
                                        ReceiverKind::Borrow,
                                        "left operand",
                                    )
                                    .into_iter()
                                    .collect::<Vec<_>>();
                                self.reject_retained_expr_overlap(
                                    &retained_left,
                                    &link.operand,
                                    &right_ty,
                                    None,
                                    &locals_before_right,
                                    locals,
                                    "right operand",
                                )?;
                            }
                            self.type_of_binary(
                                link.op_span,
                                op,
                                left_ty.clone(),
                                right_ty.clone(),
                            )?;
                            left_ty = right_ty;
                        }
                        None => {
                            let container_ty = self.type_of_expr(&link.operand, locals)?;
                            if let Some(needle_ty) = membership_needle_type(&container_ty) {
                                if left_ty != needle_ty && Self::is_numeric_literal_expr(left_expr)
                                {
                                    left_ty = self.type_of_expr_hint(
                                        left_expr,
                                        locals,
                                        Some(&needle_ty),
                                    )?;
                                }
                            }
                            self.check_membership_operands(
                                &left_ty,
                                &container_ty,
                                left_expr.span,
                                link.op_span,
                            )?;
                            left_ty = container_ty;
                        }
                    }
                    left_expr = &link.operand;
                }
                Ok(Type::named("bool"))
            }
            ExprKind::Binary { op, left, right } => {
                let locals_before = locals.clone();
                if matches!(op, BinaryOp::And | BinaryOp::Or) {
                    let left_ty = self.type_of_expr(left, locals)?;
                    let locals_after_left = locals.clone();
                    let mut right_locals = locals_after_left.clone();
                    let right_ty = self.type_of_expr(right, &mut right_locals)?;
                    let borrow_locals = locals_before.clone();
                    let mut left_borrowed_places = Vec::new();
                    self.collect_expr_borrowed_places(
                        left,
                        &borrow_locals,
                        &mut left_borrowed_places,
                    )?;
                    let left_moved_places =
                        self.newly_moved_places(&locals_before, &locals_after_left);
                    let mut right_borrowed_places = Vec::new();
                    self.collect_expr_borrowed_places(
                        right,
                        &borrow_locals,
                        &mut right_borrowed_places,
                    )?;
                    let right_moved_places =
                        self.newly_moved_places(&locals_after_left, &right_locals);
                    self.reject_expr_borrow_move_overlap(
                        &left_borrowed_places,
                        &right_moved_places,
                        expr.span,
                    )?;
                    self.reject_expr_borrow_move_overlap(
                        &right_borrowed_places,
                        &left_moved_places,
                        expr.span,
                    )?;
                    let right_reachable = match op {
                        BinaryOp::And => self.const_bool_value(left) != Some(false),
                        BinaryOp::Or => self.const_bool_value(left) != Some(true),
                        _ => true,
                    };
                    if right_reachable {
                        *locals = right_locals;
                    }
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
                let contextual_left_expected = if matches!(op, BinaryOp::Eq | BinaryOp::NotEq) {
                    self.equality_operand_hint(left, right, locals)
                } else {
                    None
                };
                let mut left_ty = self.type_of_expr_hint(
                    left,
                    locals,
                    contextual_left_expected.as_ref().or(operand_expected),
                )?;
                let locals_after_left = locals.clone();
                let mut right_ty = self.type_of_expr_hint(right, locals, Some(&left_ty))?;
                if left_ty != right_ty
                    && (Self::is_integer_literal_expr(left)
                        || matches!(left.kind, ExprKind::Float(_)))
                {
                    left_ty = self.type_of_expr_hint(left, locals, Some(&right_ty))?;
                }
                if left_ty != right_ty
                    && (Self::is_integer_literal_expr(right)
                        || matches!(right.kind, ExprKind::Float(_)))
                {
                    right_ty = self.type_of_expr_hint(right, locals, Some(&left_ty))?;
                }
                let operator_access =
                    if Self::binary_uses_builtin_value_semantics(*op, &left_ty, &right_ty) {
                        None
                    } else {
                        self.type_of_binary_operator_via_trait(expr.span, *op, &left_ty, &right_ty)?
                    };
                let borrow_locals = locals_before.clone();
                let mut left_borrowed_places = Vec::new();
                self.collect_expr_borrowed_places(left, &borrow_locals, &mut left_borrowed_places)?;
                let mut right_borrowed_places = Vec::new();
                self.collect_expr_borrowed_places(
                    right,
                    &borrow_locals,
                    &mut right_borrowed_places,
                )?;
                let left_moved_places = self.newly_moved_places(&locals_before, &locals_after_left);
                let right_moved_places = self.newly_moved_places(&locals_after_left, locals);
                let retained_left_access = match &operator_access {
                    Some(operator) => self.retained_call_place_access(
                        left,
                        &left_ty,
                        operator.receiver_passing,
                        "left operand",
                    ),
                    None => self.retained_place_access(
                        left,
                        &left_ty,
                        ReceiverKind::Borrow,
                        "left operand",
                    ),
                };
                let retained_left = retained_left_access.into_iter().collect::<Vec<_>>();
                if let Some(operator) = &operator_access {
                    if let Some(access) = self.retained_call_place_access(
                        right,
                        &right_ty,
                        operator.rhs_passing,
                        "right operand",
                    ) {
                        right_borrowed_places.push(access);
                    }
                }
                self.collect_expr_place_reads(
                    right,
                    &borrow_locals,
                    "right operand read",
                    &mut right_borrowed_places,
                );
                self.reject_retained_access_overlap(&retained_left, &right_borrowed_places)?;
                let right_moved_accesses = self.newly_moved_place_accesses(
                    &locals_after_left,
                    locals,
                    "right operand",
                    right.span,
                );
                self.reject_retained_access_overlap(&retained_left, &right_moved_accesses)?;
                self.reject_expr_borrow_move_overlap(
                    &left_borrowed_places,
                    &right_moved_places,
                    expr.span,
                )?;
                self.reject_expr_borrow_move_overlap(
                    &right_borrowed_places,
                    &left_moved_places,
                    expr.span,
                )?;
                if let Some(operator) = operator_access {
                    self.apply_operator_operand_passing(
                        left,
                        operator.receiver_passing,
                        "operator receiver",
                        locals,
                    )?;
                    self.apply_operator_operand_passing(
                        right,
                        operator.rhs_passing,
                        "operator right operand",
                        locals,
                    )?;
                }
                self.type_of_binary(expr.span, *op, left_ty, right_ty)
            }
            ExprKind::Member { object, field } => {
                if let Some(path) = self.member_access_path(expr) {
                    if let Some(binding) = locals.get(&path.root) {
                        if Self::field_path_is_moved(binding, &path.projections) {
                            let mut diagnostic = Diagnostic::at(
                                expr.span,
                                format!(
                                    "use of moved field `{}` from `{}`",
                                    path.projections, path.root
                                ),
                            );
                            if let Some(origin) =
                                Self::moved_field_origin(binding, &path.projections)
                            {
                                diagnostic = diagnostic
                                    .with_secondary(origin, "field moved here")
                                    .with_help(
                                        "borrow the field when ownership is not needed, or call `.clone()` before moving it when an independent value is required",
                                    );
                            }
                            return Err(diagnostic);
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
                            if let Some(payload_tys) =
                                self.builtin_enum_variant_payload(&explicit_ty, enum_name, field)
                            {
                                if !payload_tys.is_empty() {
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
                            return Ok(Type::Named(
                                self.canonical_enum_info_name(enum_name, enum_info),
                                explicit_args,
                            ));
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
                            return Ok(Type::named(
                                self.module_enum_type_name(&module_path, enum_info),
                            ));
                        }
                    }
                }
                if let ExprKind::Name(enum_name) = &object.kind {
                    if expected.is_none() && enum_name == "Option" && field == "None" {
                        return Err(Diagnostic::at(
                            expr.span,
                            "cannot infer type parameter `T` for enum variant `Option.None`",
                        ));
                    }
                    if let Some(expected_ty) = expected {
                        if let Some(payload_tys) =
                            self.builtin_enum_variant_payload(expected_ty, enum_name, field)
                        {
                            if !payload_tys.is_empty() {
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
                        if let Some(Type::Named(expected_name, expected_args)) = expected {
                            if self.canonical_enum_name(expected_name)
                                == self.canonical_enum_name(enum_name)
                            {
                                return Ok(Type::Named(
                                    self.canonical_enum_info_name(enum_name, enum_info),
                                    expected_args.clone(),
                                ));
                            }
                        }
                        if enum_info.decl.type_params.is_empty() {
                            return Ok(Type::named(
                                self.canonical_enum_info_name(enum_name, enum_info),
                            ));
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
                let locals_before_index = locals.clone();
                if let Type::Tuple(element_types) = &object_ty {
                    let tuple_index = match &index.kind {
                        ExprKind::Int(value) => usize::try_from(*value).ok(),
                        ExprKind::Group(inner) => match &inner.kind {
                            ExprKind::Int(value) => usize::try_from(*value).ok(),
                            _ => None,
                        },
                        _ => None,
                    }
                    .ok_or_else(|| {
                        Diagnostic::coded_at(
                            "AU2003",
                            index.span,
                            "tuple indices must be non-negative integer literals",
                        )
                    })?;
                    let element_ty = element_types.get(tuple_index).cloned().ok_or_else(|| {
                        Diagnostic::at(
                            index.span,
                            format!(
                                "tuple index {} is out of bounds for a {}-element tuple",
                                tuple_index,
                                element_types.len()
                            ),
                        )
                    })?;
                    if !self.is_copy_type(&element_ty) {
                        return Err(Diagnostic::coded_at(
                            "AU3005",
                            expr.span,
                            format!(
                                "cannot consume non-copy tuple element `{}` by indexing; unpack the tuple to move its elements",
                                element_ty
                            ),
                        ));
                    }
                    return Ok(element_ty);
                }
                if let Some(element_ty) = vec_element_type(&object_ty).cloned() {
                    self.check_vec_index_type(index, index.span, locals)?;
                    let retained_base = self
                        .retained_place_access(
                            object,
                            &object_ty,
                            ReceiverKind::Borrow,
                            "index base",
                        )
                        .into_iter()
                        .collect::<Vec<_>>();
                    let mut index_borrowed_places = Vec::new();
                    self.collect_expr_borrowed_places(
                        index,
                        &locals_before_index,
                        &mut index_borrowed_places,
                    )?;
                    self.reject_retained_access_overlap(&retained_base, &index_borrowed_places)?;
                    let index_moved_places = self.newly_moved_places(&locals_before_index, locals);
                    self.reject_expr_borrow_move_overlap(
                        &retained_base,
                        &index_moved_places,
                        expr.span,
                    )?;
                    if !self.is_copy_type(&element_ty) {
                        return Err(Diagnostic::coded_at(
                            "AU3005",
                            expr.span,
                            self.indexed_read_guidance("vector", "index", &element_ty),
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
                    let retained_base = self
                        .retained_place_access(
                            object,
                            &object_ty,
                            ReceiverKind::Borrow,
                            "index base",
                        )
                        .into_iter()
                        .collect::<Vec<_>>();
                    let mut index_borrowed_places = Vec::new();
                    self.collect_expr_borrowed_places(
                        index,
                        &locals_before_index,
                        &mut index_borrowed_places,
                    )?;
                    self.reject_retained_access_overlap(&retained_base, &index_borrowed_places)?;
                    let index_moved_places = self.newly_moved_places(&locals_before_index, locals);
                    self.reject_expr_borrow_move_overlap(
                        &retained_base,
                        &index_moved_places,
                        expr.span,
                    )?;
                    if !self.is_copy_type(value_ty) {
                        return Err(Diagnostic::coded_at(
                            "AU3005",
                            expr.span,
                            self.indexed_read_guidance("map", "key", value_ty),
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

    fn binary_uses_builtin_value_semantics(op: BinaryOp, left_ty: &Type, right_ty: &Type) -> bool {
        if is_duration_type(left_ty) || is_duration_type(right_ty) {
            return true;
        }
        if matches!(
            op,
            BinaryOp::Eq
                | BinaryOp::NotEq
                | BinaryOp::Less
                | BinaryOp::LessEq
                | BinaryOp::Greater
                | BinaryOp::GreaterEq
        ) && (matches!(left_ty, Type::Tuple(_)) || matches!(right_ty, Type::Tuple(_)))
        {
            return true;
        }
        if left_ty != right_ty {
            return false;
        }
        match op {
            BinaryOp::And | BinaryOp::Or => *left_ty == Type::named("bool"),
            BinaryOp::Add => {
                is_integer_type(left_ty)
                    || is_float_type(left_ty)
                    || *left_ty == Type::named("String")
            }
            BinaryOp::Sub | BinaryOp::Mul | BinaryOp::Div | BinaryOp::FloorDiv | BinaryOp::Mod => {
                is_integer_type(left_ty) || is_float_type(left_ty)
            }
            BinaryOp::Eq | BinaryOp::NotEq => true,
            BinaryOp::Less | BinaryOp::LessEq | BinaryOp::Greater | BinaryOp::GreaterEq => {
                is_integer_type(left_ty) || is_float_type(left_ty)
            }
        }
    }

    fn is_numeric_literal_expr(expr: &Expr) -> bool {
        Self::is_integer_literal_expr(expr) || matches!(expr.kind, ExprKind::Float(_))
    }

    /// The expected type a chain's left operand can adopt from its first link.
    fn chain_operand_hint(
        &self,
        left: &Expr,
        link: &CompareLink,
        locals: &HashMap<String, LocalBinding>,
    ) -> Option<Type> {
        match link.op.as_binary_op() {
            Some(BinaryOp::Eq | BinaryOp::NotEq) => {
                self.equality_operand_hint(left, &link.operand, locals)
            }
            Some(_) => None,
            None => self
                .type_of_expr_without_move_state(&link.operand, locals, None)
                .ok()
                .and_then(|operand_ty| membership_needle_type(&operand_ty)),
        }
    }

    fn check_membership_operands(
        &self,
        value_ty: &Type,
        container_ty: &Type,
        value_span: crate::diag::Span,
        operator_span: crate::diag::Span,
    ) -> Result<()> {
        let Some(needle_ty) = membership_needle_type(container_ty) else {
            return Err(Diagnostic::coded_at(
                "AU2003",
                operator_span,
                format!(
                    "`in` requires a `Vec[T]`, `Set[T]`, `Map[K, V]`, or `String` container, found `{}`",
                    container_ty
                ),
            )
            .with_help(
                "membership tests read `Vec` and `Set` elements, `Map` keys, and `String` substrings",
            ));
        };
        if *value_ty != needle_ty {
            let subject = match container_ty {
                Type::Named(name, _) if name == "Map" => "key",
                Type::Named(name, _) if name == "String" => "substring",
                _ => "element",
            };
            return Err(Diagnostic::coded_at(
                "AU2002",
                value_span,
                format!(
                    "`in` expects a `{}` {}, found `{}`",
                    needle_ty, subject, value_ty
                ),
            ));
        }
        Ok(())
    }

    fn type_of_binary(
        &self,
        span: crate::diag::Span,
        op: BinaryOp,
        left_ty: Type,
        right_ty: Type,
    ) -> Result<Type> {
        if let Some(result) = builtin_duration_binary_result(op, &left_ty, &right_ty) {
            return Ok(result);
        }
        if matches!(
            op,
            BinaryOp::Less | BinaryOp::LessEq | BinaryOp::Greater | BinaryOp::GreaterEq
        ) && (matches!(left_ty, Type::Tuple(_)) || matches!(right_ty, Type::Tuple(_)))
        {
            return Err(Diagnostic::coded_at(
                "AU2003",
                span,
                "tuple ordering is not supported; use `==` or `!=`, or compare tuple elements explicitly",
            ));
        }
        if matches!(op, BinaryOp::Eq | BinaryOp::NotEq)
            && left_ty != right_ty
            && (matches!(left_ty, Type::Tuple(_)) || matches!(right_ty, Type::Tuple(_)))
        {
            return Err(Diagnostic::coded_at(
                "AU2002",
                span,
                format!(
                    "tuple equality operands must have the same type, found `{}` and `{}`",
                    left_ty, right_ty
                ),
            ));
        }
        if is_duration_type(&left_ty) || is_duration_type(&right_ty) {
            return Err(Diagnostic::coded_at(
                "AU2003",
                span,
                format!(
                    "unsupported Duration operands: `{}` and `{}`; supported forms are `Duration + Duration`, `Duration - Duration`, `Duration * int64`, `int64 * Duration`, `Duration // int64`, and comparisons between two Duration values",
                    left_ty, right_ty
                ),
            ));
        }
        if op == BinaryOp::Div && left_ty == right_ty && is_integer_type(&left_ty) {
            return Err(Diagnostic::at(
                span,
                "integer `/` is not supported; use `//` for floor division, or call `.to_float()` on both operands for true division",
            ));
        }
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
            (
                BinaryOp::Sub | BinaryOp::Mul | BinaryOp::Div | BinaryOp::FloorDiv | BinaryOp::Mod,
                _,
                _,
            ) if left_ty == right_ty && (is_integer_type(&left_ty) || is_float_type(&left_ty)) => {
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
                if let Some(operator) =
                    self.type_of_binary_operator_via_trait(span, op, &left_ty, &right_ty)?
                {
                    Ok(operator.return_type)
                } else if left_ty != right_ty {
                    let non_optional_none_type = if matches!(op, BinaryOp::Eq | BinaryOp::NotEq) {
                        match (&left_ty, &right_ty) {
                            (Type::Unit, other) if !is_option_type(other) => Some(other),
                            (other, Type::Unit) if !is_option_type(other) => Some(other),
                            _ => None,
                        }
                    } else {
                        None
                    };
                    if let Some(non_optional_ty) = non_optional_none_type {
                        return Err(Diagnostic::at(
                            span,
                            format!(
                                "type `{}` is not optional; only `Option[T]` values can be compared with `None`",
                                non_optional_ty
                            ),
                        ));
                    }
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
    ) -> Result<Option<ResolvedUnaryOperatorAccess>> {
        let Some((trait_name, method_name)) = unary_operator_trait(op) else {
            return Ok(None);
        };
        if let Type::TypeParam(type_param_name) = value_ty {
            let Some(method) = self.operator_method_from_type_param(
                type_param_name,
                trait_name,
                method_name,
                None,
            )?
            else {
                return Ok(None);
            };
            let operation = format!("operator trait `{}.{}`", trait_name, method_name);
            self.enforce_resolved_rng_clone_obligations_before_method_inference(
                &operation,
                &method.rng_clone_safe_types,
                &method.decl.type_params,
                span,
            )?;
            let substitutions = self.infer_method_type_substitutions(
                &operation,
                &method.decl.type_params,
                &method.signature.params,
                &method.type_param_bounds,
                &method.signature.rng_clone_safe_type_params,
                &[],
                HashMap::new(),
                span,
            )?;
            let receiver_passing = method.decl.receiver.unwrap_or(ReceiverKind::Value);
            let return_type = substitute_type(&method.signature.return_type, &substitutions);
            return Ok(Some(ResolvedUnaryOperatorAccess {
                return_type,
                receiver_passing,
            }));
        }
        let Some((method, substitutions)) =
            self.operator_method_for_concrete_type(span, value_ty, trait_name, method_name, None)?
        else {
            return Ok(None);
        };
        let operation = format!("operator trait `{}.{}`", trait_name, method_name);
        self.enforce_resolved_rng_clone_obligations_before_method_inference(
            &operation,
            &method.rng_clone_safe_types,
            &method.decl.type_params,
            span,
        )?;
        let substitutions = self.infer_method_type_substitutions(
            &operation,
            &method.decl.type_params,
            &method.signature.params,
            &method.type_param_bounds,
            &method.signature.rng_clone_safe_type_params,
            &[],
            substitutions,
            span,
        )?;
        let receiver_passing = method.decl.receiver.unwrap_or(ReceiverKind::Value);
        let return_type = substitute_type(&method.signature.return_type, &substitutions);
        Ok(Some(ResolvedUnaryOperatorAccess {
            return_type,
            receiver_passing,
        }))
    }

    fn type_of_binary_operator_via_trait(
        &self,
        span: crate::diag::Span,
        op: BinaryOp,
        left_ty: &Type,
        right_ty: &Type,
    ) -> Result<Option<ResolvedBinaryOperatorAccess>> {
        let Some((trait_name, method_name)) = binary_operator_trait(op) else {
            return Ok(None);
        };
        if let Type::TypeParam(type_param_name) = left_ty {
            let Some(method) = self.operator_method_from_type_param(
                type_param_name,
                trait_name,
                method_name,
                Some(right_ty),
            )?
            else {
                return Ok(None);
            };
            let operation = format!("operator trait `{}.{}`", trait_name, method_name);
            self.enforce_resolved_rng_clone_obligations_before_method_inference(
                &operation,
                &method.rng_clone_safe_types,
                &method.decl.type_params,
                span,
            )?;
            let substitutions = self.infer_method_type_substitutions(
                &operation,
                &method.decl.type_params,
                &method.signature.params,
                &method.type_param_bounds,
                &method.signature.rng_clone_safe_type_params,
                std::slice::from_ref(right_ty),
                HashMap::new(),
                span,
            )?;
            let receiver_passing = method.decl.receiver.unwrap_or(ReceiverKind::Value);
            let rhs_passing = method
                .signature
                .param_passings
                .first()
                .copied()
                .unwrap_or(ReceiverKind::Value);
            let return_type = substitute_type(&method.signature.return_type, &substitutions);
            return Ok(Some(ResolvedBinaryOperatorAccess {
                return_type,
                receiver_passing,
                rhs_passing,
            }));
        }
        let Some((method, substitutions)) = self.operator_method_for_concrete_type(
            span,
            left_ty,
            trait_name,
            method_name,
            Some(right_ty),
        )?
        else {
            return Ok(None);
        };
        let operation = format!("operator trait `{}.{}`", trait_name, method_name);
        self.enforce_resolved_rng_clone_obligations_before_method_inference(
            &operation,
            &method.rng_clone_safe_types,
            &method.decl.type_params,
            span,
        )?;
        let substitutions = self.infer_method_type_substitutions(
            &operation,
            &method.decl.type_params,
            &method.signature.params,
            &method.type_param_bounds,
            &method.signature.rng_clone_safe_type_params,
            std::slice::from_ref(right_ty),
            substitutions,
            span,
        )?;
        let receiver_passing = method.decl.receiver.unwrap_or(ReceiverKind::Value);
        let rhs_passing = method
            .signature
            .param_passings
            .first()
            .copied()
            .unwrap_or(ReceiverKind::Value);
        let return_type = substitute_type(&method.signature.return_type, &substitutions);
        if matches!(
            op,
            BinaryOp::Less | BinaryOp::LessEq | BinaryOp::Greater | BinaryOp::GreaterEq
        ) && return_type != Type::named("bool")
        {
            return Err(Diagnostic::at(
                span,
                format!(
                    "operator trait `{}` for `{}` must return `bool`",
                    trait_name, method_name
                ),
            ));
        }
        Ok(Some(ResolvedBinaryOperatorAccess {
            return_type,
            receiver_passing,
            rhs_passing,
        }))
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
        let self_ty = Type::TypeParam(type_param_name.to_string());
        for bound in self
            .type_param_bounds
            .get(type_param_name)
            .into_iter()
            .flatten()
        {
            for bound in self.trait_bound_closure(bound, &self_ty) {
                if bound.trait_name != trait_name {
                    continue;
                }
                match rhs {
                    Some(rhs_ty)
                        if !bound.trait_args.is_empty() && &bound.trait_args[0] == rhs_ty => {}
                    None if bound.trait_args.len() == 1 => {}
                    _ => continue,
                }
                let trait_substitutions =
                    self_type_substitutions(&trait_info.decl, &bound.trait_args, self_ty.clone());
                matches.push(ResolvedTraitMethodInfo {
                    decl: method.decl.clone(),
                    signature: FunctionSignature {
                        params: method
                            .signature
                            .params
                            .iter()
                            .map(|param| substitute_type(param, &trait_substitutions))
                            .collect(),
                        param_passings: method.signature.param_passings.clone(),
                        return_type: substitute_type(
                            &method.signature.return_type,
                            &trait_substitutions,
                        ),
                        rng_clone_safe_type_params: method
                            .signature
                            .rng_clone_safe_type_params
                            .iter()
                            .filter(|name| method.decl.type_params.contains(name))
                            .cloned()
                            .collect(),
                    },
                    type_param_bounds: substitute_trait_bounds(
                        &method.type_param_bounds,
                        &trait_substitutions,
                    ),
                    rng_clone_safe_types: method
                        .signature
                        .rng_clone_safe_type_params
                        .iter()
                        .filter(|name| !method.decl.type_params.contains(name))
                        .map(|name| {
                            substitute_type(&Type::TypeParam(name.clone()), &trait_substitutions)
                        })
                        .collect(),
                });
            }
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
                trait_impl_specificity(trait_impl),
                ResolvedTraitMethodInfo {
                    decl: method.decl.clone(),
                    signature: FunctionSignature {
                        params: method
                            .signature
                            .params
                            .iter()
                            .map(|param| substitute_type(param, &substitutions))
                            .collect(),
                        param_passings: method.signature.param_passings.clone(),
                        return_type: substitute_type(&method.signature.return_type, &substitutions),
                        rng_clone_safe_type_params: method
                            .signature
                            .rng_clone_safe_type_params
                            .iter()
                            .filter(|name| method.decl.type_params.contains(name))
                            .cloned()
                            .collect(),
                    },
                    type_param_bounds: substitute_trait_bounds(
                        &method.type_param_bounds,
                        &substitutions,
                    ),
                    rng_clone_safe_types: method
                        .signature
                        .rng_clone_safe_type_params
                        .iter()
                        .filter(|name| !method.decl.type_params.contains(name))
                        .map(|name| substitute_type(&Type::TypeParam(name.clone()), &substitutions))
                        .collect(),
                },
                substitutions,
            ));
        }
        if matches.is_empty() {
            return Ok(None);
        }
        matches.sort_by_key(|candidate| std::cmp::Reverse(candidate.0));
        let best_score = matches[0].0;
        let mut best_matches = matches
            .into_iter()
            .filter(|(score, _, _)| *score == best_score)
            .collect::<Vec<_>>();
        match best_matches.len() {
            1 => {
                let (_, method, substitutions) = best_matches
                    .pop()
                    .expect("best operator trait impl should exist");
                Ok(Some((method, substitutions)))
            }
            _ => Err(Diagnostic::at(
                span,
                format!(
                    "operator trait `{}` is ambiguous for type `{}`",
                    trait_name, receiver_ty
                ),
            )),
        }
    }

    fn type_check_builtin_class_constructor(
        &self,
        constructor: BuiltinClassConstructor,
        args: &[Argument],
        span: crate::diag::Span,
        locals: &mut HashMap<String, LocalBinding>,
        has_explicit_type_args: bool,
        result_type: Type,
    ) -> Result<Type> {
        if has_explicit_type_args {
            return Err(Diagnostic::at(
                span,
                format!(
                    "`{}` does not take explicit type arguments",
                    constructor.qualified_name()
                ),
            ));
        }
        let ordered_args = constructor.bind_args(args, span)?;
        let seed_arg = required_ordered_arg(
            &ordered_args,
            0,
            span,
            "internal error: random.Rng should bind one seed argument",
        )?;
        let actual =
            self.type_of_expr_hint(&seed_arg.value, locals, Some(&Type::named("int64")))?;
        if actual != Type::named("int64") {
            return Err(Diagnostic::coded_at(
                "AU2002",
                seed_arg.span,
                format!(
                    "`{}` expects `int64` for `seed`, found `{}`",
                    constructor.qualified_name(),
                    actual
                ),
            ));
        }
        Ok(result_type)
    }

    #[allow(clippy::too_many_arguments)]
    fn type_check_user_class_constructor(
        &self,
        class: &ClassInfo,
        surface_name: &str,
        constructor_name: &str,
        class_type_name: String,
        explicit_type_args: Option<&[TypeRef]>,
        args: &[Argument],
        span: crate::diag::Span,
        locals: &mut HashMap<String, LocalBinding>,
        expected: Option<&Type>,
    ) -> Result<Type> {
        let mut substitutions = if let Some(type_args) = explicit_type_args {
            self.explicit_type_substitutions(
                &class.decl.type_params,
                type_args,
                span,
                &format!("class constructor `{}`", constructor_name),
            )?
        } else {
            match expected {
                Some(Type::Named(expected_name, expected_args))
                    if expected_name == &class_type_name
                        && expected_args.len() == class.decl.type_params.len() =>
                {
                    substitutions_from_decl_type_args(&class.decl.type_params, expected_args)
                }
                _ => HashMap::new(),
            }
        };
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
                let Some(field_decl) = class.decl.fields.get(next_positional_field) else {
                    return Err(Diagnostic::at(
                        argument.span,
                        format!(
                            "class constructor `{}` received too many positional arguments",
                            surface_name
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
                        surface_name, field_name
                    ),
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
            let actual =
                match self.type_of_expr_hint(&argument.value, locals, Some(&hinted_field_ty)) {
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
            if let Err(error) = unify_type_pattern(&field_info.ty, &actual, &mut substitutions) {
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
                        surface_name, field.name
                    ),
                ));
            }
        }

        let mut resolved_args = Vec::with_capacity(class.decl.type_params.len());
        for type_param in &class.decl.type_params {
            let Some(resolved) = substitutions.get(type_param).cloned() else {
                return Err(Diagnostic::at(
                    span,
                    format!(
                        "cannot infer type parameter `{}` for class constructor `{}`",
                        type_param, constructor_name
                    ),
                ));
            };
            resolved_args.push(resolved);
        }
        let resolved_substitutions =
            substitutions_from_decl_type_args(&class.decl.type_params, &resolved_args);
        for (index, type_param) in class.decl.type_params.iter().enumerate() {
            let Some(bounds) = class.type_param_bounds.get(type_param) else {
                continue;
            };
            let mut resolved_bounds = Vec::with_capacity(bounds.len());
            for bound in bounds {
                resolved_bounds.push(substitute_trait_bound(bound, &resolved_substitutions));
            }
            self.assert_type_satisfies_bounds(&resolved_args[index], &resolved_bounds, span)?;
        }

        Ok(Type::Named(class_type_name, resolved_args))
    }

    #[allow(clippy::too_many_arguments)]
    fn type_check_user_enum_variant_constructor(
        &self,
        enum_info: &EnumInfo,
        enum_name: &str,
        enum_type_name: String,
        variant_name: &str,
        explicit_type_args: Option<&[TypeRef]>,
        args: &[Argument],
        span: crate::diag::Span,
        locals: &mut HashMap<String, LocalBinding>,
        expected: Option<&Type>,
    ) -> Result<Type> {
        let Some(variant) = enum_info.variants.get(variant_name) else {
            return Err(Diagnostic::at(
                span,
                format!("enum `{}` has no variant `{}`", enum_name, variant_name),
            ));
        };
        let mut substitutions = if let Some(type_args) = explicit_type_args {
            self.explicit_type_substitutions(
                &enum_info.decl.type_params,
                type_args,
                span,
                &format!("enum `{}`", enum_name),
            )?
        } else {
            match expected {
                Some(Type::Named(expected_name, expected_args))
                    if expected_name == &enum_type_name
                        && expected_args.len() == enum_info.decl.type_params.len() =>
                {
                    substitutions_from_decl_type_args(&enum_info.decl.type_params, expected_args)
                }
                _ => HashMap::new(),
            }
        };
        let ordered_args = self.variant_payload_arguments(
            args,
            span,
            variant_name,
            enum_name,
            &variant.payloads,
            variant.named_payloads,
        )?;
        for (argument, payload) in ordered_args.iter().zip(variant.payloads.iter()) {
            let hinted_payload_ty = substitute_type(&payload.ty, &substitutions);
            let actual = if has_unresolved_type_params(&hinted_payload_ty) {
                self.type_of_expr(&argument.value, locals)?
            } else {
                self.type_of_expr_hint(&argument.value, locals, Some(&hinted_payload_ty))?
            };
            if let Err(error) = unify_type_pattern(&payload.ty, &actual, &mut substitutions) {
                return Err(Diagnostic::at(
                    argument.span,
                    format!(
                        "variant `{}` of enum `{}` expects `{}`, found `{}` ({})",
                        variant_name, enum_name, hinted_payload_ty, actual, error.message
                    ),
                ));
            }
            if !self.is_copy_type(&actual) {
                self.consume_value_expr(&argument.value, locals)?;
            }
        }

        let mut resolved_args = Vec::with_capacity(enum_info.decl.type_params.len());
        for type_param in &enum_info.decl.type_params {
            let Some(resolved) = substitutions.get(type_param).cloned() else {
                return Err(Diagnostic::at(
                    span,
                    format!(
                        "cannot infer type parameter `{}` for enum variant `{}.{}`",
                        type_param, enum_name, variant_name
                    ),
                ));
            };
            resolved_args.push(resolved);
        }
        let resolved_substitutions =
            substitutions_from_decl_type_args(&enum_info.decl.type_params, &resolved_args);
        for (index, type_param) in enum_info.decl.type_params.iter().enumerate() {
            let Some(bounds) = enum_info.type_param_bounds.get(type_param) else {
                continue;
            };
            let mut resolved_bounds = Vec::with_capacity(bounds.len());
            for bound in bounds {
                resolved_bounds.push(substitute_trait_bound(bound, &resolved_substitutions));
            }
            self.assert_type_satisfies_bounds(&resolved_args[index], &resolved_bounds, span)?;
        }

        Ok(Type::Named(enum_type_name, resolved_args))
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

        if matches!(&base_callee.kind, ExprKind::Name(name) if name == "TaskGroup")
            && explicit_type_args.is_none()
        {
            if !args.is_empty() {
                return Err(Diagnostic::at(
                    span,
                    "`TaskGroup` does not take constructor arguments",
                ));
            }
            return Ok(Type::named("TaskGroup"));
        }

        if let (ExprKind::Name(name), Some(type_args)) = (&base_callee.kind, explicit_type_args) {
            if name == "Queue" {
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
                    &format!("class `{}`", name),
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
                return Ok(Type::Named("Queue".to_string(), explicit_args));
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
            if name == "TaskGroup" {
                if !type_args.is_empty() {
                    return Err(Diagnostic::at(
                        span,
                        "`TaskGroup` does not take type arguments",
                    ));
                }
                if !args.is_empty() {
                    return Err(Diagnostic::at(
                        span,
                        "`TaskGroup` does not take constructor arguments",
                    ));
                }
                return Ok(Type::named("TaskGroup"));
            }
        }

        if let ExprKind::Member { object, field } = &base_callee.kind {
            if let ExprKind::Name(type_name) = &object.kind {
                if !locals.contains_key(type_name) {
                    if let Some(constructor) = BuiltinAssociatedFunction::resolve(type_name, field)
                    {
                        if explicit_type_args.is_some() {
                            return Err(Diagnostic::at(
                                span,
                                format!(
                                    "`{}.{}` does not take explicit type arguments",
                                    constructor.owner_name(),
                                    constructor.name()
                                ),
                            ));
                        }
                        let ordered_args = constructor.bind_args(args, span)?;
                        match constructor {
                            BuiltinAssociatedFunction::DurationMilliseconds
                            | BuiltinAssociatedFunction::DurationSeconds
                            | BuiltinAssociatedFunction::DurationMinutes => {
                                let value_arg = required_ordered_arg(
                                &ordered_args,
                                0,
                                span,
                                "internal error: Duration constructor should bind one value argument",
                            )?;
                                let actual = self.type_of_expr_hint(
                                    &value_arg.value,
                                    locals,
                                    Some(&Type::named("int64")),
                                )?;
                                if actual != Type::named("int64") {
                                    return Err(Diagnostic::at(
                                        value_arg.span,
                                        format!(
                                            "`Duration.{}` expects `int64`, found `{}`",
                                            constructor.name(),
                                            actual
                                        ),
                                    ));
                                }
                                return Ok(Type::named("Duration"));
                            }
                            BuiltinAssociatedFunction::StringFromBytes => {
                                let bytes_arg = required_ordered_arg(
                                &ordered_args,
                                0,
                                span,
                                "internal error: String.from_bytes should bind one bytes argument",
                            )?;
                                let expected =
                                    Type::Named("Vec".to_string(), vec![Type::named("uint8")]);
                                let actual = self.type_of_expr_hint(
                                    &bytes_arg.value,
                                    locals,
                                    Some(&expected),
                                )?;
                                if actual != expected {
                                    return Err(Diagnostic::at(
                                        bytes_arg.span,
                                        format!(
                                        "`String.from_bytes` expects `Vec[uint8]`, found `{actual}`"
                                    ),
                                    ));
                                }
                                return Ok(Type::Named(
                                    "Result".to_string(),
                                    vec![Type::named("String"), Type::named("bytes.Error")],
                                ));
                            }
                        }
                    }
                }
            }
        }

        match &base_callee.kind {
            ExprKind::Name(name) if BuiltinFunction::from_name(name).is_some() => {
                let Some(builtin) = BuiltinFunction::from_name(name) else {
                    unreachable!("builtin lookup is stable during call checking");
                };
                let ordered_args = builtin.bind_args(args, span)?;
                match builtin {
                    BuiltinFunction::Print => {
                        let value_arg = required_ordered_arg(
                            &ordered_args,
                            0,
                            span,
                            "internal error: `print` should bind exactly one argument",
                        )?;
                        self.type_of_expr(&value_arg.value, locals)?;
                        Ok(Type::Unit)
                    }
                    BuiltinFunction::Range => {
                        for argument in ordered_args.into_iter().flatten() {
                            let actual = self.type_of_expr_hint(
                                &argument.value,
                                locals,
                                Some(&Type::named("int32")),
                            )?;
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
                    BuiltinFunction::Cancelled => Ok(Type::named("bool")),
                    BuiltinFunction::Sleep => {
                        let duration_arg = required_ordered_arg(
                            &ordered_args,
                            0,
                            span,
                            "internal error: `sleep` should bind exactly one argument",
                        )?;
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
                    BuiltinFunction::WaitAny | BuiltinFunction::WaitAll => {
                        let tasks_arg = required_ordered_arg(
                            &ordered_args,
                            0,
                            span,
                            format!(
                                "internal error: `{}` should bind the `tasks` argument",
                                builtin.name()
                            ),
                        )?;
                        let tasks_ty = self.type_of_expr(&tasks_arg.value, locals)?;
                        let Type::Named(ref container_name, ref container_args) = tasks_ty else {
                            return Err(Diagnostic::at(
                                tasks_arg.span,
                                format!(
                                    "`{}` expects `Vec[Task[T]]`, found `{}`",
                                    builtin.name(),
                                    tasks_ty
                                ),
                            ));
                        };
                        if container_name != "Vec" || container_args.len() != 1 {
                            return Err(Diagnostic::at(
                                tasks_arg.span,
                                format!(
                                    "`{}` expects `Vec[Task[T]]`, found `{}`",
                                    builtin.name(),
                                    tasks_ty
                                ),
                            ));
                        }
                        let Type::Named(task_name, task_args) = &container_args[0] else {
                            return Err(Diagnostic::at(
                                tasks_arg.span,
                                format!(
                                    "`{}` expects `Vec[Task[T]]`, found `{}`",
                                    builtin.name(),
                                    tasks_ty
                                ),
                            ));
                        };
                        if task_name != "Task" || task_args.len() != 1 {
                            return Err(Diagnostic::at(
                                tasks_arg.span,
                                format!(
                                    "`{}` expects `Vec[Task[T]]`, found `{}`",
                                    builtin.name(),
                                    tasks_ty
                                ),
                            ));
                        }
                        self.reject_rng_duplication(builtin.name(), &task_args[0], span)?;
                        if let Some(timeout_arg) = ordered_args[1] {
                            let actual = self.type_of_expr_hint(
                                &timeout_arg.value,
                                locals,
                                Some(&Type::named("Duration")),
                            )?;
                            if actual != Type::named("Duration") {
                                return Err(Diagnostic::at(
                                    timeout_arg.span,
                                    format!(
                                        "`{}(timeout=...)` expects `Duration`, found `{}`",
                                        builtin.name(),
                                        actual
                                    ),
                                ));
                            }
                        }
                        Ok(Type::Named(
                            match builtin {
                                BuiltinFunction::WaitAny => "WaitAny".to_string(),
                                BuiltinFunction::WaitAll => "WaitAll".to_string(),
                                _ => unreachable!(),
                            },
                            vec![task_args[0].clone()],
                        ))
                    }
                    BuiltinFunction::Len => {
                        let value_arg = required_ordered_arg(
                            &ordered_args,
                            0,
                            span,
                            "internal error: `len` should bind exactly one argument",
                        )?;
                        let value_ty = self.type_of_expr(&value_arg.value, locals)?;
                        let Type::Named(receiver_name, _) = &value_ty else {
                            return Err(Diagnostic::coded_at(
                                "AU2002",
                                value_arg.span,
                                format!("`len(...)` expects a value with a `len()` member, found `{value_ty}`"),
                            ));
                        };
                        if BuiltinMember::resolve(receiver_name, "len").is_none() {
                            return Err(Diagnostic::coded_at(
                                "AU2002",
                                value_arg.span,
                                format!("`len(...)` expects a value with a `len()` member, found `{value_ty}`"),
                            )
                            .with_help(
                                "`len` delegates to the value's own `len()`; `String`, `Vec[T]`, `Map[K, V]`, and `Set[T]` provide it",
                            ));
                        }
                        Ok(Type::named("int64"))
                    }
                    BuiltinFunction::Str => {
                        let value_arg = required_ordered_arg(
                            &ordered_args,
                            0,
                            span,
                            "internal error: `str` should bind exactly one argument",
                        )?;
                        self.type_of_expr(&value_arg.value, locals)?;
                        Ok(Type::named("String"))
                    }
                    BuiltinFunction::Abs => {
                        let value_arg = required_ordered_arg(
                            &ordered_args,
                            0,
                            span,
                            "internal error: `abs` should bind exactly one argument",
                        )?;
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
                        let left_arg = required_ordered_arg(
                            &ordered_args,
                            0,
                            span,
                            format!(
                                "internal error: `{}` should bind a left argument",
                                builtin.name()
                            ),
                        )?;
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
                        let right_arg = required_ordered_arg(
                            &ordered_args,
                            1,
                            span,
                            format!(
                                "internal error: `{}` should bind a right argument",
                                builtin.name()
                            ),
                        )?;
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
                        Ok(left_ty)
                    }
                    BuiltinFunction::Sqrt => {
                        let value_arg = required_ordered_arg(
                            &ordered_args,
                            0,
                            span,
                            "internal error: `sqrt` should bind exactly one argument",
                        )?;
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
                        let text_arg = required_ordered_arg(
                            &ordered_args,
                            0,
                            span,
                            "internal error: `parse_int32` should bind exactly one argument",
                        )?;
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
                        let text_arg = required_ordered_arg(
                            &ordered_args,
                            0,
                            span,
                            "internal error: `parse_int64` should bind exactly one argument",
                        )?;
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
                        let text_arg = required_ordered_arg(
                            &ordered_args,
                            0,
                            span,
                            "internal error: `parse_float64` should bind exactly one argument",
                        )?;
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
                let Some(function) = self.resolve_function_info(name) else {
                    unreachable!("function lookup is stable during call checking");
                };
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
                    &function.signature.param_passings,
                    &function.signature.params,
                    &function.signature.return_type,
                    &function.type_param_bounds,
                    &function.signature.rng_clone_safe_type_params,
                    args,
                    span,
                    locals,
                    expected,
                    seed_substitutions,
                )
            }
            ExprKind::Name(name) if self.resolve_class_info(name).is_some() => {
                let class = self
                    .resolve_class_info(name)
                    .expect("class constructor guard should retain the resolved class");
                let class_type_name = self.canonical_class_name(name, class);
                if let Some(constructor) = class.builtin_constructor() {
                    return self.type_check_builtin_class_constructor(
                        constructor,
                        args,
                        span,
                        locals,
                        explicit_type_args.is_some(),
                        Type::named(constructor.qualified_name()),
                    );
                }
                self.type_check_user_class_constructor(
                    class,
                    name,
                    name,
                    class_type_name,
                    explicit_type_args,
                    args,
                    span,
                    locals,
                    expected,
                )
            }
            ExprKind::Name(name) if self.is_builtin_enum_variant_name(name) => {
                let Some(expected_ty) = expected else {
                    return Err(Diagnostic::at(
                        span,
                        "bare enum variants require an expected enum type or a qualified form such as `Result.Ok(...)`",
                    ));
                };
                let Type::Named(enum_name, _) = expected_ty else {
                    return Err(Diagnostic::at(
                        span,
                        "bare enum variants require an expected enum type or a qualified form such as `Result.Ok(...)`",
                    ));
                };
                if self
                    .builtin_enum_variant_payload(expected_ty, enum_name, name)
                    .is_none()
                {
                    return Err(Diagnostic::at(
                        span,
                        "bare enum variants require an expected enum type or a qualified form such as `Result.Ok(...)`",
                    ));
                }
                self.type_check_builtin_enum_variant_constructor(
                    enum_name,
                    name,
                    expected_ty,
                    args,
                    span,
                    locals,
                )
            }
            ExprKind::Member { object, field } => {
                let (base_object, object_type_args) = self.peel_specialization(object);
                if let Some((module_path, item_name)) = self.qualified_module_item(object) {
                    if let Some(namespace) = self.module_namespace(&module_path) {
                        if let Some(class_info) = namespace.classes.get(&item_name) {
                            if let Some(method) = class_info.methods.get(field) {
                                if method.decl.receiver.is_none() {
                                    let seed_substitutions =
                                        if let Some(type_args) = object_type_args {
                                            self.explicit_type_substitutions(
                                                &class_info.decl.type_params,
                                                type_args,
                                                object.span,
                                                &format!("class `{}`", item_name),
                                            )?
                                        } else {
                                            HashMap::new()
                                        };
                                    return self.type_check_callable_args(
                                        &format!("method `{}`", field),
                                        &method.decl.type_params,
                                        &method.decl.params,
                                        &method.signature.param_passings,
                                        &method.signature.params,
                                        &method.signature.return_type,
                                        &method.type_param_bounds,
                                        &method.signature.rng_clone_safe_type_params,
                                        args,
                                        span,
                                        locals,
                                        expected,
                                        seed_substitutions,
                                    );
                                }
                            }
                        }
                        if let Some(enum_info) = namespace.enums.get(&item_name) {
                            let enum_type_name =
                                self.module_enum_type_name(&module_path, enum_info);
                            return self.type_check_user_enum_variant_constructor(
                                enum_info,
                                &item_name,
                                enum_type_name,
                                field,
                                object_type_args,
                                args,
                                span,
                                locals,
                                expected,
                            );
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
                            let seed_substitutions = if let Some(type_args) = object_type_args {
                                self.explicit_type_substitutions(
                                    &class_info.decl.type_params,
                                    type_args,
                                    object.span,
                                    &format!("class `{}`", class_name),
                                )?
                            } else {
                                HashMap::new()
                            };
                            return self.type_check_callable_args(
                                &format!("method `{}`", field),
                                &method.decl.type_params,
                                &method.decl.params,
                                &method.signature.param_passings,
                                &method.signature.params,
                                &method.signature.return_type,
                                &method.type_param_bounds,
                                &method.signature.rng_clone_safe_type_params,
                                args,
                                span,
                                locals,
                                expected,
                                seed_substitutions,
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
                    if object_type_args.is_none() && expected.is_none() && enum_name == "Option" {
                        match field.as_str() {
                            "Some" => {
                                if args.len() != 1 {
                                    return Err(Diagnostic::at(
                                        span,
                                        format!(
                                            "variant `{}` of enum `{}` expects 1 payload argument, found {}",
                                            field,
                                            enum_name,
                                            args.len()
                                        ),
                                    ));
                                }
                                let actual = self.type_of_expr(&args[0].value, locals)?;
                                if !self.is_copy_type(&actual) {
                                    self.consume_value_expr(&args[0].value, locals)?;
                                }
                                return Ok(Type::Named("Option".to_string(), vec![actual]));
                            }
                            "None" => {
                                return Err(Diagnostic::at(
                                    span,
                                    "cannot infer type parameter `T` for enum variant `Option.None`",
                                ));
                            }
                            _ => {}
                        }
                    }
                    if let Some(expected_ty) = expected {
                        if let Some(variant_payloads) =
                            self.builtin_enum_variant_payload(expected_ty, enum_name, field)
                        {
                            if variant_payloads.is_empty() {
                                return Err(Diagnostic::at(
                                    span,
                                    format!(
                                        "variant `{}` of enum `{}` does not take a payload",
                                        field, enum_name
                                    ),
                                ));
                            }
                            if args.len() != variant_payloads.len() {
                                return Err(Diagnostic::at(
                                    span,
                                    format!(
                                        "variant `{}` of enum `{}` expects {} payload argument{}, found {}",
                                        field,
                                        enum_name,
                                        variant_payloads.len(),
                                        if variant_payloads.len() == 1 { "" } else { "s" },
                                        args.len()
                                    ),
                                ));
                            }
                            for (payload_ty, argument) in variant_payloads.iter().zip(args) {
                                let actual = self.type_of_expr_hint(
                                    &argument.value,
                                    locals,
                                    Some(payload_ty),
                                )?;
                                if actual != *payload_ty {
                                    return Err(Diagnostic::at(
                                        argument.span,
                                        format!(
                                            "variant `{}` of enum `{}` expects `{}`, found `{}`",
                                            field, enum_name, payload_ty, actual
                                        ),
                                    ));
                                }
                                if !self.is_copy_type(payload_ty) {
                                    self.consume_value_expr(&argument.value, locals)?;
                                }
                            }
                            return Ok(expected_ty.clone());
                        }
                    }
                    if let Some(enum_info) = self.resolve_enum_info(enum_name) {
                        let enum_type_name = self.canonical_enum_info_name(enum_name, enum_info);
                        return self.type_check_user_enum_variant_constructor(
                            enum_info,
                            enum_name,
                            enum_type_name,
                            field,
                            object_type_args,
                            args,
                            span,
                            locals,
                            expected,
                        );
                    }
                }

                let receiver_ty = self.type_of_expr(object, locals)?;
                if let Type::Named(receiver_name, _) = &receiver_ty {
                    if let Some(builtin_member) = BuiltinMember::resolve(receiver_name, field) {
                        self.reject_builtin_receiver_argument_overlap(
                            builtin_member,
                            object,
                            &receiver_ty,
                            args,
                            locals,
                        )?;
                    }
                }
                if receiver_ty == Type::named("Duration") {
                    if let Some(
                        builtin_member @ (BuiltinMember::DurationToMilliseconds
                        | BuiltinMember::DurationToSeconds),
                    ) = BuiltinMember::resolve("Duration", field)
                    {
                        builtin_member.bind_args(args, span)?;
                        return Ok(Type::named("float64"));
                    }
                }
                if let Type::Module(module_path) = &receiver_ty {
                    let namespace = self.module_namespace(module_path).ok_or_else(|| {
                        Diagnostic::at(span, format!("unknown module namespace `{}`", module_path))
                    })?;
                    if let Some(function) = namespace.functions.get(field) {
                        return self.type_check_callable_args(
                            &format!("function `{}`", function.decl.name),
                            &function.decl.type_params,
                            &function.decl.params,
                            &function.signature.param_passings,
                            &function.signature.params,
                            &function.signature.return_type,
                            &function.type_param_bounds,
                            &function.signature.rng_clone_safe_type_params,
                            args,
                            span,
                            locals,
                            expected,
                            HashMap::new(),
                        );
                    }
                    if let Some(class) = namespace.classes.get(field) {
                        if let Some(constructor) = class.builtin_constructor() {
                            return self.type_check_builtin_class_constructor(
                                constructor,
                                args,
                                span,
                                locals,
                                explicit_type_args.is_some(),
                                Type::named(constructor.qualified_name()),
                            );
                        }
                        if matches!(
                            (namespace.path.as_str(), class.decl.name.as_str()),
                            ("fs", "File")
                                | ("net", "TcpStream")
                                | ("net", "TcpListener")
                                | ("net", "UdpSocket")
                                | ("net", "UdpDatagram")
                                | ("net", "HttpListener")
                                | ("net", "HttpExchange")
                                | ("net", "HttpResponse")
                                | ("net", "WebSocketListener")
                                | ("net", "WebSocket")
                                | ("net", "UnixListener")
                                | ("net", "UnixStream")
                                | ("net", "TlsListener")
                                | ("net", "TlsStream")
                        ) {
                            return Err(Diagnostic::at(
                                span,
                                format!(
                                    "builtin resource `{}.{}` must be created through its module functions",
                                    namespace.path, class.decl.name
                                ),
                            ));
                        }
                        let class_type_name = format!("{}.{}", namespace.path, class.decl.name);
                        return self.type_check_user_class_constructor(
                            class,
                            &class.decl.name,
                            &class_type_name,
                            class_type_name.clone(),
                            explicit_type_args,
                            args,
                            span,
                            locals,
                            expected,
                        );
                    }
                    return Err(Diagnostic::coded_at(
                        "AU2001",
                        span,
                        format!(
                            "module `{}` has no callable member `{}`",
                            module_path, field
                        ),
                    ));
                }
                if let Type::Named(receiver_name, receiver_args) = &receiver_ty {
                    if receiver_name == "random.Rng" && receiver_args.is_empty() {
                        if field == "clone" {
                            return self
                                .reject_rng_duplication("random.Rng.clone", &receiver_ty, span)
                                .map(|()| Type::Unit);
                        }
                        if let Some(builtin_member) = BuiltinMember::resolve(receiver_name, field) {
                            let ordered_args = builtin_member.bind_args(args, span)?;
                            self.require_mutable_receiver(object, field, span, locals)?;
                            return match builtin_member {
                                BuiltinMember::RngNextInt => {
                                    for (index, label) in [(0, "lo"), (1, "hi")] {
                                        let argument = self.bound_argument(
                                            &ordered_args,
                                            index,
                                            span,
                                            format!("`next_int` requires a `{label}` argument"),
                                        )?;
                                        let actual = self.type_of_expr_hint(
                                            &argument.value,
                                            locals,
                                            Some(&Type::named("int64")),
                                        )?;
                                        if actual != Type::named("int64") {
                                            return Err(Diagnostic::coded_at(
                                                "AU2002",
                                                argument.span,
                                                format!(
                                                    "`next_int` expects `int64` for `{label}`, found `{actual}`"
                                                ),
                                            ));
                                        }
                                    }
                                    Ok(Type::named("int64"))
                                }
                                BuiltinMember::RngNextFloat => Ok(Type::named("float64")),
                                BuiltinMember::RngShuffle => {
                                    let values = self.bound_argument(
                                        &ordered_args,
                                        0,
                                        span,
                                        "`shuffle` requires a `values` argument",
                                    )?;
                                    let actual = self.type_of_expr(&values.value, locals)?;
                                    if !matches!(
                                        &actual,
                                        Type::Named(name, args)
                                            if name == "Vec" && args.len() == 1
                                    ) {
                                        return Err(Diagnostic::coded_at(
                                            "AU2002",
                                            values.span,
                                            format!("`shuffle` expects `Vec[T]`, found `{actual}`"),
                                        ));
                                    }
                                    self.apply_builtin_argument_passing(
                                        builtin_member,
                                        0,
                                        values,
                                        locals,
                                    )?;
                                    Ok(Type::Unit)
                                }
                                _ => unreachable!("unexpected random.Rng builtin member"),
                            };
                        }
                    }

                    if receiver_name == "Vec" && receiver_args.len() == 1 {
                        if let Some(builtin_member) = BuiltinMember::resolve(receiver_name, field) {
                            let ordered_args = builtin_member.bind_args(args, span)?;
                            return match builtin_member {
                                BuiltinMember::VecLen => Ok(Type::named("int64")),
                                BuiltinMember::VecIsEmpty => Ok(Type::named("bool")),
                                BuiltinMember::VecClone => {
                                    self.reject_rng_duplication("Vec.clone", &receiver_ty, span)?;
                                    Ok(receiver_ty.clone())
                                }
                                BuiltinMember::VecPush => {
                                    self.require_mutable_receiver(object, field, span, locals)?;
                                    let push_arg = self.bound_argument(
                                        &ordered_args,
                                        0,
                                        span,
                                        "`push` requires exactly one argument",
                                    )?;
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
                                    self.apply_builtin_argument_passing(
                                        builtin_member,
                                        0,
                                        push_arg,
                                        locals,
                                    )?;
                                    Ok(Type::Unit)
                                }
                                BuiltinMember::VecPop => {
                                    self.require_mutable_receiver(object, field, span, locals)?;
                                    Ok(Type::Named(
                                        "Option".to_string(),
                                        vec![receiver_args[0].clone()],
                                    ))
                                }
                                BuiltinMember::VecGet => {
                                    let index_arg = self.bound_argument(
                                        &ordered_args,
                                        0,
                                        span,
                                        "`get` requires exactly one argument",
                                    )?;
                                    self.check_vec_index_type(
                                        &index_arg.value,
                                        index_arg.span,
                                        locals,
                                    )?;
                                    self.reject_rng_duplication(
                                        "Vec.get",
                                        &receiver_args[0],
                                        span,
                                    )?;
                                    Ok(Type::Named(
                                        "Option".to_string(),
                                        vec![receiver_args[0].clone()],
                                    ))
                                }
                                BuiltinMember::VecSet => {
                                    self.require_mutable_receiver(object, field, span, locals)?;
                                    let index_arg = self.bound_argument(
                                        &ordered_args,
                                        0,
                                        span,
                                        "`set` requires an `index` argument",
                                    )?;
                                    self.check_vec_index_type(
                                        &index_arg.value,
                                        index_arg.span,
                                        locals,
                                    )?;
                                    let value_arg = self.bound_argument(
                                        &ordered_args,
                                        1,
                                        span,
                                        "`set` requires a `value` argument",
                                    )?;
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
                                    self.apply_builtin_argument_passing(
                                        builtin_member,
                                        1,
                                        value_arg,
                                        locals,
                                    )?;
                                    Ok(Type::Named(
                                        "Option".to_string(),
                                        vec![receiver_args[0].clone()],
                                    ))
                                }
                                BuiltinMember::VecRemove => {
                                    self.require_mutable_receiver(object, field, span, locals)?;
                                    let index_arg = self.bound_argument(
                                        &ordered_args,
                                        0,
                                        span,
                                        "`remove` requires exactly one argument",
                                    )?;
                                    self.check_vec_index_type(
                                        &index_arg.value,
                                        index_arg.span,
                                        locals,
                                    )?;
                                    Ok(Type::Named(
                                        "Option".to_string(),
                                        vec![receiver_args[0].clone()],
                                    ))
                                }
                                BuiltinMember::VecSwap => {
                                    self.require_mutable_receiver(object, field, span, locals)?;
                                    let first_arg = self.bound_argument(
                                        &ordered_args,
                                        0,
                                        span,
                                        "`swap` requires a `first` argument",
                                    )?;
                                    self.check_vec_index_type(
                                        &first_arg.value,
                                        first_arg.span,
                                        locals,
                                    )?;
                                    let second_arg = self.bound_argument(
                                        &ordered_args,
                                        1,
                                        span,
                                        "`swap` requires a `second` argument",
                                    )?;
                                    self.check_vec_index_type(
                                        &second_arg.value,
                                        second_arg.span,
                                        locals,
                                    )?;
                                    Ok(Type::named("bool"))
                                }
                                BuiltinMember::VecContains => {
                                    let value_arg = self.bound_argument(
                                        &ordered_args,
                                        0,
                                        span,
                                        "`contains` requires a `value` argument",
                                    )?;
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
                                    self.require_mutable_receiver(object, field, span, locals)?;
                                    let other_arg = self.bound_argument(
                                        &ordered_args,
                                        0,
                                        span,
                                        "`extend` requires an `other` argument",
                                    )?;
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
                                    self.apply_builtin_argument_passing(
                                        builtin_member,
                                        0,
                                        other_arg,
                                        locals,
                                    )?;
                                    Ok(Type::Unit)
                                }
                                BuiltinMember::VecInsert => {
                                    self.require_mutable_receiver(object, field, span, locals)?;
                                    let index_arg = self.bound_argument(
                                        &ordered_args,
                                        0,
                                        span,
                                        "`insert` requires an `index` argument",
                                    )?;
                                    self.check_vec_index_type(
                                        &index_arg.value,
                                        index_arg.span,
                                        locals,
                                    )?;
                                    let value_arg = self.bound_argument(
                                        &ordered_args,
                                        1,
                                        span,
                                        "`insert` requires a `value` argument",
                                    )?;
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
                                    self.apply_builtin_argument_passing(
                                        builtin_member,
                                        1,
                                        value_arg,
                                        locals,
                                    )?;
                                    Ok(Type::named("bool"))
                                }
                                BuiltinMember::VecClear | BuiltinMember::VecReverse => {
                                    self.require_mutable_receiver(object, field, span, locals)?;
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
                                BuiltinMember::StringLen | BuiltinMember::StringByteLen => {
                                    Ok(Type::named("int64"))
                                }
                                BuiltinMember::StringToBytes => {
                                    Ok(Type::Named("Vec".to_string(), vec![Type::named("uint8")]))
                                }
                                BuiltinMember::StringContains
                                | BuiltinMember::StringStartsWith
                                | BuiltinMember::StringEndsWith => {
                                    let text_arg = self.bound_argument(
                                        &ordered_args,
                                        0,
                                        span,
                                        "string predicate methods require one argument",
                                    )?;
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
                                    let text_arg = self.bound_argument(
                                        &ordered_args,
                                        0,
                                        span,
                                        "`split` requires a `text` argument",
                                    )?;
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
                                    let from_arg = self.bound_argument(
                                        &ordered_args,
                                        0,
                                        span,
                                        "`replace` requires a `from` argument",
                                    )?;
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
                                    let to_arg = self.bound_argument(
                                        &ordered_args,
                                        1,
                                        span,
                                        "`replace` requires a `to` argument",
                                    )?;
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
                                    let parts_arg = self.bound_argument(
                                        &ordered_args,
                                        0,
                                        span,
                                        "`join` requires a `parts` argument",
                                    )?;
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
                                    let text_arg = self.bound_argument(
                                        &ordered_args,
                                        0,
                                        span,
                                        "string strip methods require one `text` argument",
                                    )?;
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
                                BuiltinMember::MapLen => Ok(Type::named("int64")),
                                BuiltinMember::MapIsEmpty => Ok(Type::named("bool")),
                                BuiltinMember::MapClone => {
                                    self.reject_rng_duplication("Map.clone", &receiver_ty, span)?;
                                    Ok(receiver_ty.clone())
                                }
                                BuiltinMember::MapGet => {
                                    let key_arg = self.bound_argument(
                                        &ordered_args,
                                        0,
                                        span,
                                        "`get` requires exactly one key argument",
                                    )?;
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
                                    self.reject_rng_duplication(
                                        "Map.get",
                                        &receiver_args[1],
                                        span,
                                    )?;
                                    Ok(Type::Named(
                                        "Option".to_string(),
                                        vec![receiver_args[1].clone()],
                                    ))
                                }
                                BuiltinMember::MapSet => {
                                    self.require_mutable_receiver(object, field, span, locals)?;
                                    let key_arg = self.bound_argument(
                                        &ordered_args,
                                        0,
                                        span,
                                        "`set` requires a `key` argument",
                                    )?;
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
                                    let value_arg = self.bound_argument(
                                        &ordered_args,
                                        1,
                                        span,
                                        "`set` requires a `value` argument",
                                    )?;
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
                                    self.apply_builtin_argument_passing(
                                        builtin_member,
                                        0,
                                        key_arg,
                                        locals,
                                    )?;
                                    self.apply_builtin_argument_passing(
                                        builtin_member,
                                        1,
                                        value_arg,
                                        locals,
                                    )?;
                                    Ok(Type::Named(
                                        "Option".to_string(),
                                        vec![receiver_args[1].clone()],
                                    ))
                                }
                                BuiltinMember::MapRemove => {
                                    self.require_mutable_receiver(object, field, span, locals)?;
                                    let key_arg = self.bound_argument(
                                        &ordered_args,
                                        0,
                                        span,
                                        "`remove` requires exactly one key argument",
                                    )?;
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
                                    Ok(Type::Named(
                                        "Option".to_string(),
                                        vec![receiver_args[1].clone()],
                                    ))
                                }
                                BuiltinMember::MapContainsKey => {
                                    let key_arg = self.bound_argument(
                                        &ordered_args,
                                        0,
                                        span,
                                        "`contains_key` requires exactly one key argument",
                                    )?;
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
                                BuiltinMember::MapKeys => {
                                    self.reject_rng_duplication(
                                        "Map.keys",
                                        &receiver_args[0],
                                        span,
                                    )?;
                                    Ok(Type::Named(
                                        "Vec".to_string(),
                                        vec![receiver_args[0].clone()],
                                    ))
                                }
                                BuiltinMember::MapValues => {
                                    self.reject_rng_duplication(
                                        "Map.values",
                                        &receiver_args[1],
                                        span,
                                    )?;
                                    Ok(Type::Named(
                                        "Vec".to_string(),
                                        vec![receiver_args[1].clone()],
                                    ))
                                }
                                BuiltinMember::MapItems | BuiltinMember::MapEntries => {
                                    let entry_type = Type::Named(
                                        "MapEntry".to_string(),
                                        vec![receiver_args[0].clone(), receiver_args[1].clone()],
                                    );
                                    self.reject_rng_duplication(
                                        if matches!(builtin_member, BuiltinMember::MapItems) {
                                            "Map.items"
                                        } else {
                                            "Map.entries"
                                        },
                                        &entry_type,
                                        span,
                                    )?;
                                    Ok(Type::Named("Vec".to_string(), vec![entry_type]))
                                }
                                BuiltinMember::MapClear => {
                                    self.require_mutable_receiver(object, field, span, locals)?;
                                    Ok(Type::Unit)
                                }
                                BuiltinMember::MapExtend => {
                                    self.require_mutable_receiver(object, field, span, locals)?;
                                    let other_arg = self.bound_argument(
                                        &ordered_args,
                                        0,
                                        span,
                                        "`extend` requires an `other` argument",
                                    )?;
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
                                    self.apply_builtin_argument_passing(
                                        builtin_member,
                                        0,
                                        other_arg,
                                        locals,
                                    )?;
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
                                BuiltinMember::SetLen => Ok(Type::named("int64")),
                                BuiltinMember::SetIsEmpty => Ok(Type::named("bool")),
                                BuiltinMember::SetClone => {
                                    self.reject_rng_duplication("Set.clone", &receiver_ty, span)?;
                                    Ok(receiver_ty.clone())
                                }
                                BuiltinMember::SetContains => {
                                    let value_arg = self.bound_argument(
                                        &ordered_args,
                                        0,
                                        span,
                                        "`contains` requires a `value` argument",
                                    )?;
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
                                    self.require_mutable_receiver(object, field, span, locals)?;
                                    let value_arg = self.bound_argument(
                                        &ordered_args,
                                        0,
                                        span,
                                        "set mutation requires a `value` argument",
                                    )?;
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
                                    self.apply_builtin_argument_passing(
                                        builtin_member,
                                        0,
                                        value_arg,
                                        locals,
                                    )?;
                                    Ok(Type::named("bool"))
                                }
                                _ => unreachable!("unexpected set builtin member"),
                            };
                        }
                    }

                    if receiver_name == "Queue" && receiver_args.len() == 1 {
                        if let Some(builtin_member) = BuiltinMember::resolve(receiver_name, field) {
                            let ordered_args = builtin_member.bind_args(args, span)?;
                            return match builtin_member {
                                BuiltinMember::QueuePut | BuiltinMember::QueueTryPut => {
                                    let send_arg = self.bound_argument(
                                        &ordered_args,
                                        0,
                                        span,
                                        format!("`{}` requires exactly one argument", field),
                                    )?;
                                    let actual = self.type_of_expr_hint(
                                        &send_arg.value,
                                        locals,
                                        Some(&receiver_args[0]),
                                    )?;
                                    if actual != receiver_args[0] {
                                        return Err(Diagnostic::at(
                                            send_arg.span,
                                            format!(
                                                "`{}` expects `{}`, found `{}`",
                                                field, receiver_args[0], actual
                                            ),
                                        ));
                                    }
                                    self.apply_builtin_argument_passing(
                                        builtin_member,
                                        0,
                                        send_arg,
                                        locals,
                                    )?;
                                    if matches!(builtin_member, BuiltinMember::QueuePut) {
                                        if let Some(timeout_arg) =
                                            ordered_args.get(1).and_then(|arg| *arg)
                                        {
                                            let actual = self.type_of_expr_hint(
                                                &timeout_arg.value,
                                                locals,
                                                Some(&Type::named("Duration")),
                                            )?;
                                            if actual != Type::named("Duration") {
                                                return Err(Diagnostic::at(
                                                    timeout_arg.span,
                                                    format!(
                                                        "`put(timeout=...)` expects `Duration`, found `{}`",
                                                        actual
                                                    ),
                                                ));
                                            }
                                        }
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
                                BuiltinMember::QueueGet | BuiltinMember::QueueGetOrNone => {
                                    if let Some(timeout_arg) = ordered_args[0] {
                                        let actual = self.type_of_expr_hint(
                                            &timeout_arg.value,
                                            locals,
                                            Some(&Type::named("Duration")),
                                        )?;
                                        if actual != Type::named("Duration") {
                                            return Err(Diagnostic::at(
                                                timeout_arg.span,
                                                format!(
                                                    "`get(timeout=...)` expects `Duration`, found `{}`",
                                                    actual
                                                ),
                                            ));
                                        }
                                    }
                                    if matches!(builtin_member, BuiltinMember::QueueGetOrNone) {
                                        Ok(Type::Named(
                                            "Option".to_string(),
                                            vec![receiver_args[0].clone()],
                                        ))
                                    } else {
                                        Ok(Type::Named(
                                            "QueueReceive".to_string(),
                                            vec![receiver_args[0].clone()],
                                        ))
                                    }
                                }
                                BuiltinMember::QueueGetOr => {
                                    let default_arg = self.bound_argument(
                                        &ordered_args,
                                        0,
                                        span,
                                        "`get_or` requires a `default` argument",
                                    )?;
                                    let actual = self.type_of_expr_hint(
                                        &default_arg.value,
                                        locals,
                                        Some(&receiver_args[0]),
                                    )?;
                                    if actual != receiver_args[0] {
                                        return Err(Diagnostic::at(
                                            default_arg.span,
                                            format!(
                                                "`get_or` expects `{}`, found `{}`",
                                                receiver_args[0], actual
                                            ),
                                        ));
                                    }
                                    self.apply_builtin_argument_passing(
                                        builtin_member,
                                        0,
                                        default_arg,
                                        locals,
                                    )?;
                                    if let Some(timeout_arg) = ordered_args[1] {
                                        let actual = self.type_of_expr_hint(
                                            &timeout_arg.value,
                                            locals,
                                            Some(&Type::named("Duration")),
                                        )?;
                                        if actual != Type::named("Duration") {
                                            return Err(Diagnostic::at(
                                                timeout_arg.span,
                                                format!(
                                                    "`get_or(timeout=...)` expects `Duration`, found `{}`",
                                                    actual
                                                ),
                                            ));
                                        }
                                    }
                                    Ok(receiver_args[0].clone())
                                }
                                BuiltinMember::QueueClose => Ok(Type::Unit),
                                _ => unreachable!("unexpected queue builtin member"),
                            };
                        }
                    }

                    if receiver_name == "Task" && receiver_args.len() == 1 {
                        if let Some(builtin_member) = BuiltinMember::resolve(receiver_name, field) {
                            let ordered_args = builtin_member.bind_args(args, span)?;
                            self.reject_rng_duplication(
                                &format!("Task.{field}"),
                                &receiver_args[0],
                                span,
                            )?;
                            return match builtin_member {
                                BuiltinMember::TaskResult | BuiltinMember::TaskResultOrNone => {
                                    if let Some(timeout_arg) = ordered_args[0] {
                                        let actual = self.type_of_expr_hint(
                                            &timeout_arg.value,
                                            locals,
                                            Some(&Type::named("Duration")),
                                        )?;
                                        if actual != Type::named("Duration") {
                                            return Err(Diagnostic::at(
                                                timeout_arg.span,
                                                format!(
                                                    "`result(timeout=...)` expects `Duration`, found `{}`",
                                                    actual
                                                ),
                                            ));
                                        }
                                    }
                                    if matches!(builtin_member, BuiltinMember::TaskResultOrNone) {
                                        Ok(Type::Named(
                                            "Option".to_string(),
                                            vec![receiver_args[0].clone()],
                                        ))
                                    } else {
                                        Ok(Type::Named(
                                            "TaskResult".to_string(),
                                            vec![receiver_args[0].clone()],
                                        ))
                                    }
                                }
                                BuiltinMember::TaskResultOr => {
                                    let default_arg = self.bound_argument(
                                        &ordered_args,
                                        0,
                                        span,
                                        "`result_or` requires a `default` argument",
                                    )?;
                                    let actual = self.type_of_expr_hint(
                                        &default_arg.value,
                                        locals,
                                        Some(&receiver_args[0]),
                                    )?;
                                    if actual != receiver_args[0] {
                                        return Err(Diagnostic::at(
                                            default_arg.span,
                                            format!(
                                                "`result_or` expects `{}`, found `{}`",
                                                receiver_args[0], actual
                                            ),
                                        ));
                                    }
                                    self.apply_builtin_argument_passing(
                                        builtin_member,
                                        0,
                                        default_arg,
                                        locals,
                                    )?;
                                    if let Some(timeout_arg) = ordered_args[1] {
                                        let actual = self.type_of_expr_hint(
                                            &timeout_arg.value,
                                            locals,
                                            Some(&Type::named("Duration")),
                                        )?;
                                        if actual != Type::named("Duration") {
                                            return Err(Diagnostic::at(
                                                timeout_arg.span,
                                                format!(
                                                    "`result_or(timeout=...)` expects `Duration`, found `{}`",
                                                    actual
                                                ),
                                            ));
                                        }
                                    }
                                    Ok(receiver_args[0].clone())
                                }
                                _ => unreachable!("unexpected task builtin member"),
                            };
                        }
                    }

                    if receiver_name == "TaskGroup" && receiver_args.is_empty() {
                        match field.as_str() {
                            "start" | "start_soon" => {
                                if args.is_empty() {
                                    return Err(Diagnostic::at(
                                        span,
                                        format!(
                                            "`{}` expects a target function followed by its arguments",
                                            field
                                        ),
                                    ));
                                }
                                if args[0].name.is_some() {
                                    return Err(Diagnostic::at(
                                        args[0].span,
                                        format!("`{}` does not take keyword arguments", field),
                                    ));
                                }
                                let callable = self.resolve_spawn_callable(&args[0].value)?;
                                self.require_task_startable_function(
                                    &callable.display_name,
                                    &callable.decl.params,
                                    &callable.signature.param_passings,
                                    args[0].span,
                                )?;
                                let spawn_args = &args[1..];
                                let capture_passings = vec![
                                    ReceiverKind::Value;
                                    callable.signature.param_passings.len()
                                ];
                                self.type_check_callable_args(
                                    &callable.display_name,
                                    &callable.decl.type_params,
                                    &callable.decl.params,
                                    &capture_passings,
                                    &callable.signature.params,
                                    &callable.signature.return_type,
                                    &callable.type_param_bounds,
                                    &callable.signature.rng_clone_safe_type_params,
                                    spawn_args,
                                    span,
                                    locals,
                                    None,
                                    callable.seed_substitutions,
                                )?;
                                return Ok(if field == "start" {
                                    Type::Named(
                                        "Task".to_string(),
                                        vec![callable.signature.return_type.clone()],
                                    )
                                } else {
                                    Type::Unit
                                });
                            }
                            "cancel" => {
                                BuiltinMember::TaskGroupCancel.bind_args(args, span)?;
                                return Ok(Type::Unit);
                            }
                            _ => {}
                        }
                    }

                    if receiver_name == "fs.File" && receiver_args.is_empty() {
                        if let Some(builtin_member) = BuiltinMember::resolve(receiver_name, field) {
                            let bytes_ty =
                                Type::Named("Vec".to_string(), vec![Type::named("uint8")]);
                            let ordered_args = builtin_member.bind_args(args, span)?;
                            return match builtin_member {
                                BuiltinMember::FileReadAll => Ok(Type::Named(
                                    "Result".to_string(),
                                    vec![
                                        Type::named("String"),
                                        crate::builtin_modules::io_error_type(),
                                    ],
                                )),
                                BuiltinMember::FileReadBytes => Ok(Type::Named(
                                    "Result".to_string(),
                                    vec![bytes_ty.clone(), crate::builtin_modules::io_error_type()],
                                )),
                                BuiltinMember::FileWriteAll => {
                                    self.require_mutable_receiver(object, field, span, locals)?;
                                    let text_arg = self.bound_argument(
                                        &ordered_args,
                                        0,
                                        span,
                                        "`write_all` requires a `text` argument",
                                    )?;
                                    let actual = self.type_of_expr_hint(
                                        &text_arg.value,
                                        locals,
                                        Some(&Type::named("String")),
                                    )?;
                                    if actual != Type::named("String") {
                                        return Err(Diagnostic::at(
                                            text_arg.span,
                                            format!(
                                                "`write_all` expects `String`, found `{}`",
                                                actual
                                            ),
                                        ));
                                    }
                                    Ok(Type::Named(
                                        "Result".to_string(),
                                        vec![Type::Unit, crate::builtin_modules::io_error_type()],
                                    ))
                                }
                                BuiltinMember::FileWriteBytes => {
                                    self.require_mutable_receiver(object, field, span, locals)?;
                                    let bytes_arg = self.bound_argument(
                                        &ordered_args,
                                        0,
                                        span,
                                        "`write_bytes` requires a `bytes` argument",
                                    )?;
                                    self.check_builtin_argument_type(
                                        bytes_arg,
                                        &bytes_ty,
                                        locals,
                                        "write_bytes",
                                    )?;
                                    Ok(Type::Named(
                                        "Result".to_string(),
                                        vec![Type::Unit, crate::builtin_modules::io_error_type()],
                                    ))
                                }
                                BuiltinMember::FileFlush => Ok(Type::Named(
                                    "Result".to_string(),
                                    vec![Type::Unit, crate::builtin_modules::io_error_type()],
                                )),
                                BuiltinMember::FileClose => Ok(Type::Unit),
                                _ => unreachable!("unexpected file builtin member"),
                            };
                        }
                    }

                    if receiver_name == "process.Child" && receiver_args.is_empty() {
                        if let Some(builtin_member) = BuiltinMember::resolve(receiver_name, field) {
                            let ordered_args = builtin_member.bind_args(args, span)?;
                            return match builtin_member {
                                BuiltinMember::ProcessChildStdin
                                | BuiltinMember::ProcessChildStdout
                                | BuiltinMember::ProcessChildStderr => Ok(Type::Named(
                                    "Option".to_string(),
                                    vec![Type::named("process.Pipe")],
                                )),
                                BuiltinMember::ProcessChildWait => {
                                    self.check_optional_builtin_timeout_argument(
                                        &ordered_args,
                                        0,
                                        locals,
                                        "wait(timeout=...)",
                                    )?;
                                    Ok(Type::named("process.Wait"))
                                }
                                BuiltinMember::ProcessChildWaitOrNone => {
                                    self.check_optional_builtin_timeout_argument(
                                        &ordered_args,
                                        0,
                                        locals,
                                        "wait_or_none(timeout=...)",
                                    )?;
                                    Ok(Type::Named(
                                        "Result".to_string(),
                                        vec![
                                            Type::Named(
                                                "Option".to_string(),
                                                vec![Type::named("process.ExitStatus")],
                                            ),
                                            crate::builtin_modules::process_error_type(),
                                        ],
                                    ))
                                }
                                BuiltinMember::ProcessChildWaitOk => {
                                    self.check_optional_builtin_timeout_argument(
                                        &ordered_args,
                                        0,
                                        locals,
                                        "wait_ok(timeout=...)",
                                    )?;
                                    Ok(Type::Named(
                                        "Result".to_string(),
                                        vec![
                                            Type::named("process.ExitStatus"),
                                            crate::builtin_modules::process_error_type(),
                                        ],
                                    ))
                                }
                                BuiltinMember::ProcessChildKill
                                | BuiltinMember::ProcessChildTerminate => Ok(Type::Named(
                                    "Result".to_string(),
                                    vec![Type::Unit, crate::builtin_modules::process_error_type()],
                                )),
                                BuiltinMember::ProcessChildClose => Ok(Type::Unit),
                                _ => unreachable!("unexpected process child builtin member"),
                            };
                        }
                    }

                    if receiver_name == "process.Pipe" && receiver_args.is_empty() {
                        if let Some(builtin_member) = BuiltinMember::resolve(receiver_name, field) {
                            let bytes_ty =
                                Type::Named("Vec".to_string(), vec![Type::named("uint8")]);
                            let ordered_args = builtin_member.bind_args(args, span)?;
                            return match builtin_member {
                                BuiltinMember::ProcessPipeReadAll => Ok(Type::Named(
                                    "Result".to_string(),
                                    vec![
                                        Type::named("String"),
                                        crate::builtin_modules::process_error_type(),
                                    ],
                                )),
                                BuiltinMember::ProcessPipeReadLine => {
                                    self.check_optional_builtin_timeout_argument(
                                        &ordered_args,
                                        0,
                                        locals,
                                        "read_line(timeout=...)",
                                    )?;
                                    Ok(Type::Named(
                                        "Result".to_string(),
                                        vec![
                                            Type::Named(
                                                "Option".to_string(),
                                                vec![Type::named("String")],
                                            ),
                                            crate::builtin_modules::process_error_type(),
                                        ],
                                    ))
                                }
                                BuiltinMember::ProcessPipeReadBytes => {
                                    let count_arg = self.bound_argument(
                                        &ordered_args,
                                        0,
                                        span,
                                        "`read_bytes` requires a `max_bytes` argument",
                                    )?;
                                    self.check_builtin_argument_type(
                                        count_arg,
                                        &Type::named("int32"),
                                        locals,
                                        "read_bytes",
                                    )?;
                                    self.check_optional_builtin_timeout_argument(
                                        &ordered_args,
                                        1,
                                        locals,
                                        "read_bytes(timeout=...)",
                                    )?;
                                    Ok(Type::Named(
                                        "Result".to_string(),
                                        vec![
                                            Type::Named(
                                                "Option".to_string(),
                                                vec![bytes_ty.clone()],
                                            ),
                                            crate::builtin_modules::process_error_type(),
                                        ],
                                    ))
                                }
                                BuiltinMember::ProcessPipeWriteAll => {
                                    let text_arg = self.bound_argument(
                                        &ordered_args,
                                        0,
                                        span,
                                        "`write_all` requires a `text` argument",
                                    )?;
                                    self.check_builtin_argument_type(
                                        text_arg,
                                        &Type::named("String"),
                                        locals,
                                        "write_all",
                                    )?;
                                    self.check_optional_builtin_timeout_argument(
                                        &ordered_args,
                                        1,
                                        locals,
                                        "write_all(timeout=...)",
                                    )?;
                                    Ok(Type::Named(
                                        "Result".to_string(),
                                        vec![
                                            Type::Unit,
                                            crate::builtin_modules::process_error_type(),
                                        ],
                                    ))
                                }
                                BuiltinMember::ProcessPipeWriteBytes => {
                                    let bytes_arg = self.bound_argument(
                                        &ordered_args,
                                        0,
                                        span,
                                        "`write_bytes` requires a `bytes` argument",
                                    )?;
                                    self.check_builtin_argument_type(
                                        bytes_arg,
                                        &bytes_ty,
                                        locals,
                                        "write_bytes",
                                    )?;
                                    self.check_optional_builtin_timeout_argument(
                                        &ordered_args,
                                        1,
                                        locals,
                                        "write_bytes(timeout=...)",
                                    )?;
                                    Ok(Type::Named(
                                        "Result".to_string(),
                                        vec![
                                            Type::Unit,
                                            crate::builtin_modules::process_error_type(),
                                        ],
                                    ))
                                }
                                BuiltinMember::ProcessPipeFlush => Ok(Type::Named(
                                    "Result".to_string(),
                                    vec![Type::Unit, crate::builtin_modules::process_error_type()],
                                )),
                                BuiltinMember::ProcessPipeClose => Ok(Type::Unit),
                                _ => unreachable!("unexpected process pipe builtin member"),
                            };
                        }
                    }

                    if receiver_name == "process.Completed" && receiver_args.is_empty() {
                        if let Some(builtin_member) = BuiltinMember::resolve(receiver_name, field) {
                            builtin_member.bind_args(args, span)?;
                            return match builtin_member {
                                BuiltinMember::ProcessCompletedStatus => {
                                    Ok(Type::named("process.ExitStatus"))
                                }
                                BuiltinMember::ProcessCompletedSuccess => Ok(Type::named("bool")),
                                BuiltinMember::ProcessCompletedStdout
                                | BuiltinMember::ProcessCompletedStderr => {
                                    Ok(Type::named("String"))
                                }
                                BuiltinMember::ProcessCompletedStdoutBytes
                                | BuiltinMember::ProcessCompletedStderrBytes => {
                                    Ok(Type::Named("Vec".to_string(), vec![Type::named("uint8")]))
                                }
                                BuiltinMember::ProcessCompletedCheck => Ok(Type::Named(
                                    "Result".to_string(),
                                    vec![Type::Unit, crate::builtin_modules::process_error_type()],
                                )),
                                _ => unreachable!("unexpected process completed builtin member"),
                            };
                        }
                    }

                    if receiver_name == "process.Supervisor" && receiver_args.is_empty() {
                        if let Some(builtin_member) = BuiltinMember::resolve(receiver_name, field) {
                            let ordered_args = builtin_member.bind_args(args, span)?;
                            return match builtin_member {
                                BuiltinMember::ProcessSupervisorStart => {
                                    self.check_builtin_argument_type(
                                        self.bound_argument(
                                            &ordered_args,
                                            0,
                                            span,
                                            "`start` requires a `name` argument",
                                        )?,
                                        &Type::named("String"),
                                        locals,
                                        "start",
                                    )?;
                                    self.check_builtin_argument_type(
                                        self.bound_argument(
                                            &ordered_args,
                                            1,
                                            span,
                                            "`start` requires a `command` argument",
                                        )?,
                                        &Type::Named(
                                            "Vec".to_string(),
                                            vec![Type::named("String")],
                                        ),
                                        locals,
                                        "start",
                                    )?;
                                    if let Some(argument) = ordered_args.get(2).copied().flatten() {
                                        self.check_builtin_argument_type(
                                            argument,
                                            &Type::Named(
                                                "Option".to_string(),
                                                vec![Type::named("String")],
                                            ),
                                            locals,
                                            "start",
                                        )?;
                                    }
                                    if let Some(argument) = ordered_args.get(3).copied().flatten() {
                                        self.check_builtin_argument_type(
                                            argument,
                                            &Type::Named(
                                                "Map".to_string(),
                                                vec![Type::named("String"), Type::named("String")],
                                            ),
                                            locals,
                                            "start",
                                        )?;
                                    }
                                    if let Some(argument) = ordered_args.get(4).copied().flatten() {
                                        self.check_builtin_argument_type(
                                            argument,
                                            &Type::named("process.Stdio"),
                                            locals,
                                            "start",
                                        )?;
                                    }
                                    if let Some(argument) = ordered_args.get(5).copied().flatten() {
                                        self.check_builtin_argument_type(
                                            argument,
                                            &Type::named("process.Stdio"),
                                            locals,
                                            "start",
                                        )?;
                                    }
                                    if let Some(argument) = ordered_args.get(6).copied().flatten() {
                                        self.check_builtin_argument_type(
                                            argument,
                                            &Type::named("process.Stdio"),
                                            locals,
                                            "start",
                                        )?;
                                    }
                                    if let Some(argument) = ordered_args.get(7).copied().flatten() {
                                        let expected = Type::named("process.RestartPolicy");
                                        let actual = self.type_of_expr_hint(
                                            &argument.value,
                                            locals,
                                            Some(&expected),
                                        )?;
                                        if actual != expected
                                            && actual != Type::named("RestartPolicy")
                                        {
                                            return Err(Diagnostic::at(
                                                argument.span,
                                                format!(
                                                    "`start` expects `process.RestartPolicy`, found `{}`",
                                                    actual
                                                ),
                                            ));
                                        }
                                    }
                                    if let Some(argument) = ordered_args.get(8).copied().flatten() {
                                        self.check_builtin_argument_type(
                                            argument,
                                            &Type::named("Duration"),
                                            locals,
                                            "start",
                                        )?;
                                    }
                                    if let Some(argument) = ordered_args.get(9).copied().flatten() {
                                        self.check_builtin_argument_type(
                                            argument,
                                            &Type::named("int32"),
                                            locals,
                                            "start",
                                        )?;
                                    }
                                    if let Some(argument) = ordered_args.get(10).copied().flatten()
                                    {
                                        self.check_builtin_argument_type(
                                            argument,
                                            &Type::named("bool"),
                                            locals,
                                            "start",
                                        )?;
                                    }
                                    for (index, argument) in ordered_args.iter().enumerate() {
                                        if let Some(argument) = *argument {
                                            self.apply_builtin_argument_passing(
                                                builtin_member,
                                                index,
                                                argument,
                                                locals,
                                            )?;
                                        }
                                    }
                                    Ok(Type::Named(
                                        "Result".to_string(),
                                        vec![
                                            Type::Unit,
                                            crate::builtin_modules::process_error_type(),
                                        ],
                                    ))
                                }
                                BuiltinMember::ProcessSupervisorWait => {
                                    self.check_optional_builtin_timeout_argument(
                                        &ordered_args,
                                        0,
                                        locals,
                                        "wait(timeout=...)",
                                    )?;
                                    Ok(Type::named("process.SupervisorWait"))
                                }
                                BuiltinMember::ProcessSupervisorWaitOrNone => {
                                    self.check_optional_builtin_timeout_argument(
                                        &ordered_args,
                                        0,
                                        locals,
                                        "wait_or_none(timeout=...)",
                                    )?;
                                    Ok(Type::Named(
                                        "Result".to_string(),
                                        vec![
                                            Type::Named(
                                                "Option".to_string(),
                                                vec![Type::named("process.SupervisorEvent")],
                                            ),
                                            crate::builtin_modules::process_error_type(),
                                        ],
                                    ))
                                }
                                BuiltinMember::ProcessSupervisorStop => Ok(Type::Named(
                                    "Result".to_string(),
                                    vec![Type::Unit, crate::builtin_modules::process_error_type()],
                                )),
                                BuiltinMember::ProcessSupervisorIsEmpty => Ok(Type::named("bool")),
                                BuiltinMember::ProcessSupervisorClose => Ok(Type::Unit),
                                _ => unreachable!("unexpected process supervisor builtin member"),
                            };
                        }
                    }

                    if receiver_name == "net.TcpListener" && receiver_args.is_empty() {
                        if let Some(builtin_member) = BuiltinMember::resolve(receiver_name, field) {
                            let ordered_args = builtin_member.bind_args(args, span)?;
                            return match builtin_member {
                                BuiltinMember::TcpListenerAccept => {
                                    self.check_optional_builtin_timeout_argument(
                                        &ordered_args,
                                        0,
                                        locals,
                                        "accept(timeout=...)",
                                    )?;
                                    Ok(Type::Named(
                                        "Result".to_string(),
                                        vec![
                                            Type::named("net.TcpStream"),
                                            crate::builtin_modules::io_error_type(),
                                        ],
                                    ))
                                }
                                BuiltinMember::TcpListenerLocalAddr => Ok(Type::Named(
                                    "Result".to_string(),
                                    vec![
                                        Type::named("String"),
                                        crate::builtin_modules::io_error_type(),
                                    ],
                                )),
                                BuiltinMember::TcpListenerClose => Ok(Type::Unit),
                                _ => unreachable!("unexpected tcp listener builtin member"),
                            };
                        }
                    }

                    if receiver_name == "net.TcpStream" && receiver_args.is_empty() {
                        if let Some(builtin_member) = BuiltinMember::resolve(receiver_name, field) {
                            let bytes_ty =
                                Type::Named("Vec".to_string(), vec![Type::named("uint8")]);
                            let ordered_args = builtin_member.bind_args(args, span)?;
                            return match builtin_member {
                                BuiltinMember::TcpStreamReadAll => {
                                    self.check_optional_builtin_timeout_argument(
                                        &ordered_args,
                                        0,
                                        locals,
                                        "read_all(timeout=...)",
                                    )?;
                                    Ok(Type::Named(
                                        "Result".to_string(),
                                        vec![
                                            Type::named("String"),
                                            crate::builtin_modules::io_error_type(),
                                        ],
                                    ))
                                }
                                BuiltinMember::TcpStreamReadLine => {
                                    self.check_optional_builtin_timeout_argument(
                                        &ordered_args,
                                        0,
                                        locals,
                                        "read_line(timeout=...)",
                                    )?;
                                    Ok(Type::Named(
                                        "Result".to_string(),
                                        vec![
                                            Type::Named(
                                                "Option".to_string(),
                                                vec![Type::named("String")],
                                            ),
                                            crate::builtin_modules::io_error_type(),
                                        ],
                                    ))
                                }
                                BuiltinMember::TcpStreamReadBytes => {
                                    let count_arg = self.bound_argument(
                                        &ordered_args,
                                        0,
                                        span,
                                        "`read_bytes` requires a `max_bytes` argument",
                                    )?;
                                    self.check_builtin_argument_type(
                                        count_arg,
                                        &Type::named("int32"),
                                        locals,
                                        "read_bytes",
                                    )?;
                                    self.check_optional_builtin_timeout_argument(
                                        &ordered_args,
                                        1,
                                        locals,
                                        "read_bytes(timeout=...)",
                                    )?;
                                    Ok(Type::Named(
                                        "Result".to_string(),
                                        vec![
                                            Type::Named(
                                                "Option".to_string(),
                                                vec![bytes_ty.clone()],
                                            ),
                                            crate::builtin_modules::io_error_type(),
                                        ],
                                    ))
                                }
                                BuiltinMember::TcpStreamReadExact => {
                                    let count_arg = self.bound_argument(
                                        &ordered_args,
                                        0,
                                        span,
                                        "`read_exact` requires a `count` argument",
                                    )?;
                                    self.check_builtin_argument_type(
                                        count_arg,
                                        &Type::named("int32"),
                                        locals,
                                        "read_exact",
                                    )?;
                                    self.check_optional_builtin_timeout_argument(
                                        &ordered_args,
                                        1,
                                        locals,
                                        "read_exact(timeout=...)",
                                    )?;
                                    Ok(Type::Named(
                                        "Result".to_string(),
                                        vec![
                                            bytes_ty.clone(),
                                            crate::builtin_modules::io_error_type(),
                                        ],
                                    ))
                                }
                                BuiltinMember::TcpStreamWriteAll => {
                                    let text_arg = self.bound_argument(
                                        &ordered_args,
                                        0,
                                        span,
                                        "`write_all` requires a `text` argument",
                                    )?;
                                    let actual = self.type_of_expr_hint(
                                        &text_arg.value,
                                        locals,
                                        Some(&Type::named("String")),
                                    )?;
                                    if actual != Type::named("String") {
                                        return Err(Diagnostic::at(
                                            text_arg.span,
                                            format!(
                                                "`write_all` expects `String`, found `{}`",
                                                actual
                                            ),
                                        ));
                                    }
                                    self.check_optional_builtin_timeout_argument(
                                        &ordered_args,
                                        1,
                                        locals,
                                        "write_all(timeout=...)",
                                    )?;
                                    Ok(Type::Named(
                                        "Result".to_string(),
                                        vec![Type::Unit, crate::builtin_modules::io_error_type()],
                                    ))
                                }
                                BuiltinMember::TcpStreamWriteBytes => {
                                    let bytes_arg = self.bound_argument(
                                        &ordered_args,
                                        0,
                                        span,
                                        "`write_bytes` requires a `bytes` argument",
                                    )?;
                                    self.check_builtin_argument_type(
                                        bytes_arg,
                                        &bytes_ty,
                                        locals,
                                        "write_bytes",
                                    )?;
                                    self.check_optional_builtin_timeout_argument(
                                        &ordered_args,
                                        1,
                                        locals,
                                        "write_bytes(timeout=...)",
                                    )?;
                                    Ok(Type::Named(
                                        "Result".to_string(),
                                        vec![Type::Unit, crate::builtin_modules::io_error_type()],
                                    ))
                                }
                                BuiltinMember::TcpStreamFlush
                                | BuiltinMember::TcpStreamLocalAddr
                                | BuiltinMember::TcpStreamPeerAddr
                                | BuiltinMember::TcpStreamShutdownRead
                                | BuiltinMember::TcpStreamShutdownWrite
                                | BuiltinMember::TcpStreamShutdownBoth => {
                                    let value_ty = if matches!(
                                        builtin_member,
                                        BuiltinMember::TcpStreamFlush
                                            | BuiltinMember::TcpStreamShutdownRead
                                            | BuiltinMember::TcpStreamShutdownWrite
                                            | BuiltinMember::TcpStreamShutdownBoth
                                    ) {
                                        Type::Unit
                                    } else {
                                        Type::named("String")
                                    };
                                    Ok(Type::Named(
                                        "Result".to_string(),
                                        vec![value_ty, crate::builtin_modules::io_error_type()],
                                    ))
                                }
                                BuiltinMember::TcpStreamClose => Ok(Type::Unit),
                                _ => unreachable!("unexpected tcp stream builtin member"),
                            };
                        }
                    }

                    if receiver_name == "net.UdpSocket" && receiver_args.is_empty() {
                        if let Some(builtin_member) = BuiltinMember::resolve(receiver_name, field) {
                            let bytes_ty =
                                Type::Named("Vec".to_string(), vec![Type::named("uint8")]);
                            let ordered_args = builtin_member.bind_args(args, span)?;
                            return match builtin_member {
                                BuiltinMember::UdpSocketSendText => {
                                    let address_arg = self.bound_argument(
                                        &ordered_args,
                                        0,
                                        span,
                                        "internal error",
                                    )?;
                                    self.check_builtin_argument_type(
                                        address_arg,
                                        &Type::named("String"),
                                        locals,
                                        "send_text",
                                    )?;
                                    let text_arg = self.bound_argument(
                                        &ordered_args,
                                        1,
                                        span,
                                        "internal error",
                                    )?;
                                    self.check_builtin_argument_type(
                                        text_arg,
                                        &Type::named("String"),
                                        locals,
                                        "send_text",
                                    )?;
                                    self.check_optional_builtin_timeout_argument(
                                        &ordered_args,
                                        2,
                                        locals,
                                        "send_text(timeout=...)",
                                    )?;
                                    Ok(Type::Named(
                                        "Result".to_string(),
                                        vec![Type::Unit, crate::builtin_modules::io_error_type()],
                                    ))
                                }
                                BuiltinMember::UdpSocketSendBytes => {
                                    let address_arg = self.bound_argument(
                                        &ordered_args,
                                        0,
                                        span,
                                        "internal error",
                                    )?;
                                    self.check_builtin_argument_type(
                                        address_arg,
                                        &Type::named("String"),
                                        locals,
                                        "send_bytes",
                                    )?;
                                    let bytes_arg = self.bound_argument(
                                        &ordered_args,
                                        1,
                                        span,
                                        "internal error",
                                    )?;
                                    self.check_builtin_argument_type(
                                        bytes_arg,
                                        &bytes_ty,
                                        locals,
                                        "send_bytes",
                                    )?;
                                    self.check_optional_builtin_timeout_argument(
                                        &ordered_args,
                                        2,
                                        locals,
                                        "send_bytes(timeout=...)",
                                    )?;
                                    Ok(Type::Named(
                                        "Result".to_string(),
                                        vec![Type::Unit, crate::builtin_modules::io_error_type()],
                                    ))
                                }
                                BuiltinMember::UdpSocketRecv => {
                                    let count_arg = self.bound_argument(
                                        &ordered_args,
                                        0,
                                        span,
                                        "internal error",
                                    )?;
                                    self.check_builtin_argument_type(
                                        count_arg,
                                        &Type::named("int32"),
                                        locals,
                                        "recv",
                                    )?;
                                    self.check_optional_builtin_timeout_argument(
                                        &ordered_args,
                                        1,
                                        locals,
                                        "recv(timeout=...)",
                                    )?;
                                    Ok(Type::Named(
                                        "Result".to_string(),
                                        vec![
                                            Type::Named(
                                                "Option".to_string(),
                                                vec![bytes_ty.clone()],
                                            ),
                                            crate::builtin_modules::io_error_type(),
                                        ],
                                    ))
                                }
                                BuiltinMember::UdpSocketRecvFrom => {
                                    let count_arg = self.bound_argument(
                                        &ordered_args,
                                        0,
                                        span,
                                        "internal error",
                                    )?;
                                    self.check_builtin_argument_type(
                                        count_arg,
                                        &Type::named("int32"),
                                        locals,
                                        "recv_from",
                                    )?;
                                    self.check_optional_builtin_timeout_argument(
                                        &ordered_args,
                                        1,
                                        locals,
                                        "recv_from(timeout=...)",
                                    )?;
                                    Ok(Type::Named(
                                        "Result".to_string(),
                                        vec![
                                            Type::Named(
                                                "Option".to_string(),
                                                vec![Type::named("net.UdpDatagram")],
                                            ),
                                            crate::builtin_modules::io_error_type(),
                                        ],
                                    ))
                                }
                                BuiltinMember::UdpSocketLocalAddr
                                | BuiltinMember::UdpSocketPeerAddr => Ok(Type::Named(
                                    "Result".to_string(),
                                    vec![
                                        Type::named("String"),
                                        crate::builtin_modules::io_error_type(),
                                    ],
                                )),
                                BuiltinMember::UdpSocketClose => Ok(Type::Unit),
                                _ => unreachable!("unexpected udp socket builtin member"),
                            };
                        }
                    }

                    if receiver_name == "net.UdpDatagram" && receiver_args.is_empty() {
                        if let Some(builtin_member) = BuiltinMember::resolve(receiver_name, field) {
                            builtin_member.bind_args(args, span)?;
                            let bytes_ty =
                                Type::Named("Vec".to_string(), vec![Type::named("uint8")]);
                            return match builtin_member {
                                BuiltinMember::UdpDatagramAddress => Ok(Type::named("String")),
                                BuiltinMember::UdpDatagramBytes => Ok(bytes_ty),
                                BuiltinMember::UdpDatagramText => Ok(Type::Named(
                                    "Result".to_string(),
                                    vec![
                                        Type::named("String"),
                                        crate::builtin_modules::io_error_type(),
                                    ],
                                )),
                                _ => unreachable!("unexpected udp datagram builtin member"),
                            };
                        }
                    }

                    if receiver_name == "net.HttpListener" && receiver_args.is_empty() {
                        if let Some(builtin_member) = BuiltinMember::resolve(receiver_name, field) {
                            let ordered_args = builtin_member.bind_args(args, span)?;
                            return match builtin_member {
                                BuiltinMember::HttpListenerAccept => {
                                    self.check_optional_builtin_timeout_argument(
                                        &ordered_args,
                                        0,
                                        locals,
                                        "accept(timeout=...)",
                                    )?;
                                    Ok(Type::Named(
                                        "Result".to_string(),
                                        vec![
                                            Type::named("net.HttpExchange"),
                                            crate::builtin_modules::io_error_type(),
                                        ],
                                    ))
                                }
                                BuiltinMember::HttpListenerLocalAddr => Ok(Type::Named(
                                    "Result".to_string(),
                                    vec![
                                        Type::named("String"),
                                        crate::builtin_modules::io_error_type(),
                                    ],
                                )),
                                BuiltinMember::HttpListenerClose => Ok(Type::Unit),
                                _ => unreachable!("unexpected http listener builtin member"),
                            };
                        }
                    }

                    if receiver_name == "net.HttpExchange" && receiver_args.is_empty() {
                        if let Some(builtin_member) = BuiltinMember::resolve(receiver_name, field) {
                            let bytes_ty =
                                Type::Named("Vec".to_string(), vec![Type::named("uint8")]);
                            let headers_ty = Type::Named(
                                "Map".to_string(),
                                vec![Type::named("String"), Type::named("String")],
                            );
                            let ordered_args = builtin_member.bind_args(args, span)?;
                            return match builtin_member {
                                BuiltinMember::HttpExchangeMethod
                                | BuiltinMember::HttpExchangePath => Ok(Type::named("String")),
                                BuiltinMember::HttpExchangeHeaders => Ok(headers_ty),
                                BuiltinMember::HttpExchangeBodyText => Ok(Type::Named(
                                    "Result".to_string(),
                                    vec![
                                        Type::named("String"),
                                        crate::builtin_modules::io_error_type(),
                                    ],
                                )),
                                BuiltinMember::HttpExchangeBodyBytes => Ok(bytes_ty),
                                BuiltinMember::HttpExchangeRespondText => {
                                    let status_arg = self.bound_argument(
                                        &ordered_args,
                                        0,
                                        span,
                                        "internal error",
                                    )?;
                                    self.check_builtin_argument_type(
                                        status_arg,
                                        &Type::named("int32"),
                                        locals,
                                        "respond_text",
                                    )?;
                                    let text_arg = self.bound_argument(
                                        &ordered_args,
                                        1,
                                        span,
                                        "internal error",
                                    )?;
                                    self.check_builtin_argument_type(
                                        text_arg,
                                        &Type::named("String"),
                                        locals,
                                        "respond_text",
                                    )?;
                                    let headers_arg = self.bound_argument(
                                        &ordered_args,
                                        2,
                                        span,
                                        "internal error",
                                    )?;
                                    self.check_builtin_argument_type(
                                        headers_arg,
                                        &headers_ty,
                                        locals,
                                        "respond_text",
                                    )?;
                                    self.apply_builtin_argument_passing(
                                        builtin_member,
                                        1,
                                        text_arg,
                                        locals,
                                    )?;
                                    self.apply_builtin_argument_passing(
                                        builtin_member,
                                        2,
                                        headers_arg,
                                        locals,
                                    )?;
                                    Ok(Type::Named(
                                        "Result".to_string(),
                                        vec![Type::Unit, crate::builtin_modules::io_error_type()],
                                    ))
                                }
                                BuiltinMember::HttpExchangeRespondBytes => {
                                    let status_arg = self.bound_argument(
                                        &ordered_args,
                                        0,
                                        span,
                                        "internal error",
                                    )?;
                                    self.check_builtin_argument_type(
                                        status_arg,
                                        &Type::named("int32"),
                                        locals,
                                        "respond_bytes",
                                    )?;
                                    let bytes_arg = self.bound_argument(
                                        &ordered_args,
                                        1,
                                        span,
                                        "internal error",
                                    )?;
                                    self.check_builtin_argument_type(
                                        bytes_arg,
                                        &bytes_ty,
                                        locals,
                                        "respond_bytes",
                                    )?;
                                    let headers_arg = self.bound_argument(
                                        &ordered_args,
                                        2,
                                        span,
                                        "internal error",
                                    )?;
                                    self.check_builtin_argument_type(
                                        headers_arg,
                                        &headers_ty,
                                        locals,
                                        "respond_bytes",
                                    )?;
                                    self.apply_builtin_argument_passing(
                                        builtin_member,
                                        1,
                                        bytes_arg,
                                        locals,
                                    )?;
                                    self.apply_builtin_argument_passing(
                                        builtin_member,
                                        2,
                                        headers_arg,
                                        locals,
                                    )?;
                                    Ok(Type::Named(
                                        "Result".to_string(),
                                        vec![Type::Unit, crate::builtin_modules::io_error_type()],
                                    ))
                                }
                                _ => unreachable!("unexpected http exchange builtin member"),
                            };
                        }
                    }

                    if receiver_name == "net.HttpResponse" && receiver_args.is_empty() {
                        if let Some(builtin_member) = BuiltinMember::resolve(receiver_name, field) {
                            builtin_member.bind_args(args, span)?;
                            let bytes_ty =
                                Type::Named("Vec".to_string(), vec![Type::named("uint8")]);
                            let headers_ty = Type::Named(
                                "Map".to_string(),
                                vec![Type::named("String"), Type::named("String")],
                            );
                            return match builtin_member {
                                BuiltinMember::HttpResponseStatus => Ok(Type::named("int32")),
                                BuiltinMember::HttpResponseReason => Ok(Type::named("String")),
                                BuiltinMember::HttpResponseHeaders => Ok(headers_ty),
                                BuiltinMember::HttpResponseText => Ok(Type::Named(
                                    "Result".to_string(),
                                    vec![
                                        Type::named("String"),
                                        crate::builtin_modules::io_error_type(),
                                    ],
                                )),
                                BuiltinMember::HttpResponseBytes => Ok(bytes_ty),
                                _ => unreachable!("unexpected http response builtin member"),
                            };
                        }
                    }

                    if receiver_name == "net.WebSocketListener" && receiver_args.is_empty() {
                        if let Some(builtin_member) = BuiltinMember::resolve(receiver_name, field) {
                            let ordered_args = builtin_member.bind_args(args, span)?;
                            return match builtin_member {
                                BuiltinMember::WebSocketListenerAccept => {
                                    self.check_optional_builtin_timeout_argument(
                                        &ordered_args,
                                        0,
                                        locals,
                                        "accept(timeout=...)",
                                    )?;
                                    Ok(Type::Named(
                                        "Result".to_string(),
                                        vec![
                                            Type::named("net.WebSocket"),
                                            crate::builtin_modules::io_error_type(),
                                        ],
                                    ))
                                }
                                BuiltinMember::WebSocketListenerLocalAddr => Ok(Type::Named(
                                    "Result".to_string(),
                                    vec![
                                        Type::named("String"),
                                        crate::builtin_modules::io_error_type(),
                                    ],
                                )),
                                _ => unreachable!("unexpected websocket listener builtin member"),
                            };
                        }
                    }

                    if receiver_name == "net.WebSocket" && receiver_args.is_empty() {
                        if let Some(builtin_member) = BuiltinMember::resolve(receiver_name, field) {
                            let bytes_ty =
                                Type::Named("Vec".to_string(), vec![Type::named("uint8")]);
                            let ordered_args = builtin_member.bind_args(args, span)?;
                            return match builtin_member {
                                BuiltinMember::WebSocketSendText => {
                                    let text_arg = self.bound_argument(
                                        &ordered_args,
                                        0,
                                        span,
                                        "internal error",
                                    )?;
                                    self.check_builtin_argument_type(
                                        text_arg,
                                        &Type::named("String"),
                                        locals,
                                        "send_text",
                                    )?;
                                    self.check_optional_builtin_timeout_argument(
                                        &ordered_args,
                                        1,
                                        locals,
                                        "send_text(timeout=...)",
                                    )?;
                                    Ok(Type::Named(
                                        "Result".to_string(),
                                        vec![Type::Unit, crate::builtin_modules::io_error_type()],
                                    ))
                                }
                                BuiltinMember::WebSocketSendBytes => {
                                    let bytes_arg = self.bound_argument(
                                        &ordered_args,
                                        0,
                                        span,
                                        "internal error",
                                    )?;
                                    self.check_builtin_argument_type(
                                        bytes_arg,
                                        &bytes_ty,
                                        locals,
                                        "send_bytes",
                                    )?;
                                    self.check_optional_builtin_timeout_argument(
                                        &ordered_args,
                                        1,
                                        locals,
                                        "send_bytes(timeout=...)",
                                    )?;
                                    Ok(Type::Named(
                                        "Result".to_string(),
                                        vec![Type::Unit, crate::builtin_modules::io_error_type()],
                                    ))
                                }
                                BuiltinMember::WebSocketRecvText => {
                                    self.check_optional_builtin_timeout_argument(
                                        &ordered_args,
                                        0,
                                        locals,
                                        "recv_text(timeout=...)",
                                    )?;
                                    Ok(Type::Named(
                                        "Result".to_string(),
                                        vec![
                                            Type::Named(
                                                "Option".to_string(),
                                                vec![Type::named("String")],
                                            ),
                                            crate::builtin_modules::io_error_type(),
                                        ],
                                    ))
                                }
                                BuiltinMember::WebSocketRecvBytes => {
                                    self.check_optional_builtin_timeout_argument(
                                        &ordered_args,
                                        0,
                                        locals,
                                        "recv_bytes(timeout=...)",
                                    )?;
                                    Ok(Type::Named(
                                        "Result".to_string(),
                                        vec![
                                            Type::Named("Option".to_string(), vec![bytes_ty]),
                                            crate::builtin_modules::io_error_type(),
                                        ],
                                    ))
                                }
                                BuiltinMember::WebSocketClose => Ok(Type::Unit),
                                _ => unreachable!("unexpected websocket builtin member"),
                            };
                        }
                    }

                    if receiver_name == "net.UnixListener" && receiver_args.is_empty() {
                        if let Some(builtin_member) = BuiltinMember::resolve(receiver_name, field) {
                            let ordered_args = builtin_member.bind_args(args, span)?;
                            return match builtin_member {
                                BuiltinMember::UnixListenerAccept => {
                                    self.check_optional_builtin_timeout_argument(
                                        &ordered_args,
                                        0,
                                        locals,
                                        "accept(timeout=...)",
                                    )?;
                                    Ok(Type::Named(
                                        "Result".to_string(),
                                        vec![
                                            Type::named("net.UnixStream"),
                                            crate::builtin_modules::io_error_type(),
                                        ],
                                    ))
                                }
                                BuiltinMember::UnixListenerClose => Ok(Type::Unit),
                                _ => unreachable!("unexpected unix listener builtin member"),
                            };
                        }
                    }

                    if receiver_name == "net.UnixStream" && receiver_args.is_empty() {
                        if let Some(builtin_member) = BuiltinMember::resolve(receiver_name, field) {
                            let bytes_ty =
                                Type::Named("Vec".to_string(), vec![Type::named("uint8")]);
                            let ordered_args = builtin_member.bind_args(args, span)?;
                            return match builtin_member {
                                BuiltinMember::UnixStreamReadLine => {
                                    self.check_optional_builtin_timeout_argument(
                                        &ordered_args,
                                        0,
                                        locals,
                                        "read_line(timeout=...)",
                                    )?;
                                    Ok(Type::Named(
                                        "Result".to_string(),
                                        vec![
                                            Type::Named(
                                                "Option".to_string(),
                                                vec![Type::named("String")],
                                            ),
                                            crate::builtin_modules::io_error_type(),
                                        ],
                                    ))
                                }
                                BuiltinMember::UnixStreamReadExact => {
                                    let count_arg = self.bound_argument(
                                        &ordered_args,
                                        0,
                                        span,
                                        "internal error",
                                    )?;
                                    self.check_builtin_argument_type(
                                        count_arg,
                                        &Type::named("int32"),
                                        locals,
                                        "read_exact",
                                    )?;
                                    self.check_optional_builtin_timeout_argument(
                                        &ordered_args,
                                        1,
                                        locals,
                                        "read_exact(timeout=...)",
                                    )?;
                                    Ok(Type::Named(
                                        "Result".to_string(),
                                        vec![bytes_ty, crate::builtin_modules::io_error_type()],
                                    ))
                                }
                                BuiltinMember::UnixStreamWriteAll => {
                                    let text_arg = self.bound_argument(
                                        &ordered_args,
                                        0,
                                        span,
                                        "internal error",
                                    )?;
                                    self.check_builtin_argument_type(
                                        text_arg,
                                        &Type::named("String"),
                                        locals,
                                        "write_all",
                                    )?;
                                    self.check_optional_builtin_timeout_argument(
                                        &ordered_args,
                                        1,
                                        locals,
                                        "write_all(timeout=...)",
                                    )?;
                                    Ok(Type::Named(
                                        "Result".to_string(),
                                        vec![Type::Unit, crate::builtin_modules::io_error_type()],
                                    ))
                                }
                                BuiltinMember::UnixStreamClose => Ok(Type::Unit),
                                _ => unreachable!("unexpected unix stream builtin member"),
                            };
                        }
                    }

                    if receiver_name == "net.TlsListener" && receiver_args.is_empty() {
                        if let Some(builtin_member) = BuiltinMember::resolve(receiver_name, field) {
                            let ordered_args = builtin_member.bind_args(args, span)?;
                            return match builtin_member {
                                BuiltinMember::TlsListenerAccept => {
                                    self.check_optional_builtin_timeout_argument(
                                        &ordered_args,
                                        0,
                                        locals,
                                        "accept(timeout=...)",
                                    )?;
                                    Ok(Type::Named(
                                        "Result".to_string(),
                                        vec![
                                            Type::named("net.TlsStream"),
                                            crate::builtin_modules::io_error_type(),
                                        ],
                                    ))
                                }
                                BuiltinMember::TlsListenerLocalAddr => Ok(Type::Named(
                                    "Result".to_string(),
                                    vec![
                                        Type::named("String"),
                                        crate::builtin_modules::io_error_type(),
                                    ],
                                )),
                                BuiltinMember::TlsListenerClose => Ok(Type::Unit),
                                _ => unreachable!("unexpected tls listener builtin member"),
                            };
                        }
                    }

                    if receiver_name == "net.TlsStream" && receiver_args.is_empty() {
                        if let Some(builtin_member) = BuiltinMember::resolve(receiver_name, field) {
                            let bytes_ty =
                                Type::Named("Vec".to_string(), vec![Type::named("uint8")]);
                            let ordered_args = builtin_member.bind_args(args, span)?;
                            return match builtin_member {
                                BuiltinMember::TlsStreamReadLine => {
                                    self.check_optional_builtin_timeout_argument(
                                        &ordered_args,
                                        0,
                                        locals,
                                        "read_line(timeout=...)",
                                    )?;
                                    Ok(Type::Named(
                                        "Result".to_string(),
                                        vec![
                                            Type::Named(
                                                "Option".to_string(),
                                                vec![Type::named("String")],
                                            ),
                                            crate::builtin_modules::io_error_type(),
                                        ],
                                    ))
                                }
                                BuiltinMember::TlsStreamReadExact => {
                                    let count_arg = self.bound_argument(
                                        &ordered_args,
                                        0,
                                        span,
                                        "internal error",
                                    )?;
                                    self.check_builtin_argument_type(
                                        count_arg,
                                        &Type::named("int32"),
                                        locals,
                                        "read_exact",
                                    )?;
                                    self.check_optional_builtin_timeout_argument(
                                        &ordered_args,
                                        1,
                                        locals,
                                        "read_exact(timeout=...)",
                                    )?;
                                    Ok(Type::Named(
                                        "Result".to_string(),
                                        vec![bytes_ty, crate::builtin_modules::io_error_type()],
                                    ))
                                }
                                BuiltinMember::TlsStreamWriteAll => {
                                    let text_arg = self.bound_argument(
                                        &ordered_args,
                                        0,
                                        span,
                                        "internal error",
                                    )?;
                                    self.check_builtin_argument_type(
                                        text_arg,
                                        &Type::named("String"),
                                        locals,
                                        "write_all",
                                    )?;
                                    self.check_optional_builtin_timeout_argument(
                                        &ordered_args,
                                        1,
                                        locals,
                                        "write_all(timeout=...)",
                                    )?;
                                    Ok(Type::Named(
                                        "Result".to_string(),
                                        vec![Type::Unit, crate::builtin_modules::io_error_type()],
                                    ))
                                }
                                BuiltinMember::TlsStreamClose => Ok(Type::Unit),
                                _ => unreachable!("unexpected tls stream builtin member"),
                            };
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
                                &method.signature.param_passings,
                                &method.signature.params,
                                &method.signature.return_type,
                                &method.type_param_bounds,
                                &method.signature.rng_clone_safe_type_params,
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
                        self.enforce_resolved_rng_clone_obligations_before_method_inference(
                            &format!("method `{}`", field),
                            &method.rng_clone_safe_types,
                            &method.decl.type_params,
                            span,
                        )?;
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
                            &method.signature.param_passings,
                            &method.signature.params,
                            &method.signature.return_type,
                            &method.type_param_bounds,
                            &method.signature.rng_clone_safe_type_params,
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
                    self.trait_method_for_concrete_type(&receiver_ty, field, span)?
                {
                    self.enforce_rng_clone_obligations_before_method_inference(
                        &format!("method `{}`", field),
                        &method.signature.rng_clone_safe_type_params,
                        &impl_substitutions,
                        &method.decl.type_params,
                        span,
                    )?;
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
                        &method.signature.param_passings,
                        &substituted_params,
                        &substituted_return_type,
                        &method.type_param_bounds,
                        &method.signature.rng_clone_safe_type_params,
                        args,
                        span,
                        locals,
                        expected,
                        impl_substitutions,
                        receiver_borrows,
                    );
                }
                match (&receiver_ty, field.as_str()) {
                    (Type::Named(_name, type_args), "to_float")
                        if type_args.is_empty() && is_integer_type(&receiver_ty) =>
                    {
                        BuiltinMember::IntegerToFloat.bind_args(args, span)?;
                        Ok(Type::named("float64"))
                    }
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
                    (Type::Named(name, type_args), "append")
                        if name == "Vec" && type_args.len() == 1 =>
                    {
                        Err(Diagnostic::coded_at(
                            "AU2005",
                            span,
                            "Python-style `.append(...)` is not available; use `.push(...)` today",
                        ))
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
            Some("String") => Diagnostic::coded_at(
                "AU2005",
                span,
                "strings use quoted literals; `String(...)` is not a constructor",
            ),
            Some(form @ ("enumerate" | "zip")) => Diagnostic::coded_at(
                "AU2005",
                span,
                format!(
                    "`{form}` is a `for` loop form, not a value; write `for ... in {form}(...):`"
                ),
            ),
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
        let shared_scrutinee =
            self.shared_match_scrutinee_name(&match_stmt.scrutinee, match_stmt.capability);
        let active_match_borrow = if match_stmt.capability == ReceiverKind::BorrowMut {
            self.begin_match_borrow_mut(&match_stmt.scrutinee, match_stmt.span, locals)?
        } else {
            None
        };
        let result = (|| {
            let scrutinee_ty = self.type_of_expr(&match_stmt.scrutinee, locals)?;
            if match_stmt.capability == ReceiverKind::Value && !self.is_copy_type(&scrutinee_ty) {
                self.consume_match_scrutinee_expr(&match_stmt.scrutinee, locals)?;
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
                let mut patterns_by_variant =
                    BTreeMap::<String, Vec<crate::ast::VariantPattern>>::new();
                let mut wildcard_span = None;
                let mut all_return = true;
                let mut arm_states = Vec::new();

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
                        Pattern::Tuple(tuple) => {
                            return Err(Diagnostic::at(
                                tuple.span,
                                format!(
                                    "match over `{}` expects enum variant patterns, not a tuple pattern",
                                    enum_name
                                ),
                            ));
                        }
                        Pattern::Variant(pattern) => {
                            let pattern_enum_name =
                                if let Some(pattern_enum_name) = &pattern.enum_name {
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

                            let covers_entire_variant =
                                self.variant_pattern_covers_payloads(pattern, &variant_payload);
                            if covers_entire_variant {
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
                            }
                            patterns_by_variant
                                .entry(pattern.variant_name.clone())
                                .or_default()
                                .push(pattern.clone());

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
                                match_stmt.capability,
                                active_match_borrow.as_ref(),
                                shared_scrutinee.as_deref(),
                            )?;
                        }
                    }

                    let prior_patterns = match_stmt.arms[..index]
                        .iter()
                        .map(|previous_arm| &previous_arm.pattern)
                        .collect::<Vec<_>>();
                    if self.patterns_cover_pattern(&prior_patterns, &arm.pattern, &scrutinee_ty) {
                        return Err(Diagnostic::at(
                            self.pattern_span(&arm.pattern),
                            "unreachable match arm",
                        ));
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
                        arm_states.push(arm_locals);
                    }
                }

                for (variant_name, payloads) in &variants {
                    if covered.contains_key(variant_name) {
                        continue;
                    }
                    let Some(patterns) = patterns_by_variant.get(variant_name) else {
                        continue;
                    };
                    let pattern_refs = patterns.iter().collect::<Vec<_>>();
                    if self.variant_patterns_cover_payloads_union(&pattern_refs, payloads) {
                        let span = patterns
                            .first()
                            .map(|pattern| pattern.span)
                            .unwrap_or(match_stmt.span);
                        covered.insert(variant_name.clone(), span);
                    }
                }

                let branch_states = arm_states.iter().collect::<Vec<_>>();
                self.merge_control_flow_moves(locals, &branch_states);

                let pattern_refs = match_stmt
                    .arms
                    .iter()
                    .map(|arm| &arm.pattern)
                    .collect::<Vec<_>>();
                let missing = self.missing_patterns_for_type(&pattern_refs, &scrutinee_ty);
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

            if !matches!(scrutinee_ty, Type::Tuple(_) | Type::Named(_, _))
                || !(matches!(scrutinee_ty, Type::Tuple(_))
                    || is_integer_type(&scrutinee_ty)
                    || is_float_type(&scrutinee_ty)
                    || matches!(scrutinee_ty, Type::Named(ref name, ref args) if name == "bool" && args.is_empty())
                    || is_string_type(&scrutinee_ty))
            {
                return Err(Diagnostic::at(
                match_stmt.span,
                format!(
                    "`match` currently requires a tuple, enum, bool, integer, float, or String scrutinee, found `{}`",
                    scrutinee_ty
                ),
            ));
            }

            let mut wildcard_span = None;
            let mut all_return = true;
            let mut covered_literals = BTreeMap::<LiteralPatternKey, crate::diag::Span>::new();
            let mut covered_bools = BTreeSet::<bool>::new();
            let mut arm_states = Vec::new();

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
                    Pattern::Tuple(tuple) => {
                        if !matches!(scrutinee_ty, Type::Tuple(_)) {
                            return Err(Diagnostic::at(
                                tuple.span,
                                format!(
                                    "tuple pattern requires a tuple scrutinee, found `{}`",
                                    scrutinee_ty
                                ),
                            ));
                        }
                        self.bind_pattern_locals(
                            &arm.pattern,
                            &scrutinee_ty,
                            &mut arm_locals,
                            match_stmt.capability,
                            active_match_borrow.as_ref(),
                            shared_scrutinee.as_deref(),
                        )?;
                    }
                }

                let prior_patterns = match_stmt.arms[..index]
                    .iter()
                    .map(|previous_arm| &previous_arm.pattern)
                    .collect::<Vec<_>>();
                if self.patterns_cover_pattern(&prior_patterns, &arm.pattern, &scrutinee_ty) {
                    return Err(Diagnostic::at(
                        self.pattern_span(&arm.pattern),
                        "unreachable match arm",
                    ));
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
                    arm_states.push(arm_locals);
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
                } else if matches!(scrutinee_ty, Type::Tuple(_)) {
                    let patterns = match_stmt
                        .arms
                        .iter()
                        .map(|arm| &arm.pattern)
                        .collect::<Vec<_>>();
                    if !self
                        .missing_patterns_for_type(&patterns, &scrutinee_ty)
                        .is_empty()
                    {
                        return Err(Diagnostic::at(
                            match_stmt.span,
                            format!(
                                "non-exhaustive match over `{}`: add a covering tuple pattern or final `case _:`",
                                scrutinee_ty
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

            let branch_states = arm_states.iter().collect::<Vec<_>>();
            self.merge_control_flow_moves(locals, &branch_states);

            if all_return {
                Ok(BlockFlow::AlwaysReturns)
            } else {
                Ok(BlockFlow::FallsThrough)
            }
        })();
        self.end_match_borrow_mut(active_match_borrow);
        result
    }

    /// The scrutinee spelling to quote in a `match own <place>` suggestion.
    ///
    /// Only a bare (shared) match over a named place can be respelled, so a
    /// temporary scrutinee or an already-explicit capability yields `None`.
    fn shared_match_scrutinee_name(&self, expr: &Expr, capability: ReceiverKind) -> Option<String> {
        if capability != ReceiverKind::Borrow {
            return None;
        }
        matches!(
            expr.kind,
            ExprKind::Name(_) | ExprKind::Member { .. } | ExprKind::Index { .. }
        )
        .then(|| self.render_place_expr(expr))
    }

    fn bind_pattern_locals(
        &self,
        pattern: &Pattern,
        expected_ty: &Type,
        locals: &mut HashMap<String, LocalBinding>,
        borrow_mode: ReceiverKind,
        match_borrow_mut_place: Option<&PlacePath>,
        shared_match_scrutinee: Option<&str>,
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
                let passing = if self.is_copy_type(expected_ty) {
                    ReceiverKind::Value
                } else {
                    borrow_mode
                };
                locals.insert(
                    binding.name.clone(),
                    LocalBinding {
                        ty: expected_ty.clone(),
                        assignable: borrow_mode == ReceiverKind::BorrowMut,
                        mutable_place: borrow_mode == ReceiverKind::BorrowMut,
                        managed_resource: false,
                        passing,
                        borrow_origin: None,
                        borrowed_at: (borrow_mode != ReceiverKind::Value).then_some(binding.span),
                        match_borrow_mut_place: match_borrow_mut_place.cloned(),
                        stale_match_borrow_mut_place: None,
                        shared_match_scrutinee: (passing == ReceiverKind::Borrow)
                            .then(|| shared_match_scrutinee.map(str::to_string))
                            .flatten(),
                        moved: false,
                        moved_at: None,
                        moved_fields: BTreeMap::new(),
                        frozen_places: BTreeMap::new(),
                    },
                );
                Ok(())
            }
            Pattern::Tuple(tuple_pattern) => {
                if borrow_mode == ReceiverKind::BorrowMut {
                    return Err(Diagnostic::coded_at(
                        "AU3002",
                        tuple_pattern.span,
                        "`match mut` does not support tuple patterns; bind the tuple as one mutable name",
                    ));
                }
                let Type::Tuple(element_types) = expected_ty else {
                    return Err(Diagnostic::at(
                        tuple_pattern.span,
                        format!(
                            "tuple pattern requires a tuple scrutinee, found `{}`",
                            expected_ty
                        ),
                    ));
                };
                if tuple_pattern.elements.len() != element_types.len() {
                    return Err(Diagnostic::at(
                        tuple_pattern.span,
                        format!(
                            "tuple pattern has {} elements but the scrutinee has {}",
                            tuple_pattern.elements.len(),
                            element_types.len()
                        ),
                    ));
                }
                for (element, element_ty) in tuple_pattern.elements.iter().zip(element_types) {
                    self.bind_pattern_locals(
                        element,
                        element_ty,
                        locals,
                        borrow_mode,
                        match_borrow_mut_place,
                        shared_match_scrutinee,
                    )?;
                }
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
                    self.bind_pattern_locals(
                        subpattern,
                        payload_ty,
                        locals,
                        borrow_mode,
                        match_borrow_mut_place,
                        shared_match_scrutinee,
                    )?;
                }
                Ok(())
            }
        }
    }

    /// Types one `match` arm value the way the surrounding expression uses the
    /// match result.
    fn type_of_match_arm_value(
        &self,
        value: &Expr,
        arm_locals: &mut HashMap<String, LocalBinding>,
        result_ty: Option<&Type>,
        result_use: BranchResultUse<'_>,
    ) -> Result<Type> {
        match result_use {
            BranchResultUse::ProjectedField(field) => Ok(self
                .type_member_result_consuming(value, field.name, field.span, arm_locals, result_ty)?
                .0),
            BranchResultUse::Consumed => {
                self.type_expr_consuming_result(value, arm_locals, result_ty)
            }
            BranchResultUse::Inspected => match result_ty {
                Some(expected_ty) => self.type_of_expr_hint(value, arm_locals, Some(expected_ty)),
                None => self.type_of_expr(value, arm_locals),
            },
        }
    }

    fn type_of_match_expr(
        &self,
        parts: MatchExprParts<'_>,
        locals: &mut HashMap<String, LocalBinding>,
        expected: Option<&Type>,
        result_use: BranchResultUse<'_>,
    ) -> Result<Type> {
        let MatchExprParts {
            scrutinee,
            borrow_mode,
            arms,
            span,
        } = parts;
        let shared_scrutinee = self.shared_match_scrutinee_name(scrutinee, borrow_mode);
        let active_match_borrow = if borrow_mode == ReceiverKind::BorrowMut {
            self.begin_match_borrow_mut(scrutinee, span, locals)?
        } else {
            None
        };
        let result = (|| {
            let scrutinee_ty = self.type_of_expr(scrutinee, locals)?;
            if borrow_mode == ReceiverKind::Value && !self.is_copy_type(&scrutinee_ty) {
                self.consume_match_scrutinee_expr(scrutinee, locals)?;
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
                let mut patterns_by_variant =
                    BTreeMap::<String, Vec<crate::ast::VariantPattern>>::new();
                let mut wildcard_seen = false;
                let mut arm_states = Vec::new();

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
                        Pattern::Tuple(tuple) => {
                            return Err(Diagnostic::at(
                                tuple.span,
                                format!(
                                    "match over `{}` expects enum variant patterns, not a tuple pattern",
                                    enum_name
                                ),
                            ));
                        }
                        Pattern::Variant(pattern) => {
                            let pattern_enum_name =
                                if let Some(pattern_enum_name) = &pattern.enum_name {
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
                            if self.variant_pattern_covers_payloads(pattern, &variant_payload) {
                                covered.insert(pattern.variant_name.clone());
                            }
                            patterns_by_variant
                                .entry(pattern.variant_name.clone())
                                .or_default()
                                .push(pattern.clone());

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
                                active_match_borrow.as_ref(),
                                shared_scrutinee.as_deref(),
                            )?;
                        }
                    }

                    let prior_patterns = arms[..index]
                        .iter()
                        .map(|previous_arm| &previous_arm.pattern)
                        .collect::<Vec<_>>();
                    if self.patterns_cover_pattern(&prior_patterns, &arm.pattern, &scrutinee_ty) {
                        return Err(Diagnostic::at(
                            self.pattern_span(&arm.pattern),
                            "unreachable match arm",
                        ));
                    }

                    let arm_ty = self.type_of_match_arm_value(
                        &arm.value,
                        &mut arm_locals,
                        result_ty.as_ref(),
                        result_use,
                    )?;
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
                    arm_states.push(arm_locals);
                }

                for (variant_name, payloads) in &variants {
                    if covered.contains(variant_name) {
                        continue;
                    }
                    let Some(patterns) = patterns_by_variant.get(variant_name) else {
                        continue;
                    };
                    let pattern_refs = patterns.iter().collect::<Vec<_>>();
                    if self.variant_patterns_cover_payloads_union(&pattern_refs, payloads) {
                        covered.insert(variant_name.clone());
                    }
                }

                let branch_states = arm_states.iter().collect::<Vec<_>>();
                self.merge_control_flow_moves(locals, &branch_states);

                let pattern_refs = arms.iter().map(|arm| &arm.pattern).collect::<Vec<_>>();
                let missing = self.missing_patterns_for_type(&pattern_refs, &scrutinee_ty);
                if !wildcard_seen && !missing.is_empty() {
                    return Err(Diagnostic::at(
                        span,
                        format!(
                            "non-exhaustive match over `{}`: missing {}",
                            enum_name,
                            missing
                                .iter()
                                .map(|name| format!("`{}`", name))
                                .collect::<Vec<_>>()
                                .join(", ")
                        ),
                    ));
                }

                return Ok(result_ty.unwrap_or(Type::Unit));
            }

            if !matches!(scrutinee_ty, Type::Tuple(_) | Type::Named(_, _))
                || !(matches!(scrutinee_ty, Type::Tuple(_))
                    || is_integer_type(&scrutinee_ty)
                    || is_float_type(&scrutinee_ty)
                    || matches!(scrutinee_ty, Type::Named(ref name, ref args) if name == "bool" && args.is_empty())
                    || is_string_type(&scrutinee_ty))
            {
                return Err(Diagnostic::at(
                span,
                format!(
                    "`match` currently requires a tuple, enum, bool, integer, float, or String scrutinee, found `{}`",
                    scrutinee_ty
                ),
            ));
            }

            let mut wildcard_seen = false;
            let mut covered_literals = BTreeSet::<LiteralPatternKey>::new();
            let mut covered_bools = BTreeSet::<bool>::new();
            let mut arm_states = Vec::new();

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
                    Pattern::Tuple(tuple) => {
                        if !matches!(scrutinee_ty, Type::Tuple(_)) {
                            return Err(Diagnostic::at(
                                tuple.span,
                                format!(
                                    "tuple pattern requires a tuple scrutinee, found `{}`",
                                    scrutinee_ty
                                ),
                            ));
                        }
                        self.bind_pattern_locals(
                            &arm.pattern,
                            &scrutinee_ty,
                            &mut arm_locals,
                            borrow_mode,
                            active_match_borrow.as_ref(),
                            shared_scrutinee.as_deref(),
                        )?;
                    }
                }

                let prior_patterns = arms[..index]
                    .iter()
                    .map(|previous_arm| &previous_arm.pattern)
                    .collect::<Vec<_>>();
                if self.patterns_cover_pattern(&prior_patterns, &arm.pattern, &scrutinee_ty) {
                    return Err(Diagnostic::at(
                        self.pattern_span(&arm.pattern),
                        "unreachable match arm",
                    ));
                }

                let arm_ty = self.type_of_match_arm_value(
                    &arm.value,
                    &mut arm_locals,
                    result_ty.as_ref(),
                    result_use,
                )?;
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
                arm_states.push(arm_locals);
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
            if !wildcard_seen && matches!(scrutinee_ty, Type::Tuple(_)) {
                let patterns = arms.iter().map(|arm| &arm.pattern).collect::<Vec<_>>();
                if !self
                    .missing_patterns_for_type(&patterns, &scrutinee_ty)
                    .is_empty()
                {
                    return Err(Diagnostic::at(
                        span,
                        format!(
                            "non-exhaustive match over `{}`: add a covering tuple pattern or final `case _:`",
                            scrutinee_ty
                        ),
                    ));
                }
            }

            let branch_states = arm_states.iter().collect::<Vec<_>>();
            self.merge_control_flow_moves(locals, &branch_states);

            Ok(result_ty.unwrap_or(Type::Unit))
        })();
        self.end_match_borrow_mut(active_match_borrow);
        result
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
                if let Some(enum_info) = namespace.enums.get(field) {
                    return Ok(Type::Named(
                        self.module_enum_type_name(path, enum_info),
                        Vec::new(),
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
            Type::Tuple(_) | Type::Unit => {
                return Err(Diagnostic::at(
                    span,
                    format!("cannot access field `{}` on `{}`", field, object_ty),
                ));
            }
        };

        if let Some(variant_payloads) = self.builtin_enum_variant_payload(object_ty, name, field) {
            return match variant_payloads.is_empty() {
                false => Err(Diagnostic::at(
                    span,
                    format!("variant `{}` of enum `{}` requires a payload", field, name),
                )),
                true => Ok(object_ty.clone()),
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
            self.trait_method_for_concrete_type(object_ty, field, span)?
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

    fn borrow_call_place(&self, expr: &Expr) -> Option<PlacePath> {
        match &expr.kind {
            ExprKind::Name(name) => Some(PlacePath::root(name.clone())),
            ExprKind::Group(inner) => self.borrow_call_place(inner),
            ExprKind::Member { object, field } => {
                let parent = self.borrow_call_place(object)?;
                Some(parent.with_field(field.clone()))
            }
            _ => None,
        }
    }

    fn builtin_payload_free_variant(enum_name: &str, variant_name: &str) -> bool {
        matches!(
            (enum_name, variant_name),
            ("Option", "None")
                | ("QueueReceive", "Closed" | "TimedOut" | "Cancelled")
                | ("TaskResult", "TimedOut" | "Cancelled")
                | ("WaitAny", "TimedOut" | "Cancelled")
                | ("WaitAll", "TimedOut" | "Cancelled")
        )
    }

    fn is_payload_free_variant_expr(&self, expr: &Expr) -> bool {
        match &expr.kind {
            ExprKind::Group(inner) | ExprKind::Specialize { expr: inner, .. } => {
                self.is_payload_free_variant_expr(inner)
            }
            ExprKind::Member { object, field } => {
                let base_object = match &object.kind {
                    ExprKind::Specialize { expr, .. } => &**expr,
                    _ => &**object,
                };
                if let ExprKind::Name(enum_name) = &base_object.kind {
                    if Self::builtin_payload_free_variant(enum_name, field) {
                        return true;
                    }
                    if let Some(enum_info) = self.resolve_enum_info(enum_name) {
                        return enum_info
                            .variants
                            .get(field)
                            .is_some_and(|variant| variant.payloads.is_empty());
                    }
                }
                if let Some((module_path, enum_name)) = self.qualified_module_item(object) {
                    if let Some(namespace) = self.module_namespace(&module_path) {
                        return namespace
                            .enums
                            .get(&enum_name)
                            .and_then(|enum_info| enum_info.variants.get(field))
                            .is_some_and(|variant| variant.payloads.is_empty());
                    }
                }
                false
            }
            _ => false,
        }
    }

    fn is_enum_constructor_object(&self, expr: &Expr) -> bool {
        match &expr.kind {
            ExprKind::Group(inner) | ExprKind::Specialize { expr: inner, .. } => {
                self.is_enum_constructor_object(inner)
            }
            ExprKind::Name(name) => {
                matches!(
                    name.as_str(),
                    "Option"
                        | "Result"
                        | "SendError"
                        | "QueueReceive"
                        | "TaskResult"
                        | "WaitAny"
                        | "WaitAll"
                ) || self.resolve_enum_info(name).is_some()
            }
            _ => self
                .qualified_module_item(expr)
                .and_then(|(module_path, enum_name)| {
                    self.module_namespace(&module_path)
                        .and_then(|namespace| namespace.enums.get(&enum_name))
                })
                .is_some(),
        }
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
                    passing: binding.passing,
                })
            })),
            ExprKind::Group(inner)
            | ExprKind::Cast { expr: inner, .. }
            | ExprKind::Specialize { expr: inner, .. } => self.expr_borrow_info(inner, locals),
            ExprKind::Member { object, .. } | ExprKind::Index { object, .. } => {
                if self.is_payload_free_variant_expr(expr) {
                    return Ok(None);
                }
                let value_ty = self.type_of_expr(expr, locals)?;
                if self.is_copy_type(&value_ty) {
                    Ok(None)
                } else {
                    self.expr_borrow_info(object, locals)
                }
            }
            // A call, match, or conditional now always produces an owned
            // value: ADR-0022 removed borrowed returns, so no loan can
            // propagate out through one of these.
            _ => Ok(None),
        }
    }

    fn collect_expr_borrowed_places(
        &self,
        expr: &Expr,
        locals: &HashMap<String, LocalBinding>,
        places: &mut Vec<BorrowedCallPlace>,
    ) -> Result<()> {
        self.collect_expr_call_places(expr, locals, places, false)
    }

    fn collect_expr_consumed_places(
        &self,
        expr: &Expr,
        locals: &HashMap<String, LocalBinding>,
        places: &mut Vec<BorrowedCallPlace>,
    ) -> Result<()> {
        let mut call_places = Vec::new();
        self.collect_expr_call_places(expr, locals, &mut call_places, true)?;
        places.extend(
            call_places
                .into_iter()
                .filter(|place| place.passing == ReceiverKind::Value),
        );
        Ok(())
    }

    fn collect_result_place_accesses(
        &self,
        expr: &Expr,
        locals: &HashMap<String, LocalBinding>,
        passing: ReceiverKind,
        label: &str,
        places: &mut Vec<BorrowedCallPlace>,
    ) -> Result<()> {
        match &expr.kind {
            ExprKind::Name(_) => {
                let Some(path) = self.borrow_call_place(expr) else {
                    return Ok(());
                };
                let Some(ty) = self.place_path_type(&path, locals, expr.span)? else {
                    return Ok(());
                };
                if Self::result_place_access_is_retained(passing, self.is_copy_type(&ty)) {
                    places.push(BorrowedCallPlace {
                        path,
                        passing,
                        param_name: label.to_string(),
                        origin_span: expr.span,
                    });
                }
                Ok(())
            }
            ExprKind::Member { object, field } => self.collect_projected_member_result_accesses(
                object,
                ProjectedField {
                    name: field,
                    span: expr.span,
                },
                locals,
                passing,
                label,
                places,
            ),
            ExprKind::Group(inner)
            | ExprKind::Cast { expr: inner, .. }
            | ExprKind::Specialize { expr: inner, .. }
            | ExprKind::Try(inner) => {
                self.collect_result_place_accesses(inner, locals, passing, label, places)
            }
            ExprKind::Tuple(elements) | ExprKind::List(elements) | ExprKind::Set(elements) => {
                for element in elements {
                    self.collect_result_place_accesses(element, locals, passing, label, places)?;
                }
                Ok(())
            }
            ExprKind::Map(entries) => {
                for entry in entries {
                    self.collect_result_place_accesses(&entry.key, locals, passing, label, places)?;
                    self.collect_result_place_accesses(
                        &entry.value,
                        locals,
                        passing,
                        label,
                        places,
                    )?;
                }
                Ok(())
            }
            ExprKind::Conditional {
                then_expr,
                else_expr,
                ..
            } => {
                self.collect_result_place_accesses(then_expr, locals, passing, label, places)?;
                self.collect_result_place_accesses(else_expr, locals, passing, label, places)
            }
            ExprKind::Match { arms, .. } => {
                for arm in arms {
                    self.collect_result_place_accesses(&arm.value, locals, passing, label, places)?;
                }
                Ok(())
            }
            _ => Ok(()),
        }
    }

    /// A copy-typed argument passed by value leaves no retained access, but a
    /// copy-typed place passed as `borrow` or `borrow mut` still aliases the
    /// place for the rest of the call.
    fn result_place_access_is_retained(passing: ReceiverKind, is_copy: bool) -> bool {
        !(is_copy && passing == ReceiverKind::Value)
    }

    fn collect_projected_member_result_accesses(
        &self,
        object: &Expr,
        field: ProjectedField<'_>,
        locals: &HashMap<String, LocalBinding>,
        passing: ReceiverKind,
        label: &str,
        places: &mut Vec<BorrowedCallPlace>,
    ) -> Result<()> {
        match &object.kind {
            ExprKind::Group(inner) => self.collect_projected_member_result_accesses(
                inner, field, locals, passing, label, places,
            ),
            ExprKind::Conditional {
                then_expr,
                else_expr,
                ..
            } => {
                self.collect_projected_member_result_accesses(
                    then_expr, field, locals, passing, label, places,
                )?;
                self.collect_projected_member_result_accesses(
                    else_expr, field, locals, passing, label, places,
                )
            }
            ExprKind::Match { arms, .. } => {
                for arm in arms {
                    self.collect_projected_member_result_accesses(
                        &arm.value, field, locals, passing, label, places,
                    )?;
                }
                Ok(())
            }
            _ => {
                let member_expr = Expr {
                    kind: ExprKind::Member {
                        object: Box::new(object.clone()),
                        field: field.name.to_string(),
                    },
                    span: field.span,
                };
                let Some(path) = self.borrow_call_place(&member_expr) else {
                    return Ok(());
                };
                let Some(ty) = self.place_path_type(&path, locals, field.span)? else {
                    return Ok(());
                };
                if Self::result_place_access_is_retained(passing, self.is_copy_type(&ty)) {
                    places.push(BorrowedCallPlace {
                        path,
                        passing,
                        param_name: label.to_string(),
                        origin_span: field.span,
                    });
                }
                Ok(())
            }
        }
    }

    fn collect_expr_call_places(
        &self,
        expr: &Expr,
        locals: &HashMap<String, LocalBinding>,
        places: &mut Vec<BorrowedCallPlace>,
        include_consumed: bool,
    ) -> Result<()> {
        match &expr.kind {
            ExprKind::Group(inner)
            | ExprKind::Cast { expr: inner, .. }
            | ExprKind::Specialize { expr: inner, .. }
            | ExprKind::Try(inner) => {
                self.collect_expr_call_places(inner, locals, places, include_consumed)
            }
            ExprKind::Unary { expr: inner, .. } => {
                self.collect_expr_call_places(inner, locals, places, include_consumed)
            }
            ExprKind::Binary { left, right, .. } => {
                self.collect_expr_call_places(left, locals, places, include_consumed)?;
                self.collect_expr_call_places(right, locals, places, include_consumed)
            }
            ExprKind::Conditional {
                then_expr,
                condition,
                else_expr,
            } => {
                self.collect_expr_call_places(condition, locals, places, include_consumed)?;
                self.collect_expr_call_places(then_expr, locals, places, include_consumed)?;
                self.collect_expr_call_places(else_expr, locals, places, include_consumed)
            }
            ExprKind::Call { callee, args } => {
                self.collect_expr_call_places(callee, locals, places, include_consumed)?;
                for argument in args {
                    self.collect_expr_call_places(
                        &argument.value,
                        locals,
                        places,
                        include_consumed,
                    )?;
                }
                self.collect_call_borrowed_places(callee, args, locals, places, include_consumed)
            }
            ExprKind::Tuple(elements) | ExprKind::List(elements) | ExprKind::Set(elements) => {
                for element in elements {
                    self.collect_expr_call_places(element, locals, places, include_consumed)?;
                }
                Ok(())
            }
            ExprKind::Map(entries) => {
                for entry in entries {
                    self.collect_expr_call_places(&entry.key, locals, places, include_consumed)?;
                    self.collect_expr_call_places(&entry.value, locals, places, include_consumed)?;
                }
                Ok(())
            }
            ExprKind::FString(parts) => {
                for part in parts {
                    if let crate::ast::FormatPart::Expr(value) = part {
                        self.collect_expr_call_places(value, locals, places, include_consumed)?;
                    }
                }
                Ok(())
            }
            ExprKind::Member { object, .. } => {
                self.collect_expr_call_places(object, locals, places, include_consumed)
            }
            ExprKind::Index { object, index } => {
                self.collect_expr_call_places(object, locals, places, include_consumed)?;
                self.collect_expr_call_places(index, locals, places, include_consumed)
            }
            ExprKind::Match {
                scrutinee, arms, ..
            } => {
                self.collect_expr_call_places(scrutinee, locals, places, include_consumed)?;
                for arm in arms {
                    self.collect_expr_call_places(&arm.value, locals, places, include_consumed)?;
                }
                Ok(())
            }
            _ => Ok(()),
        }
    }

    fn collect_expr_place_reads(
        &self,
        expr: &Expr,
        locals: &HashMap<String, LocalBinding>,
        label: &str,
        places: &mut Vec<BorrowedCallPlace>,
    ) {
        let mut push_place = |path: PlacePath, span: crate::diag::Span| {
            if locals.contains_key(&path.root) {
                places.push(BorrowedCallPlace {
                    path,
                    passing: ReceiverKind::Borrow,
                    param_name: label.to_string(),
                    origin_span: span,
                });
            }
        };
        match &expr.kind {
            ExprKind::Name(name) => push_place(PlacePath::root(name.clone()), expr.span),
            ExprKind::Group(inner)
            | ExprKind::Cast { expr: inner, .. }
            | ExprKind::Specialize { expr: inner, .. }
            | ExprKind::Try(inner)
            | ExprKind::Unary { expr: inner, .. } => {
                self.collect_expr_place_reads(inner, locals, label, places)
            }
            ExprKind::Binary { left, right, .. } => {
                self.collect_expr_place_reads(left, locals, label, places);
                self.collect_expr_place_reads(right, locals, label, places);
            }
            ExprKind::Membership {
                value, container, ..
            } => {
                self.collect_expr_place_reads(value, locals, label, places);
                self.collect_expr_place_reads(container, locals, label, places);
            }
            ExprKind::CompareChain { first, links } => {
                self.collect_expr_place_reads(first, locals, label, places);
                for link in links {
                    self.collect_expr_place_reads(&link.operand, locals, label, places);
                }
            }
            ExprKind::Conditional {
                then_expr,
                condition,
                else_expr,
            } => {
                self.collect_expr_place_reads(condition, locals, label, places);
                self.collect_expr_place_reads(then_expr, locals, label, places);
                self.collect_expr_place_reads(else_expr, locals, label, places);
            }
            ExprKind::Call { callee, args } => {
                self.collect_expr_place_reads(callee, locals, label, places);
                for argument in args {
                    self.collect_expr_place_reads(&argument.value, locals, label, places);
                }
            }
            ExprKind::Tuple(elements) | ExprKind::List(elements) | ExprKind::Set(elements) => {
                for element in elements {
                    self.collect_expr_place_reads(element, locals, label, places);
                }
            }
            ExprKind::Map(entries) => {
                for entry in entries {
                    self.collect_expr_place_reads(&entry.key, locals, label, places);
                    self.collect_expr_place_reads(&entry.value, locals, label, places);
                }
            }
            ExprKind::FString(parts) => {
                for part in parts {
                    if let crate::ast::FormatPart::Expr(value) = part {
                        self.collect_expr_place_reads(value, locals, label, places);
                    }
                }
            }
            ExprKind::Member { object, .. } => {
                if let Some(path) = self.borrow_call_place(expr) {
                    push_place(path, expr.span);
                } else {
                    self.collect_expr_place_reads(object, locals, label, places);
                }
            }
            ExprKind::Index { object, index } => {
                if let Some(path) = self.borrow_call_place(object) {
                    push_place(path, object.span);
                } else {
                    self.collect_expr_place_reads(object, locals, label, places);
                }
                self.collect_expr_place_reads(index, locals, label, places);
            }
            ExprKind::Match {
                scrutinee, arms, ..
            } => {
                self.collect_expr_place_reads(scrutinee, locals, label, places);
                for arm in arms {
                    self.collect_expr_place_reads(&arm.value, locals, label, places);
                }
            }
            ExprKind::Int(_)
            | ExprKind::DurationNanos(_)
            | ExprKind::BuiltinOmitted
            | ExprKind::Float(_)
            | ExprKind::Bool(_)
            | ExprKind::String(_) => {}
        }
    }

    fn collect_call_borrowed_places(
        &self,
        callee: &Expr,
        args: &[Argument],
        locals: &HashMap<String, LocalBinding>,
        places: &mut Vec<BorrowedCallPlace>,
        include_consumed: bool,
    ) -> Result<()> {
        let mut locals_for_resolution = locals.clone();
        let (base_callee, _) = self.peel_specialization(callee);
        match &base_callee.kind {
            ExprKind::Name(name) => {
                let Some(function) = self.resolve_function_info(name) else {
                    return Ok(());
                };
                let ordered_args = bind_call_arguments(
                    &format!("function `{}`", function.decl.name),
                    &callable_params_from_decl(&function.decl.params),
                    args,
                    callee.span,
                    CallConvention::PositionalOrNamed,
                )?;
                for ((argument, param), passing) in ordered_args
                    .into_iter()
                    .zip(function.decl.params.iter())
                    .zip(function.signature.param_passings.iter().copied())
                {
                    let Some(argument) = argument else {
                        continue;
                    };
                    if let Some(path) = self.borrow_call_place(&argument.value) {
                        if passing == ReceiverKind::Value
                            && (!include_consumed
                                || self
                                    .place_path_type(&path, locals, argument.value.span)?
                                    .is_none_or(|ty| self.is_copy_type(&ty)))
                        {
                            continue;
                        }
                        places.push(BorrowedCallPlace {
                            path,
                            passing,
                            param_name: param.name.clone(),
                            origin_span: argument.value.span,
                        });
                    }
                }
                Ok(())
            }
            ExprKind::Member { object, field } => {
                if let ExprKind::Name(type_name) = &object.kind {
                    if !locals.contains_key(type_name) {
                        if let Some(associated) =
                            BuiltinAssociatedFunction::resolve(type_name, field)
                        {
                            let ordered_args = associated.bind_args(args, callee.span)?;
                            for (index, argument) in ordered_args.into_iter().enumerate() {
                                let Some(argument) = argument else {
                                    continue;
                                };
                                let Some(passing) = associated.argument_passing(index) else {
                                    continue;
                                };
                                if let Some(path) = self.borrow_call_place(&argument.value) {
                                    if passing == ReceiverKind::Value
                                        && (!include_consumed
                                            || self
                                                .place_path_type(
                                                    &path,
                                                    locals,
                                                    argument.value.span,
                                                )?
                                                .is_none_or(|ty| self.is_copy_type(&ty)))
                                    {
                                        continue;
                                    }
                                    places.push(BorrowedCallPlace {
                                        path,
                                        passing,
                                        param_name: associated
                                            .argument_name(index)
                                            .unwrap_or("argument")
                                            .to_string(),
                                        origin_span: argument.value.span,
                                    });
                                }
                            }
                            return Ok(());
                        }
                    }
                }
                if self.is_enum_constructor_object(object) {
                    return Ok(());
                }
                if let Some((module_path, item_name)) = self.qualified_module_item(object) {
                    if let Some(namespace) = self.module_namespace(&module_path) {
                        if let Some(class_info) = namespace.classes.get(&item_name) {
                            if let Some(method) = class_info.methods.get(field) {
                                self.collect_method_borrowed_places(
                                    object,
                                    (callee, args),
                                    method,
                                    locals,
                                    places,
                                    include_consumed,
                                )?;
                                return Ok(());
                            }
                        }
                    }
                }
                let receiver_ty = self.type_of_expr(object, &mut locals_for_resolution)?;
                let Type::Named(receiver_name, _) = receiver_ty else {
                    return Ok(());
                };
                let Some(class_info) = self.resolve_class_info(&receiver_name) else {
                    return Ok(());
                };
                let Some(method) = class_info.methods.get(field) else {
                    return Ok(());
                };
                self.collect_method_borrowed_places(
                    object,
                    (callee, args),
                    method,
                    locals,
                    places,
                    include_consumed,
                )
            }
            _ => Ok(()),
        }
    }

    fn collect_method_borrowed_places(
        &self,
        object: &Expr,
        call: (&Expr, &[Argument]),
        method: &MethodInfo,
        locals: &HashMap<String, LocalBinding>,
        places: &mut Vec<BorrowedCallPlace>,
        include_consumed: bool,
    ) -> Result<()> {
        let (callee, args) = call;
        if let Some(receiver_passing) = method.decl.receiver {
            if let Some(path) = self.borrow_call_place(object) {
                if receiver_passing != ReceiverKind::Value
                    || (include_consumed
                        && self
                            .place_path_type(&path, locals, object.span)?
                            .is_some_and(|ty| !self.is_copy_type(&ty)))
                {
                    places.push(BorrowedCallPlace {
                        path,
                        passing: receiver_passing,
                        param_name: "self".to_string(),
                        origin_span: object.span,
                    });
                }
            }
        }
        let ordered_args = bind_call_arguments(
            "method call",
            &callable_params_from_decl(&method.decl.params),
            args,
            callee.span,
            CallConvention::PositionalOrNamed,
        )?;
        for ((argument, param), passing) in ordered_args
            .into_iter()
            .zip(method.decl.params.iter())
            .zip(method.signature.param_passings.iter().copied())
        {
            let Some(argument) = argument else {
                continue;
            };
            if let Some(path) = self.borrow_call_place(&argument.value) {
                if passing == ReceiverKind::Value
                    && (!include_consumed
                        || self
                            .place_path_type(&path, locals, argument.value.span)?
                            .is_none_or(|ty| self.is_copy_type(&ty)))
                {
                    continue;
                }
                places.push(BorrowedCallPlace {
                    path,
                    passing,
                    param_name: param.name.clone(),
                    origin_span: argument.value.span,
                });
            }
        }
        Ok(())
    }

    fn place_path_type(
        &self,
        path: &PlacePath,
        locals: &HashMap<String, LocalBinding>,
        span: crate::diag::Span,
    ) -> Result<Option<Type>> {
        let Some(binding) = locals.get(&path.root) else {
            return Ok(None);
        };
        // A module-rooted path is a namespace or enum-variant path such as
        // `json.Value.Null`, not an owned place, so it never participates in
        // borrow or move tracking.
        if matches!(binding.ty, Type::Module(_)) {
            return Ok(None);
        }
        let mut ty = binding.ty.clone();
        for projection in &path.projections.0 {
            match projection {
                PlaceProjection::Field(field) => {
                    ty = self.resolve_member_type(&ty, field, span)?;
                }
            }
        }
        Ok(Some(ty))
    }

    fn retained_place_access(
        &self,
        expr: &Expr,
        ty: &Type,
        passing: ReceiverKind,
        label: &str,
    ) -> Option<BorrowedCallPlace> {
        if self.is_copy_type(ty) {
            return None;
        }
        self.borrow_call_place(expr).map(|path| BorrowedCallPlace {
            path,
            passing,
            param_name: label.to_string(),
            origin_span: expr.span,
        })
    }

    fn retained_call_place_access(
        &self,
        expr: &Expr,
        ty: &Type,
        passing: ReceiverKind,
        label: &str,
    ) -> Option<BorrowedCallPlace> {
        if passing == ReceiverKind::Value && self.is_copy_type(ty) {
            return None;
        }
        self.borrow_call_place(expr).map(|path| BorrowedCallPlace {
            path,
            passing,
            param_name: label.to_string(),
            origin_span: expr.span,
        })
    }

    fn retained_path_access(
        &self,
        path: PlacePath,
        ty: &Type,
        passing: ReceiverKind,
        label: &str,
        origin_span: crate::diag::Span,
    ) -> Option<BorrowedCallPlace> {
        if passing == ReceiverKind::Value && self.is_copy_type(ty) {
            return None;
        }
        Some(BorrowedCallPlace {
            path,
            passing,
            param_name: label.to_string(),
            origin_span,
        })
    }

    fn reject_builtin_receiver_argument_overlap(
        &self,
        builtin_member: BuiltinMember,
        object: &Expr,
        receiver_ty: &Type,
        args: &[Argument],
        locals: &HashMap<String, LocalBinding>,
    ) -> Result<()> {
        let retained_receiver = self
            .retained_place_access(
                object,
                receiver_ty,
                builtin_member.receiver_passing(),
                "method receiver",
            )
            .into_iter()
            .collect::<Vec<_>>();
        if retained_receiver.is_empty() {
            return Ok(());
        }
        let mut argument_accesses = Vec::new();
        for argument in args {
            self.collect_expr_borrowed_places(&argument.value, locals, &mut argument_accesses)?;
            self.collect_expr_consumed_places(&argument.value, locals, &mut argument_accesses)?;
        }
        if builtin_member.variadic_argument_passing().is_none() {
            let ordered_args = builtin_member.bind_args(args, object.span)?;
            for (index, argument) in ordered_args.into_iter().enumerate() {
                let Some(argument) = argument else {
                    continue;
                };
                if builtin_member.argument_passing(index) != Some(ReceiverKind::Value) {
                    continue;
                }
                let Some(path) = self.borrow_call_place(&argument.value) else {
                    continue;
                };
                if !locals.contains_key(&path.root) {
                    continue;
                }
                let argument_ty =
                    self.type_of_expr_without_move_state(&argument.value, locals, None)?;
                if self.is_copy_type(&argument_ty) {
                    continue;
                }
                argument_accesses.push(BorrowedCallPlace {
                    path,
                    passing: ReceiverKind::Value,
                    param_name: "owned method argument".to_string(),
                    origin_span: argument.value.span,
                });
            }
        } else if builtin_member.variadic_argument_passing() == Some(ReceiverKind::Value) {
            for argument in args.iter().skip(1) {
                let Some(path) = self.borrow_call_place(&argument.value) else {
                    continue;
                };
                if !locals.contains_key(&path.root) {
                    continue;
                }
                let argument_ty =
                    self.type_of_expr_without_move_state(&argument.value, locals, None)?;
                if self.is_copy_type(&argument_ty) {
                    continue;
                }
                argument_accesses.push(BorrowedCallPlace {
                    path,
                    passing: ReceiverKind::Value,
                    param_name: "owned variadic method argument".to_string(),
                    origin_span: argument.value.span,
                });
            }
        }
        for argument in args {
            self.collect_expr_place_reads(
                &argument.value,
                locals,
                "method argument read",
                &mut argument_accesses,
            );
        }
        self.reject_retained_access_overlap(&retained_receiver, &argument_accesses)
    }

    #[allow(clippy::too_many_arguments)]
    fn reject_retained_expr_overlap(
        &self,
        retained: &[BorrowedCallPlace],
        expr: &Expr,
        expr_ty: &Type,
        direct_passing: Option<ReceiverKind>,
        locals_before: &HashMap<String, LocalBinding>,
        locals_after: &HashMap<String, LocalBinding>,
        value_label: &str,
    ) -> Result<()> {
        if retained.is_empty() {
            return Ok(());
        }
        let mut later_accesses = Vec::new();
        self.collect_expr_borrowed_places(expr, locals_before, &mut later_accesses)?;
        if let Some(passing) = direct_passing {
            if let Some(access) =
                self.retained_call_place_access(expr, expr_ty, passing, value_label)
            {
                later_accesses.push(access);
            }
        }
        later_accesses.extend(self.newly_moved_place_accesses(
            locals_before,
            locals_after,
            value_label,
            expr.span,
        ));
        self.collect_expr_place_reads(expr, locals_before, value_label, &mut later_accesses);
        self.reject_retained_access_overlap(retained, &later_accesses)
    }

    fn reject_retained_access_overlap(
        &self,
        retained: &[BorrowedCallPlace],
        later: &[BorrowedCallPlace],
    ) -> Result<()> {
        for current in later {
            for prior in retained {
                if !prior.path.overlaps(&current.path)
                    || (prior.passing == ReceiverKind::Borrow
                        && current.passing == ReceiverKind::Borrow)
                {
                    continue;
                }
                let action = match current.passing {
                    ReceiverKind::Borrow => "borrow",
                    ReceiverKind::BorrowMut => "mutably borrow",
                    ReceiverKind::Value => "consume",
                };
                // The recovery clause names the conflicting access. A pure
                // read or consumption has no mutation to sequence, so the
                // blanket "perform the mutation" wording misdescribed those
                // sites.
                let conflicting_access = match current.passing {
                    ReceiverKind::Borrow => "read",
                    ReceiverKind::BorrowMut => "mutation",
                    ReceiverKind::Value => "consumption",
                };
                let retained_state = match prior.passing {
                    ReceiverKind::Borrow => "shared-borrowed",
                    ReceiverKind::BorrowMut => "mutably borrowed",
                    ReceiverKind::Value => "reserved for consumption",
                };
                let origin_label = match prior.passing {
                    ReceiverKind::Borrow => {
                        format!("shared borrow for the {} begins here", prior.param_name)
                    }
                    ReceiverKind::BorrowMut => {
                        format!("mutable borrow for the {} begins here", prior.param_name)
                    }
                    ReceiverKind::Value => {
                        format!("consumption by the {} begins here", prior.param_name)
                    }
                };
                return Err(Diagnostic::coded_at(
                    "AU3002",
                    current.origin_span,
                    format!(
                        "cannot {} `{}` while `{}` remains {} by the {}",
                        action, current.path, prior.path, retained_state, prior.param_name
                    ),
                )
                .with_secondary(prior.origin_span, origin_label)
                .with_help(format!(
                    "call `.clone()` before the expression when an independent value is intended, or perform the {conflicting_access} in a separate statement first"
                )));
            }
        }
        Ok(())
    }

    fn reject_expr_borrow_move_overlap(
        &self,
        borrowed_places: &[BorrowedCallPlace],
        moved_places: &[PlacePath],
        span: crate::diag::Span,
    ) -> Result<()> {
        for moved in moved_places {
            for borrowed in borrowed_places {
                if borrowed.path.overlaps(moved) {
                    return Err(Diagnostic::at(
                        span,
                        format!(
                            "cannot mix a move of `{}` with a borrow of `{}` in the same expression",
                            moved, borrowed.path
                        ),
                    ));
                }
            }
        }
        Ok(())
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

        if receiver_kind == ReceiverKind::BorrowMut {
            self.require_mutable_receiver(object, method_name, span, locals)?;
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
                origin_span: object.span,
            });
        }
        Ok(borrowed_places)
    }

    fn require_mutable_receiver(
        &self,
        object: &Expr,
        method_name: &str,
        span: crate::diag::Span,
        locals: &mut HashMap<String, LocalBinding>,
    ) -> Result<()> {
        if let Some(place) = self.borrow_call_place(object) {
            self.ensure_place_not_frozen(&place, span, locals)?;
        }
        if self.is_mutable_place(object, locals)? {
            return Ok(());
        }
        if self.is_shared_self_place(object, locals) {
            return Err(self.shared_self_mutation_diagnostic(span, locals));
        }
        Err(Diagnostic::coded_at(
            "AU3003",
            span,
            format!("method `{}` requires a mutable receiver", method_name),
        )
        .with_help("declare the receiver place with `mut` before calling a mutating method"))
    }

    fn reject_overlapping_borrow(
        &self,
        borrowed_places: &[BorrowedCallPlace],
        current_path: &PlacePath,
        current_passing: ReceiverKind,
        current_param_name: &str,
        callee_name: &str,
        span: crate::diag::Span,
    ) -> Result<()> {
        for prior in borrowed_places {
            if !prior.path.overlaps(current_path) {
                continue;
            }
            let shared =
                prior.passing == ReceiverKind::Borrow && current_passing == ReceiverKind::Borrow;
            if shared {
                continue;
            }
            let detail = match (current_passing, prior.passing) {
                (ReceiverKind::Value, ReceiverKind::Borrow) => format!(
                    "argument for parameter `{}` in {} overlaps borrow for parameter `{}`; consumed values must be exclusive",
                    current_param_name, callee_name, prior.param_name
                ),
                (ReceiverKind::Value, ReceiverKind::BorrowMut) => format!(
                    "argument for parameter `{}` in {} overlaps mutable borrow for parameter `{}`; consumed values must be exclusive",
                    current_param_name, callee_name, prior.param_name
                ),
                (ReceiverKind::Value, ReceiverKind::Value)
                | (_, ReceiverKind::Value) => format!(
                    "argument for parameter `{}` in {} overlaps consumed argument for parameter `{}`; consumed values must be exclusive",
                    current_param_name, callee_name, prior.param_name
                ),
                (ReceiverKind::BorrowMut, ReceiverKind::Borrow) => format!(
                    "argument for parameter `{}` in {} overlaps borrow for parameter `{}`; mutable borrows must be exclusive",
                    current_param_name, callee_name, prior.param_name
                ),
                (ReceiverKind::BorrowMut, ReceiverKind::BorrowMut) => format!(
                    "argument for parameter `{}` in {} overlaps mutable borrow for parameter `{}`",
                    current_param_name, callee_name, prior.param_name
                ),
                (ReceiverKind::Borrow, ReceiverKind::BorrowMut) => format!(
                    "argument for parameter `{}` in {} overlaps mutable borrow for parameter `{}`; mutable borrows must be exclusive",
                    current_param_name, callee_name, prior.param_name
                ),
                (ReceiverKind::Borrow, ReceiverKind::Borrow) => {
                    unreachable!("overlapping shared borrows are accepted above")
                }
            };
            let origin_label = match prior.passing {
                ReceiverKind::Borrow => {
                    format!(
                        "shared borrow for parameter `{}` begins here",
                        prior.param_name
                    )
                }
                ReceiverKind::BorrowMut => {
                    format!(
                        "mutable borrow for parameter `{}` begins here",
                        prior.param_name
                    )
                }
                ReceiverKind::Value => {
                    format!(
                        "value for parameter `{}` is consumed here",
                        prior.param_name
                    )
                }
            };
            let help = if current_passing == ReceiverKind::Value
                || prior.passing == ReceiverKind::Value
            {
                "pass non-overlapping places, or call `.clone()` before consuming a value that must remain borrowed"
            } else {
                "pass non-overlapping places; shared borrows may overlap, but a mutable borrow must remain exclusive"
            };
            return Err(Diagnostic::at(span, detail)
                .with_secondary(prior.origin_span, origin_label)
                .with_help(help));
        }
        Ok(())
    }

    fn newly_moved_places(
        &self,
        before: &HashMap<String, LocalBinding>,
        after: &HashMap<String, LocalBinding>,
    ) -> Vec<PlacePath> {
        let mut moved = Vec::new();
        for (name, current) in after {
            let previous = before.get(name);
            let previously_moved = previous.map(|binding| binding.moved).unwrap_or(false);
            if current.moved && !previously_moved {
                moved.push(PlacePath::root(name.clone()));
            }
            let previous_fields = previous
                .map(|binding| binding.moved_fields.clone())
                .unwrap_or_default();
            for field in current
                .moved_fields
                .keys()
                .filter(|field| !previous_fields.contains_key(*field))
            {
                moved.push(PlacePath {
                    root: name.clone(),
                    projections: field.clone(),
                });
            }
        }
        moved
    }

    fn newly_moved_place_accesses(
        &self,
        before: &HashMap<String, LocalBinding>,
        after: &HashMap<String, LocalBinding>,
        label: &str,
        fallback_span: crate::diag::Span,
    ) -> Vec<BorrowedCallPlace> {
        self.newly_moved_places(before, after)
            .into_iter()
            .map(|path| {
                let origin_span = after
                    .get(&path.root)
                    .and_then(|binding| {
                        if path.is_root() {
                            binding.moved_at
                        } else {
                            binding.moved_fields.get(&path.projections).copied()
                        }
                    })
                    .unwrap_or(fallback_span);
                BorrowedCallPlace {
                    path,
                    passing: ReceiverKind::Value,
                    param_name: label.to_string(),
                    origin_span,
                }
            })
            .collect()
    }

    fn find_frozen_place_conflict(
        &self,
        place: &PlacePath,
        locals: &HashMap<String, LocalBinding>,
    ) -> Option<(PlacePath, crate::diag::Span)> {
        let binding = locals.get(&place.root)?;
        binding
            .frozen_places
            .iter()
            .find(|(frozen, _)| frozen.overlaps(place))
            .map(|(frozen, origin)| (frozen.clone(), *origin))
    }

    fn ensure_place_not_frozen(
        &self,
        place: &PlacePath,
        span: crate::diag::Span,
        locals: &HashMap<String, LocalBinding>,
    ) -> Result<()> {
        if let Some((frozen, origin)) = self.find_frozen_place_conflict(place, locals) {
            return Err(
                Diagnostic::at(
                    span,
                    format!(
                        "cannot mutate `{}` while `{}` is borrowed for iteration",
                        place, frozen
                    ),
                )
                .with_secondary(
                    origin,
                    format!("`{}` is borrowed for this loop here", frozen),
                )
                .with_help(format!(
                    "perform owner mutation after the loop; use `for item in mut {}:` when mutating elements through the loop binding",
                    frozen
                )),
            );
        }
        Ok(())
    }

    fn ensure_place_not_frozen_for_move(
        &self,
        place: &PlacePath,
        span: crate::diag::Span,
        locals: &HashMap<String, LocalBinding>,
    ) -> Result<()> {
        if let Some((frozen, origin)) = self.find_frozen_place_conflict(place, locals) {
            return Err(
                Diagnostic::at(
                    span,
                    format!(
                        "cannot move `{}` while `{}` is borrowed for iteration",
                        place, frozen
                    ),
                )
                .with_secondary(
                    origin,
                    format!("`{}` is borrowed for this loop here", frozen),
                )
                .with_help(format!(
                    "finish iterating before moving `{}`, or iterate an owned clone when the owner must be consumed independently",
                    place.root
                )),
            );
        }
        Ok(())
    }

    fn begin_match_borrow_mut(
        &self,
        scrutinee: &Expr,
        span: crate::diag::Span,
        locals: &mut HashMap<String, LocalBinding>,
    ) -> Result<Option<PlacePath>> {
        let Some(place) = self.borrow_call_place(scrutinee) else {
            return Err(Diagnostic::coded_at(
                "AU3002",
                span,
                "`match mut` requires a mutable place scrutinee",
            ));
        };
        self.ensure_place_not_frozen(&place, span, locals)?;
        if !self.is_mutable_place(scrutinee, locals)? {
            return Err(Diagnostic::coded_at(
                "AU3002",
                span,
                "`match mut` requires a mutable place scrutinee",
            ));
        }
        if let Some(active) = self
            .active_match_borrow_mut_places
            .borrow()
            .iter()
            .find(|active| active.overlaps(&place))
            .cloned()
        {
            return Err(Diagnostic::at(
                span,
                format!(
                    "cannot start `match borrow mut` on `{}` while `{}` is already mutably borrowed by an enclosing match",
                    place, active
                ),
            ));
        }
        self.active_match_borrow_mut_places
            .borrow_mut()
            .push(place.clone());
        Ok(Some(place))
    }

    fn end_match_borrow_mut(&self, active_place: Option<PlacePath>) {
        if active_place.is_none() {
            return;
        }
        self.active_match_borrow_mut_places.borrow_mut().pop();
    }

    fn render_variant_pattern_shape(&self, variant_name: &str, payload_tys: &[Type]) -> String {
        if payload_tys.is_empty() {
            return variant_name.to_string();
        }
        let payload = std::iter::repeat_n("_", payload_tys.len())
            .collect::<Vec<_>>()
            .join(", ");
        format!("{variant_name}({payload})")
    }

    fn missing_patterns_for_type(&self, patterns: &[&Pattern], expected_ty: &Type) -> Vec<String> {
        if patterns
            .iter()
            .any(|pattern| self.pattern_covers_entire_type(pattern, expected_ty))
        {
            return Vec::new();
        }
        if let Type::Tuple(element_types) = expected_ty {
            if self.tuple_patterns_cover_type_union(patterns, element_types) {
                return Vec::new();
            }
            return vec!["_".to_string()];
        }
        let Some(variants) = self.enum_variants_for_type(expected_ty) else {
            return vec!["_".to_string()];
        };
        let mut grouped = BTreeMap::<String, Vec<&VariantPattern>>::new();
        for pattern in patterns {
            if let Pattern::Variant(variant_pattern) = pattern {
                grouped
                    .entry(variant_pattern.variant_name.clone())
                    .or_default()
                    .push(variant_pattern);
            }
        }
        let mut missing = Vec::new();
        for (variant_name, payload_tys) in variants {
            let variant_patterns = grouped.get(&variant_name).cloned().unwrap_or_default();
            if variant_patterns.is_empty() {
                missing.push(self.render_variant_pattern_shape(&variant_name, &payload_tys));
                continue;
            }
            if self.variant_patterns_cover_payloads_union(&variant_patterns, &payload_tys) {
                continue;
            }
            if payload_tys.len() == 1 {
                let nested_patterns = variant_patterns
                    .iter()
                    .filter_map(|pattern| pattern.subpatterns.first())
                    .collect::<Vec<_>>();
                let nested_missing =
                    self.missing_patterns_for_type(&nested_patterns, &payload_tys[0]);
                if !nested_missing.is_empty() {
                    missing.extend(
                        nested_missing
                            .into_iter()
                            .map(|nested| format!("{variant_name}({nested})")),
                    );
                    continue;
                }
            }
            missing.push(self.render_variant_pattern_shape(&variant_name, &payload_tys));
        }
        missing
    }

    fn pattern_span(&self, pattern: &Pattern) -> crate::diag::Span {
        match pattern {
            Pattern::Wildcard(span) => *span,
            Pattern::Literal(pattern) => pattern.span,
            Pattern::Binding(binding) => binding.span,
            Pattern::Variant(variant) => variant.span,
            Pattern::Tuple(tuple) => tuple.span,
        }
    }

    fn patterns_cover_pattern(
        &self,
        patterns: &[&Pattern],
        pattern: &Pattern,
        expected_ty: &Type,
    ) -> bool {
        match pattern {
            Pattern::Wildcard(_) | Pattern::Binding(_) => {
                if self.patterns_cover_type_union(patterns, expected_ty) {
                    return true;
                }
                if matches!(expected_ty, Type::Named(name, args) if name == "bool" && args.is_empty())
                {
                    let mut covered = BTreeSet::new();
                    for previous in patterns {
                        match previous {
                            Pattern::Wildcard(_) | Pattern::Binding(_) => return true,
                            Pattern::Literal(literal) => {
                                if let Ok(LiteralPatternKey::Bool(value)) =
                                    self.literal_pattern_key(literal, expected_ty)
                                {
                                    covered.insert(value);
                                }
                            }
                            Pattern::Variant(_) => {}
                            Pattern::Tuple(_) => {}
                        }
                    }
                    return covered.len() == 2;
                }
                patterns
                    .iter()
                    .any(|previous| self.pattern_covers_entire_type(previous, expected_ty))
            }
            Pattern::Variant(current_variant) => {
                if patterns.iter().any(|previous| {
                    self.pattern_is_covered_by_pattern(previous, pattern, expected_ty)
                }) {
                    return true;
                }
                let Some(variants) = self.enum_variants_for_type(expected_ty) else {
                    return false;
                };
                let Some((_, payload_tys)) = variants
                    .iter()
                    .find(|(variant_name, _)| variant_name == &current_variant.variant_name)
                else {
                    return false;
                };
                let variant_patterns = patterns
                    .iter()
                    .filter_map(|previous| match previous {
                        Pattern::Variant(variant)
                            if variant.variant_name == current_variant.variant_name =>
                        {
                            Some(variant)
                        }
                        _ => None,
                    })
                    .collect::<Vec<_>>();
                if current_variant.subpatterns.len() != payload_tys.len() {
                    return false;
                }
                let rows = variant_patterns
                    .iter()
                    .map(|variant| variant.subpatterns.clone())
                    .collect::<Vec<_>>();
                self.pattern_rows_cover_pattern_union(
                    &rows,
                    &current_variant.subpatterns,
                    payload_tys,
                )
            }
            Pattern::Tuple(current_tuple) => {
                if patterns.iter().any(|previous| {
                    self.pattern_is_covered_by_pattern(previous, pattern, expected_ty)
                }) {
                    return true;
                }
                let Type::Tuple(element_types) = expected_ty else {
                    return false;
                };
                if current_tuple.elements.len() != element_types.len() {
                    return false;
                }
                let rows = patterns
                    .iter()
                    .filter_map(|previous| match previous {
                        Pattern::Tuple(tuple) if tuple.elements.len() == element_types.len() => {
                            Some(tuple.elements.clone())
                        }
                        _ => None,
                    })
                    .collect::<Vec<_>>();
                self.pattern_rows_cover_pattern_union(&rows, &current_tuple.elements, element_types)
            }
            Pattern::Literal(_) => patterns
                .iter()
                .any(|previous| self.pattern_is_covered_by_pattern(previous, pattern, expected_ty)),
        }
    }

    fn pattern_is_covered_by_pattern(
        &self,
        previous: &Pattern,
        current: &Pattern,
        expected_ty: &Type,
    ) -> bool {
        if self.pattern_covers_entire_type(previous, expected_ty) {
            return true;
        }
        match (previous, current) {
            (Pattern::Literal(previous), Pattern::Literal(current)) => {
                self.literal_pattern_key(previous, expected_ty).ok()
                    == self.literal_pattern_key(current, expected_ty).ok()
            }
            (Pattern::Variant(previous), Pattern::Variant(current)) => {
                if previous.variant_name != current.variant_name {
                    return false;
                }
                let Some(variants) = self.enum_variants_for_type(expected_ty) else {
                    return false;
                };
                let Some((_, payload_tys)) = variants
                    .iter()
                    .find(|(variant_name, _)| variant_name == &current.variant_name)
                else {
                    return false;
                };
                if previous.subpatterns.len() != payload_tys.len()
                    || current.subpatterns.len() != payload_tys.len()
                {
                    return false;
                }
                previous
                    .subpatterns
                    .iter()
                    .zip(current.subpatterns.iter())
                    .zip(payload_tys.iter())
                    .all(|((previous, current), payload_ty)| {
                        self.pattern_is_covered_by_pattern(previous, current, payload_ty)
                    })
            }
            (Pattern::Tuple(previous), Pattern::Tuple(current)) => {
                let Type::Tuple(element_types) = expected_ty else {
                    return false;
                };
                previous.elements.len() == element_types.len()
                    && current.elements.len() == element_types.len()
                    && previous
                        .elements
                        .iter()
                        .zip(&current.elements)
                        .zip(element_types)
                        .all(|((previous, current), ty)| {
                            self.pattern_is_covered_by_pattern(previous, current, ty)
                        })
            }
            _ => false,
        }
    }

    fn pattern_covers_entire_type(&self, pattern: &Pattern, expected_ty: &Type) -> bool {
        match pattern {
            Pattern::Wildcard(_) | Pattern::Binding(_) => true,
            Pattern::Literal(_) => false,
            Pattern::Tuple(tuple) => {
                let Type::Tuple(element_types) = expected_ty else {
                    return false;
                };
                tuple.elements.len() == element_types.len()
                    && tuple
                        .elements
                        .iter()
                        .zip(element_types)
                        .all(|(element, ty)| self.pattern_covers_entire_type(element, ty))
            }
            Pattern::Variant(variant_pattern) => {
                let Some(variants) = self.enum_variants_for_type(expected_ty) else {
                    return false;
                };
                let Some((_, payloads)) = variants
                    .iter()
                    .find(|(name, _)| name == &variant_pattern.variant_name)
                else {
                    return false;
                };
                if variants.len() != 1 || payloads.len() != variant_pattern.subpatterns.len() {
                    return false;
                }
                variant_pattern.subpatterns.iter().zip(payloads.iter()).all(
                    |(subpattern, payload_ty)| {
                        self.pattern_covers_entire_type(subpattern, payload_ty)
                    },
                )
            }
        }
    }

    fn variant_pattern_covers_payloads(
        &self,
        variant_pattern: &VariantPattern,
        payload_tys: &[Type],
    ) -> bool {
        variant_pattern.subpatterns.len() == payload_tys.len()
            && variant_pattern
                .subpatterns
                .iter()
                .zip(payload_tys.iter())
                .all(|(subpattern, payload_ty)| {
                    self.pattern_covers_entire_type(subpattern, payload_ty)
                })
    }

    fn patterns_cover_type_union(&self, patterns: &[&Pattern], expected_ty: &Type) -> bool {
        if patterns
            .iter()
            .any(|pattern| self.pattern_covers_entire_type(pattern, expected_ty))
        {
            return true;
        }
        if let Type::Tuple(element_types) = expected_ty {
            return self.tuple_patterns_cover_type_union(patterns, element_types);
        }
        let Some(variants) = self.enum_variants_for_type(expected_ty) else {
            return false;
        };
        let mut grouped = BTreeMap::<String, Vec<&VariantPattern>>::new();
        for pattern in patterns {
            if let Pattern::Variant(variant_pattern) = pattern {
                grouped
                    .entry(variant_pattern.variant_name.clone())
                    .or_default()
                    .push(variant_pattern);
            }
        }
        variants.into_iter().all(|(variant_name, payloads)| {
            let Some(variant_patterns) = grouped.get(&variant_name) else {
                return false;
            };
            self.variant_patterns_cover_payloads_union(variant_patterns, &payloads)
        })
    }

    fn tuple_patterns_cover_type_union(
        &self,
        patterns: &[&Pattern],
        element_types: &[Type],
    ) -> bool {
        let rows = patterns
            .iter()
            .filter_map(|pattern| match pattern {
                Pattern::Tuple(tuple) if tuple.elements.len() == element_types.len() => {
                    Some(tuple.elements.clone())
                }
                _ => None,
            })
            .collect::<Vec<_>>();
        self.pattern_rows_cover_type_union(&rows, element_types)
    }

    fn pattern_rows_cover_type_union(&self, rows: &[Vec<Pattern>], types: &[Type]) -> bool {
        let Some((first_ty, remaining_types)) = types.split_first() else {
            return !rows.is_empty();
        };
        let irrefutable =
            |pattern: &Pattern| matches!(pattern, Pattern::Wildcard(_) | Pattern::Binding(_));

        if matches!(first_ty, Type::Named(name, args) if name == "bool" && args.is_empty()) {
            return [false, true].into_iter().all(|expected| {
                let specialized = rows
                    .iter()
                    .filter_map(|row| {
                        let (first, remaining) = row.split_first()?;
                        if irrefutable(first)
                            || matches!(
                                first,
                                Pattern::Literal(LiteralPattern {
                                    kind: LiteralPatternKind::Bool(actual),
                                    ..
                                }) if *actual == expected
                            )
                        {
                            Some(remaining.to_vec())
                        } else {
                            None
                        }
                    })
                    .collect::<Vec<_>>();
                self.pattern_rows_cover_type_union(&specialized, remaining_types)
            });
        }

        if let Type::Tuple(nested_types) = first_ty {
            let mut specialized_types = nested_types.clone();
            specialized_types.extend_from_slice(remaining_types);
            let specialized = rows
                .iter()
                .filter_map(|row| {
                    let (first, remaining) = row.split_first()?;
                    let mut values = match first {
                        pattern if irrefutable(pattern) => {
                            vec![Pattern::Wildcard(self.pattern_span(pattern)); nested_types.len()]
                        }
                        Pattern::Tuple(tuple) if tuple.elements.len() == nested_types.len() => {
                            tuple.elements.clone()
                        }
                        _ => return None,
                    };
                    values.extend_from_slice(remaining);
                    Some(values)
                })
                .collect::<Vec<_>>();
            return self.pattern_rows_cover_type_union(&specialized, &specialized_types);
        }

        if let Some(variants) = self.enum_variants_for_type(first_ty) {
            return variants.into_iter().all(|(variant_name, payload_types)| {
                let mut specialized_types = payload_types.clone();
                specialized_types.extend_from_slice(remaining_types);
                let specialized = rows
                    .iter()
                    .filter_map(|row| {
                        let (first, remaining) = row.split_first()?;
                        let mut values = match first {
                            pattern if irrefutable(pattern) => {
                                vec![
                                    Pattern::Wildcard(self.pattern_span(pattern));
                                    payload_types.len()
                                ]
                            }
                            Pattern::Variant(variant)
                                if variant.variant_name == variant_name
                                    && variant.subpatterns.len() == payload_types.len() =>
                            {
                                variant.subpatterns.clone()
                            }
                            _ => return None,
                        };
                        values.extend_from_slice(remaining);
                        Some(values)
                    })
                    .collect::<Vec<_>>();
                self.pattern_rows_cover_type_union(&specialized, &specialized_types)
            });
        }

        let specialized = rows
            .iter()
            .filter_map(|row| {
                let (first, remaining) = row.split_first()?;
                irrefutable(first).then(|| remaining.to_vec())
            })
            .collect::<Vec<_>>();
        self.pattern_rows_cover_type_union(&specialized, remaining_types)
    }

    fn pattern_rows_cover_pattern_union(
        &self,
        rows: &[Vec<Pattern>],
        current: &[Pattern],
        types: &[Type],
    ) -> bool {
        let Some((first_ty, remaining_types)) = types.split_first() else {
            return current.is_empty() && !rows.is_empty();
        };
        let Some((current_first, current_remaining)) = current.split_first() else {
            return false;
        };
        let irrefutable =
            |pattern: &Pattern| matches!(pattern, Pattern::Wildcard(_) | Pattern::Binding(_));

        let specialize_bool = |expected: bool| {
            rows.iter()
                .filter_map(|row| {
                    let (first, remaining) = row.split_first()?;
                    if irrefutable(first)
                        || matches!(
                            first,
                            Pattern::Literal(LiteralPattern {
                                kind: LiteralPatternKind::Bool(actual),
                                ..
                            }) if *actual == expected
                        )
                    {
                        Some(remaining.to_vec())
                    } else {
                        None
                    }
                })
                .collect::<Vec<_>>()
        };

        if matches!(first_ty, Type::Named(name, args) if name == "bool" && args.is_empty()) {
            return match current_first {
                Pattern::Wildcard(_) | Pattern::Binding(_) => {
                    [false, true].into_iter().all(|expected| {
                        self.pattern_rows_cover_pattern_union(
                            &specialize_bool(expected),
                            current_remaining,
                            remaining_types,
                        )
                    })
                }
                Pattern::Literal(LiteralPattern {
                    kind: LiteralPatternKind::Bool(expected),
                    ..
                }) => self.pattern_rows_cover_pattern_union(
                    &specialize_bool(*expected),
                    current_remaining,
                    remaining_types,
                ),
                _ => false,
            };
        }

        if let Type::Tuple(nested_types) = first_ty {
            let current_nested = match current_first {
                pattern if irrefutable(pattern) => {
                    vec![Pattern::Wildcard(self.pattern_span(pattern)); nested_types.len()]
                }
                Pattern::Tuple(tuple) if tuple.elements.len() == nested_types.len() => {
                    tuple.elements.clone()
                }
                _ => return false,
            };
            let mut specialized_types = nested_types.clone();
            specialized_types.extend_from_slice(remaining_types);
            let mut specialized_current = current_nested;
            specialized_current.extend_from_slice(current_remaining);
            let specialized_rows = rows
                .iter()
                .filter_map(|row| {
                    let (first, remaining) = row.split_first()?;
                    let mut values = match first {
                        pattern if irrefutable(pattern) => {
                            vec![Pattern::Wildcard(self.pattern_span(pattern)); nested_types.len()]
                        }
                        Pattern::Tuple(tuple) if tuple.elements.len() == nested_types.len() => {
                            tuple.elements.clone()
                        }
                        _ => return None,
                    };
                    values.extend_from_slice(remaining);
                    Some(values)
                })
                .collect::<Vec<_>>();
            return self.pattern_rows_cover_pattern_union(
                &specialized_rows,
                &specialized_current,
                &specialized_types,
            );
        }

        if let Some(variants) = self.enum_variants_for_type(first_ty) {
            let current_variant_payloads = match current_first {
                pattern if irrefutable(pattern) => None,
                Pattern::Variant(current_variant) => Some(current_variant.subpatterns.clone()),
                _ => return false,
            };
            let variants_to_cover = if let Pattern::Variant(current_variant) = current_first {
                variants
                    .into_iter()
                    .filter(|(variant_name, payload_types)| {
                        variant_name == &current_variant.variant_name
                            && payload_types.len() == current_variant.subpatterns.len()
                    })
                    .collect()
            } else {
                variants
            };
            if variants_to_cover.is_empty() {
                return false;
            }
            return variants_to_cover
                .into_iter()
                .all(|(variant_name, payload_types)| {
                    let current_payloads = current_variant_payloads.clone().unwrap_or_else(|| {
                        vec![
                            Pattern::Wildcard(self.pattern_span(current_first));
                            payload_types.len()
                        ]
                    });
                    let mut specialized_types = payload_types.clone();
                    specialized_types.extend_from_slice(remaining_types);
                    let mut specialized_current = current_payloads;
                    specialized_current.extend_from_slice(current_remaining);
                    let specialized_rows = rows
                        .iter()
                        .filter_map(|row| {
                            let (first, remaining) = row.split_first()?;
                            let mut values = match first {
                                pattern if irrefutable(pattern) => {
                                    vec![
                                        Pattern::Wildcard(self.pattern_span(pattern));
                                        payload_types.len()
                                    ]
                                }
                                Pattern::Variant(variant)
                                    if variant.variant_name == variant_name
                                        && variant.subpatterns.len() == payload_types.len() =>
                                {
                                    variant.subpatterns.clone()
                                }
                                _ => return None,
                            };
                            values.extend_from_slice(remaining);
                            Some(values)
                        })
                        .collect::<Vec<_>>();
                    self.pattern_rows_cover_pattern_union(
                        &specialized_rows,
                        &specialized_current,
                        &specialized_types,
                    )
                });
        }

        let specialized = rows
            .iter()
            .filter_map(|row| {
                let (first, remaining) = row.split_first()?;
                match current_first {
                    Pattern::Wildcard(_) | Pattern::Binding(_) => {
                        irrefutable(first).then(|| remaining.to_vec())
                    }
                    Pattern::Literal(_) => self
                        .pattern_is_covered_by_pattern(first, current_first, first_ty)
                        .then(|| remaining.to_vec()),
                    _ => None,
                }
            })
            .collect::<Vec<_>>();
        self.pattern_rows_cover_pattern_union(&specialized, current_remaining, remaining_types)
    }

    fn variant_patterns_cover_payloads_union(
        &self,
        patterns: &[&VariantPattern],
        payload_tys: &[Type],
    ) -> bool {
        if patterns
            .iter()
            .any(|pattern| self.variant_pattern_covers_payloads(pattern, payload_tys))
        {
            return true;
        }
        let rows = patterns
            .iter()
            .filter(|pattern| pattern.subpatterns.len() == payload_tys.len())
            .map(|pattern| pattern.subpatterns.clone())
            .collect::<Vec<_>>();
        self.pattern_rows_cover_type_union(&rows, payload_tys)
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

    fn is_shared_self_place(&self, expr: &Expr, locals: &HashMap<String, LocalBinding>) -> bool {
        self.member_access_path(expr)
            .filter(|place| place.root == "self")
            .and_then(|_| locals.get("self"))
            .is_some_and(|binding| binding.passing == ReceiverKind::Borrow)
    }

    fn shared_self_mutation_diagnostic(
        &self,
        span: crate::diag::Span,
        locals: &HashMap<String, LocalBinding>,
    ) -> Diagnostic {
        let mut diagnostic = Diagnostic::coded_at(
            "AU3003",
            span,
            "cannot mutate through shared receiver `self`; declare the receiver as `mut self`",
        );
        if let Some(origin) = locals.get("self").and_then(|binding| binding.borrowed_at) {
            diagnostic =
                diagnostic.with_secondary(origin, "shared receiver `self` is declared here");
        }
        diagnostic
            .with_help("declare the receiver as `mut self` when the method mutates through `self`")
    }

    fn borrowed_root_binding_name(
        &self,
        expr: &Expr,
        locals: &HashMap<String, LocalBinding>,
    ) -> Option<String> {
        match &expr.kind {
            ExprKind::Name(name) => locals
                .get(name)
                .filter(|binding| binding.passing != ReceiverKind::Value)
                .map(|_| name.clone()),
            ExprKind::Group(inner) => self.borrowed_root_binding_name(inner, locals),
            ExprKind::Member { object, .. } => self.borrowed_root_binding_name(object, locals),
            ExprKind::Index { object, .. } => self.borrowed_root_binding_name(object, locals),
            _ => None,
        }
    }

    fn member_access_path(&self, expr: &Expr) -> Option<PlacePath> {
        match &expr.kind {
            ExprKind::Name(name) => Some(PlacePath::root(name.clone())),
            ExprKind::Group(inner)
            | ExprKind::Cast { expr: inner, .. }
            | ExprKind::Specialize { expr: inner, .. } => self.member_access_path(inner),
            ExprKind::Member { object, field } => {
                let parent = self.member_access_path(object)?;
                Some(parent.with_field(field.clone()))
            }
            _ => None,
        }
    }

    fn member_target_path(&self, object: &Expr, field: &str) -> Option<PlacePath> {
        let parent = self.member_access_path(object)?;
        Some(parent.with_field(field.to_string()))
    }

    fn field_path_is_moved(binding: &LocalBinding, path: &ProjectionPath) -> bool {
        binding
            .moved_fields
            .keys()
            .any(|moved_path| moved_path.overlaps(path))
    }

    fn moved_field_origin(
        binding: &LocalBinding,
        path: &ProjectionPath,
    ) -> Option<crate::diag::Span> {
        binding
            .moved_fields
            .iter()
            .find(|(moved_path, _)| moved_path.overlaps(path))
            .map(|(_, span)| *span)
    }

    fn clear_moved_field_path(binding: &mut LocalBinding, path: &ProjectionPath) {
        binding
            .moved_fields
            .retain(|moved_path, _| !moved_path.is_descendant_of_or_equal(path));
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
                    return Err(self.moved_value_diagnostic(name, expr.span, binding));
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
            .or_else(|| find_namespace_in_modules(self.imported_modules, path))
    }

    fn current_module_namespace(&self) -> Option<&ModuleNamespace> {
        if self.module_name == self.root_module_name {
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

    fn canonical_nominal_type_name(
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

    fn canonical_class_name(&self, surface_name: &str, class_info: &ClassInfo) -> String {
        self.canonical_nominal_type_name(
            surface_name,
            &class_info.module_name,
            &class_info.decl.name,
        )
    }

    fn canonical_enum_info_name(&self, surface_name: &str, enum_info: &EnumInfo) -> String {
        self.canonical_nominal_type_name(surface_name, &enum_info.module_name, &enum_info.decl.name)
    }

    fn canonical_enum_name(&self, name: &str) -> String {
        if let Some(enum_info) = self.resolve_enum_info(name) {
            return self.canonical_enum_info_name(name, enum_info);
        }
        name.rsplit_once('.')
            .map(|(_, leaf)| leaf.to_string())
            .unwrap_or_else(|| name.to_string())
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

    fn collect_trait_bound_closure(
        &self,
        bound: &TraitBound,
        self_ty: &Type,
        seen: &mut BTreeSet<String>,
        closure: &mut Vec<TraitBound>,
    ) {
        let key = format!("{} for {}", bound, self_ty);
        if !seen.insert(key) {
            return;
        }
        closure.push(bound.clone());
        let Some(trait_info) = self.traits.get(&bound.trait_name) else {
            return;
        };
        let substitutions =
            self_type_substitutions(&trait_info.decl, &bound.trait_args, self_ty.clone());
        for supertrait in &trait_info.supertraits {
            let resolved = substitute_trait_bound(supertrait, &substitutions);
            self.collect_trait_bound_closure(&resolved, self_ty, seen, closure);
        }
    }

    fn trait_bound_closure(&self, bound: &TraitBound, self_ty: &Type) -> Vec<TraitBound> {
        let mut closure = Vec::new();
        let mut seen = BTreeSet::new();
        self.collect_trait_bound_closure(bound, self_ty, &mut seen, &mut closure);
        closure
    }

    fn resolved_trait_bound_for_impl(
        &self,
        trait_impl: &TraitImplInfo,
        substitutions: &HashMap<String, Type>,
    ) -> TraitBound {
        TraitBound {
            trait_name: trait_impl.trait_name.clone(),
            trait_args: trait_impl
                .trait_args
                .iter()
                .map(|arg| substitute_type(arg, substitutions))
                .collect(),
        }
    }

    fn type_implements_trait_bound(&self, ty: &Type, bound: &TraitBound) -> bool {
        self.trait_impls_in_scope().any(|trait_impl| {
            let Some(substitutions) = self.trait_impl_substitutions(trait_impl, ty) else {
                return false;
            };
            let implemented = self.resolved_trait_bound_for_impl(trait_impl, &substitutions);
            self.trait_bound_closure(&implemented, ty)
                .into_iter()
                .any(|candidate| candidate == *bound)
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
                    let self_ty = Type::TypeParam(name.clone());
                    let satisfies = current_bounds.into_iter().any(|current| {
                        self.trait_bound_closure(&current, &self_ty)
                            .into_iter()
                            .any(|candidate| candidate == *bound)
                    });
                    if !satisfies {
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
        let self_ty = Type::TypeParam(type_param_name.to_string());
        for bound in self
            .type_param_bounds
            .get(type_param_name)
            .into_iter()
            .flatten()
        {
            for bound in self.trait_bound_closure(bound, &self_ty) {
                if let Some(trait_info) = self.traits.get(&bound.trait_name) {
                    if let Some(method) = trait_info.methods.get(method_name) {
                        let trait_substitutions = self_type_substitutions(
                            &trait_info.decl,
                            &bound.trait_args,
                            self_ty.clone(),
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
                                param_passings: method.signature.param_passings.clone(),
                                return_type: substitute_type(
                                    &method.signature.return_type,
                                    &trait_substitutions,
                                ),
                                rng_clone_safe_type_params: method
                                    .signature
                                    .rng_clone_safe_type_params
                                    .iter()
                                    .filter(|name| method.decl.type_params.contains(name))
                                    .cloned()
                                    .collect(),
                            },
                            type_param_bounds: substitute_trait_bounds(
                                &method.type_param_bounds,
                                &trait_substitutions,
                            ),
                            rng_clone_safe_types: method
                                .signature
                                .rng_clone_safe_type_params
                                .iter()
                                .map(|name| {
                                    substitute_type(
                                        &Type::TypeParam(name.clone()),
                                        &trait_substitutions,
                                    )
                                })
                                .collect(),
                        });
                    }
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
        span: crate::diag::Span,
    ) -> Result<Option<TraitMethodMatch<'_>>> {
        let mut matches = Vec::new();
        for trait_impl in self.trait_impls_in_scope() {
            let Some(substitutions) = self.trait_impl_substitutions(trait_impl, ty) else {
                continue;
            };
            let Some(method) = trait_impl.methods.get(method_name) else {
                continue;
            };
            matches.push((
                trait_impl_specificity(trait_impl),
                trait_impl,
                method,
                substitutions,
            ));
        }

        if matches.is_empty() {
            return Ok(None);
        }

        matches.sort_by_key(|candidate| std::cmp::Reverse(candidate.0));
        let best_score = matches[0].0;
        let mut best_matches = matches
            .into_iter()
            .filter(|(score, _, _, _)| *score == best_score)
            .collect::<Vec<_>>();
        match best_matches.len() {
            1 => {
                let (_, trait_impl, method, substitutions) = best_matches
                    .pop()
                    .expect("best trait impl match should exist");
                Ok(Some((trait_impl, method, substitutions)))
            }
            _ => Err(Diagnostic::at(
                span,
                format!(
                    "method `{}` is ambiguous for type `{}` because multiple trait impls match with the same specificity",
                    method_name, ty
                ),
            )),
        }
    }

    fn has_from_conversion(
        &self,
        source_ty: &Type,
        target_ty: &Type,
        span: crate::diag::Span,
    ) -> Result<bool> {
        for trait_impl in self.trait_impls_in_scope() {
            if trait_impl.trait_name != "From" || trait_impl.trait_args.len() != 1 {
                continue;
            }
            let Some(method) = trait_impl.methods.get("from") else {
                continue;
            };
            let Some(mut substitutions) = self.trait_impl_substitutions(trait_impl, target_ty)
            else {
                continue;
            };
            if substitute_type(&trait_impl.trait_args[0], &substitutions) != *source_ty {
                continue;
            }
            let operation = "implicit `From.from` conversion";
            self.enforce_rng_clone_obligations_before_method_inference(
                operation,
                &method.signature.rng_clone_safe_type_params,
                &substitutions,
                &method.decl.type_params,
                span,
            )?;
            substitutions = self.infer_method_type_substitutions(
                operation,
                &method.decl.type_params,
                &method.signature.params,
                &method.type_param_bounds,
                &method.signature.rng_clone_safe_type_params,
                std::slice::from_ref(source_ty),
                substitutions,
                span,
            )?;
            let _ = substitutions;
            return Ok(true);
        }
        Ok(false)
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
                            "task start target must be a callable function, found `{}`",
                            function_name
                        ),
                    )
                })?;
                Ok(ResolvedCallableInfo {
                    display_name: function_name.clone(),
                    decl: function.decl.clone(),
                    signature: function.signature.clone(),
                    type_param_bounds: function.type_param_bounds.clone(),
                    seed_substitutions: HashMap::new(),
                })
            }
            ExprKind::Member { object, field } => {
                let (base_object, object_type_args) = self.peel_specialization(object);
                if let Some((module_path, item_name)) = self.qualified_module_item(object) {
                    if let Some(namespace) = self.module_namespace(&module_path) {
                        if let Some(class_info) = namespace.classes.get(&item_name) {
                            if let Some(method) = class_info.methods.get(field) {
                                if method.decl.receiver.is_none() {
                                    let seed_substitutions = if let Some(type_args) = object_type_args
                                    {
                                        self.explicit_type_substitutions(
                                            &class_info.decl.type_params,
                                            type_args,
                                            object.span,
                                            &format!("class `{}`", item_name),
                                        )?
                                    } else {
                                        HashMap::new()
                                    };
                                    return Ok(ResolvedCallableInfo {
                                        display_name: format!("{}.{}", item_name, field),
                                        decl: method.decl.clone(),
                                        signature: method.signature.clone(),
                                        type_param_bounds: method.type_param_bounds.clone(),
                                        seed_substitutions,
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
                                seed_substitutions: HashMap::new(),
                            });
                        }
                    }
                }

                if let ExprKind::Name(class_name) = &base_object.kind {
                    if let Some(class_info) = self.resolve_class_info(class_name) {
                        if let Some(method) = class_info.methods.get(field) {
                            if method.decl.receiver.is_none() {
                                let seed_substitutions = if let Some(type_args) = object_type_args {
                                    self.explicit_type_substitutions(
                                        &class_info.decl.type_params,
                                        type_args,
                                        object.span,
                                        &format!("class `{}`", class_name),
                                    )?
                                } else {
                                    HashMap::new()
                                };
                                return Ok(ResolvedCallableInfo {
                                    display_name: format!("{}.{}", class_name, field),
                                    decl: method.decl.clone(),
                                    signature: method.signature.clone(),
                                    type_param_bounds: method.type_param_bounds.clone(),
                                    seed_substitutions,
                                });
                            }
                        }
                    }
                }

                Err(Diagnostic::at(
                    callee.span,
                    "task starting currently supports named functions and associated methods without `self`",
                ))
            }
            _ => Err(Diagnostic::at(
                callee.span,
                "task starting currently supports named functions and associated methods without `self`",
            )),
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn type_check_callable_args(
        &self,
        callee_name: &str,
        callee_type_params: &[String],
        param_decls: &[Param],
        param_passings: &[ReceiverKind],
        param_types: &[Type],
        return_type: &Type,
        callee_type_param_bounds: &BTreeMap<String, Vec<TraitBound>>,
        callee_rng_clone_safe_type_params: &BTreeSet<String>,
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
            param_passings,
            param_types,
            return_type,
            callee_type_param_bounds,
            callee_rng_clone_safe_type_params,
            args,
            span,
            locals,
            expected_return,
            seed_substitutions,
            Vec::new(),
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn type_check_callable_args_seeded(
        &self,
        callee_name: &str,
        callee_type_params: &[String],
        param_decls: &[Param],
        param_passings: &[ReceiverKind],
        param_types: &[Type],
        return_type: &Type,
        callee_type_param_bounds: &BTreeMap<String, Vec<TraitBound>>,
        callee_rng_clone_safe_type_params: &BTreeSet<String>,
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
        let mut resolved_args = Vec::new();
        for ((argument, expected), param_decl) in ordered_args
            .into_iter()
            .zip(param_types.iter())
            .zip(param_decls.iter())
        {
            let locals_before = locals.clone();
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
                let default = param_decl.default.as_ref().ok_or_else(|| {
                    Diagnostic::at(
                        span,
                        "internal error: optional parameter is missing its default expression",
                    )
                })?;
                if matches!(default.kind, ExprKind::BuiltinOmitted) {
                    hinted_expected.clone()
                } else {
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
                }
            };
            let nested_move_span = argument
                .map(|argument| argument.value.span)
                .or_else(|| param_decl.default.as_ref().map(|default| default.span))
                .unwrap_or(span);
            let nested_moved_accesses = self.newly_moved_place_accesses(
                &locals_before,
                locals,
                "nested argument consumption",
                nested_move_span,
            );
            let mut nested_borrowed_places = Vec::new();
            if let Some(argument) = argument {
                self.collect_expr_borrowed_places(
                    &argument.value,
                    &locals_before,
                    &mut nested_borrowed_places,
                )?;
            }
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
            resolved_args.push((
                argument,
                actual,
                nested_moved_accesses,
                nested_borrowed_places,
            ));
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

        self.enforce_rng_clone_obligations(
            callee_name,
            callee_rng_clone_safe_type_params,
            &substitutions,
            span,
        )?;

        let resolved_return_type = substitute_type(return_type, &substitutions);

        let mut enclosing_accesses = seeded_borrowed_places.clone();
        for (
            (((argument, _actual, _nested_moved, _nested_borrowed), _expected), param_decl),
            param_passing,
        ) in resolved_args
            .iter()
            .zip(param_types.iter())
            .zip(param_decls.iter())
            .zip(param_passings.iter().copied())
        {
            let Some(argument) = *argument else {
                continue;
            };
            let label = format!("parameter `{}`", param_decl.name);
            self.collect_result_place_accesses(
                &argument.value,
                locals,
                param_passing,
                &label,
                &mut enclosing_accesses,
            )?;
        }
        for (_argument, _actual, _nested_moved, nested_borrowed) in &resolved_args {
            self.reject_retained_access_overlap(&enclosing_accesses, nested_borrowed)?;
        }

        let mut source_order_accesses = seeded_borrowed_places.clone();
        let mut branch_only_accesses: Vec<BorrowedCallPlace> = Vec::new();
        for source_argument in args {
            let Some(index) = resolved_args.iter().position(|(argument, _, _, _)| {
                argument.is_some_and(|argument| std::ptr::eq(argument, source_argument))
            }) else {
                continue;
            };
            self.reject_retained_access_overlap(&source_order_accesses, &resolved_args[index].2)?;
            let mut point_reads = Vec::new();
            self.collect_expr_place_reads(
                &source_argument.value,
                locals,
                "argument read",
                &mut point_reads,
            );
            let label = format!("parameter `{}`", param_decls[index].name);
            let mut direct_accesses = Vec::new();
            self.collect_result_place_accesses(
                &source_argument.value,
                locals,
                param_passings[index],
                &label,
                &mut direct_accesses,
            )?;
            for direct_access in &direct_accesses {
                point_reads.retain(|read| {
                    read.path != direct_access.path || read.origin_span != direct_access.origin_span
                });
            }
            self.reject_retained_access_overlap(&source_order_accesses, &point_reads)?;
            // An argument that is itself a place is compared pairwise below,
            // where a parameter-aware diagnostic is produced, so this
            // source-ordered pass only reports it against places that the
            // pairwise pass cannot see: the extra places contributed by an
            // earlier branching or composite argument.
            let whole_argument_place = self.borrow_call_place(&source_argument.value);
            let (whole_accesses, branch_accesses): (Vec<_>, Vec<_>) = direct_accesses
                .iter()
                .cloned()
                .partition(|access| whole_argument_place.as_ref() == Some(&access.path));
            self.reject_retained_access_overlap(&branch_only_accesses, &whole_accesses)?;
            self.reject_retained_access_overlap(&source_order_accesses, &branch_accesses)?;
            source_order_accesses.extend(direct_accesses);
            branch_only_accesses.extend(branch_accesses);
        }

        let mut borrowed_places = seeded_borrowed_places;
        for (
            (
                ((argument, actual, nested_moved_accesses, _nested_borrowed_places), expected),
                param_decl,
            ),
            param_passing,
        ) in resolved_args
            .into_iter()
            .zip(param_types.iter())
            .zip(param_decls.iter())
            .zip(param_passings.iter().copied())
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
                match param_passing {
                    ReceiverKind::Value => {
                        if !self.is_copy_type(&expected) {
                            if let Some(place) = self.borrow_call_place(&argument.value) {
                                self.reject_overlapping_borrow(
                                    &borrowed_places,
                                    &place,
                                    ReceiverKind::Value,
                                    &param_decl.name,
                                    callee_name,
                                    argument.span,
                                )?;
                                borrowed_places.push(BorrowedCallPlace {
                                    path: place,
                                    passing: ReceiverKind::Value,
                                    param_name: param_decl.name.clone(),
                                    origin_span: argument.value.span,
                                });
                            }
                            for moved_access in nested_moved_accesses {
                                self.reject_overlapping_borrow(
                                    &borrowed_places,
                                    &moved_access.path,
                                    ReceiverKind::Value,
                                    &param_decl.name,
                                    callee_name,
                                    moved_access.origin_span,
                                )?;
                                borrowed_places.push(BorrowedCallPlace {
                                    path: moved_access.path,
                                    passing: ReceiverKind::Value,
                                    param_name: param_decl.name.clone(),
                                    origin_span: moved_access.origin_span,
                                });
                            }
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
                                origin_span: argument.value.span,
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
                            self.ensure_place_not_frozen(&place, argument.span, locals)?;
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
                                origin_span: argument.value.span,
                            });
                        }
                    }
                }
            }
        }

        self.invalidate_match_borrow_mut_bindings_for_borrowed_places(&borrowed_places, locals);

        Ok(resolved_return_type)
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
            Type::Named(name, args) if name == "SendError" && args.len() == 1 => Some(vec![
                ("Closed".to_string(), vec![args[0].clone()]),
                ("Cancelled".to_string(), vec![args[0].clone()]),
                ("TimedOut".to_string(), vec![args[0].clone()]),
                ("Full".to_string(), vec![args[0].clone()]),
            ]),
            Type::Named(name, args) if name == "QueueReceive" && args.len() == 1 => Some(vec![
                ("Item".to_string(), vec![args[0].clone()]),
                ("Closed".to_string(), Vec::new()),
                ("TimedOut".to_string(), Vec::new()),
                ("Cancelled".to_string(), Vec::new()),
            ]),
            Type::Named(name, args) if name == "TaskResult" && args.len() == 1 => Some(vec![
                ("Ready".to_string(), vec![args[0].clone()]),
                ("Error".to_string(), vec![Type::named("String")]),
                ("TimedOut".to_string(), Vec::new()),
                ("Cancelled".to_string(), Vec::new()),
            ]),
            Type::Named(name, args) if name == "WaitAny" && args.len() == 1 => Some(vec![
                (
                    "Ready".to_string(),
                    vec![Type::named("int32"), args[0].clone()],
                ),
                (
                    "Error".to_string(),
                    vec![Type::named("int32"), Type::named("String")],
                ),
                ("TimedOut".to_string(), Vec::new()),
                ("Cancelled".to_string(), Vec::new()),
            ]),
            Type::Named(name, args) if name == "WaitAll" && args.len() == 1 => Some(vec![
                (
                    "Ready".to_string(),
                    vec![Type::Named("Vec".to_string(), vec![args[0].clone()])],
                ),
                (
                    "Error".to_string(),
                    vec![Type::named("int32"), Type::named("String")],
                ),
                ("TimedOut".to_string(), Vec::new()),
                ("Cancelled".to_string(), Vec::new()),
            ]),
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
    ) -> Option<Vec<Type>> {
        let Type::Named(expected_name, args) = expected else {
            return None;
        };
        if expected_name != enum_name {
            return None;
        }
        match (enum_name, variant_name, args.as_slice()) {
            ("Option", "Some", [inner]) => Some(vec![inner.clone()]),
            ("Option", "None", [_]) => Some(Vec::new()),
            ("Result", "Ok", [ok, _err]) => Some(vec![ok.clone()]),
            ("Result", "Err", [_ok, err]) => Some(vec![err.clone()]),
            ("SendError", "Closed" | "Cancelled" | "TimedOut" | "Full", [value]) => {
                Some(vec![value.clone()])
            }
            ("QueueReceive", "Item", [value]) => Some(vec![value.clone()]),
            ("QueueReceive", "Closed" | "TimedOut" | "Cancelled", [_]) => Some(Vec::new()),
            ("TaskResult", "Ready", [value]) => Some(vec![value.clone()]),
            ("TaskResult", "Error", [_]) => Some(vec![Type::named("String")]),
            ("TaskResult", "TimedOut" | "Cancelled", [_]) => Some(Vec::new()),
            ("WaitAny", "Ready", [value]) => Some(vec![Type::named("int32"), value.clone()]),
            ("WaitAny", "Error", [_]) => Some(vec![Type::named("int32"), Type::named("String")]),
            ("WaitAny", "TimedOut" | "Cancelled", [_]) => Some(Vec::new()),
            ("WaitAll", "Ready", [values]) => Some(vec![values.clone()]),
            ("WaitAll", "Error", [_]) => Some(vec![Type::named("int32"), Type::named("String")]),
            ("WaitAll", "TimedOut" | "Cancelled", [_]) => Some(Vec::new()),
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
            "QueueReceive" => 1,
            "TaskResult" => 1,
            "WaitAny" => 1,
            "WaitAll" => 1,
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
            ExprKind::Name(name) => matches!(
                name.as_str(),
                "Option"
                    | "Result"
                    | "SendError"
                    | "QueueReceive"
                    | "TaskResult"
                    | "WaitAny"
                    | "WaitAll"
            ),
            ExprKind::Specialize { expr, .. } => self.is_builtin_enum_constructor_expr(expr),
            ExprKind::Group(inner) => self.is_builtin_enum_constructor_expr(inner),
            _ => false,
        }
    }

    fn is_builtin_enum_variant_name(&self, name: &str) -> bool {
        matches!(
            name,
            "Some"
                | "None"
                | "Ok"
                | "Err"
                | "Closed"
                | "Cancelled"
                | "TimedOut"
                | "Full"
                | "Item"
                | "Ready"
        )
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
        if variant_payload.is_empty() {
            if !args.is_empty() {
                return Err(Diagnostic::at(
                    span,
                    format!(
                        "variant `{}` of enum `{}` does not take a payload",
                        variant_name, enum_name
                    ),
                ));
            }
        } else {
            if args.len() != variant_payload.len() {
                return Err(Diagnostic::at(
                    span,
                    format!(
                        "variant `{}` of enum `{}` expects {} payload argument{}, found {}",
                        variant_name,
                        enum_name,
                        variant_payload.len(),
                        if variant_payload.len() == 1 { "" } else { "s" },
                        args.len()
                    ),
                ));
            }
            for (payload_ty, argument) in variant_payload.iter().zip(args) {
                let actual = self.type_of_expr_hint(&argument.value, locals, Some(payload_ty))?;
                if actual != *payload_ty {
                    return Err(Diagnostic::at(
                        argument.span,
                        format!(
                            "variant `{}` of enum `{}` expects `{}`, found `{}`",
                            variant_name, enum_name, payload_ty, actual
                        ),
                    ));
                }
                if !self.is_copy_type(payload_ty) {
                    self.consume_value_expr(&argument.value, locals)?;
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
        if is_builtin_io_resource_type(name, args) {
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
            return Err(Diagnostic::coded_at(
                "AU3002",
                span,
                format!(
                    "class `{}` cannot be used with `with` because it does not define `close(mut self)`",
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
                    "`with` resources must define `close(mut self)` returning `None`; `{}` does not",
                    name
                ),
            ));
        }

        Ok(())
    }

    fn require_task_startable_function(
        &self,
        function_name: &str,
        params: &[Param],
        param_passings: &[ReceiverKind],
        span: crate::diag::Span,
    ) -> Result<()> {
        if let Some(param) = params
            .iter()
            .zip(param_passings)
            .find_map(|(param, passing)| (*passing == ReceiverKind::BorrowMut).then_some(param))
        {
            return Err(Diagnostic::at(
                span,
                format!(
                    "task starting does not support `borrow mut` parameter `{}` on function `{}`; child tasks cannot write back through the starting call frame",
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
            let payload_names = payloads
                .iter()
                .map(|payload| {
                    payload.name.as_deref().ok_or_else(|| {
                        Diagnostic::at(
                            span,
                            format!(
                                "internal error: named enum payload metadata for `{}.{}` is missing its field name",
                                enum_name, variant_name
                            ),
                        )
                    })
                })
                .collect::<Result<BTreeSet<_>>>()?;
            if args.len() > payloads.len() {
                if let Some(extra) = args
                    .iter()
                    .filter_map(|argument| argument.name.as_deref())
                    .find(|name| !payload_names.contains(name))
                {
                    return Err(Diagnostic::at(
                        span,
                        format!(
                            "variant `{}` of enum `{}` has no payload named `{}`",
                            variant_name, enum_name, extra
                        ),
                    ));
                }
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
            for payload in payloads {
                let payload_name = payload.name.as_deref().expect(
                    "named enum payload metadata should have been validated before ordering",
                );
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
                ordered.push(argument);
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
