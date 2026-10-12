//! The prelude `Display` trait and its implementation rules (ADR-0055).
//!
//! Every module sees one prelude trait `Display` with the single method
//! `def display(self) -> str`, unless the module declares or imports its own
//! item named `Display`. A user implementation must target a class or enum
//! that the implementing module declares, must cover every specialization of
//! a generic target, and must match the trait method exactly. These rules give
//! every nominal value one stable rendering that both backends dispatch by the
//! value's runtime nominal name.

use super::{
    BTreeMap, BTreeSet, ClassInfo, Diagnostic, EnumInfo, FunctionSignature, ImplDecl, Item,
    ReceiverKind, Result, TraitImplInfo, TraitInfo, TraitMethodInfo, Type,
};

/// The module name recorded on the prelude trait. It is not a valid module
/// path, so no source module can collide with it.
pub(crate) const PRELUDE_MODULE: &str = "<prelude>";

/// The name of the prelude rendering trait.
pub(crate) const DISPLAY_TRAIT: &str = "Display";

/// The name of the prelude rendering method.
pub(crate) const DISPLAY_METHOD: &str = "display";

const PRELUDE_DISPLAY_SOURCE: &str = "trait Display:\n    def display(self) -> str\n";

/// Builds the checked prelude `Display` trait.
pub(crate) fn prelude_display_trait() -> TraitInfo {
    let module = crate::parser::parse(PRELUDE_DISPLAY_SOURCE)
        .expect("the prelude Display declaration should parse");
    let Some(Item::Trait(decl)) = module.items.into_iter().next() else {
        unreachable!("the prelude Display source declares exactly one trait");
    };
    let method = decl
        .methods
        .first()
        .cloned()
        .expect("the prelude Display trait declares `display`");
    let mut methods = BTreeMap::new();
    methods.insert(
        DISPLAY_METHOD.to_string(),
        TraitMethodInfo {
            decl: method,
            signature: FunctionSignature {
                params: Vec::new(),
                param_passings: Vec::new(),
                return_type: Type::named("str"),
                rng_clone_safe_type_params: BTreeSet::new(),
                array_equality_safe_type_params: BTreeSet::new(),
            },
            type_param_bounds: BTreeMap::new(),
        },
    );
    TraitInfo {
        module_name: PRELUDE_MODULE.to_string(),
        decl,
        supertraits: Vec::new(),
        methods,
    }
}

/// Whether a checked trait is the prelude `Display` trait rather than a
/// same-named user trait.
pub(crate) fn is_prelude_display(trait_info: &TraitInfo) -> bool {
    trait_info.module_name == PRELUDE_MODULE && trait_info.decl.name == DISPLAY_TRAIT
}

fn display_error(span: crate::diag::Span, message: impl Into<String>) -> Diagnostic {
    Diagnostic::coded_at("AU2043", span, message)
}

/// The rules for the target of `impl Display for Target:`.
pub(super) fn validate_display_target(
    impl_decl: &ImplDecl,
    for_type: &Type,
    impl_type_params: &[String],
    module_name: &str,
    classes: &BTreeMap<String, ClassInfo>,
    enums: &BTreeMap<String, EnumInfo>,
    trait_impls: &[TraitImplInfo],
) -> Result<()> {
    let span = impl_decl.span;
    let (name, args) = match for_type {
        Type::Named(name, args) => (name, args),
        Type::Tuple(_) => {
            return Err(display_error(
                span,
                format!("`Display` cannot be implemented for the tuple type `{for_type}`"),
            )
            .with_help("tuples render structurally; wrap the values in a class to customize them"))
        }
        Type::Union(_) => {
            return Err(display_error(
                span,
                format!("`Display` cannot be implemented for the union type `{for_type}`"),
            )
            .with_help(
                "a union renders its active member; implement `Display` for a member class",
            ))
        }
        Type::TypeParam(_) => {
            return Err(display_error(
                span,
                format!("`Display` cannot be implemented for the type parameter `{for_type}`"),
            )
            .with_help("blanket `Display` implementations are not supported"))
        }
        _ => {
            return Err(display_error(
                span,
                format!("`Display` cannot be implemented for `{for_type}`"),
            )
            .with_help("implement `Display` for a class or enum declared in this module"))
        }
    };

    let local_type_params = if let Some(class) = classes
        .get(name)
        .filter(|class| class.module_name == module_name && !class.is_builtin)
    {
        &class.decl.type_params
    } else if let Some(enum_info) = enums
        .get(name)
        .filter(|enum_info| enum_info.module_name == module_name)
    {
        &enum_info.decl.type_params
    } else {
        let foreign = classes
            .values()
            .find(|class| !class.is_builtin && local_or_qualified_name(class, name))
            .map(|class| class.module_name.clone())
            .or_else(|| {
                enums
                    .values()
                    .find(|enum_info| {
                        format!("{}.{}", enum_info.module_name, enum_info.decl.name) == *name
                    })
                    .map(|enum_info| enum_info.module_name.clone())
            });
        return Err(match foreign {
            Some(owner) => display_error(
                span,
                format!(
                    "`Display` for `{for_type}` must be implemented in module `{owner}`, which declares the type"
                ),
            )
            .with_help("a type has one rendering, chosen by the module that declares it"),
            None => display_error(
                span,
                format!("`Display` cannot be implemented for the builtin type `{for_type}`"),
            )
            .with_help("builtin values keep their compiler-defined rendering"),
        });
    };

    let mut seen = BTreeSet::new();
    let covers_every_specialization = args.len() == local_type_params.len()
        && args.iter().all(|arg| match arg {
            Type::TypeParam(param) => impl_type_params.contains(param) && seen.insert(param),
            _ => false,
        });
    if !covers_every_specialization {
        let generic_target = format!("{}[{}]", name, local_type_params.join(", "));
        return Err(display_error(
            span,
            format!("`Display` for `{for_type}` must cover every specialization of `{name}`"),
        )
        .with_help(format!(
            "write `impl[{}] Display for {generic_target}:`; one implementation renders every specialization",
            local_type_params.join(", ")
        )));
    }
    if !impl_decl.type_param_bounds.is_empty() {
        return Err(display_error(
            span,
            format!("`Display` for `{for_type}` cannot constrain its type parameters"),
        )
        .with_help("every specialization must be displayable; render a type parameter with `str(value)` or an f-string"));
    }

    if let Some(existing) = trait_impls.iter().find(|existing| {
        existing.display
            && matches!(&existing.for_type, Type::Named(existing_name, _) if existing_name == name)
    }) {
        return Err(display_error(
            span,
            format!("`Display` for `{name}` overlaps an earlier implementation"),
        )
        .with_secondary(
            existing.decl.span,
            "the earlier `Display` implementation is here",
        )
        .with_help("a type has exactly one `Display` implementation"));
    }
    Ok(())
}

fn local_or_qualified_name(class: &ClassInfo, name: &str) -> bool {
    format!("{}.{}", class.module_name, class.decl.name) == name
}

/// The rules for the methods of `impl Display for Target:`.
pub(super) fn validate_display_methods(impl_decl: &ImplDecl) -> Result<()> {
    for method in &impl_decl.methods {
        if method.name != DISPLAY_METHOD {
            return Err(display_error(
                method.span,
                format!("method `{}` is not part of trait `Display`", method.name),
            )
            .with_help("`Display` has exactly one method: `def display(self) -> str`"));
        }
        let returns_str = method.view_return.is_none()
            && method
                .return_type
                .named_parts()
                .is_some_and(|(name, args)| name == "str" && args.is_empty());
        let mismatch = if method.receiver != Some(ReceiverKind::Borrow) {
            Some("its receiver must be bare shared `self`")
        } else if !method.params.is_empty() {
            Some("it takes no parameters")
        } else if !method.type_params.is_empty() {
            Some("it takes no type parameters")
        } else if !returns_str {
            Some("it returns `str`")
        } else {
            None
        };
        if let Some(rule) = mismatch {
            return Err(display_error(
                method.span,
                format!("`display` does not match `Display`: {rule}"),
            )
            .with_help("declare it exactly as `def display(self) -> str:`"));
        }
    }
    if !impl_decl
        .methods
        .iter()
        .any(|method| method.name == DISPLAY_METHOD)
    {
        return Err(display_error(
            impl_decl.span,
            "a `Display` implementation must define `display`",
        )
        .with_help("add `def display(self) -> str:`"));
    }
    Ok(())
}

impl super::FunctionChecker<'_> {
    /// Whether values of `ty` render through a user `Display` implementation.
    pub(super) fn has_custom_display(&self, ty: &Type) -> bool {
        let Type::Named(name, _) = ty else {
            return false;
        };
        self.trait_impls_in_scope().any(|trait_impl| {
            trait_impl.display
                && matches!(&trait_impl.for_type, Type::Named(target, _) if target == name)
        })
    }
}

/// Validates an explicit f-string format specification for a value that
/// renders through a custom `Display` implementation: only the string subset
/// of fill, alignment, width, precision, and `s` applies.
pub(super) fn validate_custom_display_format_spec(
    spec: &crate::runtime_value::ParsedFormatSpec,
    value_type: &Type,
    spec_span: crate::diag::Span,
) -> Result<()> {
    let rejected = if spec.sign.is_some() {
        Some("a sign")
    } else if spec.zero_pad {
        Some("zero padding")
    } else if spec.grouping {
        Some("a thousands separator")
    } else if spec.ty.is_some_and(|code| code != 's') {
        Some("a numeric format code")
    } else {
        None
    };
    match rejected {
        Some(feature) => Err(Diagnostic::coded_at(
            "AU2002",
            spec_span,
            format!(
                "`{value_type}` renders through `Display`, so its format specification cannot use {feature}"
            ),
        )
        .with_help("a `Display` value accepts fill, alignment, width, precision, and `s`")),
        None => Ok(()),
    }
}
