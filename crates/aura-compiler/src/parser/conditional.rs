//! Parse declaration suites in their surrounding grammar. Selection runs
//! after syntax parsing and before a module's imports leave the parser.
use super::*;
use std::collections::BTreeSet;
use std::ops::Range;

mod predicates;
mod resolver;
mod statements;
pub(super) use resolver::Resolver;

/// Root discovery can use already selected imports. If a guard itself needs a
/// user import, its unconditional imports identify the root before selection.
pub(crate) fn root_imports(source: &str) -> Result<Vec<Vec<String>>> {
    let imports = match super::parse(source) {
        Ok(module) => module.imports,
        Err(_) => super::parse_all_syntax(source)?
            .imports
            .into_iter()
            .filter(|import| import.span.column == 1)
            .collect(),
    };
    Ok(imports
        .into_iter()
        .map(|import| match import.kind {
            ImportKind::From { module_path, .. } => module_path,
            ImportKind::Module { path, .. } => path,
        })
        .filter(|path| {
            path.first()
                .is_some_and(|root| !crate::builtin_modules::is_builtin_root_module(root))
        })
        .collect())
}

pub(super) struct Suite {
    header: Span,
    required: bool,
    arms: Vec<Arm>,
}

struct Arm {
    condition: Option<Expr>,
    lines: Range<usize>,
    header: Range<usize>,
    dedent: usize,
}

impl Parser {
    pub(super) fn declaration_kind(
        &self,
        imports: &[ImportDecl],
        constants: &[ConstantDecl],
        seen_statement: bool,
    ) -> Option<bool> {
        let header = self.tokens[self.index + 1..]
            .iter()
            .take_while(|token| token.kind != TokenKind::Newline);
        let marker = header
            .filter_map(|token| match &token.kind {
                TokenKind::Identifier(name) => Some(name.as_str()),
                _ => None,
            })
            .any(|name| {
                constants
                    .iter()
                    .any(|binding| binding.required && binding.name == name)
                    || imports
                        .iter()
                        .any(|import| predicates::imports_name(import, name))
            });

        // A declaration is valid only in a selected suite. A guard on an
        // executable script can still have a runtime operand.
        let mut cursor = self.index;
        loop {
            let indent = self.tokens[cursor..]
                .iter()
                .position(|token| token.kind == TokenKind::Indent)?;
            cursor += indent + 1;
            let mut depth = 1;
            while let Some(token) = self.tokens.get(cursor) {
                cursor += 1;
                match &token.kind {
                    TokenKind::Indent => depth += 1,
                    TokenKind::Dedent => {
                        depth -= 1;
                        if depth == 0 {
                            break;
                        }
                    }
                    TokenKind::KwImport
                    | TokenKind::KwExtern
                    | TokenKind::KwClass
                    | TokenKind::KwEnum
                    | TokenKind::KwTrait
                    | TokenKind::KwImpl
                    | TokenKind::KwPublic => return Some(true),
                    TokenKind::KwDef if depth == 1 && !seen_statement => return Some(true),
                    TokenKind::Identifier(name)
                        if name == "const"
                            && matches!(
                                self.peek_kind_at(cursor),
                                Some(TokenKind::Identifier(_))
                            ) =>
                    {
                        return Some(true)
                    }
                    _ => {}
                }
            }
            if !matches!(
                self.peek_kind_at(cursor),
                Some(TokenKind::KwElse | TokenKind::KwElif)
            ) {
                return (marker && !seen_statement).then_some(false);
            }
        }
    }

    pub(super) fn parse_declaration_arms<T>(
        &mut self,
        required: bool,
        mut parse_body: impl FnMut(&mut Self) -> Result<T>,
    ) -> Result<Vec<T>> {
        self.enter_recursion("conditional declaration")?;
        let header = self.current_span();
        let mut arms = Vec::new();
        let mut bodies = Vec::new();
        loop {
            let first = self.index;
            let keyword = self.bump();
            let condition = if keyword.kind == TokenKind::KwElse {
                None
            } else {
                Some(self.parse_expr()?)
            };
            self.expect_simple(TokenKind::Colon)?;
            let line = self.expect_newline()?.span.line;
            self.expect_simple(TokenKind::Indent)?;
            let header = first..self.index;
            bodies.push(parse_body(self)?);
            let end = self.expect_simple(TokenKind::Dedent)?.span.line - 1;
            arms.push(Arm {
                condition,
                lines: line..end,
                header,
                dedent: self.index - 1,
            });
            if keyword.kind == TokenKind::KwElse
                || !matches!(self.current_kind(), TokenKind::KwElif | TokenKind::KwElse)
            {
                break;
            }
        }
        self.exit_recursion();
        self.declaration_ifs.push(Suite {
            header,
            required,
            arms,
        });
        Ok(bodies)
    }

    pub(super) fn select_conditionals(
        &mut self,
        module: &mut Module,
        resolver: &mut Resolver,
    ) -> Result<()> {
        if self.declaration_ifs.is_empty()
            && !module.constants.iter().any(|binding| binding.required)
            && module.imports.is_empty()
        {
            return Ok(());
        }
        if !self
            .tokens
            .iter()
            .any(|token| token.kind == TokenKind::KwIf)
        {
            return Ok(());
        }
        let mut declarations = std::mem::take(&mut self.declaration_ifs);
        declarations.sort_by_key(|suite| suite.header.line);
        let mut inactive = Vec::new();
        let saw_groups = !declarations.is_empty();
        let mut omissions = Vec::new();
        for suite in declarations {
            if !is_active(suite.header, &inactive) {
                continue;
            }
            let mut choice = None;
            let mut runtime = false;
            for (index, arm) in suite.arms.iter().enumerate() {
                if let Some(guard) = &arm.condition {
                    match predicates::evaluate(
                        resolver,
                        module,
                        &self.tokens,
                        guard,
                        &BTreeSet::new(),
                        &inactive,
                        true,
                    )? {
                        Some(false) => continue,
                        Some(true) => {}
                        None if !suite.required => {
                            runtime = true;
                            break;
                        }
                        None => return Err(Diagnostic::coded_at(
                            "AU2071",
                            guard.span,
                            "a declaration condition must use build constants or const bindings",
                        )),
                    }
                }
                choice = Some(index);
                break;
            }
            if runtime {
                continue;
            }
            for (index, arm) in suite.arms.into_iter().enumerate() {
                if Some(index) == choice {
                    omissions.push(arm.header);
                    omissions.push(arm.dedent..arm.dedent + 1);
                } else {
                    inactive.push(arm.lines);
                    omissions.push(arm.header.start..arm.dedent + 1);
                }
            }
        }
        // Parse the active token stream in its original declaration order.
        // In particular, only a retained script statement turns subsequent
        // bare bindings into locals. All token spans remain source spans.
        if !omissions.is_empty() || saw_groups {
            omissions.sort_by_key(|range| range.start);
            let mut tokens = Vec::with_capacity(self.tokens.len());
            let mut kept = 0;
            for range in omissions {
                if kept < range.start {
                    tokens.extend_from_slice(&self.tokens[kept..range.start]);
                }
                kept = kept.max(range.end);
            }
            tokens.extend_from_slice(&self.tokens[kept..]);
            let mut parser = Parser::new(tokens);
            parser.projected = true;
            parser.source_lines.clone_from(&self.source_lines);
            *module = parser.parse_module()?;
        }
        statements::select(resolver, module, &self.tokens, &mut inactive)?;
        inactive.sort_by_key(|range| range.start);
        module.inactive_lines = inactive;
        Ok(())
    }
}

fn is_active(span: Span, ranges: &[Range<usize>]) -> bool {
    !ranges
        .iter()
        .any(|lines| lines.contains(&span.line.saturating_sub(1)))
}

fn item_span(item: &Item) -> Span {
    match item {
        Item::Function(f) => f.span,
        Item::Class(c) => c.span,
        Item::Enum(e) => e.span,
        Item::Trait(t) => t.span,
        Item::Impl(i) => i.span,
        Item::TypeAlias(a) => a.span,
        Item::ExternFunction(f) => f.span,
        Item::ExternOpaqueClass(c) => c.span,
    }
}
