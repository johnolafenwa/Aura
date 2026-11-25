use crate::lower_source_to_mir;
use crate::mir::MirModule;
use crate::runtime_abi::RuntimeFn;
use crate::runtime_ops::{ListOp, RuntimeOp, StrOp, ValueOp};

use super::super::validated_for_emission;
use super::{
    appended_copies, clone_moves, element_slots, extended_bytes, formatted_reads, literal_chunks,
    plan_views, receiver_view_read, spliced_formats, ViewParam, ViewRead,
};

const SOURCE: &str = r#"
def kind(text: str) -> int64:
    if text == "a":
        return 1
    if "b" != text:
        return 2
    return 3

def size(text: str) -> int64:
    return text.byte_len()

def same(left: str, right: str, count: int64) -> bool:
    return left == right and count > 0

def find(name: str, row: dict[str, str]) -> str:
    match row.get(name):
        case Lookup.Found(value):
            return value.clone()
        case Lookup.Missing:
            return name.to_upper()

def passes(text: str) -> int64:
    return kind(text)

def relays(text: str, row: dict[str, str]) -> str:
    return find(text, row)

def shouts(text: str) -> str:
    return text.to_upper()

def lonely(text: str) -> bool:
    return text == "alone"

def taken(text: own str) -> bool:
    return text == "taken"

def upper_first(text: str) -> int64:
    size = text.to_upper().len()
    if text == "a":
        return size
    return 0

def loops(text: str, count: int64) -> int64:
    if count == 0:
        return shouts(text).len()
    return loops(text, count - 1)

def main():
    words = ["a", "b"]
    row: dict[str, str] = {}
    local = words[0].to_upper()
    print(kind(words[0]))
    print(kind("b"))
    print(size(words[1]))
    print(same(words[0], "a", 1))
    print(find(words[0], row))
    print(passes("x"))
    print(relays(words[1], row))
    print(shouts(words[0]))
    print(lonely(local))
    print(taken("taken"))
    print(loops(words[0], 2))
    print(upper_first(words[0]))
"#;

fn module() -> MirModule {
    let mir = lower_source_to_mir(SOURCE).expect("the source should lower");
    validated_for_emission(&mir).expect("the source should validate")
}

fn view(position: usize, pure: bool) -> ViewParam {
    ViewParam { position, pure }
}

#[test]
fn a_function_that_reads_the_bytes_of_a_str_parameter_gets_a_view_twin() {
    let views = plan_views(&module());
    let mut twins = views.keys().map(String::as_str).collect::<Vec<_>>();
    twins.sort_unstable();
    assert_eq!(twins, ["find", "kind", "passes", "relays", "same", "size"]);
    assert_eq!(views["kind"], [view(0, true)]);
    assert_eq!(views["size"], [view(0, true)]);
    assert_eq!(views["same"], [view(0, true), view(1, true)]);
    assert_eq!(views["passes"], [view(0, true)]);
}

#[test]
fn a_parameter_some_path_loads_as_a_box_is_not_pure() {
    let views = plan_views(&module());
    assert_eq!(views["find"], [view(0, false)]);
    assert_eq!(views["relays"], [view(0, false)]);
}

#[test]
fn a_function_gets_no_view_twin_without_a_read_of_the_bytes_or_a_call_with_words() {
    let views = plan_views(&module());
    // `shouts` only passes its text to an operation that takes the box,
    // no call of `lonely` has its argument as words, `taken` owns its
    // parameter, `loops` only passes its text on to `shouts`, and
    // `upper_first` takes the box of its text before it reads the bytes.
    for function in ["shouts", "lonely", "taken", "loops", "upper_first", "main"] {
        assert!(!views.contains_key(function), "`{function}` has no twin");
    }
}

#[test]
fn a_module_without_str_parameters_has_no_view_twins() {
    let mir = lower_source_to_mir("def main():\n    print(1)\n").expect("lowers");
    let module = validated_for_emission(&mir).expect("validates");
    assert!(plan_views(&module).is_empty());
}

#[test]
fn the_runtime_operations_that_read_a_str_receiver_from_its_bytes() {
    assert_eq!(
        receiver_view_read(RuntimeOp::Value(ValueOp::Clone)),
        Some(ViewRead::NewBox(RuntimeFn::StringFromView))
    );
    assert_eq!(
        receiver_view_read(RuntimeOp::Str(StrOp::ByteLen)),
        Some(ViewRead::ByteLen)
    );
    assert_eq!(receiver_view_read(RuntimeOp::Str(StrOp::ToUpper)), None);
    assert_eq!(receiver_view_read(RuntimeOp::List(ListOp::Len)), None);
}

#[test]
fn a_short_literal_is_compared_in_words_that_cover_it() {
    assert_eq!(literal_chunks(0), []);
    assert_eq!(literal_chunks(1), [(0, 1)]);
    assert_eq!(literal_chunks(3), [(0, 2), (1, 2)]);
    assert_eq!(literal_chunks(4), [(0, 4)]);
    assert_eq!(literal_chunks(7), [(0, 4), (3, 4)]);
    assert_eq!(literal_chunks(8), [(0, 8)]);
    assert_eq!(literal_chunks(13), [(0, 8), (5, 8)]);
    assert_eq!(literal_chunks(16), [(0, 8), (8, 8)]);
    assert_eq!(literal_chunks(17), []);
}

const COPIES: &str = r#"
def gather(words: list[str], out: mut list[str]) -> str:
    out.append(words[0].clone())
    kept = words[1].clone()
    out.append(kept.clone())
    last = words[2].clone()
    print(last)
    out.append(last)
    return kept

def main():
    mut out: list[str] = []
    print(gather(["a", "b", "c"], out))
"#;

#[test]
fn a_copy_made_only_to_be_appended_is_planned_as_words() {
    let mir = lower_source_to_mir(COPIES).expect("the source should lower");
    let module = validated_for_emission(&mir).expect("the source should validate");
    let gather = module
        .functions
        .iter()
        .find(|function| function.name == "gather")
        .expect("the module has `gather`");
    // The copy of `words[0]` and the copy of `kept` are each appended by
    // the next operation. `kept` and `last` are read in between.
    assert_eq!(appended_copies(gather).len(), 2);
    let main = module
        .functions
        .iter()
        .find(|function| function.name == "main")
        .expect("the module has `main`");
    assert!(appended_copies(main).is_empty());
}

const METHODS: &str = r#"
class Tokens:
    pos: int64
    texts: list[str]

    def is_word(self, text: str) -> bool:
        return self.texts[self.pos] == text

    def code(self, text: str) -> int64:
        if text == "select":
            return 1
        return text.byte_len()

    def relay(self, text: str) -> int64:
        return self.code(text)

    def keeps(mut self, text: str):
        self.texts.append(text.to_upper())

def main():
    mut tokens = Tokens(pos=0, texts=["a"])
    print(tokens.is_word("a"))
    print(tokens.code(tokens.texts[0]))
    print(tokens.relay("b"))
    tokens.keeps("c")
"#;

#[test]
fn a_method_that_reads_the_bytes_of_a_str_parameter_gets_a_view_twin() {
    let mir = lower_source_to_mir(METHODS).expect("the source should lower");
    let module = validated_for_emission(&mir).expect("the source should validate");
    let views = plan_views(&module);
    let mut twins = views.keys().map(String::as_str).collect::<Vec<_>>();
    twins.sort_unstable();
    // `keeps` only boxes its text.
    assert_eq!(twins, ["Tokens.code", "Tokens.is_word", "Tokens.relay"]);
    assert_eq!(views["Tokens.is_word"], [view(0, true)]);
    assert_eq!(views["Tokens.code"], [view(0, true)]);
    assert_eq!(views["Tokens.relay"], [view(0, true)]);
}

const LOOPS: &str = r#"
def kind(text: str) -> int64:
    if text == "a":
        return 1
    return 2

def shout(text: str) -> str:
    return text.to_upper()

def main():
    words = ["a", "b"]
    mut total = 0
    for word in words:
        total += kind(word)
    for word in words:
        print(shout(word))
    for word in words:
        if word == "b":
            total += 1
    print(total)
"#;

#[test]
fn a_loop_variable_read_through_its_bytes_is_an_element_slot() {
    let mir = lower_source_to_mir(LOOPS).expect("the source should lower");
    let module = validated_for_emission(&mir).expect("the source should validate");
    let views = plan_views(&module);
    assert_eq!(views["kind"], [view(0, true)]);
    let main = module
        .functions
        .iter()
        .find(|function| function.name == "main")
        .expect("the module has `main`");
    let kept = crate::mir::ownership::kept_arguments(&module);
    let borrowed = |instruction| {
        crate::mir::ownership::callee_borrowed_operands(instruction, |name| kept.get(name))
    };
    let slots = element_slots(main, |_| true, borrowed, &views);
    // The variable passed to `kind` is a slot, the one only passed to
    // `shout`, which boxes it, is not, and the one compared with a literal
    // is a loan of the element already.
    assert_eq!(slots.lists.len(), 1, "{slots:?}");
    assert!(slots.lists.values().all(|list| list == "words"));
    assert!(element_slots(main, |_| false, borrowed, &views)
        .lists
        .is_empty());
}

const LOOKUPS: &str = r#"
def lookup(name: str, row: dict[str, str]) -> str:
    match row.get(name):
        case Lookup.Found(value):
            return value.clone()
        case Lookup.Missing:
            return ""

def shouted(name: str, row: dict[str, str]) -> str:
    match row.get(name):
        case Lookup.Found(value):
            copy = value.clone()
            return value.to_upper() + copy
        case Lookup.Missing:
            return ""

def plain(text: str) -> str:
    upper = text.to_upper()
    return upper.clone()

enum Cell:
    Int(value: int64)
    Text(value: str)

def named(cells: list[Cell]) -> str:
    mut name = ""
    match cells[0]:
        case Cell.Text(text):
            name = text.clone()
        case Cell.Int(value):
            name = f"{value}"
    return name

def main():
    row = {"a": "b"}
    print(lookup("a", row) + shouted("a", row) + plain("c"))
    print(named([Cell.Text(value="t")]))
"#;

#[test]
fn a_copy_that_is_the_last_read_of_a_str_local_may_move_its_reference() {
    let mir = lower_source_to_mir(LOOKUPS).expect("the source should lower");
    let module = validated_for_emission(&mir).expect("the source should validate");
    let function = |name: &str| {
        module
            .functions
            .iter()
            .find(|function| function.name == name)
            .expect("the module has the function")
    };
    // The found payload is released with its lookup.
    let moves = clone_moves(function("lookup"));
    assert_eq!(moves.len(), 1, "{moves:?}");
    assert!(moves
        .values()
        .all(|(_, scrutinee)| matches!(scrutinee, Some((_, true)))));
    // The payload of a lent element's variant is moved on, and the lent
    // element outlives the copy's block.
    let moves = clone_moves(function("named"));
    assert_eq!(moves.len(), 1, "{moves:?}");
    assert!(
        moves
            .values()
            .all(|(_, scrutinee)| matches!(scrutinee, Some((_, false)))),
        "{moves:?}"
    );
    let moves = clone_moves(function("plain"));
    assert_eq!(moves.len(), 1, "{moves:?}");
    assert!(moves.values().all(|(_, scrutinee)| scrutinee.is_none()));
    // The payload is read again after the copy.
    assert!(clone_moves(function("shouted")).is_empty());
    assert!(clone_moves(function("main")).is_empty());
}

const FORMATS: &str = r#"
def label(words: list[str]) -> str:
    view first = words[0]
    view second = words[1]
    return f"{first}{second}"

def kept(words: list[str]) -> str:
    view first = words[0]
    text = f"{first}"
    print(text)
    return text

def main():
    words = ["a", "b"]
    print(label(words) + kept(words))
"#;

#[test]
fn a_view_read_only_into_an_f_string_is_pushed_from_its_words() {
    let mir = lower_source_to_mir(FORMATS).expect("the source should lower");
    let module = validated_for_emission(&mir).expect("the source should validate");
    let function = |name: &str| {
        module
            .functions
            .iter()
            .find(|function| function.name == name)
            .expect("the module has the function")
    };
    let label = function("label");
    let reads = formatted_reads(label);
    assert_eq!(reads.len(), 2, "{reads:?}");
    // Each part is an f-string of its own that the result takes.
    assert_eq!(spliced_formats(label, &reads).len(), 2);
    // `text` is printed and returned as well.
    let kept = function("kept");
    let reads = formatted_reads(kept);
    assert_eq!(reads.len(), 1, "{reads:?}");
    assert!(!spliced_formats(kept, &reads).contains("text"));
    assert!(formatted_reads(function("main")).is_empty());
}

const KEYS: &str = r#"
def add_to(totals: mut dict[str, int64], key: str, amount: int64):
    match mut totals.lookup(key):
        case Lookup.Found(total):
            total += amount
        case Lookup.Missing:
            totals[key.clone()] = amount

def known(key: str, totals: dict[str, int64], seen: set[str]) -> bool:
    return key in totals or key in seen

def main():
    mut totals: dict[str, int64] = {}
    words = ["a", "b"]
    add_to(totals, words[0], 1)
    seen: set[str] = {"a"}
    print(known(words[1], totals, seen))
"#;

#[test]
fn a_str_key_a_dict_or_set_looks_up_is_read_from_its_bytes() {
    let mir = lower_source_to_mir(KEYS).expect("the source should lower");
    let module = validated_for_emission(&mir).expect("the source should validate");
    let views = plan_views(&module);
    // The counting lookup probes by the key's bytes and its entry loan
    // reads the probe's slot; `in` tests a dict and a set.
    assert_eq!(views["add_to"], [view(1, true)]);
    assert_eq!(views["known"], [view(0, true)]);
}

const BYTES: &str = r#"
def add_text(out: mut list[uint8], text: str):
    out.extend(text.to_bytes())

def keep(text: str) -> list[uint8]:
    return text.to_bytes()

def size(out: mut list[uint8], text: str) -> int64:
    bytes = text.to_bytes()
    out.extend("-".to_bytes())
    out.extend(bytes)
    return out.len()

def main():
    mut out: list[uint8] = []
    add_text(out, "a")
    print(keep("b").len())
    print(size(out, "c"))
"#;

#[test]
fn bytes_made_only_to_extend_a_list_are_appended_from_the_words() {
    let mir = lower_source_to_mir(BYTES).expect("the source should lower");
    let module = validated_for_emission(&mir).expect("the source should validate");
    let function = |name: &str| {
        module
            .functions
            .iter()
            .find(|function| function.name == name)
            .expect("the module has the function")
    };
    assert_eq!(extended_bytes(function("add_text")).len(), 1);
    assert!(extended_bytes(function("keep")).is_empty());
    // Only the literal's bytes extend the list as the next operation.
    assert_eq!(extended_bytes(function("size")).len(), 1);
    assert!(extended_bytes(function("main")).is_empty());
    // Each reads its text only through the bytes, which are copied from
    // the words.
    let views = plan_views(&module);
    assert_eq!(views["add_text"], [view(1, true)]);
    assert_eq!(views["keep"], [view(0, true)]);
    assert_eq!(views["size"], [view(1, true)]);
}
