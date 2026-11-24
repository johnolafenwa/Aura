use super::*;

fn lower(source: &str) -> MirModule {
    let module = crate::parse_source(source).expect("source should parse");
    let program = crate::check_module_with_builtin_imports(module).expect("source should check");
    crate::mir::lower_with_source_locations(&program)
}

fn object(source: &str, target: FreestandingTarget) -> Vec<u8> {
    emit_freestanding_object(&lower(source), "kernel.au", source, target).unwrap_or_else(
        |problems| {
            panic!(
                "freestanding build failed: {:?}",
                problems
                    .iter()
                    .map(|problem| &problem.message)
                    .collect::<Vec<_>>()
            )
        },
    )
}

/// The module the emitter gets for , as JSON text.
fn prepared(source: &str) -> String {
    let module = super::super::freestanding::prepare(&lower(source)).unwrap_or_else(|problems| {
        panic!(
            "{:?}",
            problems
                .iter()
                .map(|problem| &problem.message)
                .collect::<Vec<_>>()
        )
    });
    serde_json::to_string(&module).expect("MIR is JSON")
}

fn refusals(source: &str) -> Vec<String> {
    check_freestanding(&lower(source))
        .into_iter()
        .map(|problem| {
            assert_eq!(problem.code, "AU2052");
            problem.message
        })
        .collect()
}

/// Tuples, enums with and without payloads, a generic enum, a result, an
/// enum that holds a class, and a class that holds both.
const PLAIN_VALUES: &str = include_str!("../../aura/tests/freestanding_values.au");

#[test]
fn tuples_and_enums_of_words_become_classes_on_every_target() {
    let module = prepared(PLAIN_VALUES);
    for left in [
        "TupleLiteral",
        "TupleElement",
        "EnumVariant",
        "VariantPayload",
        "\"Match\"",
    ] {
        assert!(!module.contains(left), "{left} is left");
    }
    assert!(module.contains("aura::values::"));
    for target in FreestandingTarget::ALL {
        let bytes = object(PLAIN_VALUES, target);
        assert_eq!(bytes, object(PLAIN_VALUES, target), "{}", target.name());
    }
}

#[test]
fn a_class_for_a_type_that_needs_drop_needs_drop() {
    let module = super::super::freestanding::prepare(&lower(PLAIN_VALUES)).expect("it prepares");
    let made = module
        .classes
        .iter()
        .filter(|class| class.name.starts_with(CLASS_PREFIX))
        .collect::<Vec<_>>();
    // `Item` holds a `Point`, which is not Copy. `Color` holds nothing.
    let holds = |class: &&&MirClass, field: &str| {
        class
            .fields
            .iter()
            .any(|known| matches!(&known.ty, Type::Named(name, _) if name == field))
    };
    let item = made
        .iter()
        .find(|class| holds(class, "Point"))
        .expect("the class of Item");
    assert!(!item.copy);
    let color = made
        .iter()
        .find(|class| class.fields.len() == 1 && class.fields[0].name == TAG_FIELD)
        .expect("the class of Color");
    assert!(color.copy);
}

#[test]
fn a_tuple_holds_a_union_of_words() {
    let source = r#"
def pick(n: int64) -> (int64, int64 | None):
    if n > 0:
        return (n, n)
    return (n, None)

export "C" def first(n: int64) -> int64:
    return pick(n)[0]
"#;
    for target in FreestandingTarget::ALL {
        object(source, target);
    }
}

#[test]
fn a_wildcard_arm_ends_the_tag_comparisons() {
    let source = r#"
enum Light:
    Red
    Amber
    Green

export "C" def go(n: int64) -> int64:
    mut light = Light.Red
    if n > 0:
        light = Light.Green
    match light:
        case Light.Green:
            return 1
        case _:
            return 0
"#;
    let module = prepared(source);
    assert!(!module.contains("\"Match\""));
    for target in FreestandingTarget::ALL {
        object(source, target);
    }
}

#[test]
fn an_enum_that_holds_text_is_still_refused() {
    let source = r#"
enum Token:
    Word(str)
    End

def length(token: Token) -> int64:
    match token:
        case Token.Word(_):
            return 1
        case Token.End:
            return 0

export "C" def run() -> int64:
    return length(Token.End)
"#;
    let found = refusals(source);
    assert!(
        found
            .iter()
            .any(|message| message.contains("takes `token` of type `Token`")),
        "{found:?}"
    );
    assert!(found
        .iter()
        .any(|message| message.contains("matches on an enum value")));
}

#[test]
fn an_enum_that_holds_itself_is_still_refused() {
    let source = r#"
enum Chain:
    End
    Link(int64, indirect Chain)

export "C" def run(n: int64) -> int64:
    chain = Chain.Link(n, Chain.End)
    match chain:
        case Chain.Link(value, _):
            return value
        case Chain.End:
            return 0
"#;
    let found = refusals(source);
    assert!(
        found
            .iter()
            .any(|message| message.contains("holds `chain` of type `Chain`")),
        "{found:?}"
    );
}

#[test]
fn try_on_a_result_of_words_becomes_a_tag_test_and_a_return() {
    let source = include_str!("../../aura/tests/freestanding_try.au");
    let module = prepared(source);
    assert!(!module.contains("\"Try\""));
    assert!(module.contains("_value_try_0_error"));
    for target in FreestandingTarget::ALL {
        object(source, target);
    }
}

#[test]
fn try_converts_its_error_through_a_from_impl() {
    let source = r#"
enum Small:
    Bad

enum Large:
    Worse(int64)

impl From[Small] for Large:
    def from(value: own Small) -> Large:
        return Large.Worse(7)

def inner(n: int64) -> Result[int64, Small]:
    if n > 0:
        return Ok(n)
    return Err(Small.Bad)

def outer(n: int64) -> Result[int64, Large]:
    value = try inner(n)
    return Ok(value + 1)

export "C" def run(n: int64) -> int64:
    match outer(n):
        case Ok(value):
            return value
        case Err(Large.Worse(code)):
            return 0 - code
"#;
    let module = prepared(source);
    assert!(!module.contains("\"Try\""));
    for target in FreestandingTarget::ALL {
        object(source, target);
    }
}
