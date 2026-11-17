use std::hint::black_box;
use std::time::Instant;

fn measure_local(seed: i64, count: i64) {
    let step = |value: i64| (value * 3 + seed) & 1_048_575;
    let mut result = 1;
    let start = Instant::now();
    for _ in 0..count {
        result = step(result);
    }
    println!("local {count} {} {result}", start.elapsed().as_millis());
}

fn main() {
    measure_local(black_box(7), black_box(48_000_000));
}
