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

fn main() -> Result<(), std::num::ParseIntError> {
    let mut args = std::env::args().skip(1);
    let seed = args.next().as_deref().unwrap_or("7").parse()?;
    let count = args.next().as_deref().unwrap_or("48000000").parse()?;
    measure_local(seed, count);
    Ok(())
}
