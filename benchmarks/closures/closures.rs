// The Rust side of the stored closure benchmark. Every closure is a
// `Box<dyn Fn>` or `Box<dyn FnMut>`, the owned storage Aura's
// `Callable[...]` corresponds to.
use std::hint::black_box;
use std::time::Instant;

type Step = Box<dyn Fn(i64) -> i64>;
type Tally = Box<dyn FnMut(i64) -> i64>;

struct Total {
    sum: i64,
}

impl Total {
    fn add(&mut self, amount: i64) -> i64 {
        self.sum += amount;
        self.sum
    }
}

struct Rule {
    weight: i64,
    step: Step,
}

#[inline(never)]
fn make_adder(amount: i64) -> Step {
    Box::new(move |value| value + amount)
}

#[inline(never)]
fn apply(step: &Step, value: i64) -> i64 {
    step(value)
}

#[inline(never)]
fn running_total() -> Tally {
    let mut total = Total { sum: 0 };
    Box::new(move |amount| total.add(amount))
}

fn report(name: &str, calls: i64, start: Instant, acc: i64) {
    println!("{name} {calls} {} {acc}", start.elapsed().as_millis());
}

fn list_call(count: i64, reps: i64) {
    let mut steps: Vec<Step> = Vec::new();
    for index in 0..count {
        if index % 2 == 0 {
            steps.push(make_adder(index));
        } else {
            steps.push(Box::new(move |value| value * 3 + index));
        }
    }
    let mut acc = 0i64;
    let start = Instant::now();
    for _ in 0..reps {
        for step in &steps {
            acc = step(acc) & 1048575;
        }
    }
    report("list_call", count * reps, start, acc);
}

fn field_call(calls: i64) {
    let rule = black_box(Rule { weight: 3, step: make_adder(7) });
    let mut acc = 0i64;
    let start = Instant::now();
    for _ in 0..calls {
        acc = (rule.step)(acc + rule.weight) & 1048575;
    }
    report("field_call", calls, start, acc);
}

fn mutable_list_call(count: i64, reps: i64) {
    let mut tallies: Vec<Tally> = Vec::new();
    for _ in 0..count {
        tallies.push(running_total());
    }
    let mut acc = 0i64;
    let start = Instant::now();
    for rep in 0..reps {
        for tally in tallies.iter_mut() {
            acc = (acc + tally(rep & 7)) & 1048575;
        }
    }
    report("mutable_list_call", count * reps, start, acc);
}

fn mutable_call(calls: i64) {
    let mut tally = black_box(running_total());
    let mut acc = 0i64;
    let start = Instant::now();
    for index in 0..calls {
        acc = tally(index & 7);
    }
    report("mutable_call", calls, start, acc);
}

fn create_call(calls: i64) {
    let mut acc = 0i64;
    let start = Instant::now();
    for index in 0..calls {
        let step = make_adder(index & 15);
        acc = step(acc) & 1048575;
    }
    report("create_call", calls, start, acc);
}

fn argument_call(calls: i64) {
    let mut acc = 0i64;
    let start = Instant::now();
    for index in 0..calls {
        let step: Step = Box::new(move |value| value + index);
        acc = apply(&step, acc) & 1048575;
    }
    report("argument_call", calls, start, acc);
}

fn main() {
    list_call(black_box(64), 500_000);
    field_call(black_box(30_000_000));
    mutable_list_call(black_box(16), 1_000_000);
    mutable_call(black_box(30_000_000));
    create_call(black_box(10_000_000));
    argument_call(black_box(10_000_000));
}
