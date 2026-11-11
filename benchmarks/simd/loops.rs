// The Rust side of the list loop benchmark: the same indexed loops over
// `Vec<f64>`, with the bounds checks Rust keeps.
use std::hint::black_box;
use std::time::Instant;

#[inline(never)]
fn scale(xs: &mut [f64], factor: f64) {
    for i in 0..xs.len() {
        xs[i] *= factor;
    }
}

#[inline(never)]
fn saxpy(a: f64, xs: &[f64], ys: &mut [f64]) {
    for i in 0..xs.len() {
        ys[i] = a * xs[i] + ys[i];
    }
}

#[inline(never)]
fn blur(xs: &[f64], out: &mut [f64]) {
    for i in 1..xs.len() - 1 {
        out[i] = (xs[i - 1] + xs[i] + xs[i + 1]) / 3.0;
    }
}

#[inline(never)]
fn clamp(xs: &mut [f64], low: f64, high: f64) {
    for i in 0..xs.len() {
        let x = xs[i];
        if x < low {
            xs[i] = low;
        } else if x > high {
            xs[i] = high;
        }
    }
}

#[inline(never)]
fn dot(xs: &[f64], ys: &[f64]) -> f64 {
    let mut total = 0.0;
    for i in 0..xs.len() {
        total += xs[i] * ys[i];
    }
    total
}

fn checksum(xs: &[f64]) -> f64 {
    xs.iter().sum()
}

fn run(n: usize, reps: usize) {
    let xs: Vec<f64> = (0..n).map(|i| (i % 1000) as f64 * 0.5 + 1.0).collect();
    let mut ys: Vec<f64> = (0..n).map(|i| (i % 1000) as f64 * 0.25 - 3.0).collect();
    let mut out = vec![0.0; n];

    let start = Instant::now();
    for _ in 0..reps {
        scale(black_box(&mut ys), 1.0000001);
    }
    println!("scale {n} {} {:?}", start.elapsed().as_millis(), checksum(&ys));

    let start = Instant::now();
    for _ in 0..reps {
        saxpy(0.0000001, black_box(&xs), black_box(&mut ys));
    }
    println!("saxpy {n} {} {:?}", start.elapsed().as_millis(), checksum(&ys));

    let start = Instant::now();
    for _ in 0..reps {
        blur(black_box(&ys), black_box(&mut out));
    }
    println!("blur {n} {} {:?}", start.elapsed().as_millis(), checksum(&out));

    let start = Instant::now();
    for _ in 0..reps {
        clamp(black_box(&mut out), -1.0, 200.0);
    }
    println!("clamp {n} {} {:?}", start.elapsed().as_millis(), checksum(&out));

    let mut acc = 0.0;
    let start = Instant::now();
    for _ in 0..reps {
        acc += dot(black_box(&xs), black_box(&ys));
    }
    println!("dot {n} {} {acc:?}", start.elapsed().as_millis());
}

fn main() {
    run(1024, 400000);
    run(1000000, 300);
}
