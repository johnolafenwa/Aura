use crate::lower_source_to_mir;
use crate::mir::{MirFunction, MirModule};

use super::super::validated_for_emission;
use super::{split_parts, SplitPart};

const SOURCE: &str = r#"
def to_int(text: str) -> int64:
    match parse_int64(text):
        case Result.Ok(value):
            return value
        case Result.Err(_):
            return -1

def one(line: str) -> int64:
    fields = line.split(" ")
    return to_int(fields[3])

def two(line: str) -> int64:
    fields = line.split("=")
    if fields[0] == "size":
        return to_int(fields[1])
    return 0

def temporary(line: str) -> str:
    return line.split(" ")[1].clone()

def formatted(line: str) -> str:
    parts = line.split(", ")
    return f"{parts[1]}/{parts[0]}"

def local(seed: int64) -> int64:
    line = f"event {seed} value {seed * 3}"
    fields = line.split(" ")
    return to_int(fields[3])

def looped(text: str) -> int64:
    mut total = 0
    for line in text.split("\n"):
        fields = line.split(" ")
        total += to_int(fields[3])
    return total

def changed() -> str:
    mut line = "one two"
    fields = line.split(" ")
    line = line + " three"
    return f"{fields[1]}|{line}"

def counted(line: str) -> int64:
    fields = line.split(" ")
    return fields.len() + to_int(fields[0])

def walked(line: str) -> int64:
    fields = line.split(" ")
    mut total = 0
    mut index = 0
    while index < fields.len():
        total += to_int(fields[index])
        index += 1
    return total

def branches(line: str) -> int64:
    fields = line.split(" ")
    mut total = to_int(fields[0])
    if total > 1:
        total += to_int(fields[1])
    return total

def from_end(line: str) -> int64:
    fields = line.split(" ")
    return to_int(fields[-1])

def by_text(line: str, separator: str) -> int64:
    fields = line.split(separator)
    return to_int(fields[0])

def returned(line: str) -> list[str]:
    fields = line.split(" ")
    return fields

def unread(line: str) -> int64:
    fields = line.split(" ")
    return 1

def twice(left: list[str], right: list[str]) -> int64:
    mut total = 0
    for line in left:
        fields = line.split(" ")
        total += to_int(fields[0])
    for line in right:
        fields = line.split(",")
        total += to_int(fields[1])
    return total

def twice_counted(left: list[str], right: list[str]) -> int64:
    mut total = 0
    for line in left:
        fields = line.split(" ")
        total += to_int(fields[0])
    for line in right:
        fields = line.split(",")
        total += fields.len()
    return total

def mixed(batch: list[str]) -> int64:
    mut total = 0
    for line in batch:
        fields = line.split(" ")
        total += to_int(fields[2])
        if fields[1] == "retry":
            total += 1
    return total

def blank(text: str) -> bool:
    return text.trim() == ""

def tag_name(raw: str) -> str:
    return raw.trim_start().to_lower()

def padded(seed: int64) -> int64:
    line = f"  {seed} "
    return to_int(line.trim_end())

def late(text: str) -> int64:
    trimmed = text.trim()
    if trimmed == "":
        return 0
    return trimmed.len()

def regrown(seed: int64) -> str:
    mut line = f" {seed} "
    line = line.trim()
    return line

def main():
    print(blank(" "))
    print(tag_name(" A"))
    print(padded(1))
    print(late(" a "))
    print(regrown(2))
    print(twice(["1 2"], ["3,4"]))
    print(twice_counted(["1 2"], ["3,4"]))
    print(mixed(["1 retry 2"]))
    print(one("a b c 4"))
    print(two("size=2"))
    print(temporary("a b"))
    print(formatted("a, b"))
    print(local(1))
    print(looped("a b c 4"))
    print(changed())
    print(counted("1 2"))
    print(walked("1 2"))
    print(branches("1 2"))
    print(from_end("1 2"))
    print(by_text("1 2", " "))
    print(returned("a b"))
    print(unread("a"))
"#;

fn module() -> MirModule {
    let mir = lower_source_to_mir(SOURCE).expect("the source should lower");
    validated_for_emission(&mir).expect("the source should validate")
}

fn function<'m>(module: &'m MirModule, name: &str) -> &'m MirFunction {
    module
        .functions
        .iter()
        .find(|function| function.name == name)
        .unwrap_or_else(|| panic!("no function `{name}`"))
}

fn parts(name: &str) -> Vec<SplitPart> {
    split_parts(function(&module(), name))
}

#[test]
fn a_split_read_at_one_known_position_makes_no_list() {
    let found = parts("one");
    let [part] = found.as_slice() else {
        panic!("expected one split, found {found:?}");
    };
    assert_eq!(part.receiver, "line");
    assert_eq!(part.separator, " ");
    assert!(part.lists.iter().any(|list| list == "fields"));
    let [loan] = part.loans.as_slice() else {
        panic!("expected one loan, found {:?}", part.loans);
    };
    assert_eq!(loan.index, 3);
    assert!(part.split < loan.begin && loan.begin < loan.end);
    assert_eq!(part.window_end, loan.end);
    // A parameter is not released by the function that borrows it.
    assert_eq!(part.receiver_release, None);
    assert!(!part.list_releases.is_empty());
}

#[test]
fn every_read_of_the_list_in_its_block_is_a_part() {
    // A read in another block than the split's makes the list.
    assert_eq!(parts("two"), Vec::new());
    assert_eq!(parts("branches"), Vec::new());
    let found = parts("formatted");
    let [part] = found.as_slice() else {
        panic!("expected one split, found {found:?}");
    };
    assert_eq!(part.separator, ", ");
    let mut indexes = part.loans.iter().map(|loan| loan.index).collect::<Vec<_>>();
    indexes.sort();
    assert_eq!(indexes, [0, 1]);
    assert_eq!(
        part.window_end,
        part.loans.iter().map(|loan| loan.end).max().unwrap()
    );
}

#[test]
fn a_temporary_list_read_once_is_a_part() {
    let found = parts("temporary");
    let [part] = found.as_slice() else {
        panic!("expected one split, found {found:?}");
    };
    assert_eq!(part.lists.len(), 1);
    assert_eq!(part.alias, None);
    // The element is read into a local, which its release ends.
    let [read] = part.loans.as_slice() else {
        panic!("expected one read, found {:?}", part.loans);
    };
    assert!(read.copied);
    assert_eq!(read.index, 1);
    assert_eq!(part.window_end, read.end);
}

#[test]
fn a_local_two_splits_fill_is_made_unless_neither_makes_it() {
    // Both loops read their `fields` at known positions.
    let found = parts("twice");
    assert_eq!(found.len(), 2, "{found:?}");
    assert!(found
        .iter()
        .all(|part| part.lists.iter().any(|list| list == "fields")));
    assert_ne!(found[0].block, found[1].block);
    // The second loop asks for the length, so both lists are made.
    assert_eq!(parts("twice_counted"), Vec::new());
    // A loan and an element read of one list, in one block.
    let found = parts("mixed");
    let [part] = found.as_slice() else {
        panic!("expected one split, found {found:?}");
    };
    assert_eq!(
        part.loans
            .iter()
            .map(|read| (read.index, read.copied))
            .collect::<Vec<_>>(),
        [(2, false), (1, true)]
    );
}

#[test]
fn the_release_of_a_local_receiver_waits_for_the_last_read() {
    for name in ["local", "looped"] {
        let found = parts(name);
        let [part] = found.as_slice() else {
            panic!("expected one split in `{name}`, found {found:?}");
        };
        let release = part
            .receiver_release
            .unwrap_or_else(|| panic!("`{name}` releases its receiver in the window"));
        assert!(part.split < release && release < part.window_end);
    }
}

#[test]
fn a_list_read_any_other_way_is_made() {
    for name in [
        // The receiver is assigned between the split and the read.
        "changed", // Its length is read.
        "counted", // It is read at a position found at run time, in a loop.
        "walked",  // A position from the end depends on the length.
        "from_end",
        // The separator is not a literal.
        "by_text",  // The list leaves the function.
        "returned", // Nothing reads a part.
        "unread",
    ] {
        assert_eq!(parts(name), Vec::new(), "`{name}`");
    }
}

#[test]
fn a_trim_read_and_released_in_its_block_reads_the_text() {
    for (name, kind, released) in [
        ("blank", 0, false),
        ("tag_name", 1, false),
        ("padded", 2, true),
    ] {
        let found = parts(name);
        let [part] = found.as_slice() else {
            panic!("expected one trim in `{name}`, found {found:?}");
        };
        assert_eq!(part.trim, Some(kind), "`{name}`");
        assert!(part.lists.is_empty() && part.list_releases.is_empty());
        let [read] = part.loans.as_slice() else {
            panic!("expected one read in `{name}`, found {:?}", part.loans);
        };
        assert!(read.copied);
        assert_eq!((read.begin, read.end), (part.split, part.window_end));
        // A local that is trimmed is released once its text is read.
        assert_eq!(part.receiver_release.is_some(), released, "`{name}`");
    }
    // A trimmed text read in another block, or kept, is made.
    assert_eq!(parts("late"), Vec::new());
    assert_eq!(parts("regrown"), Vec::new());
}
