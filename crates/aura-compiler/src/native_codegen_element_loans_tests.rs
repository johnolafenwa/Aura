use crate::lower_source_to_mir;

use super::host_borrowed_operand;

const SOURCE: &str = r#"
import json

def text(docs: dict[int64, json.Value], id: int64) -> str:
    return json.dumps(docs[id])

def absent(docs: dict[int64, json.Value], id: int64) -> bool:
    return json.is_null(docs[id])

def number(docs: dict[int64, json.Value], id: int64) -> int64 | None:
    return json.as_int(docs[id])

def main():
    docs = {1: json.Value.Int(1)}
    print(text(docs, 1))
    print(absent(docs, 1))
    match number(docs, 1):
        case int64 as found:
            print(found)
        case None:
            print("none")
"#;

/// How many instructions of `name` a host builtin borrows an argument in.
fn host_borrows(name: &str) -> usize {
    let module = lower_source_to_mir(SOURCE).expect("the source should lower");
    let function = module
        .functions
        .iter()
        .find(|function| function.name == name)
        .expect("the function should exist");
    function
        .blocks
        .iter()
        .flat_map(|block| &block.instructions)
        .filter(|instruction| host_borrowed_operand(instruction).is_some())
        .count()
}

#[test]
fn dumps_and_is_null_read_their_argument_where_it_lies() {
    assert_eq!(host_borrows("text"), 1);
    assert_eq!(host_borrows("absent"), 1);
}

#[test]
fn another_host_builtin_borrows_nothing() {
    assert_eq!(host_borrows("number"), 0);
}

const PUSH_SOURCE: &str = r#"
def appended(index: mut dict[str, list[int64]], key: str, id: int64):
    match mut index.lookup(key):
        case Lookup.Found(ids):
            ids.append(id)
        case Lookup.Missing:
            pass

def appended_twice(index: mut dict[int64, list[float64]], key: int64, weight: float64):
    match mut index.lookup(key):
        case Lookup.Found(weights):
            weights.append(weight)
            weights.append(weight * 2.0)
        case Lookup.Missing:
            pass

def measured(index: mut dict[str, list[int64]], key: str, id: int64) -> int64:
    match mut index.lookup(key):
        case Lookup.Found(ids):
            ids.append(id)
            return ids.len()
        case Lookup.Missing:
            return 0

def cleared(index: mut dict[str, list[int64]], key: str):
    match mut index.lookup(key):
        case Lookup.Found(ids):
            ids.clear()
        case Lookup.Missing:
            pass

def counted(counts: mut dict[str, int64], key: str):
    match mut counts.lookup(key):
        case Lookup.Found(count):
            count += 1
        case Lookup.Missing:
            pass

def read(index: dict[str, list[int64]], key: str) -> int64:
    match index.lookup(key):
        case Lookup.Found(ids):
            return ids.len()
        case Lookup.Missing:
            return 0

def main():
    mut index: dict[str, list[int64]] = {"a": []}
    appended(index, "a", 1)
    print(measured(index, "a", 2))
    cleared(index, "a")
    print(read(index, "a"))
    mut weights: dict[int64, list[float64]] = {1: []}
    appended_twice(weights, 1, 0.5)
    mut counts: dict[str, int64] = {"a": 0}
    counted(counts, "a")
"#;

fn push_loans(name: &str) -> usize {
    let function = lower_source_to_mir(PUSH_SOURCE)
        .expect("the source should lower")
        .functions
        .into_iter()
        .find(|function| function.name == name)
        .expect("the function should exist");
    super::push_entry_loans(&function).len()
}

#[test]
fn an_entry_loan_that_only_appends_scalars_follows_its_entry() {
    assert_eq!(push_loans("appended"), 1);
    assert_eq!(push_loans("appended_twice"), 1);
}

#[test]
fn an_entry_loan_that_reads_or_changes_its_list_another_way_walks_its_path() {
    for name in ["measured", "cleared", "counted", "read"] {
        assert_eq!(push_loans(name), 0, "{name}");
    }
}

const CLONE_SOURCE: &str = r#"
def lookup(name: str, context: dict[str, str]) -> str:
    return context[name].clone()

def pick(names: list[str], index: int64) -> str:
    return names[index].clone()

def main():
    mut context: dict[str, str] = {}
    context["a"] = "b"
    print(lookup("a", context))
    print(pick(["x"], 0))
"#;

#[test]
fn a_clone_of_a_dict_entry_loan_reads_it_where_it_lies() {
    let mir = lower_source_to_mir(CLONE_SOURCE).expect("the source should lower");
    let function = |name: &str| {
        mir.functions
            .iter()
            .find(|function| function.name == name)
            .expect("the function should exist")
    };
    let dict = |source: &str| source == "context";
    // The clone is the loan's one read, later in its block, and it only
    // borrows the found value while it copies it.
    let copies = super::entry_copy_loans(function("lookup"), dict);
    assert_eq!(copies.values().collect::<Vec<_>>(), [&true], "{copies:?}");
    let shadows = super::shadow_entry_copy_loans(
        function("lookup"),
        &copies,
        &std::collections::HashSet::new(),
        |_| Vec::new(),
    );
    assert_eq!(shadows.len(), 1, "{shadows:?}");
    // A list element's clone keeps its own path, as does a dict's when the
    // caller does not say the source is a dict.
    assert!(super::entry_copy_loans(function("pick"), dict).is_empty());
    assert!(super::entry_copy_loans(function("lookup"), |_| false).is_empty());
}

const BLOCK_SOURCE: &str = r#"
class Job:
    id: int64
    rank: int64
    done: bool
    title: str

def priority(job: Job) -> int64:
    return job.rank * 1000 + job.id

def done(job: Job) -> bool:
    if job.done:
        return true
    return job.done

def length(job: Job) -> int64:
    return job.title.byte_len()

def relay(job: Job) -> int64:
    return priority(job)

def rank_of(job: Job) -> int64:
    return job.rank

def main():
    mut jobs: list[Job] = []
    for i in range(4):
        jobs.append(Job(id=i, rank=i % 2, done=false, title="t"))
    jobs.sort(key=priority)
    mut total = 0
    for i in range(jobs.len()):
        total += priority(jobs[i]) + length(jobs[i])
    if done(jobs[0]):
        total += rank_of(jobs[1])
    print(total)
"#;

/// A parameter whose function reads only its scalar fields, in an
/// assignment, a branch, or a return, takes an element's bytes; one read
/// for a text field, or passed on, does not. A loan passed only to such
/// parameters is lent the element's bytes, with the fields they read.
#[test]
fn block_params_read_only_scalar_fields() {
    use super::{block_params, block_passed_loans, native_element_loans, BlockParam};
    use crate::mir::{Instruction, Rvalue};
    let module = lower_source_to_mir(BLOCK_SOURCE).expect("the source should lower");
    let classes = module
        .classes
        .iter()
        .map(|class| (class.name.clone(), class.clone()))
        .collect();
    let params = block_params(&module, &classes);
    let param = |fields: &[&str]| BlockParam {
        class: "Job".to_string(),
        fields: fields.iter().map(|field| field.to_string()).collect(),
    };
    assert_eq!(
        params.get(&("priority".to_string(), 0)),
        Some(&param(&["id", "rank"]))
    );
    assert_eq!(
        params.get(&("done".to_string(), 0)),
        Some(&param(&["done"]))
    );
    assert_eq!(
        params.get(&("rank_of".to_string(), 0)),
        Some(&param(&["rank"]))
    );
    for name in ["length", "relay", "main"] {
        assert!(
            !params.keys().any(|(function, _)| function == name),
            "{name}"
        );
    }
    let main = module
        .functions
        .iter()
        .find(|function| function.name == "main")
        .expect("main should lower");
    let (uses, _) = native_element_loans(
        main,
        // Every parameter here is borrowed.
        |instruction| match instruction {
            Instruction::Assign {
                value: Rvalue::Call { args, .. },
                ..
            } => args.iter().map(|arg| &arg.value).collect(),
            _ => Vec::new(),
        },
        |_| true,
    );
    let mut passed = block_passed_loans(main, &uses, &params)
        .into_values()
        .map(|param| param.fields)
        .collect::<Vec<_>>();
    passed.sort();
    // The key sort's loan and `priority(jobs[i])`, `done(jobs[0])`, and
    // `rank_of(jobs[1])`; the loan passed to `length` is not.
    assert_eq!(
        passed,
        [
            vec!["done".to_string()],
            vec!["id".to_string(), "rank".to_string()],
            vec!["id".to_string(), "rank".to_string()],
            vec!["rank".to_string()],
        ]
    );
}
