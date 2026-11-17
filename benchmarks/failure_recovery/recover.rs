use std::hint::black_box;
use std::panic::{catch_unwind, set_hook};
use std::time::Instant;

fn checked(value: i64) -> i64 {
    assert!(value >= 0, "negative input");
    value
}

fn main() {
    set_hook(Box::new(|_| {}));
    let passes = 1_000_000;
    let failures = 100_000;
    let samples: Vec<i64> = (0..256).map(|i| (i * 37) % 257).collect();
    let samples = black_box(samples.as_slice());
    let mut sum = 0;
    let start = Instant::now();
    for i in 0..passes {
        sum += catch_unwind(|| checked(samples[i & 255])).expect("expected success");
    }
    let elapsed = start.elapsed().as_secs_f64() * 1000.0;
    assert_eq!(sum, 127639971);
    println!("success {passes} {elapsed:.3} {sum}");

    let mut trapped = 0;
    let start = Instant::now();
    for i in 0..failures {
        if catch_unwind(|| checked(-samples[i & 255] - 1)).is_err() {
            trapped += 1;
        }
    }
    let elapsed = start.elapsed().as_secs_f64() * 1000.0;
    assert_eq!(trapped, failures);
    println!("trap {failures} {elapsed:.3} {trapped}");
}
