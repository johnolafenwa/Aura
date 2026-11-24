//! Parse declaration suites in their surrounding grammar. Selection runs
//! after syntax parsing and before a module's imports leave the parser.
use super::*;
use std::collections::{BTreeMap, BTreeSet};
use std::ops::Range;

mod predicates;
mod statements;

pub(super) struct Suite {
    header: Span,
    arms: Vec<(Option<Expr>, Range<usize>)>,
}

impl Parser {
    pub(super) fn is_declaration_if(
        &self,
        imports: &[ImportDecl],
        constants: &[ConstantDecl],
    ) -> bool {
        let header = self.tokens[self.index + 1..]
            .iter()
            .take_while(|token| token.kind != TokenKind::Newline);
        header
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
            })
            || self.has_declaration_in_suite()
    }

    fn has_declaration_in_suite(&self) -> bool {
        let Some(start) = self.tokens[self.index..]
            .iter()
            .position(|t| t.kind == TokenKind::Indent)
        else {
            return false;
        };
        let mut depth = 1;
        for token in &self.tokens[self.index + start + 1..] {
            match &token.kind {
                TokenKind::Dedent => {
                    depth -= 1;
                    if depth == 0 {
                        break;
                    }
                }
                TokenKind::Indent => depth += 1,
                TokenKind::KwImport
                | TokenKind::KwFrom
                | TokenKind::KwExtern
                | TokenKind::KwClass
                | TokenKind::KwEnum
                | TokenKind::KwImpl
                | TokenKind::KwTrait
                | TokenKind::KwPublic
                    if depth == 1 =>
                {
                    return true
                }
                _ => {}
            }
        }
        false
    }

    pub(super) fn parse_declaration_arms<T>(
        &mut self,
        mut parse_body: impl FnMut(&mut Self) -> Result<T>,
    ) -> Result<Vec<T>> {
        self.enter_recursion("conditional declaration")?;
        let header = self.current_span();
        let mut arms = Vec::new();
        let mut bodies = Vec::new();
        loop {
            let keyword = self.bump();
            let condition = if keyword.kind == TokenKind::KwElse {
                None
            } else {
                Some(self.parse_expr()?)
            };
            self.expect_simple(TokenKind::Colon)?;
            let line = self.expect_newline()?.span.line;
            self.expect_simple(TokenKind::Indent)?;
            bodies.push(parse_body(self)?);
            let end = self.expect_simple(TokenKind::Dedent)?.span.line - 1;
            arms.push((condition, line..end));
            if keyword.kind == TokenKind::KwElse
                || !matches!(self.current_kind(), TokenKind::KwElif | TokenKind::KwElse)
            {
                break;
            }
        }
        self.exit_recursion();
        self.declaration_ifs.push(Suite { header, arms });
        Ok(bodies)
    }

    pub(super) fn select_conditionals(&mut self, module: &mut Module) -> Result<()> {
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
        for suite in declarations {
            if !is_active(suite.header, &inactive) {
                continue;
            }
            let mut taken = false;
            for (condition, lines) in suite.arms {
                let selected =
                    if taken {
                        false
                    } else if let Some(condition) = condition {
                        predicates::evaluate(
                            module,
                            &self.tokens,
                            &condition,
                            &BTreeSet::new(),
                            &inactive,
                            true,
                        )?
                        .ok_or_else(|| {
                            Diagnostic::coded_at("AU2071", condition.span,
                            "a declaration condition must use build constants or const bindings")
                        })?
                    } else {
                        true
                    };
                if selected {
                    taken = true
                } else {
                    inactive.push(lines);
                }
            }
        }
        module
            .imports
            .retain(|decl| is_active(decl.span, &inactive));
        module
            .constants
            .retain(|decl| is_active(decl.span, &inactive));
        module
            .items
            .retain(|item| is_active(item_span(item), &inactive));
        module
            .top_level_stmts
            .retain(|stmt| is_active(crate::sema::statement_span(stmt), &inactive));
        for item in &mut module.items {
            if let Item::Class(class) = item {
                class.fields.retain(|decl| is_active(decl.span, &inactive));
                class.methods.retain(|decl| is_active(decl.span, &inactive));
            }
        }
        statements::select(module, &self.tokens, &mut inactive)?;
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
