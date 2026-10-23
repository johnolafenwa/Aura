//! Specialization of generic functions that call an associated trait method
//! through a type parameter, such as `T.make(seed)`.
//!
//! Generic bodies are otherwise erased. The checker records each such call
//! and each use of a function that needs one (`sema::type_param_calls`).
//! Lowering gives every recorded use a placeholder callee that names its
//! source position. Once the whole program is lowered, this pass copies each
//! such function, with the lambdas it owns, once per concrete type argument
//! list. It points every placeholder at a copy or at the selected
//! implementation, and drops the generic originals.

use super::{
    imported_module_function_name, CallTarget, Instruction, Lowerer, MirFunction, MirModule,
    MirTraitImpl, Operand, Rvalue,
};
use crate::ast::{Argument, Expr, ExprKind};
use crate::diag::Span;
use crate::sema::{
    collect_type_param_calls, has_unresolved_type_params, lookup_type_param_call, substitute_type,
    Program, Type, TypeParamCall, TypeParamCallId,
};
use std::collections::{BTreeMap, BTreeSet, HashMap};

const PLACEHOLDER_PREFIX: &str = "__aura_type_param_call ";

/// The most copies one program may create. Polymorphic recursion, such as
/// `f[T]` calling `f[list[T]]`, would otherwise never finish.
const SPECIALIZATION_LIMIT: usize = 4096;

fn placeholder(id: &TypeParamCallId) -> String {
    format!(
        "{PLACEHOLDER_PREFIX}{}:{} {}",
        id.line, id.column, id.module_name
    )
}

fn parse_placeholder(name: &str) -> Option<TypeParamCallId> {
    let rest = name.strip_prefix(PLACEHOLDER_PREFIX)?;
    let (position, module_name) = rest.split_once(' ')?;
    let (line, column) = position.split_once(':')?;
    Some(TypeParamCallId {
        module_name: module_name.to_string(),
        line: line.parse().ok()?,
        column: column.parse().ok()?,
    })
}

fn runtime_function_name(program: &Program, module_name: &str, name: &str) -> String {
    if module_name == program.module_name {
        name.to_string()
    } else {
        imported_module_function_name(module_name, name)
    }
}

fn program_type_param_calls(program: &Program) -> BTreeMap<TypeParamCallId, TypeParamCall> {
    collect_type_param_calls(
        &program.type_definitions.type_param_calls.borrow(),
        &program.module_registry,
        &program.imported_modules,
    )
}

fn program_specialized_functions(program: &Program) -> &BTreeSet<(String, String)> {
    program
        .type_definitions
        .specialized_functions
        .get_or_init(|| crate::sema::specialized_functions(&program_type_param_calls(program)))
}

impl Lowerer<'_> {
    fn type_param_call_id(&self, span: Span) -> TypeParamCallId {
        TypeParamCallId {
            module_name: self.module_name.to_string(),
            line: span.line,
            column: span.column,
        }
    }

    fn type_param_call(&self, span: Span) -> Option<TypeParamCall> {
        lookup_type_param_call(
            &self.program.type_definitions.type_param_calls.borrow(),
            &self.program.module_registry,
            &self.program.imported_modules,
            &self.type_param_call_id(span),
        )
    }

    /// The callee for a use of a generic free function at `span`: a
    /// placeholder when the function is specialized per type, otherwise
    /// `runtime_name`.
    pub(super) fn specialized_function_use(&self, span: Span, runtime_name: String) -> String {
        match self.type_param_call(span) {
            Some(TypeParamCall::Generic {
                callee_module,
                callee,
                ..
            }) if program_specialized_functions(self.program)
                .contains(&(callee_module.clone(), callee.clone())) =>
            {
                placeholder(&self.type_param_call_id(span))
            }
            _ => runtime_name,
        }
    }

    /// Lowers `T.method(args)`, which the checker recorded at this call,
    /// to a placeholder call that specialization resolves.
    pub(super) fn lower_type_param_associated_call(
        &mut self,
        expr: &Expr,
        callee: &Expr,
        args: &[Argument],
    ) -> Option<Operand> {
        let ExprKind::Member { field, .. } = &callee.kind else {
            return None;
        };
        let Some(TypeParamCall::Associated { type_param, .. }) = self.type_param_call(expr.span)
        else {
            return None;
        };
        let (_, _, method, substitutions) =
            self.bounded_trait_method_for_receiver(&Type::TypeParam(type_param.clone()), field)?;
        let method = method.clone();
        let param_types = method
            .signature
            .params
            .iter()
            .map(|param| substitute_type(param, &substitutions))
            .collect::<Vec<_>>();
        let return_type = substitute_type(&method.signature.return_type, &substitutions);
        self.default_helper_owner = None;
        let lowered_args = self.lower_user_args_with_types(
            &format!("associated method `{type_param}.{field}`"),
            &method.decl.params,
            args,
            callee.span,
            Some(&param_types),
            Some(&method.signature.param_passings),
        );
        let temp = self.new_typed_temp(return_type);
        self.emit(Instruction::Assign {
            target: temp.clone(),
            value: Rvalue::Call {
                callee: CallTarget::Name(placeholder(&self.type_param_call_id(expr.span))),
                args: lowered_args,
                type_args: Default::default(),
            },
        });
        Some(Operand::Place(temp))
    }

    /// The return type of a recorded `T.method(...)` call.
    pub(super) fn type_param_associated_call_type(&self, expr: &Expr) -> Option<Type> {
        let ExprKind::Call { callee, .. } = &expr.kind else {
            return None;
        };
        let ExprKind::Member { field, .. } = &callee.kind else {
            return None;
        };
        let Some(TypeParamCall::Associated { type_param, .. }) = self.type_param_call(expr.span)
        else {
            return None;
        };
        let (_, _, method, substitutions) =
            self.bounded_trait_method_for_receiver(&Type::TypeParam(type_param), field)?;
        Some(substitute_type(
            &method.signature.return_type,
            &substitutions,
        ))
    }
}

/// Specializes every function that needs a concrete type and resolves every
/// placeholder. A program with no `T.method(...)` call is left unchanged.
pub(super) fn specialize_type_param_calls(program: &Program, module: &mut MirModule) {
    let records = program_type_param_calls(program);
    if !records
        .values()
        .any(|record| matches!(record, TypeParamCall::Associated { .. }))
    {
        return;
    }
    let specialized = program_specialized_functions(program);
    let template_names = specialized
        .iter()
        .map(|(module_name, name)| runtime_function_name(program, module_name, name))
        .collect::<BTreeSet<_>>();
    let (templates, kept): (Vec<_>, Vec<_>) = std::mem::take(&mut module.functions)
        .into_iter()
        .partition(|function| {
            template_names
                .iter()
                .any(|template| owned_by(&function.name, template))
        });
    let mut specializer = Specializer {
        program,
        records: &records,
        specialized,
        trait_impls: &module.trait_impls,
        templates,
        created: BTreeSet::new(),
        queue: Vec::new(),
    };
    let mut functions = kept
        .iter()
        .map(|function| specializer.rewrite(function, &HashMap::new(), None))
        .collect::<Vec<_>>();
    let mut top_level = module.top_level.take();
    if let Some(function) = &mut top_level {
        *function = specializer.rewrite(function, &HashMap::new(), None);
    }
    while let Some(job) = specializer.queue.pop() {
        let family = specializer
            .templates
            .iter()
            .filter(|function| owned_by(&function.name, &job.template))
            .cloned()
            .collect::<Vec<_>>();
        for function in &family {
            functions.push(specializer.rewrite(
                function,
                &job.substitutions,
                Some((&job.template, &job.name)),
            ));
        }
    }
    module.functions = functions;
    module.top_level = top_level;
}

/// `name` is `template` itself or a function it owns, such as one of its
/// lambdas (`template::__lambda_3_9`).
fn owned_by(name: &str, template: &str) -> bool {
    name == template
        || name
            .strip_prefix(template)
            .is_some_and(|rest| rest.starts_with("::__"))
}

struct Job {
    template: String,
    name: String,
    substitutions: HashMap<String, Type>,
}

struct Specializer<'a> {
    program: &'a Program,
    records: &'a BTreeMap<TypeParamCallId, TypeParamCall>,
    specialized: &'a BTreeSet<(String, String)>,
    trait_impls: &'a [MirTraitImpl],
    templates: Vec<MirFunction>,
    created: BTreeSet<String>,
    queue: Vec<Job>,
}

impl Specializer<'_> {
    /// A copy of `function` with each placeholder resolved under
    /// `substitutions`. With `rename`, the template `rename.0` and the
    /// functions it owns take the prefix `rename.1`.
    fn rewrite(
        &mut self,
        function: &MirFunction,
        substitutions: &HashMap<String, Type>,
        rename: Option<(&str, &str)>,
    ) -> MirFunction {
        // MIR is plain serializable data, so every name in it is a string.
        let Ok(mut value) = serde_json::to_value(function) else {
            return function.clone();
        };
        self.rewrite_value(&mut value, substitutions, rename);
        let mut copy: MirFunction =
            serde_json::from_value(value).unwrap_or_else(|_| function.clone());
        if let Some((from, to)) = rename {
            if copy.name == from {
                copy.name = to.to_string();
            }
        }
        copy
    }

    fn rewrite_value(
        &mut self,
        value: &mut serde_json::Value,
        substitutions: &HashMap<String, Type>,
        rename: Option<(&str, &str)>,
    ) {
        match value {
            serde_json::Value::String(text) => {
                if let Some(id) = parse_placeholder(text) {
                    if let Some(resolved) = self.resolve(&id, substitutions) {
                        *text = resolved;
                    }
                } else if let Some((from, to)) = rename {
                    // Only the names of functions the template owns carry
                    // its prefix. A string equal to the template name may be
                    // a literal, so the copy's own name is set above.
                    if let Some(rest) = text
                        .strip_prefix(from)
                        .filter(|rest| rest.starts_with("::__"))
                    {
                        *text = format!("{to}{rest}");
                    }
                }
            }
            serde_json::Value::Array(items) => {
                for item in items {
                    self.rewrite_value(item, substitutions, rename);
                }
            }
            serde_json::Value::Object(fields) => {
                for item in fields.values_mut() {
                    self.rewrite_value(item, substitutions, rename);
                }
            }
            _ => {}
        }
    }

    fn resolve(
        &mut self,
        id: &TypeParamCallId,
        substitutions: &HashMap<String, Type>,
    ) -> Option<String> {
        match self.records.get(id)? {
            TypeParamCall::Associated {
                type_param,
                trait_name,
                method,
                ..
            } => {
                let ty = substitutions.get(type_param)?;
                self.implementation(trait_name, ty, method)
            }
            TypeParamCall::Generic {
                callee_module,
                callee,
                type_args,
                ..
            } => {
                let runtime_name = runtime_function_name(self.program, callee_module, callee);
                if !self
                    .specialized
                    .contains(&(callee_module.clone(), callee.clone()))
                {
                    return Some(runtime_name);
                }
                let type_args = type_args
                    .iter()
                    .map(|(name, ty)| (name.clone(), substitute_type(ty, substitutions)))
                    .collect::<Vec<_>>();
                if type_args
                    .iter()
                    .any(|(_, ty)| has_unresolved_type_params(ty))
                {
                    return None;
                }
                let name = format!(
                    "{runtime_name}[{}]",
                    type_args
                        .iter()
                        .map(|(_, ty)| ty.to_string())
                        .collect::<Vec<_>>()
                        .join(", ")
                );
                if !self.created.contains(&name) {
                    if self.created.len() >= SPECIALIZATION_LIMIT {
                        return None;
                    }
                    self.created.insert(name.clone());
                    self.queue.push(Job {
                        template: runtime_name,
                        name: name.clone(),
                        substitutions: type_args.into_iter().collect(),
                    });
                }
                Some(name)
            }
        }
    }

    /// The function implementing `trait_name.method` for `ty`, chosen as
    /// trait dispatch chooses it: the most specific matching impl.
    fn implementation(&self, trait_name: &str, ty: &Type, method: &str) -> Option<String> {
        let mut best: Option<(usize, &str)> = None;
        for trait_impl in self.trait_impls {
            if trait_impl.trait_name != trait_name {
                continue;
            }
            let mut type_params = BTreeSet::new();
            super::collect_type_params_from_type(&trait_impl.for_type, &mut type_params);
            if !crate::sema::type_pattern_matches(
                &trait_impl.for_type,
                ty,
                &type_params,
                &mut HashMap::new(),
            ) {
                continue;
            }
            let Some(found) = trait_impl
                .methods
                .iter()
                .find(|candidate| candidate.name == method)
            else {
                continue;
            };
            let specificity = crate::sema::trait_impl_specificity_parts(
                &trait_impl.for_type,
                &trait_impl.trait_args,
            );
            if best.is_none_or(|(score, _)| specificity > score) {
                best = Some((specificity, &found.function_name));
            }
        }
        best.map(|(_, name)| name.to_string())
    }
}
