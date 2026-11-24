use super::*;

impl Parser {
    pub(super) fn at_layout_modifier(&self) -> bool {
        matches!(self.current_kind(), TokenKind::Identifier(word) if word == "packed" || word == "align")
    }

    fn layout_argument(&mut self) -> Result<usize> {
        let span = self.bump().span;
        self.expect_simple(TokenKind::LParen)?;
        let TokenKind::IntLiteral(n) = self.bump().kind else {
            return Err(Diagnostic::coded_at(
                "AU2085",
                span,
                "expected a positive integer literal in the C layout declaration",
            ));
        };
        self.expect_simple(TokenKind::RParen)?;
        usize::try_from(n)
            .map_err(|_| Diagnostic::coded_at("AU2085", span, "C layout argument is too large"))
    }

    pub(super) fn parse_controlled_c_class(&mut self, public: bool) -> Result<ClassDecl> {
        let mut packed = false;
        let mut alignment = None;
        while self.at_layout_modifier() {
            let span = self.current_span();
            if matches!(self.current_kind(), TokenKind::Identifier(word) if word == "packed") {
                self.bump();
                if std::mem::replace(&mut packed, true) {
                    return Err(Diagnostic::coded_at(
                        "AU2085",
                        span,
                        "duplicate packed modifier",
                    ));
                }
            } else {
                let value = self.layout_argument()?;
                if !value.is_power_of_two() || value > 8192 || alignment.replace(value).is_some() {
                    return Err(Diagnostic::coded_at(
                        "AU2085",
                        span,
                        "supply one power-of-two C alignment from 1 through 8192",
                    ));
                }
            }
        }
        let mut class = self.parse_extern_class(public)?;
        class.layout_controls.packed = packed;
        class.layout_controls.alignment = alignment;
        Ok(class)
    }

    pub(super) fn parse_field_width(&mut self, field: &str, ty: &TypeRef) -> Result<()> {
        if !matches!(self.current_kind(), TokenKind::Identifier(word) if word == "bits") {
            return Ok(());
        }
        let span = self.current_span();
        let width = self.layout_argument()?;
        let limit = match &ty.kind {
            TypeRefKind::Named { name, args } if args.is_empty() && !ty.indirect => {
                match name.as_str() {
                    "uint8" => 8,
                    "uint16" => 16,
                    "uint32" => 32,
                    "uint64" => 64,
                    _ => 0,
                }
            }
            _ => 0,
        };
        if width == 0 || width > limit {
            return Err(Diagnostic::coded_at("AU2085", span, "a C bit-field needs an unsigned integer type and a positive width within that type"));
        }
        self.c_bit_widths.insert(field.to_owned(), width as u8);
        Ok(())
    }
}
