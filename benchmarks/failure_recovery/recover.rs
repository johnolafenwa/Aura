use std::hint::black_box;
use std::panic::{catch_unwind, set_hook};
use std::time::Instant;

#[inline(never)]
fn checked(value: i64) -> i64 {
    assert!(value >= 0, "negative input");
    value
}

fn main() {
    set_hook(Box::new(|_| {}));
    let passes = 1_000_000;
    let failures = 100_000;
    let mut sum = 0;
    let start = Instant::now();
    for i in 0..passes {
        sum += catch_unwind(|| checked(black_box(i))).expect("expected success");
    }
    let elapsed = start.elapsed().as_secs_f64() * 1000.0;
    assert_eq!(sum, passes * (passes - 1) / 2);
    println!("success {passes} {elapsed:.3} {sum}");

    let mut trapped = 0;
    let start = Instant::now();
    for i in 0..failures {
        if catch_unwind(|| checked(black_box(-i - 1))).is_err() {
            trapped += 1;
        }
    }
    let elapsed = start.elapsed().as_secs_f64() * 1000.0;
    assert_eq!(trapped, failures);
    println!("trap {failures} {elapsed:.3} {trapped}");
}
