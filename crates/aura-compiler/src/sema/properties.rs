//! Structural Copy, clone, equality, task observation, and Transfer queries.

use super::{
    find_namespace_in_modules, integer_type_bounds_impl, substitute_type,
    substitutions_from_decl_type_args, BTreeMap, BTreeSet, ClassInfo, ClosureCaptureMode, EnumInfo,
    FunctionChecker, IntegerBounds, ModuleNamespace, Type,
};

#[derive(Clone, Debug)]
pub(super) enum TransferNominal {
    Class(ClassInfo),
    Enum(EnumInfo),
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(super) struct TransferSummary {
    pub(super) failure: Option<String>,
    /// Formal parameter indices paired with the first stored-component path
    /// that makes the parameter relevant to Transfer.
    pub(super) requirements: Vec<(usize, String)>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(super) struct TaskObservationSummary {
    /// A stored Task whose result is non-copy for every specialization.
    pub(super) unconditional_result: Option<Type>,
    /// Formals whose stored components may themselves contain a
    /// non-repeatable Task observation right.
    pub(super) containment_requirements: Vec<usize>,
    /// A formal whose non-copyness makes the paired Task result
    /// non-repeatable. The Type is the result template in nominal formals.
    pub(super) noncopy_requirements: Vec<(usize, Type)>,
}

#[derive(Clone, Debug, Default)]
pub(super) struct SymbolicCopyShape {
    pub(super) intrinsic_noncopy: bool,
    pub(super) noncopy_formals: Vec<usize>,
}

pub(super) fn is_builtin_copy_named_type(name: &str, args: &[Type]) -> bool {
    match name {
        "Queue" => args.len() == 1,
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

#[cfg(test)]
pub(super) fn type_is_copy_in_context(
    ty: &Type,
    classes: &BTreeMap<String, ClassInfo>,
    enums: &BTreeMap<String, EnumInfo>,
) -> bool {
    type_is_copy_in_context_inner(ty, classes, enums, None, None, &mut BTreeSet::new())
}

pub(super) fn type_is_copy_in_context_with_modules(
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

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(super) enum RngCloneSafety {
    Safe,
    ContainsRng,
    Unknown,
}

impl RngCloneSafety {
    pub(super) fn combine(self, other: Self) -> Self {
        match (self, other) {
            (Self::ContainsRng, _) | (_, Self::ContainsRng) => Self::ContainsRng,
            (Self::Unknown, _) | (_, Self::Unknown) => Self::Unknown,
            _ => Self::Safe,
        }
    }
}

pub(super) fn rng_clone_safety_in_context_with_modules(
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

pub(super) fn rng_clone_safety_in_context_inner(
    ty: &Type,
    classes: &BTreeMap<String, ClassInfo>,
    enums: &BTreeMap<String, EnumInfo>,
    imported_modules: &BTreeMap<String, ModuleNamespace>,
    module_registry: &BTreeMap<String, ModuleNamespace>,
    visiting: &mut BTreeSet<String>,
) -> RngCloneSafety {
    let Type::Named(name, args) = ty else {
        return match ty {
            Type::Union(union) => {
                union
                    .members
                    .iter()
                    .fold(RngCloneSafety::Safe, |safety, member| {
                        safety.combine(rng_clone_safety_in_context_inner(
                            member,
                            classes,
                            enums,
                            imported_modules,
                            module_registry,
                            visiting,
                        ))
                    })
            }
            Type::TypeParam(_) => RngCloneSafety::Unknown,
            Type::Unit
            | Type::Module(_)
            | Type::Function { .. }
            | Type::Callable(_)
            | Type::ReturnedView(_) => RngCloneSafety::Safe,
            Type::Closure { captures, .. } => {
                captures
                    .iter()
                    .fold(RngCloneSafety::Safe, |safety, capture| {
                        safety.combine(rng_clone_safety_in_context_inner(
                            &capture.ty,
                            classes,
                            enums,
                            imported_modules,
                            module_registry,
                            visiting,
                        ))
                    })
            }
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

pub(super) fn rng_clone_obligation_params_in_context_with_modules(
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

pub(super) fn collect_rng_clone_obligation_params_from_args(
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

pub(super) fn collect_rng_clone_obligation_params_in_context_inner(
    ty: &Type,
    classes: &BTreeMap<String, ClassInfo>,
    enums: &BTreeMap<String, EnumInfo>,
    imported_modules: &BTreeMap<String, ModuleNamespace>,
    module_registry: &BTreeMap<String, ModuleNamespace>,
    visiting: &mut BTreeSet<String>,
    params: &mut BTreeSet<String>,
) {
    match ty {
        Type::Union(union) => {
            for member in &union.members {
                collect_rng_clone_obligation_params_in_context_inner(
                    member,
                    classes,
                    enums,
                    imported_modules,
                    module_registry,
                    visiting,
                    params,
                );
            }
        }
        Type::TypeParam(name) => {
            params.insert(name.clone());
        }
        Type::Unit | Type::Module(_) | Type::Function { .. } | Type::ReturnedView(_) => {}
        Type::Callable(_) => {}
        Type::Closure { captures, .. } => {
            for capture in captures.iter() {
                collect_rng_clone_obligation_params_in_context_inner(
                    &capture.ty,
                    classes,
                    enums,
                    imported_modules,
                    module_registry,
                    visiting,
                    params,
                );
            }
        }
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

pub(super) fn copy_class_info_from_modules<'a>(
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

pub(super) fn find_copy_class_in_modules<'a>(
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

pub(super) fn copy_enum_info_from_modules<'a>(
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
            .or(namespace.all_enums.get(item_name));
    }

    let mut found = None;
    let mut ambiguous = false;
    find_copy_enum_in_modules(imported_modules, name, &mut found, &mut ambiguous);
    (!ambiguous).then_some(found).flatten()
}

pub(super) fn find_copy_enum_in_modules<'a>(
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

pub(super) fn type_is_copy_in_context_inner(
    ty: &Type,
    classes: &BTreeMap<String, ClassInfo>,
    enums: &BTreeMap<String, EnumInfo>,
    imported_modules: Option<&BTreeMap<String, ModuleNamespace>>,
    module_registry: Option<&BTreeMap<String, ModuleNamespace>>,
    visiting: &mut BTreeSet<String>,
) -> bool {
    match ty {
        // A union is Copy exactly when every member is Copy (ADR-0052 A6):
        // its storage is a tag plus one member payload.
        Type::Union(union) => union.members.iter().all(|member| {
            type_is_copy_in_context_inner(
                member,
                classes,
                enums,
                imported_modules,
                module_registry,
                visiting,
            )
        }),
        Type::Unit => true,
        Type::Module(_) => false,
        Type::ReturnedView(_) => false,
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
        Type::Function { .. } => true,
        Type::Closure { .. } | Type::Callable(_) => false,
        Type::Named(name, args) if name == "Task" && args.len() == 1 => {
            type_is_copy_in_context_inner(
                &args[0],
                classes,
                enums,
                imported_modules,
                module_registry,
                visiting,
            )
        }
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
        Type::Named(name, args) if name == "SelectOutcome" && args.len() == 2 => false,
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

pub(super) fn type_contains_named(ty: &Type, target: &str) -> bool {
    match ty {
        Type::Union(union) => union
            .members
            .iter()
            .any(|member| type_contains_named(member, target)),
        Type::Tuple(elements) => elements
            .iter()
            .any(|element| type_contains_named(element, target)),
        Type::Named(name, args) => {
            name == target || args.iter().any(|arg| type_contains_named(arg, target))
        }
        // A function field stores only a code pointer, not values of its
        // parameter or return types, so it cannot create recursive storage.
        Type::Function { .. } | Type::TypeParam(_) | Type::Module(_) | Type::Unit => false,
        Type::ReturnedView(view) => type_contains_named(&view.pointee, target),
        Type::Closure { captures, .. } => captures
            .iter()
            .any(|capture| type_contains_named(&capture.ty, target)),
        Type::Callable(callable) => {
            callable
                .params
                .iter()
                .any(|param| type_contains_named(&param.ty, target))
                || type_contains_named(&callable.return_type, target)
        }
    }
}

pub(super) fn type_contains_closure_value(ty: &Type) -> bool {
    match ty {
        Type::Union(union) => union.members.iter().any(type_contains_closure_value),
        Type::Closure { .. } => true,
        Type::Tuple(elements) | Type::Named(_, elements) => {
            elements.iter().any(type_contains_closure_value)
        }
        // Function parameter and return types describe calls; they are not
        // values stored inside the function pointer itself.
        Type::Callable(_) => false,
        Type::Function { .. }
        | Type::ReturnedView(_)
        | Type::TypeParam(_)
        | Type::Module(_)
        | Type::Unit => false,
    }
}

pub(super) fn type_contains_loan_closure(ty: &Type) -> bool {
    match ty {
        Type::Union(union) => union.members.iter().any(type_contains_loan_closure),
        Type::Callable(_) => false,
        Type::Closure { captures, .. } => captures.iter().any(|capture| {
            matches!(
                capture.mode,
                ClosureCaptureMode::SharedView | ClosureCaptureMode::MutableView
            )
        }),
        Type::Tuple(elements) | Type::Named(_, elements) => {
            elements.iter().any(type_contains_loan_closure)
        }
        Type::Function { .. }
        | Type::ReturnedView(_)
        | Type::TypeParam(_)
        | Type::Module(_)
        | Type::Unit => false,
    }
}

pub(super) fn type_reaches_class_through_non_indirect_fields(
    ty: &Type,
    target: &str,
    classes: &BTreeMap<String, ClassInfo>,
    visiting: &mut BTreeSet<String>,
) -> bool {
    match ty {
        Type::Union(union) => union.members.iter().any(|member| {
            type_reaches_class_through_non_indirect_fields(member, target, classes, visiting)
        }),
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
        Type::Function { .. }
        | Type::ReturnedView(_)
        | Type::TypeParam(_)
        | Type::Module(_)
        | Type::Unit => false,
        Type::Callable(_) => false,
        Type::Closure { captures, .. } => captures.iter().any(|capture| {
            type_reaches_class_through_non_indirect_fields(&capture.ty, target, classes, visiting)
        }),
    }
}

pub(super) fn is_builtin_type(name: &str) -> bool {
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
            | "str"
            | "Array"
            | "list"
            | "set"
            | "dict"
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
            | "SelectOutcome"
            | "TaskGroup"
            | "Duration"
    )
}

pub(super) fn preserves_qualified_builtin_type_name(type_name: &str) -> bool {
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

pub(super) fn is_integer_type(ty: &Type) -> bool {
    integer_type_bounds(ty).is_some()
}

pub(super) fn is_float_type(ty: &Type) -> bool {
    matches!(ty, Type::Named(name, args) if args.is_empty() && matches!(name.as_str(), "float32" | "float64"))
}

pub(super) fn is_string_type(ty: &Type) -> bool {
    matches!(ty, Type::Named(name, args) if name == "str" && args.is_empty())
}

pub(super) fn is_option_type(ty: &Type) -> bool {
    matches!(ty, Type::Named(name, args) if name == "Option" && args.len() == 1)
}

pub(super) fn is_numeric_type(ty: &Type) -> bool {
    is_integer_type(ty) || is_float_type(ty)
}

pub(super) fn is_array_dtype(ty: &Type) -> bool {
    matches!(
        ty,
        Type::Named(name, args)
            if args.is_empty()
                && matches!(name.as_str(), "int32" | "int64" | "float32" | "float64")
    )
}

pub(super) fn vec_element_type(ty: &Type) -> Option<&Type> {
    match ty {
        Type::Named(name, args) if name == "list" && args.len() == 1 => Some(&args[0]),
        _ => None,
    }
}

pub(super) fn array_element_type(ty: &Type) -> Option<&Type> {
    match ty {
        Type::Named(name, args) if name == "Array" && args.len() == 1 => Some(&args[0]),
        _ => None,
    }
}

pub(super) fn set_element_type(ty: &Type) -> Option<&Type> {
    match ty {
        Type::Named(name, args) if name == "set" && args.len() == 1 => Some(&args[0]),
        _ => None,
    }
}

pub(super) fn map_key_value_types(ty: &Type) -> Option<(&Type, &Type)> {
    match ty {
        Type::Named(name, args) if name == "dict" && args.len() == 2 => Some((&args[0], &args[1])),
        _ => None,
    }
}

pub(super) fn is_builtin_io_resource_type(name: &str, args: &[Type]) -> bool {
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

impl<'a> FunctionChecker<'a> {
    pub(super) fn is_copy_type(&self, ty: &Type) -> bool {
        type_is_copy_in_context_with_modules(
            ty,
            self.classes,
            self.enums,
            self.imported_modules,
            self.module_registry,
        )
    }

    /// Returns the first structural reason `ty` cannot cross a task or queue
    /// boundary. `None` means the compiler derived `Transfer` for the whole
    /// value. Transfer is intentionally not represented as a user trait:
    /// every accepted case below is a compiler-known value shape.
    pub(super) fn transfer_failure(&self, ty: &Type) -> Option<String> {
        let mut nominals = BTreeMap::new();
        self.collect_transfer_nominals(ty, &mut nominals);
        let mut summaries = nominals
            .keys()
            .map(|key| (key.clone(), TransferSummary::default()))
            .collect::<BTreeMap<_, _>>();

        // Transfer is a conjunction-only structural property. Each nominal
        // summary can only gain an intrinsic failure or a dependency on one
        // of its finitely many formal parameters, so this least fixed point
        // terminates even when recursive specializations keep changing.
        loop {
            let mut changed = false;
            for (key, nominal) in &nominals {
                let derived = self.transfer_nominal_summary(nominal, &summaries);
                let current = summaries
                    .get_mut(key)
                    .expect("every discovered Transfer nominal has a summary");
                changed |= Self::merge_transfer_summary(current, derived);
            }
            if !changed {
                break;
            }
        }

        self.transfer_shape(ty, &BTreeMap::new(), &summaries)
            .failure
    }

    pub(super) fn collect_transfer_nominals(
        &self,
        ty: &Type,
        nominals: &mut BTreeMap<String, TransferNominal>,
    ) {
        let Type::Named(name, args) = ty else {
            match ty {
                Type::Tuple(elements) => {
                    for element in elements {
                        self.collect_transfer_nominals(element, nominals);
                    }
                }
                Type::Closure { captures, .. } => {
                    for capture in captures.iter() {
                        self.collect_transfer_nominals(&capture.ty, nominals);
                    }
                }
                _ => {}
            }
            return;
        };
        for arg in args {
            self.collect_transfer_nominals(arg, nominals);
        }
        if let Some(class_info) = self.classes.get(name).cloned().or_else(|| {
            copy_class_info_from_modules(name, self.imported_modules, self.module_registry).cloned()
        }) {
            if class_info.is_builtin {
                return;
            }
            let key = format!("class:{}:{}", class_info.module_name, class_info.decl.name);
            if nominals
                .insert(key.clone(), TransferNominal::Class(class_info.clone()))
                .is_some()
            {
                return;
            }
            for field in class_info.fields.values() {
                self.collect_transfer_nominals(&field.ty, nominals);
            }
            return;
        }
        if let Some(enum_info) = self.enums.get(name).cloned().or_else(|| {
            copy_enum_info_from_modules(name, self.imported_modules, self.module_registry).cloned()
        }) {
            let key = format!("enum:{}:{}", enum_info.module_name, enum_info.decl.name);
            if nominals
                .insert(key, TransferNominal::Enum(enum_info.clone()))
                .is_some()
            {
                return;
            }
            for variant in enum_info.variants.values() {
                for payload in &variant.payloads {
                    self.collect_transfer_nominals(&payload.ty, nominals);
                }
            }
        }
    }

    pub(super) fn transfer_nominal_summary(
        &self,
        nominal: &TransferNominal,
        summaries: &BTreeMap<String, TransferSummary>,
    ) -> TransferSummary {
        match nominal {
            TransferNominal::Class(class_info) => {
                let formals = class_info
                    .decl
                    .type_params
                    .iter()
                    .enumerate()
                    .map(|(index, name)| (name.clone(), index))
                    .collect::<BTreeMap<_, _>>();
                let mut result = TransferSummary::default();
                for field_decl in &class_info.decl.fields {
                    let Some(field) = class_info.fields.get(&field_decl.name) else {
                        continue;
                    };
                    let field_summary = Self::prefix_transfer_summary(
                        self.transfer_shape(&field.ty, &formals, summaries),
                        &format!("field `{}` of `{}`", field_decl.name, class_info.decl.name),
                    );
                    Self::merge_transfer_summary(&mut result, field_summary);
                    if result.failure.is_some() {
                        break;
                    }
                }
                result
            }
            TransferNominal::Enum(enum_info) => {
                let formals = enum_info
                    .decl
                    .type_params
                    .iter()
                    .enumerate()
                    .map(|(index, name)| (name.clone(), index))
                    .collect::<BTreeMap<_, _>>();
                let mut result = TransferSummary::default();
                for variant_decl in &enum_info.decl.variants {
                    let Some(variant) = enum_info.variants.get(&variant_decl.name) else {
                        continue;
                    };
                    for (index, payload) in variant.payloads.iter().enumerate() {
                        let payload_label = payload
                            .name
                            .as_ref()
                            .map(|name| format!("payload `{name}`"))
                            .unwrap_or_else(|| format!("payload {}", index + 1));
                        let payload_summary = Self::prefix_transfer_summary(
                            self.transfer_shape(&payload.ty, &formals, summaries),
                            &format!(
                                "variant `{}` of `{}` -> {payload_label}",
                                variant_decl.name, enum_info.decl.name
                            ),
                        );
                        Self::merge_transfer_summary(&mut result, payload_summary);
                        if result.failure.is_some() {
                            return result;
                        }
                    }
                }
                result
            }
        }
    }

    pub(super) fn transfer_shape(
        &self,
        ty: &Type,
        formals: &BTreeMap<String, usize>,
        summaries: &BTreeMap<String, TransferSummary>,
    ) -> TransferSummary {
        if self.is_copy_type(ty) {
            return TransferSummary::default();
        }
        if self.is_opaque_handle_type(ty) {
            return TransferSummary {
                failure: Some(format!(
                    "`{ty}` is an opaque FFI handle and is not Transfer"
                )),
                requirements: Vec::new(),
            };
        }
        match ty {
            Type::Union(union) => {
                let mut result = TransferSummary::default();
                for member in &union.members {
                    let member_summary = Self::prefix_transfer_summary(
                        self.transfer_shape(member, formals, summaries),
                        &format!("member `{member}` of `{ty}`"),
                    );
                    Self::merge_transfer_summary(&mut result, member_summary);
                    if result.failure.is_some() {
                        break;
                    }
                }
                result
            }
            Type::Unit | Type::Function { .. } | Type::ReturnedView(_) => {
                TransferSummary::default()
            }
            Type::Callable(callable) => {
                if callable.task {
                    return TransferSummary::default();
                }
                TransferSummary {
                    failure: Some(
                        "an erased `Callable` hides its environment and is not Transfer; pack a `TaskCallable[...]` to prove its captures"
                            .to_string(),
                    ),
                    ..TransferSummary::default()
                }
            }
            Type::Closure { captures, .. } => {
                if let Some(capture) = captures.iter().find(|capture| {
                    matches!(
                        capture.mode,
                        ClosureCaptureMode::SharedView | ClosureCaptureMode::MutableView
                    )
                }) {
                    return TransferSummary {
                        failure: Some(format!(
                            "capture `{}` is a live {} loan and is not Transfer",
                            capture.name,
                            if capture.mode == ClosureCaptureMode::MutableView {
                                "mutable"
                            } else {
                                "shared"
                            }
                        )),
                        requirements: Vec::new(),
                    };
                }
                let mut result = TransferSummary::default();
                for capture in captures.iter() {
                    let capture_summary = Self::prefix_transfer_summary(
                        self.transfer_shape(&capture.ty, formals, summaries),
                        &format!("capture `{}` of `{ty}`", capture.name),
                    );
                    Self::merge_transfer_summary(&mut result, capture_summary);
                    if result.failure.is_some() {
                        break;
                    }
                }
                result
            }
            Type::Module(name) => TransferSummary {
                failure: Some(format!(
                    "`module {name}` is a module capability and is not Transfer"
                )),
                requirements: Vec::new(),
            },
            Type::TypeParam(name) => match formals.get(name) {
                Some(index) => TransferSummary {
                    failure: None,
                    requirements: vec![(*index, String::new())],
                },
                None => TransferSummary {
                    failure: Some(format!(
                        "type parameter `{name}` has no compiler-proven Transfer specialization"
                    )),
                    requirements: Vec::new(),
                },
            },
            Type::Tuple(elements) => {
                let mut result = TransferSummary::default();
                for (index, element) in elements.iter().enumerate() {
                    let element_summary = Self::prefix_transfer_summary(
                        self.transfer_shape(element, formals, summaries),
                        &format!("element {} of `{ty}`", index + 1),
                    );
                    Self::merge_transfer_summary(&mut result, element_summary);
                    if result.failure.is_some() {
                        break;
                    }
                }
                result
            }
            Type::Named(name, args) if name == "str" && args.is_empty() => {
                TransferSummary::default()
            }
            // Range keeps its established move-only source semantics, but its
            // runtime representation is owned start/end data with no host
            // authority, so it may cross task and Queue boundaries.
            Type::Named(name, args) if name == "Range" && args.is_empty() => {
                TransferSummary::default()
            }
            Type::Named(name, args)
                if matches!(name.as_str(), "Queue" | "Task") && args.len() == 1 =>
            {
                TransferSummary::default()
            }
            Type::Named(name, args)
                if matches!(name.as_str(), "list" | "set" | "Array") && args.len() == 1 =>
            {
                Self::prefix_transfer_summary(
                    self.transfer_shape(&args[0], formals, summaries),
                    &format!("element of `{ty}`"),
                )
            }
            Type::Named(name, args) if name == "dict" && args.len() == 2 => {
                let mut result = Self::prefix_transfer_summary(
                    self.transfer_shape(&args[0], formals, summaries),
                    &format!("key of `{ty}`"),
                );
                if result.failure.is_none() {
                    Self::merge_transfer_summary(
                        &mut result,
                        Self::prefix_transfer_summary(
                            self.transfer_shape(&args[1], formals, summaries),
                            &format!("value of `{ty}`"),
                        ),
                    );
                }
                result
            }
            Type::Named(name, args)
                if matches!(
                    name.as_str(),
                    "Option" | "SendError" | "QueueReceive" | "TaskResult" | "WaitAny" | "WaitAll"
                ) && args.len() == 1 =>
            {
                Self::prefix_transfer_summary(
                    self.transfer_shape(&args[0], formals, summaries),
                    &format!("payload of `{ty}`"),
                )
            }
            Type::Named(name, args) if name == "Result" && args.len() == 2 => {
                let mut result = Self::prefix_transfer_summary(
                    self.transfer_shape(&args[0], formals, summaries),
                    &format!("success payload of `{ty}`"),
                );
                if result.failure.is_none() {
                    Self::merge_transfer_summary(
                        &mut result,
                        Self::prefix_transfer_summary(
                            self.transfer_shape(&args[1], formals, summaries),
                            &format!("error payload of `{ty}`"),
                        ),
                    );
                }
                result
            }
            Type::Named(name, args) if name == "SelectOutcome" && args.len() == 2 => {
                let mut result = Self::prefix_transfer_summary(
                    self.transfer_shape(&args[0], formals, summaries),
                    &format!("queue payload of `{ty}`"),
                );
                if result.failure.is_none() {
                    Self::merge_transfer_summary(
                        &mut result,
                        Self::prefix_transfer_summary(
                            self.transfer_shape(&args[1], formals, summaries),
                            &format!("task payload of `{ty}`"),
                        ),
                    );
                }
                result
            }
            Type::Named(name, args) if name == "random.Rng" && args.is_empty() => TransferSummary {
                failure: Some(
                    "`random.Rng` is a stateful generator and is not Transfer".to_string(),
                ),
                requirements: Vec::new(),
            },
            Type::Named(name, args)
                if args.is_empty()
                    && matches!(
                        name.as_str(),
                        "process.Completed" | "net.HttpResponse" | "net.UdpDatagram"
                    ) =>
            {
                TransferSummary::default()
            }
            Type::Named(name, args) if is_builtin_io_resource_type(name, args) => TransferSummary {
                failure: Some(format!("`{ty}` is a host resource and is not Transfer")),
                requirements: Vec::new(),
            },
            Type::Named(name, args) => {
                let nominal = if let Some(class_info) =
                    self.classes.get(name).cloned().or_else(|| {
                        copy_class_info_from_modules(
                            name,
                            self.imported_modules,
                            self.module_registry,
                        )
                        .cloned()
                    }) {
                    Some((
                        format!("class:{}:{}", class_info.module_name, class_info.decl.name),
                        class_info.decl.type_params.len(),
                        class_info.is_builtin,
                    ))
                } else {
                    self.enums
                        .get(name)
                        .cloned()
                        .or_else(|| {
                            copy_enum_info_from_modules(
                                name,
                                self.imported_modules,
                                self.module_registry,
                            )
                            .cloned()
                        })
                        .map(|enum_info| {
                            (
                                format!("enum:{}:{}", enum_info.module_name, enum_info.decl.name),
                                enum_info.decl.type_params.len(),
                                false,
                            )
                        })
                };
                // Type lowering has already rejected unknown nominal names,
                // unwhitelisted builtin classes, and generic arity mismatches.
                // Transfer analysis therefore operates only on validated
                // structural types instead of preserving unreachable fallback
                // diagnostics that no Aura program can observe.
                let (key, arity, builtin) =
                    nominal.expect("validated named type must have a structural definition");
                assert!(
                    !builtin,
                    "validated builtin type must use an earlier Transfer case"
                );
                assert_eq!(
                    args.len(),
                    arity,
                    "validated named type must have exact arity"
                );
                let callee = summaries.get(&key).cloned().unwrap_or_default();
                if callee.failure.is_some() {
                    return callee;
                }
                let mut result = TransferSummary::default();
                for (index, witness) in callee.requirements {
                    let applied = Self::prefix_transfer_summary(
                        self.transfer_shape(&args[index], formals, summaries),
                        &witness,
                    );
                    Self::merge_transfer_summary(&mut result, applied);
                    if result.failure.is_some() {
                        break;
                    }
                }
                result
            }
        }
    }

    pub(super) fn prefix_transfer_summary(
        mut summary: TransferSummary,
        prefix: &str,
    ) -> TransferSummary {
        let join = |suffix: &str| {
            if prefix.is_empty() {
                suffix.to_string()
            } else if suffix.is_empty() {
                prefix.to_string()
            } else {
                format!("{prefix} -> {suffix}")
            }
        };
        summary.failure = summary.failure.map(|failure| join(&failure));
        for (_, witness) in &mut summary.requirements {
            *witness = join(witness);
        }
        summary
    }

    pub(super) fn merge_transfer_summary(
        current: &mut TransferSummary,
        incoming: TransferSummary,
    ) -> bool {
        let mut changed = false;
        if current.failure.is_none() {
            if let Some(failure) = incoming.failure {
                current.failure = Some(failure);
                changed = true;
            }
        }
        for (index, witness) in incoming.requirements {
            if current
                .requirements
                .iter()
                .all(|(existing, _)| *existing != index)
            {
                current.requirements.push((index, witness));
                changed = true;
            }
        }
        changed
    }

    pub(super) fn rng_clone_safety(&self, ty: &Type) -> RngCloneSafety {
        rng_clone_safety_in_context_with_modules(
            ty,
            self.classes,
            self.enums,
            self.imported_modules,
            self.module_registry,
        )
    }

    pub(super) fn non_cloneable_rng_reason(ty: &Type) -> String {
        if matches!(ty, Type::Named(name, args) if name == "random.Rng" && args.is_empty()) {
            format!("`{ty}` is directly non-cloneable")
        } else {
            format!("`{ty}` contains non-cloneable `random.Rng` state")
        }
    }

    pub(super) fn task_observation_nominal_summary(
        &self,
        nominal: &TransferNominal,
        summaries: &BTreeMap<String, TaskObservationSummary>,
    ) -> TaskObservationSummary {
        match nominal {
            TransferNominal::Class(class_info) => {
                let formals = class_info
                    .decl
                    .type_params
                    .iter()
                    .enumerate()
                    .map(|(index, name)| (name.clone(), index))
                    .collect::<BTreeMap<_, _>>();
                let mut result = TaskObservationSummary::default();
                for field_decl in &class_info.decl.fields {
                    let Some(field) = class_info.fields.get(&field_decl.name) else {
                        continue;
                    };
                    let field_summary = self.task_observation_shape(&field.ty, &formals, summaries);
                    Self::merge_task_observation_summary(&mut result, field_summary);
                    if result.unconditional_result.is_some() {
                        break;
                    }
                }
                result
            }
            TransferNominal::Enum(enum_info) => {
                let formals = enum_info
                    .decl
                    .type_params
                    .iter()
                    .enumerate()
                    .map(|(index, name)| (name.clone(), index))
                    .collect::<BTreeMap<_, _>>();
                let mut result = TaskObservationSummary::default();
                for variant_decl in &enum_info.decl.variants {
                    let Some(variant) = enum_info.variants.get(&variant_decl.name) else {
                        continue;
                    };
                    for payload in &variant.payloads {
                        let payload_summary =
                            self.task_observation_shape(&payload.ty, &formals, summaries);
                        Self::merge_task_observation_summary(&mut result, payload_summary);
                        if result.unconditional_result.is_some() {
                            return result;
                        }
                    }
                }
                result
            }
        }
    }

    pub(super) fn task_observation_shape(
        &self,
        ty: &Type,
        formals: &BTreeMap<String, usize>,
        summaries: &BTreeMap<String, TaskObservationSummary>,
    ) -> TaskObservationSummary {
        match ty {
            Type::Union(union) => {
                let mut result = TaskObservationSummary::default();
                for member in &union.members {
                    Self::merge_task_observation_summary(
                        &mut result,
                        self.task_observation_shape(member, formals, summaries),
                    );
                }
                result
            }
            Type::Unit
            | Type::Module(_)
            | Type::Function { .. }
            | Type::Callable(_)
            | Type::ReturnedView(_) => TaskObservationSummary::default(),
            Type::Closure { captures, .. } => {
                let mut result = TaskObservationSummary::default();
                for capture in captures.iter() {
                    let capture_summary =
                        self.task_observation_shape(&capture.ty, formals, summaries);
                    Self::merge_task_observation_summary(&mut result, capture_summary);
                }
                result
            }
            Type::TypeParam(name) => {
                formals
                    .get(name)
                    .map_or_else(TaskObservationSummary::default, |index| {
                        TaskObservationSummary {
                            unconditional_result: None,
                            containment_requirements: vec![*index],
                            noncopy_requirements: Vec::new(),
                        }
                    })
            }
            Type::Tuple(elements) => {
                let mut result = TaskObservationSummary::default();
                for element in elements {
                    let element_summary = self.task_observation_shape(element, formals, summaries);
                    Self::merge_task_observation_summary(&mut result, element_summary);
                    if result.unconditional_result.is_some() {
                        break;
                    }
                }
                result
            }
            Type::Named(name, args) if name == "Queue" && args.len() == 1 => {
                TaskObservationSummary::default()
            }
            Type::Named(name, args) if name == "Task" && args.len() == 1 => {
                let copy_shape = self.symbolic_copy_shape(&args[0], formals, &mut BTreeSet::new());
                if copy_shape.intrinsic_noncopy {
                    TaskObservationSummary {
                        unconditional_result: Some(args[0].clone()),
                        containment_requirements: Vec::new(),
                        noncopy_requirements: Vec::new(),
                    }
                } else {
                    TaskObservationSummary {
                        unconditional_result: None,
                        containment_requirements: Vec::new(),
                        noncopy_requirements: copy_shape
                            .noncopy_formals
                            .into_iter()
                            .map(|index| (index, args[0].clone()))
                            .collect(),
                    }
                }
            }
            Type::Named(name, args)
                if matches!(
                    name.as_str(),
                    "list"
                        | "set"
                        | "dict"
                        | "Option"
                        | "Result"
                        | "SendError"
                        | "QueueReceive"
                        | "TaskResult"
                        | "WaitAny"
                        | "WaitAll"
                        | "SelectOutcome"
                ) =>
            {
                let mut result = TaskObservationSummary::default();
                for arg in args {
                    let arg_summary = self.task_observation_shape(arg, formals, summaries);
                    Self::merge_task_observation_summary(&mut result, arg_summary);
                    if result.unconditional_result.is_some() {
                        break;
                    }
                }
                result
            }
            Type::Named(name, args) => {
                let (key, params) = if let Some(class_info) =
                    self.classes.get(name).cloned().or_else(|| {
                        copy_class_info_from_modules(
                            name,
                            self.imported_modules,
                            self.module_registry,
                        )
                        .cloned()
                    }) {
                    if class_info.is_builtin || args.len() != class_info.decl.type_params.len() {
                        return TaskObservationSummary::default();
                    }
                    (
                        format!("class:{}:{}", class_info.module_name, class_info.decl.name),
                        class_info.decl.type_params,
                    )
                } else if let Some(enum_info) = self.enums.get(name).cloned().or_else(|| {
                    copy_enum_info_from_modules(name, self.imported_modules, self.module_registry)
                        .cloned()
                }) {
                    if args.len() != enum_info.decl.type_params.len() {
                        return TaskObservationSummary::default();
                    }
                    (
                        format!("enum:{}:{}", enum_info.module_name, enum_info.decl.name),
                        enum_info.decl.type_params,
                    )
                } else {
                    return TaskObservationSummary::default();
                };
                let substitutions = substitutions_from_decl_type_args(&params, args);
                let callee = summaries.get(&key).cloned().unwrap_or_default();
                let mut result = TaskObservationSummary::default();
                if let Some(template) = callee.unconditional_result {
                    result.unconditional_result = Some(substitute_type(&template, &substitutions));
                    return result;
                }
                for index in callee.containment_requirements {
                    let arg_summary = self.task_observation_shape(&args[index], formals, summaries);
                    Self::merge_task_observation_summary(&mut result, arg_summary);
                    if result.unconditional_result.is_some() {
                        return result;
                    }
                }
                for (index, template) in callee.noncopy_requirements {
                    let copy_shape =
                        self.symbolic_copy_shape(&args[index], formals, &mut BTreeSet::new());
                    let applied_template = substitute_type(&template, &substitutions);
                    if copy_shape.intrinsic_noncopy {
                        result.unconditional_result = Some(applied_template);
                        return result;
                    }
                    for formal in copy_shape.noncopy_formals {
                        if result
                            .noncopy_requirements
                            .iter()
                            .all(|(existing, _)| *existing != formal)
                        {
                            result
                                .noncopy_requirements
                                .push((formal, applied_template.clone()));
                        }
                    }
                }
                result
            }
        }
    }

    pub(super) fn symbolic_copy_shape(
        &self,
        ty: &Type,
        formals: &BTreeMap<String, usize>,
        visiting: &mut BTreeSet<String>,
    ) -> SymbolicCopyShape {
        match ty {
            Type::Union(union) => {
                self.combine_symbolic_copy_shapes(union.members.iter(), formals, visiting)
            }
            Type::Unit | Type::Function { .. } | Type::ReturnedView(_) => {
                SymbolicCopyShape::default()
            }
            Type::Closure { .. } | Type::Callable(_) => SymbolicCopyShape {
                intrinsic_noncopy: true,
                noncopy_formals: Vec::new(),
            },
            Type::Module(_) => SymbolicCopyShape {
                intrinsic_noncopy: true,
                noncopy_formals: Vec::new(),
            },
            Type::TypeParam(name) => formals.get(name).map_or(
                SymbolicCopyShape {
                    intrinsic_noncopy: true,
                    noncopy_formals: Vec::new(),
                },
                |index| SymbolicCopyShape {
                    intrinsic_noncopy: false,
                    noncopy_formals: vec![*index],
                },
            ),
            Type::Tuple(elements) => {
                self.combine_symbolic_copy_shapes(elements.iter(), formals, visiting)
            }
            Type::Named(name, args) if is_builtin_copy_named_type(name, args) => {
                SymbolicCopyShape::default()
            }
            Type::Named(name, args)
                if matches!(
                    name.as_str(),
                    "Task" | "Option" | "SendError" | "QueueReceive"
                ) && args.len() == 1 =>
            {
                self.symbolic_copy_shape(&args[0], formals, visiting)
            }
            Type::Named(name, args) if name == "Result" && args.len() == 2 => {
                self.combine_symbolic_copy_shapes(args.iter(), formals, visiting)
            }
            Type::Named(name, args) => {
                let key = ty.to_string();
                if !visiting.insert(key.clone()) {
                    return SymbolicCopyShape {
                        intrinsic_noncopy: true,
                        noncopy_formals: Vec::new(),
                    };
                }
                let result = if let Some(class_info) =
                    self.classes.get(name).cloned().or_else(|| {
                        copy_class_info_from_modules(
                            name,
                            self.imported_modules,
                            self.module_registry,
                        )
                        .cloned()
                    }) {
                    if !class_info.decl.copy || args.len() != class_info.decl.type_params.len() {
                        SymbolicCopyShape {
                            intrinsic_noncopy: true,
                            noncopy_formals: Vec::new(),
                        }
                    } else {
                        self.combine_symbolic_copy_shapes(args.iter(), formals, visiting)
                    }
                } else if let Some(enum_info) = self.enums.get(name).cloned().or_else(|| {
                    copy_enum_info_from_modules(name, self.imported_modules, self.module_registry)
                        .cloned()
                }) {
                    assert_eq!(
                        args.len(),
                        enum_info.decl.type_params.len(),
                        "validated enum type must have exact arity"
                    );
                    let substitutions =
                        substitutions_from_decl_type_args(&enum_info.decl.type_params, args);
                    let payload_types = enum_info
                        .decl
                        .variants
                        .iter()
                        .filter_map(|variant_decl| enum_info.variants.get(&variant_decl.name))
                        .flat_map(|variant| variant.payloads.iter())
                        .map(|payload| substitute_type(&payload.ty, &substitutions))
                        .collect::<Vec<_>>();
                    self.combine_symbolic_copy_shapes(payload_types.iter(), formals, visiting)
                } else {
                    SymbolicCopyShape {
                        intrinsic_noncopy: true,
                        noncopy_formals: Vec::new(),
                    }
                };
                visiting.remove(&key);
                result
            }
        }
    }

    pub(super) fn combine_symbolic_copy_shapes<'b>(
        &self,
        types: impl Iterator<Item = &'b Type>,
        formals: &BTreeMap<String, usize>,
        visiting: &mut BTreeSet<String>,
    ) -> SymbolicCopyShape {
        let mut result = SymbolicCopyShape::default();
        for ty in types {
            let child = self.symbolic_copy_shape(ty, formals, visiting);
            result.intrinsic_noncopy |= child.intrinsic_noncopy;
            for formal in child.noncopy_formals {
                if !result.noncopy_formals.contains(&formal) {
                    result.noncopy_formals.push(formal);
                }
            }
        }
        result
    }

    pub(super) fn merge_task_observation_summary(
        current: &mut TaskObservationSummary,
        incoming: TaskObservationSummary,
    ) -> bool {
        let mut changed = false;
        if current.unconditional_result.is_none() {
            if let Some(result) = incoming.unconditional_result {
                current.unconditional_result = Some(result);
                changed = true;
            }
        }
        for index in incoming.containment_requirements {
            if !current.containment_requirements.contains(&index) {
                current.containment_requirements.push(index);
                changed = true;
            }
        }
        for (index, template) in incoming.noncopy_requirements {
            if current
                .noncopy_requirements
                .iter()
                .all(|(existing, _)| *existing != index)
            {
                current.noncopy_requirements.push((index, template));
                changed = true;
            }
        }
        changed
    }

    /// Returns the first callable runtime value structurally contained in
    /// `ty`. Callable signatures describe code, not a value relation: neither
    /// function pointers nor closure environments acquire identity equality
    /// merely because they are stored inside another type.
    pub(super) fn callable_in_equality_type(&self, ty: &Type) -> Option<Type> {
        self.callable_in_equality_type_inner(ty, &mut BTreeSet::new())
    }

    pub(super) fn callable_in_equality_type_inner(
        &self,
        ty: &Type,
        visiting: &mut BTreeSet<String>,
    ) -> Option<Type> {
        match ty {
            Type::Union(union) => union
                .members
                .iter()
                .find_map(|member| self.callable_in_equality_type_inner(member, visiting)),
            Type::Function { .. } | Type::Closure { .. } | Type::Callable(_) => Some(ty.clone()),
            Type::Tuple(elements) => elements
                .iter()
                .find_map(|element| self.callable_in_equality_type_inner(element, visiting)),
            Type::Named(name, args) => {
                if let Some(class_info) = self.resolve_class_info(name) {
                    debug_assert_eq!(args.len(), class_info.decl.type_params.len());
                    let key = format!("class:{}:{}", class_info.module_name, class_info.decl.name);
                    if !visiting.insert(key.clone()) {
                        return None;
                    }
                    let substitutions =
                        substitutions_from_decl_type_args(&class_info.decl.type_params, args);
                    let callable = class_info.fields.values().find_map(|field| {
                        let field_ty = substitute_type(&field.ty, &substitutions);
                        self.callable_in_equality_type_inner(&field_ty, visiting)
                    });
                    visiting.remove(&key);
                    return callable;
                }

                if let Some(enum_info) = self.resolve_enum_info(name) {
                    debug_assert_eq!(args.len(), enum_info.decl.type_params.len());
                    let key = format!("enum:{}:{}", enum_info.module_name, enum_info.decl.name);
                    if !visiting.insert(key.clone()) {
                        return None;
                    }
                    let substitutions =
                        substitutions_from_decl_type_args(&enum_info.decl.type_params, args);
                    let callable = enum_info
                        .variants
                        .values()
                        .flat_map(|variant| &variant.payloads)
                        .find_map(|payload| {
                            let payload_ty = substitute_type(&payload.ty, &substitutions);
                            self.callable_in_equality_type_inner(&payload_ty, visiting)
                        });
                    visiting.remove(&key);
                    return callable;
                }

                args.iter()
                    .find_map(|arg| self.callable_in_equality_type_inner(arg, visiting))
            }
            Type::ReturnedView(_) | Type::TypeParam(_) | Type::Module(_) | Type::Unit => None,
        }
    }

    /// Returns the first structurally contained Array whose lack of equality
    /// makes `ty` unavailable to equality-bearing operations.
    pub(super) fn array_in_equality_type(&self, ty: &Type) -> Option<Type> {
        self.array_in_equality_type_inner(ty, &mut BTreeSet::new())
    }

    pub(super) fn array_in_equality_type_inner(
        &self,
        ty: &Type,
        visiting: &mut BTreeSet<String>,
    ) -> Option<Type> {
        match ty {
            Type::Union(union) => union
                .members
                .iter()
                .find_map(|member| self.array_in_equality_type_inner(member, visiting)),
            Type::Tuple(elements) => elements
                .iter()
                .find_map(|element| self.array_in_equality_type_inner(element, visiting)),
            Type::Named(name, args) if name == "Array" && args.len() == 1 => Some(ty.clone()),
            Type::Named(name, args) => {
                if let Some(class_info) = self.resolve_class_info(name) {
                    debug_assert_eq!(args.len(), class_info.decl.type_params.len());
                    let key = format!("class:{}:{}", class_info.module_name, class_info.decl.name);
                    if !visiting.insert(key.clone()) {
                        return None;
                    }
                    let substitutions =
                        substitutions_from_decl_type_args(&class_info.decl.type_params, args);
                    let array = class_info.fields.values().find_map(|field| {
                        let field_ty = substitute_type(&field.ty, &substitutions);
                        self.array_in_equality_type_inner(&field_ty, visiting)
                    });
                    visiting.remove(&key);
                    return array;
                }

                if let Some(enum_info) = self.resolve_enum_info(name) {
                    debug_assert_eq!(args.len(), enum_info.decl.type_params.len());
                    let key = format!("enum:{}:{}", enum_info.module_name, enum_info.decl.name);
                    if !visiting.insert(key.clone()) {
                        return None;
                    }
                    let substitutions =
                        substitutions_from_decl_type_args(&enum_info.decl.type_params, args);
                    let array = enum_info
                        .variants
                        .values()
                        .flat_map(|variant| &variant.payloads)
                        .find_map(|payload| {
                            let payload_ty = substitute_type(&payload.ty, &substitutions);
                            self.array_in_equality_type_inner(&payload_ty, visiting)
                        });
                    visiting.remove(&key);
                    return array;
                }

                args.iter()
                    .find_map(|arg| self.array_in_equality_type_inner(arg, visiting))
            }
            // Callable equality has its own dedicated diagnostic. Function
            // parameter and result types are contracts rather than retained
            // runtime values, while generic equality obligations are enforced
            // when concrete substitutions are available.
            Type::Closure { .. }
            | Type::Callable(_)
            | Type::ReturnedView(_)
            | Type::Function { .. }
            | Type::TypeParam(_)
            | Type::Module(_)
            | Type::Unit => None,
        }
    }

    pub(super) fn array_equality_type_params(&self, ty: &Type) -> BTreeSet<String> {
        let mut params = BTreeSet::new();
        self.collect_array_equality_type_params_inner(ty, &mut BTreeSet::new(), &mut params);
        params
    }

    pub(super) fn collect_array_equality_type_params_inner(
        &self,
        ty: &Type,
        visiting: &mut BTreeSet<String>,
        params: &mut BTreeSet<String>,
    ) {
        match ty {
            Type::Union(union) => {
                for member in &union.members {
                    self.collect_array_equality_type_params_inner(member, visiting, params);
                }
            }
            Type::TypeParam(name) => {
                params.insert(name.clone());
            }
            Type::Tuple(elements) => {
                for element in elements {
                    self.collect_array_equality_type_params_inner(element, visiting, params);
                }
            }
            Type::Named(name, args) => {
                if let Some(class_info) = self.resolve_class_info(name) {
                    debug_assert_eq!(args.len(), class_info.decl.type_params.len());
                    let key = format!("class:{}:{}", class_info.module_name, class_info.decl.name);
                    if !visiting.insert(key.clone()) {
                        return;
                    }
                    let substitutions =
                        substitutions_from_decl_type_args(&class_info.decl.type_params, args);
                    for field in class_info.fields.values() {
                        self.collect_array_equality_type_params_inner(
                            &substitute_type(&field.ty, &substitutions),
                            visiting,
                            params,
                        );
                    }
                    visiting.remove(&key);
                    return;
                } else if let Some(enum_info) = self.resolve_enum_info(name) {
                    debug_assert_eq!(args.len(), enum_info.decl.type_params.len());
                    let key = format!("enum:{}:{}", enum_info.module_name, enum_info.decl.name);
                    if !visiting.insert(key.clone()) {
                        return;
                    }
                    let substitutions =
                        substitutions_from_decl_type_args(&enum_info.decl.type_params, args);
                    for payload in enum_info
                        .variants
                        .values()
                        .flat_map(|variant| &variant.payloads)
                    {
                        self.collect_array_equality_type_params_inner(
                            &substitute_type(&payload.ty, &substitutions),
                            visiting,
                            params,
                        );
                    }
                    visiting.remove(&key);
                    return;
                }
                for arg in args {
                    self.collect_array_equality_type_params_inner(arg, visiting, params);
                }
            }
            Type::Closure { .. }
            | Type::Callable(_)
            | Type::ReturnedView(_)
            | Type::Function { .. }
            | Type::Module(_)
            | Type::Unit => {}
        }
    }

    pub(super) fn noncloneable_closure_in_type(&self, ty: &Type) -> Option<Type> {
        self.noncloneable_closure_in_type_inner(ty, &mut BTreeSet::new())
    }

    pub(super) fn noncloneable_closure_in_type_inner(
        &self,
        ty: &Type,
        visiting: &mut BTreeSet<String>,
    ) -> Option<Type> {
        match ty {
            Type::Union(union) => union
                .members
                .iter()
                .find_map(|member| self.noncloneable_closure_in_type_inner(member, visiting)),
            Type::Closure { .. } | Type::Callable(_) => Some(ty.clone()),
            Type::Tuple(elements) => elements
                .iter()
                .find_map(|element| self.noncloneable_closure_in_type_inner(element, visiting)),
            Type::Named(name, args) => {
                if let Some(closure) = args
                    .iter()
                    .find_map(|arg| self.noncloneable_closure_in_type_inner(arg, visiting))
                {
                    return Some(closure);
                }

                if let Some(class_info) = self.resolve_class_info(name) {
                    if args.len() != class_info.decl.type_params.len() {
                        return None;
                    }
                    let key = format!("class:{}:{}", class_info.module_name, class_info.decl.name);
                    if !visiting.insert(key.clone()) {
                        return None;
                    }
                    let substitutions =
                        substitutions_from_decl_type_args(&class_info.decl.type_params, args);
                    let closure = class_info.fields.values().find_map(|field| {
                        let field_ty = substitute_type(&field.ty, &substitutions);
                        self.noncloneable_closure_in_type_inner(&field_ty, visiting)
                    });
                    visiting.remove(&key);
                    return closure;
                }

                if let Some(enum_info) = self.resolve_enum_info(name) {
                    if args.len() != enum_info.decl.type_params.len() {
                        return None;
                    }
                    let key = format!("enum:{}:{}", enum_info.module_name, enum_info.decl.name);
                    if !visiting.insert(key.clone()) {
                        return None;
                    }
                    let substitutions =
                        substitutions_from_decl_type_args(&enum_info.decl.type_params, args);
                    let closure = enum_info
                        .variants
                        .values()
                        .flat_map(|variant| &variant.payloads)
                        .find_map(|payload| {
                            let payload_ty = substitute_type(&payload.ty, &substitutions);
                            self.noncloneable_closure_in_type_inner(&payload_ty, visiting)
                        });
                    visiting.remove(&key);
                    return closure;
                }

                None
            }
            Type::Function { .. }
            | Type::ReturnedView(_)
            | Type::TypeParam(_)
            | Type::Module(_)
            | Type::Unit => None,
        }
    }
}
