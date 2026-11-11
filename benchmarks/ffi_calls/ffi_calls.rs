//! The Rust side of the FFI call-overhead benchmark: the same calls as
//! `src/main.au`, through `extern "C"` declarations.

use std::ffi::{c_char, CStr, CString};
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
    fn fc_cstr(text: *const c_char) -> u64;
    fn fc_name() -> *const c_char;
    fn fc_out(value: i64, out: *mut i64) -> i32;
    fn fc_floats(values: *const f64, count: i64) -> f64;
    fn fc_bump(values: *mut f64, count: i64);
    fn fc_vec2_add(a: Vec2, b: Vec2) -> Vec2;
    fn fc_triple_next(t: Triple) -> Triple;
    fn fc_vec2_move(v: *mut Vec2, by: f64);
    fn fc_apply(f: extern "C" fn(i64, i32) -> i64, value: i64) -> i64;
}

extern "C" fn advance(value: i64, step: i32) -> i64 {
    value + step as i64
}

#[repr(C)]
#[derive(Clone, Copy)]
struct Vec2 {
    x: f64,
    y: f64,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct Triple {
    a: i64,
    b: i64,
    c: i64,
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

    // A `String` has no terminator, so each call makes a `CString`, as an
    // Aura `cstr` argument does.
    let mut cstr_total: u64 = 0;
    let started = Instant::now();
    for _ in 0..calls {
        let terminated = CString::new(text.as_str()).unwrap();
        cstr_total += unsafe { fc_cstr(terminated.as_ptr()) };
    }
    report("cstr", started, calls);

    // The result is copied into an owned `String`, as an Aura `cstr` result is.
    let mut name_total: i64 = 0;
    let started = Instant::now();
    for _ in 0..calls {
        let name = unsafe { CStr::from_ptr(fc_name()) }.to_str().unwrap().to_owned();
        name_total += name.len() as i64;
    }
    report("name", started, calls);

    let mut out: i64 = 0;
    let mut status: i32 = 0;
    let started = Instant::now();
    for i in 0..calls {
        status += unsafe { fc_out(i, &mut out) };
    }
    report("out", started, calls);

    let mut floats: Vec<f64> = vec![1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0];
    let mut floats_total: f64 = 0.0;
    let started = Instant::now();
    for _ in 0..calls {
        floats_total += unsafe { fc_floats(floats.as_ptr(), floats.len() as i64) };
    }
    report("floats", started, calls);

    let started = Instant::now();
    for _ in 0..calls {
        unsafe { fc_bump(floats.as_mut_ptr(), floats.len() as i64) };
    }
    report("bump", started, calls);

    let step = Vec2 { x: 0.5, y: -0.25 };
    let mut point = Vec2 { x: 0.0, y: 0.0 };
    let started = Instant::now();
    for _ in 0..calls {
        point = unsafe { fc_vec2_add(point, step) };
    }
    report("vec2", started, calls);

    let mut triple = Triple { a: 1, b: 2, c: 3 };
    let started = Instant::now();
    for _ in 0..calls {
        triple = unsafe { fc_triple_next(triple) };
    }
    report("triple", started, calls);

    let started = Instant::now();
    for _ in 0..calls {
        unsafe { fc_vec2_move(&mut point, 0.5) };
    }
    report("move", started, calls);

    let mut applied: i64 = 0;
    let started = Instant::now();
    for _ in 0..calls {
        applied = unsafe { fc_apply(advance, applied) };
    }
    report("callback", started, calls);

    println!(
        "checksum {total} {narrow} {scaled} {six} {text_total} {bytes_total} {}",
        bytes[0]
    );
    println!(
        "checksum {cstr_total} {name_total} {out} {status} {floats_total} {}",
        floats[7]
    );
    println!(
        "checksum {:?} {:?} {} {} {} {applied}",
        point.x, point.y, triple.a, triple.b, triple.c
    );
}
