use std::collections::HashSet;

use super::{cyclic_nodes, recursive_functions};
use crate::lower_source_to_mir;

fn recursive(source: &str) -> HashSet<String> {
    let module = lower_source_to_mir(source).expect("source should lower to MIR");
    recursive_functions(&module)
}

fn user(names: &HashSet<String>) -> Vec<String> {
    let mut names = names
        .iter()
        .filter(|name| !name.contains("::") && !name.starts_with("__"))
        .cloned()
        .collect::<Vec<_>>();
    names.sort();
    names
}

#[test]
fn self_and_mutual_recursion_are_recursive_and_plain_calls_are_not() {
    let names = recursive(
        r#"
def fib(n: int64) -> int64:
    if n < 2:
        return n
    return fib(n - 1) + fib(n - 2)

def is_even(n: int64) -> bool:
    if n == 0:
        return true
    return is_odd(n - 1)

def is_odd(n: int64) -> bool:
    if n == 0:
        return false
    return is_even(n - 1)

def double(n: int64) -> int64:
    return n + n

def main() -> int32:
    total = fib(10) + double(2)
    if is_even(total):
        return 0
    return 1
"#,
    );
    assert_eq!(user(&names), vec!["fib", "is_even", "is_odd"]);
}

#[test]
fn a_call_through_a_function_value_reaches_every_escaping_function() {
    let names = recursive(
        r#"
def apply(f: def(int64) -> int64, n: int64) -> int64:
    return f(n)

def walk(n: int64) -> int64:
    if n == 0:
        return 0
    return apply(walk, n - 1)

def leaf(n: int64) -> int64:
    return n

def main() -> int32:
    total = walk(3) + leaf(1)
    return total as int32
"#,
    );
    let names = user(&names);
    assert!(names.contains(&"walk".to_string()), "{names:?}");
    assert!(names.contains(&"apply".to_string()), "{names:?}");
    assert!(!names.contains(&"leaf".to_string()), "{names:?}");
    assert!(!names.contains(&"main".to_string()), "{names:?}");
}

#[test]
fn method_recursion_is_recursive() {
    let names = recursive(
        r#"
class Counter:
    start: int64

    def down(self, n: int64) -> int64:
        if n == 0:
            return self.start
        return self.down(n - 1)

def main() -> int32:
    counter = Counter(start=1)
    return counter.down(3) as int32
"#,
    );
    assert!(names.iter().any(|name| name.contains("down")), "{names:?}");
}

#[test]
fn cyclic_nodes_finds_self_loops_and_components() {
    // 0 -> 1 -> 2 -> 1, 3 -> 3, 4 -> 0.
    let edges = vec![vec![1], vec![2], vec![1], vec![3], vec![0]];
    assert_eq!(cyclic_nodes(&edges), vec![1, 2, 3]);
    assert!(cyclic_nodes(&[]).is_empty());
}

#[test]
fn interpreter_lowering_finds_the_same_recursion() {
    let source = r#"
import sys

def burn(depth: int64, deadline: int64) -> bool:
    if depth == 0:
        return sys.monotonic_time_ms() >= deadline
    if burn(depth - 1, deadline):
        return true
    return burn(depth - 1, deadline)

def main() -> int32:
    _ = burn(3, 0)
    return 0
"#;
    let program = crate::check_source(source).expect("source should check");
    let module = crate::mir::lower_implicit(&program);
    let names = recursive_functions(&module);
    assert_eq!(user(&names), vec!["burn"], "{names:?}");
}
