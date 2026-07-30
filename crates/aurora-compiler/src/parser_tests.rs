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

fn named_type_ref(ty: &TypeRef) -> Option<(&str, &[TypeRef])> {
    match &ty.kind {
        TypeRefKind::Named { name, args } => Some((name, args)),
        TypeRefKind::Tuple(_) => None,
        TypeRefKind::Function { .. } => None,
    }
}

fn tokens_with_newline_after_first_indent(source: &str) -> Vec<Token> {
    let mut tokens = lex(source).expect("source should lex");
    let indent = tokens
        .iter()
        .position(|token| token.kind == TokenKind::Indent)
        .expect("source should contain an indent");
    let span = tokens[indent].span;
    tokens.insert(
        indent + 1,
        Token {
            kind: TokenKind::Newline,
            span,
        },
    );
    tokens
}

#[test]
fn d3_parser_accepts_int_alias_as_numeric_cast_target() {
    let cast = parse_expression("value as int").expect("`int` should be a numeric cast target");
    assert!(matches!(
        cast.kind,
        ExprKind::Cast { ty, .. }
            if matches!(named_type_ref(&ty), Some(("int", args)) if args.is_empty())
    ));
}

#[test]
fn tuple_literals_are_parenthesized_and_distinct_from_groups() {
    let pair = parse_expression("(1, \"one\")").expect("pair tuple should parse");
    assert!(matches!(
        pair.kind,
        ExprKind::Tuple(ref elements)
            if elements.len() == 2
                && matches!(elements[0].kind, ExprKind::Int(1))
                && matches!(elements[1].kind, ExprKind::String(ref value) if value == "one")
    ));

    let singleton = parse_expression("(1,)").expect("singleton tuple should parse");
    assert!(matches!(
        singleton.kind,
        ExprKind::Tuple(ref elements)
            if elements.len() == 1 && matches!(elements[0].kind, ExprKind::Int(1))
    ));

    let group = parse_expression("(1)").expect("group should remain valid");
    assert!(matches!(
        group.kind,
        ExprKind::Group(ref inner) if matches!(inner.kind, ExprKind::Int(1))
    ));

    let nested = parse_expression("(\n    (1, \"one\"),\n    (2,)\n)")
        .expect("nested multiline tuples should parse");
    assert!(matches!(
        nested.kind,
        ExprKind::Tuple(ref elements)
            if elements.len() == 2
                && matches!(elements[0].kind, ExprKind::Tuple(ref inner) if inner.len() == 2)
                && matches!(elements[1].kind, ExprKind::Tuple(ref inner) if inner.len() == 1)
    ));

    let unparenthesized =
        parse_expression("1, 2").expect_err("tuple expressions require parentheses");
    assert!(unparenthesized
        .message
        .contains("unexpected trailing tokens after expression"));
}

#[test]
fn tuple_types_are_structural_and_support_singletons_nesting_and_option() {
    let item = parse_item_from(
        "def rotate(value: (int32, String), only: (int64,)) -> ((String, int32), bool)?:\n    return None\n",
    )
    .expect("tuple parameter and return types should parse");
    let Item::Function(function) = item else {
        panic!("expected function");
    };

    let TypeRefKind::Tuple(value_elements) = &function.params[0].ty.kind else {
        panic!("expected pair tuple type");
    };
    assert_eq!(value_elements.len(), 2);
    assert!(matches!(
        named_type_ref(&value_elements[0]),
        Some(("int32", args)) if args.is_empty()
    ));
    assert!(matches!(
        named_type_ref(&value_elements[1]),
        Some(("String", args)) if args.is_empty()
    ));

    assert!(matches!(
        function.params[1].ty.kind,
        TypeRefKind::Tuple(ref elements) if elements.len() == 1
    ));
    let TypeRefKind::Named {
        name,
        args: option_args,
    } = &function.return_type.kind
    else {
        panic!("optional tuple return should lower to an Option type reference");
    };
    assert_eq!(name, "Option");
    assert!(matches!(
        option_args.as_slice(),
        [TypeRef {
            kind: TypeRefKind::Tuple(elements),
            ..
        }] if elements.len() == 2
            && matches!(elements[0].kind, TypeRefKind::Tuple(ref nested) if nested.len() == 2)
    ));

    let indirect = parse_item_from("def invalid(value: indirect (int32,)):\n    pass\n")
        .expect_err("indirect tuple types must not silently discard the modifier");
    assert!(indirect
        .message
        .contains("`indirect` applies only to named types"));
}

#[test]
fn function_types_use_declaration_shaped_syntax_and_nest_structurally() {
    let item = parse_item_from(
        "def apply(callback: def(int32, mut String, own Vec[int32]) -> bool, factory: def() -> def(own String) -> int64) -> def((int32, String)) -> None:\n    pass\n",
    )
    .expect("function type annotations should parse without changing function declarations");
    let Item::Function(function) = item else {
        panic!("expected named function declaration");
    };
    assert_eq!(function.name, "apply");

    let TypeRefKind::Function {
        params,
        return_type,
    } = &function.params[0].ty.kind
    else {
        panic!("expected callback function type");
    };
    assert_eq!(params.len(), 3);
    assert_eq!(params[0].mode, ParamMode::Default);
    assert_eq!(params[1].mode, ParamMode::BorrowMut);
    assert_eq!(params[2].mode, ParamMode::Own);
    assert!(matches!(
        named_type_ref(&params[0].ty),
        Some(("int32", args)) if args.is_empty()
    ));
    assert!(matches!(
        named_type_ref(&params[1].ty),
        Some(("String", args)) if args.is_empty()
    ));
    assert!(matches!(
        named_type_ref(&params[2].ty),
        Some(("Vec", args))
            if matches!(
                args,
                [TypeRef {
                    kind: TypeRefKind::Named { name, args },
                    ..
                }] if name == "int32" && args.is_empty()
            )
    ));
    assert!(matches!(
        named_type_ref(return_type),
        Some(("bool", args)) if args.is_empty()
    ));

    assert!(matches!(
        &function.params[1].ty.kind,
        TypeRefKind::Function {
            params,
            return_type,
        } if params.is_empty()
            && matches!(
                &return_type.kind,
                TypeRefKind::Function {
                    params,
                    return_type,
                } if matches!(
                    params.as_slice(),
                    [FunctionTypeParam {
                        mode: ParamMode::Own,
                        ty: TypeRef {
                            kind: TypeRefKind::Named { name, args },
                            ..
                        },
                        ..
                    }] if name == "String" && args.is_empty()
                )
                    && matches!(
                        named_type_ref(return_type),
                        Some(("int64", args)) if args.is_empty()
                    )
            )
    ));

    let TypeRefKind::Function {
        params,
        return_type,
    } = &function.return_type.kind
    else {
        panic!("expected function return type");
    };
    assert!(matches!(
        params.as_slice(),
        [FunctionTypeParam {
            mode: ParamMode::Default,
            ty: TypeRef {
                kind: TypeRefKind::Tuple(elements),
                ..
            },
            ..
        }] if elements.len() == 2
    ));
    assert!(matches!(
        named_type_ref(return_type),
        Some(("None", args)) if args.is_empty()
    ));
}

#[test]
fn function_type_syntax_requires_an_arrow_and_rejects_indirect() {
    let missing_open = parse_item_from("def apply(callback: def int32 -> bool):\n    pass\n")
        .expect_err("function types must open their parameter list");
    assert_eq!(missing_open.code, "AU1101");
    assert!(
        missing_open.message.contains("expected LParen"),
        "unexpected missing-opening-delimiter diagnostic: {missing_open}"
    );

    let missing_close = parse_item_from("def apply(callback: def(int32 -> bool):\n    pass\n")
        .expect_err("function types must close their parameter list before the arrow");
    assert_eq!(missing_close.code, "AU1001");
    assert!(
        missing_close
            .message
            .contains("expected `)` before end of file"),
        "unexpected missing-closing-delimiter diagnostic: {missing_close}"
    );

    let trailing_comma = parse_item_from("def apply(callback: def(int32,) -> bool):\n    pass\n")
        .expect_err("function type parameter lists must not accept an empty trailing slot");
    assert_eq!(trailing_comma.message, "expected a function parameter type");

    let missing_arrow = parse_item_from("def apply(callback: def(int32) bool):\n    pass\n")
        .expect_err("function types must spell their result after an arrow");
    assert!(missing_arrow.message.contains("expected `->`"));

    let indirect = parse_item_from("def apply(callback: indirect def(int32) -> bool):\n    pass\n")
        .expect_err("function values are already represented by code pointers");
    assert!(indirect
        .message
        .contains("`indirect` is not valid on function types"));
}

#[test]
fn function_type_parameter_capabilities_reject_invalid_placements_precisely() {
    let retired = parse_stmt_from("callback: def(borrow String) -> None = factory\n")
        .expect_err("retired borrow must not become a type name");
    assert!(retired.message.contains("omit `borrow` for shared access"));

    let retired_mutable =
        parse_item_from("def apply(callback: def(borrow mut String) -> None):\n    pass\n")
            .expect_err("retired mutable-borrow spelling must point to the current capability");
    assert!(retired_mutable
        .message
        .contains("write `mut T` for mutable access"));

    let doubled = parse_stmt_from("callback: def(mut own String) -> None = factory\n")
        .expect_err("a function type parameter has exactly one capability");
    assert!(doubled.message.contains("only one capability modifier"));

    let missing_shared_type =
        parse_item_from("def apply(callback: def(, String) -> None):\n    pass\n")
            .expect_err("an empty function-type parameter slot must be rejected");
    assert_eq!(
        missing_shared_type.message,
        "expected a function parameter type"
    );

    let missing_mutable_type =
        parse_item_from("def apply(callback: def(mut) -> None):\n    pass\n")
            .expect_err("a capability without a parameter type must be rejected");
    assert_eq!(
        missing_mutable_type.message,
        "expected a type after the function parameter capability"
    );

    let named = parse_stmt_from("callback: def(value: String) -> None = factory\n")
        .expect_err("function type parameters contain types, not names");
    assert!(named.message.contains("contain types only"));

    let default = parse_stmt_from("callback: def(String = \"x\") -> None = factory\n")
        .expect_err("function type parameters cannot have defaults");
    assert!(default.message.contains("cannot declare default values"));

    let nested_default = parse_stmt_from(
        "callback: def(String = build((1, 2), [3, 4], {5: 6}), int32) -> None = factory\n",
    )
    .expect_err(
        "typed-binding lookahead should preserve the function-type default-value diagnostic",
    );
    assert_eq!(
        nested_default.message,
        "function type parameters cannot declare default values"
    );

    let return_capability = parse_stmt_from("callback: def() -> own String = factory\n")
        .expect_err("return capability labels remain invalid");
    assert!(return_capability
        .message
        .contains("`own` is not valid in a type position"));
}

#[test]
fn typed_binding_lookahead_recognizes_function_type_annotations() {
    let stmt = parse_stmt_from("selected: def(int32) -> int32 = increment\n")
        .expect("function-typed local binding should be classified as an assignment");
    let Stmt::Assign(AssignStmt {
        target: AssignTarget::Name(name),
        annotation:
            Some(TypeRef {
                kind:
                    TypeRefKind::Function {
                        params,
                        return_type,
                    },
                ..
            }),
        value,
        ..
    }) = stmt
    else {
        panic!("expected a typed assignment");
    };
    assert_eq!(name, "selected");
    assert!(matches!(
        params.as_slice(),
        [FunctionTypeParam {
            mode: ParamMode::Default,
            ty: TypeRef {
                kind: TypeRefKind::Named { name, args },
                ..
            },
            ..
        }] if name == "int32" && args.is_empty()
    ));
    assert!(matches!(
        return_type.kind,
        TypeRefKind::Named { ref name, ref args } if name == "int32" && args.is_empty()
    ));
    assert!(matches!(value.kind, ExprKind::Name(ref name) if name == "increment"));

    let nested = parse_stmt_from(
        "mut pipeline: def(def(int32) -> int32, (String, int32)) -> def() -> bool = choose\n",
    )
    .expect("lookahead should skip nested function and tuple type components");
    assert!(matches!(
        nested,
        Stmt::Assign(AssignStmt {
            mutable: true,
            annotation: Some(TypeRef {
                kind: TypeRefKind::Function { .. },
                ..
            }),
            ..
        })
    ));
}

#[test]
fn destructuring_and_for_targets_use_recursive_binding_target_nodes() {
    let flat = parse_stmt_from("left, right = pair\n").expect("flat destructuring should parse");
    assert!(matches!(
        flat,
        Stmt::Destructure(DestructureStmt {
            target: BindingTarget::Tuple { ref elements, .. },
            ..
        }) if matches!(
            elements.as_slice(),
            [
                BindingTarget::Name { name: left, .. },
                BindingTarget::Name { name: right, .. }
            ] if left == "left" && right == "right"
        )
    ));

    let nested = parse_stmt_from("(left, (middle, right)) = value\n")
        .expect("nested parenthesized destructuring should parse");
    assert!(matches!(
        nested,
        Stmt::Destructure(DestructureStmt {
            target:
                BindingTarget::Tuple {
                    ref elements,
                    ..
                },
            ..
        }) if matches!(
            elements.as_slice(),
            [
                BindingTarget::Name { name: left, .. },
                BindingTarget::Tuple {
                    elements: nested,
                    ..
                }
            ] if left == "left" && nested.len() == 2
        )
    ));

    let for_stmt = parse_stmt_from("for index, label in rows:\n    pass\n")
        .expect("tuple for target should parse");
    assert!(matches!(
        for_stmt,
        Stmt::For(ForStmt {
            target: BindingTarget::Tuple { ref elements, .. },
            ..
        }) if elements.len() == 2
    ));

    let ordinary =
        parse_stmt_from("for item in rows:\n    pass\n").expect("name for target should parse");
    assert!(matches!(
        ordinary,
        Stmt::For(ForStmt {
            target: BindingTarget::Name { ref name, .. },
            ..
        }) if name == "item"
    ));
}

#[test]
fn tuple_patterns_are_recursive_and_do_not_reuse_variant_payload_nodes() {
    let pattern = parse_pattern_from("(0, (name,))").expect("recursive tuple pattern should parse");
    assert!(matches!(
        pattern,
        Pattern::Tuple(TuplePattern {
            ref elements,
            ..
        }) if elements.len() == 2
            && matches!(elements[0], Pattern::Literal(_))
            && matches!(
                elements[1],
                Pattern::Tuple(TuplePattern {
                    elements: ref nested,
                    ..
                }) if nested.len() == 1
            )
    ));
}

#[test]
fn tuple_parsing_keeps_container_commas_and_rejects_unsupported_forms() {
    let call = parse_expression("consume((1, 2), 3)").expect("call separators should remain owned");
    assert!(matches!(
        call.kind,
        ExprKind::Call { ref args, .. }
            if args.len() == 2
                && matches!(args[0].value.kind, ExprKind::Tuple(ref elements) if elements.len() == 2)
                && matches!(args[1].value.kind, ExprKind::Int(3))
    ));

    let list = parse_expression("[(1, 2), (3,)]").expect("list separators should remain owned");
    assert!(matches!(
        list.kind,
        ExprKind::List(ref elements)
            if elements.len() == 2
                && elements
                    .iter()
                    .all(|element| matches!(element.kind, ExprKind::Tuple(_)))
    ));

    for source in [
        "value: Map[String, int32] = values\n",
        "value: (String, int32) = pair\n",
    ] {
        assert!(
            matches!(parse_stmt_from(source), Ok(Stmt::Assign(_))),
            "commas inside an annotation belong to the type: {source}"
        );
    }

    for source in ["()", "(1, 2,)", "(1,)", "(1, 2)"] {
        let parsed = parse_expression(source);
        if source == "(1,)" || source == "(1, 2)" {
            assert!(parsed.is_ok(), "{source} should be a valid tuple");
        } else {
            let error = parsed.expect_err("unsupported tuple form should fail");
            assert_eq!(error.code, "AU1101");
        }
    }

    let missing_type_comma = parse_item_from("def read(value: (String)):\n    pass\n")
        .expect_err("a singleton tuple type needs a comma");
    assert!(missing_type_comma
        .message
        .contains("tuple types need a comma"));

    let duplicate = parse_stmt_from("left, left = pair\n")
        .expect_err("duplicate destructuring names should fail in the parser");
    assert!(duplicate
        .message
        .contains("duplicate binding target `left`"));

    let compound =
        parse_stmt_from("left, right += pair\n").expect_err("compound destructuring should fail");
    assert!(compound
        .message
        .contains("destructuring only supports plain `=`"));

    let mutable = parse_stmt_from("mut left, right = pair\n")
        .expect_err("mutable destructuring is outside the minimal tuple ticket");
    assert!(mutable
        .message
        .contains("`mut` destructuring is not supported"));

    for (source, expected) in [
        ("()", "empty tuple patterns are not supported"),
        (
            "(name)",
            "tuple patterns need a comma; write `(pattern,)` for a singleton tuple pattern",
        ),
        (
            "(left, right,)",
            "trailing commas are only allowed for singleton tuple patterns",
        ),
    ] {
        let error = parse_pattern_from(source)
            .expect_err("unsupported tuple pattern syntax must produce a teaching diagnostic");
        assert_eq!(error.message, expected);
    }

    for (source, expected) in [
        (
            "def invalid(value: ()):\n    pass\n",
            "empty tuple types are not supported",
        ),
        (
            "def invalid(value: (int32, String,)):\n    pass\n",
            "trailing commas are only allowed for singleton tuple types",
        ),
    ] {
        let error = parse_item_from(source)
            .expect_err("unsupported tuple type syntax must produce a teaching diagnostic");
        assert_eq!(error.message, expected);
    }

    for (source, expected) in [
        (
            "left, = pair\n",
            "an unparenthesized destructuring target cannot end with a comma; write `(name,)` for a singleton tuple target",
        ),
        (
            "(left, right,) = pair\n",
            "trailing commas are only allowed for singleton tuple targets",
        ),
        (
            "((), right) = pair\n",
            "empty tuple binding targets are not supported",
        ),
        (
            "left, 1 = pair\n",
            "binding targets must be names or recursively nested tuple targets",
        ),
    ] {
        let error = parse_stmt_from(source)
            .expect_err("unsupported tuple target syntax must produce a teaching diagnostic");
        assert_eq!(error.message, expected);
    }

    let tokens = lex("left, right\n").expect("diagnostic source should lex");
    let mut parser = Parser::new(tokens);
    let missing_equal = parser
        .parse_destructure_stmt()
        .expect_err("a parsed destructuring target still requires `=`");
    assert_eq!(
        missing_equal.message,
        "expected `=` after destructuring target"
    );

    assert!(matches!(
        parse_stmt_from("pair: (int32, String)? = None\n"),
        Ok(Stmt::Assign(_))
    ));
}

#[test]
fn conditional_expressions_are_low_precedence_and_right_associative() {
    let expression = parse_expression("a or b if c else d if e else f or g")
        .expect("conditional expression should parse");
    let ExprKind::Conditional {
        then_expr,
        condition,
        else_expr,
    } = expression.kind
    else {
        panic!("expected outer conditional expression");
    };
    assert!(matches!(
        then_expr.kind,
        ExprKind::Binary {
            op: BinaryOp::Or,
            ..
        }
    ));
    assert!(matches!(condition.kind, ExprKind::Name(ref name) if name == "c"));
    assert!(matches!(
        else_expr.kind,
        ExprKind::Conditional {
            else_expr,
            ..
        } if matches!(
            else_expr.kind,
            ExprKind::Binary {
                op: BinaryOp::Or,
                ..
            }
        )
    ));
}

#[test]
fn conditional_expression_requires_else_arm() {
    let error = parse_expression("value if ready")
        .expect_err("a conditional expression without `else` must be rejected");
    assert_eq!(
        error.message,
        "conditional expression requires `else` and an alternative value"
    );
    assert_eq!(error.span, Some(Span::new(1, 7)));
}

#[test]
fn d3_parser_preserves_assert_forms_keyword_span_and_comma_boundary() {
    let bare = parse_stmt_from("assert ready\n").expect("bare assertion should parse");
    let Stmt::Assert(bare) = bare else {
        panic!("expected assertion statement");
    };
    assert_eq!(bare.span, Span::new(1, 1));
    assert!(matches!(bare.condition.kind, ExprKind::Name(ref name) if name == "ready"));
    assert!(bare.message.is_none());

    let custom =
        parse_stmt_from("assert left == right, message\n").expect("custom assertion should parse");
    let Stmt::Assert(custom) = custom else {
        panic!("expected assertion statement");
    };
    assert_eq!(custom.span, Span::new(1, 1));
    assert!(matches!(
        custom.condition.kind,
        ExprKind::Binary {
            op: BinaryOp::Eq,
            ..
        }
    ));
    assert!(matches!(
        custom.message,
        Some(Expr {
            kind: ExprKind::Name(ref name),
            ..
        }) if name == "message"
    ));

    let nested = parse_stmt_from("assert check(1, 2), format(3, 4)\n")
        .expect("commas inside calls must not terminate either assertion expression");
    assert!(matches!(
        nested,
        Stmt::Assert(AssertStmt {
            condition:
                Expr {
                    kind: ExprKind::Call { ref args, .. },
                    ..
                },
            message:
                Some(Expr {
                    kind: ExprKind::Call {
                        args: ref message_args,
                        ..
                    },
                    ..
                }),
            ..
        }) if args.len() == 2 && message_args.len() == 2
    ));

    let missing_message =
        parse_stmt_from("assert true,\n").expect_err("trailing assertion comma needs a message");
    assert_eq!(missing_message.code, "AU1101");
    assert_eq!(missing_message.span, Some(Span::new(1, 13)));
}

#[test]
fn parser_preserves_duration_nanoseconds_and_keeps_literal_payloads_nonnegative() {
    for (source, expected_nanos) in [
        ("5ms", 5_000_000),
        ("2s", 2_000_000_000),
        ("1m", 60_000_000_000),
    ] {
        let parsed = parse_expression(source).expect("duration literal should parse");
        assert!(matches!(
            parsed.kind,
            ExprKind::DurationNanos(value) if value == expected_nanos
        ));
    }

    let parsed = parse_expression("-1ms").expect("negative duration expression should parse");
    assert!(matches!(
        parsed.kind,
        ExprKind::Unary {
            op: UnaryOp::Neg,
            expr,
        } if matches!(expr.kind, ExprKind::DurationNanos(1_000_000))
    ));
}

#[test]
fn parse_expression_reports_trailing_tokens_and_primary_errors() {
    let lex_error =
        parse_expression("\"unterminated").expect_err("expected expression lexing failure");
    assert!(lex_error.message.contains("unterminated string literal"));
    assert_eq!(lex_error.code, "AU1001");

    let trailing = parse_expression("1 2").expect_err("expected trailing-token parse failure");
    assert!(trailing
        .message
        .contains("unexpected trailing tokens after expression"));
    assert_eq!(trailing.code, "AU1101");

    let unexpected = parse_expression(")").expect_err("expected unmatched-delimiter failure");
    assert!(unexpected.message.contains("no matching opener"));
    assert_eq!(unexpected.code, "AU1001");

    let borrowed = parse_expression("borrow value").expect_err("expected borrow-prefix failure");
    assert!(borrowed
        .message
        .contains("call arguments cannot start with `borrow`"));
    assert_eq!(borrowed.code, "AU1101");

    let identity = parse_expression("value is None").expect_err("`is` should be rejected");
    assert_eq!(
        identity.message,
        "`is` is not supported; use `== None` or `match` for optional values"
    );

    let bool_expr = parse_expression("true").expect("bool literal should parse");
    assert!(matches!(bool_expr.kind, ExprKind::Bool(true)));

    let string_expr = parse_expression("\"aurora\"").expect("string literal should parse");
    assert!(matches!(string_expr.kind, ExprKind::String(ref value) if value == "aurora"));
}

#[test]
fn d4_parser_accepts_single_quoted_expressions_patterns_and_fstring_arguments() {
    let single_quoted_fstring =
        parse_expression("f'aurora'").expect_err("single-quoted f-strings remain unsupported");
    assert_eq!(
        single_quoted_fstring.message,
        "f-strings must be double-quoted; use `f\"...\"`"
    );
    assert_eq!(single_quoted_fstring.code, "AU1002");

    let string_expr = parse_expression("'aurora'").expect("single-quoted string should parse");
    assert!(matches!(
        string_expr.kind,
        ExprKind::String(ref value) if value == "aurora"
    ));

    let pattern = parse_pattern_from("'ready'").expect("single-quoted pattern should parse");
    assert!(matches!(
        pattern,
        Pattern::Literal(LiteralPattern {
            kind: LiteralPatternKind::String(ref value),
            ..
        }) if value == "ready"
    ));

    let formatted = parse_expression("f\"{echo('{left')}\"")
        .expect("braces inside a single-quoted interpolation argument should stay literal");
    let ExprKind::FString(parts) = formatted.kind else {
        panic!("expected f-string expression");
    };
    let [FormatPart::Expr(interpolation)] = parts.as_slice() else {
        panic!("expected one interpolation");
    };
    let ExprKind::Call { args, .. } = &interpolation.kind else {
        panic!("expected interpolation call");
    };
    assert!(matches!(
        args.as_slice(),
        [Argument {
            value: Expr {
                kind: ExprKind::String(value),
                ..
            },
            ..
        }] if value == "{left"
    ));
}

#[test]
fn comparison_chains_keep_every_operator_at_one_precedence_level() {
    for (source, expected_ops, expected_columns) in [
        (
            "1 < 2 < 3",
            vec![CompareOp::Less, CompareOp::Less],
            vec![3, 7],
        ),
        (
            "1 == 1 == 1",
            vec![CompareOp::Eq, CompareOp::Eq],
            vec![3, 8],
        ),
        (
            "1 < 2 == 2",
            vec![CompareOp::Less, CompareOp::Eq],
            vec![3, 7],
        ),
        (
            "1 == 1 < 2",
            vec![CompareOp::Eq, CompareOp::Less],
            vec![3, 8],
        ),
        (
            "1 in xs not in ys",
            vec![CompareOp::In, CompareOp::NotIn],
            vec![3, 9],
        ),
    ] {
        let expr = parse_expression(source).expect("a comparison chain should parse");
        let ExprKind::CompareChain { links, .. } = &expr.kind else {
            panic!("{source} should parse as one comparison chain, found {expr:?}");
        };
        assert_eq!(
            links.iter().map(|link| link.op).collect::<Vec<_>>(),
            expected_ops,
            "{source}"
        );
        assert_eq!(
            links
                .iter()
                .map(|link| link.op_span.column)
                .collect::<Vec<_>>(),
            expected_columns,
            "{source}"
        );
    }

    for (source, expected_negated) in [("1 in xs", false), ("1 not in xs", true)] {
        let expr = parse_expression(source).expect("a membership test should parse");
        let ExprKind::Membership { negated, .. } = &expr.kind else {
            panic!("{source} should parse as a membership test, found {expr:?}");
        };
        assert_eq!(*negated, expected_negated, "{source}");
    }
}

#[test]
fn d6_parser_preserves_parameter_modes_and_keeps_own_out_of_match() {
    let item = parse_item_from(
        "def modes(copy_value: int32, inferred: String, owned: own String, shared: String, mutable: mut String):\n    pass\n",
    )
    .expect("all ordinary parameter modes should parse");
    let Item::Function(function) = item else {
        panic!("expected function item");
    };
    assert_eq!(
        function
            .params
            .iter()
            .map(|param| param.mode)
            .collect::<Vec<_>>(),
        vec![
            ParamMode::Default,
            ParamMode::Default,
            ParamMode::Own,
            ParamMode::Default,
            ParamMode::BorrowMut,
        ]
    );

    let own_loop = parse_stmt_from("for item in own values:\n    pass\n")
        .expect("owned place iteration should parse");
    assert!(matches!(
        own_loop,
        Stmt::For(ForStmt {
            borrow_mode: Some(ReceiverKind::Value),
            ..
        })
    ));

    // ADR-0022 Q2 inverts this: bare `match` is shared and `match own` is the
    // consuming form, so `own` is now exactly what a consuming match writes.
    let own_match = parse_stmt_from("match own value:\n    case _:\n        pass\n")
        .expect("`match own` is the consuming form");
    let Stmt::Match(own_match) = own_match else {
        panic!("expected match statement");
    };
    assert_eq!(own_match.capability, ReceiverKind::Value);
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
            "    public def read(own self) -> T:",
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
            assert!(trait_decl.supertraits.is_empty());
            assert_eq!(trait_decl.methods.len(), 1);
            assert!(matches!(
                named_type_ref(&trait_decl.methods[0].return_type),
                Some(("None", args)) if args.is_empty()
            ));
        }
        other => panic!("expected trait item, got {other:?}"),
    }

    let supertrait_item = parse_item_from(
        ["trait Child: Parent, Debug[T]:", "    def render(self)"]
            .join("\n")
            .as_str(),
    )
    .expect("trait with supertraits should parse");
    match &supertrait_item {
        Item::Trait(trait_decl) => {
            assert_eq!(trait_decl.supertraits.len(), 2);
            assert!(matches!(
                named_type_ref(&trait_decl.supertraits[0]),
                Some(("Parent", args)) if args.is_empty()
            ));
            assert!(matches!(
                named_type_ref(&trait_decl.supertraits[1]),
                Some(("Debug", args)) if args.len() == 1
            ));
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
fn parser_skips_synthetic_newlines_inside_indented_item_blocks() {
    let mut class_parser = Parser::new(tokens_with_newline_after_first_indent(
        "class Box:\n    value: int32\n",
    ));
    let class_item = class_parser
        .parse_item()
        .expect("class parser should skip synthetic newlines");
    assert!(matches!(class_item, Item::Class(_)));

    let mut enum_parser = Parser::new(tokens_with_newline_after_first_indent(
        "enum Flag:\n    On\n",
    ));
    let enum_item = enum_parser
        .parse_item()
        .expect("enum parser should skip synthetic newlines");
    assert!(matches!(enum_item, Item::Enum(_)));

    let mut trait_parser = Parser::new(tokens_with_newline_after_first_indent(
        "trait Show:\n    def render(self)\n",
    ));
    let trait_item = trait_parser
        .parse_item()
        .expect("trait parser should skip synthetic newlines");
    assert!(matches!(trait_item, Item::Trait(_)));

    let mut impl_parser = Parser::new(tokens_with_newline_after_first_indent(
        "impl Show for Box:\n    def render(self):\n        pass\n",
    ));
    let impl_item = impl_parser
        .parse_item()
        .expect("impl parser should skip synthetic newlines");
    assert!(matches!(impl_item, Item::Impl(_)));
}

#[test]
fn parse_params_and_receivers_cover_error_and_receiver_only_forms() {
    let class_item = parse_item_from(
        [
            "class Counter:",
            "    def read(self):",
            "        pass",
            "    def read_explicit(self):",
            "        pass",
            "    def consume(own self):",
            "        pass",
            "    def bump(mut self, amount: int32 = 1):",
            "        pass",
        ]
        .join("\n")
        .as_str(),
    )
    .expect("class should parse");

    let Item::Class(class_decl) = class_item else {
        panic!("expected class");
    };
    assert_eq!(class_decl.methods.len(), 4);
    assert_eq!(class_decl.methods[0].receiver, Some(ReceiverKind::Borrow));
    assert_eq!(class_decl.methods[0].params.len(), 0);
    assert_eq!(class_decl.methods[1].receiver, Some(ReceiverKind::Borrow));
    assert_eq!(class_decl.methods[1].params.len(), 0);
    assert_eq!(class_decl.methods[2].receiver, Some(ReceiverKind::Value));
    assert_eq!(class_decl.methods[2].params.len(), 0);
    assert_eq!(
        class_decl.methods[3].receiver,
        Some(ReceiverKind::BorrowMut)
    );
    assert_eq!(class_decl.methods[3].params.len(), 1);

    let typed_self = parse_item_from(
        [
            "class Counter:",
            "    def read(self: Counter) -> int32:",
            "        return self.value",
        ]
        .join("\n")
        .as_str(),
    )
    .expect_err("typed self should fail with the receiver teaching diagnostic");
    assert_eq!(
        typed_self.message,
        "`self: Type` is not a method receiver; use `self` for shared access, `own self` to consume, or `mut self` to mutate"
    );

    let bad_param = parse_item_from(
        ["def read(borrow counter: Counter):", "    pass"]
            .join("\n")
            .as_str(),
    )
    .expect_err("prefix borrowed parameter should fail");
    assert!(bad_param
        .message
        .contains("ordinary parameters are written as"));

    let bad_owned_param = parse_item_from(
        ["def read(own counter: Counter):", "    pass"]
            .join("\n")
            .as_str(),
    )
    .expect_err("prefix owned parameter should fail");
    assert_eq!(
        bad_owned_param.message,
        "ordinary owned parameters must be written as `name: own Type`"
    );

    let late_borrow_receiver = parse_item_from(
        [
            "class Counter:",
            "    def read(value: int32, mut self):",
            "        pass",
        ]
        .join("\n")
        .as_str(),
    )
    .expect_err("mutable method receivers must come first");
    assert!(late_borrow_receiver
        .message
        .contains("method receiver must be the first parameter"));

    let late_owned_receiver = parse_item_from(
        [
            "class Counter:",
            "    def consume(value: int32, own self):",
            "        pass",
        ]
        .join("\n")
        .as_str(),
    )
    .expect_err("owned method receivers must come first");
    assert!(late_owned_receiver
        .message
        .contains("method receiver must be the first parameter"));
}

#[test]
fn parser_treats_from_as_a_contextual_expression_and_argument_name() {
    let name = parse_expression("from").expect("`from` should parse as an expression name");
    assert!(matches!(name.kind, ExprKind::Name(ref value) if value == "from"));

    let call = parse_expression("choose(from=\"source\")")
        .expect("`from` should parse as a named argument");
    let ExprKind::Call { args, .. } = call.kind else {
        panic!("expected call expression");
    };
    assert!(matches!(
        args.as_slice(),
        [Argument {
            name: Some(name),
            ..
        }] if name == "from"
    ));

    let module = parse("from pkg.tools import choose\n").expect("from-import should still parse");
    assert!(matches!(
        module.imports.as_slice(),
        [ImportDecl {
            kind: ImportKind::From { .. },
            ..
        }]
    ));

    let local_bindings = parse(
        [
            "def main() -> int32:",
            "    mut from = 1",
            "    from += 1",
            "    return from",
        ]
        .join("\n")
        .as_str(),
    )
    .expect("`from` should parse in local bindings and reassignment targets");
    let Item::Function(main) = &local_bindings.items[0] else {
        panic!("expected function item");
    };
    assert!(matches!(
        main.body.as_slice(),
        [
            Stmt::Assign(AssignStmt {
                target: AssignTarget::Name(first),
                mutable: true,
                ..
            }),
            Stmt::Assign(AssignStmt {
                target: AssignTarget::Name(second),
                op: Some(BinaryOp::Add),
                ..
            }),
            Stmt::Return(_),
        ] if first == "from" && second == "from"
    ));

    let top_level_bindings = parse("mut from = 1\nfrom = 2\nprint(from)\n")
        .expect("`from` should parse in top-level bindings and reassignment targets");
    assert_eq!(top_level_bindings.imports.len(), 0);
    assert!(matches!(
        top_level_bindings.top_level_stmts.as_slice(),
        [Stmt::Assign(_), Stmt::Assign(_), Stmt::Expr(_)]
    ));
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
        ("value //= 1\n", Some(BinaryOp::FloorDiv)),
        ("value %= 1\n", Some(BinaryOp::Mod)),
    ] {
        let stmt = parse_stmt_from(source).expect("compound assignment should parse");
        let Stmt::Assign(assign) = stmt else {
            panic!("expected assignment statement");
        };
        assert_eq!(assign.op, expected);
    }

    let floor_chain = parse_expression("8 // 3 * 2").expect("floor division should parse");
    assert!(matches!(
        floor_chain.kind,
        ExprKind::Binary {
            op: BinaryOp::Mul,
            left,
            ..
        } if matches!(left.kind, ExprKind::Binary { op: BinaryOp::FloorDiv, .. })
    ));

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

    let unclosed_type =
        lex("Vec[Map[String, int32]\n").expect_err("unclosed type brackets should fail lexing");
    assert_eq!(unclosed_type.code, "AU1001");
    assert!(unclosed_type.message.contains("expected `]`"));

    let tokens = lex("[value]\n").expect("tokens");
    let parser = Parser::new(tokens);
    assert_eq!(parser.skip_bracketed_tokens(0), Some(3));

    let unclosed_bracket = lex("[value\n").expect_err("unclosed bracket should fail lexing");
    assert_eq!(unclosed_bracket.code, "AU1001");
    assert!(unclosed_bracket.message.contains("expected `]`"));

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
fn parse_expression_reports_the_chain_limit_for_many_not_operators() {
    let supported = format!("{}true", "not ".repeat(crate::limits::RECURSION_LIMIT - 1));
    parse_expression(&supported).expect("a supported-length `not` chain should parse");

    let excessive = format!("{}true", "not ".repeat(crate::limits::RECURSION_LIMIT + 32));
    let error = parse_expression(&excessive).expect_err("an excessive `not` chain should fail");
    assert!(error.message.contains("expression chain exceeds"));
}

#[test]
fn parse_expression_accepts_reasonably_deep_generated_expressions() {
    let depth = 32usize;
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
    assert!(matches!(indexed.kind, ExprKind::Index { .. }));

    let multi_index = parse_expression("triple[String, int32, Option[bool]]")
        .expect("comma-separated index expression");
    let ExprKind::Index { index, .. } = multi_index.kind else {
        panic!("a bare multi-type suffix remains an index until callable context resolves it");
    };
    let ExprKind::Tuple(elements) = index.kind else {
        panic!("comma-separated index elements should be represented as a tuple");
    };
    assert_eq!(elements.len(), 3);
    assert!(matches!(
        elements[2].kind,
        ExprKind::Index {
            object: _,
            index: _
        }
    ));

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

    let brace_escaped = parser
        .parse_format_parts("literal {{value}} tail", Span::new(1, 1))
        .expect("escaped literal braces should parse");
    assert_eq!(brace_escaped.len(), 1);
    let FormatPart::Literal(text) = &brace_escaped[0] else {
        panic!("expected literal brace escape to stay literal");
    };
    assert_eq!(text, "literal {value} tail");

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
                                    kind: ExprKind::Name("task".to_string()),
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
                                                                                kind: ExprKind::DurationNanos(
                                                                                    5_000_000,
                                                                                ),
                                                                                span,
                                                                            }),
                                                                        },
                                                                        span,
                                                                    }),
                                                                },
                                                                span,
                                                            }),
                                                            ty: TypeRef::named(
                                                                "float64",
                                                                vec![TypeRef::named(
                                                                    "Vec",
                                                                    vec![TypeRef::named(
                                                                        "int32",
                                                                        vec![],
                                                                        false,
                                                                        span,
                                                                    )],
                                                                    false,
                                                                    span,
                                                                )],
                                                                false,
                                                                span,
                                                            ),
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

    let mut ty = TypeRef::named(
        "Map",
        vec![TypeRef::named(
            "Vec",
            vec![TypeRef::named("int32", vec![], false, span)],
            false,
            span,
        )],
        false,
        span,
    );
    offset_type_ref_span(&mut ty, 9, 5);
    assert_eq!(ty.span.line, 9);
    assert_eq!(ty.span.column, 6);
    let (_, ty_args) = named_type_ref(&ty).expect("named Map type");
    assert_eq!(ty_args[0].span.line, 9);
    let (_, nested_args) = named_type_ref(&ty_args[0]).expect("named Vec type");
    assert_eq!(nested_args[0].span.column, 6);
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
            "match mut value:",
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
    assert_eq!(match_stmt.capability, ReceiverKind::BorrowMut);
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

    let for_stmt = parse_stmt_from("for item in mut values:\n    pass\n")
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

    let start_soon_stmt = parse_stmt_from("group.start_soon(worker)\n")
        .expect("start_soon expression statements should parse");
    assert!(matches!(
        start_soon_stmt,
        Stmt::Expr(ExprStmt {
            expr: Expr {
                kind: ExprKind::Call { .. },
                ..
            },
            ..
        })
    ));

    let removed_select = parse_stmt_from("select:\n    case ready = jobs.get():\n        pass\n")
        .expect_err("select syntax should be rejected");
    assert!(!removed_select.message.is_empty());

    let while_stmt = parse_stmt_from("while true:\n    break\n").expect("while should parse");
    assert!(matches!(while_stmt, Stmt::While(_)));
    let continue_stmt = parse_stmt_from("continue\n").expect("continue should parse");
    assert!(matches!(continue_stmt, Stmt::Continue(_)));

    let bad_pattern = parse_stmt_from("match value:\n    case -name:\n        pass\n")
        .expect_err("invalid negative match pattern should fail");
    assert!(bad_pattern
        .message
        .contains("boolean/string/integer/float literals"));

    let out_of_range_pattern = parse_pattern_from("-170141183460469231731687303715884105729")
        .expect_err("negative pattern should fit the signed literal range");
    assert!(out_of_range_pattern
        .message
        .contains("negative integer literal in pattern is outside the supported range"));

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

    let mut specialized = parse_expression("f\"value={Box[Vec[int32]](items[0])}\"")
        .expect("specialized f-string interpolation should parse");
    offset_expr_span(&mut specialized, 9, 5);
    let ExprKind::FString(specialized_parts) = &specialized.kind else {
        panic!("expected specialized f-string expression");
    };
    let FormatPart::Expr(specialized_inner) = &specialized_parts[1] else {
        panic!("expected specialized interpolation part");
    };
    let ExprKind::Call { callee, .. } = &specialized_inner.kind else {
        panic!("expected specialized call inside interpolation");
    };
    let ExprKind::Specialize { type_args, .. } = &callee.kind else {
        panic!("expected specialized callee inside interpolation");
    };
    assert_eq!(type_args[0].span.line, 9);
    assert!(type_args[0].span.column >= 5);
    let (_, specialized_args) = named_type_ref(&type_args[0]).expect("named Box type");
    assert_eq!(specialized_args[0].span.line, 9);
    assert!(specialized_args[0].span.column >= type_args[0].span.column);

    let mut type_ref = TypeRef::named(
        "Box",
        vec![TypeRef::named(
            "Vec",
            vec![TypeRef::named("int32", Vec::new(), false, Span::new(1, 11))],
            false,
            Span::new(1, 7),
        )],
        false,
        Span::new(1, 3),
    );
    offset_type_ref_span(&mut type_ref, 12, 4);
    assert_eq!(type_ref.span, Span::new(12, 7));
    let (_, type_ref_args) = named_type_ref(&type_ref).expect("named Box type");
    assert_eq!(type_ref_args[0].span, Span::new(12, 11));
    let (_, inner_args) = named_type_ref(&type_ref_args[0]).expect("named Vec type");
    assert_eq!(inner_args[0].span, Span::new(12, 15));

    let mut function_type = TypeRef::function(
        vec![TypeRef::named("String", Vec::new(), false, Span::new(1, 7))],
        TypeRef::named("bool", Vec::new(), false, Span::new(1, 18)),
        Span::new(1, 3),
    );
    offset_type_ref_span(&mut function_type, 14, 5);
    assert_eq!(function_type.span, Span::new(14, 8));
    let (params, return_type) = function_type
        .function_parts()
        .expect("function type should remain structural after span offset");
    assert_eq!(params[0].span, Span::new(14, 12));
    assert_eq!(params[0].ty.span, Span::new(14, 12));
    assert_eq!(return_type.span, Span::new(14, 23));
}

#[test]
fn parser_covers_blank_lines_empty_literals_and_specialization_offsets() {
    let class_decl = parse_item_from("class Box:\n\n    value: int32\n")
        .expect("class with blank lines should parse");
    assert_eq!(class_decl.name(), "Box");

    let enum_decl =
        parse_item_from("enum Flag:\n\n    On\n").expect("enum with blank lines should parse");
    assert_eq!(enum_decl.name(), "Flag");

    let trait_decl = parse_item_from("trait Named:\n\n    def name(self) -> String\n")
        .expect("trait with blank lines should parse");
    assert_eq!(trait_decl.name(), "Named");

    let impl_decl = parse_item_from(
        "impl Named for Box:\n\n    def name(self) -> String:\n        return \"box\"\n",
    )
    .expect("impl with blank lines should parse");
    assert_eq!(impl_decl.name(), "Named");

    let match_stmt = parse_stmt_from("match value:\n\n    case 1:\n        pass\n")
        .expect("match with blank lines should parse");
    assert!(matches!(match_stmt, Stmt::Match(_)));

    let start_soon_stmt = parse_stmt_from("group.start_soon(worker)\n")
        .expect("start_soon expression statements should parse");
    assert!(matches!(start_soon_stmt, Stmt::Expr(_)));

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

    let unclosed_specialization =
        lex("Value[int32\n").expect_err("unclosed specialization should fail lexing");
    assert_eq!(unclosed_specialization.code, "AU1001");
    assert!(unclosed_specialization.message.contains("expected `]`"));

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
fn parse_match_expressions_in_argument_and_nested_block_positions() {
    let call_expr = parse_expression(
        [
            "print_text(match value:",
            "    case 1: \"a\"",
            "    case _: \"b\")",
        ]
        .join("\n")
        .as_str(),
    )
    .expect("match expression argument should parse");
    let ExprKind::Call { args, .. } = &call_expr.kind else {
        panic!("expected call expression");
    };
    assert_eq!(args.len(), 1);
    assert!(matches!(args[0].value.kind, ExprKind::Match { .. }));

    let nested_match_return = parse_stmt_from(
        [
            "return match outer:",
            "    case Outer.A:",
            "        match inner:",
            "            case Inner.X: 1",
            "            case Inner.Y: 2",
            "    case Outer.B: 3",
        ]
        .join("\n")
        .as_str(),
    )
    .expect("nested block-form match expression should parse");
    let Stmt::Return(ReturnStmt {
        value: Some(expr), ..
    }) = nested_match_return
    else {
        panic!("expected return statement with value");
    };
    let ExprKind::Match { arms, .. } = &expr.kind else {
        panic!("expected outer match expression");
    };
    assert_eq!(arms.len(), 2);
    assert!(matches!(arms[0].value.kind, ExprKind::Match { .. }));
}

#[test]
fn parser_accepts_multiline_expressions_without_newline_skipping_workarounds() {
    let function = parse_item_from(
        [
            "def add(",
            "    left: int32,",
            "    right: int32",
            ") -> int32:",
            "    return left + right",
        ]
        .join("\n")
        .as_str(),
    )
    .expect("function parameters should continue inside parentheses");
    let Item::Function(function) = function else {
        panic!("expected function item");
    };
    assert_eq!(function.params.len(), 2);

    let call = parse_expression(["add(", "    1,", "    2", ")"].join("\n").as_str())
        .expect("call arguments should continue inside parentheses");
    assert!(matches!(
        call.kind,
        ExprKind::Call { ref args, .. } if args.len() == 2
    ));

    let list = parse_expression(["[", "    1,", "    2", "]"].join("\n").as_str())
        .expect("list elements should continue inside brackets");
    assert!(matches!(list.kind, ExprKind::List(ref values) if values.len() == 2));

    let map = parse_expression(
        ["{", "    \"left\": 1,", "    \"right\": 2", "}"]
            .join("\n")
            .as_str(),
    )
    .expect("map entries should continue inside braces");
    assert!(matches!(map.kind, ExprKind::Map(ref entries) if entries.len() == 2));

    let grouped = parse_expression(["(", "    1 +", "    2", ")"].join("\n").as_str())
        .expect("grouped arithmetic should continue inside parentheses");
    assert!(matches!(grouped.kind, ExprKind::Group(_)));
}

#[test]
fn parser_preserves_match_layout_islands_nested_in_delimiters() {
    let call = parse_expression(
        [
            "choose(",
            "    match value:",
            "        case 1: \"one\"",
            "        case _: \"other\"",
            ")",
        ]
        .join("\n")
        .as_str(),
    )
    .expect("a block-form match should parse as a multiline call argument");
    let ExprKind::Call { args, .. } = call.kind else {
        panic!("expected call expression");
    };
    assert!(matches!(args[0].value.kind, ExprKind::Match { .. }));

    let list = parse_expression(
        [
            "[match value:",
            "    case 1: \"one\"",
            "    case _: \"other\"]",
        ]
        .join("\n")
        .as_str(),
    )
    .expect("the match suite should dedent before a same-line closing bracket");
    let ExprKind::List(values) = list.kind else {
        panic!("expected list expression");
    };
    assert!(matches!(values[0].kind, ExprKind::Match { .. }));

    let nested = parse_expression(
        [
            "consume([",
            "    match outer:",
            "        case 1:",
            "            match inner:",
            "                case 2: \"two\"",
            "                case _: \"other\"",
            "        case _: \"outer\"",
            "])",
        ]
        .join("\n")
        .as_str(),
    )
    .expect("nested match islands should coexist with ordinary delimiter continuation");
    assert!(matches!(nested.kind, ExprKind::Call { .. }));

    let multiline_scrutinee = parse_expression(
        [
            "choose(",
            "    match inspect(",
            "        value",
            "    ):",
            "        case 1: \"one\"",
            "        case _: \"other\"",
            ")",
        ]
        .join("\n")
        .as_str(),
    )
    .expect("a delimited match header may contain a continued scrutinee");
    assert!(matches!(
        multiline_scrutinee.kind,
        ExprKind::Call { ref args, .. }
            if matches!(args[0].value.kind, ExprKind::Match { .. })
    ));

    let visually_indented_closer = parse_item_from(
        [
            "def main():",
            "    print(",
            "        match 1:",
            "            case 1: \"one\"",
            "                )",
        ]
        .join("\n")
        .as_str(),
    )
    .expect("a match container closer may use arbitrary continuation indentation");
    assert!(matches!(visually_indented_closer, Item::Function(_)));
}

#[test]
fn parser_additional_payload_borrow_return_and_match_expression_edges_are_covered() {
    let mixed_payload = parse_item_from("enum Bad:\n    Value(first: int32, String)\n")
        .expect_err("mixed named and positional payloads should fail");
    assert!(mixed_payload
        .message
        .contains("enum variant payloads must be either all named or all positional"));

    // ADR-0022 supersedes ADR-0009's borrowed-return syntax, so the labelled
    // form no longer parses at all.
    let borrowed_return = parse_item_from(
        "def borrow_return(value: borrow[src] String) -> borrow mut[src] String:\n    return value\n",
    )
    .expect_err("borrowed parameter and return labels were removed");
    assert_eq!(
        borrowed_return.message,
        "`borrow T` was removed; write `T` for shared access"
    );

    let match_expr = parse_expression(
        ["match mut value:", "    case Ready: 1", "    case _: 2"]
            .join("\n")
            .as_str(),
    )
    .expect("borrow-mut match expression should parse");
    let ExprKind::Match {
        capability, arms, ..
    } = &match_expr.kind
    else {
        panic!("expected match expression");
    };
    assert_eq!(*capability, ReceiverKind::BorrowMut);
    assert_eq!(arms.len(), 2);

    let delimited_match_expr = parse_expression("(match value:\n    case Ready:\n        1\n)")
        .expect("delimited multiline match expression should parse");
    assert!(matches!(delimited_match_expr.kind, ExprKind::Group(_)));

    let span = Span::new(1, 1);
    let mut manual_match = Expr {
        kind: ExprKind::Match {
            scrutinee: Box::new(Expr {
                kind: ExprKind::Name("value".to_string()),
                span,
            }),
            capability: ReceiverKind::Borrow,
            arms: vec![MatchExprArm {
                pattern: Pattern::Wildcard(span),
                value: Expr {
                    kind: ExprKind::Int(1),
                    span,
                },
                span,
            }],
        },
        span,
    };
    offset_expr_span(&mut manual_match, 4, 2);
    let ExprKind::Match {
        scrutinee, arms, ..
    } = &manual_match.kind
    else {
        panic!("expected manual match expression");
    };
    assert_eq!(scrutinee.span.line, 4);
    assert_eq!(scrutinee.span.column, 3);
    assert_eq!(arms[0].span.line, 4);
    assert_eq!(arms[0].span.column, 3);
    assert_eq!(arms[0].value.span.column, 3);
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
            "match value:",
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
    assert_eq!(match_stmt.capability, ReceiverKind::Borrow);
    assert_eq!(match_stmt.arms.len(), 2);

    // ADR-0022: bare iteration is shared, spelled with no modifier at all.
    let for_stmt =
        parse_stmt_from("for item in values:\n    pass\n").expect("bare for-loop should parse");
    let Stmt::For(for_stmt) = for_stmt else {
        panic!("expected for statement");
    };
    assert_eq!(for_stmt.borrow_mode, None);

    let with_stmt =
        parse_stmt_from("with resource as handle:\n    pass\n").expect("with/as form should parse");
    let Stmt::With(with_stmt) = with_stmt else {
        panic!("expected with statement");
    };
    assert_eq!(with_stmt.binding, "handle");

    let wait_any_stmt = parse_stmt_from("winner = wait_any(tasks, timeout=1ms)\n")
        .expect("wait_any assignments should parse");
    assert!(matches!(wait_any_stmt, Stmt::Assign(_)));

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
            "match value:",
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

    let removed_select = parse_stmt_from(
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
    .expect_err("select syntax should stay removed");
    assert!(!removed_select.message.is_empty());

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

    let tokens = lex("value[\n]\n").expect("tokens");
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
            "    def name(self) -> String",
            "    def label(self) -> String:",
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

#[test]
fn parser_skips_synthetic_newlines_inside_statement_and_expression_blocks() {
    let mut function_parser = Parser::new(tokens_with_newline_after_first_indent(
        "def main():\n    return 1\n",
    ));
    let function_item = function_parser
        .parse_item()
        .expect("function body should skip synthetic newlines");
    assert!(matches!(function_item, Item::Function(_)));

    let mut match_stmt_parser = Parser::new(tokens_with_newline_after_first_indent(
        "match value:\n    case _:\n        pass\n",
    ));
    let match_stmt = match_stmt_parser
        .parse_stmt()
        .expect("match statement should skip synthetic newlines");
    assert!(matches!(match_stmt, Stmt::Match(_)));

    let mut match_expr_parser = Parser::new(tokens_with_newline_after_first_indent(
        "match value:\n    case _: 1\n",
    ));
    let match_expr = match_expr_parser
        .parse_expr()
        .expect("match expression should skip synthetic newlines");
    assert!(matches!(match_expr.kind, ExprKind::Match { .. }));
}

#[test]
fn parser_internal_helpers_cover_member_names_patterns_and_match_terminators() {
    let member = parse_expression("object.from").expect("keyword member names should parse");
    assert!(matches!(member.kind, ExprKind::Member { ref field, .. } if field == "from"));

    let variant_pattern =
        parse_pattern_from("Result.Ok(value, _)").expect("variant subpatterns should parse");
    assert!(matches!(
        variant_pattern,
        Pattern::Variant(VariantPattern { subpatterns, .. }) if subpatterns.len() == 2
    ));

    let empty_variant_pattern =
        parse_pattern_from("Result.Ok()").expect("empty variant subpatterns should parse");
    assert!(matches!(
        empty_variant_pattern,
        Pattern::Variant(VariantPattern { subpatterns, .. }) if subpatterns.is_empty()
    ));

    let span = Span::new(1, 1);
    let mut parser = Parser::new(vec![
        Token {
            kind: TokenKind::Dedent,
            span,
        },
        Token {
            kind: TokenKind::Identifier("next".to_string()),
            span,
        },
        Token {
            kind: TokenKind::Eof,
            span,
        },
    ]);
    parser.index = 1;
    parser
        .expect_match_expr_arm_terminator()
        .expect("a consumed dedent should terminate a match expression arm");

    let mut parser = Parser::new(vec![Token {
        kind: TokenKind::Eof,
        span,
    }]);
    parser
        .expect_match_expr_arm_terminator()
        .expect("EOF should terminate a match expression arm");

    let mut parser = Parser::new(vec![
        Token {
            kind: TokenKind::Identifier("next".to_string()),
            span,
        },
        Token {
            kind: TokenKind::Eof,
            span,
        },
    ]);
    let error = parser
        .expect_match_expr_arm_terminator()
        .expect_err("non-terminator token should fail");
    assert!(error.message.contains("expected Newline"));

    let mut parser = Parser::new(vec![
        Token {
            kind: TokenKind::KwMatch,
            span,
        },
        Token {
            kind: TokenKind::Identifier("value".to_string()),
            span,
        },
        Token {
            kind: TokenKind::Colon,
            span,
        },
        Token {
            kind: TokenKind::Newline,
            span,
        },
        Token {
            kind: TokenKind::Indent,
            span,
        },
        Token {
            kind: TokenKind::Eof,
            span,
        },
    ]);
    let error = parser
        .parse_expr()
        .expect_err("unterminated match expression should report its missing end");
    assert!(error.message.contains("expected end of match expression"));
}

#[test]
fn capability_prefixes_parse_as_bare_mut_and_own() {
    // ADR-0022: bare means shared everywhere, `mut` is mutable access, and
    // `own` is ownership transfer. `borrow` no longer appears in any position.
    let params = parse_item_from(
        "def f(shared: String, mutable: mut String, owned: own String) -> int32:\n    return 0\n",
    )
    .expect("the three parameter capabilities should parse");
    let Item::Function(function) = params else {
        panic!("expected a function item");
    };
    let modes: Vec<_> = function.params.iter().map(|param| param.mode).collect();
    assert_eq!(
        modes,
        vec![ParamMode::Default, ParamMode::BorrowMut, ParamMode::Own]
    );

    for (receiver_source, expected) in [
        ("self", ReceiverKind::Borrow),
        ("mut self", ReceiverKind::BorrowMut),
        ("own self", ReceiverKind::Value),
    ] {
        let source =
            format!("class C:\n    value: int32\n    def read({receiver_source}) -> int32:\n        return 0\n");
        let Item::Class(class) = parse_item_from(&source).expect(&source) else {
            panic!("expected a class item");
        };
        assert_eq!(class.methods[0].receiver, Some(expected), "{source}");
    }

    for (source, expected) in [
        (
            "match value:\n    case _:\n        pass\n",
            ReceiverKind::Borrow,
        ),
        (
            "match mut value:\n    case _:\n        pass\n",
            ReceiverKind::BorrowMut,
        ),
        (
            "match own value:\n    case _:\n        pass\n",
            ReceiverKind::Value,
        ),
    ] {
        let Stmt::Match(match_stmt) = parse_stmt_from(source).expect(source) else {
            panic!("expected a match statement");
        };
        assert_eq!(match_stmt.capability, expected, "{source}");
    }

    for (source, expected) in [
        ("for item in values:\n    pass\n", None),
        (
            "for item in mut values:\n    pass\n",
            Some(ReceiverKind::BorrowMut),
        ),
        (
            "for item in own values:\n    pass\n",
            Some(ReceiverKind::Value),
        ),
    ] {
        let Stmt::For(for_stmt) = parse_stmt_from(source).expect(source) else {
            panic!("expected a for statement");
        };
        assert_eq!(for_stmt.borrow_mode, expected, "{source}");
    }
}

#[test]
fn retired_borrow_spellings_report_their_exact_replacement() {
    // `borrow` stays reserved for one compatibility window, parsed only far
    // enough to say what to write instead.
    for (source, expected) in [
        (
            "def f(value: borrow String):\n    pass\n",
            "`borrow T` was removed; write `T` for shared access",
        ),
        (
            "def f(value: borrow mut String):\n    pass\n",
            "`borrow mut T` was removed; write `mut T`",
        ),
        (
            "class C:\n    value: int32\n    def read(borrow self) -> int32:\n        return 0\n",
            "`borrow self` was removed; write `self` for a shared receiver",
        ),
        (
            "class C:\n    value: int32\n    def bump(borrow mut self):\n        pass\n",
            "`borrow mut self` was removed; write `mut self`",
        ),
        (
            "match borrow value:\n    case _:\n        pass\n",
            "`match borrow` was removed; write `match` for shared access",
        ),
        (
            "match borrow mut value:\n    case _:\n        pass\n",
            "`match borrow mut` was removed; write `match mut`",
        ),
        (
            "for item in borrow values:\n    pass\n",
            "`in borrow` was removed; write `in` for shared iteration",
        ),
        (
            "for item in borrow mut values:\n    pass\n",
            "`in borrow mut` was removed; write `in mut`",
        ),
    ] {
        let parsed = if source.starts_with("class") || source.starts_with("def") {
            parse_item_from(source).map(|_| ())
        } else {
            parse_stmt_from(source).map(|_| ())
        };
        let error = parsed.expect_err(source);
        assert_eq!(error.message, expected, "{source}");
    }
}

#[test]
fn retired_borrowed_returns_report_their_replacement() {
    let error = parse_item_from("def pick(a: String) -> borrow String:\n    return a\n")
        .expect_err("borrowed returns were removed with ADR-0009's syntax");
    assert_eq!(
        error.message,
        "borrowed returns were removed; return an owned value instead"
    );
}

#[test]
fn misplaced_capability_prefixes_name_the_valid_positions() {
    let type_position_message = |capability: &str| {
        format!(
            "`{capability}` is not valid in a type position; capability modifiers belong only on parameters and receivers or on supported `for` and `match` selectors (`mut` also declares mutable local bindings)"
        )
    };
    let expression_position_message = |capability: &str| {
        format!(
            "`{capability}` cannot prefix a call argument or other expression; pass the value directly because the callee parameter declares shared, mutable, or owned access. Capability modifiers belong only on parameters and receivers or on supported `for` and `match` selectors (`mut` also declares mutable local bindings)"
        )
    };

    for capability in ["mut", "own"] {
        for source in [
            format!("class C:\n    value: {capability} String\n"),
            format!("enum Maybe:\n    Some({capability} String)\n"),
            format!("def pick(value: String) -> {capability} String:\n    return value\n"),
            format!(
                "def convert(value: int32) -> int32:\n    return value as {capability} int32\n"
            ),
        ] {
            let error = parse_item_from(&source).expect_err(&source);
            assert_eq!(error.code, "AU1101", "{source}");
            assert_eq!(error.message, type_position_message(capability), "{source}");
        }

        let source = format!(
            "def use(value: String):\n    pass\n\ndef main():\n    value = \"aurora\"\n    use({capability} value)\n"
        );
        let error = parse(&source).expect_err(&source);
        assert_eq!(error.code, "AU1101", "{source}");
        assert_eq!(
            error.message,
            expression_position_message(capability),
            "{source}"
        );
    }

    // Existing capability-bearing positions remain unambiguous.
    parse_item_from(
        "def use(shared: String, changed: mut String, consumed: own String):\n    mut local = shared\n",
    )
    .expect("parameter capabilities and mutable local bindings remain valid");
    parse_stmt_from("for item in mut values:\n    pass\n")
        .expect("mutable loop selectors remain valid");
    parse_stmt_from("match own value:\n    case _:\n        pass\n")
        .expect("owned match selectors remain valid");
}
