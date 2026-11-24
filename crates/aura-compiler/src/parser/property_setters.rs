use super::{Diagnostic, FunctionDecl, Parser, Result, TokenKind};
use crate::ast::PROPERTY_SETTER_SUFFIX;

impl Parser {
    pub(super) fn at_property_setter(&self, methods: &[FunctionDecl]) -> bool {
        if !self.at_simple(&TokenKind::At) {
            return false;
        }
        let Some(TokenKind::Identifier(name)) = self.peek_kind(1) else {
            return false;
        };
        if !matches!(self.peek_kind(2), Some(TokenKind::Dot))
            || !matches!(self.peek_kind(3), Some(TokenKind::Identifier(member)) if member == "setter")
            || !matches!(self.peek_kind(4), Some(TokenKind::Newline))
        {
            return false;
        }
        if methods
            .iter()
            .any(|method| method.property.is_some() && method.name == *name)
        {
            return true;
        }
        // A module may export an ordinary decorator called setter.
        // Before seeing the getter, only recognize a marker on its namesake.
        let mut offset = 5;
        while matches!(self.peek_kind(offset), Some(TokenKind::Newline)) {
            offset += 1;
        }
        if matches!(self.peek_kind(offset), Some(TokenKind::KwPublic)) {
            offset += 1;
        }
        matches!(self.peek_kind(offset), Some(TokenKind::KwDef))
            && matches!(self.peek_kind(offset + 1), Some(TokenKind::Identifier(method)) if method == name)
    }

    pub(super) fn parse_property_setter(
        &mut self,
        methods: &[FunctionDecl],
    ) -> Result<FunctionDecl> {
        let span = self.bump().span;
        let name = self.expect_identifier()?;
        self.bump(); // dot
        self.bump(); // setter
        self.expect_newline()?;
        self.skip_newlines();
        let public = self.eat_simple(&TokenKind::KwPublic).is_some();
        if !self.at_keyword_def() {
            return Err(Diagnostic::coded_at(
                "AU2064",
                span,
                "a setter must precede a method",
            ));
        }
        let mut method = self.parse_function_with_receiver(true, public)?;
        if method.name != name {
            return Err(Diagnostic::coded_at(
                "AU2064",
                span,
                format!("setter for `{name}` must decorate a method named `{name}`"),
            ));
        }
        method.name.push_str(PROPERTY_SETTER_SUFFIX);
        if methods.iter().any(|other| other.name == method.name) {
            return Err(Diagnostic::coded_at(
                "AU2064",
                span,
                format!("duplicate setter for `{name}`"),
            ));
        }
        Ok(method)
    }
}
