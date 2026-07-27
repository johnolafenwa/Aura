use crate::ast::{
    Argument, AssertStmt, AssignStmt, AssignTarget, BinaryOp, BindingPattern, BindingTarget,
    BreakStmt, ClassDecl, CompareLink, CompareOp, ContinueStmt, DestructureStmt, EnumDecl,
    EnumPayloadFieldDecl, EnumVariantDecl, Expr, ExprKind, ExprStmt, FieldDecl, ForStmt,
    FormatPart, FunctionDecl, IfBranch, IfStmt, ImplDecl, ImportDecl, ImportKind, Item,
    LiteralPattern, LiteralPatternKind, MapEntryExpr, MatchArm, MatchExprArm, MatchStmt, Module,
    Param, ParamMode, Pattern, ReceiverKind, ReturnStmt, Stmt, TraitDecl, TuplePattern, TypeRef,
    TypeRefKind, UnaryOp, VariantPattern, WhileStmt, WithStmt,
};
use crate::diag::{Diagnostic, Result, Span};
use crate::integer::IntegerValue;
use crate::lexer::{lex, Token, TokenKind};
use crate::limits::RECURSION_LIMIT;

type TypeParamBounds = std::collections::BTreeMap<String, Vec<TypeRef>>;
type ParsedTypeParams = (Vec<String>, TypeParamBounds);

fn parse_error(span: Span, message: impl Into<String>) -> Diagnostic {
    Diagnostic::coded_at("AU1101", span, message)
}

pub fn parse(source: &str) -> Result<Module> {
    let tokens = lex(source)?;
    Parser::new(tokens).parse_module()
}

pub fn parse_expression(source: &str) -> Result<Expr> {
    parse_expression_with_recursion_depth(source, 0)
}

fn parse_expression_with_recursion_depth(source: &str, recursion_depth: usize) -> Result<Expr> {
    let tokens = lex(source)?;
    let mut parser = Parser::with_recursion_depth(tokens, recursion_depth);
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
    recursion_depth: usize,
}

impl Parser {
    fn new(tokens: Vec<Token>) -> Self {
        Self::with_recursion_depth(tokens, 0)
    }

    fn with_recursion_depth(tokens: Vec<Token>, recursion_depth: usize) -> Self {
        Self {
            tokens,
            index: 0,
            recursion_depth,
        }
    }

    fn enter_recursion(&mut self, kind: &str) -> Result<()> {
        if self.recursion_depth >= RECURSION_LIMIT {
            return Err(self.error_here(format!(
                "{} nesting exceeds the supported recursion limit of {}",
                kind, RECURSION_LIMIT
            )));
        }
        self.recursion_depth += 1;
        Ok(())
    }

    fn exit_recursion(&mut self) {
        self.recursion_depth = self.recursion_depth.saturating_sub(1);
    }

    fn check_expression_chain_limit(&self, count: usize) -> Result<()> {
        if count >= RECURSION_LIMIT {
            Err(self.error_here(format!(
                "expression chain exceeds the supported recursion limit of {}",
                RECURSION_LIMIT
            )))
        } else {
            Ok(())
        }
    }

    fn parse_module(&mut self) -> Result<Module> {
        let mut imports = Vec::new();
        let mut items = Vec::new();
        let mut top_level_stmts = Vec::new();
        self.skip_newlines();

        while !self.at_eof() {
            if self.at_keyword_import() || self.at_from_import_start() {
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
        let (payloads, named_payloads) = if self.eat_simple(&TokenKind::LParen).is_some() {
            let mut payloads = Vec::new();
            let mut saw_named = false;
            let mut saw_unnamed = false;
            loop {
                let field_span = self.current_span();
                let (field_name, field_ty) =
                    if matches!(self.current_kind(), TokenKind::Identifier(_))
                        && matches!(self.peek_kind(1), Some(TokenKind::Colon))
                    {
                        let field_name = self.expect_identifier()?;
                        self.expect_simple(TokenKind::Colon)?;
                        let field_ty = self.parse_type()?;
                        saw_named = true;
                        (Some(field_name), field_ty)
                    } else {
                        let field_ty = self.parse_type()?;
                        saw_unnamed = true;
                        (None, field_ty)
                    };
                payloads.push(EnumPayloadFieldDecl {
                    name: field_name,
                    ty: field_ty,
                    span: field_span,
                });
                if self.eat_simple(&TokenKind::Comma).is_none() {
                    break;
                }
            }
            self.expect_simple(TokenKind::RParen)?;
            if saw_named && saw_unnamed {
                return Err(parse_error(
                    span,
                    "enum variant payloads must be either all named or all positional",
                ));
            }
            (payloads, saw_named)
        } else {
            (Vec::new(), false)
        };
        self.expect_newline()?;
        Ok(EnumVariantDecl {
            name,
            payloads,
            named_payloads,
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
        let return_type = self.parse_return_annotation(span)?;
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
        let mut supertraits = Vec::new();
        if !self.at_simple(&TokenKind::Newline) {
            loop {
                supertraits.push(self.parse_type()?);
                if self.eat_simple(&TokenKind::Comma).is_none() {
                    break;
                }
            }
            self.expect_simple(TokenKind::Colon)?;
        }
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
            methods.push(self.parse_trait_method()?);
        }

        self.expect_simple(TokenKind::Dedent)?;
        Ok(TraitDecl {
            public,
            name,
            type_params,
            supertraits,
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

    fn parse_trait_method(&mut self) -> Result<FunctionDecl> {
        let span = self.expect_keyword(TokenKind::KwDef)?.span;
        let name = self.expect_identifier()?;
        let (type_params, type_param_bounds) = self.parse_optional_type_params(true)?;
        self.expect_simple(TokenKind::LParen)?;
        let (receiver, params) = self.parse_params(true)?;
        self.expect_simple(TokenKind::RParen)?;
        let return_type = self.parse_return_annotation(span)?;
        let body = if self.eat_simple(&TokenKind::Colon).is_some() {
            self.expect_newline()?;
            self.parse_block()?
        } else {
            self.expect_newline()?;
            Vec::new()
        };
        Ok(FunctionDecl {
            public: false,
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

    fn parse_return_annotation(&mut self, span: Span) -> Result<TypeRef> {
        if self.eat_simple(&TokenKind::Arrow).is_none() {
            return Ok(TypeRef::named("None", Vec::new(), false, span));
        }

        // ADR-0022 supersedes ADR-0009's borrowed-return syntax: labels are
        // gone and every return is an ordinary owned return.
        if self.at_simple(&TokenKind::KwBorrow) {
            return Err(parse_error(
                self.current_span(),
                "borrowed returns were removed; return an owned value instead",
            ));
        }

        self.parse_type()
    }

    fn parse_optional_type_params(&mut self, allow_bounds: bool) -> Result<ParsedTypeParams> {
        let mut type_params = Vec::new();
        let mut bounds = TypeParamBounds::new();
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
                if self.at_typed_receiver_start() {
                    return Err(Diagnostic::coded_at(
                        "AU3004",
                        self.current_span(),
                        "`self: Type` is not a method receiver; use `self` for shared access, `own self` to consume, or `mut self` to mutate",
                    ));
                }

                if self.at_borrow_receiver_start() {
                    let span = self.current_span();
                    self.bump();
                    let message = if self.at_simple(&TokenKind::KwMut) {
                        "`borrow mut self` was removed; write `mut self`"
                    } else {
                        "`borrow self` was removed; write `self` for a shared receiver"
                    };
                    return Err(parse_error(span, message));
                }

                if self.at_mut_receiver_start() {
                    if !params.is_empty() {
                        return Err(parse_error(
                            self.current_span(),
                            "method receiver must be the first parameter",
                        ));
                    }
                    self.bump();
                    self.expect_identifier()?;
                    receiver = Some(ReceiverKind::BorrowMut);
                    if self.eat_simple(&TokenKind::Comma).is_none() {
                        break;
                    }
                    continue;
                }

                if self.at_own_receiver_start() {
                    if !params.is_empty() {
                        return Err(parse_error(
                            self.current_span(),
                            "method receiver must be the first parameter",
                        ));
                    }
                    self.bump();
                    self.expect_identifier()?;
                    receiver = Some(ReceiverKind::Value);
                    if self.eat_simple(&TokenKind::Comma).is_none() {
                        break;
                    }
                    continue;
                }

                if self.at_value_receiver_start() {
                    if !params.is_empty() {
                        return Err(parse_error(
                            self.current_span(),
                            "method receiver must be the first parameter",
                        ));
                    }
                    self.bump();
                    receiver = Some(ReceiverKind::Borrow);
                    if self.eat_simple(&TokenKind::Comma).is_none() {
                        break;
                    }
                    continue;
                }
            }

            let span = self.current_span();
            if self.at_simple(&TokenKind::KwBorrow) {
                return Err(parse_error(
                    self.current_span(),
                    "ordinary parameters are written as `name: Type`, `name: mut Type`, or `name: own Type`",
                ));
            }
            if self.at_simple(&TokenKind::KwOwn) {
                return Err(parse_error(
                    self.current_span(),
                    "ordinary owned parameters must be written as `name: own Type`",
                ));
            }
            let mut mode = ParamMode::Default;
            let name = self.expect_identifier()?;
            self.expect_simple(TokenKind::Colon)?;
            if self.eat_simple(&TokenKind::KwOwn).is_some() {
                mode = ParamMode::Own;
            } else if self.at_simple(&TokenKind::KwBorrow) {
                let span = self.current_span();
                self.bump();
                let message = if self.at_simple(&TokenKind::KwMut) {
                    "`borrow mut T` was removed; write `mut T`"
                } else {
                    "`borrow T` was removed; write `T` for shared access"
                };
                return Err(parse_error(span, message));
            } else if self.eat_simple(&TokenKind::KwMut).is_some() {
                mode = ParamMode::BorrowMut;
            }
            let ty = self.parse_type()?;
            let default = if self.eat_simple(&TokenKind::Equal).is_some() {
                Some(self.parse_expr()?)
            } else {
                None
            };
            params.push(Param {
                name,
                mode,
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

    fn at_mut_receiver_start(&self) -> bool {
        matches!(
            (
                self.current_kind(),
                self.peek_kind_at(self.index + 1),
                self.peek_kind_at(self.index + 2),
            ),
            (TokenKind::KwMut, Some(TokenKind::Identifier(name)), next)
                if name == "self" && !matches!(next, Some(TokenKind::Colon))
        )
    }

    fn at_own_receiver_start(&self) -> bool {
        matches!(
            (
                self.current_kind(),
                self.peek_kind_at(self.index + 1),
                self.peek_kind_at(self.index + 2),
            ),
            (TokenKind::KwOwn, Some(TokenKind::Identifier(name)), next)
                if name == "self" && !matches!(next, Some(TokenKind::Colon))
        )
    }

    fn at_typed_receiver_start(&self) -> bool {
        matches!(
            (self.current_kind(), self.peek_kind_at(self.index + 1)),
            (TokenKind::Identifier(name), Some(TokenKind::Colon)) if name == "self"
        )
    }

    fn at_value_receiver_start(&self) -> bool {
        matches!(
            (self.current_kind(), self.peek_kind_at(self.index + 1)),
            (TokenKind::Identifier(name), next) if name == "self" && !matches!(next, Some(TokenKind::Colon))
        )
    }

    fn parse_stmt(&mut self) -> Result<Stmt> {
        self.enter_recursion("statement")?;
        let result = self.parse_stmt_inner();
        self.exit_recursion();
        result
    }

    fn parse_stmt_inner(&mut self) -> Result<Stmt> {
        if self.at_simple(&TokenKind::KwTry) && matches!(self.peek_kind(1), Some(TokenKind::Colon))
        {
            Err(Diagnostic::coded_at(
                "AU2005",
                self.current_span(),
                "`try`/`except` is not supported; use `Result` with `match` today",
            ))
        } else if self.at_simple(&TokenKind::KwReturn) {
            self.parse_return_stmt()
        } else if self.at_simple(&TokenKind::KwAssert) {
            self.parse_assert_stmt()
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
        } else if self.at_simple(&TokenKind::KwWhile) {
            self.parse_while_stmt()
        } else if self.at_simple(&TokenKind::KwBreak) {
            self.parse_break_stmt()
        } else if self.at_simple(&TokenKind::KwContinue) {
            self.parse_continue_stmt()
        } else if self.is_destructure_assignment_stmt() {
            self.parse_destructure_stmt()
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
        self.expect_statement_terminator()?;
        Ok(Stmt::Return(ReturnStmt { value, span }))
    }

    fn parse_assert_stmt(&mut self) -> Result<Stmt> {
        let span = self.expect_keyword(TokenKind::KwAssert)?.span;
        let condition = self.parse_non_tuple_expr()?;
        let message = if self.eat_simple(&TokenKind::Comma).is_some() {
            Some(self.parse_non_tuple_expr()?)
        } else {
            None
        };
        self.expect_statement_terminator()?;
        Ok(Stmt::Assert(AssertStmt {
            condition,
            message,
            span,
        }))
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
        self.expect_statement_terminator()?;

        Ok(Stmt::Assign(AssignStmt {
            mutable,
            target,
            annotation,
            op,
            value,
            span,
        }))
    }

    fn parse_destructure_stmt(&mut self) -> Result<Stmt> {
        if self.eat_simple(&TokenKind::KwMut).is_some() {
            return Err(parse_error(
                self.current_span(),
                "`mut` destructuring is not supported yet; bind the tuple first and mutate named values explicitly",
            ));
        }

        let span = self.current_span();
        let target = self.parse_binding_target_sequence(false)?;
        self.reject_duplicate_binding_names(&target)?;

        if !self.at_simple(&TokenKind::Equal) {
            if self.is_assignment_operator_kind(Some(self.current_kind())) {
                return Err(parse_error(
                    self.current_span(),
                    "destructuring only supports plain `=`; compound assignment requires a single assignable place",
                ));
            }
            return Err(parse_error(
                self.current_span(),
                "expected `=` after destructuring target",
            ));
        }
        self.bump();

        let value = self.parse_expr()?;
        self.expect_statement_terminator()?;
        Ok(Stmt::Destructure(DestructureStmt {
            target,
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
        let capability = self.parse_match_capability()?;
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
            capability,
            arms,
            span,
        }))
    }

    fn parse_for_stmt(&mut self) -> Result<Stmt> {
        let span = self.expect_keyword(TokenKind::KwFor)?.span;
        let target = self.parse_binding_target_sequence(false)?;
        self.reject_duplicate_binding_names(&target)?;
        self.expect_simple(TokenKind::KwIn)?;
        let borrow_mode = self.parse_optional_for_mode()?;
        let iterable = self.parse_expr()?;
        self.expect_simple(TokenKind::Colon)?;
        self.expect_newline()?;
        let body = self.parse_block()?;
        Ok(Stmt::For(ForStmt {
            target,
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

    fn parse_match_expr_arm(&mut self) -> Result<MatchExprArm> {
        let span = self.expect_keyword(TokenKind::KwCase)?.span;
        let pattern = self.parse_pattern()?;
        self.expect_simple(TokenKind::Colon)?;
        let value = if self.at_simple(&TokenKind::Newline) {
            self.expect_newline()?;
            self.expect_simple(TokenKind::Indent)?;
            let value = self.parse_expr()?;
            self.expect_statement_terminator()?;
            self.expect_simple(TokenKind::Dedent)?;
            value
        } else {
            let value = self.parse_expr()?;
            self.expect_match_expr_arm_terminator()?;
            value
        };
        Ok(MatchExprArm {
            pattern,
            value,
            span,
        })
    }

    fn parse_pattern(&mut self) -> Result<Pattern> {
        self.enter_recursion("pattern")?;
        let result = self.parse_pattern_inner();
        self.exit_recursion();
        result
    }

    fn parse_pattern_inner(&mut self) -> Result<Pattern> {
        let span = self.current_span();
        if self.eat_simple(&TokenKind::LParen).is_some() {
            if self.at_simple(&TokenKind::RParen) {
                return Err(parse_error(span, "empty tuple patterns are not supported"));
            }

            let first = self.parse_pattern()?;
            if self.eat_simple(&TokenKind::Comma).is_none() {
                return Err(parse_error(
                    self.current_span(),
                    "tuple patterns need a comma; write `(pattern,)` for a singleton tuple pattern",
                ));
            }

            let mut elements = vec![first];
            if self.eat_simple(&TokenKind::RParen).is_some() {
                return Ok(Pattern::Tuple(TuplePattern { elements, span }));
            }

            loop {
                elements.push(self.parse_pattern()?);
                if self.eat_simple(&TokenKind::Comma).is_none() {
                    break;
                }
                if self.at_simple(&TokenKind::RParen) {
                    return Err(parse_error(
                        self.current_span(),
                        "trailing commas are only allowed for singleton tuple patterns",
                    ));
                }
            }
            self.expect_simple(TokenKind::RParen)?;
            return Ok(Pattern::Tuple(TuplePattern { elements, span }));
        }

        if matches!(self.current_kind(), TokenKind::Identifier(name) if name == "_") {
            self.bump();
            return Ok(Pattern::Wildcard(span));
        }
        match self.current_kind().clone() {
            TokenKind::BoolLiteral(value) => {
                self.bump();
                return Ok(Pattern::Literal(LiteralPattern {
                    kind: LiteralPatternKind::Bool(value),
                    span,
                }));
            }
            TokenKind::StringLiteral(value) => {
                self.bump();
                return Ok(Pattern::Literal(LiteralPattern {
                    kind: LiteralPatternKind::String(value),
                    span,
                }));
            }
            TokenKind::FloatLiteral(value) => {
                self.bump();
                return Ok(Pattern::Literal(LiteralPattern {
                    kind: LiteralPatternKind::Float(value),
                    span,
                }));
            }
            TokenKind::IntLiteral(value) => {
                self.bump();
                return Ok(Pattern::Literal(LiteralPattern {
                    kind: LiteralPatternKind::Int(IntegerValue::from_literal(value)),
                    span,
                }));
            }
            TokenKind::Minus => {
                let minus = self.bump();
                let kind = match self.current_kind().clone() {
                    TokenKind::IntLiteral(value) => {
                        self.bump();
                        let negative = match IntegerValue::from_literal(value).checked_neg() {
                            Some(value) => value,
                            None => {
                                return Err(parse_error(
                                    minus.span,
                                    "negative integer literal in pattern is outside the supported range",
                                ));
                            }
                        };
                        LiteralPatternKind::Int(negative)
                    }
                    TokenKind::FloatLiteral(value) => {
                        self.bump();
                        LiteralPatternKind::Float(-value)
                    }
                    _ => {
                        return Err(parse_error(
                            minus.span,
                            "match patterns currently support enum variants, `_`, and boolean/string/integer/float literals",
                        ));
                    }
                };
                return Ok(Pattern::Literal(LiteralPattern {
                    kind,
                    span: minus.span,
                }));
            }
            _ => {}
        }
        if !matches!(self.current_kind(), TokenKind::Identifier(_)) {
            return Err(parse_error(
                span,
                "match patterns currently support enum variants, `_`, and boolean/string/integer/float literals",
            ));
        }
        let mut segments = vec![self.expect_identifier()?];
        while self.eat_simple(&TokenKind::Dot).is_some() {
            segments.push(self.expect_identifier()?);
        }
        if segments.len() == 1
            && !matches!(self.current_kind(), TokenKind::LParen)
            && segments[0]
                .chars()
                .next()
                .is_some_and(|ch| ch == '_' || ch.is_ascii_lowercase())
        {
            return Ok(Pattern::Binding(BindingPattern {
                name: segments.remove(0),
                span,
            }));
        }
        let variant_name = segments
            .pop()
            .expect("pattern should contain a variant segment");
        let enum_name = if segments.is_empty() {
            None
        } else {
            Some(segments.join("."))
        };
        let subpatterns = if self.eat_simple(&TokenKind::LParen).is_some() {
            let mut subpatterns = Vec::new();
            if !self.at_simple(&TokenKind::RParen) {
                loop {
                    subpatterns.push(self.parse_pattern()?);
                    if self.eat_simple(&TokenKind::Comma).is_none() {
                        break;
                    }
                }
            }
            self.expect_simple(TokenKind::RParen)?;
            subpatterns
        } else {
            Vec::new()
        };
        Ok(Pattern::Variant(VariantPattern {
            enum_name,
            variant_name,
            subpatterns,
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
        self.expect_statement_terminator()?;
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
        self.enter_recursion("type")?;
        let result = self.parse_type_inner();
        self.exit_recursion();
        result
    }

    fn parse_type_inner(&mut self) -> Result<TypeRef> {
        let span = self.current_span();
        let capability = match self.current_kind() {
            TokenKind::KwMut => Some("mut"),
            TokenKind::KwOwn => Some("own"),
            _ => None,
        };
        if let Some(capability) = capability {
            return Err(parse_error(
                span,
                format!(
                    "`{capability}` is not valid in a type position; capability modifiers belong only on parameters and receivers or on supported `for` and `match` selectors (`mut` also declares mutable local bindings)"
                ),
            ));
        }

        let indirect = self.eat_simple(&TokenKind::KwIndirect).is_some();
        let mut ty = if self.eat_simple(&TokenKind::LParen).is_some() {
            if indirect {
                return Err(parse_error(
                    span,
                    "`indirect` applies only to named types, not tuple types",
                ));
            }
            if self.at_simple(&TokenKind::RParen) {
                return Err(parse_error(span, "empty tuple types are not supported"));
            }

            let first = self.parse_type()?;
            if self.eat_simple(&TokenKind::Comma).is_none() {
                return Err(parse_error(
                    self.current_span(),
                    "tuple types need a comma; write `(T,)` for a singleton tuple type or `T` for the type itself",
                ));
            }

            let mut elements = vec![first];
            if self.eat_simple(&TokenKind::RParen).is_none() {
                loop {
                    elements.push(self.parse_type()?);
                    if self.eat_simple(&TokenKind::Comma).is_none() {
                        break;
                    }
                    if self.at_simple(&TokenKind::RParen) {
                        return Err(parse_error(
                            self.current_span(),
                            "trailing commas are only allowed for singleton tuple types",
                        ));
                    }
                }
                self.expect_simple(TokenKind::RParen)?;
            }
            TypeRef::tuple(elements, false, span)
        } else {
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

            TypeRef::named(name, args, indirect, span)
        };
        if self.eat_simple(&TokenKind::Question).is_some() {
            ty = TypeRef::named("Option", vec![ty], indirect, span);
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
        self.enter_recursion("expression")?;
        let result = self.parse_non_tuple_expr_inner();
        self.exit_recursion();
        result
    }

    /// Parse an expression whose outermost form cannot consume a statement-level
    /// comma. Tuple expressions will be layered above this entry point so
    /// comma-delimited statement syntax can keep an explicit boundary.
    fn parse_non_tuple_expr(&mut self) -> Result<Expr> {
        self.enter_recursion("expression")?;
        let result = self.parse_non_tuple_expr_inner();
        self.exit_recursion();
        result
    }

    fn parse_non_tuple_expr_inner(&mut self) -> Result<Expr> {
        self.parse_conditional()
    }

    fn parse_conditional(&mut self) -> Result<Expr> {
        let then_expr = self.parse_or()?;
        let Some(if_token) = self.eat_simple(&TokenKind::KwIf) else {
            return Ok(then_expr);
        };

        self.enter_recursion("conditional expression")?;
        let result = (|| {
            let condition = self.parse_or()?;
            if self.eat_simple(&TokenKind::KwElse).is_none() {
                return Err(parse_error(
                    if_token.span,
                    "conditional expression requires `else` and an alternative value",
                ));
            }
            let else_expr = self.parse_conditional()?;
            let span = then_expr.span;
            Ok(Expr {
                kind: ExprKind::Conditional {
                    then_expr: Box::new(then_expr),
                    condition: Box::new(condition),
                    else_expr: Box::new(else_expr),
                },
                span,
            })
        })();
        self.exit_recursion();
        result
    }

    fn parse_or(&mut self) -> Result<Expr> {
        let mut expr = self.parse_and()?;
        let mut chain_len = 0usize;

        while self.eat_simple(&TokenKind::KwOr).is_some() {
            chain_len += 1;
            self.check_expression_chain_limit(chain_len)?;
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
        let mut chain_len = 0usize;

        while self.eat_simple(&TokenKind::KwAnd).is_some() {
            chain_len += 1;
            self.check_expression_chain_limit(chain_len)?;
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
        let mut operator_spans = Vec::new();
        while let Some(token) = self.eat_simple(&TokenKind::KwNot) {
            operator_spans.push(token.span);
            self.check_expression_chain_limit(operator_spans.len())?;
        }

        let mut value = self.parse_comparison_chain()?;
        while let Some(span) = operator_spans.pop() {
            value = Expr {
                kind: ExprKind::Unary {
                    op: UnaryOp::Not,
                    expr: Box::new(value),
                },
                span,
            };
        }

        Ok(value)
    }

    /// Parses the Python-shaped comparison level. Equality, ordering, and
    /// membership share one precedence and chain left to right, so
    /// `a < b <= c` is one chain rather than a comparison of a comparison.
    fn parse_comparison_chain(&mut self) -> Result<Expr> {
        let first = self.parse_additive()?;
        let mut links: Vec<CompareLink> = Vec::new();

        loop {
            if let Some(token) = self.eat_simple(&TokenKind::KwIs) {
                return Err(Diagnostic::coded_at(
                    "AU2005",
                    token.span,
                    "`is` is not supported; use `== None` or `match` for optional values",
                ));
            }
            let Some((op, op_span)) = self.eat_comparison_operator() else {
                break;
            };
            self.check_expression_chain_limit(links.len().saturating_add(1))?;
            let operand = self.parse_additive()?;
            links.push(CompareLink {
                op,
                op_span,
                operand,
            });
        }

        if links.is_empty() {
            return Ok(first);
        }
        let span = first.span;
        if links.len() == 1 {
            let CompareLink {
                op,
                op_span,
                operand,
            } = links.remove(0);
            let kind = match op.as_binary_op() {
                Some(op) => ExprKind::Binary {
                    op,
                    left: Box::new(first),
                    right: Box::new(operand),
                },
                None => ExprKind::Membership {
                    value: Box::new(first),
                    container: Box::new(operand),
                    negated: op == CompareOp::NotIn,
                    operator_span: op_span,
                },
            };
            return Ok(Expr { kind, span });
        }

        Ok(Expr {
            kind: ExprKind::CompareChain {
                first: Box::new(first),
                links,
            },
            span,
        })
    }

    /// Consumes one comparison operator, including the two-token `not in`.
    fn eat_comparison_operator(&mut self) -> Option<(CompareOp, Span)> {
        if self.at_simple(&TokenKind::KwNot) && matches!(self.peek_kind(1), Some(TokenKind::KwIn)) {
            let span = self.current_span();
            self.bump();
            self.bump();
            return Some((CompareOp::NotIn, span));
        }
        for (kind, op) in [
            (TokenKind::EqEq, CompareOp::Eq),
            (TokenKind::NotEq, CompareOp::NotEq),
            (TokenKind::LessEq, CompareOp::LessEq),
            (TokenKind::GreaterEq, CompareOp::GreaterEq),
            (TokenKind::Less, CompareOp::Less),
            (TokenKind::Greater, CompareOp::Greater),
            (TokenKind::KwIn, CompareOp::In),
        ] {
            if let Some(token) = self.eat_simple(&kind) {
                return Some((op, token.span));
            }
        }
        None
    }

    fn parse_additive(&mut self) -> Result<Expr> {
        let mut expr = self.parse_multiplicative()?;
        let mut chain_len = 0usize;

        loop {
            let op = if self.eat_simple(&TokenKind::Plus).is_some() {
                Some(BinaryOp::Add)
            } else if self.eat_simple(&TokenKind::Minus).is_some() {
                Some(BinaryOp::Sub)
            } else {
                None
            };

            let Some(op) = op else { break };
            chain_len += 1;
            self.check_expression_chain_limit(chain_len)?;
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
        let mut chain_len = 0usize;

        loop {
            let op = if self.eat_simple(&TokenKind::Star).is_some() {
                Some(BinaryOp::Mul)
            } else if self.eat_simple(&TokenKind::Slash).is_some() {
                Some(BinaryOp::Div)
            } else if self.eat_simple(&TokenKind::DoubleSlash).is_some() {
                Some(BinaryOp::FloorDiv)
            } else if self.eat_simple(&TokenKind::Percent).is_some() {
                Some(BinaryOp::Mod)
            } else {
                None
            };

            let Some(op) = op else { break };
            chain_len += 1;
            self.check_expression_chain_limit(chain_len)?;
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
        self.enter_recursion("expression")?;
        let result = self.parse_prefix_inner();
        self.exit_recursion();
        result
    }

    fn parse_prefix_inner(&mut self) -> Result<Expr> {
        if let Some(token) = self.eat_simple(&TokenKind::KwMatch) {
            let capability = self.parse_match_capability()?;
            let scrutinee = self.parse_expr()?;
            self.expect_simple(TokenKind::Colon)?;
            self.expect_newline()?;
            self.expect_simple(TokenKind::Indent)?;
            let mut arms = Vec::new();
            while !self.at_match_expr_end() && !self.at_eof() {
                if self.at_simple(&TokenKind::Newline) {
                    self.bump();
                    continue;
                }
                arms.push(self.parse_match_expr_arm()?);
            }
            if !self.at_simple(&TokenKind::Dedent) {
                return Err(self.error_here("expected end of match expression"));
            }
            self.expect_simple(TokenKind::Dedent)?;
            return Ok(Expr {
                kind: ExprKind::Match {
                    scrutinee: Box::new(scrutinee),
                    capability,
                    arms,
                },
                span: token.span,
            });
        }

        if let Some(token) = self.eat_simple(&TokenKind::KwTry) {
            let value = self.parse_prefix()?;
            return Ok(Expr {
                kind: ExprKind::Try(Box::new(value)),
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
        let mut chain_len = 0usize;

        loop {
            if self.at_simple(&TokenKind::LBracket) && self.starts_specialization_suffix(&expr) {
                chain_len += 1;
                self.check_expression_chain_limit(chain_len)?;
                self.bump();
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

            if self.eat_simple(&TokenKind::LBracket).is_some() {
                chain_len += 1;
                self.check_expression_chain_limit(chain_len)?;
                let index = self.parse_expr()?;
                self.expect_simple(TokenKind::RBracket)?;
                let span = expr.span;
                expr = Expr {
                    kind: ExprKind::Index {
                        object: Box::new(expr),
                        index: Box::new(index),
                    },
                    span,
                };
                continue;
            }

            if self.eat_simple(&TokenKind::Dot).is_some() {
                chain_len += 1;
                self.check_expression_chain_limit(chain_len)?;
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
                chain_len += 1;
                self.check_expression_chain_limit(chain_len)?;
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
                chain_len += 1;
                self.check_expression_chain_limit(chain_len)?;
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
        matches!(self.peek_kind(1), Some(TokenKind::KwMut | TokenKind::KwOwn))
            || matches!(
                self.peek_kind(1),
                Some(TokenKind::Identifier(name))
                    if matches!(
                        name.as_str(),
                        "int"
                            | "int8"
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
            TokenKind::Identifier(name) => {
                if name == "lambda" {
                    Err(Diagnostic::coded_at(
                        "AU2005",
                        token.span,
                        "lambda expressions are not available yet; use a named `def` today",
                    ))
                } else if name == "Set" && self.eat_simple(&TokenKind::LBrace).is_some() {
                    let mut elements = Vec::new();
                    if !self.at_simple(&TokenKind::RBrace) {
                        loop {
                            elements.push(self.parse_expr()?);
                            if self.eat_simple(&TokenKind::Comma).is_none() {
                                break;
                            }
                        }
                    }
                    self.expect_simple(TokenKind::RBrace)?;
                    Ok(Expr {
                        kind: ExprKind::Set(elements),
                        span: token.span,
                    })
                } else {
                    Ok(Expr {
                        kind: ExprKind::Name(name),
                        span: token.span,
                    })
                }
            }
            TokenKind::KwFrom => Ok(Expr {
                kind: ExprKind::Name("from".to_string()),
                span: token.span,
            }),
            TokenKind::IntLiteral(value) => Ok(Expr {
                kind: ExprKind::Int(value),
                span: token.span,
            }),
            TokenKind::DurationLiteral(value) => Ok(Expr {
                kind: ExprKind::DurationNanos(value),
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
                if self.at_simple(&TokenKind::RParen) {
                    return Err(parse_error(
                        token.span,
                        "empty tuple literals are not supported; use `None` for unit",
                    ));
                }

                let first = self.parse_non_tuple_expr()?;
                if self.eat_simple(&TokenKind::Comma).is_none() {
                    self.expect_simple(TokenKind::RParen)?;
                    return Ok(Expr {
                        kind: ExprKind::Group(Box::new(first)),
                        span: token.span,
                    });
                }

                let mut elements = vec![first];
                if self.eat_simple(&TokenKind::RParen).is_some() {
                    return Ok(Expr {
                        kind: ExprKind::Tuple(elements),
                        span: token.span,
                    });
                }

                loop {
                    elements.push(self.parse_non_tuple_expr()?);
                    if self.eat_simple(&TokenKind::Comma).is_none() {
                        break;
                    }
                    if self.at_simple(&TokenKind::RParen) {
                        return Err(parse_error(
                            self.current_span(),
                            "trailing commas are only allowed for singleton tuple literals",
                        ));
                    }
                }
                self.expect_simple(TokenKind::RParen)?;
                Ok(Expr {
                    kind: ExprKind::Tuple(elements),
                    span: token.span,
                })
            }
            TokenKind::LBracket => {
                let mut elements = Vec::new();
                if !self.at_simple(&TokenKind::RBracket) {
                    loop {
                        elements.push(self.parse_expr()?);
                        if self.at_simple(&TokenKind::KwFor) {
                            return Err(Diagnostic::coded_at(
                                "AU2005",
                                self.current_span(),
                                "comprehensions are not available yet; use an explicit loop today",
                            ));
                        }
                        if self.eat_simple(&TokenKind::Comma).is_none() {
                            break;
                        }
                    }
                }
                self.expect_simple(TokenKind::RBracket)?;
                Ok(Expr {
                    kind: ExprKind::List(elements),
                    span: token.span,
                })
            }
            TokenKind::LBrace => {
                if self.at_simple(&TokenKind::RBrace) {
                    self.bump();
                    return Ok(Expr {
                        kind: ExprKind::Map(Vec::new()),
                        span: token.span,
                    });
                }

                let first = self.parse_expr()?;
                if self.eat_simple(&TokenKind::Colon).is_none() {
                    let mut elements = vec![first];
                    while self.eat_simple(&TokenKind::Comma).is_some() {
                        elements.push(self.parse_expr()?);
                    }
                    self.expect_simple(TokenKind::RBrace)?;
                    return Ok(Expr {
                        kind: ExprKind::Set(elements),
                        span: token.span,
                    });
                }

                let mut entries = Vec::new();
                let value = self.parse_expr()?;
                entries.push(MapEntryExpr { key: first, value });
                while self.eat_simple(&TokenKind::Comma).is_some() {
                    let key = self.parse_expr()?;
                    self.expect_simple(TokenKind::Colon)?;
                    let value = self.parse_expr()?;
                    entries.push(MapEntryExpr { key, value });
                }
                self.expect_simple(TokenKind::RBrace)?;
                Ok(Expr {
                    kind: ExprKind::Map(entries),
                    span: token.span,
                })
            }
            TokenKind::KwBorrow => Err(parse_error(
                token.span,
                "call arguments cannot start with `borrow`; pass the value directly",
            )),
            TokenKind::KwMut => Err(parse_error(
                token.span,
                "`mut` cannot prefix a call argument or other expression; pass the value directly because the callee parameter declares shared, mutable, or owned access. Capability modifiers belong only on parameters and receivers or on supported `for` and `match` selectors (`mut` also declares mutable local bindings)",
            )),
            TokenKind::KwOwn => Err(parse_error(
                token.span,
                "`own` cannot prefix a call argument or other expression; pass the value directly because the callee parameter declares shared, mutable, or owned access. Capability modifiers belong only on parameters and receivers or on supported `for` and `match` selectors (`mut` also declares mutable local bindings)",
            )),
            other => Err(parse_error(
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
            let contextual_name = match self.current_kind() {
                TokenKind::Identifier(name) => Some(name.clone()),
                TokenKind::KwFrom => Some("from".to_string()),
                _ => None,
            };
            let argument = if let Some(name) = contextual_name {
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

        if !self.is_contextual_identifier_at(idx) {
            return false;
        }
        idx += 1;
        let mut saw_suffix = false;

        loop {
            if matches!(self.peek_kind_at(idx), Some(TokenKind::Dot))
                && self.is_contextual_identifier_at(idx + 1)
            {
                saw_suffix = true;
                idx += 2;
                continue;
            }

            if matches!(self.peek_kind_at(idx), Some(TokenKind::LBracket)) {
                let Some(next_idx) = self.skip_bracketed_tokens(idx) else {
                    return false;
                };
                saw_suffix = true;
                idx = next_idx;
                continue;
            }

            break;
        }

        if self.is_assignment_operator_kind(self.peek_kind_at(idx)) {
            return true;
        }

        if !saw_suffix && matches!(self.peek_kind_at(idx), Some(TokenKind::Colon)) {
            idx += 1;
            idx = self.skip_type_tokens(idx);
            return self.is_assignment_operator_kind(self.peek_kind_at(idx));
        }

        false
    }

    fn is_destructure_assignment_stmt(&self) -> bool {
        let mut idx = self.index;
        if matches!(self.peek_kind_at(idx), Some(TokenKind::KwMut)) {
            idx += 1;
        }

        let mut paren_depth = 0usize;
        let mut bracket_depth = 0usize;
        let mut brace_depth = 0usize;
        let mut saw_comma = false;
        loop {
            match self.peek_kind_at(idx) {
                Some(TokenKind::LParen) => paren_depth += 1,
                Some(TokenKind::RParen) => {
                    if paren_depth == 0 {
                        return false;
                    }
                    paren_depth -= 1;
                }
                Some(TokenKind::LBracket) => bracket_depth += 1,
                Some(TokenKind::RBracket) => {
                    if bracket_depth == 0 {
                        return false;
                    }
                    bracket_depth -= 1;
                }
                Some(TokenKind::LBrace) => brace_depth += 1,
                Some(TokenKind::RBrace) => {
                    if brace_depth == 0 {
                        return false;
                    }
                    brace_depth -= 1;
                }
                Some(TokenKind::Colon)
                    if paren_depth == 0 && bracket_depth == 0 && brace_depth == 0 && !saw_comma =>
                {
                    return false;
                }
                Some(TokenKind::Comma) if bracket_depth == 0 && brace_depth == 0 => {
                    saw_comma = true;
                }
                kind if paren_depth == 0
                    && bracket_depth == 0
                    && brace_depth == 0
                    && self.is_assignment_operator_kind(kind) =>
                {
                    return saw_comma;
                }
                Some(TokenKind::Newline | TokenKind::Eof) | None => return false,
                Some(_) => {}
            }
            idx += 1;
        }
    }

    fn parse_binding_target_sequence(
        &mut self,
        allow_unparenthesized_singleton: bool,
    ) -> Result<BindingTarget> {
        let span = self.current_span();
        let first = self.parse_binding_target_atom()?;
        if self.eat_simple(&TokenKind::Comma).is_none() {
            return Ok(first);
        }

        let mut elements = vec![first];
        if self.binding_target_sequence_terminator() {
            if allow_unparenthesized_singleton {
                return Ok(BindingTarget::Tuple { elements, span });
            }
            return Err(parse_error(
                self.current_span(),
                "an unparenthesized destructuring target cannot end with a comma; write `(name,)` for a singleton tuple target",
            ));
        }

        loop {
            elements.push(self.parse_binding_target_atom()?);
            if self.eat_simple(&TokenKind::Comma).is_none() {
                break;
            }
            if self.binding_target_sequence_terminator() {
                return Err(parse_error(
                    self.current_span(),
                    "trailing commas are only allowed for singleton tuple targets",
                ));
            }
        }

        Ok(BindingTarget::Tuple { elements, span })
    }

    fn parse_binding_target_atom(&mut self) -> Result<BindingTarget> {
        let span = self.current_span();
        if self.eat_simple(&TokenKind::LParen).is_some() {
            if self.at_simple(&TokenKind::RParen) {
                return Err(parse_error(
                    span,
                    "empty tuple binding targets are not supported",
                ));
            }
            let target = self.parse_binding_target_sequence(true)?;
            self.expect_simple(TokenKind::RParen)?;
            return Ok(target);
        }

        let name = self.expect_identifier().map_err(|_| {
            parse_error(
                span,
                "binding targets must be names or recursively nested tuple targets",
            )
        })?;
        Ok(BindingTarget::Name { name, span })
    }

    fn binding_target_sequence_terminator(&self) -> bool {
        self.at_simple(&TokenKind::RParen)
            || self.at_simple(&TokenKind::KwIn)
            || self.is_assignment_operator_kind(Some(self.current_kind()))
    }

    fn reject_duplicate_binding_names(&self, target: &BindingTarget) -> Result<()> {
        fn visit(
            target: &BindingTarget,
            names: &mut std::collections::BTreeSet<String>,
        ) -> Result<()> {
            match target {
                BindingTarget::Name { name, span } => {
                    if !names.insert(name.clone()) {
                        return Err(parse_error(
                            *span,
                            format!("duplicate binding target `{name}`"),
                        ));
                    }
                }
                BindingTarget::Tuple { elements, .. } => {
                    for element in elements {
                        visit(element, names)?;
                    }
                }
            }
            Ok(())
        }

        visit(target, &mut std::collections::BTreeSet::new())
    }

    fn parse_assign_target(&mut self) -> Result<AssignTarget> {
        let span = self.current_span();
        let name = self.expect_identifier()?;
        let mut target = AssignTarget::Name(name);

        loop {
            if self.eat_simple(&TokenKind::Dot).is_some() {
                let field = self.expect_member_name()?;
                let object = assign_target_to_expr(target, span);
                target = AssignTarget::Member {
                    object: Box::new(object),
                    field,
                };
                continue;
            }

            if self.eat_simple(&TokenKind::LBracket).is_some() {
                let index = self.parse_expr()?;
                self.expect_simple(TokenKind::RBracket)?;
                let object = assign_target_to_expr(target, span);
                target = AssignTarget::Index {
                    object: Box::new(object),
                    index: Box::new(index),
                };
                continue;
            }

            break;
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
            TokenKind::DoubleSlashEqual => Ok(Some(BinaryOp::FloorDiv)),
            TokenKind::PercentEqual => Ok(Some(BinaryOp::Mod)),
            other => Err(parse_error(
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
                    | TokenKind::DoubleSlashEqual
                    | TokenKind::PercentEqual
            )
        )
    }

    fn skip_type_tokens(&self, mut idx: usize) -> usize {
        if matches!(self.peek_kind_at(idx), Some(TokenKind::KwIndirect)) {
            idx += 1;
        }

        if matches!(self.peek_kind_at(idx), Some(TokenKind::LParen)) {
            let mut depth = 0usize;
            loop {
                match self.peek_kind_at(idx) {
                    Some(TokenKind::LParen) => depth += 1,
                    Some(TokenKind::RParen) => {
                        depth = depth.saturating_sub(1);
                        idx += 1;
                        if depth == 0 {
                            break;
                        }
                        continue;
                    }
                    Some(TokenKind::Newline | TokenKind::Eof) | None => return idx,
                    Some(_) => {}
                }
                idx += 1;
            }
            if matches!(self.peek_kind_at(idx), Some(TokenKind::Question)) {
                idx += 1;
            }
            return idx;
        }

        if !self.is_contextual_identifier_at(idx) {
            return idx;
        }
        idx += 1;
        while matches!(self.peek_kind_at(idx), Some(TokenKind::Dot))
            && self.is_contextual_identifier_at(idx + 1)
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

    fn parse_optional_for_mode(&mut self) -> Result<Option<ReceiverKind>> {
        if self.at_simple(&TokenKind::KwBorrow) {
            return Err(self.retired_borrow_error("in", "in", "shared iteration"));
        }
        if self.eat_simple(&TokenKind::KwOwn).is_some() {
            return Ok(Some(ReceiverKind::Value));
        }
        if self.eat_simple(&TokenKind::KwMut).is_some() {
            return Ok(Some(ReceiverKind::BorrowMut));
        }
        Ok(None)
    }

    fn parse_match_capability(&mut self) -> Result<ReceiverKind> {
        if self.at_simple(&TokenKind::KwBorrow) {
            return Err(self.retired_borrow_error("match", "match", "shared access"));
        }
        if self.eat_simple(&TokenKind::KwOwn).is_some() {
            return Ok(ReceiverKind::Value);
        }
        if self.eat_simple(&TokenKind::KwMut).is_some() {
            return Ok(ReceiverKind::BorrowMut);
        }
        Ok(ReceiverKind::Borrow)
    }

    /// Builds the exact replacement diagnostic for a retired `borrow`
    /// spelling in statement position.
    ///
    /// `borrow` stays reserved for one announced compatibility window and is
    /// parsed only far enough to say what to write instead, so `prefix` is
    /// consumed context (`match`, `in`) and `bare` is what the shared form
    /// spells today.
    fn retired_borrow_error(&mut self, prefix: &str, bare: &str, shared: &str) -> Diagnostic {
        let span = self.current_span();
        self.bump();
        if self.eat_simple(&TokenKind::KwMut).is_some() {
            return parse_error(
                span,
                format!("`{prefix} borrow mut` was removed; write `{bare} mut`"),
            );
        }
        parse_error(
            span,
            format!("`{prefix} borrow` was removed; write `{bare}` for {shared}"),
        )
    }

    fn starts_specialization_suffix(&self, expr: &Expr) -> bool {
        let mut idx = self.index + 1;
        loop {
            let next = self.skip_type_tokens(idx);
            if next == idx {
                return false;
            }
            idx = next;
            if matches!(self.peek_kind_at(idx), Some(TokenKind::Comma)) {
                idx += 1;
                continue;
            }
            break;
        }

        if !matches!(self.peek_kind_at(idx), Some(TokenKind::RBracket)) {
            return false;
        }

        match self.peek_kind_at(idx + 1) {
            Some(TokenKind::LParen) => {
                matches!(expr.kind, ExprKind::Name(_) | ExprKind::Member { .. })
            }
            Some(TokenKind::Dot) => specialization_target_name(expr)
                .map(is_static_specialization_target_name)
                .unwrap_or(false),
            _ => false,
        }
    }

    fn skip_bracketed_tokens(&self, start_idx: usize) -> Option<usize> {
        let mut idx = start_idx;
        let mut depth = 0usize;
        loop {
            match self.peek_kind_at(idx) {
                Some(TokenKind::LBracket) => depth += 1,
                Some(TokenKind::RBracket) => {
                    depth = depth.saturating_sub(1);
                    idx += 1;
                    if depth == 0 {
                        return Some(idx);
                    }
                    continue;
                }
                Some(_) => {}
                None => return None,
            }
            idx += 1;
        }
    }

    fn at_copy_class_start(&self) -> bool {
        matches!(
            (self.current_kind(), self.peek_kind_at(self.index + 1)),
            (TokenKind::Identifier(name), Some(TokenKind::KwClass)) if name == "copy"
        )
    }

    fn parse_format_parts(&mut self, value: &str, span: Span) -> Result<Vec<FormatPart>> {
        let mut parts = Vec::new();
        let chars = value.char_indices().collect::<Vec<_>>();
        let mut index = 0usize;
        let mut literal = String::new();

        while index < chars.len() {
            let (offset, ch) = chars[index];
            if ch == '{' && matches!(chars.get(index + 1), Some((_, '{'))) {
                literal.push('{');
                index += 2;
                continue;
            }
            if ch == '}' && matches!(chars.get(index + 1), Some((_, '}'))) {
                literal.push('}');
                index += 2;
                continue;
            }
            if ch != '{' {
                literal.push(ch);
                index += 1;
                continue;
            }

            if !literal.is_empty() {
                parts.push(FormatPart::Literal(std::mem::take(&mut literal)));
            }

            let expr_start = offset + ch.len_utf8();
            index += 1;
            let mut expr_end = None;
            let mut brace_depth = 0usize;
            let mut string_quote = None;
            let mut escaped = false;
            while index < chars.len() {
                let (candidate_offset, candidate) = chars[index];
                if let Some(quote) = string_quote {
                    if escaped {
                        escaped = false;
                    } else if candidate == '\\' {
                        escaped = true;
                    } else if candidate == quote {
                        string_quote = None;
                    }
                    index += 1;
                    continue;
                }
                match candidate {
                    '"' | '\'' => string_quote = Some(candidate),
                    '{' => brace_depth += 1,
                    '}' if brace_depth == 0 => {
                        expr_end = Some(candidate_offset);
                        break;
                    }
                    '}' => brace_depth -= 1,
                    _ => {}
                }
                index += 1;
            }

            let Some(expr_end) = expr_end else {
                return Err(parse_error(span, "unterminated f-string interpolation"));
            };
            let raw_expr_text = &value[expr_start..expr_end];
            let leading_ws = raw_expr_text.len() - raw_expr_text.trim_start().len();
            let expr_text = raw_expr_text.trim();
            if expr_text.is_empty() {
                return Err(parse_error(span, "f-string interpolation cannot be empty"));
            }
            let mut expr =
                match parse_expression_with_recursion_depth(expr_text, self.recursion_depth) {
                    Ok(expr) => expr,
                    Err(error) => {
                        return Err(parse_error(
                            span,
                            format!("invalid f-string interpolation `{}`: {}", expr_text, error),
                        ))
                    }
                };
            let column_offset = span.column + expr_start + leading_ws + 1;
            offset_expr_span(&mut expr, span.line, column_offset);
            parts.push(FormatPart::Expr(expr));
            index += 1;
        }

        if !literal.is_empty() {
            parts.push(FormatPart::Literal(literal));
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
            Err(parse_error(
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

    fn expect_statement_terminator(&mut self) -> Result<()> {
        if self.eat_simple(&TokenKind::Newline).is_some()
            || self.at_simple(&TokenKind::Dedent)
            || matches!(
                self.tokens
                    .get(self.index.saturating_sub(1))
                    .map(|token| &token.kind),
                Some(TokenKind::Dedent)
            )
            || self.at_eof()
        {
            Ok(())
        } else {
            Err(self.error_here("expected Newline"))
        }
    }

    fn expect_match_expr_arm_terminator(&mut self) -> Result<()> {
        if self.eat_simple(&TokenKind::Newline).is_some()
            || self.at_simple(&TokenKind::Dedent)
            || matches!(
                self.tokens
                    .get(self.index.saturating_sub(1))
                    .map(|token| &token.kind),
                Some(TokenKind::Dedent)
            )
            || self.at_eof()
        {
            Ok(())
        } else {
            Err(self.error_here("expected Newline"))
        }
    }

    fn expect_identifier(&mut self) -> Result<String> {
        let token = self.bump();
        match token.kind {
            TokenKind::Identifier(name) => Ok(name),
            TokenKind::KwFrom => Ok("from".to_string()),
            _ => Err(parse_error(token.span, "expected identifier")),
        }
    }

    fn expect_member_name(&mut self) -> Result<String> {
        let token = self.bump();
        match token.kind {
            TokenKind::Identifier(name) => Ok(name),
            TokenKind::KwFrom => Ok("from".to_string()),
            other => Err(parse_error(
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

    fn at_from_import_start(&self) -> bool {
        if !self.at_keyword_from() {
            return false;
        }

        let mut index = self.index + 1;
        if !self.is_contextual_identifier_at(index) {
            return false;
        }
        index += 1;
        while matches!(self.peek_kind_at(index), Some(TokenKind::Dot))
            && self.is_contextual_identifier_at(index + 1)
        {
            index += 2;
        }
        matches!(self.peek_kind_at(index), Some(TokenKind::KwImport))
    }

    fn is_contextual_identifier_at(&self, index: usize) -> bool {
        matches!(
            self.peek_kind_at(index),
            Some(TokenKind::Identifier(_) | TokenKind::KwFrom)
        )
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

    fn at_match_expr_end(&self) -> bool {
        self.at_simple(&TokenKind::Dedent)
    }

    fn error_here(&self, message: impl Into<String>) -> Diagnostic {
        parse_error(self.current_span(), message)
    }
}

fn specialization_target_name(expr: &Expr) -> Option<&str> {
    match &expr.kind {
        ExprKind::Name(name) => Some(name.as_str()),
        ExprKind::Member { field, .. } => Some(field.as_str()),
        _ => None,
    }
}

fn is_static_specialization_target_name(name: &str) -> bool {
    name.chars()
        .next()
        .map(|ch| ch.is_ascii_uppercase())
        .unwrap_or(false)
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
        AssignTarget::Index { object, index } => Expr {
            kind: ExprKind::Index { object, index },
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
        ExprKind::Conditional {
            then_expr,
            condition,
            else_expr,
        } => {
            offset_expr_span(then_expr, line, column_offset);
            offset_expr_span(condition, line, column_offset);
            offset_expr_span(else_expr, line, column_offset);
        }
        ExprKind::Membership {
            value,
            container,
            operator_span,
            ..
        } => {
            operator_span.line = line;
            operator_span.column += column_offset;
            offset_expr_span(value, line, column_offset);
            offset_expr_span(container, line, column_offset);
        }
        ExprKind::CompareChain { first, links } => {
            offset_expr_span(first, line, column_offset);
            for link in links {
                link.op_span.line = line;
                link.op_span.column += column_offset;
                offset_expr_span(&mut link.operand, line, column_offset);
            }
        }
        ExprKind::Tuple(elements) | ExprKind::List(elements) => {
            for element in elements {
                offset_expr_span(element, line, column_offset);
            }
        }
        ExprKind::Set(elements) => {
            for element in elements {
                offset_expr_span(element, line, column_offset);
            }
        }
        ExprKind::Map(entries) => {
            for entry in entries {
                offset_expr_span(&mut entry.key, line, column_offset);
                offset_expr_span(&mut entry.value, line, column_offset);
            }
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
        ExprKind::Member { object, .. } => offset_expr_span(object, line, column_offset),
        ExprKind::Index { object, index } => {
            offset_expr_span(object, line, column_offset);
            offset_expr_span(index, line, column_offset);
        }
        ExprKind::Match {
            scrutinee, arms, ..
        } => {
            offset_expr_span(scrutinee, line, column_offset);
            for arm in arms {
                arm.span.line = line;
                arm.span.column += column_offset;
                offset_expr_span(&mut arm.value, line, column_offset);
            }
        }
        ExprKind::Name(_)
        | ExprKind::Int(_)
        | ExprKind::DurationNanos(_)
        | ExprKind::BuiltinOmitted
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
    match &mut type_ref.kind {
        TypeRefKind::Named { args, .. } | TypeRefKind::Tuple(args) => {
            for arg in args {
                offset_type_ref_span(arg, line, column_offset);
            }
        }
    }
}

#[cfg(test)]
#[path = "parser_tests.rs"]
mod tests;
