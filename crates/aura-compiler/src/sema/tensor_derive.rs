//! Derived `tensor.Module` implementations (ADR-0099).
//!
//! `impl Module for Net` with no body asks the compiler for `collect`, the
//! method that names each field of a model that holds weights. Before
//! checking, this pass writes it as ordinary Aura source, one line for each
//! field whose type is a parameter, a module, or a list of modules. A field
//! of any other type is left out. A field that holds weights in a shape the
//! pass cannot walk is an error, so a derived `collect` never drops weights
//! silently.
//!
//! The generated source is parsed with its lines shifted past the lines of
//! derived `json.Codec` source, and a checker error inside it is reported at
//! the `impl` line.

use super::{Diagnostic, ImportedBinding, Item, Module, ModuleContext, Result};
use crate::ast::{ImplDecl, TypeRef, TypeRefKind};
use crate::diag::Span;

/// First line number of generated source.
const LINE_OFFSET: usize = super::GENERATED_LINE_OFFSET + 500_000_000;

/// Module binding the generated source uses for the `tensor` module,
/// whatever name the user imported it under.
const TENSOR: &str = "__aura_tensor";

const CODE: &str = "AU2046";

/// Where each derived implementation's generated source sits, so a checker
/// error inside it can be reported at the `impl`.
#[derive(Default)]
pub(super) struct DerivedModules {
    regions: Vec<Region>,
}

struct Region {
    lines: std::ops::Range<usize>,
    class: String,
    span: Span,
}

impl DerivedModules {
    pub(super) fn map_error(&self, mut diagnostic: Diagnostic) -> Diagnostic {
        let Some(span) = diagnostic.span else {
            return diagnostic;
        };
        let found = self
            .regions
            .iter()
            .find(|region| region.lines.contains(&span.line));
        if let Some(region) = found {
            diagnostic.span = Some(region.span);
            diagnostic.message = format!(
                "derived `tensor.Module` for `{}`: {}",
                region.class, diagnostic.message
            );
        }
        diagnostic
    }
}

/// What a field's type holds, as far as `collect` is concerned.
enum Holds {
    /// A `Parameter[float32]`.
    Parameter,
    /// A type that implements `tensor.Module`.
    Module,
    /// A list of one such type.
    Modules,
    /// No weights.
    Nothing,
    /// Weights in a shape a derived `collect` cannot walk.
    Unreachable,
}

/// Writes `collect` for every bodyless `impl` of `tensor.Module` in
/// `module`, and marks the implementation as written. Bodyless
/// implementations of other traits are left for the pass that owns them.
pub(super) fn expand(module: &mut Module, context: &mut ModuleContext) -> Result<DerivedModules> {
    let derived = module
        .items
        .iter()
        .enumerate()
        .filter_map(|(index, item)| match item {
            Item::Impl(decl)
                if decl.derived && names_tensor_trait(&decl.trait_name, context, &["Module"]) =>
            {
                Some(index)
            }
            _ => None,
        })
        .collect::<Vec<_>>();
    if derived.is_empty() {
        return Ok(DerivedModules::default());
    }
    let Some(namespace) =
        crate::builtin_sources::importable_builtin_namespace(&["tensor".to_string()])
    else {
        let Item::Impl(decl) = &module.items[derived[0]] else {
            unreachable!("a derived item is an impl");
        };
        return Err(Diagnostic::coded_at(
            CODE,
            decl.span,
            "cannot load the `tensor` module",
        ));
    };
    context
        .imported_bindings
        .insert(TENSOR.to_string(), ImportedBinding::Module(namespace));

    let mut source = String::new();
    let mut regions = Vec::new();
    let mut targets = Vec::new();
    let mut line = LINE_OFFSET;
    for &index in &derived {
        let Item::Impl(decl) = &module.items[index] else {
            continue;
        };
        let class = derived_target(module, decl)?;
        let text = collect_source(module, context, decl, &class)?;
        let start = line;
        line += text.lines().count() + 2;
        source.push_str(&text);
        source.push_str("\n\n");
        regions.push(Region {
            lines: start..line,
            class: class.clone(),
            span: decl.span,
        });
        targets.push((index, class));
    }
    let regions = DerivedModules { regions };
    let generated = match crate::parser::parse_generated(&source, LINE_OFFSET - 1) {
        Ok(generated) => generated,
        Err(error) => return Err(regions.map_error(error)),
    };
    let mut written = generated.items.into_iter().filter_map(|item| match item {
        Item::Impl(decl) => Some(decl.methods),
        _ => None,
    });
    for (index, _) in &targets {
        if let Item::Impl(decl) = &mut module.items[*index] {
            decl.methods = written.next().unwrap_or_default();
            decl.derived = false;
        }
    }
    Ok(regions)
}

/// Whether `written` names one of the `tensor` module's traits in `names`
/// through this module's imports: `tensor.Module`, an aliased `t.Module`,
/// or a from-imported `Module`.
fn names_tensor_trait(written: &str, context: &ModuleContext, names: &[&str]) -> bool {
    match written.rsplit_once('.') {
        Some((module_path, name)) => {
            names.contains(&name)
                && !module_path.contains('.')
                && matches!(
                    context.imported_bindings.get(module_path),
                    Some(ImportedBinding::Module(namespace)) if namespace.path == "tensor"
                )
        }
        None => matches!(
            context.imported_bindings.get(written),
            Some(ImportedBinding::Trait(info))
                if info.module_name == "tensor" && names.contains(&info.decl.name.as_str())
        ),
    }
}

/// Checks that `decl` derives `collect` for a class of this module that is
/// not generic, and returns the class's name.
fn derived_target(module: &Module, decl: &ImplDecl) -> Result<String> {
    let reject = |message: String| {
        Err(Diagnostic::coded_at(CODE, decl.span, message)
            .with_help("write `collect` in an indented block after `:`"))
    };
    let TypeRefKind::Named { name, args } = &decl.for_type.kind else {
        return reject(
            "a derived `tensor.Module` needs a class declared in this module".to_string(),
        );
    };
    let class = module.items.iter().find_map(|item| match item {
        Item::Class(class) if &class.name == name => Some(class),
        _ => None,
    });
    let Some(class) = class else {
        return reject(format!(
            "a derived `tensor.Module` must be in the module that declares the class `{name}`"
        ));
    };
    if !args.is_empty() || !decl.type_params.is_empty() || !class.type_params.is_empty() {
        return reject(format!(
            "a derived `tensor.Module` for the generic class `{name}` is not implemented"
        ));
    }
    Ok(name.clone())
}

/// The source of the implementation with its `collect` method.
fn collect_source(
    module: &Module,
    context: &ModuleContext,
    decl: &ImplDecl,
    class: &str,
) -> Result<String> {
    let fields = module
        .items
        .iter()
        .find_map(|item| match item {
            Item::Class(found) if found.name == class => Some(&found.fields),
            _ => None,
        })
        .expect("the derived target is a class of this module");
    let mut text = format!(
        "impl {TENSOR}.Module for {class}:\n    def collect(self, into: mut {TENSOR}.Collector) -> None:\n"
    );
    for field in fields {
        let name = &field.name;
        let call = match holds(module, context, &field.ty) {
            Holds::Parameter => "parameter",
            Holds::Module => "add",
            Holds::Modules => "add_list",
            Holds::Nothing => continue,
            Holds::Unreachable => {
                return Err(Diagnostic::coded_at(
                    CODE,
                    decl.span,
                    format!(
                        "cannot derive `tensor.Module` for `{class}`: field `{name}` holds weights in a type a derived `collect` cannot walk"
                    ),
                )
                .with_help(
                    "a derived `collect` walks a `Parameter[float32]`, a module, and a list of modules; write `collect` by hand for other shapes",
                ));
            }
        };
        text.push_str(&format!("        into.{call}(\"{name}\", self.{name})\n"));
    }
    text.push_str("        return");
    Ok(text)
}

/// What a field of type `ty` holds.
fn holds(module: &Module, context: &ModuleContext, ty: &TypeRef) -> Holds {
    if let Some(trait_ref) = ty.as_trait_object() {
        return match &trait_ref.kind {
            TypeRefKind::Named { name, .. } if names_module_trait(module, context, name) => {
                Holds::Module
            }
            _ => Holds::Nothing,
        };
    }
    match &ty.kind {
        TypeRefKind::Named { name, args } if args.is_empty() => {
            if is_module_class(module, context, name) {
                Holds::Module
            } else {
                Holds::Nothing
            }
        }
        TypeRefKind::Named { name, args } => {
            if names_tensor_class(name, context, "Parameter") {
                let float32 = matches!(
                    args.as_slice(),
                    [TypeRef { kind: TypeRefKind::Named { name, args }, .. }]
                        if name == "float32" && args.is_empty()
                );
                return if float32 {
                    Holds::Parameter
                } else {
                    Holds::Unreachable
                };
            }
            if name == "list" && args.len() == 1 {
                return match holds(module, context, &args[0]) {
                    Holds::Module => Holds::Modules,
                    Holds::Nothing => Holds::Nothing,
                    _ => Holds::Unreachable,
                };
            }
            within(module, context, args)
        }
        TypeRefKind::Union(members) | TypeRefKind::Tuple(members) => {
            within(module, context, members)
        }
        TypeRefKind::Callable { .. } | TypeRefKind::Function { .. } => Holds::Nothing,
    }
}

/// `Unreachable` when any of `types` holds weights, since they sit inside a
/// type `collect` does not walk.
fn within(module: &Module, context: &ModuleContext, types: &[TypeRef]) -> Holds {
    if types
        .iter()
        .all(|ty| matches!(holds(module, context, ty), Holds::Nothing))
    {
        Holds::Nothing
    } else {
        Holds::Unreachable
    }
}

/// Whether `written` names the `tensor` module's class `class`.
fn names_tensor_class(written: &str, context: &ModuleContext, class: &str) -> bool {
    match written.rsplit_once('.') {
        Some((module_path, name)) => {
            name == class
                && matches!(
                    context.imported_bindings.get(module_path),
                    Some(ImportedBinding::Module(namespace)) if namespace.path == "tensor"
                )
        }
        None => matches!(
            context.imported_bindings.get(written),
            Some(ImportedBinding::Class(info))
                if info.module_name == "tensor" && info.decl.name == class
        ),
    }
}

/// Whether `written` names `tensor.Module`, `tensor.Layer`, or a trait of
/// this module that extends one of them.
fn names_module_trait(module: &Module, context: &ModuleContext, written: &str) -> bool {
    if names_tensor_trait(written, context, &["Module", "Layer"]) {
        return true;
    }
    module.items.iter().any(|item| match item {
        Item::Trait(decl) if decl.name == written => {
            decl.supertraits
                .iter()
                .any(|supertrait| match &supertrait.kind {
                    TypeRefKind::Named { name, .. } => {
                        name != written && names_module_trait(module, context, name)
                    }
                    _ => false,
                })
        }
        _ => false,
    })
}

/// Whether the class `written` names implements `tensor.Module`: a class of
/// this module with an implementation here, or an imported class whose
/// module has one.
fn is_module_class(module: &Module, context: &ModuleContext, written: &str) -> bool {
    let implemented_here = module.items.iter().any(|item| match item {
        Item::Impl(decl) => {
            names_tensor_trait(&decl.trait_name, context, &["Module"])
                && matches!(&decl.for_type.kind, TypeRefKind::Named { name, .. } if name == written)
        }
        _ => false,
    });
    if implemented_here {
        return true;
    }
    let (namespace, class) = match written.rsplit_once('.') {
        Some((module_path, class)) => match context.imported_bindings.get(module_path) {
            Some(ImportedBinding::Module(namespace)) => (Some(namespace), class),
            _ => return false,
        },
        None => match context.imported_bindings.get(written) {
            Some(ImportedBinding::Class(info)) => (
                context.module_registry.get(&info.module_name).or_else(|| {
                    match context.imported_bindings.get(TENSOR) {
                        Some(ImportedBinding::Module(tensor)) if info.module_name == "tensor" => {
                            Some(tensor)
                        }
                        _ => None,
                    }
                }),
                info.decl.name.as_str(),
            ),
            _ => return false,
        },
    };
    namespace.is_some_and(|namespace| {
        namespace.trait_impls.iter().any(|trait_impl| {
            trait_impl.trait_name.rsplit('.').next() == Some("Module")
                && matches!(
                    &trait_impl.for_type,
                    super::Type::Named(name, _) if name.rsplit('.').next() == Some(class)
                )
        })
    })
}
