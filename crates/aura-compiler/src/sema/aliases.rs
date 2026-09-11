//! Alias declaration contracts and source-scoped specialization validation.
use super::*;
use crate::ast::{FormatPart, TypeRefKind};
use crate::diag::Span;

impl FunctionChecker<'_> {
    /// Only a written alias supplies a presentation name. Inferred types keep
    /// their normalized spelling, independent of which aliases are in scope.
    pub(super) fn written_alias_type(&self, source: &TypeRef, expanded: &Type) -> Option<String> {
        let TypeRefKind::Named { name, args } = &source.kind else {
            return None;
        };
        if self.type_params.contains_key(name)
            || !(self.type_names.aliases.contains_key(name)
                || self.type_names.checked_aliases.contains_key(name)
                || self.type_names.imported_aliases.contains_key(name))
        {
            return None;
        }
        let mut written = name.clone();
        if !args.is_empty() {
            let args = args
                .iter()
                .map(|arg| {
                    lower_type(
                        arg,
                        self.type_names,
                        self.type_arities,
                        self.canonical_type_names,
                        &self.type_params,
                    )
                    .map(|ty| ty.to_string())
                })
                .collect::<Result<Vec<_>>>()
                .ok()?;
            written.push('[');
            written.push_str(&args.join(", "));
            written.push(']');
        }
        Some(format!("{written} (= {expanded})"))
    }

    pub(super) fn check_alias_constructor_bounds(
        &self,
        alias: &AliasInfo,
        substitutions: &HashMap<String, Type>,
        span: Span,
    ) -> Result<()> {
        for param in &alias.decl.type_params {
            let actual = substitutions.get(param).ok_or_else(|| {
                Diagnostic::coded_at(
                    "AU2002",
                    span,
                    format!(
                        "cannot infer type parameter `{param}` for type alias `{}`",
                        alias.decl.name
                    ),
                )
            })?;
            if let Some(bounds) = alias.type_param_bounds.get(param) {
                let bounds = bounds
                    .iter()
                    .map(|bound| substitute_trait_bound(bound, substitutions))
                    .collect::<Vec<_>>();
                self.assert_type_satisfies_bounds(actual, &bounds, span)?;
            }
        }
        Ok(())
    }
}

pub(super) fn validate_alias_contracts(
    program: &Program,
    definitions: &TypeDefinitions,
    arities: &BTreeMap<String, usize>,
) -> Result<()> {
    let checker = FunctionChecker::new(
        &program.module_name,
        definitions,
        arities,
        &program.canonical_type_names,
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
    let validator = AliasValidator { program, checker };
    for item in &program.module.items {
        match item {
            Item::TypeAlias(decl) => {
                let info = &program.aliases[&decl.name];
                let scoped = validator.scoped(&decl.type_params, &info.type_param_bounds);
                scoped.ty(&decl.target, false)?;
                if decl.public {
                    if let Some(private) = private_type(program, &info.target) {
                        let span =
                            find_type_span(&decl.target, &private).unwrap_or(decl.target.span);
                        return Err(Diagnostic::coded_at(
                            "AU2005",
                            span,
                            format!(
                                "public type alias `{}` exposes private type `{private}`",
                                decl.name
                            ),
                        ));
                    }
                }
            }
            Item::Function(decl) => validator.function(
                decl,
                &program.functions[&decl.name].type_param_bounds,
                decl.public,
            )?,
            Item::Class(decl) => {
                let info = &program.classes[&decl.name];
                let scoped = validator.scoped(&decl.type_params, &info.type_param_bounds);
                for field in &decl.fields {
                    scoped.ty(&field.ty, decl.public && field.public)?;
                    if let Some(default) = &field.default {
                        scoped.expr(default)?;
                    }
                }
                for method in &decl.methods {
                    scoped.function(
                        method,
                        &info.methods[&method.name].type_param_bounds,
                        decl.public && method.public,
                    )?;
                }
            }
            Item::Enum(decl) => {
                let info = &program.enums[&decl.name];
                let scoped = validator.scoped(&decl.type_params, &info.type_param_bounds);
                for variant in &decl.variants {
                    for field in &variant.payloads {
                        scoped.ty(&field.ty, decl.public)?;
                    }
                }
            }
            Item::Trait(decl) => {
                let scoped = validator.scoped(&decl.type_params, &BTreeMap::new());
                for ty in &decl.supertraits {
                    scoped.ty(ty, decl.public)?;
                }
                for method in &decl.methods {
                    scoped.function(
                        method,
                        &program.traits[&decl.name].methods[&method.name].type_param_bounds,
                        decl.public,
                    )?;
                }
            }
            Item::Impl(decl) => {
                let bounds = lower_trait_bounds(
                    &decl.type_param_bounds,
                    &program.traits,
                    definitions,
                    arities,
                    &program.canonical_type_names,
                    &type_param_scope(&decl.type_params),
                )?;
                let scoped = validator.scoped(&decl.type_params, &bounds);
                scoped.ty(&decl.for_type, false)?;
                for ty in &decl.trait_args {
                    scoped.ty(ty, false)?;
                }
                for method in &decl.methods {
                    let params =
                        merged_type_param_scope(&scoped.checker.type_params, &method.type_params);
                    let method_bounds = lower_trait_bounds(
                        &method.type_param_bounds,
                        &program.traits,
                        definitions,
                        arities,
                        &program.canonical_type_names,
                        &params,
                    )?;
                    scoped.function(method, &method_bounds, false)?;
                }
            }
            Item::ExternFunction(decl) => {
                for param in &decl.params {
                    validator.ty(&param.ty, decl.public)?;
                }
                validator.ty(&decl.return_type, decl.public)?;
            }
            Item::ExternOpaqueClass(_) => {}
        }
    }
    validator.stmts(&program.module.top_level_stmts)
}

struct AliasValidator<'a> {
    program: &'a Program,
    checker: FunctionChecker<'a>,
}
impl<'a> AliasValidator<'a> {
    fn scoped(&self, params: &[String], bounds: &BTreeMap<String, Vec<TraitBound>>) -> Self {
        Self {
            program: self.program,
            checker: self.checker.with_type_params(
                merged_type_param_scope(&self.checker.type_params, params),
                merge_trait_bounds(&self.checker.type_param_bounds, bounds),
            ),
        }
    }
    fn function(
        &self,
        decl: &FunctionDecl,
        bounds: &BTreeMap<String, Vec<TraitBound>>,
        public: bool,
    ) -> Result<()> {
        let scoped = self.scoped(&decl.type_params, bounds);
        for param in &decl.params {
            scoped.ty(&param.ty, public)?;
            if let Some(default) = &param.default {
                scoped.expr(default)?;
            }
        }
        scoped.ty(&decl.return_type, public)?;
        for bounds in decl.type_param_bounds.values() {
            for bound in bounds {
                scoped.ty(bound, public)?;
            }
        }
        scoped.stmts(&decl.body)
    }
    fn ty(&self, ty: &TypeRef, public: bool) -> Result<()> {
        match &ty.kind {
            TypeRefKind::Named { name, args } => {
                for arg in args {
                    self.ty(arg, public)?;
                }
                if self.checker.type_params.contains_key(name) {
                    return Ok(());
                }
                let alias = self
                    .program
                    .aliases
                    .get(name)
                    .or_else(|| self.checker.type_names.imported_aliases.get(name));
                if let Some(alias) = alias {
                    let args = args
                        .iter()
                        .map(|arg| {
                            lower_type(
                                arg,
                                self.checker.type_names,
                                self.checker.type_arities,
                                self.checker.canonical_type_names,
                                &self.checker.type_params,
                            )
                        })
                        .collect::<Result<Vec<_>>>()?;
                    let substitutions =
                        substitutions_from_decl_type_args(&alias.decl.type_params, &args);
                    for (param, bounds) in &alias.type_param_bounds {
                        if let Some(actual) = substitutions.get(param) {
                            let bounds = bounds
                                .iter()
                                .map(|bound| substitute_trait_bound(bound, &substitutions))
                                .collect::<Vec<_>>();
                            self.checker
                                .assert_type_satisfies_bounds(actual, &bounds, ty.span)?;
                        }
                    }
                    if public {
                        if let Some(private) = private_type(
                            self.program,
                            &substitute_type(&alias.target, &substitutions),
                        ) {
                            return Err(Diagnostic::coded_at("AU2005", ty.span, format!("public signature uses alias `{name}` exposing private type `{private}`")));
                        }
                    }
                }
            }
            TypeRefKind::Tuple(members) | TypeRefKind::Union(members) => {
                for member in members {
                    self.ty(member, public)?;
                }
            }
            TypeRefKind::Function {
                params,
                return_type,
            } => {
                for param in params {
                    self.ty(&param.ty, public)?;
                }
                self.ty(return_type, public)?;
            }
            TypeRefKind::Callable { signature, .. } => self.ty(signature, public)?,
        }
        Ok(())
    }
    fn stmts(&self, stmts: &[Stmt]) -> Result<()> {
        for stmt in stmts {
            match stmt {
                Stmt::Assign(assign) => {
                    if let Some(ty) = &assign.annotation {
                        self.ty(ty, false)?;
                    }
                    match &assign.target {
                        AssignTarget::Member { object, .. } => self.expr(object)?,
                        AssignTarget::Index { object, index } => {
                            self.expr(object)?;
                            self.expr(index)?;
                        }
                        AssignTarget::Name(_) => {}
                    }
                    self.expr(&assign.value)?;
                }
                Stmt::View(stmt) => self.expr(&stmt.source)?,
                Stmt::Destructure(stmt) => self.expr(&stmt.value)?,
                Stmt::Assert(stmt) => {
                    self.expr(&stmt.condition)?;
                    if let Some(message) = &stmt.message {
                        self.expr(message)?;
                    }
                }
                Stmt::Return(stmt) => {
                    if let Some(value) = &stmt.value {
                        self.expr(value)?;
                    }
                }
                Stmt::If(stmt) => {
                    for branch in &stmt.branches {
                        self.expr(&branch.condition)?;
                        self.stmts(&branch.body)?;
                    }
                    if let Some(body) = &stmt.else_body {
                        self.stmts(body)?;
                    }
                }
                Stmt::Match(stmt) => {
                    self.expr(&stmt.scrutinee)?;
                    for arm in &stmt.arms {
                        if let Some(guard) = &arm.guard {
                            self.expr(guard)?;
                        }
                        self.stmts(&arm.body)?;
                    }
                }
                Stmt::For(stmt) => {
                    self.expr(&stmt.iterable)?;
                    self.stmts(&stmt.body)?;
                }
                Stmt::With(stmt) => {
                    self.expr(&stmt.value)?;
                    self.stmts(&stmt.body)?;
                }
                Stmt::While(stmt) => {
                    self.expr(&stmt.condition)?;
                    self.stmts(&stmt.body)?;
                }
                Stmt::Expr(stmt) => self.expr(&stmt.expr)?,
                Stmt::Pass(_) | Stmt::Break(_) | Stmt::Continue(_) => {}
            }
        }
        Ok(())
    }
    fn expr(&self, expr: &Expr) -> Result<()> {
        match &expr.kind {
            ExprKind::Cast { expr, ty } => {
                self.ty(ty, false)?;
                self.expr(expr)?;
            }
            ExprKind::Specialize { expr, type_args } => {
                for ty in type_args {
                    self.ty(ty, false)?;
                }
                self.expr(expr)?;
            }
            ExprKind::Unary { expr, .. }
            | ExprKind::Try(expr)
            | ExprKind::Group(expr)
            | ExprKind::Lambda { body: expr, .. }
            | ExprKind::Member { object: expr, .. }
            | ExprKind::IsNone { value: expr, .. } => self.expr(expr)?,
            ExprKind::Binary { left, right, .. }
            | ExprKind::Index {
                object: left,
                index: right,
            }
            | ExprKind::Membership {
                value: left,
                container: right,
                ..
            } => {
                self.expr(left)?;
                self.expr(right)?;
            }
            ExprKind::Conditional {
                then_expr,
                condition,
                else_expr,
            } => {
                self.expr(then_expr)?;
                self.expr(condition)?;
                self.expr(else_expr)?;
            }
            ExprKind::Tuple(items) | ExprKind::List(items) | ExprKind::Set(items) => {
                for item in items {
                    self.expr(item)?;
                }
            }
            ExprKind::Map(items) => {
                for item in items {
                    self.expr(&item.key)?;
                    self.expr(&item.value)?;
                }
            }
            ExprKind::FString(parts) => {
                for part in parts {
                    match part {
                        FormatPart::Expr(expr) | FormatPart::Formatted { expr, .. } => {
                            self.expr(expr)?
                        }
                        FormatPart::Literal(_) => {}
                    }
                }
            }
            ExprKind::Call { callee, args } => {
                self.expr(callee)?;
                for arg in args {
                    self.expr(&arg.value)?;
                }
            }
            ExprKind::Slice {
                object, start, end, ..
            } => {
                self.expr(object)?;
                for expr in start.iter().chain(end.iter()) {
                    self.expr(expr)?;
                }
            }
            ExprKind::Match {
                scrutinee, arms, ..
            } => {
                self.expr(scrutinee)?;
                for arm in arms {
                    if let Some(guard) = &arm.guard {
                        self.expr(guard)?;
                    }
                    self.expr(&arm.value)?;
                }
            }
            ExprKind::CompareChain { first, links } => {
                self.expr(first)?;
                for link in links {
                    self.expr(&link.operand)?;
                }
            }
            ExprKind::Comprehension { output, clauses } => {
                for clause in clauses {
                    self.expr(&clause.iterable)?;
                    for filter in &clause.filters {
                        self.expr(filter)?;
                    }
                }
                match output {
                    ComprehensionOutput::List(expr) | ComprehensionOutput::Set(expr) => {
                        self.expr(expr)?
                    }
                    ComprehensionOutput::Map { key, value } => {
                        self.expr(key)?;
                        self.expr(value)?;
                    }
                }
            }
            ExprKind::Name(_)
            | ExprKind::Int(_)
            | ExprKind::Float(_)
            | ExprKind::Bool(_)
            | ExprKind::String(_)
            | ExprKind::DurationNanos(_)
            | ExprKind::BuiltinOmitted => {}
        }
        Ok(())
    }
}

fn private_type(program: &Program, ty: &Type) -> Option<String> {
    match ty {
        Type::Named(name, args) => {
            let local = name
                .strip_prefix(&format!("{}.", program.module_name))
                .unwrap_or(name);
            let private =
                program.classes.get(local).is_some_and(|info| {
                    info.module_name == program.module_name && !info.decl.public
                }) || program.enums.get(local).is_some_and(|info| {
                    info.module_name == program.module_name && !info.decl.public
                }) || program.opaque_handles.get(local).is_some_and(|info| {
                    info.module_name == program.module_name && !info.decl.public
                }) || program.traits.get(local).is_some_and(|info| {
                    info.module_name == program.module_name && !info.decl.public
                });
            if private {
                Some(local.to_string())
            } else {
                args.iter().find_map(|arg| private_type(program, arg))
            }
        }
        Type::Tuple(members) => members
            .iter()
            .find_map(|member| private_type(program, member)),
        Type::Union(union) => union
            .members
            .iter()
            .find_map(|member| private_type(program, member)),
        Type::Function {
            params,
            return_type,
        } => params
            .iter()
            .find_map(|param| private_type(program, &param.ty))
            .or_else(|| private_type(program, return_type)),
        Type::Closure {
            params,
            return_type,
            captures,
            ..
        } => params
            .iter()
            .find_map(|param| private_type(program, &param.ty))
            .or_else(|| private_type(program, return_type))
            .or_else(|| {
                captures
                    .iter()
                    .find_map(|capture| private_type(program, &capture.ty))
            }),
        Type::Callable(callable) => callable
            .params
            .iter()
            .find_map(|param| private_type(program, &param.ty))
            .or_else(|| private_type(program, &callable.return_type)),
        Type::Unit | Type::TypeParam(_) | Type::Module(_) => None,
    }
}

fn find_type_span(ty: &TypeRef, name: &str) -> Option<crate::diag::Span> {
    match &ty.kind {
        TypeRefKind::Named {
            name: candidate,
            args,
        } => {
            if candidate == name {
                Some(ty.span)
            } else {
                args.iter().find_map(|arg| find_type_span(arg, name))
            }
        }
        TypeRefKind::Tuple(members) | TypeRefKind::Union(members) => members
            .iter()
            .find_map(|member| find_type_span(member, name)),
        TypeRefKind::Function {
            params,
            return_type,
        } => params
            .iter()
            .find_map(|param| find_type_span(&param.ty, name))
            .or_else(|| find_type_span(return_type, name)),
        TypeRefKind::Callable { signature, .. } => find_type_span(signature, name),
    }
}

/// Shared constructor/member adaptation. Only a resolved type alias can enter
/// this path; ordinary values and their runtime indexing retain their meaning.
pub(crate) fn expand_alias_callee<'a>(
    callee: &Expr,
    resolve: &impl Fn(&Expr) -> Option<&'a AliasInfo>,
    lower: &impl Fn(&TypeRef) -> Result<Type>,
    validate: &impl Fn(&AliasInfo, &[Type], Span) -> Result<()>,
    module_name: &str,
    canonical_names: &BTreeMap<String, String>,
    budget: &super::type_budget::ExpansionBudget,
) -> Result<Option<Expr>> {
    let specialized = match &callee.kind {
        ExprKind::Specialize { expr, type_args } => Some((expr.as_ref(), type_args.clone())),
        ExprKind::Index { object, index } if resolve(object).is_some() => {
            let items = match &index.kind {
                ExprKind::Tuple(items) => items.as_slice(),
                _ => std::slice::from_ref(index.as_ref()),
            };
            let refs = items
                .iter()
                .map(FunctionChecker::spawn_type_ref_from_expr)
                .collect::<Option<Vec<_>>>()
                .ok_or_else(|| {
                    Diagnostic::at(
                        index.span,
                        "type alias specialization expects type arguments",
                    )
                })?;
            Some((object.as_ref(), refs))
        }
        _ => None,
    };
    if let Some((base, type_args)) = &specialized {
        if let Some(alias) = resolve(base) {
            let args = type_args.iter().map(lower).collect::<Result<Vec<_>>>()?;
            let expanded = alias.constructor_callee(
                Some(&args),
                callee.span,
                module_name,
                canonical_names,
                budget,
            )?;
            validate(alias, &args, callee.span)?;
            return Ok(expanded);
        }
    } else if let Some(alias) = resolve(callee) {
        return alias.constructor_callee(None, callee.span, module_name, canonical_names, budget);
    }
    let kind = match &callee.kind {
        ExprKind::Member { object, field } => expand_alias_callee(
            object,
            resolve,
            lower,
            validate,
            module_name,
            canonical_names,
            budget,
        )?
        .map(|object| ExprKind::Member {
            object: Box::new(object),
            field: field.clone(),
        }),
        ExprKind::Specialize { expr, type_args } => expand_alias_callee(
            expr,
            resolve,
            lower,
            validate,
            module_name,
            canonical_names,
            budget,
        )?
        .map(|expr| ExprKind::Specialize {
            expr: Box::new(expr),
            type_args: type_args.clone(),
        }),
        ExprKind::Group(expr) => expand_alias_callee(
            expr,
            resolve,
            lower,
            validate,
            module_name,
            canonical_names,
            budget,
        )?
        .map(|expr| ExprKind::Group(Box::new(expr))),
        _ => None,
    };
    Ok(kind.map(|kind| Expr {
        kind,
        span: callee.span,
    }))
}
