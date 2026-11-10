//! The Rust side of the FFI call-overhead benchmark: the same calls as
//! `src/main.au`, through `extern "C"` declarations.

use std::time::Instant;

#[link(name = "ffi_calls")]
extern "C" {
    fn fc_add(a: i64, b: i64) -> i64;
    fn fc_narrow(a: i32, b: u8) -> i32;
    fn fc_scale(value: f64, factor: f32) -> f64;
    fn fc_six(a: i64, b: i64, c: i64, d: i64, e: i64, f: i64) -> i64;
    fn fc_text(text: *const u8, length: u64) -> u64;
    fn fc_bytes(bytes: *const u8, length: u64) -> u64;
    fn fc_fill(bytes: *mut u8, length: u64);
}

fn report(name: &str, started: Instant, calls: i64) {
    let per_call = started.elapsed().as_nanos() as f64 / calls as f64;
    println!("{name} {per_call}");
}

fn main() {
    let calls: i64 = 50_000_000;

    let mut total: i64 = 0;
    let started = Instant::now();
    for i in 0..calls {
        total = unsafe { fc_add(total, i) };
    }
    report("add", started, calls);

    let mut narrow: i32 = 0;
    let started = Instant::now();
    for _ in 0..calls {
        narrow = unsafe { fc_narrow(narrow % 1000, 3) };
    }
    report("narrow", started, calls);

    let mut scaled: f64 = 1.0;
    let started = Instant::now();
    for _ in 0..calls {
        scaled = unsafe { fc_scale(scaled, 1.0) };
    }
    report("scale", started, calls);

    let mut six: i64 = 0;
    let started = Instant::now();
    for i in 0..calls {
        six = unsafe { fc_six(six, i, 1, 2, 3, 4) };
    }
    report("six", started, calls);

    let text = String::from("hello, world");
    let mut text_total: u64 = 0;
    let started = Instant::now();
    for _ in 0..calls {
        text_total += unsafe { fc_text(text.as_ptr(), text.len() as u64) };
    }
    report("text", started, calls);

    let mut bytes: Vec<u8> = vec![1, 2, 3, 4, 5, 6, 7, 8];
    let mut bytes_total: u64 = 0;
    let started = Instant::now();
    for _ in 0..calls {
        bytes_total += unsafe { fc_bytes(bytes.as_ptr(), bytes.len() as u64) };
    }
    report("bytes", started, calls);

    let started = Instant::now();
    for _ in 0..calls {
        unsafe { fc_fill(bytes.as_mut_ptr(), bytes.len() as u64) };
    }
    report("fill", started, calls);

    println!(
        "checksum {total} {narrow} {scaled} {six} {text_total} {bytes_total} {}",
        bytes[0]
    );
}
