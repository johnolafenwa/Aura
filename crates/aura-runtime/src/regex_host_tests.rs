use super::*;

fn number(pattern: &str) -> i64 {
    pattern_number(pattern).expect("a valid pattern")
}

/// Calls a search builtin for `pattern` over the rest of `args`.
fn call(name: &str, args: &[&str]) -> Result<Value> {
    let mut values = vec![Value::Int(IntegerValue::from_i64(number(args[0])))];
    values.extend(args[1..].iter().map(|text| Value::string(*text)));
    host_builtins::host_builtin_function(name).expect("a regex builtin")(values, None)
}

fn host(name: &str, args: Vec<Value>) -> Result<Value> {
    host_builtins::host_builtin_function(name).expect("a regex builtin")(args, None)
}

/// The number and message `regex::__aura_compile` returns for `pattern`.
fn compile_result(pattern: &str) -> (Value, String) {
    let Value::Tuple(result) =
        host("regex::__aura_compile", vec![Value::string(pattern)]).expect("compile returns")
    else {
        panic!("expected a tuple");
    };
    (result.elements[0].clone(), text(&result.elements[1]))
}

fn text(value: &Value) -> String {
    host_string_ref_arg(std::slice::from_ref(value), 0, "test")
        .expect("a str value")
        .to_string()
}

fn texts(value: &Value) -> Vec<String> {
    match value {
        Value::Vec(list) => list.elements.iter().map(|item| text(&item)).collect(),
        other => panic!("expected a list, found {other:?}"),
    }
}

fn items(value: &Value) -> Vec<Value> {
    match value {
        Value::Vec(list) => list.elements.iter().map(|item| item.into_owned()).collect(),
        other => panic!("expected a list, found {other:?}"),
    }
}

fn int_values(values: &[i64]) -> Vec<Value> {
    values
        .iter()
        .map(|value| Value::Int(IntegerValue::from_i64(*value)))
        .collect()
}

fn payload(value: Value) -> Option<Value> {
    match value {
        Value::Union(union) => match union.payload {
            Value::Unit => None,
            payload => Some(payload),
        },
        other => panic!("expected a union, found {other:?}"),
    }
}

#[test]
fn compile_numbers_a_pattern_once_and_explains_a_bad_one() {
    let (first, message) = compile_result("a+b");
    assert_eq!(message, "");
    assert_eq!(first, Value::Int(IntegerValue::from_i64(number("a+b"))));
    assert_eq!(compile_result("a+b").0, first);
    assert_eq!(
        text(&host("regex::__aura_pattern", vec![first]).expect("a known number")),
        "a+b"
    );

    let minus_one = Value::Int(IntegerValue::from_i64(-1));
    let (failed, message) = compile_result("(open");
    assert_eq!(failed, minus_one);
    assert!(message.contains("unclosed group"), "{message}");
    // Backreferences and look-around are not part of the syntax.
    assert_eq!(compile_result(r"(a)\1").0, minus_one);
    let (_, huge) = compile_result(r"\w{5000}{5000}");
    assert!(huge.contains("size limit"), "{huge}");
}

#[test]
fn searches_report_matches_and_scalar_offsets() {
    let pattern = r"\d+";
    let subject = "é1 éé22 333";
    assert_eq!(
        call("regex::__aura_is_match", &[pattern, subject]).unwrap(),
        Value::Bool(true)
    );
    assert_eq!(
        call("regex::__aura_is_match", &[pattern, "none"]).unwrap(),
        Value::Bool(false)
    );
    assert_eq!(
        call("regex::__aura_count", &[pattern, subject]).unwrap(),
        Value::Int(IntegerValue::from_i64(3))
    );
    assert_eq!(
        texts(&call("regex::__aura_find_all", &[pattern, subject]).unwrap()),
        ["1", "22", "333"]
    );

    let found = payload(call("regex::__aura_find", &[pattern, "éé22"]).unwrap()).expect("a match");
    let Value::Tuple(found) = found else {
        panic!("expected a tuple");
    };
    assert_eq!(text(&found.elements[0]), "22");
    assert_eq!(found.elements[1], Value::Int(IntegerValue::from_i64(2)));
    assert_eq!(found.elements[2], Value::Int(IntegerValue::from_i64(4)));
    assert!(payload(call("regex::__aura_find", &[pattern, "none"]).unwrap()).is_none());

    let Value::Tuple(all) = call("regex::__aura_matches", &[pattern, subject]).unwrap() else {
        panic!("expected a tuple");
    };
    assert_eq!(texts(&all.elements[0]), ["333", "22", "1"]);
    assert_eq!(items(&all.elements[1]), int_values(&[1, 2, 5, 7, 8, 11]));
    // An all-ASCII text takes the byte offsets as they are.
    let Value::Tuple(ascii) = call("regex::__aura_matches", &[pattern, "a12b3"]).unwrap() else {
        panic!("expected a tuple");
    };
    assert_eq!(items(&ascii.elements[1]), int_values(&[1, 3, 4, 5]));
}

#[test]
fn captures_mark_groups_that_took_no_part() {
    let pattern = r"(?P<key>[a-z]+)=(\d+)(;)?";
    let Value::Tuple(found) = call("regex::__aura_captures", &[pattern, "x bb=22 y"]).unwrap()
    else {
        panic!("expected a tuple");
    };
    assert_eq!(texts(&found.elements[0]), ["bb=22", "bb", "22", ""]);
    assert_eq!(
        items(&found.elements[1]),
        [true, true, true, false].map(Value::Bool)
    );
    let Value::Tuple(none) = call("regex::__aura_captures", &[pattern, "none"]).unwrap() else {
        panic!("expected a tuple");
    };
    assert!(texts(&none.elements[0]).is_empty());
    let index = |name: &str| call("regex::__aura_group_index", &[pattern, name]).unwrap();
    assert_eq!(index("key"), Value::Int(IntegerValue::from_i64(1)));
    assert_eq!(index("nope"), Value::Int(IntegerValue::from_i64(-1)));
}

#[test]
fn split_replace_and_escape_follow_the_pattern() {
    assert_eq!(
        texts(&call("regex::__aura_split", &[r",\s*", "a, b,c"]).unwrap()),
        ["a", "b", "c"]
    );
    assert_eq!(
        text(
            &call(
                "regex::__aura_replace",
                &[r"(\w+)@(\w+)", "a@b c@d", "$2 at ${1}"]
            )
            .unwrap()
        ),
        "b at a c@d"
    );
    assert_eq!(
        text(
            &call(
                "regex::__aura_replace_all",
                &[r"(\w+)@(\w+)", "a@b c@d", "$2$$"]
            )
            .unwrap()
        ),
        "b$ d$"
    );
    assert_eq!(
        text(&host("regex::__aura_escape", vec![Value::string("a.b*c")]).unwrap()),
        r"a\.b\*c"
    );
}

#[test]
fn calls_reject_unknown_numbers_arity_and_oversized_results() {
    let int = |value: i64| Value::Int(IntegerValue::from_i64(value));
    for unknown in [-1, i64::MAX] {
        let error = host(
            "regex::__aura_is_match",
            vec![int(unknown), Value::string("text")],
        )
        .expect_err("no pattern has this number");
        assert_eq!(error.code.as_str(), ("AU4001"));
        assert!(error.message.contains("no compiled pattern"), "{error:?}");
        assert!(host("regex::__aura_pattern", vec![int(unknown)]).is_err());
    }
    let not_a_number = host(
        "regex::__aura_count",
        vec![Value::string("a"), Value::string("a")],
    )
    .expect_err("a text is not a pattern number");
    assert!(not_a_number.message.contains("pattern number"));
    assert!(host("regex::__aura_count", vec![int(number("a"))]).is_err());
    assert!(call("regex::__aura_replace", &["a", "b"]).is_err());
    assert!(host("regex::__aura_compile", Vec::new()).is_err());
    assert!(host("regex::__aura_pattern", Vec::new()).is_err());
    assert!(host("regex::__aura_escape", Vec::new()).is_err());
    let a = number("a");
    assert!(evaluate_regex_builtin("regex::__aura_replace", a, "a", RegexArgument::None).is_err());
    assert!(evaluate_regex_builtin("regex::__aura_nope", a, "a", RegexArgument::None).is_err());
    assert!(host_builtins::host_builtin_function("regex::__aura_nope").is_none());

    let big = "a".repeat(1 << 20);
    let replacement = "b".repeat(MAX_STRING_BYTES / (1 << 20) + 1);
    let error = evaluate_regex_builtin(
        "regex::__aura_replace_all",
        a,
        &big,
        RegexArgument::Text(&replacement),
    )
    .expect_err("the result is over the string limit");
    assert_eq!(error.code.as_str(), ("AU4005"));
}

#[test]
fn the_thread_cache_starts_again_when_it_is_full_and_recompiles_from_the_text() {
    let numbers: Vec<i64> = (0..=REGEX_CACHE_PATTERNS)
        .map(|index| number(&format!("a{{{}}}", index + 1)))
        .collect();
    let (all, recent) = COMPILED.with(|cache| {
        let cache = cache.borrow();
        (cache.all.len(), cache.recent.len())
    });
    assert!(all <= REGEX_CACHE_PATTERNS, "{all}");
    assert_eq!(recent, RECENT_PATTERNS);
    // A pattern that left the recent list comes back from the full cache.
    COMPILED.with(|cache| cache.borrow_mut().recent.clear());
    assert!(compiled(*numbers.last().expect("a number")).is_ok());
    // A pattern the thread dropped is compiled again from its text.
    COMPILED.with(|cache| *cache.borrow_mut() = Compiled::default());
    assert!(compiled(numbers[0]).expect("recompiles").is_match("a"));
    // Another thread compiles its own program for the same number.
    let first = numbers[0];
    let matched = std::thread::spawn(move || compiled(first).expect("compiles").is_match("a"))
        .join()
        .expect("the thread ends");
    assert!(matched);
}
