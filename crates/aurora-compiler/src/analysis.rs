use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use serde::Serialize;

use crate::ast::{
    AssignStmt, AssignTarget, BinaryOp, Expr, ExprKind, FunctionDecl, ImportKind, Item, MatchArm,
    Module, Pattern, SelectArm, Stmt, TypeRef, VariantPattern,
};
use crate::call::{BuiltinFunction, BuiltinMember, ALL_BUILTIN_FUNCTIONS};
use crate::diag::{Diagnostic, Result, Span};
use crate::parser;
use crate::sema::{
    substitute_trait_bound, ClassInfo, EnumInfo, FunctionInfo, MethodInfo, Program, TraitBound,
    Type,
};

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct AnalysisOutput {
    pub diagnostics: Vec<AnalysisDiagnostic>,
    pub symbols: Vec<AnalysisSymbol>,
    pub occurrences: Vec<AnalysisOccurrence>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct AnalysisDiagnostic {
    pub line: usize,
    pub start_character: usize,
    pub end_character: usize,
    pub message: String,
    pub severity: u8,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct AnalysisSymbol {
    pub name: String,
    pub kind: String,
    pub detail: String,
    pub line: usize,
    pub start_character: usize,
    pub end_character: usize,
    pub children: Vec<AnalysisSymbol>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct AnalysisOccurrence {
    pub line: usize,
    pub start_character: usize,
    pub end_character: usize,
    pub hover: String,
    pub definition: Option<AnalysisRange>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct AnalysisRange {
    pub file_path: Option<String>,
    pub line: usize,
    pub start_character: usize,
    pub end_character: usize,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct AnalysisCompletion {
    pub name: String,
    pub kind: String,
    pub detail: String,
}

#[derive(Clone)]
struct BindingInfo {
    ty: Type,
    trait_bounds: Vec<TraitBound>,
    definition: AnalysisRange,
    hover: String,
}

#[derive(Clone)]
struct ResolvedSymbol {
    hover: String,
    definition: Option<AnalysisRange>,
}

#[derive(Clone)]
struct ResolvedMember {
    hover: String,
    definition: Option<AnalysisRange>,
    ty: Option<Type>,
}

pub fn analyze_source(source: &str) -> AnalysisOutput {
    analyze_with_checker(source, crate::check_source)
}

pub fn analyze_path_source(path: &Path, source: &str) -> AnalysisOutput {
    analyze_with_checker(source, |candidate| {
        crate::check_path_with_source(path, candidate)
    })
}

pub fn complete_path_source(
    path: &Path,
    source: &str,
    line: usize,
    character: usize,
    trigger_character: Option<char>,
) -> Result<Vec<AnalysisCompletion>> {
    complete_with_checker(source, line, character, trigger_character, |candidate| {
        crate::check_path_with_source(path, candidate)
    })
}

pub fn analyze_program(source: &str, program: &Program) -> AnalysisOutput {
    let symbols = symbols_from_module(&program.module);
    AnalysisBuilder::new(source, program, symbols).build()
}

pub fn complete_source(
    source: &str,
    line: usize,
    character: usize,
    trigger_character: Option<char>,
) -> Result<Vec<AnalysisCompletion>> {
    complete_with_checker(
        source,
        line,
        character,
        trigger_character,
        crate::check_source,
    )
}

fn analyze_with_checker<F>(source: &str, mut check_program: F) -> AnalysisOutput
where
    F: FnMut(&str) -> Result<Program>,
{
    match parser::parse(source) {
        Err(error) => {
            if let Some(program) =
                recover_checked_program_after_parse_error_with(source, &error, &mut check_program)
            {
                let mut output = analyze_program(source, &program);
                output.diagnostics.insert(0, analysis_diagnostic(&error));
                output
            } else {
                AnalysisOutput {
                    diagnostics: vec![analysis_diagnostic(&error)],
                    symbols: Vec::new(),
                    occurrences: Vec::new(),
                }
            }
        }
        Ok(module) => {
            let symbols = symbols_from_module(&module);
            match check_program(source) {
                Err(error) => AnalysisOutput {
                    diagnostics: vec![analysis_diagnostic(&error)],
                    symbols,
                    occurrences: Vec::new(),
                },
                Ok(program) => AnalysisBuilder::new(source, &program, symbols).build(),
            }
        }
    }
}

fn complete_with_checker<F>(
    source: &str,
    line: usize,
    character: usize,
    trigger_character: Option<char>,
    mut check_program: F,
) -> Result<Vec<AnalysisCompletion>>
where
    F: FnMut(&str) -> Result<Program>,
{
    let program = match check_program(source) {
        Ok(program) => program,
        Err(error) if trigger_character == Some('.') => {
            recover_checked_program_after_position(source, line, character, &mut check_program)
                .ok_or(error)?
        }
        Err(error) => return Err(error),
    };
    let builder = AnalysisBuilder::new(source, &program, Vec::new());
    builder.complete(line, character, trigger_character)
}

struct AnalysisBuilder<'a> {
    source_lines: Vec<&'a str>,
    program: &'a Program,
    output: AnalysisOutput,
}

impl<'a> AnalysisBuilder<'a> {
    fn new(source: &'a str, program: &'a Program, symbols: Vec<AnalysisSymbol>) -> Self {
        Self {
            source_lines: source.lines().collect(),
            program,
            output: AnalysisOutput {
                diagnostics: Vec::new(),
                symbols,
                occurrences: Vec::new(),
            },
        }
    }

    fn build(mut self) -> AnalysisOutput {
        let mut top_level_scope = BTreeMap::new();
        self.visit_stmts(&self.program.top_level_stmts, &mut top_level_scope);

        for item in &self.program.module.items {
            match item {
                Item::Function(function_decl) => {
                    let function_info = self.program.functions.get(&function_decl.name).unwrap();
                    let mut scope = self.function_scope(function_decl, function_info);
                    self.visit_stmts(&function_decl.body, &mut scope);
                }
                Item::Class(class_decl) => {
                    let class_info = self.program.classes.get(&class_decl.name).unwrap();
                    for method in &class_decl.methods {
                        let method_info = class_info.methods.get(&method.name).unwrap();
                        let mut scope =
                            self.method_scope(class_decl.name.as_str(), method, method_info);
                        self.visit_stmts(&method.body, &mut scope);
                    }
                }
                Item::Enum(_) | Item::Trait(_) | Item::Impl(_) => {}
            }
        }

        self.output
    }

    fn complete(
        &self,
        line: usize,
        character: usize,
        trigger_character: Option<char>,
    ) -> Result<Vec<AnalysisCompletion>> {
        let line_text = self.source_lines.get(line).copied().unwrap_or("");

        if trigger_character == Some('.') {
            let Some(receiver_text) = extract_receiver_before_dot(line_text, character) else {
                return Ok(Vec::new());
            };
            let receiver_expr = parser::parse_expression(&receiver_text)?;
            let scope = self.scope_for_line(line);
            if let ExprKind::Name(name) = &receiver_expr.kind {
                if let Some(binding) = scope.get(name) {
                    if !binding.trait_bounds.is_empty() {
                        return Ok(self.trait_bound_member_completions(&binding.trait_bounds));
                    }
                }
            }
            let Some(receiver_type) = self.infer_expr_type(&receiver_expr, &scope) else {
                return Ok(Vec::new());
            };
            return Ok(self.member_completions(&receiver_type));
        }

        Ok(self.top_level_completions())
    }

    fn function_scope(
        &self,
        function_decl: &FunctionDecl,
        function_info: &FunctionInfo,
    ) -> BTreeMap<String, BindingInfo> {
        let mut scope = BTreeMap::new();
        for (param, ty) in function_decl
            .params
            .iter()
            .zip(&function_info.signature.params)
        {
            let range = range_from_span(param.span, param.name.len());
            scope.insert(
                param.name.clone(),
                BindingInfo {
                    ty: ty.clone(),
                    trait_bounds: match ty {
                        Type::TypeParam(name) => function_info
                            .type_param_bounds
                            .get(name)
                            .cloned()
                            .unwrap_or_default(),
                        _ => Vec::new(),
                    },
                    definition: range.clone(),
                    hover: format_value_hover("param", &param.name, ty),
                },
            );
        }
        scope
    }

    fn method_scope(
        &self,
        class_name: &str,
        method_decl: &FunctionDecl,
        method_info: &MethodInfo,
    ) -> BTreeMap<String, BindingInfo> {
        let mut scope = self.function_scope(
            method_decl,
            &FunctionInfo {
                module_name: self.program.module_name.clone(),
                decl: method_decl.clone(),
                signature: method_info.signature.clone(),
                type_param_bounds: method_info.type_param_bounds.clone(),
            },
        );

        if method_decl.receiver.is_some() {
            let definition = self
                .find_identifier_range(method_decl.span.line, "self")
                .unwrap_or_else(|| range_from_span(method_decl.span, method_decl.name.len()));
            scope.insert(
                "self".to_string(),
                BindingInfo {
                    ty: Type::named(class_name),
                    trait_bounds: Vec::new(),
                    definition,
                    hover: format_value_hover("param", "self", &Type::named(class_name)),
                },
            );
        }

        scope
    }

    fn scope_for_line(&self, line: usize) -> BTreeMap<String, BindingInfo> {
        let target_line = line + 1;

        if let Some((function_decl, function_info)) = self.enclosing_function(target_line) {
            let mut scope = self.function_scope(function_decl, function_info);
            self.accumulate_scope_from_stmts(&function_decl.body, target_line, &mut scope);
            return scope;
        }

        if let Some((class_name, method_decl, method_info)) = self.enclosing_method(target_line) {
            let mut scope = self.method_scope(class_name, method_decl, method_info);
            self.accumulate_scope_from_stmts(&method_decl.body, target_line, &mut scope);
            return scope;
        }

        let mut scope = BTreeMap::new();
        self.accumulate_scope_from_stmts(&self.program.top_level_stmts, target_line, &mut scope);
        scope
    }

    fn enclosing_function(&self, line: usize) -> Option<(&FunctionDecl, &FunctionInfo)> {
        self.program
            .module
            .items
            .iter()
            .filter_map(|item| match item {
                Item::Function(function_decl)
                    if callable_contains_line(&function_decl.body, line) =>
                {
                    Some((
                        function_decl,
                        self.program.functions.get(&function_decl.name).unwrap(),
                    ))
                }
                _ => None,
            })
            .last()
    }

    fn enclosing_method(&self, line: usize) -> Option<(&str, &FunctionDecl, &MethodInfo)> {
        self.program
            .module
            .items
            .iter()
            .filter_map(|item| match item {
                Item::Class(class_decl) => class_decl
                    .methods
                    .iter()
                    .filter(|method| callable_contains_line(&method.body, line))
                    .map(|method| {
                        (
                            class_decl.name.as_str(),
                            method,
                            self.program.classes[&class_decl.name]
                                .methods
                                .get(&method.name)
                                .unwrap(),
                        )
                    })
                    .last(),
                _ => None,
            })
            .last()
    }

    fn accumulate_scope_from_stmts(
        &self,
        stmts: &[Stmt],
        target_line: usize,
        scope: &mut BTreeMap<String, BindingInfo>,
    ) {
        for stmt in stmts {
            if stmt_start_line(stmt) > target_line {
                break;
            }

            match stmt {
                Stmt::Assign(assign) => self.bind_assignment(assign, scope),
                Stmt::If(if_stmt) => {
                    for branch in &if_stmt.branches {
                        if block_contains_line(&branch.body, target_line) {
                            self.accumulate_scope_from_stmts(&branch.body, target_line, scope);
                            return;
                        }
                    }
                    if let Some(body) = &if_stmt.else_body {
                        if block_contains_line(body, target_line) {
                            self.accumulate_scope_from_stmts(body, target_line, scope);
                            return;
                        }
                    }
                }
                Stmt::Match(match_stmt) => {
                    let scrutinee_type = self.infer_expr_type(&match_stmt.scrutinee, scope);
                    for arm in &match_stmt.arms {
                        if block_contains_line(&arm.body, target_line) {
                            self.bind_match_arm_scope(arm, scrutinee_type.as_ref(), scope);
                            self.accumulate_scope_from_stmts(&arm.body, target_line, scope);
                            return;
                        }
                    }
                }
                Stmt::For(for_stmt) => {
                    if block_contains_line(&for_stmt.body, target_line) {
                        let binding_ty = self
                            .infer_iterable_binding_type(&for_stmt.iterable, scope)
                            .unwrap_or(Type::Unit);
                        self.insert_scope_binding(
                            &for_stmt.binding,
                            binding_ty,
                            for_stmt.span.line,
                            "local",
                            scope,
                        );
                        self.accumulate_scope_from_stmts(&for_stmt.body, target_line, scope);
                        return;
                    }
                }
                Stmt::With(with_stmt) => {
                    if block_contains_line(&with_stmt.body, target_line) {
                        let binding_ty = self
                            .infer_expr_type(&with_stmt.value, scope)
                            .unwrap_or(Type::Unit);
                        self.insert_scope_binding(
                            &with_stmt.binding,
                            binding_ty,
                            with_stmt.span.line,
                            "local",
                            scope,
                        );
                        self.accumulate_scope_from_stmts(&with_stmt.body, target_line, scope);
                        return;
                    }
                }
                Stmt::Select(select_stmt) => {
                    for arm in &select_stmt.arms {
                        if block_contains_line(&arm.body, target_line) {
                            if let Some(binding) = &arm.binding {
                                let binding_ty =
                                    self.infer_expr_type(&arm.expr, scope).unwrap_or(Type::Unit);
                                self.insert_scope_binding(
                                    binding,
                                    binding_ty,
                                    arm.span.line,
                                    "local",
                                    scope,
                                );
                            }
                            self.accumulate_scope_from_stmts(&arm.body, target_line, scope);
                            return;
                        }
                    }
                }
                Stmt::While(while_stmt) => {
                    if block_contains_line(&while_stmt.body, target_line) {
                        self.accumulate_scope_from_stmts(&while_stmt.body, target_line, scope);
                        return;
                    }
                }
                Stmt::Pass(_)
                | Stmt::Return(_)
                | Stmt::Break(_)
                | Stmt::Continue(_)
                | Stmt::Expr(_) => {}
            }
        }
    }

    fn bind_assignment(&self, assign: &AssignStmt, scope: &mut BTreeMap<String, BindingInfo>) {
        let AssignTarget::Name(name) = &assign.target else {
            return;
        };
        if scope.contains_key(name) {
            return;
        }
        let binding_ty = assign
            .annotation
            .as_ref()
            .map(lower_type_ref)
            .or_else(|| self.infer_expr_type(&assign.value, scope))
            .unwrap_or(Type::Unit);
        let definition = self
            .find_identifier_range(assign.span.line, name)
            .unwrap_or_else(|| range_from_span(assign.span, name.len()));
        scope.insert(
            name.clone(),
            BindingInfo {
                ty: binding_ty.clone(),
                trait_bounds: Vec::new(),
                definition: definition.clone(),
                hover: format_value_hover("binding", name, &binding_ty),
            },
        );
    }

    fn top_level_completions(&self) -> Vec<AnalysisCompletion> {
        let mut completions = Vec::new();
        for keyword in KEYWORDS {
            completions.push(AnalysisCompletion {
                name: keyword.to_string(),
                kind: "keyword".to_string(),
                detail: "Aurora keyword".to_string(),
            });
        }
        for class_info in self.program.classes.values() {
            completions.push(AnalysisCompletion {
                name: class_info.decl.name.clone(),
                kind: "class".to_string(),
                detail: "Aurora class".to_string(),
            });
        }
        for enum_info in self.program.enums.values() {
            completions.push(AnalysisCompletion {
                name: enum_info.decl.name.clone(),
                kind: "enum".to_string(),
                detail: "Aurora enum".to_string(),
            });
        }
        for trait_info in self.program.traits.values() {
            completions.push(AnalysisCompletion {
                name: trait_info.decl.name.clone(),
                kind: "trait".to_string(),
                detail: "Aurora trait".to_string(),
            });
        }
        for builtin_enum in BUILTIN_ENUM_COMPLETIONS {
            completions.push(AnalysisCompletion {
                name: builtin_enum.name.to_string(),
                kind: "enum".to_string(),
                detail: builtin_enum.detail.to_string(),
            });
        }
        for function_info in self.program.functions.values() {
            completions.push(AnalysisCompletion {
                name: function_info.decl.name.clone(),
                kind: "function".to_string(),
                detail: format_function_detail(&function_info.decl),
            });
        }
        for builtin in ALL_BUILTIN_FUNCTIONS {
            completions.push(AnalysisCompletion {
                name: builtin.name().to_string(),
                kind: "function".to_string(),
                detail: builtin.detail().to_string(),
            });
        }
        for namespace in self.program.imported_modules.values() {
            completions.push(AnalysisCompletion {
                name: namespace.name.clone(),
                kind: "module".to_string(),
                detail: format!("module {}", namespace.path),
            });
        }
        completions
    }

    fn member_completions(&self, receiver_type: &Type) -> Vec<AnalysisCompletion> {
        let mut completions = Vec::new();
        if let Type::Module(path) = receiver_type {
            if let Some(namespace) = self.module_namespace(path) {
                for child in namespace.modules.values() {
                    completions.push(AnalysisCompletion {
                        name: child.name.clone(),
                        kind: "module".to_string(),
                        detail: format!("module {}", child.path),
                    });
                }
                for function in namespace.functions.values() {
                    completions.push(AnalysisCompletion {
                        name: function.decl.name.clone(),
                        kind: "function".to_string(),
                        detail: format_function_detail(&function.decl),
                    });
                }
                for class_info in namespace.classes.values() {
                    completions.push(AnalysisCompletion {
                        name: class_info.decl.name.clone(),
                        kind: "class".to_string(),
                        detail: "Aurora class".to_string(),
                    });
                }
                for enum_info in namespace.enums.values() {
                    completions.push(AnalysisCompletion {
                        name: enum_info.decl.name.clone(),
                        kind: "enum".to_string(),
                        detail: "Aurora enum".to_string(),
                    });
                }
                for trait_info in namespace.traits.values() {
                    completions.push(AnalysisCompletion {
                        name: trait_info.decl.name.clone(),
                        kind: "trait".to_string(),
                        detail: "Aurora trait".to_string(),
                    });
                }
            }
            return completions;
        }
        let base_name = base_type_name(receiver_type);

        if let Some(class_info) = self.program.classes.get(base_name) {
            for (name, field) in &class_info.fields {
                completions.push(AnalysisCompletion {
                    name: name.clone(),
                    kind: "field".to_string(),
                    detail: field.ty.to_string(),
                });
            }
            for (name, method) in &class_info.methods {
                completions.push(AnalysisCompletion {
                    name: name.clone(),
                    kind: "method".to_string(),
                    detail: format_function_detail(&method.decl),
                });
            }
        }

        for trait_impl in self.trait_impls_in_scope() {
            if self
                .trait_impl_substitutions(trait_impl, receiver_type)
                .is_some()
            {
                for (name, method) in &trait_impl.methods {
                    if completions.iter().any(|existing| existing.name == *name) {
                        continue;
                    }
                    completions.push(AnalysisCompletion {
                        name: name.clone(),
                        kind: "method".to_string(),
                        detail: format_function_detail(&method.decl),
                    });
                }
            }
        }

        if let Some(enum_info) = self.program.enums.get(base_name) {
            for (name, variant) in &enum_info.variants {
                completions.push(AnalysisCompletion {
                    name: name.clone(),
                    kind: "variant".to_string(),
                    detail: variant
                        .payloads
                        .first()
                        .map(|payload| format!("{}({}) -> {}", name, payload.ty, base_name))
                        .unwrap_or_else(|| format!("{} -> {}", name, base_name)),
                });
            }
        }

        for builtin in builtin_enum_variant_completions(base_name) {
            completions.push(builtin);
        }
        for builtin in builtin_member_completions(receiver_type) {
            completions.push(builtin);
        }
        completions
    }

    fn trait_impls_in_scope(&self) -> impl Iterator<Item = &crate::sema::TraitImplInfo> + '_ {
        self.program.trait_impls.iter().chain(
            self.program
                .module_registry
                .values()
                .flat_map(|namespace| namespace.trait_impls.iter()),
        )
    }

    fn trait_impl_substitutions(
        &self,
        trait_impl: &crate::sema::TraitImplInfo,
        actual: &Type,
    ) -> Option<std::collections::HashMap<String, Type>> {
        let type_params = trait_impl
            .type_params
            .iter()
            .cloned()
            .collect::<BTreeSet<_>>();
        let mut substitutions = std::collections::HashMap::new();
        if !crate::sema::type_pattern_matches(
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
        trait_impl: &crate::sema::TraitImplInfo,
        actual: &Type,
        bound: &TraitBound,
    ) -> Option<std::collections::HashMap<String, Type>> {
        if trait_impl.trait_name != bound.trait_name
            || trait_impl.trait_args.len() != bound.trait_args.len()
        {
            return None;
        }
        let type_params = trait_impl
            .type_params
            .iter()
            .cloned()
            .collect::<std::collections::BTreeSet<_>>();
        let mut substitutions = std::collections::HashMap::new();
        if !crate::sema::type_pattern_matches(
            &trait_impl.for_type,
            actual,
            &type_params,
            &mut substitutions,
        ) {
            return None;
        }
        for (pattern, actual_arg) in trait_impl.trait_args.iter().zip(&bound.trait_args) {
            if !crate::sema::type_pattern_matches(
                pattern,
                actual_arg,
                &type_params,
                &mut substitutions,
            ) {
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

    fn trait_method_for_receiver(
        &self,
        receiver_type: &Type,
        field: &str,
    ) -> Option<(
        &crate::sema::TraitImplInfo,
        &crate::sema::TraitImplMethodInfo,
        std::collections::HashMap<String, Type>,
    )> {
        self.trait_impls_in_scope()
            .filter_map(|trait_impl| {
                self.trait_impl_substitutions(trait_impl, receiver_type)
                    .map(|substitutions| (trait_impl, substitutions))
            })
            .find_map(|(trait_impl, substitutions)| {
                trait_impl
                    .methods
                    .get(field)
                    .map(|method| (trait_impl, method, substitutions))
            })
    }

    fn module_namespace(&self, path: &str) -> Option<&crate::sema::ModuleNamespace> {
        let mut segments = path.split('.');
        let first = segments.next()?;
        let mut namespace = self.program.imported_modules.get(first)?;
        for segment in segments {
            namespace = namespace.modules.get(segment)?;
        }
        Some(namespace)
    }

    fn current_source_path(&self) -> Option<String> {
        self.program.source_path.clone()
    }

    fn module_source_path(&self, module_name: &str) -> Option<String> {
        if self.program.module_name == module_name {
            return self.current_source_path();
        }
        self.program
            .module_registry
            .get(module_name)
            .and_then(|namespace| namespace.source_path.clone())
    }

    fn definition_range(&self, module_name: &str, span: Span, len: usize) -> AnalysisRange {
        range_from_span_with_path(span, len, self.module_source_path(module_name))
    }

    fn function_definition(&self, function: &FunctionInfo) -> AnalysisRange {
        self.definition_range(
            &function.module_name,
            function.decl.span,
            function.decl.name.len(),
        )
    }

    fn class_definition(&self, class_info: &ClassInfo) -> AnalysisRange {
        self.definition_range(
            &class_info.module_name,
            class_info.decl.span,
            class_info.decl.name.len(),
        )
    }

    fn enum_definition(&self, enum_info: &EnumInfo) -> AnalysisRange {
        self.definition_range(
            &enum_info.module_name,
            enum_info.decl.span,
            enum_info.decl.name.len(),
        )
    }

    fn trait_definition(&self, trait_info: &crate::sema::TraitInfo) -> AnalysisRange {
        self.definition_range(
            &trait_info.module_name,
            trait_info.decl.span,
            trait_info.decl.name.len(),
        )
    }

    fn find_imported_module_range(&self, target_path: &str) -> Option<AnalysisRange> {
        if let Some(namespace) = self.module_namespace(target_path) {
            if let Some(file_path) = &namespace.source_path {
                return Some(AnalysisRange {
                    file_path: Some(file_path.clone()),
                    line: 0,
                    start_character: 0,
                    end_character: 0,
                });
            }
        }
        let target_segments = target_path.split('.').collect::<Vec<_>>();
        for import in &self.program.module.imports {
            let ImportKind::Module { path } = &import.kind else {
                continue;
            };
            if path.len() < target_segments.len() {
                continue;
            }
            if !path
                .iter()
                .take(target_segments.len())
                .map(String::as_str)
                .eq(target_segments.iter().copied())
            {
                continue;
            }
            let line_index = import.span.line.checked_sub(1)?;
            let line = *self.source_lines.get(line_index)?;
            let token = target_segments.join(".");
            if let Some((start, end)) = line.find(&token).map(|start| (start, start + token.len()))
            {
                return Some(AnalysisRange {
                    file_path: self.current_source_path(),
                    line: line_index,
                    start_character: start,
                    end_character: end,
                });
            }
        }
        None
    }

    fn resolve_named_enum_info(&self, name: &str) -> Option<&EnumInfo> {
        if let Some((module_path, item_name)) = name.rsplit_once('.') {
            return self.module_namespace(module_path)?.enums.get(item_name);
        }
        self.program.enums.get(name)
    }

    fn resolve_match_variant_enum(&self, enum_name: &str) -> Option<ResolvedSymbol> {
        match enum_name {
            "Option" => Some(ResolvedSymbol {
                hover: builtin_enum_hover(
                    "Option[T]",
                    "Optional values with `Some(T)` and `None`.",
                ),
                definition: None,
            }),
            "Result" => Some(ResolvedSymbol {
                hover: builtin_enum_hover(
                    "Result[T, E]",
                    "Success-or-error values with `Ok(T)` and `Err(E)`.",
                ),
                definition: None,
            }),
            "SendError" => Some(ResolvedSymbol {
                hover: builtin_enum_hover(
                    "SendError[T]",
                    "Channel send failures that preserve the unsent value.",
                ),
                definition: None,
            }),
            _ => self
                .resolve_named_enum_info(enum_name)
                .map(|enum_info| ResolvedSymbol {
                    hover: format_enum_hover(enum_info),
                    definition: Some(self.enum_definition(enum_info)),
                }),
        }
    }

    fn resolve_match_variant(
        &self,
        scrutinee_type: Option<&Type>,
        variant: &VariantPattern,
    ) -> Option<ResolvedSymbol> {
        if let Some(ty) = scrutinee_type {
            match (base_type_name(ty), variant.variant_name.as_str()) {
                ("Option", "Some") => {
                    return Some(ResolvedSymbol {
                        hover: format_variant_hover("Option", "Some", ty.type_arguments().first()),
                        definition: None,
                    })
                }
                ("Option", "None") => {
                    return Some(ResolvedSymbol {
                        hover: format_variant_hover("Option", "None", None),
                        definition: None,
                    })
                }
                ("Result", "Ok") => {
                    return Some(ResolvedSymbol {
                        hover: format_variant_hover("Result", "Ok", ty.type_arguments().first()),
                        definition: None,
                    })
                }
                ("Result", "Err") => {
                    return Some(ResolvedSymbol {
                        hover: format_variant_hover("Result", "Err", ty.type_arguments().get(1)),
                        definition: None,
                    })
                }
                ("SendError", "Closed") => {
                    return Some(ResolvedSymbol {
                        hover: format_variant_hover(
                            "SendError",
                            "Closed",
                            ty.type_arguments().first(),
                        ),
                        definition: None,
                    })
                }
                _ => {}
            }
        }

        let enum_name = variant
            .enum_name
            .as_deref()
            .or_else(|| scrutinee_type.map(base_type_name))?;
        let enum_info = self.resolve_named_enum_info(enum_name)?;
        let variant_decl = enum_info
            .decl
            .variants
            .iter()
            .find(|decl| decl.name == variant.variant_name)?;
        let payload = enum_info
            .variants
            .get(&variant.variant_name)
            .and_then(|variant_info| variant_info.payloads.first().map(|payload| &payload.ty));
        Some(ResolvedSymbol {
            hover: format_variant_hover(&enum_info.decl.name, &variant.variant_name, payload),
            definition: Some(self.definition_range(
                &enum_info.module_name,
                variant_decl.span,
                variant_decl.name.len(),
            )),
        })
    }

    fn trait_bound_member_completions(&self, bounds: &[TraitBound]) -> Vec<AnalysisCompletion> {
        let mut completions = Vec::new();
        for bound in bounds {
            let Some(trait_info) = self.program.traits.get(&bound.trait_name) else {
                continue;
            };
            for method in trait_info.methods.values() {
                if completions
                    .iter()
                    .any(|existing: &AnalysisCompletion| existing.name == method.decl.name)
                {
                    continue;
                }
                completions.push(AnalysisCompletion {
                    name: method.decl.name.clone(),
                    kind: "method".to_string(),
                    detail: format_function_detail(&method.decl),
                });
            }
        }
        completions
    }

    fn visit_stmts(&mut self, stmts: &[Stmt], scope: &mut BTreeMap<String, BindingInfo>) {
        for stmt in stmts {
            self.visit_stmt(stmt, scope);
        }
    }

    fn visit_stmt(&mut self, stmt: &Stmt, scope: &mut BTreeMap<String, BindingInfo>) {
        match stmt {
            Stmt::Assign(assign) => self.visit_assign(assign, scope),
            Stmt::Return(ret) => {
                if let Some(value) = &ret.value {
                    self.visit_expr(value, scope);
                }
            }
            Stmt::If(if_stmt) => {
                for branch in &if_stmt.branches {
                    self.visit_expr(&branch.condition, scope);
                    let mut branch_scope = scope.clone();
                    self.visit_stmts(&branch.body, &mut branch_scope);
                }
                if let Some(body) = &if_stmt.else_body {
                    let mut else_scope = scope.clone();
                    self.visit_stmts(body, &mut else_scope);
                }
            }
            Stmt::Match(match_stmt) => {
                self.visit_expr(&match_stmt.scrutinee, scope);
                let scrutinee_type = self.infer_expr_type(&match_stmt.scrutinee, scope);
                for arm in &match_stmt.arms {
                    let mut arm_scope = scope.clone();
                    self.visit_match_arm_pattern(arm, scrutinee_type.as_ref());
                    self.bind_match_arm(arm, scrutinee_type.as_ref(), &mut arm_scope);
                    self.visit_stmts(&arm.body, &mut arm_scope);
                }
            }
            Stmt::For(for_stmt) => {
                self.visit_expr(&for_stmt.iterable, scope);
                let mut body_scope = scope.clone();
                let binding_ty = self
                    .infer_iterable_binding_type(&for_stmt.iterable, scope)
                    .unwrap_or(Type::Unit);
                self.bind_named_value(
                    &for_stmt.binding,
                    binding_ty,
                    for_stmt.span.line,
                    "local",
                    &mut body_scope,
                );
                self.visit_stmts(&for_stmt.body, &mut body_scope);
            }
            Stmt::With(with_stmt) => {
                self.visit_expr(&with_stmt.value, scope);
                let mut body_scope = scope.clone();
                let binding_ty = self
                    .infer_expr_type(&with_stmt.value, scope)
                    .unwrap_or(Type::Unit);
                self.bind_named_value(
                    &with_stmt.binding,
                    binding_ty,
                    with_stmt.span.line,
                    "local",
                    &mut body_scope,
                );
                self.visit_stmts(&with_stmt.body, &mut body_scope);
            }
            Stmt::Select(select_stmt) => {
                for arm in &select_stmt.arms {
                    self.visit_select_arm(arm, scope);
                }
            }
            Stmt::While(while_stmt) => {
                self.visit_expr(&while_stmt.condition, scope);
                let mut loop_scope = scope.clone();
                self.visit_stmts(&while_stmt.body, &mut loop_scope);
            }
            Stmt::Pass(_) | Stmt::Break(_) | Stmt::Continue(_) => {}
            Stmt::Expr(expr_stmt) => self.visit_expr(&expr_stmt.expr, scope),
        }
    }

    fn visit_assign(&mut self, assign: &AssignStmt, scope: &mut BTreeMap<String, BindingInfo>) {
        self.visit_expr(&assign.value, scope);

        match &assign.target {
            AssignTarget::Name(name) => {
                if let Some(existing) = scope.get(name) {
                    self.push_occurrence(
                        self.find_identifier_range(assign.span.line, name)
                            .unwrap_or_else(|| range_from_span(assign.span, name.len())),
                        existing.hover.clone(),
                        Some(existing.definition.clone()),
                    );
                    return;
                }

                let binding_ty = assign
                    .annotation
                    .as_ref()
                    .map(lower_type_ref)
                    .or_else(|| self.infer_expr_type(&assign.value, scope))
                    .unwrap_or(Type::Unit);
                self.bind_named_value(name, binding_ty, assign.span.line, "binding", scope);
            }
            AssignTarget::Member { object, field } => {
                self.visit_expr(object, scope);
                if let Some(member) = self.resolve_member_expr(object, field, scope) {
                    if let Some(range) = self.find_identifier_range(assign.span.line, field) {
                        self.push_occurrence(range, member.hover, member.definition);
                    }
                }
            }
            AssignTarget::Index { object, index } => {
                self.visit_expr(object, scope);
                self.visit_expr(index, scope);
            }
        }
    }

    fn visit_select_arm(&mut self, arm: &SelectArm, scope: &mut BTreeMap<String, BindingInfo>) {
        self.visit_expr(&arm.expr, scope);
        let mut arm_scope = scope.clone();
        if let Some(binding) = &arm.binding {
            let binding_ty = self.infer_expr_type(&arm.expr, scope).unwrap_or(Type::Unit);
            self.bind_named_value(binding, binding_ty, arm.span.line, "local", &mut arm_scope);
        }
        self.visit_stmts(&arm.body, &mut arm_scope);
    }

    fn bind_match_arm(
        &mut self,
        arm: &MatchArm,
        scrutinee_type: Option<&Type>,
        scope: &mut BTreeMap<String, BindingInfo>,
    ) {
        let Pattern::Variant(variant) = &arm.pattern else {
            return;
        };
        let Some(binding_name) = variant
            .subpatterns
            .iter()
            .find_map(|pattern| match pattern {
                Pattern::Binding(binding) => Some(binding.name.as_str()),
                _ => None,
            })
        else {
            return;
        };
        let binding_ty = self
            .match_binding_type(
                scrutinee_type,
                variant.enum_name.as_deref(),
                &variant.variant_name,
            )
            .unwrap_or(Type::Unit);
        self.bind_named_value(binding_name, binding_ty, arm.span.line, "local", scope);
    }

    fn bind_match_arm_scope(
        &self,
        arm: &MatchArm,
        scrutinee_type: Option<&Type>,
        scope: &mut BTreeMap<String, BindingInfo>,
    ) {
        let Pattern::Variant(variant) = &arm.pattern else {
            return;
        };
        let Some(binding_name) = variant
            .subpatterns
            .iter()
            .find_map(|pattern| match pattern {
                Pattern::Binding(binding) => Some(binding.name.as_str()),
                _ => None,
            })
        else {
            return;
        };
        let binding_ty = self
            .match_binding_type(
                scrutinee_type,
                variant.enum_name.as_deref(),
                &variant.variant_name,
            )
            .unwrap_or(Type::Unit);
        self.insert_scope_binding(binding_name, binding_ty, arm.span.line, "local", scope);
    }

    fn visit_match_arm_pattern(&mut self, arm: &MatchArm, scrutinee_type: Option<&Type>) {
        let Pattern::Variant(variant) = &arm.pattern else {
            return;
        };
        if let Some(resolved) = self.resolve_match_variant(scrutinee_type, variant) {
            if let Some(range) = self.find_match_variant_range(arm.span.line, variant) {
                self.push_occurrence(range, resolved.hover, resolved.definition);
            }
        }
        if let Some(enum_name) = &variant.enum_name {
            if let Some(resolved_enum) = self.resolve_match_variant_enum(enum_name) {
                if let Some(range) = self.find_match_enum_range(arm.span.line, enum_name) {
                    self.push_occurrence(range, resolved_enum.hover, resolved_enum.definition);
                }
            }
        }
    }

    fn bind_named_value(
        &mut self,
        name: &str,
        ty: Type,
        line: usize,
        kind: &str,
        scope: &mut BTreeMap<String, BindingInfo>,
    ) {
        let definition = self
            .find_identifier_range(line, name)
            .unwrap_or(AnalysisRange {
                file_path: self.current_source_path(),
                line: line.saturating_sub(1),
                start_character: 0,
                end_character: name.len(),
            });
        let hover = format_value_hover(kind, name, &ty);
        scope.insert(
            name.to_string(),
            BindingInfo {
                ty,
                trait_bounds: Vec::new(),
                definition: definition.clone(),
                hover: hover.clone(),
            },
        );
        self.push_occurrence(definition.clone(), hover, Some(definition));
    }

    fn visit_expr(&mut self, expr: &Expr, scope: &BTreeMap<String, BindingInfo>) {
        match &expr.kind {
            ExprKind::Name(name) => {
                if let Some(resolved) = self.resolve_name(name, scope) {
                    self.push_occurrence(
                        range_from_span(expr.span, name.len()),
                        resolved.hover,
                        resolved.definition,
                    );
                }
            }
            ExprKind::Member { object, field } => {
                self.visit_expr(object, scope);
                if let Some(resolved) = self.resolve_member_expr(object, field, scope) {
                    self.push_occurrence(
                        range_from_span(expr.span, field.len()),
                        resolved.hover,
                        resolved.definition,
                    );
                }
            }
            ExprKind::Specialize { expr, .. } => self.visit_expr(expr, scope),
            ExprKind::Call { callee, args } => {
                self.visit_expr(callee, scope);
                for arg in args {
                    self.visit_expr(&arg.value, scope);
                }
            }
            ExprKind::FString(parts) => {
                for part in parts {
                    if let crate::ast::FormatPart::Expr(expr) = part {
                        self.visit_expr(expr, scope);
                    }
                }
            }
            ExprKind::Binary { left, right, .. } => {
                self.visit_expr(left, scope);
                self.visit_expr(right, scope);
            }
            ExprKind::Cast { expr, .. } => self.visit_expr(expr, scope),
            ExprKind::Unary { expr, .. } => self.visit_expr(expr, scope),
            ExprKind::Spawn { value, .. } => self.visit_expr(value, scope),
            ExprKind::Try(inner) | ExprKind::Group(inner) => self.visit_expr(inner, scope),
            ExprKind::List(elements) | ExprKind::Set(elements) => {
                for element in elements {
                    self.visit_expr(element, scope);
                }
            }
            ExprKind::Map(entries) => {
                for entry in entries {
                    self.visit_expr(&entry.key, scope);
                    self.visit_expr(&entry.value, scope);
                }
            }
            ExprKind::Index { object, index } => {
                self.visit_expr(object, scope);
                self.visit_expr(index, scope);
            }
            ExprKind::Match {
                scrutinee, arms, ..
            } => {
                self.visit_expr(scrutinee, scope);
                for arm in arms {
                    self.visit_expr(&arm.value, scope);
                }
            }
            ExprKind::Int(_)
            | ExprKind::DurationMillis(_)
            | ExprKind::Float(_)
            | ExprKind::Bool(_)
            | ExprKind::String(_) => {}
        }
    }

    fn resolve_name(
        &self,
        name: &str,
        scope: &BTreeMap<String, BindingInfo>,
    ) -> Option<ResolvedSymbol> {
        if let Some(binding) = scope.get(name) {
            return Some(ResolvedSymbol {
                hover: binding.hover.clone(),
                definition: Some(binding.definition.clone()),
            });
        }

        if let Some(function) = self.program.functions.get(name) {
            return Some(ResolvedSymbol {
                hover: format_function_hover(&function.decl),
                definition: Some(self.function_definition(function)),
            });
        }

        if let Some(class_info) = self.program.classes.get(name) {
            return Some(ResolvedSymbol {
                hover: format_class_hover(class_info),
                definition: Some(self.class_definition(class_info)),
            });
        }

        if let Some(enum_info) = self.program.enums.get(name) {
            return Some(ResolvedSymbol {
                hover: format_enum_hover(enum_info),
                definition: Some(self.enum_definition(enum_info)),
            });
        }

        if let Some(builtin) = BuiltinFunction::from_name(name) {
            return Some(ResolvedSymbol {
                hover: builtin_function_hover(builtin.detail(), builtin.docs()),
                definition: None,
            });
        }

        if let Some(namespace) = self.program.imported_modules.get(name) {
            return Some(ResolvedSymbol {
                hover: format!("```aurora\nmodule {}\n```", namespace.path),
                definition: self.find_imported_module_range(&namespace.path),
            });
        }

        match name {
            "Option" => Some(ResolvedSymbol {
                hover: builtin_enum_hover(
                    "Option[T]",
                    "Optional values with `Some(T)` and `None`.",
                ),
                definition: None,
            }),
            "Result" => Some(ResolvedSymbol {
                hover: builtin_enum_hover(
                    "Result[T, E]",
                    "Success-or-error values with `Ok(T)` and `Err(E)`.",
                ),
                definition: None,
            }),
            "SendError" => Some(ResolvedSymbol {
                hover: builtin_enum_hover(
                    "SendError[T]",
                    "Channel send failures that preserve the unsent value.",
                ),
                definition: None,
            }),
            _ => None,
        }
    }

    fn resolve_member_expr(
        &self,
        object: &Expr,
        field: &str,
        scope: &BTreeMap<String, BindingInfo>,
    ) -> Option<ResolvedMember> {
        let receiver_type = self.infer_expr_type(object, scope)?;
        self.resolve_member_type(&receiver_type, field)
    }

    fn resolve_member_type(&self, receiver_type: &Type, field: &str) -> Option<ResolvedMember> {
        if let Type::Module(path) = receiver_type {
            let namespace = self.module_namespace(path)?;
            if let Some(child) = namespace.modules.get(field) {
                return Some(ResolvedMember {
                    hover: format!("```aurora\nmodule {}\n```", child.path),
                    definition: self.find_imported_module_range(&child.path),
                    ty: Some(Type::Module(child.path.clone())),
                });
            }
            if let Some(function) = namespace.functions.get(field) {
                return Some(ResolvedMember {
                    hover: format_function_hover(&function.decl),
                    definition: Some(self.function_definition(function)),
                    ty: Some(function.signature.return_type.clone()),
                });
            }
            if let Some(class_info) = namespace.classes.get(field) {
                return Some(ResolvedMember {
                    hover: format_class_hover(class_info),
                    definition: Some(self.class_definition(class_info)),
                    ty: Some(Type::named(&class_info.decl.name)),
                });
            }
            if let Some(enum_info) = namespace.enums.get(field) {
                return Some(ResolvedMember {
                    hover: format_enum_hover(enum_info),
                    definition: Some(self.enum_definition(enum_info)),
                    ty: Some(Type::named(&enum_info.decl.name)),
                });
            }
            if let Some(trait_info) = namespace.traits.get(field) {
                return Some(ResolvedMember {
                    hover: format!("```aurora\ntrait {}\n```", trait_info.decl.name),
                    definition: Some(self.trait_definition(trait_info)),
                    ty: None,
                });
            }
            return None;
        }

        let base_name = base_type_name(receiver_type);
        if let Some(class_info) = self.program.classes.get(base_name) {
            if let Some(field_info) = class_info.fields.get(field) {
                return Some(ResolvedMember {
                    hover: format_value_hover("field", field, &field_info.ty),
                    definition: Some(self.definition_range(
                        &class_info.module_name,
                        field_info.span,
                        field.len(),
                    )),
                    ty: Some(field_info.ty.clone()),
                });
            }
            if let Some(method_info) = class_info.methods.get(field) {
                return Some(ResolvedMember {
                    hover: format_method_hover(&method_info.decl),
                    definition: Some(self.definition_range(
                        &class_info.module_name,
                        method_info.decl.span,
                        method_info.decl.name.len(),
                    )),
                    ty: Some(method_info.signature.return_type.clone()),
                });
            }
        }

        if let Some((trait_impl, trait_method, substitutions)) =
            self.trait_method_for_receiver(receiver_type, field)
        {
            return Some(ResolvedMember {
                hover: format_method_hover(&trait_method.decl),
                definition: Some(self.definition_range(
                    &trait_impl.module_name,
                    trait_method.decl.span,
                    trait_method.decl.name.len(),
                )),
                ty: Some(crate::sema::substitute_type(
                    &trait_method.signature.return_type,
                    &substitutions,
                )),
            });
        }

        if base_name == "MapEntry" {
            return match field {
                "key" => Some(ResolvedMember {
                    hover: format_value_hover(
                        "field",
                        "key",
                        &receiver_type
                            .type_arguments()
                            .first()
                            .cloned()
                            .unwrap_or(Type::named("Unknown")),
                    ),
                    definition: None,
                    ty: receiver_type.type_arguments().first().cloned(),
                }),
                "value" => Some(ResolvedMember {
                    hover: format_value_hover(
                        "field",
                        "value",
                        &receiver_type
                            .type_arguments()
                            .get(1)
                            .cloned()
                            .unwrap_or(Type::named("Unknown")),
                    ),
                    definition: None,
                    ty: receiver_type.type_arguments().get(1).cloned(),
                }),
                _ => None,
            };
        }

        if let Some(enum_info) = self.program.enums.get(base_name) {
            if let Some(variant_info) = enum_info.variants.get(field) {
                return Some(ResolvedMember {
                    hover: format_variant_hover(
                        base_name,
                        field,
                        variant_info.payloads.first().map(|payload| &payload.ty),
                    ),
                    definition: Some(self.definition_range(
                        &enum_info.module_name,
                        variant_info.span,
                        field.len(),
                    )),
                    ty: Some(Type::named(base_name)),
                });
            }
        }

        if let Some(builtin_member) = BuiltinMember::resolve(base_name, field) {
            let ty = match builtin_member {
                BuiltinMember::FloatSqrt => Some(Type::named("float64")),
                BuiltinMember::StringLen => Some(Type::named("int32")),
                BuiltinMember::StringContains
                | BuiltinMember::StringStartsWith
                | BuiltinMember::StringEndsWith => Some(Type::named("bool")),
                BuiltinMember::StringSplit => {
                    Some(Type::Named("Vec".to_string(), vec![Type::named("String")]))
                }
                BuiltinMember::StringReplace
                | BuiltinMember::StringToLower
                | BuiltinMember::StringToUpper
                | BuiltinMember::StringTrim
                | BuiltinMember::StringJoin
                | BuiltinMember::ScalarToString => Some(Type::named("String")),
                BuiltinMember::StringStripPrefix | BuiltinMember::StringStripSuffix => Some(
                    Type::Named("Option".to_string(), vec![Type::named("String")]),
                ),
                BuiltinMember::VecLen => Some(Type::named("int32")),
                BuiltinMember::VecIsEmpty => Some(Type::named("bool")),
                BuiltinMember::VecClone => Some(receiver_type.clone()),
                BuiltinMember::VecPush | BuiltinMember::VecClear | BuiltinMember::VecReverse => {
                    Some(Type::Unit)
                }
                BuiltinMember::VecInsert => Some(Type::named("bool")),
                BuiltinMember::VecSwap | BuiltinMember::VecContains => Some(Type::named("bool")),
                BuiltinMember::VecExtend => Some(Type::Unit),
                BuiltinMember::MapLen => Some(Type::named("int32")),
                BuiltinMember::MapIsEmpty => Some(Type::named("bool")),
                BuiltinMember::MapClone => Some(receiver_type.clone()),
                BuiltinMember::MapContainsKey => Some(Type::named("bool")),
                BuiltinMember::MapKeys => receiver_type
                    .type_arguments()
                    .first()
                    .cloned()
                    .map(|key| Type::Named("Vec".to_string(), vec![key])),
                BuiltinMember::MapValues => receiver_type
                    .type_arguments()
                    .get(1)
                    .cloned()
                    .map(|value| Type::Named("Vec".to_string(), vec![value])),
                BuiltinMember::MapItems | BuiltinMember::MapEntries => Some(Type::Named(
                    "Vec".to_string(),
                    vec![Type::Named(
                        "MapEntry".to_string(),
                        vec![
                            receiver_type
                                .type_arguments()
                                .first()
                                .cloned()
                                .unwrap_or(Type::Unit),
                            receiver_type
                                .type_arguments()
                                .get(1)
                                .cloned()
                                .unwrap_or(Type::Unit),
                        ],
                    )],
                )),
                BuiltinMember::MapClear | BuiltinMember::MapExtend => Some(Type::Unit),
                BuiltinMember::MapGet | BuiltinMember::MapSet | BuiltinMember::MapRemove => {
                    let payload = receiver_type
                        .type_arguments()
                        .get(1)
                        .cloned()
                        .unwrap_or(Type::Unit);
                    Some(Type::Named("Option".to_string(), vec![payload]))
                }
                BuiltinMember::VecPop => {
                    let payload = receiver_type
                        .type_arguments()
                        .first()
                        .cloned()
                        .unwrap_or(Type::Unit);
                    Some(Type::Named("Option".to_string(), vec![payload]))
                }
                BuiltinMember::VecGet => {
                    let payload = receiver_type
                        .type_arguments()
                        .first()
                        .cloned()
                        .unwrap_or(Type::Unit);
                    Some(Type::Named("Option".to_string(), vec![payload]))
                }
                BuiltinMember::VecSet => {
                    let payload = receiver_type
                        .type_arguments()
                        .first()
                        .cloned()
                        .unwrap_or(Type::Unit);
                    Some(Type::Named("Option".to_string(), vec![payload]))
                }
                BuiltinMember::VecRemove => {
                    let payload = receiver_type
                        .type_arguments()
                        .first()
                        .cloned()
                        .unwrap_or(Type::Unit);
                    Some(Type::Named("Option".to_string(), vec![payload]))
                }
                BuiltinMember::StringClone => Some(Type::named("String")),
                BuiltinMember::SetLen => Some(Type::named("int32")),
                BuiltinMember::SetIsEmpty => Some(Type::named("bool")),
                BuiltinMember::SetClone => Some(receiver_type.clone()),
                BuiltinMember::SetContains
                | BuiltinMember::SetInsert
                | BuiltinMember::SetRemove => Some(Type::named("bool")),
                BuiltinMember::ChannelClone => Some(receiver_type.clone()),
                BuiltinMember::ChannelSend => {
                    let payload = receiver_type
                        .type_arguments()
                        .first()
                        .cloned()
                        .unwrap_or(Type::Unit);
                    Some(Type::Named(
                        "Result".to_string(),
                        vec![
                            Type::Unit,
                            Type::Named("SendError".to_string(), vec![payload]),
                        ],
                    ))
                }
                BuiltinMember::ChannelRecv => {
                    let payload = receiver_type
                        .type_arguments()
                        .first()
                        .cloned()
                        .unwrap_or(Type::Unit);
                    Some(Type::Named("Option".to_string(), vec![payload]))
                }
                BuiltinMember::ChannelClose | BuiltinMember::TaskGroupCancel => Some(Type::Unit),
                BuiltinMember::TaskClone => Some(receiver_type.clone()),
                BuiltinMember::TaskJoin => receiver_type.type_arguments().first().cloned(),
            };
            return Some(ResolvedMember {
                hover: builtin_function_hover(builtin_member.detail(), builtin_member.docs()),
                definition: None,
                ty,
            });
        }

        match base_name {
            "TaskGroup" if field == "spawn" => Some(ResolvedMember {
                hover: builtin_function_hover(
                    "spawn(function, ...) -> Task[T]",
                    "Spawns a child task in the current task group.",
                ),
                definition: None,
                ty: Some(Type::Named("Task".to_string(), vec![Type::Unit])),
            }),
            "Option" if field == "Some" => Some(ResolvedMember {
                hover: format_variant_hover("Option", "Some", Some(&Type::named("T"))),
                definition: None,
                ty: Some(Type::named("Option")),
            }),
            "Option" if field == "None" => Some(ResolvedMember {
                hover: format_variant_hover("Option", "None", None),
                definition: None,
                ty: Some(Type::named("Option")),
            }),
            "Result" if field == "Ok" => Some(ResolvedMember {
                hover: format_variant_hover("Result", "Ok", Some(&Type::named("T"))),
                definition: None,
                ty: Some(Type::named("Result")),
            }),
            "Result" if field == "Err" => Some(ResolvedMember {
                hover: format_variant_hover("Result", "Err", Some(&Type::named("E"))),
                definition: None,
                ty: Some(Type::named("Result")),
            }),
            "SendError" if field == "Closed" => Some(ResolvedMember {
                hover: format_variant_hover("SendError", "Closed", Some(&Type::named("T"))),
                definition: None,
                ty: Some(Type::named("SendError")),
            }),
            _ => None,
        }
    }

    fn infer_expr_type(&self, expr: &Expr, scope: &BTreeMap<String, BindingInfo>) -> Option<Type> {
        match &expr.kind {
            ExprKind::Int(_) => Some(Type::named("int32")),
            ExprKind::DurationMillis(_) => Some(Type::named("Duration")),
            ExprKind::Float(_) => Some(Type::named("float64")),
            ExprKind::Bool(_) => Some(Type::named("bool")),
            ExprKind::String(_) => Some(Type::named("String")),
            ExprKind::List(elements) => Some(Type::Named(
                "Vec".to_string(),
                vec![elements
                    .first()
                    .and_then(|element| self.infer_expr_type(element, scope))
                    .unwrap_or(Type::named("Unknown"))],
            )),
            ExprKind::Set(elements) => Some(Type::Named(
                "Set".to_string(),
                vec![elements
                    .first()
                    .and_then(|element| self.infer_expr_type(element, scope))
                    .unwrap_or(Type::named("Unknown"))],
            )),
            ExprKind::Map(entries) => Some(Type::Named(
                "Map".to_string(),
                vec![
                    entries
                        .first()
                        .and_then(|entry| self.infer_expr_type(&entry.key, scope))
                        .unwrap_or(Type::named("Unknown")),
                    entries
                        .first()
                        .and_then(|entry| self.infer_expr_type(&entry.value, scope))
                        .unwrap_or(Type::named("Unknown")),
                ],
            )),
            ExprKind::FString(_) => Some(Type::named("String")),
            ExprKind::Specialize { expr, type_args } => match &expr.kind {
                ExprKind::Name(name)
                    if self.program.classes.contains_key(name)
                        || self.program.enums.contains_key(name)
                        || matches!(
                            name.as_str(),
                            "Option"
                                | "Result"
                                | "SendError"
                                | "Channel"
                                | "Vec"
                                | "Set"
                                | "Map"
                                | "Task"
                        ) =>
                {
                    Some(Type::Named(
                        name.clone(),
                        type_args.iter().map(lower_type_ref).collect(),
                    ))
                }
                _ => self.infer_expr_type(expr, scope),
            },
            ExprKind::Group(inner) => self.infer_expr_type(inner, scope),
            ExprKind::Cast { ty, .. } => Some(lower_type_ref(ty)),
            ExprKind::Unary { op, expr } => {
                let inner_ty = self.infer_expr_type(expr, scope)?;
                match op {
                    crate::ast::UnaryOp::Not => Some(Type::named("bool")),
                    crate::ast::UnaryOp::Neg => Some(inner_ty),
                }
            }
            ExprKind::Try(inner) => {
                let inner_ty = self.infer_expr_type(inner, scope)?;
                match inner_ty {
                    Type::Named(name, mut args) if name == "Result" && args.len() == 2 => {
                        Some(args.remove(0))
                    }
                    _ => None,
                }
            }
            ExprKind::Spawn { detached, value } => {
                if *detached {
                    Some(Type::Unit)
                } else {
                    let inner = self.infer_expr_type(value, scope).unwrap_or(Type::Unit);
                    Some(Type::Named("Task".to_string(), vec![inner]))
                }
            }
            ExprKind::Name(name) => {
                if let Some(binding) = scope.get(name) {
                    return Some(binding.ty.clone());
                }
                if let Some(namespace) = self.program.imported_modules.get(name) {
                    return Some(Type::Module(namespace.path.clone()));
                }
                if self.program.classes.contains_key(name)
                    || self.program.enums.contains_key(name)
                    || matches!(name.as_str(), "Option" | "Result" | "SendError")
                {
                    return Some(Type::named(name));
                }
                if let Some(function) = self.program.functions.get(name) {
                    return Some(function.signature.return_type.clone());
                }
                builtin_function_return_type(name)
            }
            ExprKind::Member { object, field } => self
                .resolve_member_expr(object, field, scope)
                .and_then(|member| member.ty),
            ExprKind::Index { object, .. } => {
                self.infer_expr_type(object, scope)
                    .and_then(|ty| match base_type_name(&ty) {
                        "Vec" => ty.type_arguments().first().cloned(),
                        "Map" => ty.type_arguments().get(1).cloned(),
                        _ => None,
                    })
            }
            ExprKind::Call { callee, args } => self.infer_call_type(callee, args, scope),
            ExprKind::Match { arms, .. } => arms
                .first()
                .and_then(|arm| self.infer_expr_type(&arm.value, scope)),
            ExprKind::Binary { op, left, right } => {
                let left_ty = self.infer_expr_type(left, scope)?;
                let right_ty = self.infer_expr_type(right, scope)?;
                match op {
                    BinaryOp::And | BinaryOp::Or => Some(Type::named("bool")),
                    BinaryOp::Eq
                    | BinaryOp::NotEq
                    | BinaryOp::Less
                    | BinaryOp::LessEq
                    | BinaryOp::Greater
                    | BinaryOp::GreaterEq => Some(Type::named("bool")),
                    BinaryOp::Add
                        if left_ty == Type::named("String")
                            && right_ty == Type::named("String") =>
                    {
                        Some(Type::named("String"))
                    }
                    _ if left_ty == Type::named("float64")
                        || right_ty == Type::named("float64") =>
                    {
                        Some(Type::named("float64"))
                    }
                    _ if left_ty == right_ty => Some(left_ty),
                    _ => None,
                }
            }
        }
    }

    fn infer_call_type(
        &self,
        callee: &Expr,
        args: &[crate::ast::Argument],
        scope: &BTreeMap<String, BindingInfo>,
    ) -> Option<Type> {
        match &callee.kind {
            ExprKind::Name(name) => {
                if let Some(function) = self.program.functions.get(name) {
                    return Some(function.signature.return_type.clone());
                }
                if self.program.classes.contains_key(name) {
                    return Some(Type::named(name));
                }
                return match BuiltinFunction::from_name(name)? {
                    BuiltinFunction::Abs | BuiltinFunction::Min | BuiltinFunction::Max => args
                        .first()
                        .and_then(|arg| self.infer_expr_type(&arg.value, scope)),
                    BuiltinFunction::Sqrt => args
                        .first()
                        .and_then(|arg| self.infer_expr_type(&arg.value, scope)),
                    _ => builtin_function_return_type(name),
                };
            }
            ExprKind::Member { object, field } => {
                if let ExprKind::Name(enum_name) = &object.kind {
                    if matches!(enum_name.as_str(), "Option" | "Result" | "SendError") {
                        return infer_builtin_variant_call(enum_name, field, args, |expr| {
                            self.infer_expr_type(expr, scope)
                        });
                    }
                    if self.program.enums.contains_key(enum_name) {
                        return Some(Type::named(enum_name));
                    }
                }
                self.resolve_member_expr(object, field, scope)
                    .and_then(|member| member.ty)
            }
            ExprKind::Specialize { expr, type_args } => match &expr.kind {
                ExprKind::Name(name)
                    if self.program.classes.contains_key(name)
                        || matches!(name.as_str(), "Channel" | "Vec" | "Set" | "Map" | "Task") =>
                {
                    Some(Type::Named(
                        name.clone(),
                        type_args.iter().map(lower_type_ref).collect(),
                    ))
                }
                _ => self.infer_call_type(expr, args, scope),
            },
            _ => None,
        }
    }

    fn infer_iterable_binding_type(
        &self,
        iterable: &Expr,
        scope: &BTreeMap<String, BindingInfo>,
    ) -> Option<Type> {
        if matches!(
            &iterable.kind,
            ExprKind::Call { callee, .. } if matches!(&callee.kind, ExprKind::Name(name) if name == "range")
        ) {
            return Some(Type::named("int32"));
        }

        let iterable_ty = self.infer_expr_type(iterable, scope)?;
        match base_type_name(&iterable_ty) {
            "Channel" | "Vec" | "Set" => iterable_ty.type_arguments().first().cloned(),
            _ => None,
        }
    }

    fn match_binding_type(
        &self,
        scrutinee_type: Option<&Type>,
        enum_name: Option<&str>,
        variant_name: &str,
    ) -> Option<Type> {
        if let Some(ty) = scrutinee_type {
            match (base_type_name(ty), variant_name) {
                ("Option", "Some") => return ty.type_arguments().first().cloned(),
                ("Result", "Ok") => return ty.type_arguments().first().cloned(),
                ("Result", "Err") => return ty.type_arguments().get(1).cloned(),
                ("SendError", "Closed") => return ty.type_arguments().first().cloned(),
                _ => {}
            }
        }

        self.resolve_named_enum_info(enum_name?)
            .and_then(|info| info.variants.get(variant_name))
            .and_then(|variant| variant.payloads.first().map(|payload| payload.ty.clone()))
    }

    fn push_occurrence(
        &mut self,
        range: AnalysisRange,
        hover: String,
        definition: Option<AnalysisRange>,
    ) {
        self.output.occurrences.push(AnalysisOccurrence {
            line: range.line,
            start_character: range.start_character,
            end_character: range.end_character,
            hover,
            definition,
        });
    }

    fn insert_scope_binding(
        &self,
        name: &str,
        ty: Type,
        line: usize,
        kind: &str,
        scope: &mut BTreeMap<String, BindingInfo>,
    ) {
        let definition = self
            .find_identifier_range(line, name)
            .unwrap_or(AnalysisRange {
                file_path: self.current_source_path(),
                line: line.saturating_sub(1),
                start_character: 0,
                end_character: name.len(),
            });
        let hover = format_value_hover(kind, name, &ty);
        scope.insert(
            name.to_string(),
            BindingInfo {
                ty,
                trait_bounds: Vec::new(),
                definition,
                hover,
            },
        );
    }

    fn find_identifier_range(&self, line_number: usize, name: &str) -> Option<AnalysisRange> {
        let line_index = line_number.checked_sub(1)?;
        let text = *self.source_lines.get(line_index)?;
        find_identifier_in_line(text, name).map(|(start, end)| AnalysisRange {
            file_path: self.current_source_path(),
            line: line_index,
            start_character: start,
            end_character: end,
        })
    }

    fn find_match_enum_range(&self, line_number: usize, enum_name: &str) -> Option<AnalysisRange> {
        let line_index = line_number.checked_sub(1)?;
        let text = *self.source_lines.get(line_index)?;
        text.find(enum_name).map(|start| AnalysisRange {
            file_path: self.current_source_path(),
            line: line_index,
            start_character: start,
            end_character: start + enum_name.len(),
        })
    }

    fn find_match_variant_range(
        &self,
        line_number: usize,
        variant: &VariantPattern,
    ) -> Option<AnalysisRange> {
        let line_index = line_number.checked_sub(1)?;
        let text = *self.source_lines.get(line_index)?;
        let token = variant
            .enum_name
            .as_ref()
            .map(|enum_name| format!("{}.{}", enum_name, variant.variant_name))
            .unwrap_or_else(|| variant.variant_name.clone());
        let start = text.find(&token)?;
        let variant_start = start + token.len().saturating_sub(variant.variant_name.len());
        Some(AnalysisRange {
            file_path: self.current_source_path(),
            line: line_index,
            start_character: variant_start,
            end_character: variant_start + variant.variant_name.len(),
        })
    }
}

fn infer_builtin_variant_call<F>(
    enum_name: &str,
    variant_name: &str,
    args: &[crate::ast::Argument],
    infer_arg: F,
) -> Option<Type>
where
    F: Fn(&Expr) -> Option<Type>,
{
    match (enum_name, variant_name) {
        ("Option", "Some") => Some(Type::Named(
            "Option".to_string(),
            vec![args
                .first()
                .and_then(|arg| infer_arg(&arg.value))
                .unwrap_or(Type::Unit)],
        )),
        ("Option", "None") => Some(Type::Named("Option".to_string(), vec![Type::Unit])),
        ("Result", "Ok") => Some(Type::Named(
            "Result".to_string(),
            vec![
                args.first()
                    .and_then(|arg| infer_arg(&arg.value))
                    .unwrap_or(Type::Unit),
                Type::Unit,
            ],
        )),
        ("Result", "Err") => Some(Type::Named(
            "Result".to_string(),
            vec![
                Type::Unit,
                args.first()
                    .and_then(|arg| infer_arg(&arg.value))
                    .unwrap_or(Type::Unit),
            ],
        )),
        ("SendError", "Closed") => Some(Type::Named(
            "SendError".to_string(),
            vec![args
                .first()
                .and_then(|arg| infer_arg(&arg.value))
                .unwrap_or(Type::Unit)],
        )),
        _ => None,
    }
}

fn symbols_from_module(module: &Module) -> Vec<AnalysisSymbol> {
    let mut symbols = Vec::new();
    for item in &module.items {
        match item {
            Item::Class(class_decl) => {
                symbols.push(AnalysisSymbol {
                    name: class_decl.name.clone(),
                    kind: "class".to_string(),
                    detail: String::new(),
                    line: class_decl.span.line.saturating_sub(1),
                    start_character: class_decl.span.column.saturating_sub(1),
                    end_character: class_decl.span.column.saturating_sub(1) + class_decl.name.len(),
                    children: class_decl
                        .fields
                        .iter()
                        .map(|field| AnalysisSymbol {
                            name: field.name.clone(),
                            kind: "field".to_string(),
                            detail: lower_type_ref(&field.ty).to_string(),
                            line: field.span.line.saturating_sub(1),
                            start_character: field.span.column.saturating_sub(1),
                            end_character: field.span.column.saturating_sub(1) + field.name.len(),
                            children: Vec::new(),
                        })
                        .chain(class_decl.methods.iter().map(|method| AnalysisSymbol {
                            name: method.name.clone(),
                            kind: "method".to_string(),
                            detail: lower_type_ref(&method.return_type).to_string(),
                            line: method.span.line.saturating_sub(1),
                            start_character: method.span.column.saturating_sub(1),
                            end_character: method.span.column.saturating_sub(1) + method.name.len(),
                            children: Vec::new(),
                        }))
                        .collect(),
                });
            }
            Item::Enum(enum_decl) => {
                symbols.push(AnalysisSymbol {
                    name: enum_decl.name.clone(),
                    kind: "enum".to_string(),
                    detail: String::new(),
                    line: enum_decl.span.line.saturating_sub(1),
                    start_character: enum_decl.span.column.saturating_sub(1),
                    end_character: enum_decl.span.column.saturating_sub(1) + enum_decl.name.len(),
                    children: enum_decl
                        .variants
                        .iter()
                        .map(|variant| AnalysisSymbol {
                            name: variant.name.clone(),
                            kind: "variant".to_string(),
                            detail: if variant.payloads.is_empty() {
                                String::new()
                            } else {
                                variant
                                    .payloads
                                    .iter()
                                    .map(|payload| lower_type_ref(&payload.ty).to_string())
                                    .collect::<Vec<_>>()
                                    .join(", ")
                            },
                            line: variant.span.line.saturating_sub(1),
                            start_character: variant.span.column.saturating_sub(1),
                            end_character: variant.span.column.saturating_sub(1)
                                + variant.name.len(),
                            children: Vec::new(),
                        })
                        .collect(),
                });
            }
            Item::Function(function_decl) => {
                symbols.push(AnalysisSymbol {
                    name: function_decl.name.clone(),
                    kind: "function".to_string(),
                    detail: lower_type_ref(&function_decl.return_type).to_string(),
                    line: function_decl.span.line.saturating_sub(1),
                    start_character: function_decl.span.column.saturating_sub(1),
                    end_character: function_decl.span.column.saturating_sub(1)
                        + function_decl.name.len(),
                    children: Vec::new(),
                });
            }
            Item::Trait(trait_decl) => {
                symbols.push(AnalysisSymbol {
                    name: trait_decl.name.clone(),
                    kind: "trait".to_string(),
                    detail: String::new(),
                    line: trait_decl.span.line.saturating_sub(1),
                    start_character: trait_decl.span.column.saturating_sub(1),
                    end_character: trait_decl.span.column.saturating_sub(1) + trait_decl.name.len(),
                    children: trait_decl
                        .methods
                        .iter()
                        .map(|method| AnalysisSymbol {
                            name: method.name.clone(),
                            kind: "method".to_string(),
                            detail: lower_type_ref(&method.return_type).to_string(),
                            line: method.span.line.saturating_sub(1),
                            start_character: method.span.column.saturating_sub(1),
                            end_character: method.span.column.saturating_sub(1) + method.name.len(),
                            children: Vec::new(),
                        })
                        .collect(),
                });
            }
            Item::Impl(_) => {}
        }
    }
    symbols
}

fn analysis_diagnostic(error: &Diagnostic) -> AnalysisDiagnostic {
    let (line, start_character) = error
        .span
        .map(|span| (span.line.saturating_sub(1), span.column.saturating_sub(1)))
        .unwrap_or((0, 0));
    AnalysisDiagnostic {
        line,
        start_character,
        end_character: start_character + 1,
        message: error.message.clone(),
        severity: 1,
    }
}

fn range_from_span(span: Span, len: usize) -> AnalysisRange {
    AnalysisRange {
        file_path: None,
        line: span.line.saturating_sub(1),
        start_character: span.column.saturating_sub(1),
        end_character: span.column.saturating_sub(1) + len,
    }
}

fn range_from_span_with_path(span: Span, len: usize, file_path: Option<String>) -> AnalysisRange {
    AnalysisRange {
        file_path,
        line: span.line.saturating_sub(1),
        start_character: span.column.saturating_sub(1),
        end_character: span.column.saturating_sub(1) + len,
    }
}

fn find_identifier_in_line(line: &str, name: &str) -> Option<(usize, usize)> {
    let mut search_from = 0;
    while let Some(offset) = line[search_from..].find(name) {
        let start = search_from + offset;
        let end = start + name.len();
        let before_ok = start == 0
            || !line[..start]
                .chars()
                .next_back()
                .map(is_identifier_continue)
                .unwrap_or(false);
        let after_ok = end == line.len()
            || !line[end..]
                .chars()
                .next()
                .map(is_identifier_continue)
                .unwrap_or(false);
        if before_ok && after_ok {
            return Some((start, end));
        }
        search_from = end;
    }
    None
}

fn is_identifier_continue(ch: char) -> bool {
    ch.is_ascii_alphanumeric() || ch == '_'
}

fn lower_type_ref(ty: &TypeRef) -> Type {
    if ty.name == "None" {
        return Type::Unit;
    }
    let name = if ty.name == "str" { "String" } else { &ty.name };
    Type::Named(
        name.to_string(),
        ty.args.iter().map(lower_type_ref).collect(),
    )
}

fn base_type_name(ty: &Type) -> &str {
    match ty {
        Type::Unit => "None",
        Type::Module(name) => name.as_str(),
        Type::TypeParam(name) => name.as_str(),
        Type::Named(name, _) => name.as_str(),
    }
}

trait TypeExt {
    fn type_arguments(&self) -> &[Type];
}

impl TypeExt for Type {
    fn type_arguments(&self) -> &[Type] {
        match self {
            Type::Unit => &[],
            Type::Module(_) => &[],
            Type::TypeParam(_) => &[],
            Type::Named(_, args) => args.as_slice(),
        }
    }
}

fn format_value_hover(kind: &str, name: &str, ty: &Type) -> String {
    format!("```aurora\n{} {}: {}\n```", kind, name, ty)
}

fn format_function_hover(function_decl: &FunctionDecl) -> String {
    let params = function_decl
        .params
        .iter()
        .map(|param| format!("{}: {}", param.name, lower_type_ref(&param.ty)))
        .collect::<Vec<_>>()
        .join(", ");
    format!(
        "```aurora\nfunction {}({}) -> {}\n```",
        function_decl.name,
        params,
        lower_type_ref(&function_decl.return_type)
    )
}

fn format_method_hover(method_decl: &FunctionDecl) -> String {
    let params = method_decl
        .params
        .iter()
        .map(|param| format!("{}: {}", param.name, lower_type_ref(&param.ty)))
        .collect::<Vec<_>>()
        .join(", ");
    format!(
        "```aurora\nmethod {}({}) -> {}\n```",
        method_decl.name,
        params,
        lower_type_ref(&method_decl.return_type)
    )
}

fn format_class_hover(class_info: &ClassInfo) -> String {
    let mut fields = class_info
        .fields
        .iter()
        .map(|(name, field)| format!("{}: {}", name, field.ty))
        .collect::<Vec<_>>();
    fields.sort();
    if fields.is_empty() {
        format!("```aurora\nclass {}\n```", class_info.decl.name)
    } else {
        format!(
            "```aurora\nclass {}\n{}\n```",
            class_info.decl.name,
            fields.join("\n")
        )
    }
}

fn format_enum_hover(enum_info: &EnumInfo) -> String {
    format!("```aurora\nenum {}\n```", enum_info.decl.name)
}

fn builtin_enum_hover(detail: &str, docs: &str) -> String {
    format!("```aurora\nenum {}\n```\n{}", detail, docs)
}

fn builtin_function_hover(detail: &str, docs: &str) -> String {
    format!("```aurora\n{}\n```\n{}", detail, docs)
}

fn format_variant_hover(enum_name: &str, variant_name: &str, payload: Option<&Type>) -> String {
    match payload {
        Some(payload) => format!(
            "```aurora\nvariant {}({}) -> {}\n```",
            variant_name, payload, enum_name
        ),
        None => format!("```aurora\nvariant {} -> {}\n```", variant_name, enum_name),
    }
}

const KEYWORDS: &[&str] = &[
    "class", "enum", "trait", "def", "if", "elif", "else", "while", "for", "in", "match", "case",
    "with", "select", "return", "try", "spawn", "detached", "public", "mut", "borrow", "indirect",
    "copy", "break", "continue", "pass",
];

struct CompletionMeta {
    name: &'static str,
    detail: &'static str,
}

const BUILTIN_ENUM_COMPLETIONS: &[CompletionMeta] = &[
    CompletionMeta {
        name: "Option",
        detail: "enum Option[T]",
    },
    CompletionMeta {
        name: "Result",
        detail: "enum Result[T, E]",
    },
    CompletionMeta {
        name: "SendError",
        detail: "enum SendError[T]",
    },
];

fn builtin_enum_variant_completions(base_name: &str) -> Vec<AnalysisCompletion> {
    match base_name {
        "Option" => vec![
            AnalysisCompletion {
                name: "Some".to_string(),
                kind: "variant".to_string(),
                detail: "Some(T) -> Option".to_string(),
            },
            AnalysisCompletion {
                name: "None".to_string(),
                kind: "variant".to_string(),
                detail: "None -> Option".to_string(),
            },
        ],
        "Result" => vec![
            AnalysisCompletion {
                name: "Ok".to_string(),
                kind: "variant".to_string(),
                detail: "Ok(T) -> Result".to_string(),
            },
            AnalysisCompletion {
                name: "Err".to_string(),
                kind: "variant".to_string(),
                detail: "Err(E) -> Result".to_string(),
            },
        ],
        "SendError" => vec![AnalysisCompletion {
            name: "Closed".to_string(),
            kind: "variant".to_string(),
            detail: "Closed(T) -> SendError".to_string(),
        }],
        _ => Vec::new(),
    }
}

fn builtin_member_completions(receiver_type: &Type) -> Vec<AnalysisCompletion> {
    let mut completions = Vec::new();
    match base_type_name(receiver_type) {
        "Vec" => {
            completions.extend([
                AnalysisCompletion {
                    name: "len".to_string(),
                    kind: "method".to_string(),
                    detail: "len() -> int32".to_string(),
                },
                AnalysisCompletion {
                    name: "is_empty".to_string(),
                    kind: "method".to_string(),
                    detail: "is_empty() -> bool".to_string(),
                },
                AnalysisCompletion {
                    name: "clone".to_string(),
                    kind: "method".to_string(),
                    detail: "clone() -> Vec[T]".to_string(),
                },
                AnalysisCompletion {
                    name: "push".to_string(),
                    kind: "method".to_string(),
                    detail: "push(value) -> None".to_string(),
                },
                AnalysisCompletion {
                    name: "pop".to_string(),
                    kind: "method".to_string(),
                    detail: "pop() -> Option[T]".to_string(),
                },
                AnalysisCompletion {
                    name: "get".to_string(),
                    kind: "method".to_string(),
                    detail: "get(index: int32) -> Option[T]".to_string(),
                },
                AnalysisCompletion {
                    name: "set".to_string(),
                    kind: "method".to_string(),
                    detail: "set(index: int32, value: T) -> Option[T]".to_string(),
                },
                AnalysisCompletion {
                    name: "remove".to_string(),
                    kind: "method".to_string(),
                    detail: "remove(index: int32) -> Option[T]".to_string(),
                },
                AnalysisCompletion {
                    name: "swap".to_string(),
                    kind: "method".to_string(),
                    detail: "swap(first: int32, second: int32) -> bool".to_string(),
                },
                AnalysisCompletion {
                    name: "contains".to_string(),
                    kind: "method".to_string(),
                    detail: "contains(value: T) -> bool".to_string(),
                },
                AnalysisCompletion {
                    name: "insert".to_string(),
                    kind: "method".to_string(),
                    detail: "insert(index: int32, value: T) -> bool".to_string(),
                },
                AnalysisCompletion {
                    name: "clear".to_string(),
                    kind: "method".to_string(),
                    detail: "clear() -> None".to_string(),
                },
                AnalysisCompletion {
                    name: "reverse".to_string(),
                    kind: "method".to_string(),
                    detail: "reverse() -> None".to_string(),
                },
                AnalysisCompletion {
                    name: "extend".to_string(),
                    kind: "method".to_string(),
                    detail: "extend(other: Vec[T]) -> None".to_string(),
                },
            ]);
        }
        "Map" => {
            completions.extend([
                AnalysisCompletion {
                    name: "items".to_string(),
                    kind: "method".to_string(),
                    detail: "items() -> Vec[MapEntry[K, V]]".to_string(),
                },
                AnalysisCompletion {
                    name: "entries".to_string(),
                    kind: "method".to_string(),
                    detail: "entries() -> Vec[MapEntry[K, V]]".to_string(),
                },
                AnalysisCompletion {
                    name: "clear".to_string(),
                    kind: "method".to_string(),
                    detail: "clear() -> None".to_string(),
                },
                AnalysisCompletion {
                    name: "extend".to_string(),
                    kind: "method".to_string(),
                    detail: "extend(other: Map[K, V]) -> None".to_string(),
                },
            ]);
        }
        "Set" => {
            completions.extend([
                AnalysisCompletion {
                    name: "len".to_string(),
                    kind: "method".to_string(),
                    detail: "len() -> int32".to_string(),
                },
                AnalysisCompletion {
                    name: "is_empty".to_string(),
                    kind: "method".to_string(),
                    detail: "is_empty() -> bool".to_string(),
                },
                AnalysisCompletion {
                    name: "clone".to_string(),
                    kind: "method".to_string(),
                    detail: "clone() -> Set[T]".to_string(),
                },
                AnalysisCompletion {
                    name: "contains".to_string(),
                    kind: "method".to_string(),
                    detail: "contains(value: T) -> bool".to_string(),
                },
                AnalysisCompletion {
                    name: "insert".to_string(),
                    kind: "method".to_string(),
                    detail: "insert(value: T) -> bool".to_string(),
                },
                AnalysisCompletion {
                    name: "remove".to_string(),
                    kind: "method".to_string(),
                    detail: "remove(value: T) -> bool".to_string(),
                },
            ]);
        }
        "MapEntry" => {
            completions.extend([
                AnalysisCompletion {
                    name: "key".to_string(),
                    kind: "field".to_string(),
                    detail: "key: K".to_string(),
                },
                AnalysisCompletion {
                    name: "value".to_string(),
                    kind: "field".to_string(),
                    detail: "value: V".to_string(),
                },
            ]);
        }
        "TaskGroup" => {
            completions.push(AnalysisCompletion {
                name: "spawn".to_string(),
                kind: "method".to_string(),
                detail: "spawn(function, ...) -> Task[T]".to_string(),
            });
        }
        _ => {}
    }

    for builtin in [
        BuiltinMember::FloatSqrt,
        BuiltinMember::StringLen,
        BuiltinMember::StringContains,
        BuiltinMember::StringStartsWith,
        BuiltinMember::StringEndsWith,
        BuiltinMember::StringSplit,
        BuiltinMember::StringReplace,
        BuiltinMember::StringToLower,
        BuiltinMember::StringToUpper,
        BuiltinMember::StringJoin,
        BuiltinMember::StringStripPrefix,
        BuiltinMember::StringStripSuffix,
        BuiltinMember::StringTrim,
        BuiltinMember::StringClone,
        BuiltinMember::ScalarToString,
        BuiltinMember::VecInsert,
        BuiltinMember::VecClear,
        BuiltinMember::VecReverse,
        BuiltinMember::MapLen,
        BuiltinMember::MapIsEmpty,
        BuiltinMember::MapClone,
        BuiltinMember::MapGet,
        BuiltinMember::MapSet,
        BuiltinMember::MapRemove,
        BuiltinMember::MapContainsKey,
        BuiltinMember::MapKeys,
        BuiltinMember::MapValues,
        BuiltinMember::MapItems,
        BuiltinMember::MapEntries,
        BuiltinMember::MapClear,
        BuiltinMember::MapExtend,
        BuiltinMember::SetLen,
        BuiltinMember::SetIsEmpty,
        BuiltinMember::SetClone,
        BuiltinMember::SetContains,
        BuiltinMember::SetInsert,
        BuiltinMember::SetRemove,
        BuiltinMember::ChannelClone,
        BuiltinMember::ChannelSend,
        BuiltinMember::ChannelRecv,
        BuiltinMember::ChannelClose,
        BuiltinMember::TaskClone,
        BuiltinMember::TaskJoin,
        BuiltinMember::TaskGroupCancel,
    ] {
        if BuiltinMember::resolve(base_type_name(receiver_type), builtin.name()) == Some(builtin) {
            completions.push(AnalysisCompletion {
                name: builtin.name().to_string(),
                kind: "method".to_string(),
                detail: builtin.detail().to_string(),
            });
        }
    }

    completions
}

fn builtin_function_return_type(name: &str) -> Option<Type> {
    match BuiltinFunction::from_name(name)? {
        BuiltinFunction::Print => Some(Type::Unit),
        BuiltinFunction::Range => Some(Type::named("Range")),
        BuiltinFunction::TaskGroup => Some(Type::named("TaskGroup")),
        BuiltinFunction::Cancelled => Some(Type::named("bool")),
        BuiltinFunction::After => Some(Type::named("Duration")),
        BuiltinFunction::Sleep => Some(Type::Unit),
        BuiltinFunction::Abs => None,
        BuiltinFunction::Min => None,
        BuiltinFunction::Max => None,
        BuiltinFunction::Sqrt => None,
        BuiltinFunction::Channel => None,
        BuiltinFunction::ParseInt32 => Some(Type::Named(
            "Result".to_string(),
            vec![Type::named("int32"), Type::named("String")],
        )),
        BuiltinFunction::ParseInt64 => Some(Type::Named(
            "Result".to_string(),
            vec![Type::named("int64"), Type::named("String")],
        )),
        BuiltinFunction::ParseFloat64 => Some(Type::Named(
            "Result".to_string(),
            vec![Type::named("float64"), Type::named("String")],
        )),
    }
}

fn format_function_detail(function_decl: &FunctionDecl) -> String {
    let params = function_decl
        .params
        .iter()
        .map(|param| lower_type_ref(&param.ty).to_string())
        .collect::<Vec<_>>()
        .join(", ");
    format!(
        "{}({}) -> {}",
        function_decl.name,
        params,
        lower_type_ref(&function_decl.return_type)
    )
}

fn callable_contains_line(stmts: &[Stmt], line: usize) -> bool {
    block_contains_line(stmts, line)
}

fn block_contains_line(stmts: &[Stmt], line: usize) -> bool {
    if stmts.is_empty() {
        return false;
    }
    let start = stmt_start_line(&stmts[0]);
    let end = stmts.iter().map(stmt_end_line).max().unwrap_or(start);
    start <= line && line <= end
}

fn stmt_start_line(stmt: &Stmt) -> usize {
    match stmt {
        Stmt::Assign(assign) => assign.span.line,
        Stmt::Return(ret) => ret.span.line,
        Stmt::If(if_stmt) => if_stmt.span.line,
        Stmt::Match(match_stmt) => match_stmt.span.line,
        Stmt::For(for_stmt) => for_stmt.span.line,
        Stmt::With(with_stmt) => with_stmt.span.line,
        Stmt::Select(select_stmt) => select_stmt.span.line,
        Stmt::While(while_stmt) => while_stmt.span.line,
        Stmt::Break(stmt) => stmt.span.line,
        Stmt::Continue(stmt) => stmt.span.line,
        Stmt::Pass(stmt) => stmt.span.line,
        Stmt::Expr(stmt) => stmt.span.line,
    }
}

fn stmt_end_line(stmt: &Stmt) -> usize {
    match stmt {
        Stmt::Assign(assign) => assign.span.line,
        Stmt::Return(ret) => ret.span.line,
        Stmt::If(if_stmt) => {
            let mut end = if_stmt.span.line;
            for branch in &if_stmt.branches {
                end = end.max(
                    branch
                        .body
                        .iter()
                        .map(stmt_end_line)
                        .max()
                        .unwrap_or(branch.span.line),
                );
            }
            if let Some(body) = &if_stmt.else_body {
                end = end.max(
                    body.iter()
                        .map(stmt_end_line)
                        .max()
                        .unwrap_or(if_stmt.span.line),
                );
            }
            end
        }
        Stmt::Match(match_stmt) => match_stmt
            .arms
            .iter()
            .map(|arm| {
                arm.body
                    .iter()
                    .map(stmt_end_line)
                    .max()
                    .unwrap_or(arm.span.line)
            })
            .max()
            .unwrap_or(match_stmt.span.line),
        Stmt::For(for_stmt) => for_stmt
            .body
            .iter()
            .map(stmt_end_line)
            .max()
            .unwrap_or(for_stmt.span.line),
        Stmt::With(with_stmt) => with_stmt
            .body
            .iter()
            .map(stmt_end_line)
            .max()
            .unwrap_or(with_stmt.span.line),
        Stmt::Select(select_stmt) => select_stmt
            .arms
            .iter()
            .map(|arm| {
                arm.body
                    .iter()
                    .map(stmt_end_line)
                    .max()
                    .unwrap_or(arm.span.line)
            })
            .max()
            .unwrap_or(select_stmt.span.line),
        Stmt::While(while_stmt) => while_stmt
            .body
            .iter()
            .map(stmt_end_line)
            .max()
            .unwrap_or(while_stmt.span.line),
        Stmt::Break(stmt) => stmt.span.line,
        Stmt::Continue(stmt) => stmt.span.line,
        Stmt::Pass(stmt) => stmt.span.line,
        Stmt::Expr(stmt) => stmt.span.line,
    }
}

fn extract_receiver_before_dot(line_text: &str, character: usize) -> Option<String> {
    extract_receiver_ending_before(line_text, character).map(|value| value.trim().to_string())
}

fn recover_checked_program_after_parse_error_with<F>(
    source: &str,
    error: &Diagnostic,
    check_program: &mut F,
) -> Option<Program>
where
    F: FnMut(&str) -> Result<Program>,
{
    if !error.message.starts_with("expected member name") {
        return None;
    }
    let span = error.span?;
    recover_checked_program_after_position(
        source,
        span.line.saturating_sub(1),
        span.column.saturating_sub(1),
        check_program,
    )
}

fn recover_checked_program_after_position<F>(
    source: &str,
    line: usize,
    character: usize,
    check_program: &mut F,
) -> Option<Program>
where
    F: FnMut(&str) -> Result<Program>,
{
    let sanitized = sanitize_member_completion_source(source, line, character);
    if let Some(program) = recover_checked_program_after_member_errors(&sanitized, check_program) {
        return Some(program);
    }

    let fallback = replace_dangling_member_stmt_with_recovery_stmt(source, line);
    recover_checked_program_after_member_errors(&fallback, check_program)
}

fn recover_checked_program_after_member_errors<F>(
    source: &str,
    check_program: &mut F,
) -> Option<Program>
where
    F: FnMut(&str) -> Result<Program>,
{
    let mut candidate = source.to_string();
    for _ in 0..8 {
        match parser::parse(&candidate) {
            Ok(_) => return check_program(&candidate).ok(),
            Err(error) if error.message.starts_with("expected member name") => {
                let Some(line) = error.span.map(|span| span.line.saturating_sub(1)) else {
                    return None;
                };
                let next = replace_dangling_member_stmt_with_recovery_stmt(&candidate, line);
                if next == candidate {
                    return None;
                }
                candidate = next;
            }
            Err(_) => return None,
        }
    }
    None
}

fn sanitize_member_completion_source(source: &str, line: usize, character: usize) -> String {
    let mut lines = source.lines().map(str::to_string).collect::<Vec<_>>();
    let Some(line_text) = lines.get_mut(line) else {
        return source.to_string();
    };
    let byte_index = line_text
        .char_indices()
        .map(|(index, _)| index)
        .chain(std::iter::once(line_text.len()))
        .nth(character)
        .unwrap_or(line_text.len());
    if byte_index == 0 || byte_index > line_text.len() {
        return source.to_string();
    }

    let dot_index = line_text[..byte_index]
        .char_indices()
        .rev()
        .find_map(|(index, ch)| (ch == '.').then_some(index));
    let Some(dot_index) = dot_index else {
        return source.to_string();
    };

    line_text.remove(dot_index);
    lines.join("\n")
}

fn replace_dangling_member_stmt_with_recovery_stmt(source: &str, line: usize) -> String {
    let mut lines = source.lines().map(str::to_string).collect::<Vec<_>>();
    let Some(line_text) = lines.get_mut(line) else {
        return source.to_string();
    };
    let indent = line_text
        .chars()
        .take_while(|ch| ch.is_whitespace())
        .collect::<String>();
    let replacement = enclosing_function_return_placeholder(source, line)
        .map(|value| format!("{}{}", indent, value))
        .unwrap_or_else(|| format!("{}pass", indent));
    *line_text = replacement;
    lines.join("\n")
}

fn enclosing_function_return_placeholder(source: &str, line: usize) -> Option<String> {
    let lines = source.lines().collect::<Vec<_>>();
    let target_indent = lines
        .get(line)?
        .chars()
        .take_while(|ch| ch.is_whitespace())
        .count();

    for candidate in (0..line).rev() {
        let text = lines[candidate];
        let indent = text.chars().take_while(|ch| ch.is_whitespace()).count();
        if indent >= target_indent {
            continue;
        }
        let trimmed = text.trim_start();
        if !trimmed.starts_with("def ") && !trimmed.starts_with("public def ") {
            continue;
        }
        let return_type = trimmed
            .split_once("->")
            .and_then(|(_, rest)| rest.split_once(':').map(|(ty, _)| ty.trim()))
            .unwrap_or("None");
        return placeholder_stmt_for_return_type(return_type);
    }

    None
}

fn placeholder_stmt_for_return_type(return_type: &str) -> Option<String> {
    match return_type {
        "None" => Some("return".to_string()),
        "bool" => Some("return false".to_string()),
        "float32" | "float64" => Some("return 0.0".to_string()),
        "String" | "str" => Some("return \"\"".to_string()),
        "Duration" => Some("return 0ms".to_string()),
        ty if matches!(
            ty,
            "int8"
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
        ) =>
        {
            Some("return 0".to_string())
        }
        ty if ty.starts_with("Option[") => Some("return Option.None".to_string()),
        _ => None,
    }
}

fn extract_receiver_ending_before(line_text: &str, end_index_exclusive: usize) -> Option<&str> {
    if line_text.is_empty() {
        return None;
    }

    let mut index = end_index_exclusive.min(line_text.len()).saturating_sub(1);
    let bytes = line_text.as_bytes();
    while index > 0
        && bytes
            .get(index)
            .copied()
            .unwrap_or_default()
            .is_ascii_whitespace()
    {
        index -= 1;
    }
    if bytes.get(index).copied() != Some(b'.') {
        return None;
    }

    if index == 0 {
        return None;
    }
    index -= 1;
    while index > 0
        && bytes
            .get(index)
            .copied()
            .unwrap_or_default()
            .is_ascii_whitespace()
    {
        index -= 1;
    }

    let end = index + 1;
    let start = find_receiver_start(line_text, index)?;
    Some(&line_text[start..end])
}

fn find_receiver_start(line_text: &str, index: usize) -> Option<usize> {
    let bytes = line_text.as_bytes();
    if bytes.get(index).copied() == Some(b')') {
        let mut depth = 1isize;
        let mut cursor = index as isize - 1;
        while cursor >= 0 {
            match bytes[cursor as usize] {
                b')' => depth += 1,
                b'(' => {
                    depth -= 1;
                    if depth == 0 {
                        return Some(cursor as usize);
                    }
                }
                _ => {}
            }
            cursor -= 1;
        }
        return None;
    }

    if is_identifier_char(bytes.get(index).copied()? as char) {
        let mut cursor = index as isize;
        while cursor >= 0 {
            let ch = bytes[cursor as usize] as char;
            if is_identifier_char(ch) || ch == '.' {
                cursor -= 1;
                continue;
            }
            break;
        }
        return Some((cursor + 1) as usize);
    }

    None
}

fn is_identifier_char(ch: char) -> bool {
    ch.is_ascii_alphanumeric() || ch == '_'
}

#[cfg(test)]
#[path = "analysis_tests.rs"]
mod tests;
