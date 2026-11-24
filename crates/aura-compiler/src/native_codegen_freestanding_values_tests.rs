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

/// The module the emitter gets for `source`, as JSON text.
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
fn an_enum_that_holds_a_collection_is_still_refused() {
    let source = r#"
enum Token:
    Word(list[str])
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

/// Trees, a list, values that hold them, and `try` on results of them.
const BOXES: &str = include_str!("../../aura/tests/freestanding_boxes.au");

#[test]
fn a_type_inside_itself_is_kept_in_a_counted_block_on_every_target() {
    let module = prepared(BOXES);
    for made in [
        "aura::values::value_box_new",
        "aura::values::box_store::",
        "aura::values::box_load::",
        "aura::values::drop_box::",
        "aura::values::retain::",
        "aura::values::release::",
    ] {
        assert!(module.contains(made), "{made}");
    }
    // No lend, enum, or `try` is left for the emitter.
    for gone in ["\"Lend\"", "\"EndLend\"", "\"Try\"", "\"EnumVariant\""] {
        assert!(!module.contains(gone), "{gone}");
    }
    for target in FreestandingTarget::ALL {
        object(BOXES, target);
    }
}

/// Text in literals, classes, enums, results, and the text operations.
const TEXT: &str = include_str!("../../aura/tests/freestanding_text.au");

#[test]
fn text_becomes_calls_of_the_text_library_on_every_target() {
    let module = prepared(TEXT);
    for made in [
        "aura::values::str",
        "aura::values::text_literal",
        "aura::values::value_text_new",
        "aura::values::value_text_concat",
        "aura::values::value_text_equal",
        "aura::values::value_text_compare",
        "aura::values::value_text_length",
        "aura::values::value_text_from_int",
        "aura::values::value_text_from_uint",
        "aura::values::value_text_from_bool",
        "aura::values::value_text_release",
        "aura::values::release::str",
    ] {
        assert!(module.contains(made), "{made}");
    }
    assert!(
        !module.contains("\"Runtime\":{\"op\":{\"Str\""),
        "a text op is left"
    );
    assert!(!module.contains("FormatString"), "an f-string is left");
    for target in FreestandingTarget::ALL {
        object(TEXT, target);
    }
}

#[test]
fn a_program_without_text_gets_no_text_library() {
    for source in [PLAIN_VALUES, BOXES] {
        let module = prepared(source);
        assert!(!module.contains("value_text_"));
        assert!(!module.contains("aura::values::str"));
        assert!(!module.contains("value_list_"));
    }
}

/// Lists of integers, floats, and `bool`: literals, `append`, element
/// reads and writes, loops, a `mut` parameter, and a class that holds one.
const LISTS: &str = include_str!("../../aura/tests/freestanding_list.au");

#[test]
fn a_list_of_numbers_becomes_calls_of_the_list_library_on_every_target() {
    let module = prepared(LISTS);
    for made in [
        "aura::values::list::int64",
        "aura::values::list::float64",
        "aura::values::list::bool",
        "aura::values::list::uint8",
        "aura::values::value_list_new",
        "aura::values::value_list_push_slot",
        "aura::values::value_list_slot",
        "aura::values::value_list_slot_unchecked",
        "aura::values::value_list_release",
        "aura::values::box_store::float64",
        "aura::values::box_load::uint8",
    ] {
        assert!(module.contains(made), "{made}");
    }
    for gone in ["VecLiteral", "IndexInRange", "\"List\""] {
        assert!(!module.contains(gone), "{gone}");
    }
    for target in FreestandingTarget::ALL {
        object(LISTS, target);
    }
}

#[test]
fn a_list_of_other_values_and_other_list_operations_are_still_refused() {
    for body in [
        "    names = [\"a\", \"b\"]\n    return names.len() + n\n",
        "    mut items = [n, 2]\n    items.reverse()\n    return items[0]\n",
    ] {
        let source = format!("export \"C\" def run(n: int64) -> int64:\n{body}");
        let found = refusals(&source);
        assert!(!found.is_empty(), "{body}");
    }
}

#[test]
fn an_f_string_with_a_float_or_a_format_is_still_refused() {
    for body in ["f\"{x}\"", "f\"{n:>4}\""] {
        let source = format!(
            "export \"C\" def run(n: int64, x: float64) -> int64:\n    return {body}.len()\n"
        );
        let found = refusals(&source);
        assert!(
            found.iter().any(|message| message.contains("builds text")),
            "{body}: {found:?}"
        );
    }
}

#[test]
fn a_text_operation_the_library_lacks_is_still_refused() {
    let source = r#"
export "C" def run(n: int64) -> int64:
    text = "a b"
    return text.to_upper().len() + n
"#;
    let found = refusals(source);
    assert!(!found.is_empty(), "{found:?}");
}

#[test]
fn a_program_without_a_type_inside_itself_gets_no_box_functions() {
    let module = prepared(PLAIN_VALUES);
    assert!(!module.contains("aura::values::value_box_new"));
    assert!(!module.contains("aura::values::box_store::"));
}

#[test]
fn the_last_variant_of_a_match_needs_no_test() {
    let source = r#"
enum Chain:
    End
    Link(int64, indirect Chain)

def first(chain: own Chain) -> int64:
    match own chain:
        case Chain.End:
            return 0
        case Chain.Link(value, _):
            return value

export "C" def run(n: int64) -> int64:
    return first(Chain.Link(n, Chain.End))
"#;
    let module = super::super::freestanding::prepare(&lower(source)).expect("the program builds");
    let first = module
        .functions
        .iter()
        .find(|function| function.name.ends_with("first"))
        .expect("the function is there");
    let branches = first
        .blocks
        .iter()
        .filter(|block| matches!(block.terminator, Terminator::Branch { .. }))
        .count();
    assert_eq!(branches, 1, "{:?}", first.blocks);
    object(source, FreestandingTarget::ALL[0]);
}

#[test]
fn a_write_to_a_part_that_owns_heap_memory_is_refused() {
    let source = r#"
enum Chain:
    End
    Link(int64, indirect Chain)

class Holder:
    chain: Chain
    extra: int64

export "C" def run(n: int64) -> int64:
    mut holder = Holder(chain=Chain.End, extra=n)
    holder.chain = Chain.Link(1, Chain.End)
    return holder.extra
"#;
    let found = refusals(source);
    assert!(
        found.iter().any(|message| message
            == "`run` changes a part of a value that owns heap memory, which a freestanding build cannot compile"),
        "{found:?}"
    );
}

#[test]
fn a_union_of_values_that_own_heap_memory_is_refused() {
    let source = r#"
enum Chain:
    End
    Link(int64, indirect Chain)

def pick(n: int64) -> (Chain | int64, int64):
    if n > 0:
        return (Chain.Link(n, Chain.End), n)
    return (n, n)

export "C" def run(n: int64) -> int64:
    both = pick(n)
    return both[1]
"#;
    let found = refusals(source);
    assert!(!found.is_empty(), "{found:?}");
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
