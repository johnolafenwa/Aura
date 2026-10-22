use super::super::ownership::{insert_ownership, strip_ownership};
use super::super::{validate_loan_flow, validate_ownership, Instruction, MirFunction, MirModule};

/// A store whose methods read and change its collection fields.
const STORE: &str = r#"
class Store:
    docs: list[str] = []
    by_id: dict[str, int64] = {}

    def add(mut self, key: str, value: int64):
        self.docs.append(key.clone())
        self.by_id[key.clone()] = value
        fill(self.docs, self.by_id, key)

    def count(self) -> int64:
        return total(self.docs) + self.docs.len()

    def scan(self) -> int64:
        mut sum = 0
        for doc in self.docs:
            sum += doc.len()
        return sum

    def twice(self) -> int64:
        return pair(self.docs, self.docs)

    def with_owner(self, other: Store) -> int64:
        return owner_total(self.docs, other)

def total(xs: list[str]) -> int64:
    return xs.len()

def pair(xs: list[str], ys: list[str]) -> int64:
    return xs.len() + ys.len()

def owner_total(xs: list[str], owner: Store) -> int64:
    return xs.len() + owner.docs.len()

def fill(docs: mut list[str], by_id: mut dict[str, int64], key: str):
    docs.append(key.clone())
    by_id[key.clone()] = 0

def main():
    mut s = Store()
    s.add("a", 1)
    s.add("bc", 2)
    print(s.count())
    print(s.scan())
    print(s.twice())
    print(s.with_owner(Store()))
"#;

/// Buckets reached through a lookup, whose methods read or change one.
const REGISTRY: &str = r#"
class Bucket:
    items: list[int64] = []

    def size(self) -> int64:
        return self.items.len()

    def add(mut self, value: int64):
        self.items.append(value)

class Registry:
    buckets: dict[str, Bucket] = {}

    def add(mut self, name: str, value: int64):
        if name not in self.buckets:
            self.buckets[name.clone()] = Bucket()
        match mut self.buckets.lookup(name):
            case Lookup.Found(bucket):
                bucket.add(value)
            case Lookup.Missing:
                pass

    def size(self, name: str) -> int64:
        match self.buckets.lookup(name):
            case Lookup.Found(bucket):
                return bucket.size()
            case Lookup.Missing:
                return 0

def main():
    mut registry = Registry()
    registry.add("a", 1)
    registry.add("a", 2)
    print(registry.size("a"))
"#;

fn lowered(source: &str) -> MirModule {
    crate::lower_source_to_mir(source).expect("the program should lower")
}

fn function<'a>(module: &'a MirModule, name: &str) -> &'a MirFunction {
    module
        .functions
        .iter()
        .find(|function| function.name == name)
        .unwrap_or_else(|| panic!("function `{name}` should exist"))
}

fn function_mut<'a>(module: &'a mut MirModule, name: &str) -> &'a mut MirFunction {
    module
        .functions
        .iter_mut()
        .find(|function| function.name == name)
        .unwrap_or_else(|| panic!("function `{name}` should exist"))
}

fn instructions(function: &MirFunction) -> impl Iterator<Item = &Instruction> {
    function.blocks.iter().flat_map(|block| &block.instructions)
}

/// The lends of `function` in order, each as its place with `mut ` before
/// a mutable one.
fn lends(function: &MirFunction) -> Vec<String> {
    instructions(function)
        .filter_map(|instruction| match instruction {
            Instruction::Lend { place, mutable, .. } => {
                Some(format!("{}{place}", if *mutable { "mut " } else { "" }))
            }
            _ => None,
        })
        .collect()
}

fn retained(function: &MirFunction) -> Vec<&str> {
    instructions(function)
        .filter_map(|instruction| match instruction {
            Instruction::Retain { place } => Some(place.as_str()),
            _ => None,
        })
        .collect()
}

fn checks(module: &MirModule) -> Result<(), String> {
    validate_loan_flow(module)?;
    validate_ownership(module).map_err(|error| format!("{error:?}"))
}

fn json(module: &MirModule) -> serde_json::Value {
    serde_json::to_value(module).expect("MIR should serialize")
}

#[test]
fn a_field_given_to_a_shared_parameter_or_receiver_is_lent() {
    let module = lowered(STORE);
    checks(&module).expect("the lent module should check");
    let count = function(&module, "Store.count");
    assert_eq!(lends(count), ["self.docs", "self.docs"]);
    // The callee owns its parameter, so the shared temporary is retained
    // for the call and moved back afterwards.
    let Some(Instruction::Lend { temp, .. }) = instructions(count).next() else {
        panic!("the first instruction should lend: {count:#?}");
    };
    assert_eq!(retained(count), [temp.as_str()]);
}

#[test]
fn a_mut_parameter_whose_writeback_is_the_field_takes_a_mutable_lend() {
    let module = lowered(STORE);
    let add = function(&module, "Store.add");
    assert_eq!(
        lends(add),
        [
            "mut self.docs",
            "mut self.by_id",
            "mut self.docs",
            "mut self.by_id"
        ]
    );
    // The call takes the temporary and returns it through the writeback, so
    // no temporary is retained.
    let temps = instructions(add)
        .filter_map(|instruction| match instruction {
            Instruction::Lend { temp, .. } => Some(temp.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert!(
        retained(add).iter().all(|place| !temps.contains(place)),
        "{add:#?}"
    );
}

#[test]
fn a_place_another_operand_reaches_is_not_lent() {
    let module = lowered(STORE);
    // Both arguments read the one field.
    assert!(lends(function(&module, "Store.twice")).is_empty());
    // An argument holding an instance could be the field's owner.
    assert!(lends(function(&module, "Store.with_owner")).is_empty());
}

#[test]
fn a_covered_element_read_is_lent_before_its_fact() {
    let module = lowered(STORE);
    let scan = function(&module, "Store.scan");
    assert_eq!(lends(scan), ["self.docs", "self.docs"]);
    let block = scan
        .blocks
        .iter()
        .find(|block| {
            block
                .instructions
                .iter()
                .any(|instruction| matches!(instruction, Instruction::IndexInRange { .. }))
        })
        .expect("the loop should prove its reads");
    let [Instruction::Lend { temp, .. }, Instruction::IndexInRange { list, .. }, ..] =
        block.instructions.as_slice()
    else {
        panic!("the lend should come before the fact: {block:#?}");
    };
    assert_eq!(list, temp);
}

#[test]
fn an_element_a_loan_selects_is_lent_to_a_method() {
    let module = lowered(REGISTRY);
    checks(&module).expect("the lent module should check");
    assert!(lends(function(&module, "Registry.size")).contains(&"%t1".to_string()));
    assert!(lends(function(&module, "Registry.add")).contains(&"mut %t7".to_string()));
}

#[test]
fn stripping_undoes_the_lends_and_the_pass_makes_them_again() {
    for source in [STORE, REGISTRY] {
        let module = lowered(source);
        let mut stripped = module.clone();
        strip_ownership(&mut stripped);
        assert!(stripped
            .functions
            .iter()
            .all(|function| lends(function).is_empty()));
        checks(&stripped).expect("the stripped module should check");
        insert_ownership(&mut stripped);
        assert_eq!(json(&stripped), json(&module));
    }
}

#[test]
fn the_interpreter_runs_lends() {
    for (source, expected) in [(STORE, "8\n6\n8\n4\n"), (REGISTRY, "2\n")] {
        let module = lowered(source);
        let output = crate::mir_runtime::run(&module).expect("the lent module should run");
        assert_eq!(output.stdout, expected);
    }
}

/// `function` with `edit` applied to its instruction list in `block`.
fn forged(source: &str, name: &str, edit: impl Fn(&mut Vec<Instruction>)) -> Result<(), String> {
    let mut module = lowered(source);
    let function = function_mut(&mut module, name);
    let block = function
        .blocks
        .iter_mut()
        .find(|block| {
            block
                .instructions
                .iter()
                .any(|instruction| matches!(instruction, Instruction::Lend { .. }))
        })
        .expect("a block should lend");
    edit(&mut block.instructions);
    checks(&module)
}

fn position(instructions: &[Instruction], lend: bool) -> usize {
    instructions
        .iter()
        .position(|instruction| match instruction {
            Instruction::Lend { .. } => lend,
            Instruction::EndLend { .. } => !lend,
            _ => false,
        })
        .expect("the block should lend")
}

#[test]
fn the_validator_refuses_forged_lends() {
    // A lend that never ends.
    let error = forged(STORE, "Store.count", |instructions| {
        instructions.remove(position(instructions, false));
    })
    .expect_err("an open lend should be refused");
    assert!(error.contains("does not end the lend"), "{error}");
    // An end with no lend.
    let error = forged(STORE, "Store.count", |instructions| {
        instructions.remove(position(instructions, true));
    })
    .expect_err("a lone end should be refused");
    assert!(error.contains("never began"), "{error}");
    // A shared lend of a field the call changes.
    let error = forged(STORE, "Store.add", |instructions| {
        let first = position(instructions, true);
        let end = position(instructions, false);
        for index in [first, end] {
            if let Instruction::Lend { mutable, .. } | Instruction::EndLend { mutable, .. } =
                &mut instructions[index]
            {
                *mutable = false;
            }
        }
    })
    .expect_err("a shared lend to a mut parameter should be refused");
    assert!(error.contains("could reach another way"), "{error}");
    // A lend around something other than a call.
    let error = forged(STORE, "Store.count", |instructions| {
        let first = position(instructions, true);
        instructions.insert(first + 1, Instruction::Safepoint);
    })
    .expect_err("a lend around a safepoint should be refused");
    assert!(error.contains("other than a call"), "{error}");
}

#[test]
fn the_checker_refuses_an_unretained_shared_lend() {
    let error = forged(STORE, "Store.count", |instructions| {
        let retain = instructions
            .iter()
            .position(|instruction| matches!(instruction, Instruction::Retain { .. }))
            .expect("the shared call retains its temporary");
        instructions.remove(retain);
    })
    .expect_err("a consumed temporary needs its retain");
    assert!(error.contains("without a retain"), "{error}");
}

#[test]
fn an_implicit_module_holds_no_lend() {
    let mut module = lowered(STORE);
    module.ownership = super::super::MirOwnership::Implicit;
    for function in &mut module.functions {
        for block in &mut function.blocks {
            block.instructions.retain(|instruction| {
                !matches!(
                    instruction,
                    Instruction::Retain { .. } | Instruction::Release { .. }
                )
            });
        }
    }
    let error = validate_loan_flow(&module).expect_err("an implicit lend should be refused");
    assert!(error.contains("states ownership"), "{error}");
}
