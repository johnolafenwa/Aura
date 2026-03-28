use crate::ast::{
    Argument, AssignStmt, AssignTarget, BinaryOp, BreakStmt, ClassDecl, ContinueStmt, EnumDecl,
    EnumVariantDecl, Expr, ExprKind, ExprStmt, FieldDecl, ForStmt, FormatPart, FunctionDecl,
    IfBranch, IfStmt, ImplDecl, ImportDecl, ImportKind, Item, MatchArm, MatchStmt, Module, Param,
    Pattern, ReceiverKind, ReturnStmt, SelectArm, SelectStmt, Stmt, TraitDecl, TypeRef, UnaryOp,
    VariantPattern, WhileStmt, WithStmt,
};
use crate::diag::{Diagnostic, Result, Span};
use crate::lexer::{lex, Token, TokenKind};

pub fn parse(source: &str) -> Result<Module> {
    let tokens = lex(source)?;
    Parser { tokens, index: 0 }.parse_module()
}

pub fn parse_expression(source: &str) -> Result<Expr> {
    let tokens = lex(source)?;
    let mut parser = Parser { tokens, index: 0 };
    parser.skip_newlines();
    let expr = parser.parse_expr()?;
    parser.skip_newlines();
    if !parser.at_eof() {
        return Err(parser.error_here("unexpected trailing tokens after expression"));
    }
    Ok(expr)
}

struct Parser {
    tokens: Vec<Token>,
    index: usize,
}

impl Parser {
    fn parse_module(&mut self) -> Result<Module> {
        let mut imports = Vec::new();
        let mut items = Vec::new();
        let mut top_level_stmts = Vec::new();
        self.skip_newlines();

        while !self.at_eof() {
            if self.at_keyword_import() || self.at_keyword_from() {
                imports.push(self.parse_import()?);
            } else if self.at_simple(&TokenKind::KwPublic)
                || self.at_copy_class_start()
                || self.at_keyword_class()
                || self.at_keyword_enum()
                || self.at_keyword_def()
                || self.at_keyword_trait()
                || self.at_keyword_impl()
            {
                items.push(self.parse_item()?);
            } else {
                top_level_stmts.push(self.parse_stmt()?);
            }
            self.skip_newlines();
        }

        Ok(Module {
            imports,
            items,
            top_level_stmts,
        })
    }

    fn parse_item(&mut self) -> Result<Item> {
        let public = self.eat_simple(&TokenKind::KwPublic).is_some();
        if self.at_copy_class_start() || self.at_keyword_class() {
            Ok(Item::Class(self.parse_class(public)?))
        } else if self.at_keyword_enum() {
            Ok(Item::Enum(self.parse_enum(public)?))
        } else if self.at_keyword_def() {
            Ok(Item::Function(self.parse_function(public)?))
        } else if self.at_keyword_trait() {
            Ok(Item::Trait(self.parse_trait(public)?))
        } else if self.at_keyword_impl() {
            if public {
                return Err(self.error_here("`public` is not allowed on `impl` blocks"));
            }
            Ok(Item::Impl(self.parse_impl()?))
        } else {
            Err(self.error_here("expected `class`, `enum`, `def`, `trait`, or `impl`"))
        }
    }

    fn parse_import(&mut self) -> Result<ImportDecl> {
        if self.at_keyword_import() {
            let span = self.expect_keyword(TokenKind::KwImport)?.span;
            let path = self.parse_identifier_path()?;
            self.expect_newline()?;
            return Ok(ImportDecl {
                kind: ImportKind::Module { path },
                span,
            });
        }

        let span = self.expect_keyword(TokenKind::KwFrom)?.span;
        let module_path = self.parse_identifier_path()?;
        self.expect_keyword(TokenKind::KwImport)?;
        let mut names = Vec::new();
        loop {
            names.push(self.expect_identifier()?);
            if self.eat_simple(&TokenKind::Comma).is_none() {
                break;
            }
        }
        self.expect_newline()?;
        Ok(ImportDecl {
            kind: ImportKind::From { module_path, names },
            span,
        })
    }

    fn parse_class(&mut self, public: bool) -> Result<ClassDecl> {
        let copy = if matches!(self.current_kind(), TokenKind::Identifier(name) if name == "copy") {
            self.bump();
            true
        } else {
            false
        };
        let span = self.expect_keyword(TokenKind::KwClass)?.span;
        let name = self.expect_identifier()?;
        let (type_params, type_param_bounds) = self.parse_optional_type_params(true)?;
        self.expect_simple(TokenKind::Colon)?;
        self.expect_newline()?;
        self.expect_simple(TokenKind::Indent)?;

        let mut fields = Vec::new();
        let mut methods = Vec::new();
        while !self.at_simple(&TokenKind::Dedent) && !self.at_eof() {
            if self.at_simple(&TokenKind::Newline) {
                self.bump();
                continue;
            }
            if self.at_simple(&TokenKind::KwPass) {
                self.parse_pass_stmt()?;
                continue;
            }
            let method_public = self.at_simple(&TokenKind::KwPublic)
                && matches!(self.peek_kind_at(self.index + 1), Some(TokenKind::KwDef));
            if method_public {
                self.bump();
            }
            if self.at_keyword_def() {
                methods.push(self.parse_function_with_receiver(true, method_public)?);
            } else {
                fields.push(self.parse_field()?);
            }
        }

        self.expect_simple(TokenKind::Dedent)?;

        Ok(ClassDecl {
            public,
            copy,
            name,
            type_params,
            type_param_bounds,
            fields,
            methods,
            span,
        })
    }

    fn parse_enum(&mut self, public: bool) -> Result<EnumDecl> {
        let span = self.expect_keyword(TokenKind::KwEnum)?.span;
        let name = self.expect_identifier()?;
        let (type_params, type_param_bounds) = self.parse_optional_type_params(true)?;
        self.expect_simple(TokenKind::Colon)?;
        self.expect_newline()?;
        self.expect_simple(TokenKind::Indent)?;

        let mut variants = Vec::new();
        while !self.at_simple(&TokenKind::Dedent) && !self.at_eof() {
            if self.at_simple(&TokenKind::Newline) {
                self.bump();
                continue;
            }
            variants.push(self.parse_enum_variant()?);
        }

        self.expect_simple(TokenKind::Dedent)?;

        Ok(EnumDecl {
            public,
            name,
            type_params,
            type_param_bounds,
            variants,
            span,
        })
    }

    fn parse_enum_variant(&mut self) -> Result<EnumVariantDecl> {
        let span = self.current_span();
        let name = self.expect_identifier()?;
        let payload = if self.eat_simple(&TokenKind::LParen).is_some() {
            let ty = self.parse_type()?;
            self.expect_simple(TokenKind::RParen)?;
            Some(ty)
        } else {
            None
        };
        self.expect_newline()?;
        Ok(EnumVariantDecl {
            name,
            payload,
            span,
        })
    }

    fn parse_field(&mut self) -> Result<FieldDecl> {
        let public = self.eat_simple(&TokenKind::KwPublic).is_some();
        let span = self.current_span();
        let name = self.expect_identifier()?;
        self.expect_simple(TokenKind::Colon)?;
        let ty = self.parse_type()?;
        let default = if self.eat_simple(&TokenKind::Equal).is_some() {
            Some(self.parse_expr()?)
        } else {
            None
        };
        self.expect_newline()?;

        Ok(FieldDecl {
            public,
            name,
            ty,
            default,
            span,
        })
    }

    fn parse_function(&mut self, public: bool) -> Result<FunctionDecl> {
        self.parse_function_with_receiver(false, public)
    }

    fn parse_function_with_receiver(
        &mut self,
        allow_receiver: bool,
        public: bool,
    ) -> Result<FunctionDecl> {
        let span = self.expect_keyword(TokenKind::KwDef)?.span;
        let name = self.expect_identifier()?;
        let (type_params, type_param_bounds) = self.parse_optional_type_params(true)?;
        self.expect_simple(TokenKind::LParen)?;
        let (receiver, params) = self.parse_params(allow_receiver)?;
        self.expect_simple(TokenKind::RParen)?;
        let return_type = if self.eat_simple(&TokenKind::Arrow).is_some() {
            self.parse_type()?
        } else {
            TypeRef {
                name: "None".to_string(),
                args: Vec::new(),
                indirect: false,
                span,
            }
        };
        self.expect_simple(TokenKind::Colon)?;
        self.expect_newline()?;
        let body = self.parse_block()?;

        Ok(FunctionDecl {
            public,
            name,
            type_params,
            type_param_bounds,
            receiver,
            params,
            return_type,
            body,
            span,
        })
    }

    fn parse_trait(&mut self, public: bool) -> Result<TraitDecl> {
        let span = self.expect_keyword(TokenKind::KwTrait)?.span;
        let name = self.expect_identifier()?;
        let (type_params, _) = self.parse_optional_type_params(false)?;
        self.expect_simple(TokenKind::Colon)?;
        self.expect_newline()?;
        self.expect_simple(TokenKind::Indent)?;

        let mut methods = Vec::new();
        while !self.at_simple(&TokenKind::Dedent) && !self.at_eof() {
            if self.at_simple(&TokenKind::Newline) {
                self.bump();
                continue;
            }
            if self.at_simple(&TokenKind::KwPass) {
                self.parse_pass_stmt()?;
                continue;
            }
            methods.push(self.parse_trait_method_signature()?);
        }

        self.expect_simple(TokenKind::Dedent)?;
        Ok(TraitDecl {
            public,
            name,
            type_params,
            methods,
            span,
        })
    }

    fn parse_impl(&mut self) -> Result<ImplDecl> {
        let span = self.expect_keyword(TokenKind::KwImpl)?.span;
        let (type_params, type_param_bounds) = self.parse_optional_type_params(true)?;
        let trait_name = self.expect_identifier()?;
        let mut trait_args = Vec::new();
        if self.eat_simple(&TokenKind::LBracket).is_some() {
            loop {
                trait_args.push(self.parse_type()?);
                if self.eat_simple(&TokenKind::Comma).is_none() {
                    break;
                }
            }
            self.expect_simple(TokenKind::RBracket)?;
        }
        self.expect_keyword(TokenKind::KwFor)?;
        let for_type = self.parse_type()?;
        self.expect_simple(TokenKind::Colon)?;
        self.expect_newline()?;
        self.expect_simple(TokenKind::Indent)?;

        let mut methods = Vec::new();
        while !self.at_simple(&TokenKind::Dedent) && !self.at_eof() {
            if self.at_simple(&TokenKind::Newline) {
                self.bump();
                continue;
            }
            if self.at_simple(&TokenKind::KwPass) {
                self.parse_pass_stmt()?;
                continue;
            }
            methods.push(self.parse_function_with_receiver(true, false)?);
        }

        self.expect_simple(TokenKind::Dedent)?;
        Ok(ImplDecl {
            type_params,
            type_param_bounds,
            trait_name,
            trait_args,
            for_type,
            methods,
            span,
        })
    }

    fn parse_trait_method_signature(&mut self) -> Result<FunctionDecl> {
        let span = self.expect_keyword(TokenKind::KwDef)?.span;
        let name = self.expect_identifier()?;
        let (type_params, type_param_bounds) = self.parse_optional_type_params(true)?;
        self.expect_simple(TokenKind::LParen)?;
        let (receiver, params) = self.parse_params(true)?;
        self.expect_simple(TokenKind::RParen)?;
        let return_type = if self.eat_simple(&TokenKind::Arrow).is_some() {
            self.parse_type()?
        } else {
            TypeRef {
                name: "None".to_string(),
                args: Vec::new(),
                indirect: false,
                span,
            }
        };
        self.expect_newline()?;
        Ok(FunctionDecl {
            public: false,
            name,
            type_params,
            type_param_bounds,
            receiver,
            params,
            return_type,
            body: Vec::new(),
            span,
        })
    }

    fn parse_optional_type_params(
        &mut self,
        allow_bounds: bool,
    ) -> Result<(
        Vec<String>,
        std::collections::BTreeMap<String, Vec<TypeRef>>,
    )> {
        let mut type_params = Vec::new();
        let mut bounds = std::collections::BTreeMap::new();
        if self.eat_simple(&TokenKind::LBracket).is_none() {
            return Ok((type_params, bounds));
        }

        loop {
            let name = self.expect_identifier()?;
            let mut param_bounds = Vec::new();
            if allow_bounds && self.eat_simple(&TokenKind::Colon).is_some() {
                loop {
                    param_bounds.push(self.parse_type()?);
                    if self.eat_simple(&TokenKind::Plus).is_none() {
                        break;
                    }
                }
            }
            type_params.push(name);
            if !param_bounds.is_empty() {
                bounds.insert(type_params.last().unwrap().clone(), param_bounds);
            }
            if self.eat_simple(&TokenKind::Comma).is_none() {
                break;
            }
        }

        self.expect_simple(TokenKind::RBracket)?;
        Ok((type_params, bounds))
    }

    fn parse_params(&mut self, allow_receiver: bool) -> Result<(Option<ReceiverKind>, Vec<Param>)> {
        let mut receiver = None;
        let mut params = Vec::new();

        if self.at_simple(&TokenKind::RParen) {
            return Ok((receiver, params));
        }

        loop {
            if allow_receiver && receiver.is_none() {
                if self.at_borrow_receiver_start() {
                    self.bump();
                    let receiver_kind = if self.eat_simple(&TokenKind::KwMut).is_some() {
                        ReceiverKind::BorrowMut
                    } else {
                        ReceiverKind::Borrow
                    };
                    let self_name = self.expect_identifier()?;
                    if self_name != "self" {
                        return Err(Diagnostic::at(
                            self.current_span(),
                            "expected `self` after receiver modifier",
                        ));
                    }
                    receiver = Some(receiver_kind);
                    if self.eat_simple(&TokenKind::Comma).is_none() {
                        break;
                    }
                    continue;
                }

                if self.at_value_receiver_start() {
                    self.bump();
                    receiver = Some(ReceiverKind::Value);
                    if self.eat_simple(&TokenKind::Comma).is_none() {
                        break;
                    }
                    continue;
                }
            }

            let span = self.current_span();
            let mut passing = ReceiverKind::Value;
            let name = if self.eat_simple(&TokenKind::KwBorrow).is_some() {
                passing = if self.eat_simple(&TokenKind::KwMut).is_some() {
                    ReceiverKind::BorrowMut
                } else {
                    ReceiverKind::Borrow
                };
                self.expect_identifier()?
            } else {
                self.expect_identifier()?
            };
            self.expect_simple(TokenKind::Colon)?;
            if passing == ReceiverKind::Value && self.eat_simple(&TokenKind::KwBorrow).is_some() {
                passing = if self.eat_simple(&TokenKind::KwMut).is_some() {
                    ReceiverKind::BorrowMut
                } else {
                    ReceiverKind::Borrow
                };
            } else if passing != ReceiverKind::Value && self.at_simple(&TokenKind::KwBorrow) {
                return Err(Diagnostic::at(
                    self.current_span(),
                    "parameter borrow mode may be written either before the name or after the colon, not both",
                ));
            }
            let ty = self.parse_type()?;
            let default = if self.eat_simple(&TokenKind::Equal).is_some() {
                Some(self.parse_expr()?)
            } else {
                None
            };
            params.push(Param {
                name,
                passing,
                ty,
                default,
                span,
            });

            if self.eat_simple(&TokenKind::Comma).is_none() {
                break;
            }
        }

        Ok((receiver, params))
    }

    fn at_borrow_receiver_start(&self) -> bool {
        if !self.at_simple(&TokenKind::KwBorrow) {
            return false;
        }

        let mut index = self.index + 1;
        if matches!(self.peek_kind_at(index), Some(TokenKind::KwMut)) {
            index += 1;
        }
        matches!(
            (self.peek_kind_at(index), self.peek_kind_at(index + 1)),
            (Some(TokenKind::Identifier(name)), next) if name == "self" && !matches!(next, Some(TokenKind::Colon))
        )
    }

    fn at_value_receiver_start(&self) -> bool {
        matches!(
            (self.current_kind(), self.peek_kind_at(self.index + 1)),
            (TokenKind::Identifier(name), next) if name == "self" && !matches!(next, Some(TokenKind::Colon))
        )
    }

    fn parse_stmt(&mut self) -> Result<Stmt> {
        if self.at_simple(&TokenKind::KwReturn) {
            self.parse_return_stmt()
        } else if self.at_simple(&TokenKind::KwPass) {
            self.parse_pass_stmt()
        } else if self.at_simple(&TokenKind::KwIf) {
            self.parse_if_stmt()
        } else if self.at_simple(&TokenKind::KwMatch) {
            self.parse_match_stmt()
        } else if self.at_simple(&TokenKind::KwFor) {
            self.parse_for_stmt()
        } else if self.at_simple(&TokenKind::KwWith) {
            self.parse_with_stmt()
        } else if self.at_simple(&TokenKind::KwSelect) {
            self.parse_select_stmt()
        } else if self.at_simple(&TokenKind::KwWhile) {
            self.parse_while_stmt()
        } else if self.at_simple(&TokenKind::KwBreak) {
            self.parse_break_stmt()
        } else if self.at_simple(&TokenKind::KwContinue) {
            self.parse_continue_stmt()
        } else if self.is_assignment_stmt() {
            self.parse_assign_stmt()
        } else {
            self.parse_expr_stmt()
        }
    }

    fn parse_return_stmt(&mut self) -> Result<Stmt> {
        let span = self.expect_keyword(TokenKind::KwReturn)?.span;
        let value = if self.at_simple(&TokenKind::Newline) {
            None
        } else {
            Some(self.parse_expr()?)
        };
        self.expect_newline()?;
        Ok(Stmt::Return(ReturnStmt { value, span }))
    }

    fn parse_assign_stmt(&mut self) -> Result<Stmt> {
        let mutable = self.eat_simple(&TokenKind::KwMut).is_some();
        let span = self.current_span();
        let target = self.parse_assign_target()?;
        let annotation = if matches!(target, AssignTarget::Name(_))
            && self.eat_simple(&TokenKind::Colon).is_some()
        {
            Some(self.parse_type()?)
        } else {
            None
        };
        let op = self.parse_assignment_operator()?;
        let value = self.parse_expr()?;
        self.expect_newline()?;

        Ok(Stmt::Assign(AssignStmt {
            mutable,
            target,
            annotation,
            op,
            value,
            span,
        }))
    }

    fn parse_if_stmt(&mut self) -> Result<Stmt> {
        let span = self.expect_keyword(TokenKind::KwIf)?.span;
        let mut branches = Vec::new();
        let condition = self.parse_expr()?;
        self.expect_simple(TokenKind::Colon)?;
        self.expect_newline()?;
        let body = self.parse_block()?;
        branches.push(IfBranch {
            condition,
            body,
            span,
        });

        while self.at_simple(&TokenKind::KwElif) {
            let branch_span = self.expect_keyword(TokenKind::KwElif)?.span;
            let condition = self.parse_expr()?;
            self.expect_simple(TokenKind::Colon)?;
            self.expect_newline()?;
            let body = self.parse_block()?;
            branches.push(IfBranch {
                condition,
                body,
                span: branch_span,
            });
        }

        let else_body = if self.at_simple(&TokenKind::KwElse) {
            self.bump();
            self.expect_simple(TokenKind::Colon)?;
            self.expect_newline()?;
            Some(self.parse_block()?)
        } else {
            None
        };

        Ok(Stmt::If(IfStmt {
            branches,
            else_body,
            span,
        }))
    }

    fn parse_match_stmt(&mut self) -> Result<Stmt> {
        let span = self.expect_keyword(TokenKind::KwMatch)?.span;
        let borrow_mode = self.parse_optional_borrow_mode();
        let scrutinee = self.parse_expr()?;
        self.expect_simple(TokenKind::Colon)?;
        self.expect_newline()?;
        self.expect_simple(TokenKind::Indent)?;

        let mut arms = Vec::new();
        while !self.at_simple(&TokenKind::Dedent) && !self.at_eof() {
            if self.at_simple(&TokenKind::Newline) {
                self.bump();
                continue;
            }
            arms.push(self.parse_match_arm()?);
        }

        self.expect_simple(TokenKind::Dedent)?;

        Ok(Stmt::Match(MatchStmt {
            scrutinee,
            borrow_mode,
            arms,
            span,
        }))
    }

    fn parse_for_stmt(&mut self) -> Result<Stmt> {
        let span = self.expect_keyword(TokenKind::KwFor)?.span;
        let binding = self.expect_identifier()?;
        self.expect_simple(TokenKind::KwIn)?;
        let borrow_mode = self.parse_optional_borrow_mode();
        let iterable = self.parse_expr()?;
        self.expect_simple(TokenKind::Colon)?;
        self.expect_newline()?;
        let body = self.parse_block()?;
        Ok(Stmt::For(ForStmt {
            binding,
            iterable,
            borrow_mode,
            body,
            span,
        }))
    }

    fn parse_with_stmt(&mut self) -> Result<Stmt> {
        let span = self.expect_keyword(TokenKind::KwWith)?.span;
        let (binding, value) = if matches!(self.current_kind(), TokenKind::Identifier(_))
            && matches!(self.peek_kind(1), Some(TokenKind::Equal))
        {
            let binding = self.expect_identifier()?;
            self.expect_simple(TokenKind::Equal)?;
            let value = self.parse_expr()?;
            (binding, value)
        } else {
            let value = self.parse_expr()?;
            self.expect_simple(TokenKind::KwAs)?;
            let binding = self.expect_identifier()?;
            (binding, value)
        };
        self.expect_simple(TokenKind::Colon)?;
        self.expect_newline()?;
        let body = self.parse_block()?;
        Ok(Stmt::With(WithStmt {
            binding,
            value,
            body,
            span,
        }))
    }

    fn parse_select_stmt(&mut self) -> Result<Stmt> {
        let span = self.expect_keyword(TokenKind::KwSelect)?.span;
        self.expect_simple(TokenKind::Colon)?;
        self.expect_newline()?;
        self.expect_simple(TokenKind::Indent)?;

        let mut arms = Vec::new();
        while !self.at_simple(&TokenKind::Dedent) && !self.at_eof() {
            if self.at_simple(&TokenKind::Newline) {
                self.bump();
                continue;
            }
            arms.push(self.parse_select_arm()?);
        }

        self.expect_simple(TokenKind::Dedent)?;
        Ok(Stmt::Select(SelectStmt { arms, span }))
    }

    fn parse_select_arm(&mut self) -> Result<SelectArm> {
        let span = self.expect_keyword(TokenKind::KwCase)?.span;
        let (binding, expr) = if matches!(self.current_kind(), TokenKind::Identifier(_))
            && matches!(self.peek_kind(1), Some(TokenKind::Equal))
        {
            let binding = self.expect_identifier()?;
            self.expect_simple(TokenKind::Equal)?;
            let expr = self.parse_expr()?;
            (Some(binding), expr)
        } else {
            (None, self.parse_expr()?)
        };
        self.expect_simple(TokenKind::Colon)?;
        self.expect_newline()?;
        let body = self.parse_block()?;
        Ok(SelectArm {
            binding,
            expr,
            body,
            span,
        })
    }

    fn parse_match_arm(&mut self) -> Result<MatchArm> {
        let span = self.expect_keyword(TokenKind::KwCase)?.span;
        let pattern = self.parse_pattern()?;
        self.expect_simple(TokenKind::Colon)?;
        self.expect_newline()?;
        let body = self.parse_block()?;
        Ok(MatchArm {
            pattern,
            body,
            span,
        })
    }

    fn parse_pattern(&mut self) -> Result<Pattern> {
        let span = self.current_span();
        if matches!(self.current_kind(), TokenKind::Identifier(name) if name == "_") {
            self.bump();
            return Ok(Pattern::Wildcard(span));
        }
        let mut segments = vec![self.expect_identifier()?];
        while self.eat_simple(&TokenKind::Dot).is_some() {
            segments.push(self.expect_identifier()?);
        }
        let variant_name = segments
            .pop()
            .expect("pattern should contain a variant segment");
        let enum_name = if segments.is_empty() {
            None
        } else {
            Some(segments.join("."))
        };
        let binding = if self.eat_simple(&TokenKind::LParen).is_some() {
            let name = self.expect_identifier()?;
            self.expect_simple(TokenKind::RParen)?;
            Some(name)
        } else {
            None
        };
        Ok(Pattern::Variant(VariantPattern {
            enum_name,
            variant_name,
            binding,
            span,
        }))
    }

    fn parse_while_stmt(&mut self) -> Result<Stmt> {
        let span = self.expect_keyword(TokenKind::KwWhile)?.span;
        let condition = self.parse_expr()?;
        self.expect_simple(TokenKind::Colon)?;
        self.expect_newline()?;
        let body = self.parse_block()?;
        Ok(Stmt::While(WhileStmt {
            condition,
            body,
            span,
        }))
    }

    fn parse_break_stmt(&mut self) -> Result<Stmt> {
        let span = self.expect_keyword(TokenKind::KwBreak)?.span;
        self.expect_newline()?;
        Ok(Stmt::Break(BreakStmt { span }))
    }

    fn parse_continue_stmt(&mut self) -> Result<Stmt> {
        let span = self.expect_keyword(TokenKind::KwContinue)?.span;
        self.expect_newline()?;
        Ok(Stmt::Continue(ContinueStmt { span }))
    }

    fn parse_pass_stmt(&mut self) -> Result<Stmt> {
        let span = self.expect_keyword(TokenKind::KwPass)?.span;
        self.expect_newline()?;
        Ok(Stmt::Pass(crate::ast::PassStmt { span }))
    }

    fn parse_expr_stmt(&mut self) -> Result<Stmt> {
        let span = self.current_span();
        let expr = self.parse_expr()?;
        self.expect_newline()?;
        Ok(Stmt::Expr(ExprStmt { expr, span }))
    }

    fn parse_block(&mut self) -> Result<Vec<Stmt>> {
        self.expect_simple(TokenKind::Indent)?;

        let mut body = Vec::new();
        while !self.at_simple(&TokenKind::Dedent) && !self.at_eof() {
            if self.at_simple(&TokenKind::Newline) {
                self.bump();
                continue;
            }
            body.push(self.parse_stmt()?);
        }

        self.expect_simple(TokenKind::Dedent)?;
        Ok(body)
    }

    fn parse_type(&mut self) -> Result<TypeRef> {
        let span = self.current_span();
        let indirect = self.eat_simple(&TokenKind::KwIndirect).is_some();
        let name = self.parse_identifier_path()?.join(".");
        let mut args = Vec::new();

        if self.eat_simple(&TokenKind::LBracket).is_some() {
            loop {
                args.push(self.parse_type()?);
                if self.eat_simple(&TokenKind::Comma).is_none() {
                    break;
                }
            }
            self.expect_simple(TokenKind::RBracket)?;
        }

        let mut ty = TypeRef {
            name,
            args,
            indirect,
            span,
        };
        if self.eat_simple(&TokenKind::Question).is_some() {
            ty = TypeRef {
                name: "Option".to_string(),
                args: vec![ty],
                indirect,
                span,
            };
        }

        Ok(ty)
    }

    fn parse_identifier_path(&mut self) -> Result<Vec<String>> {
        let mut path = vec![self.expect_identifier()?];
        while self.eat_simple(&TokenKind::Dot).is_some() {
            path.push(self.expect_identifier()?);
        }
        Ok(path)
    }

    fn parse_expr(&mut self) -> Result<Expr> {
        self.parse_or()
    }

    fn parse_or(&mut self) -> Result<Expr> {
        let mut expr = self.parse_and()?;

        while self.eat_simple(&TokenKind::KwOr).is_some() {
            let right = self.parse_and()?;
            let span = expr.span;
            expr = Expr {
                kind: ExprKind::Binary {
                    op: BinaryOp::Or,
                    left: Box::new(expr),
                    right: Box::new(right),
                },
                span,
            };
        }

        Ok(expr)
    }

    fn parse_and(&mut self) -> Result<Expr> {
        let mut expr = self.parse_not()?;

        while self.eat_simple(&TokenKind::KwAnd).is_some() {
            let right = self.parse_not()?;
            let span = expr.span;
            expr = Expr {
                kind: ExprKind::Binary {
                    op: BinaryOp::And,
                    left: Box::new(expr),
                    right: Box::new(right),
                },
                span,
            };
        }

        Ok(expr)
    }

    fn parse_not(&mut self) -> Result<Expr> {
        if let Some(token) = self.eat_simple(&TokenKind::KwNot) {
            let value = self.parse_not()?;
            return Ok(Expr {
                kind: ExprKind::Unary {
                    op: UnaryOp::Not,
                    expr: Box::new(value),
                },
                span: token.span,
            });
        }

        self.parse_equality()
    }

    fn parse_equality(&mut self) -> Result<Expr> {
        let mut expr = self.parse_comparison()?;

        loop {
            let op = if self.eat_simple(&TokenKind::EqEq).is_some() {
                Some(BinaryOp::Eq)
            } else if self.eat_simple(&TokenKind::NotEq).is_some() {
                Some(BinaryOp::NotEq)
            } else {
                None
            };

            let Some(op) = op else { break };
            let right = self.parse_comparison()?;
            let span = expr.span;
            expr = Expr {
                kind: ExprKind::Binary {
                    op,
                    left: Box::new(expr),
                    right: Box::new(right),
                },
                span,
            };
        }

        Ok(expr)
    }

    fn parse_comparison(&mut self) -> Result<Expr> {
        let mut expr = self.parse_additive()?;

        loop {
            let op = if self.eat_simple(&TokenKind::Less).is_some() {
                Some(BinaryOp::Less)
            } else if self.eat_simple(&TokenKind::LessEq).is_some() {
                Some(BinaryOp::LessEq)
            } else if self.eat_simple(&TokenKind::Greater).is_some() {
                Some(BinaryOp::Greater)
            } else if self.eat_simple(&TokenKind::GreaterEq).is_some() {
                Some(BinaryOp::GreaterEq)
            } else {
                None
            };

            let Some(op) = op else { break };
            let right = self.parse_additive()?;
            let span = expr.span;
            expr = Expr {
                kind: ExprKind::Binary {
                    op,
                    left: Box::new(expr),
                    right: Box::new(right),
                },
                span,
            };
        }

        Ok(expr)
    }

    fn parse_additive(&mut self) -> Result<Expr> {
        let mut expr = self.parse_multiplicative()?;

        loop {
            let op = if self.eat_simple(&TokenKind::Plus).is_some() {
                Some(BinaryOp::Add)
            } else if self.eat_simple(&TokenKind::Minus).is_some() {
                Some(BinaryOp::Sub)
            } else {
                None
            };

            let Some(op) = op else { break };
            let right = self.parse_multiplicative()?;
            let span = expr.span;
            expr = Expr {
                kind: ExprKind::Binary {
                    op,
                    left: Box::new(expr),
                    right: Box::new(right),
                },
                span,
            };
        }

        Ok(expr)
    }

    fn parse_multiplicative(&mut self) -> Result<Expr> {
        let mut expr = self.parse_prefix()?;

        loop {
            let op = if self.eat_simple(&TokenKind::Star).is_some() {
                Some(BinaryOp::Mul)
            } else if self.eat_simple(&TokenKind::Slash).is_some() {
                Some(BinaryOp::Div)
            } else if self.eat_simple(&TokenKind::Percent).is_some() {
                Some(BinaryOp::Mod)
            } else {
                None
            };

            let Some(op) = op else { break };
            let right = self.parse_prefix()?;
            let span = expr.span;
            expr = Expr {
                kind: ExprKind::Binary {
                    op,
                    left: Box::new(expr),
                    right: Box::new(right),
                },
                span,
            };
        }

        Ok(expr)
    }

    fn parse_prefix(&mut self) -> Result<Expr> {
        if let Some(token) = self.eat_simple(&TokenKind::KwTry) {
            let value = self.parse_prefix()?;
            return Ok(Expr {
                kind: ExprKind::Try(Box::new(value)),
                span: token.span,
            });
        }

        if let Some(token) = self.eat_simple(&TokenKind::KwSpawn) {
            let detached = self.eat_simple(&TokenKind::KwDetached).is_some();
            let value = self.parse_postfix()?;
            return Ok(Expr {
                kind: ExprKind::Spawn {
                    detached,
                    value: Box::new(value),
                },
                span: token.span,
            });
        }

        if let Some(token) = self.eat_simple(&TokenKind::Minus) {
            let value = self.parse_prefix()?;
            return Ok(Expr {
                kind: ExprKind::Unary {
                    op: UnaryOp::Neg,
                    expr: Box::new(value),
                },
                span: token.span,
            });
        }

        self.parse_postfix()
    }

    fn parse_postfix(&mut self) -> Result<Expr> {
        let mut expr = self.parse_primary()?;

        loop {
            if self.eat_simple(&TokenKind::LBracket).is_some() {
                let mut type_args = Vec::new();
                loop {
                    type_args.push(self.parse_type()?);
                    if self.eat_simple(&TokenKind::Comma).is_none() {
                        break;
                    }
                }
                self.expect_simple(TokenKind::RBracket)?;
                let span = expr.span;
                expr = Expr {
                    kind: ExprKind::Specialize {
                        expr: Box::new(expr),
                        type_args,
                    },
                    span,
                };
                continue;
            }

            if self.eat_simple(&TokenKind::Dot).is_some() {
                let field_span = self.current_span();
                let field = self.expect_member_name()?;
                expr = Expr {
                    kind: ExprKind::Member {
                        object: Box::new(expr),
                        field,
                    },
                    span: field_span,
                };
                continue;
            }

            if self.eat_simple(&TokenKind::LParen).is_some() {
                let args = self.parse_args()?;
                self.expect_simple(TokenKind::RParen)?;
                let span = expr.span;
                expr = Expr {
                    kind: ExprKind::Call {
                        callee: Box::new(expr),
                        args,
                    },
                    span,
                };
                continue;
            }

            if self.at_simple(&TokenKind::KwAs) && self.next_starts_numeric_cast_type() {
                self.bump();
                let ty = self.parse_type()?;
                let span = expr.span;
                expr = Expr {
                    kind: ExprKind::Cast {
                        expr: Box::new(expr),
                        ty,
                    },
                    span,
                };
                continue;
            }

            break;
        }

        Ok(expr)
    }

    fn next_starts_numeric_cast_type(&self) -> bool {
        matches!(
            self.peek_kind(1),
            Some(TokenKind::Identifier(name))
                if matches!(
                    name.as_str(),
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
                        | "float32"
                        | "float64"
                )
        )
    }

    fn parse_primary(&mut self) -> Result<Expr> {
        let token = self.bump();

        match token.kind {
            TokenKind::Identifier(name) => Ok(Expr {
                kind: ExprKind::Name(name),
                span: token.span,
            }),
            TokenKind::IntLiteral(value) => Ok(Expr {
                kind: ExprKind::Int(value),
                span: token.span,
            }),
            TokenKind::DurationLiteral(value) => Ok(Expr {
                kind: ExprKind::DurationMillis(value),
                span: token.span,
            }),
            TokenKind::FloatLiteral(value) => Ok(Expr {
                kind: ExprKind::Float(value),
                span: token.span,
            }),
            TokenKind::BoolLiteral(value) => Ok(Expr {
                kind: ExprKind::Bool(value),
                span: token.span,
            }),
            TokenKind::StringLiteral(value) => Ok(Expr {
                kind: ExprKind::String(value),
                span: token.span,
            }),
            TokenKind::FStringLiteral(value) => Ok(Expr {
                kind: ExprKind::FString(self.parse_format_parts(&value, token.span)?),
                span: token.span,
            }),
            TokenKind::LParen => {
                let inner = self.parse_expr()?;
                self.expect_simple(TokenKind::RParen)?;
                Ok(Expr {
                    kind: ExprKind::Group(Box::new(inner)),
                    span: token.span,
                })
            }
            TokenKind::KwBorrow => Err(Diagnostic::at(
                token.span,
                "call arguments cannot start with `borrow`; pass the value directly",
            )),
            TokenKind::LBracket => Err(Diagnostic::at(
                token.span,
                "list literals are not implemented yet",
            )),
            other => Err(Diagnostic::at(
                token.span,
                format!("unexpected token in expression: {:?}", other),
            )),
        }
    }

    fn parse_args(&mut self) -> Result<Vec<Argument>> {
        let mut args = Vec::new();

        if self.at_simple(&TokenKind::RParen) {
            return Ok(args);
        }

        loop {
            let span = self.current_span();
            let argument = if let TokenKind::Identifier(name) = self.current_kind().clone() {
                if matches!(self.peek_kind(1), Some(TokenKind::Equal)) {
                    self.bump();
                    self.bump();
                    let value = self.parse_expr()?;
                    Argument {
                        name: Some(name),
                        value,
                        span,
                    }
                } else {
                    let value = self.parse_expr()?;
                    Argument {
                        name: None,
                        value,
                        span,
                    }
                }
            } else {
                let value = self.parse_expr()?;
                Argument {
                    name: None,
                    value,
                    span,
                }
            };

            args.push(argument);
            if self.eat_simple(&TokenKind::Comma).is_none() {
                break;
            }
        }

        Ok(args)
    }

    fn is_assignment_stmt(&self) -> bool {
        let mut idx = self.index;
        if matches!(self.peek_kind_at(idx), Some(TokenKind::KwMut)) {
            idx += 1;
        }

        if !matches!(self.peek_kind_at(idx), Some(TokenKind::Identifier(_))) {
            return false;
        }
        idx += 1;
        let mut saw_member = false;

        while matches!(self.peek_kind_at(idx), Some(TokenKind::Dot))
            && matches!(self.peek_kind_at(idx + 1), Some(TokenKind::Identifier(_)))
        {
            saw_member = true;
            idx += 2;
        }

        if self.is_assignment_operator_kind(self.peek_kind_at(idx)) {
            return true;
        }

        if !saw_member && matches!(self.peek_kind_at(idx), Some(TokenKind::Colon)) {
            idx += 1;
            idx = self.skip_type_tokens(idx);
            return self.is_assignment_operator_kind(self.peek_kind_at(idx));
        }

        false
    }

    fn parse_assign_target(&mut self) -> Result<AssignTarget> {
        let span = self.current_span();
        let name = self.expect_identifier()?;
        let mut target = AssignTarget::Name(name);

        while self.eat_simple(&TokenKind::Dot).is_some() {
            let field = self.expect_member_name()?;
            let object = assign_target_to_expr(target, span);
            target = AssignTarget::Member {
                object: Box::new(object),
                field,
            };
        }

        Ok(target)
    }

    fn parse_assignment_operator(&mut self) -> Result<Option<BinaryOp>> {
        let token = self.bump();
        match token.kind {
            TokenKind::Equal => Ok(None),
            TokenKind::PlusEqual => Ok(Some(BinaryOp::Add)),
            TokenKind::MinusEqual => Ok(Some(BinaryOp::Sub)),
            TokenKind::StarEqual => Ok(Some(BinaryOp::Mul)),
            TokenKind::SlashEqual => Ok(Some(BinaryOp::Div)),
            TokenKind::PercentEqual => Ok(Some(BinaryOp::Mod)),
            other => Err(Diagnostic::at(
                token.span,
                format!("expected assignment operator, found {:?}", other),
            )),
        }
    }

    fn is_assignment_operator_kind(&self, kind: Option<&TokenKind>) -> bool {
        matches!(
            kind,
            Some(
                TokenKind::Equal
                    | TokenKind::PlusEqual
                    | TokenKind::MinusEqual
                    | TokenKind::StarEqual
                    | TokenKind::SlashEqual
                    | TokenKind::PercentEqual
            )
        )
    }

    fn skip_type_tokens(&self, mut idx: usize) -> usize {
        if matches!(self.peek_kind_at(idx), Some(TokenKind::KwIndirect)) {
            idx += 1;
        }
        if !matches!(self.peek_kind_at(idx), Some(TokenKind::Identifier(_))) {
            return idx;
        }
        idx += 1;
        while matches!(self.peek_kind_at(idx), Some(TokenKind::Dot))
            && matches!(self.peek_kind_at(idx + 1), Some(TokenKind::Identifier(_)))
        {
            idx += 2;
        }

        while matches!(self.peek_kind_at(idx), Some(TokenKind::LBracket)) {
            let mut depth = 0usize;
            loop {
                match self.peek_kind_at(idx) {
                    Some(TokenKind::LBracket) => depth += 1,
                    Some(TokenKind::RBracket) => {
                        depth = depth.saturating_sub(1);
                        idx += 1;
                        if depth == 0 {
                            break;
                        }
                        continue;
                    }
                    Some(_) => {}
                    None => return idx,
                }
                idx += 1;
            }
        }

        if matches!(self.peek_kind_at(idx), Some(TokenKind::Question)) {
            idx += 1;
        }

        idx
    }

    fn parse_optional_borrow_mode(&mut self) -> Option<ReceiverKind> {
        if self.eat_simple(&TokenKind::KwBorrow).is_none() {
            return None;
        }
        if self.eat_simple(&TokenKind::KwMut).is_some() {
            Some(ReceiverKind::BorrowMut)
        } else {
            Some(ReceiverKind::Borrow)
        }
    }

    fn at_copy_class_start(&self) -> bool {
        matches!(
            (self.current_kind(), self.peek_kind_at(self.index + 1)),
            (TokenKind::Identifier(name), Some(TokenKind::KwClass)) if name == "copy"
        )
    }

    fn parse_format_parts(&self, value: &str, span: Span) -> Result<Vec<FormatPart>> {
        let mut parts = Vec::new();
        let mut literal_start = 0usize;
        let chars = value.char_indices().collect::<Vec<_>>();
        let mut index = 0usize;

        while index < chars.len() {
            let (offset, ch) = chars[index];
            if ch != '{' {
                index += 1;
                continue;
            }

            if literal_start < offset {
                parts.push(FormatPart::Literal(
                    value[literal_start..offset].to_string(),
                ));
            }

            let expr_start = offset + ch.len_utf8();
            index += 1;
            let mut expr_end = None;
            while index < chars.len() {
                let (candidate_offset, candidate) = chars[index];
                if candidate == '}' {
                    expr_end = Some(candidate_offset);
                    break;
                }
                index += 1;
            }

            let Some(expr_end) = expr_end else {
                return Err(Diagnostic::at(span, "unterminated f-string interpolation"));
            };
            let raw_expr_text = &value[expr_start..expr_end];
            let leading_ws = raw_expr_text.len() - raw_expr_text.trim_start().len();
            let expr_text = raw_expr_text.trim();
            if expr_text.is_empty() {
                return Err(Diagnostic::at(
                    span,
                    "f-string interpolation cannot be empty",
                ));
            }
            let mut expr = parse_expression(expr_text).map_err(|error| {
                Diagnostic::at(
                    span,
                    format!("invalid f-string interpolation `{}`: {}", expr_text, error),
                )
            })?;
            let column_offset = span.column + expr_start + leading_ws + 1;
            offset_expr_span(&mut expr, span.line, column_offset);
            parts.push(FormatPart::Expr(expr));
            index += 1;
            literal_start = expr_end + 1;
        }

        if literal_start < value.len() {
            parts.push(FormatPart::Literal(value[literal_start..].to_string()));
        }

        Ok(parts)
    }

    fn skip_newlines(&mut self) {
        while self.at_simple(&TokenKind::Newline) {
            self.bump();
        }
    }

    fn expect_keyword(&mut self, kind: TokenKind) -> Result<Token> {
        let token = self.bump();
        if token.kind == kind {
            Ok(token)
        } else {
            Err(Diagnostic::at(
                token.span,
                format!("expected {:?}, found {:?}", kind, token.kind),
            ))
        }
    }

    fn expect_simple(&mut self, kind: TokenKind) -> Result<Token> {
        self.expect_keyword(kind)
    }

    fn expect_newline(&mut self) -> Result<Token> {
        self.expect_simple(TokenKind::Newline)
    }

    fn expect_identifier(&mut self) -> Result<String> {
        let token = self.bump();
        if let TokenKind::Identifier(name) = token.kind {
            Ok(name)
        } else {
            Err(Diagnostic::at(token.span, "expected identifier"))
        }
    }

    fn expect_member_name(&mut self) -> Result<String> {
        let token = self.bump();
        match token.kind {
            TokenKind::Identifier(name) => Ok(name),
            TokenKind::KwSpawn => Ok("spawn".to_string()),
            other => Err(Diagnostic::at(
                token.span,
                format!("expected member name, found {:?}", other),
            )),
        }
    }

    fn eat_simple(&mut self, kind: &TokenKind) -> Option<Token> {
        if self.at_simple(kind) {
            Some(self.bump())
        } else {
            None
        }
    }

    fn at_simple(&self, kind: &TokenKind) -> bool {
        self.current_kind() == kind
    }

    fn at_keyword_class(&self) -> bool {
        self.at_simple(&TokenKind::KwClass)
    }

    fn at_keyword_enum(&self) -> bool {
        self.at_simple(&TokenKind::KwEnum)
    }

    fn at_keyword_def(&self) -> bool {
        self.at_simple(&TokenKind::KwDef)
    }

    fn at_keyword_trait(&self) -> bool {
        self.at_simple(&TokenKind::KwTrait)
    }

    fn at_keyword_impl(&self) -> bool {
        self.at_simple(&TokenKind::KwImpl)
    }

    fn at_keyword_import(&self) -> bool {
        self.at_simple(&TokenKind::KwImport)
    }

    fn at_keyword_from(&self) -> bool {
        self.at_simple(&TokenKind::KwFrom)
    }

    fn at_eof(&self) -> bool {
        self.at_simple(&TokenKind::Eof)
    }

    fn current_kind(&self) -> &TokenKind {
        &self.tokens[self.index].kind
    }

    fn current_span(&self) -> Span {
        self.tokens[self.index].span
    }

    fn peek_kind(&self, offset: usize) -> Option<&TokenKind> {
        self.tokens
            .get(self.index + offset)
            .map(|token| &token.kind)
    }

    fn peek_kind_at(&self, index: usize) -> Option<&TokenKind> {
        self.tokens.get(index).map(|token| &token.kind)
    }

    fn bump(&mut self) -> Token {
        let token = self.tokens[self.index].clone();
        self.index += 1;
        token
    }

    fn error_here(&self, message: impl Into<String>) -> Diagnostic {
        Diagnostic::at(self.current_span(), message)
    }
}

fn assign_target_to_expr(target: AssignTarget, span: Span) -> Expr {
    match target {
        AssignTarget::Name(name) => Expr {
            kind: ExprKind::Name(name),
            span,
        },
        AssignTarget::Member { object, field } => Expr {
            kind: ExprKind::Member { object, field },
            span,
        },
    }
}

fn offset_expr_span(expr: &mut Expr, line: usize, column_offset: usize) {
    expr.span.line = line;
    expr.span.column += column_offset;

    match &mut expr.kind {
        ExprKind::Unary { expr: inner, .. } | ExprKind::Try(inner) | ExprKind::Group(inner) => {
            offset_expr_span(inner, line, column_offset)
        }
        ExprKind::Cast { expr: inner, .. } => offset_expr_span(inner, line, column_offset),
        ExprKind::Binary { left, right, .. } => {
            offset_expr_span(left, line, column_offset);
            offset_expr_span(right, line, column_offset);
        }
        ExprKind::Call { callee, args } => {
            offset_expr_span(callee, line, column_offset);
            for argument in args {
                argument.span.line = line;
                argument.span.column += column_offset;
                offset_expr_span(&mut argument.value, line, column_offset);
            }
        }
        ExprKind::Specialize {
            expr: inner,
            type_args,
        } => {
            offset_expr_span(inner, line, column_offset);
            for type_arg in type_args {
                offset_type_ref_span(type_arg, line, column_offset);
            }
        }
        ExprKind::Member { object, .. } | ExprKind::Spawn { value: object, .. } => {
            offset_expr_span(object, line, column_offset);
        }
        ExprKind::Name(_)
        | ExprKind::Int(_)
        | ExprKind::DurationMillis(_)
        | ExprKind::Float(_)
        | ExprKind::Bool(_)
        | ExprKind::String(_) => {}
        ExprKind::FString(parts) => {
            for part in parts {
                if let FormatPart::Expr(inner) = part {
                    offset_expr_span(inner, line, column_offset);
                }
            }
        }
    }
}

fn offset_type_ref_span(type_ref: &mut TypeRef, line: usize, column_offset: usize) {
    type_ref.span.line = line;
    type_ref.span.column += column_offset;
    for arg in &mut type_ref.args {
        offset_type_ref_span(arg, line, column_offset);
    }
}
