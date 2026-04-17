use super::*;

fn parse_item_from(source: &str) -> Result<Item> {
    let tokens = lex(source)?;
    let mut parser = Parser::new(tokens);
    parser.parse_item()
}

fn parse_stmt_from(source: &str) -> Result<Stmt> {
    let tokens = lex(source)?;
    let mut parser = Parser::new(tokens);
    parser.parse_stmt()
}

fn parse_pattern_from(source: &str) -> Result<Pattern> {
    let tokens = lex(source)?;
    let mut parser = Parser::new(tokens);
    parser.parse_pattern()
}

#[test]
fn parse_expression_reports_trailing_tokens_and_primary_errors() {
    let trailing = parse_expression("1 2").expect_err("expected trailing-token parse failure");
    assert!(trailing
        .message
        .contains("unexpected trailing tokens after expression"));

    let unexpected = parse_expression(")").expect_err("expected unexpected-token failure");
    assert!(unexpected
        .message
        .contains("unexpected token in expression"));

    let borrowed = parse_expression("borrow value").expect_err("expected borrow-prefix failure");
    assert!(borrowed
        .message
        .contains("call arguments cannot start with `borrow`"));

    let bool_expr = parse_expression("true").expect("bool literal should parse");
    assert!(matches!(bool_expr.kind, ExprKind::Bool(true)));

    let string_expr = parse_expression("\"aurora\"").expect("string literal should parse");
    assert!(matches!(string_expr.kind, ExprKind::String(ref value) if value == "aurora"));
}

#[test]
fn parse_item_rejects_public_impl_and_non_item_tokens() {
    let public_impl =
        parse_item_from("public impl Show for Point:\n    pass\n").expect_err("public impl");
    assert!(public_impl
        .message
        .contains("`public` is not allowed on `impl` blocks"));

    let non_item = parse_item_from("return 1\n").expect_err("non-item token");
    assert!(non_item
        .message
        .contains("expected `class`, `enum`, `def`, `trait`, or `impl`"));
}

#[test]
fn parse_module_imports_and_generic_bounds_cover_success_paths() {
    let module = parse(
        [
            "import pkg.tools",
            "from pkg.user import Box, wrap",
            "",
            "public def map[T: Show + Clone](value: T):",
            "    return",
            "",
            "public def main():",
            "    return",
            "",
            "count = 1",
        ]
        .join("\n")
        .as_str(),
    )
    .expect("module should parse");

    assert_eq!(module.imports.len(), 2);
    assert_eq!(module.items.len(), 2);
    assert_eq!(module.top_level_stmts.len(), 1);

    let Item::Function(function_decl) = &module.items[0] else {
        panic!("expected first item to be a function");
    };
    assert_eq!(function_decl.type_params, vec!["T"]);
    assert_eq!(
        function_decl
            .type_param_bounds
            .get("T")
            .expect("function bound")
            .len(),
        2
    );
}

#[test]
fn parse_structural_items_tolerate_blank_lines_and_pass() {
    let class_item = parse_item_from(
        [
            "copy class Box[T]:",
            "",
            "    pass",
            "    value: T",
            "    public def read(self) -> T:",
            "        return self.value",
        ]
        .join("\n")
        .as_str(),
    )
    .expect("class should parse");
    match &class_item {
        Item::Class(class_decl) => {
            assert!(class_decl.copy);
            assert_eq!(class_decl.type_params, vec!["T"]);
            assert_eq!(class_decl.fields.len(), 1);
            assert_eq!(class_decl.methods.len(), 1);
        }
        other => panic!("expected class item, got {other:?}"),
    }

    let enum_item = parse_item_from(
        ["enum Flag[T]:", "", "    On", "    Off(T)"]
            .join("\n")
            .as_str(),
    )
    .expect("enum should parse");
    match &enum_item {
        Item::Enum(enum_decl) => {
            assert_eq!(enum_decl.type_params, vec!["T"]);
            assert_eq!(enum_decl.variants.len(), 2);
        }
        other => panic!("expected enum item, got {other:?}"),
    }

    let trait_item = parse_item_from(
        ["trait Show:", "", "    pass", "    def render(self)"]
            .join("\n")
            .as_str(),
    )
    .expect("trait should parse");
    match &trait_item {
        Item::Trait(trait_decl) => {
            assert_eq!(trait_decl.methods.len(), 1);
            assert_eq!(trait_decl.methods[0].return_type.name, "None");
        }
        other => panic!("expected trait item, got {other:?}"),
    }

    let impl_item = parse_item_from(
        [
            "impl[T] Show for Box[T]:",
            "",
            "    pass",
            "    def render(self) -> None:",
            "        pass",
        ]
        .join("\n")
        .as_str(),
    )
    .expect("impl should parse");
    match &impl_item {
        Item::Impl(impl_decl) => {
            assert_eq!(impl_decl.type_params, vec!["T"]);
            assert_eq!(impl_decl.methods.len(), 1);
        }
        other => panic!("expected impl item, got {other:?}"),
    }
}

#[test]
fn parse_params_and_receivers_cover_error_and_receiver_only_forms() {
    let class_item = parse_item_from(
        [
            "class Counter:",
            "    def read(self):",
            "        pass",
            "    def bump(borrow mut self, amount: int32 = 1):",
            "        pass",
        ]
        .join("\n")
        .as_str(),
    )
    .expect("class should parse");

    let Item::Class(class_decl) = class_item else {
        panic!("expected class");
    };
    assert_eq!(class_decl.methods.len(), 2);
    assert_eq!(class_decl.methods[0].receiver, Some(ReceiverKind::Value));
    assert_eq!(class_decl.methods[0].params.len(), 0);
    assert_eq!(
        class_decl.methods[1].receiver,
        Some(ReceiverKind::BorrowMut)
    );
    assert_eq!(class_decl.methods[1].params.len(), 1);

    let bad_param = parse_item_from(
        ["def read(borrow counter: Counter):", "    pass"]
            .join("\n")
            .as_str(),
    )
    .expect_err("prefix borrowed parameter should fail");
    assert!(bad_param
        .message
        .contains("ordinary borrowed parameters must be written as"));
}

#[test]
fn parse_statement_and_operator_variants_cover_remaining_forms() {
    let less_eq = parse_expression("a <= b").expect("<= expression should parse");
    assert!(matches!(
        less_eq.kind,
        ExprKind::Binary {
            op: BinaryOp::LessEq,
            ..
        }
    ));

    let greater_eq = parse_expression("a >= b").expect(">= expression should parse");
    assert!(matches!(
        greater_eq.kind,
        ExprKind::Binary {
            op: BinaryOp::GreaterEq,
            ..
        }
    ));

    for (source, expected) in [
        ("value -= 1\n", Some(BinaryOp::Sub)),
        ("value *= 1\n", Some(BinaryOp::Mul)),
        ("value /= 1\n", Some(BinaryOp::Div)),
        ("value %= 1\n", Some(BinaryOp::Mod)),
    ] {
        let stmt = parse_stmt_from(source).expect("compound assignment should parse");
        let Stmt::Assign(assign) = stmt else {
            panic!("expected assignment statement");
        };
        assert_eq!(assign.op, expected);
    }

    let tokens = lex("==\n").expect("tokens");
    let mut parser = Parser::new(tokens);
    let bad = parser
        .parse_assignment_operator()
        .expect_err("invalid assignment operator should fail");
    assert!(bad
        .message
        .contains("expected assignment operator, found EqEq"));
}

#[test]
fn parser_helpers_cover_brackets_keywords_and_member_names() {
    let tokens = lex("Vec[Map[String, int32]]?\n").expect("tokens");
    let parser = Parser::new(tokens);
    assert_eq!(parser.skip_type_tokens(0), 10);

    let tokens = lex("Vec[Map[String, int32]\n").expect("tokens");
    let parser = Parser::new(tokens);
    assert_eq!(parser.skip_type_tokens(0), 10);

    let tokens = lex("[value]\n").expect("tokens");
    let parser = Parser::new(tokens);
    assert_eq!(parser.skip_bracketed_tokens(0), Some(3));

    let tokens = lex("[value\n").expect("tokens");
    let parser = Parser::new(tokens);
    assert_eq!(parser.skip_bracketed_tokens(0), None);

    let tokens = lex("import name\n").expect("tokens");
    let mut parser = Parser::new(tokens);
    assert!(parser.at_keyword_import());
    assert!(!parser.at_keyword_from());
    assert!(parser.eat_simple(&TokenKind::KwImport).is_some());
    assert!(parser.eat_simple(&TokenKind::KwFrom).is_none());

    let tokens = lex("spawn\n").expect("tokens");
    let mut parser = Parser::new(tokens);
    assert_eq!(
        parser.expect_member_name().expect("spawn member name"),
        "spawn".to_string()
    );

    let tokens = lex("1\n").expect("tokens");
    let mut parser = Parser::new(tokens);
    let err = parser
        .expect_member_name()
        .expect_err("numeric member name should fail");
    assert!(err
        .message
        .contains("expected member name, found IntLiteral"));

    let tokens = lex("1\n").expect("tokens");
    let mut parser = Parser::new(tokens);
    let err = parser
        .expect_identifier()
        .expect_err("numeric identifier should fail");
    assert!(err.message.contains("expected identifier"));

    let custom = parser.error_here("custom parser error");
    assert_eq!(custom.message, "custom parser error");
}

#[test]
fn parse_expression_reports_recursion_limit_for_deep_nesting() {
    let depth = crate::limits::RECURSION_LIMIT + 32;
    let mut source = "(".repeat(depth);
    source.push('1');
    source.push_str(&")".repeat(depth));

    let error = parse_expression(&source).expect_err("deeply nested expressions should fail");
    assert!(error.message.contains("recursion limit"));
}

#[test]
fn parse_expression_accepts_reasonably_deep_generated_expressions() {
    let depth = 48usize;
    let mut source = "(".repeat(depth);
    source.push('1');
    source.push_str(&")".repeat(depth));

    let expr = parse_expression(&source).expect("generated nested expressions should parse");
    assert!(matches!(expr.kind, ExprKind::Group(_)));
}

#[test]
fn parser_reports_recursion_limits_for_nested_statements_types_and_patterns() {
    let depth = crate::limits::RECURSION_LIMIT + 32;

    let mut statement_source = String::from("def main() -> None:\n");
    for level in 0..depth {
        statement_source.push_str(&"    ".repeat(level + 1));
        statement_source.push_str("if true:\n");
    }
    statement_source.push_str(&"    ".repeat(depth + 1));
    statement_source.push_str("pass\n");
    let statement_error =
        parse(&statement_source).expect_err("deeply nested statements should fail");
    assert!(statement_error.message.contains("nesting exceeds"));

    let mut type_source = String::from("def main(value: ");
    type_source.push_str(&"Vec[".repeat(depth));
    type_source.push_str("int32");
    type_source.push_str(&"]".repeat(depth));
    type_source.push_str(") -> None:\n    pass\n");
    let type_error = parse_item_from(&type_source).expect_err("deeply nested types should fail");
    assert!(type_error.message.contains("nesting exceeds"));

    let mut pattern_source = "Wrap(".repeat(depth);
    pattern_source.push('_');
    pattern_source.push_str(&")".repeat(depth));
    let pattern_error =
        parse_pattern_from(&pattern_source).expect_err("deeply nested patterns should fail");
    assert!(pattern_error.message.contains("nesting exceeds"));
}

#[test]
fn parser_helpers_cover_specialization_and_format_parts() {
    let named = parse_expression("Value[int32](1)").expect("specialized call should parse");
    let ExprKind::Call { callee, .. } = named.kind else {
        panic!("expected call");
    };
    assert!(matches!(&callee.kind, ExprKind::Specialize { .. }));

    let indexed = parse_expression("values[idx]").expect("index expression");
    let tokens = lex("[Type].field\n").expect("tokens");
    let parser = Parser::new(tokens);
    assert!(!parser.starts_specialization_suffix(&indexed));

    let mut parser = Parser::new(vec![Token {
        kind: TokenKind::Eof,
        span: Span::new(1, 1),
    }]);
    let parts = parser
        .parse_format_parts("hello {config[\"name\"]} tail", Span::new(1, 1))
        .expect("format parts");
    assert_eq!(parts.len(), 3);

    let escaped = parser
        .parse_format_parts("{call(\"x\\\"y\")}", Span::new(1, 1))
        .expect("escaped interpolation");
    assert_eq!(escaped.len(), 1);

    let empty = parser
        .parse_format_parts("value {}", Span::new(1, 1))
        .expect_err("empty interpolation should fail");
    assert!(empty
        .message
        .contains("f-string interpolation cannot be empty"));

    let invalid = parser
        .parse_format_parts("value {1 + }", Span::new(1, 1))
        .expect_err("invalid interpolation should fail");
    assert!(invalid.message.contains("invalid f-string interpolation"));

    let unterminated = parser
        .parse_format_parts("value {name", Span::new(1, 1))
        .expect_err("unterminated interpolation should fail");
    assert!(unterminated
        .message
        .contains("unterminated f-string interpolation"));
}

#[test]
fn parse_format_parts_reuses_the_current_recursion_budget() {
    let mut parser = Parser::new(lex("value\n").expect("tokenization should succeed"));
    parser.recursion_depth = crate::limits::RECURSION_LIMIT;
    let error = parser
        .parse_format_parts("value {1}", Span::new(1, 1))
        .expect_err("f-string interpolation should share the parser recursion budget");
    assert!(error.message.contains("expression nesting"));
}

#[test]
fn parser_helper_functions_cover_assignment_targets_and_span_offsets() {
    let span = Span::new(1, 1);
    assert_eq!(
        specialization_target_name(&Expr {
            kind: ExprKind::Name("Thing".to_string()),
            span,
        }),
        Some("Thing")
    );
    assert_eq!(
        specialization_target_name(&Expr {
            kind: ExprKind::Member {
                object: Box::new(Expr {
                    kind: ExprKind::Name("pkg".to_string()),
                    span,
                }),
                field: "Thing".to_string(),
            },
            span,
        }),
        Some("Thing")
    );
    assert_eq!(
        specialization_target_name(&Expr {
            kind: ExprKind::Bool(true),
            span,
        }),
        None
    );
    assert!(is_static_specialization_target_name("Thing"));
    assert!(!is_static_specialization_target_name("thing"));

    let member_target = assign_target_to_expr(
        AssignTarget::Member {
            object: Box::new(Expr {
                kind: ExprKind::Name("point".to_string()),
                span,
            }),
            field: "x".to_string(),
        },
        span,
    );
    assert!(matches!(member_target.kind, ExprKind::Member { .. }));

    let index_target = assign_target_to_expr(
        AssignTarget::Index {
            object: Box::new(Expr {
                kind: ExprKind::Name("values".to_string()),
                span,
            }),
            index: Box::new(Expr {
                kind: ExprKind::Int(0),
                span,
            }),
        },
        span,
    );
    assert!(matches!(index_target.kind, ExprKind::Index { .. }));

    let mut expr = Expr {
            kind: ExprKind::FString(vec![
                FormatPart::Literal("prefix".to_string()),
                FormatPart::Expr(Expr {
                    kind: ExprKind::Call {
                        callee: Box::new(Expr {
                            kind: ExprKind::Member {
                                object: Box::new(Expr {
                                    kind: ExprKind::Spawn {
                                        detached: false,
                                        value: Box::new(Expr {
                                            kind: ExprKind::Name("worker".to_string()),
                                            span,
                                        }),
                                    },
                                    span,
                                }),
                                field: "join".to_string(),
                            },
                            span,
                        }),
                        args: vec![Argument {
                            name: Some("value".to_string()),
                            value: Expr {
                                kind: ExprKind::Map(vec![MapEntryExpr {
                                    key: Expr {
                                        kind: ExprKind::String("k".to_string()),
                                        span,
                                    },
                                    value: Expr {
                                        kind: ExprKind::List(vec![
                                            Expr {
                                                kind: ExprKind::Set(vec![Expr {
                                                    kind: ExprKind::Bool(true),
                                                    span,
                                                }]),
                                                span,
                                            },
                                            Expr {
                                                kind: ExprKind::Group(Box::new(Expr {
                                                    kind: ExprKind::Try(Box::new(Expr {
                                                        kind: ExprKind::Cast {
                                                            expr: Box::new(Expr {
                                                                kind: ExprKind::Unary {
                                                                    op: UnaryOp::Neg,
                                                                    expr: Box::new(Expr {
                                                                        kind: ExprKind::Binary {
                                                                            op: BinaryOp::Add,
                                                                            left: Box::new(Expr {
                                                                                kind: ExprKind::Float(
                                                                                    1.0,
                                                                                ),
                                                                                span,
                                                                            }),
                                                                            right: Box::new(Expr {
                                                                                kind: ExprKind::DurationMillis(
                                                                                    5,
                                                                                ),
                                                                                span,
                                                                            }),
                                                                        },
                                                                        span,
                                                                    }),
                                                                },
                                                                span,
                                                            }),
                                                            ty: TypeRef {
                                                                name: "float64".to_string(),
                                                                args: vec![TypeRef {
                                                                    name: "Vec".to_string(),
                                                                    args: vec![TypeRef {
                                                                        name: "int32".to_string(),
                                                                        args: vec![],
                                                                        indirect: false,
                                                                        span,
                                                                    }],
                                                                    indirect: false,
                                                                    span,
                                                                }],
                                                                indirect: false,
                                                                span,
                                                            },
                                                        },
                                                        span,
                                                    })),
                                                    span,
                                                })),
                                                span,
                                            },
                                        ]),
                                        span,
                                    },
                                }]),
                                span,
                            },
                            span,
                        }],
                    },
                    span,
                }),
            ]),
            span,
        };

    offset_expr_span(&mut expr, 7, 3);
    assert_eq!(expr.span.line, 7);
    assert_eq!(expr.span.column, 4);

    let mut ty = TypeRef {
        name: "Map".to_string(),
        args: vec![TypeRef {
            name: "Vec".to_string(),
            args: vec![TypeRef {
                name: "int32".to_string(),
                args: vec![],
                indirect: false,
                span,
            }],
            indirect: false,
            span,
        }],
        indirect: false,
        span,
    };
    offset_type_ref_span(&mut ty, 9, 5);
    assert_eq!(ty.span.line, 9);
    assert_eq!(ty.span.column, 6);
    assert_eq!(ty.args[0].span.line, 9);
    assert_eq!(ty.args[0].args[0].span.column, 6);
}

#[test]
fn parse_control_flow_patterns_and_helper_errors_cover_more_branches() {
    let return_stmt = parse_stmt_from("return\n").expect("bare return should parse");
    assert!(matches!(
        return_stmt,
        Stmt::Return(ReturnStmt { value: None, .. })
    ));

    let assign_stmt =
        parse_stmt_from("mut count: int32 = 1\n").expect("annotated assignment should parse");
    assert!(matches!(
        assign_stmt,
        Stmt::Assign(AssignStmt {
            mutable: true,
            annotation: Some(_),
            ..
        })
    ));

    let match_stmt = parse_stmt_from(
        [
            "match borrow mut value:",
            "    case true:",
            "        pass",
            "    case \"ok\":",
            "        pass",
            "    case -1:",
            "        pass",
            "    case Status.Ready(item):",
            "        pass",
            "    case _:",
            "        pass",
        ]
        .join("\n")
        .as_str(),
    )
    .expect("match with literal and variant patterns should parse");
    let Stmt::Match(match_stmt) = match_stmt else {
        panic!("expected match statement");
    };
    assert_eq!(match_stmt.borrow_mode, Some(ReceiverKind::BorrowMut));
    assert!(matches!(
        match_stmt.arms[0].pattern,
        Pattern::Literal(LiteralPattern {
            kind: LiteralPatternKind::Bool(true),
            ..
        })
    ));
    assert!(matches!(
        match_stmt.arms[1].pattern,
        Pattern::Literal(LiteralPattern {
            kind: LiteralPatternKind::String(ref value),
            ..
        }) if value == "ok"
    ));
    assert!(matches!(
        match_stmt.arms[2].pattern,
        Pattern::Literal(LiteralPattern {
            kind: LiteralPatternKind::Int(_),
            ..
        })
    ));
    assert!(matches!(
        &match_stmt.arms[3].pattern,
        Pattern::Variant(VariantPattern {
            enum_name: Some(ref name),
            variant_name: ref variant,
            subpatterns,
            ..
        }) if name == "Status"
            && variant == "Ready"
            && matches!(subpatterns.as_slice(), [Pattern::Binding(binding)] if binding.name == "item")
    ));
    assert!(matches!(match_stmt.arms[4].pattern, Pattern::Wildcard(_)));

    let for_stmt = parse_stmt_from("for item in borrow mut values:\n    pass\n")
        .expect("borrow-mut for should parse");
    assert!(matches!(
        for_stmt,
        Stmt::For(ForStmt {
            borrow_mode: Some(ReceiverKind::BorrowMut),
            ..
        })
    ));

    let with_binding = parse_stmt_from("with handle = open():\n    pass\n")
        .expect("with binding form should parse");
    assert!(matches!(
        with_binding,
        Stmt::With(WithStmt { ref binding, .. }) if binding == "handle"
    ));

    let select_stmt = parse_stmt_from(
        ["select:", "    case ready = jobs.recv():", "        pass"]
            .join("\n")
            .as_str(),
    )
    .expect("select should parse");
    let Stmt::Select(select_stmt) = select_stmt else {
        panic!("expected select statement");
    };
    assert_eq!(select_stmt.arms.len(), 1);
    assert_eq!(select_stmt.arms[0].binding.as_deref(), Some("ready"));

    let while_stmt = parse_stmt_from("while true:\n    break\n").expect("while should parse");
    assert!(matches!(while_stmt, Stmt::While(_)));
    let continue_stmt = parse_stmt_from("continue\n").expect("continue should parse");
    assert!(matches!(continue_stmt, Stmt::Continue(_)));

    let bad_pattern = parse_stmt_from("match value:\n    case -name:\n        pass\n")
        .expect_err("invalid negative match pattern should fail");
    assert!(bad_pattern
        .message
        .contains("boolean/string/integer/float literals"));

    let bad_member = parse_expression("value.1").expect_err("numeric member should fail");
    assert!(bad_member
        .message
        .contains("expected member name, found IntLiteral"));
}

#[test]
fn parser_helper_functions_cover_format_offsets_and_specialization_checks() {
    let tokens = lex("Value[int32](1)\n").expect("tokenization should succeed");
    let mut parser = Parser::new(tokens);
    parser.index = 1;
    let expr = Expr {
        kind: ExprKind::Name("Value".to_string()),
        span: Span::new(1, 1),
    };
    assert!(parser.starts_specialization_suffix(&expr));
    assert_eq!(parser.skip_bracketed_tokens(1), Some(4));

    let tokens = lex("values[idx].clone()\n").expect("tokenization should succeed");
    let mut parser = Parser::new(tokens);
    parser.index = 1;
    let expr = Expr {
        kind: ExprKind::Name("values".to_string()),
        span: Span::new(1, 1),
    };
    assert!(!parser.starts_specialization_suffix(&expr));
    assert_eq!(parser.skip_bracketed_tokens(1), Some(4));

    let mut parser = Parser::new(lex("value\n").expect("tokenization should succeed"));
    let format_error = parser
        .parse_format_parts("{ }", Span::new(3, 4))
        .expect_err("empty interpolation should fail");
    assert!(format_error
        .message
        .contains("f-string interpolation cannot be empty"));

    let mut parser = Parser::new(lex("value\n").expect("tokenization should succeed"));
    let unterminated = parser
        .parse_format_parts("{value", Span::new(3, 4))
        .expect_err("unterminated interpolation should fail");
    assert!(unterminated
        .message
        .contains("unterminated f-string interpolation"));
}

#[test]
fn offset_helpers_cover_fstring_expression_parts() {
    let mut expr =
        parse_expression("f\"value={items[0]}\"").expect("f-string expression should parse");
    offset_expr_span(&mut expr, 7, 3);

    let ExprKind::FString(parts) = &expr.kind else {
        panic!("expected f-string expression");
    };
    let FormatPart::Expr(inner) = &parts[1] else {
        panic!("expected interpolation part");
    };
    assert_eq!(inner.span.line, 7);
    assert!(inner.span.column >= 3);
}

#[test]
fn parser_covers_blank_lines_empty_literals_and_specialization_offsets() {
    let class_decl = parse_item_from("class Box:\n\n    value: int32\n")
        .expect("class with blank lines should parse");
    assert_eq!(class_decl.name(), "Box");

    let enum_decl =
        parse_item_from("enum Flag:\n\n    On\n").expect("enum with blank lines should parse");
    assert_eq!(enum_decl.name(), "Flag");

    let trait_decl = parse_item_from("trait Named:\n\n    def name(borrow self) -> String\n")
        .expect("trait with blank lines should parse");
    assert_eq!(trait_decl.name(), "Named");

    let impl_decl = parse_item_from(
        "impl Named for Box:\n\n    def name(borrow self) -> String:\n        return \"box\"\n",
    )
    .expect("impl with blank lines should parse");
    assert_eq!(impl_decl.name(), "Named");

    let match_stmt = parse_stmt_from("match value:\n\n    case 1:\n        pass\n")
        .expect("match with blank lines should parse");
    assert!(matches!(match_stmt, Stmt::Match(_)));

    let select_stmt = parse_stmt_from("select:\n\n    case after(1ms):\n        pass\n")
        .expect("select with blank lines should parse");
    assert!(matches!(select_stmt, Stmt::Select(_)));

    let empty_list = parse_expression("[]").expect("empty list should parse");
    assert!(matches!(empty_list.kind, ExprKind::List(ref items) if items.is_empty()));

    let empty_map = parse_expression("{}").expect("empty map should parse");
    assert!(matches!(empty_map.kind, ExprKind::Map(ref items) if items.is_empty()));

    let empty_set = parse_expression("Set{}").expect("empty set should parse");
    assert!(matches!(empty_set.kind, ExprKind::Set(ref items) if items.is_empty()));

    let tokens = lex("Value[int32].make()\n").expect("tokenization should succeed");
    let mut parser = Parser::new(tokens);
    parser.index = 1;
    let expr = Expr {
        kind: ExprKind::Name("Value".to_string()),
        span: Span::new(1, 1),
    };
    assert!(parser.starts_specialization_suffix(&expr));
    assert_eq!(parser.skip_bracketed_tokens(1), Some(4));

    let tokens = lex("Value[int32\n").expect("tokenization should succeed");
    let mut parser = Parser::new(tokens);
    parser.index = 1;
    assert_eq!(parser.skip_bracketed_tokens(1), None);
    assert!(!parser.starts_specialization_suffix(&expr));

    let invalid_pattern = parse_stmt_from("match value:\n    case +name:\n        pass\n")
        .expect_err("invalid match pattern should fail");
    assert!(invalid_pattern
        .message
        .contains("boolean/string/integer/float literals"));

    let mut specialize =
        parse_expression("Value[int32](1)").expect("specialization expression should parse");
    offset_expr_span(&mut specialize, 9, 4);
    let ExprKind::Call { callee, .. } = &specialize.kind else {
        panic!("expected call expression");
    };
    let ExprKind::Specialize {
        expr: inner,
        type_args,
    } = &callee.kind
    else {
        panic!("expected specialization callee");
    };
    assert_eq!(inner.span.line, 9);
    assert_eq!(type_args[0].span.line, 9);
}

#[test]
fn parser_additional_blank_line_and_pattern_overflow_edges_are_covered() {
    let class_item = parse_item_from("class Box:\n\n\n    pass\n")
        .expect("class with repeated blank lines should parse");
    assert_eq!(class_item.name(), "Box");

    let enum_item = parse_item_from("enum Flag:\n\n\n    On\n")
        .expect("enum with repeated blank lines should parse");
    assert_eq!(enum_item.name(), "Flag");

    let while_stmt = parse_stmt_from("while true:\n\n    pass\n")
        .expect("while body should tolerate leading blank lines");
    assert!(matches!(while_stmt, Stmt::While(_)));

    let expr_stmt = parse_stmt_from("value\n").expect("plain expression statements should parse");
    assert!(matches!(expr_stmt, Stmt::Expr(_)));

    let overflow = parse_stmt_from(
        "match value:\n    case -170141183460469231731687303715884105729:\n        pass\n",
    )
    .expect_err("negative pattern literals outside the signed range should fail");
    assert!(overflow.message.contains("outside the supported range"));
}

#[test]
fn parser_additional_trait_impl_block_and_helper_edges_are_covered() {
    let class_item = parse_item_from(
        ["class Box:", "    value: int32", "", "    pass"]
            .join("\n")
            .as_str(),
    )
    .expect("class with blank lines between members should parse");
    assert_eq!(class_item.name(), "Box");

    let enum_item = parse_item_from(["enum Flag:", "    On", "", "    Off"].join("\n").as_str())
        .expect("enum with blank lines between variants should parse");
    assert_eq!(enum_item.name(), "Flag");

    let trait_item = parse_item_from(
        ["trait Mapper[T]:", "    def map(value: T)", "", "    pass"]
            .join("\n")
            .as_str(),
    )
    .expect("trait with repeated blank lines should parse");
    match trait_item {
        Item::Trait(trait_decl) => {
            assert_eq!(trait_decl.type_params, vec!["T"]);
            assert_eq!(trait_decl.methods.len(), 1);
        }
        other => panic!("expected trait item, found {other:?}"),
    }

    let impl_item = parse_item_from(
        [
            "impl Mapper[T] for Box[T]:",
            "    def map(self):",
            "        pass",
            "",
            "    pass",
        ]
        .join("\n")
        .as_str(),
    )
    .expect("impl with trait args and repeated blank lines should parse");
    match impl_item {
        Item::Impl(impl_decl) => {
            assert_eq!(impl_decl.trait_name, "Mapper");
            assert_eq!(impl_decl.trait_args.len(), 1);
            assert_eq!(impl_decl.methods.len(), 1);
        }
        other => panic!("expected impl item, found {other:?}"),
    }

    let match_stmt = parse_stmt_from(
        [
            "match borrow value:",
            "    case _:",
            "        pass",
            "",
            "    case _:",
            "        pass",
        ]
        .join("\n")
        .as_str(),
    )
    .expect("match with blank lines should parse");
    let Stmt::Match(match_stmt) = match_stmt else {
        panic!("expected match statement");
    };
    assert_eq!(match_stmt.borrow_mode, Some(ReceiverKind::Borrow));
    assert_eq!(match_stmt.arms.len(), 2);

    let for_stmt = parse_stmt_from("for item in borrow values:\n    pass\n")
        .expect("borrowed for-loop should parse");
    let Stmt::For(for_stmt) = for_stmt else {
        panic!("expected for statement");
    };
    assert_eq!(for_stmt.borrow_mode, Some(ReceiverKind::Borrow));

    let with_stmt =
        parse_stmt_from("with resource as handle:\n    pass\n").expect("with/as form should parse");
    let Stmt::With(with_stmt) = with_stmt else {
        panic!("expected with statement");
    };
    assert_eq!(with_stmt.binding, "handle");

    let select_stmt = parse_stmt_from(
        [
            "select:",
            "    case after(1ms):",
            "        pass",
            "",
            "    case after(2ms):",
            "        pass",
        ]
        .join("\n")
        .as_str(),
    )
    .expect("select with blank lines should parse");
    let Stmt::Select(select_stmt) = select_stmt else {
        panic!("expected select statement");
    };
    assert_eq!(select_stmt.arms.len(), 2);

    let whitespace_heavy_item = parse_item_from(
        [
            "class Holder:",
            "    value: int32",
            "    ",
            "    def read(self) -> int32:",
            "        return self.value",
        ]
        .join("\n")
        .as_str(),
    )
    .expect("class with whitespace-only blank lines should parse");
    let Item::Class(class_decl) = whitespace_heavy_item else {
        panic!("expected class item");
    };
    assert_eq!(class_decl.methods.len(), 1);

    let whitespace_heavy_enum = parse_item_from(
        ["enum State:", "    Ready", "    ", "    Waiting"]
            .join("\n")
            .as_str(),
    )
    .expect("enum with whitespace-only blank lines should parse");
    let Item::Enum(enum_decl) = whitespace_heavy_enum else {
        panic!("expected enum item");
    };
    assert_eq!(enum_decl.variants.len(), 2);

    let whitespace_heavy_trait = parse_item_from(
        [
            "trait Show:",
            "    def show(self) -> String",
            "    ",
            "    pass",
        ]
        .join("\n")
        .as_str(),
    )
    .expect("trait with whitespace-only blank lines should parse");
    let Item::Trait(trait_decl) = whitespace_heavy_trait else {
        panic!("expected trait item");
    };
    assert_eq!(trait_decl.methods.len(), 1);

    let whitespace_heavy_impl = parse_item_from(
        [
            "impl Show for Holder:",
            "    def show(self) -> String:",
            "        return \"ok\"",
            "    ",
            "    pass",
        ]
        .join("\n")
        .as_str(),
    )
    .expect("impl with whitespace-only blank lines should parse");
    let Item::Impl(impl_decl) = whitespace_heavy_impl else {
        panic!("expected impl item");
    };
    assert_eq!(impl_decl.methods.len(), 1);

    let if_stmt = parse_stmt_from(["if true:", "    pass", "", "    pass"].join("\n").as_str())
        .expect("if block with blank lines should parse");
    assert!(matches!(if_stmt, Stmt::If(_)));

    let match_with_whitespace_only_gap = parse_stmt_from(
        [
            "match borrow value:",
            "    case _:",
            "        pass",
            "    ",
            "    case _:",
            "        pass",
        ]
        .join("\n")
        .as_str(),
    )
    .expect("match should tolerate whitespace-only blank lines between arms");
    assert!(matches!(match_with_whitespace_only_gap, Stmt::Match(_)));

    let select_with_whitespace_only_gap = parse_stmt_from(
        [
            "select:",
            "    case after(1ms):",
            "        pass",
            "    ",
            "    case after(2ms):",
            "        pass",
        ]
        .join("\n")
        .as_str(),
    )
    .expect("select should tolerate whitespace-only blank lines between arms");
    assert!(matches!(select_with_whitespace_only_gap, Stmt::Select(_)));

    let with_binding_equals_stmt = parse_stmt_from("with handle = open():\n    pass\n")
        .expect("with binding=value form should parse");
    let Stmt::With(with_binding_equals_stmt) = with_binding_equals_stmt else {
        panic!("expected with statement");
    };
    assert_eq!(with_binding_equals_stmt.binding, "handle");

    let whitespace_only_block_stmt =
        parse_stmt_from(["if true:", "    ", "    pass"].join("\n").as_str())
            .expect("blocks should tolerate whitespace-only blank lines");
    assert!(matches!(whitespace_only_block_stmt, Stmt::If(_)));

    let tokens = lex("value[\n").expect("tokens");
    let parser = Parser::new(tokens);
    assert!(!parser.is_assignment_stmt());

    let tokens = lex("indirect Value\n").expect("tokens");
    let parser = Parser::new(tokens);
    assert_eq!(parser.skip_type_tokens(0), 2);

    let tokens = lex("[[value]]\n").expect("tokens");
    let parser = Parser::new(tokens);
    assert_eq!(parser.skip_bracketed_tokens(0), Some(5));

    let fstring = parse_expression("f\"{Set{1}}\"")
        .expect("f-string interpolation with nested set braces should parse");
    assert!(matches!(fstring.kind, ExprKind::FString(_)));

    let trait_with_default_method = parse_item_from(
        [
            "trait Named:",
            "    def name(borrow self) -> String",
            "    def label(borrow self) -> String:",
            "        return self.name()",
        ]
        .join("\n")
        .as_str(),
    )
    .expect("trait default method should parse");
    let Item::Trait(trait_decl) = trait_with_default_method else {
        panic!("expected trait item");
    };
    assert_eq!(trait_decl.methods.len(), 2);
    assert!(trait_decl.methods[0].body.is_empty());
    assert!(matches!(
        trait_decl.methods[1].body.as_slice(),
        [Stmt::Return(ReturnStmt { value: Some(_), .. })]
    ));
}
