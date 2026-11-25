use super::*;

/// Verifies `source` with the solver on this machine. Without one the test
/// is skipped, except under CI, which installs a pinned Z3.
fn verify(source: &str) -> Option<Report> {
    let options = Options::default();
    if smt::SolverCommand::find(options.solver.as_deref()).is_err() {
        assert!(
            std::env::var_os("CI").is_none(),
            "CI must install an SMT solver for the verifier tests"
        );
        eprintln!("skipping: no SMT solver on PATH");
        return None;
    }
    let program = crate::check_source(source).expect("the program checks");
    let module = crate::mir::lower_for_verification(&program);
    Some(verify_module(&program, &module, &options).expect("the verifier runs"))
}

/// Each obligation that was not proved, as `line kind status`.
fn open_checks(report: &Report) -> Vec<String> {
    report
        .obligations
        .iter()
        .filter(|obligation| obligation.status != Status::Proved)
        .map(|obligation| {
            let status = match &obligation.status {
                Status::Proved => "proved",
                Status::Failed { .. } => "failed",
                Status::Unknown => "unknown",
                Status::Unsupported(_) => "unsupported",
            };
            format!(
                "{} {} {status}",
                obligation.span.line,
                obligation.kind.name()
            )
        })
        .collect()
}

/// Verifies a program that must prove completely, and runs it to show the
/// interpreter agrees that no check fails.
fn assert_proved(source: &str) {
    let Some(report) = verify(source) else {
        return;
    };
    assert_eq!(open_checks(&report), Vec::<String>::new(), "{report:#?}");
    assert!(report.is_proved());
    assert!(!report.obligations.is_empty());
    crate::run_source(source).expect("a proved program runs without a failed check");
}

fn assert_open(source: &str, expected: &[&str]) -> Option<Report> {
    let report = verify(source)?;
    assert_eq!(open_checks(&report), expected, "{report:#?}");
    Some(report)
}

/// Runs a program whose assertions state a model's facts for concrete
/// inputs: the interpreter must agree with every one.
fn assert_runs(source: &str) {
    crate::run_source(source).expect("the model's facts hold at run time");
}

#[test]
fn split_gives_at_least_one_part() {
    assert_proved(
        r##"
def parts(text: str) -> int64:
    ensures result >= 1
    ensures result - 2 <= len(text)
    return len(text.split(","))

def first(text: str) -> str:
    ensures len(result) <= len(text)
    ensures "," not in result
    return text.split(",")[0].clone()

def whole(text: str) -> str:
    requires "," not in text
    ensures result == text
    parts = text.split(",")
    assert len(parts) == 1
    return parts[0].clone()

def second(text: str) -> str:
    requires "," in text
    ensures len(result) <= len(text)
    return text.split(",")[1].clone()

def main():
    print(parts("a,b"))
    print(first("a,b"))
    print(whole("ab"))
    print(second("a,b"))
"##,
    );
}

#[test]
fn split_does_not_promise_a_second_part() {
    assert_open(
        r##"
def parts(text: str) -> int64:
    ensures result >= 2
    return len(text.split(","))

def second(text: str) -> str:
    return text.split(",")[1].clone()

def bounded(text: str, sep: str) -> int64:
    ensures result - 1 <= len(text)
    return len(text.split(sep))

def main():
    print(parts("a,b"))
    print(second("a,b"))
    print(bounded("ab", ""))
"##,
        &[
            "3 ensures unsupported",
            "7 index in range unsupported",
            "10 ensures unsupported",
        ],
    );
}

#[test]
fn split_of_literals_is_exact() {
    assert_proved(
        r##"
def main():
    parts = "a,b,,c".split(",")
    assert len(parts) == 4
    assert parts[0] == "a"
    assert parts[2] == ""
    assert parts[3] == "c"
    empty = "".split(",")
    assert len(empty) == 1
    each = "ab".split("")
    assert len(each) == 4
    assert each[1] == "a"
"##,
    );
    assert_open(
        r##"
def count():
    parts = "a,b,,c".split(",")
    assert len(parts) == 3

def main():
    parts = "a,b,,c".split(",")
    assert parts[1] == "a"
"##,
        &["4 assert failed", "8 assert failed"],
    );
}

#[test]
fn an_f_string_is_its_parts_one_after_another() {
    assert_proved(
        r##"
def fmt(n: int64) -> str:
    ensures len(result) > 0
    ensures result.starts_with("n=")
    return f"n={n}"

def digits(n: int64) -> str:
    ensures len(result) >= 1
    ensures len(result) <= 20
    return str(n)

def method(n: int32) -> str:
    ensures len(result) >= 1
    return n.to_string()

def flag(on: bool) -> str:
    ensures result == "on=true" or result == "on=false"
    return f"on={on}"

def small(n: int64) -> str:
    requires 0 <= n and n <= 9
    ensures len(result) == 1
    return str(n)

def signed(n: int64) -> bool:
    ensures result == (n < 0)
    return str(n).starts_with("-")

def byte(b: uint8) -> str:
    ensures len(result) <= 3
    ensures (result == "0") == (b == 0)
    return f"{b}"

def same(n: int64) -> bool:
    ensures result
    return str(n) == f"{n}"

def greet(name: str) -> str:
    ensures result == "hello " + name
    ensures len(result) == len(name) + 6
    return f"hello {name}"

def main():
    assert f"{12}|{-3}|{true}" == "12|-3|true"
    assert str(7) == "7"
    print(fmt(4))
    print(digits(-5))
    print(method(3))
    print(flag(false))
    print(small(3))
    print(signed(-3))
    print(byte(7))
    print(same(7))
    print(greet("you"))
"##,
    );
}

#[test]
fn an_f_string_promises_no_more_than_its_parts() {
    let report = assert_open(
        r##"
def one(n: int64) -> str:
    ensures len(result) > 1
    return f"{n}"

def long(n: int64) -> str:
    ensures len(result) <= 19
    return str(n)

def real(x: float64) -> str:
    ensures len(result) > 2
    return f"x={x}"

def padded(n: int64) -> str:
    ensures len(result) == 5
    return f"{n:>5}"

def main():
    print(one(1))
    print(long(1))
    print(real(1.5))
    print(padded(1))
"##,
        &[
            "3 ensures unsupported",
            "7 ensures unsupported",
            "11 ensures unsupported",
            "15 ensures unsupported",
        ],
    );
    let Some(report) = report else {
        return;
    };
    let unsupported = report
        .obligations
        .iter()
        .find(|obligation| obligation.span.line == 11)
        .expect("the check of `real`");
    assert!(
        matches!(&unsupported.status, Status::Unsupported(reason) if reason.contains("an f-string at line 12")),
        "{unsupported:?}"
    );
}

#[test]
fn a_float_part_leaves_the_rest_of_an_f_string_known() {
    assert_proved(
        r##"
def real(x: float64) -> str:
    ensures len(result) >= 2
    ensures result.starts_with("x=")
    return f"x={x}"

def main():
    print(real(1.5))
"##,
    );
}

#[test]
fn a_str_index_reads_one_character() {
    assert_proved(
        r##"
def head(text: str) -> str:
    requires len(text) > 0
    ensures len(result) == 1
    ensures text.starts_with(result)
    return text[0]

def last(text: str) -> str:
    requires len(text) > 0
    ensures len(result) == 1
    ensures text.ends_with(result)
    return text[-1]

def at(text: str, i: int64) -> str:
    requires 0 <= i and i < len(text)
    ensures result == text[i]
    return text[i]

def main():
    assert "abc"[1] == "b"
    assert "abc"[-1] == "c"
    print(head("abc"))
    print(last("abc"))
    print(at("abc", 1))
"##,
    );
}

#[test]
fn a_str_index_past_the_end_fails() {
    assert_open(
        r##"
def head(text: str) -> str:
    return text[0]

def end(text: str) -> str:
    return text[len(text)]

def back(text: str) -> str:
    requires len(text) > 0
    return text[-len(text) - 1]

def main():
    print(head("abc"))
"##,
        &[
            "3 index in range failed",
            "6 index in range failed",
            "10 index in range failed",
        ],
    );
}

#[test]
fn a_str_slice_is_a_substring() {
    assert_proved(
        r##"
def middle(text: str) -> int64:
    requires len(text) >= 3
    ensures result == 2
    return len(text[1:3])

def tail(text: str) -> str:
    requires len(text) >= 1
    ensures len(result) == len(text) - 1
    ensures text.ends_with(result)
    return text[1:]

def front(text: str, n: int64) -> str:
    requires 0 <= n and n <= len(text)
    ensures len(result) == n
    ensures result == text[:n]
    return text[:n]

def but_last(text: str) -> str:
    requires len(text) >= 2
    ensures len(result) == len(text) - 2
    return text[1:-1]

def main():
    assert "hello"[1:3] == "el"
    assert "hello"[-3:] == "llo"
    assert "hello"[:] == "hello"
    print(middle("abc"))
    print(tail("abc"))
    print(front("abc", 2))
    print(but_last("abc"))
"##,
    );
}

#[test]
fn a_str_slice_outside_the_text_fails() {
    assert_open(
        r##"
def middle(text: str) -> str:
    return text[1:3]

def crossed(text: str) -> str:
    requires len(text) >= 3
    return text[2:1]

def short(text: str) -> int64:
    requires len(text) >= 3
    ensures result == 3
    return len(text[1:3])

def main():
    print(middle("abc"))
"##,
        &[
            "3 index in range failed",
            "7 index in range failed",
            "11 ensures failed",
        ],
    );
}

#[test]
fn a_part_read_through_a_view_is_the_part() {
    assert_proved(
        r##"
def whole(text: str) -> str:
    requires "," not in text
    ensures result == text
    parts = text.split(",")
    assert len(parts) == 1
    return parts[0].clone()

def twice(text: str) -> bool:
    ensures result
    parts = text.split(",")
    return len(parts[0]) <= len(text) and parts[0] in text

def main():
    print(whole("ab"))
    print(twice("a,b"))
"##,
    );
}

#[test]
fn trimming_gives_a_part_of_the_text() {
    assert_proved(
        r##"
def trimmed(text: str) -> str:
    ensures len(result) <= len(text)
    ensures result in text
    ensures not result.starts_with(" ")
    ensures not result.ends_with(" ")
    ensures result == text.trim()
    return text.trim()

def left(text: str) -> str:
    ensures text.ends_with(result)
    ensures not result.starts_with(" ")
    return text.trim_start()

def right(text: str) -> str:
    ensures text.starts_with(result)
    ensures not result.ends_with(" ")
    return text.trim_end()

def name(text: str) -> str:
    requires len(text.trim()) > 0
    ensures len(result) > 0
    return text.trim()

def caller(text: str) -> int64:
    ensures result <= len(text)
    return len(trimmed(text))

def main():
    assert "  a b ".trim() == "a b"
    assert " a ".trim_start() == "a "
    assert " a ".trim_end() == " a"
    assert " \"a\" ".trim() == "\"a\""
    print(trimmed(" a "))
    print(left(" a "))
    print(right(" a "))
    print(name(" a "))
    print(caller(" a "))
"##,
    );
    assert_open(
        r##"
def shorter(text: str) -> str:
    ensures len(result) < len(text)
    return text.trim()

def same(text: str) -> str:
    ensures result == text
    return text.trim_start()

def blank(text: str) -> str:
    ensures result == ""
    return text.trim_end()

def main():
    assert " a ".trim() == " a"
"##,
        &[
            "3 ensures unsupported",
            "7 ensures unsupported",
            "11 ensures unsupported",
            "15 assert failed",
        ],
    );
}

#[test]
fn find_is_the_first_position() {
    assert_proved(
        r##"
def position(text: str, needle: str) -> int64:
    ensures result >= -1 and result <= len(text)
    ensures needle in text or result == -1
    ensures needle not in text or result >= 0
    match text.find(needle):
        case None:
            return -1
        case int64 as index:
            return index

def where(text: str) -> bool:
    requires text.find("x") is not None
    ensures result
    return "x" in text

def nowhere(text: str) -> bool:
    requires text.find("x") is None
    ensures not result
    return "x" in text

def caller(text: str) -> bool:
    requires "x" in text
    ensures result
    return where(text)

def main():
    print(position("abc", "c"))
    print(where("x"))
    print(nowhere("y"))
    print(caller("x"))
"##,
    );
    assert_open(
        r##"
def position(text: str, needle: str) -> int64:
    ensures result >= 0
    match text.find(needle):
        case None:
            return -1
        case int64 as index:
            return index

def where(text: str) -> bool:
    requires text.find("x") is not None
    return "x" in text

def caller(text: str) -> bool:
    return where(text)

def main():
    print(position("abc", "c"))
"##,
        &["3 ensures failed", "15 requires failed"],
    );
}

#[test]
fn replace_keeps_what_every_replacement_keeps() {
    assert_proved(
        r##"
def swapped(text: str) -> str:
    ensures len(result) == len(text)
    ensures "," not in result
    return text.replace(",", ";")

def same(text: str) -> str:
    requires "," not in text
    ensures result == text
    return text.replace(",", ";")

def dropped(text: str) -> str:
    ensures len(result) <= len(text)
    return text.replace("ab", "")

def doubled(text: str) -> str:
    ensures len(result) >= len(text)
    return text.replace("a", "aa")

def general(text: str, a: str, b: str) -> str:
    requires a not in text
    ensures result == text
    return text.replace(a, b)

def main():
    assert "a,b".replace(",", ";") == "a;b"
    assert "ab".replace("", "-") == "-a-b-"
    print(swapped("a,b"))
    print(same("ab"))
    print(dropped("abab"))
    print(doubled("aba"))
    print(general("ab", "c", "d"))
"##,
    );
    assert_open(
        r##"
def doubled(text: str) -> str:
    ensures len(result) == len(text)
    return text.replace("a", "aa")

def kept(text: str) -> str:
    ensures "a" not in result
    return text.replace("a", "aa")

def spread(text: str) -> str:
    ensures result == text
    return text.replace("", "-")

def main():
    assert "a,b".replace(",", ";") == "a,b"
"##,
        &[
            "3 ensures unsupported",
            "7 ensures unsupported",
            "11 ensures unsupported",
            "15 assert failed",
        ],
    );
}

#[test]
fn case_changes_may_add_characters() {
    assert_proved(
        r##"
def shout(text: str) -> str:
    ensures len(result) >= len(text)
    ensures len(text) > 0 or result == ""
    return text.to_upper()

def quiet(text: str) -> str:
    ensures len(result) >= len(text)
    return text.to_lower()

def main():
    assert "Straße".to_upper() == "STRASSE"
    assert "ABC".to_lower() == "abc"
    print(shout("a"))
    print(quiet("A"))
"##,
    );
    // `"ß".to_upper()` is `"SS"`, so the length is not kept.
    assert_open(
        r##"
def shout(text: str) -> str:
    ensures len(result) == len(text)
    return text.to_upper()

def quiet(text: str) -> str:
    ensures len(result) == len(text)
    return text.to_lower()

def main():
    assert "Straße".to_upper() == "STRAßE"
"##,
        &[
            "3 ensures unsupported",
            "7 ensures unsupported",
            "11 assert failed",
        ],
    );
}

#[test]
fn join_and_repeat_write_their_parts() {
    assert_proved(
        r##"
def pair(a: str, b: str) -> str:
    ensures result == a + ", " + b
    return ", ".join([a.clone(), b.clone()])

def only(parts: list[str]) -> str:
    requires len(parts) == 1
    ensures result == parts[0]
    return ",".join(parts)

def none(parts: list[str], sep: str) -> str:
    requires len(parts) == 0
    ensures result == ""
    return sep.join(parts)

def wide(parts: list[str]) -> str:
    requires len(parts) >= 3
    ensures len(result) >= 4
    return "--".join(parts)

def twice(text: str) -> str:
    ensures result == text + text
    return text.repeat(2)

def once(text: str) -> str:
    ensures result == text
    return text.repeat(1)

def never(text: str) -> str:
    ensures result == ""
    return text.repeat(0)

def many(text: str, n: int64) -> str:
    requires n >= 1
    ensures len(result) >= len(text)
    ensures result.starts_with(text)
    return text.repeat(n)

def main():
    assert "ab".repeat(3) == "ababab"
    print(pair("a", "b"))
    print(only(["a"]))
    empty: list[str] = []
    print(none(empty, ","))
    print(wide(["a", "b", "c"]))
    print(twice("ab"))
    print(once("ab"))
    print(never("ab"))
    print(many("ab", 3))
"##,
    );
    assert_open(
        r##"
def pair(a: str, b: str) -> str:
    ensures result == a + b
    return ", ".join([a.clone(), b.clone()])

def filled(parts: list[str]) -> str:
    ensures len(result) > 0
    return ",".join(parts)

def many(text: str, n: int64) -> str:
    requires n >= 1
    ensures len(result) > len(text)
    return text.repeat(n)

def main():
    assert "ab".repeat(3) == "abab"
"##,
        &[
            "3 ensures failed",
            "7 ensures unsupported",
            "12 ensures unsupported",
            "16 assert failed",
        ],
    );
}

#[test]
fn counts_and_parts_stay_inside_the_text() {
    assert_proved(
        r##"
def occurrences(text: str) -> int64:
    ensures result >= 0 and result <= len(text)
    ensures "ab" in text or result == 0
    return text.count("ab")

def times(text: str, needle: str) -> int64:
    requires needle != ""
    ensures result >= 0 and result <= len(text)
    return text.count(needle)

def letters(text: str) -> int64:
    ensures result == len(text)
    return len(text.chars())

def letter(text: str) -> str:
    requires len(text) > 2
    ensures result == text[2]
    ensures len(result) == 1
    return text.chars()[2].clone()

def size(text: str) -> int64:
    ensures result >= len(text)
    return text.byte_len()

def raw(text: str) -> int64:
    ensures result >= len(text)
    return len(text.to_bytes())

def rows(text: str) -> int64:
    ensures result >= 0 and result <= len(text)
    ensures text != "" or result == 0
    return len(text.lines())

def row(text: str) -> str:
    requires text != ""
    ensures "\n" not in result
    ensures result in text
    return text.lines()[0].clone()

def words(text: str) -> int64:
    ensures result >= 0 and result <= len(text)
    return len(text.split_whitespace())

def word(text: str) -> str:
    requires len(text.split_whitespace()) > 0
    ensures len(result) >= 1
    ensures " " not in result
    return text.split_whitespace()[0].clone()

def main():
    assert "banana".count("an") == 2
    assert len("héllo".chars()) == 5
    assert "héllo".chars()[1] == "é"
    assert "héllo".byte_len() == 6
    assert len("héllo".to_bytes()) == 6
    assert len("a\nb\r\nc\n".lines()) == 3
    assert "a\nb\r\nc\n".lines()[1] == "b"
    assert len(" a  b ".split_whitespace()) == 2
    print(occurrences("abab"))
    print(times("abab", "b"))
    print(letters("abc"))
    print(letter("abc"))
    print(size("abc"))
    print(raw("abc"))
    print(rows("a\nb"))
    print(row("a\nb"))
    print(words("a b"))
    print(word("a b"))
"##,
    );
    assert_open(
        r##"
def occurrences(text: str) -> int64:
    ensures result >= 1
    return text.count("ab")

def letter(text: str) -> str:
    requires len(text) > 2
    ensures result == text[1]
    return text.chars()[2].clone()

def size(text: str) -> int64:
    ensures result == len(text)
    return text.byte_len()

def raw(text: str) -> int64:
    ensures result == len(text)
    return len(text.to_bytes())

def rows(text: str) -> int64:
    ensures result >= 1
    return len(text.lines())

def words(text: str) -> int64:
    requires text != ""
    ensures result >= 1
    return len(text.split_whitespace())

def main():
    assert "banana".count("an") == 3
"##,
        &[
            "3 ensures unsupported",
            "8 ensures unsupported",
            "12 ensures unsupported",
            "16 ensures unsupported",
            "20 ensures unsupported",
            "25 ensures unsupported",
            "29 assert failed",
        ],
    );
}

#[test]
fn stripping_and_splitting_once_are_exact() {
    assert_proved(
        r##"
def rest(text: str) -> str:
    ensures len(result) <= len(text)
    match text.strip_prefix("#"):
        case str as rest:
            assert text == "#" + rest
            return "" + rest
        case None:
            assert not text.starts_with("#")
            return text.clone()

def stem(text: str) -> str:
    ensures len(result) <= len(text)
    match text.strip_suffix(".au"):
        case str as rest:
            assert text == rest + ".au"
            return "" + rest
        case None:
            return text.clone()

def key(text: str) -> str:
    ensures len(result) <= len(text)
    ensures "=" not in result
    found = text.split_once("=")
    if found is None:
        return ""
    before, after = found
    assert text == before + "=" + after
    return before

def main():
    print(rest("#a"))
    print(stem("a.au"))
    print(key("a=b"))
"##,
    );
    assert_open(
        r##"
def rest(text: str) -> str:
    ensures len(result) < len(text)
    match text.strip_prefix("#"):
        case str as rest:
            return "" + rest
        case None:
            return text.clone()

def value(text: str) -> str:
    ensures "=" not in result
    found = text.split_once("=")
    if found is None:
        return ""
    before, after = found
    return after

def last(text: str) -> str:
    ensures len(result) <= len(text)
    found = text.rsplit_once("=")
    if found is None:
        return ""
    before, after = found
    return after

def main():
    print(rest("#a"))
"##,
        &[
            "3 ensures failed",
            "11 ensures failed",
            "19 ensures unsupported",
        ],
    );
}

#[test]
fn parsing_gives_a_result_of_either_kind() {
    assert_proved(
        r##"
def number(text: str) -> int64:
    ensures result >= 0
    match parse_int64(text):
        case Ok(value):
            if value < 0:
                return 0
            return value
        case Err(_):
            return 0

def main():
    print(number("12"))
"##,
    );
    let report = assert_open(
        r##"
def exact(text: str) -> int64:
    requires text == "12"
    ensures result == 12
    match parse_int64(text):
        case Ok(value):
            return value
        case Err(_):
            return 0

def main():
    print(exact("12"))
"##,
        &["4 ensures unsupported"],
    );
    let Some(report) = report else {
        return;
    };
    assert!(
        matches!(&report.obligations[0].status, Status::Unsupported(reason) if reason.contains("`parse_int64`")),
        "{report:#?}"
    );
}

#[test]
fn a_caller_reads_text_operations_in_contracts() {
    assert_proved(
        r##"
def count(text: str) -> int64:
    ensures result == len(text.split(","))
    return len(text.split(","))

def label(n: int64) -> str:
    ensures result == f"n={n}"
    ensures result == "n=" + str(n)
    return f"n={n}"

def tail(text: str) -> str:
    requires len(text) > 0
    ensures result == text[1:]
    return text[1:]

def last(text: str) -> str:
    requires len(text) > 0
    ensures result == text[-1]
    return text[len(text) - 1]

def blank(text: str) -> bool:
    requires all([text[i] == " " for i in range(len(text))])
    requires len(text) > 0
    ensures result
    return text[0] == " "

def copy(text: str) -> str:
    ensures result == text.clone()
    ensures text.len() == len(result)
    ensures result.byte_len() >= len(result)
    ensures result.to_lower() == text.to_lower()
    ensures len(result.lines()) <= len(text)
    ensures len(result.chars()) == len(text)
    ensures result.count("a") <= len(text)
    return text.clone()

def caller(text: str, n: int64) -> int64:
    requires len(text) > 0
    ensures result >= 1
    assert len(label(n)) >= 3
    assert len(tail(text)) == len(text) - 1
    assert last(text) == text[len(text) - 1]
    same = copy(text)
    assert same == text
    assert same.byte_len() >= len(text)
    assert len(same.chars()) == len(text)
    assert blank(" ")
    return count(text)

def main():
    print(caller("a,b", 3))
"##,
    );
}

#[test]
fn a_contract_the_models_do_not_read_is_not_assumed() {
    assert_open(
        r##"
class D:
    e: int64 | None

class C:
    d: D

class B:
    c: C

class A:
    b: B

def deep(a: A, n: int64) -> int64:
    requires a.b.c.d.e is None
    return n

def odd(parts: list[str], n: int64) -> int64:
    requires all([parts[0].count(part) >= 0 for part in parts])
    return n

def sub(xs: list[int64], n: int64) -> int64:
    requires len(xs[1:]) >= 0
    return n

def real(x: float64, n: int64) -> int64:
    requires str(x) != ""
    return n

def pad(n: int64) -> int64:
    requires f"{n:>5}" != ""
    return n

def each(xs: list[int64]) -> int64:
    requires all([str(x) != "" for x in xs])
    return 0

def never(n: int64 | None) -> int64:
    requires n is not None
    return 0

def caller(a: A, parts: list[str], xs: list[int64], n: int64) -> int64:
    requires len(parts) > 0
    print(deep(a, n))
    print(odd(parts, n))
    print(sub(xs, n))
    print(real(1.5, n))
    print(pad(n))
    print(each(xs))
    return never(n)

def main():
    print(pad(1))
"##,
        &[
            // The slice in the contract of `sub` traps on an empty list.
            "23 index in range failed",
            "44 requires unsupported",
            "45 requires unsupported",
            "46 requires unsupported",
            "47 requires unsupported",
            "48 requires unsupported",
            "49 requires unsupported",
            "53 requires unsupported",
        ],
    );
}

#[test]
fn text_the_models_leave_open_stays_open() {
    assert_open(
        r##"
def stepped(text: str) -> str:
    ensures len(result) <= len(text)
    return text[::2]

def real(x: float64) -> str:
    ensures len(result) > 0
    return x.to_string()

def far(text: str) -> str:
    ensures result == "𰀀" + text
    return f"𰀀{text}"

def main():
    print(stepped("abc"))
"##,
        &[
            "3 ensures unsupported",
            "7 ensures unsupported",
            "11 ensures unsupported",
        ],
    );
    assert_proved(
        r##"
def long(text: str) -> str:
    ensures len(result) >= 400
    return f"0123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789{text}{text}"

def empty() -> str:
    ensures result == ""
    return f""

def many() -> int64:
    ensures result >= 1
    parts = "a,a,a,a,a,a,a,a,a,a,a,a,a,a,a,a,a,a,a,a,a,a,a,a,a,a,a,a,a,a,a,a,a,a,a,a,a,a,a,a".split(",")
    return len(parts)

def main():
    print(long("a"))
    print(empty())
    print(many())
"##,
    );
}

#[test]
fn a_check_gets_the_facts_of_the_results_it_reads() {
    // Through a join of two paths, a list element, and a second operation.
    assert_proved(
        r##"
def joined(n: int64, flag: bool) -> int64:
    ensures result >= 1
    mut text = f"{n}"
    if flag:
        text = f"a{n}"
    return len(text)

def stored(n: int64) -> int64:
    ensures result >= 2
    mut words: list[str] = []
    words.append(f"w{n}")
    return len(words[0])

def chained(n: int64) -> int64:
    ensures result <= 66
    return len(f"<{n}>".to_upper().trim())

def main():
    print(joined(3, true))
    print(stored(4))
    print(chained(5))
"##,
    );
    assert_open(
        r##"
def joined(n: int64, flag: bool) -> int64:
    ensures result >= 2
    mut text = f"{n}"
    if flag:
        text = f"a{n}"
    return len(text)

def main():
    print(joined(3, true))
"##,
        &["3 ensures unsupported"],
    );
}

#[test]
fn the_models_agree_with_the_interpreter() {
    assert_runs(
        r##"
def index_of(text: str, needle: str) -> int64:
    match text.find(needle):
        case None:
            return -1
        case int64 as index:
            return index

def check_split(text: str, sep: str):
    parts = text.split(sep)
    n = len(parts)
    assert n >= 1
    if sep == "":
        assert n == len(text) + 2
    else:
        assert n <= len(text) + 1
    assert (n >= 2) == (sep in text)
    if sep in text:
        assert parts[0] == text[:index_of(text, sep)]
    else:
        assert parts[0] == text
    for part in parts:
        assert part in text
        assert len(part) <= len(text)
        assert sep == "" or sep not in part

def check_find(text: str, needle: str):
    found = index_of(text, needle)
    assert (found >= 0) == (needle in text)
    assert found >= -1 and found <= len(text)
    if found >= 0:
        assert text[found:found + len(needle)] == needle
        if needle != "":
            assert needle not in text[:found + len(needle) - 1]
    if needle != "":
        count = text.count(needle)
        assert count >= 0
        assert count * len(needle) <= len(text)
        assert (count >= 1) == (needle in text)
    once = text.split_once(needle) if needle != "" else None
    if once is None:
        assert needle == "" or needle not in text
    else:
        before, after = once
        assert needle in text
        assert before == text[:found]
        assert after == text[found + len(needle):]
        assert text == before + needle + after
        assert needle not in before

def check_affix(text: str, other: str):
    match text.strip_prefix(other):
        case None:
            assert not text.starts_with(other)
        case str as rest:
            assert text.starts_with(other)
            assert rest == text[len(other):]
    match text.strip_suffix(other):
        case None:
            assert not text.ends_with(other)
        case str as rest:
            assert text.ends_with(other)
            assert rest == text[:len(text) - len(other)]

def check_replace(text: str, source: str, target: str):
    result = text.replace(source, target)
    if source not in text:
        assert result == text
    else:
        assert target in result
    if source == target:
        assert result == text
    if len(target) < len(source):
        assert len(result) <= len(text)
    if len(target) == len(source):
        assert len(result) == len(text)
    if len(target) > len(source):
        assert len(result) >= len(text)

def check_apart(text: str, source: str, target: str):
    assert source not in text.replace(source, target)

def is_space(part: str) -> bool:
    return part == " " or part == "\t" or part == "\n" or part == "\r"

def check_one(text: str):
    n = len(text)
    for index in range(n):
        assert len(text[index]) == 1
        assert text[index] == text[index:index + 1]
        assert text[index - n] == text[index]
        assert text.chars()[index] == text[index]
    assert len(text.chars()) == n
    assert text[:] == text
    for cut in range(n + 1):
        assert text[:cut] + text[cut:] == text
        assert len(text[cut:]) == n - cut
        if cut < n:
            assert text[cut - n:] == text[cut:]
            assert text[:cut - n] == text[:cut]
        assert text.starts_with(text[:cut])
    assert text[-n:] == text or n == 0
    for kind in range(3):
        mut trimmed = text.trim()
        if kind == 1:
            trimmed = text.trim_start()
        if kind == 2:
            trimmed = text.trim_end()
        assert trimmed in text
        assert len(trimmed) <= n
        if kind != 2 and len(trimmed) > 0:
            assert not is_space(trimmed[0])
        if kind != 1 and len(trimmed) > 0:
            assert not is_space(trimmed[-1])
        if kind == 1:
            assert text.ends_with(trimmed)
        if kind == 2:
            assert text.starts_with(trimmed)
    upper = text.to_upper()
    lower = text.to_lower()
    assert n <= len(upper) and len(upper) <= 3 * n
    assert n <= len(lower) and len(lower) <= 3 * n
    bytes = text.byte_len()
    assert n <= bytes and bytes <= 4 * n
    assert len(text.to_bytes()) == bytes
    lines = text.lines()
    assert len(lines) <= n
    assert (len(lines) == 0) == (text == "")
    for line in lines:
        assert line in text
        assert "\n" not in line
    words = text.split_whitespace()
    assert 2 * len(words) - 1 <= n
    for word in words:
        assert len(word) >= 1
        assert word in text
        assert " " not in word and "\t" not in word and "\n" not in word and "\r" not in word
    assert text.repeat(0) == ""
    assert text.repeat(1) == text
    assert text.repeat(2) == text + text
    for copies in range(1, 5):
        repeated = text.repeat(copies)
        assert repeated.starts_with(text)
        assert len(repeated) >= n
    assert f"<{text}>" == "<" + text + ">"
    assert "".join([text.clone()]) == text
    assert "-".join([text.clone(), text.clone()]) == text + "-" + text
    assert text.join(["a", "b", "c"]) == "a" + text + "b" + text + "c"
    empty: list[str] = []
    assert text.join(empty) == ""
    assert len(text.join(["a", "b", "c", "d", "e"])) >= 4 * n

def check_int(value: int64):
    text = f"{value}"
    assert text == str(value)
    assert text == value.to_string()
    assert len(text) >= 1 and len(text) <= 20
    assert (value < 0) == text.starts_with("-")
    assert f"n={value}" == "n=" + text
    match parse_int64(text):
        case Ok(parsed):
            assert parsed == value
        case Err(_):
            assert false

def main():
    texts = ["", " ", "a", "ab", "a,b", ",", ",,", "a,,b,", "  x y  ", "\ta\n", "line\r\nnext\n", "\n\n", "héllo wörld", "ß", "İstanbul", "ǆ", "日本語のテキスト", "🙂a🙂b", "aaa", "abab", "x=y=z", "#tag", "name.au", " \u{a0}pad\u{3000}", "ΐ"]
    needles = ["", ",", "a", "aa", "ab", "=", "#", ".au", "🙂", "é", " ", "\n", "b", "ba"]
    for text in texts:
        check_one(text)
        for needle in needles:
            check_split(text, needle)
            check_find(text, needle)
            check_affix(text, needle)
            for target in ["", "-", "ab", ",", "xyz"]:
                check_replace(text, needle, target)
        check_apart(text, ",", ";")
        check_apart(text, "ab", "c")
        check_apart(text, "a", "xy")
    for value in [0, 1, -1, 9, 10, -10, 12345, -9223372036854775807 - 1, 9223372036854775807]:
        check_int(value)
    assert f"{true}|{false}" == "true|false"
    assert true.to_string() == "true"
    print("ok")
"##,
    );
}
