//! The Rust side of the Array view benchmark: each row written the way a
//! Rust program writes it first, with slices, `chunks`, and `step_by` over
//! one `Vec<f64>` in row-major order.

use std::time::Instant;

fn matrix(n: usize) -> Vec<f64> {
    (0..n * n).map(|i| (i % 97) as f64 * 0.25 + 1.0).collect()
}

/// Eight partial sums, the reordered sum a Rust program writes by hand.
fn fast_sum(values: impl Iterator<Item = f64>) -> f64 {
    let mut partial = [0.0_f64; 8];
    for (index, value) in values.enumerate() {
        partial[index % 8] += value;
    }
    partial.iter().sum()
}

fn report(name: &str, size: usize, start: Instant, acc: f64) {
    println!("{name} {size} {} {acc}", start.elapsed().as_millis());
}

fn row_sums(n: usize, reps: usize) {
    let m = matrix(n);
    let mut acc = 0.0;
    let start = Instant::now();
    for _ in 0..reps {
        for row in m.chunks(n) {
            acc += fast_sum(row.iter().copied());
        }
    }
    report("row_sums", n, start, acc);
}

fn column_sums(n: usize, reps: usize) {
    let m = matrix(n);
    let mut acc = 0.0;
    let start = Instant::now();
    for _ in 0..reps {
        for j in 0..n {
            acc += fast_sum(m[j..].iter().step_by(n).copied());
        }
    }
    report("column_sums", n, start, acc);
}

fn transposed_add(n: usize, reps: usize) {
    let m = matrix(n);
    let mut acc = 0.0;
    let start = Instant::now();
    for _ in 0..reps {
        let mut sum = vec![0.0; n * n];
        for i in 0..n {
            for j in 0..n {
                sum[i * n + j] = m[j * n + i] + m[i * n + j];
            }
        }
        acc += std::hint::black_box(&sum)[(n - 1) * n];
    }
    report("transposed_add", n, start, acc);
}

fn stencil(n: usize, reps: usize) {
    let mut u: Vec<f64> = (0..n).map(|i| (i % 1000) as f64).collect();
    let start = Instant::now();
    for _ in 0..reps {
        let next: Vec<f64> = u[..n - 2]
            .iter()
            .zip(&u[2..])
            .map(|(left, right)| (left + right) * 0.5)
            .collect();
        u[1..n - 1].copy_from_slice(&next);
    }
    report("stencil", n, start, u[n / 2]);
}

fn row_swaps(n: usize, reps: usize) {
    let mut m = matrix(n);
    let start = Instant::now();
    for _ in 0..reps {
        for i in 0..n / 2 {
            let (upper, lower) = m.split_at_mut((n - 1 - i) * n);
            upper[i * n..(i + 1) * n].swap_with_slice(&mut lower[..n]);
        }
    }
    report("row_swaps", n, start, m[1] + m[(n - 1) * n + 2]);
}

fn column_scale(n: usize, reps: usize) {
    let mut m = matrix(n);
    let start = Instant::now();
    for _ in 0..reps {
        for j in 0..n {
            for value in m[j..].iter_mut().step_by(n) {
                *value *= 1.0001;
            }
        }
    }
    report("column_scale", n, start, m[n * n - 1]);
}

fn reshape_reverse(n: usize, reps: usize) {
    let m = matrix(n);
    let mut acc = 0.0;
    let start = Instant::now();
    for rep in 0..reps {
        let flat = std::hint::black_box(&m[..]);
        acc += flat[n * n - 1 - rep % n] + fast_sum(flat.iter().step_by(2).copied());
    }
    report("reshape_reverse", n, start, acc);
}

fn main() {
    row_sums(512, 200);
    column_sums(512, 200);
    transposed_add(512, 200);
    stencil(1_000_000, 200);
    row_swaps(512, 400);
    column_scale(512, 100);
    reshape_reverse(512, 400);
}
