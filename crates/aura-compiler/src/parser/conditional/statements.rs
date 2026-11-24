use super::*;

pub(super) fn select(
    resolver: &mut Resolver,
    module: &mut Module,
    tokens: &[Token],
    inactive: &mut Vec<Range<usize>>,
) -> Result<()> {
    if !has_guard_name(module, resolver, tokens) {
        return Ok(());
    }
    let declarations = module.clone();
    let mut selector = Statements {
        resolver,
        module: &declarations,
        tokens,
        inactive,
    };
    for item in &mut module.items {
        let methods = match item {
            Item::Function(function) => std::slice::from_mut(function),
            Item::Class(class) => &mut class.methods,
            Item::Trait(trait_decl) => &mut trait_decl.methods,
            Item::Impl(implementation) => &mut implementation.methods,
            _ => continue,
        };
        for method in methods {
            let scope = method.params.iter().map(|p| p.name.clone()).collect();
            selector.block(&mut method.body, scope)?;
        }
    }
    selector.block(&mut module.top_level_stmts, BTreeSet::new())
}

struct Statements<'a, 'loader> {
    resolver: &'a mut Resolver<'loader>,
    module: &'a Module,
    tokens: &'a [Token],
    inactive: &'a mut Vec<Range<usize>>,
}

impl Statements<'_, '_> {
    fn block(&mut self, statements: &mut [Stmt], mut locals: BTreeSet<String>) -> Result<()> {
        for statement in statements.iter_mut() {
            match statement {
                Stmt::If(stmt) => self.conditional(stmt, &locals)?,
                Stmt::While(stmt) => self.block(&mut stmt.body, locals.clone())?,
                Stmt::For(stmt) => {
                    let mut scope = locals.clone();
                    bind_target(&stmt.target, &mut scope);
                    self.block(&mut stmt.body, scope)?;
                }
                Stmt::With(stmt) => {
                    let mut scope = locals.clone();
                    scope.insert(stmt.binding.clone());
                    self.block(&mut stmt.body, scope)?;
                }
                Stmt::Match(stmt) => {
                    for arm in &mut stmt.arms {
                        let mut scope = locals.clone();
                        bind_pattern(&arm.pattern, &mut scope);
                        self.block(&mut arm.body, scope)?;
                    }
                }
                Stmt::Assign(stmt) => {
                    if let ExprKind::Lambda {
                        params,
                        body: crate::ast::ClosureBody::Function { statements, .. },
                        ..
                    } = &mut stmt.value.kind
                    {
                        let mut scope = locals.clone();
                        scope.extend(params.iter().map(|param| param.name.clone()));
                        self.block(statements, scope)?;
                    }
                    if let AssignTarget::Name(name) = &stmt.target {
                        locals.insert(name.clone());
                    }
                }
                Stmt::View(stmt) => {
                    locals.insert(stmt.name.clone());
                }
                Stmt::Destructure(stmt) => bind_target(&stmt.target, &mut locals),
                _ => {}
            }
        }
        Ok(())
    }

    fn conditional(&mut self, stmt: &mut IfStmt, locals: &BTreeSet<String>) -> Result<()> {
        let mut selected = false;
        let mut branches = Vec::new();
        let mut fallback = None;
        let mut last_header = stmt.span;
        for mut arm in std::mem::take(&mut stmt.branches) {
            last_header = arm.span;
            let condition = if selected {
                Some(false)
            } else {
                predicates::evaluate(
                    self.resolver,
                    self.module,
                    self.tokens,
                    &arm.condition,
                    locals,
                    &[],
                    false,
                )?
            };
            match condition {
                Some(false) => {
                    self.inactive.push(self.suite_lines(arm.span).0);
                }
                Some(true) => {
                    selected = true;
                    self.block(&mut arm.body, locals.clone())?;
                    fallback = Some(arm.body);
                }
                None => {
                    self.block(&mut arm.body, locals.clone())?;
                    branches.push(arm);
                }
            }
        }
        if let Some(mut body) = stmt.else_body.take() {
            if selected {
                let (_, following) = self.suite_lines(last_header);
                if let Some(token) = self.tokens.get(following) {
                    self.inactive.push(self.suite_lines(token.span).0);
                }
            } else {
                self.block(&mut body, locals.clone())?;
                fallback = Some(body);
            }
        }
        stmt.branches = branches;
        stmt.else_body = fallback;
        Ok(())
    }

    /// Look up a parsed suite by its header. The lexer's layout tokens keep
    /// blank lines, multiline conditions and nested statements unambiguous.
    fn suite_lines(&self, header: Span) -> (Range<usize>, usize) {
        let mut index = self
            .tokens
            .partition_point(|token| token.span.line < header.line);
        while self.tokens[index].kind != TokenKind::Indent {
            index += 1;
        }
        // A preceding header can end at the first token on this same line.
        if self.tokens[index].span.line == header.line {
            index += 1;
            while self.tokens[index].kind != TokenKind::Indent {
                index += 1;
            }
        }
        let start = self.tokens[index - 1].span.line;
        let mut depth = 1;
        index += 1;
        loop {
            match self.tokens[index].kind {
                TokenKind::Indent => depth += 1,
                TokenKind::Dedent => {
                    depth -= 1;
                    if depth == 0 {
                        break;
                    }
                }
                _ => {}
            }
            index += 1;
        }
        (start..self.tokens[index].span.line - 1, index + 1)
    }
}

fn bind_target(target: &BindingTarget, locals: &mut BTreeSet<String>) {
    match target {
        BindingTarget::Name { name, .. } | BindingTarget::Rest { name, .. } => {
            locals.insert(name.clone());
        }
        BindingTarget::Tuple { elements, .. } => elements
            .iter()
            .for_each(|element| bind_target(element, locals)),
    }
}

fn bind_pattern(pattern: &Pattern, locals: &mut BTreeSet<String>) {
    match pattern {
        Pattern::Binding(p) => {
            locals.insert(p.name.clone());
        }
        Pattern::Type(p) => {
            locals.insert(p.binding.name.clone());
        }
        Pattern::Rest(p) => locals.extend(p.name.iter().cloned()),
        Pattern::Or(p) => p.alternatives.iter().for_each(|p| bind_pattern(p, locals)),
        Pattern::Tuple(p) => p.elements.iter().for_each(|p| bind_pattern(p, locals)),
        Pattern::Sequence(p) => p.elements.iter().for_each(|p| bind_pattern(p, locals)),
        Pattern::Variant(p) => p.subpatterns.iter().for_each(|p| bind_pattern(p, locals)),
        Pattern::Class(p) => p
            .fields
            .iter()
            .for_each(|field| bind_pattern(&field.pattern, locals)),
        Pattern::Literal(_) | Pattern::Wildcard(_) => {}
    }
}

/// An ordinary function guard cannot be selected unless it references a marked
/// binding or a name imported from a possible provider of marked bindings.
/// Check that necessary condition before cloning a module and walking its bodies.
fn has_guard_name(module: &Module, resolver: &Resolver, tokens: &[Token]) -> bool {
    let mut names: BTreeSet<&String> = module
        .constants
        .iter()
        .filter(|constant| constant.required)
        .map(|constant| &constant.name)
        .collect();
    for import in module
        .imports
        .iter()
        .filter(|import| resolver.potential(import))
    {
        match &import.kind {
            ImportKind::Module { path, alias } => names.extend(alias.as_ref().or(path.first())),
            ImportKind::From { names: symbols, .. } => {
                names.extend(
                    symbols
                        .iter()
                        .map(|symbol| symbol.alias.as_ref().unwrap_or(&symbol.name)),
                );
            }
        }
    }
    if names.is_empty() {
        return false;
    }
    // Newline is a logical line break. A continued parenthesized condition
    // has no Newline token until after its closing colon.
    let mut in_guard = false;
    for token in tokens {
        match &token.kind {
            TokenKind::KwIf | TokenKind::KwElif => in_guard = true,
            TokenKind::Newline => in_guard = false,
            TokenKind::Identifier(name) if in_guard && names.contains(name) => return true,
            _ => {}
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn search_build_names_only_inside_statement_guards() {
        fn candidate(source: &str) -> bool {
            let module = crate::parser::parse_all_syntax(source).unwrap();
            has_guard_name(&module, &Resolver::new(None), &lex(source).unwrap())
        }

        let ordinary = concat!(
            "from build import hosted as host\n",
            "def f(value: bool) -> bool:\n",
            "    if value:\n",
            "        return host\n",
            "    return false\n",
        );
        assert!(!candidate(ordinary));
        assert!(candidate(&ordinary.replace("if value", "if host")));
        assert!(candidate(&ordinary.replace(
            "    return false",
            "    elif (\n        host\n    ):\n        return true\n    return false"
        )));
        assert!(candidate("const ready = true\ndef f() -> bool:\n    if ready:\n        return true\n    return false\n"));
        assert!(!candidate(&ordinary.replace("if value", "if true")));
    }
}
