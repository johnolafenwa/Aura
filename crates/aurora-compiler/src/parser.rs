use crate::ast::{
    Argument, AssignStmt, BinaryOp, ClassDecl, Expr, ExprKind, ExprStmt, FieldDecl, FunctionDecl,
    Item, Module, Param, ReturnStmt, Stmt, TypeRef,
};
use crate::diag::{Diagnostic, Result, Span};
use crate::lexer::{lex, Token, TokenKind};

pub fn parse(source: &str) -> Result<Module> {
    let tokens = lex(source)?;
    Parser { tokens, index: 0 }.parse_module()
}

struct Parser {
    tokens: Vec<Token>,
    index: usize,
}

impl Parser {
    fn parse_module(&mut self) -> Result<Module> {
        let mut items = Vec::new();
        let mut top_level_stmts = Vec::new();
        self.skip_newlines();

        while !self.at_eof() {
            if self.at_keyword_class() || self.at_keyword_def() {
                items.push(self.parse_item()?);
            } else {
                top_level_stmts.push(self.parse_stmt()?);
            }
            self.skip_newlines();
        }

        Ok(Module {
            items,
            top_level_stmts,
        })
    }

    fn parse_item(&mut self) -> Result<Item> {
        if self.at_keyword_class() {
            Ok(Item::Class(self.parse_class()?))
        } else if self.at_keyword_def() {
            Ok(Item::Function(self.parse_function()?))
        } else {
            Err(self.error_here("expected `class` or `def`"))
        }
    }

    fn parse_class(&mut self) -> Result<ClassDecl> {
        let span = self.expect_keyword(TokenKind::KwClass)?.span;
        let name = self.expect_identifier()?;
        self.expect_simple(TokenKind::Colon)?;
        self.expect_newline()?;
        self.expect_simple(TokenKind::Indent)?;

        let mut fields = Vec::new();
        while !self.at_simple(&TokenKind::Dedent) && !self.at_eof() {
            if self.at_simple(&TokenKind::Newline) {
                self.bump();
                continue;
            }
            fields.push(self.parse_field()?);
        }

        self.expect_simple(TokenKind::Dedent)?;

        Ok(ClassDecl { name, fields, span })
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

    fn parse_function(&mut self) -> Result<FunctionDecl> {
        let span = self.expect_keyword(TokenKind::KwDef)?.span;
        let name = self.expect_identifier()?;
        self.expect_simple(TokenKind::LParen)?;
        let params = self.parse_params()?;
        self.expect_simple(TokenKind::RParen)?;
        let return_type = if self.eat_simple(&TokenKind::Arrow).is_some() {
            self.parse_type()?
        } else {
            TypeRef {
                name: "None".to_string(),
                args: Vec::new(),
                span,
            }
        };
        self.expect_simple(TokenKind::Colon)?;
        self.expect_newline()?;
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

        Ok(FunctionDecl {
            name,
            params,
            return_type,
            body,
            span,
        })
    }

    fn parse_params(&mut self) -> Result<Vec<Param>> {
        let mut params = Vec::new();

        if self.at_simple(&TokenKind::RParen) {
            return Ok(params);
        }

        loop {
            let span = self.current_span();
            let name = self.expect_identifier()?;
            self.expect_simple(TokenKind::Colon)?;
            let ty = self.parse_type()?;
            params.push(Param { name, ty, span });

            if self.eat_simple(&TokenKind::Comma).is_none() {
                break;
            }
        }

        Ok(params)
    }

    fn parse_stmt(&mut self) -> Result<Stmt> {
        if self.at_simple(&TokenKind::KwReturn) {
            self.parse_return_stmt()
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
        let name = self.expect_identifier()?;
        let annotation = if self.eat_simple(&TokenKind::Colon).is_some() {
            Some(self.parse_type()?)
        } else {
            None
        };
        self.expect_simple(TokenKind::Equal)?;
        let value = self.parse_expr()?;
        self.expect_newline()?;

        Ok(Stmt::Assign(AssignStmt {
            mutable,
            name,
            annotation,
            value,
            span,
        }))
    }

    fn parse_expr_stmt(&mut self) -> Result<Stmt> {
        let span = self.current_span();
        let expr = self.parse_expr()?;
        self.expect_newline()?;
        Ok(Stmt::Expr(ExprStmt { expr, span }))
    }

    fn parse_type(&mut self) -> Result<TypeRef> {
        let span = self.current_span();
        let name = self.expect_identifier()?;
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

        Ok(TypeRef { name, args, span })
    }

    fn parse_expr(&mut self) -> Result<Expr> {
        self.parse_additive()
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
        let mut expr = self.parse_postfix()?;

        loop {
            let op = if self.eat_simple(&TokenKind::Star).is_some() {
                Some(BinaryOp::Mul)
            } else if self.eat_simple(&TokenKind::Slash).is_some() {
                Some(BinaryOp::Div)
            } else {
                None
            };

            let Some(op) = op else { break };
            let right = self.parse_postfix()?;
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

    fn parse_postfix(&mut self) -> Result<Expr> {
        let mut expr = self.parse_primary()?;

        loop {
            if self.eat_simple(&TokenKind::Dot).is_some() {
                let field_span = self.current_span();
                let field = self.expect_identifier()?;
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

            break;
        }

        Ok(expr)
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
            TokenKind::FloatLiteral(value) => Ok(Expr {
                kind: ExprKind::Float(value),
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

        if matches!(self.peek_kind_at(idx), Some(TokenKind::Equal)) {
            return true;
        }

        if matches!(self.peek_kind_at(idx), Some(TokenKind::Colon)) {
            idx += 1;
            idx = self.skip_type_tokens(idx);
            return matches!(self.peek_kind_at(idx), Some(TokenKind::Equal));
        }

        false
    }

    fn skip_type_tokens(&self, mut idx: usize) -> usize {
        if !matches!(self.peek_kind_at(idx), Some(TokenKind::Identifier(_))) {
            return idx;
        }
        idx += 1;

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

        idx
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

    fn at_keyword_def(&self) -> bool {
        self.at_simple(&TokenKind::KwDef)
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
