use super::*;

impl Parser {
    pub(super) fn has_explicit_entry(&self) -> bool {
        let mut depth = 0;
        for (index, token) in self.tokens.iter().enumerate() {
            match &token.kind {
                TokenKind::Indent => depth += 1,
                TokenKind::Dedent => depth -= 1,
                TokenKind::KwDef
                    if depth == 0
                        && matches!(self.tokens.get(index + 1).map(|t| &t.kind), Some(TokenKind::Identifier(name)) if name == "main") =>
                {
                    return true
                }
                TokenKind::Identifier(name)
                    if depth == 0
                        && name == "export"
                        && matches!(
                            self.tokens.get(index + 1).map(|t| &t.kind),
                            Some(TokenKind::StringLiteral(_))
                        ) =>
                {
                    return true
                }
                _ => {}
            }
        }
        false
    }
}
