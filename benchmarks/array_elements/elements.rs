//! The Rust side of the Array element benchmark: each row written the way
//! a Rust program writes it first, with index arithmetic over one
//! `Vec<f64>` in row-major order.

use std::hint::black_box;
use std::time::Instant;

fn matrix(n: usize) -> Vec<f64> {
    (0..n * n).map(|i| (i % 97) as f64 * 0.25 + 1.0).collect()
}

fn report(name: &str, size: impl std::fmt::Display, start: Instant, acc: f64) {
    println!("{name} {size} {} {acc}", start.elapsed().as_millis());
}

fn element_sum(n: usize, reps: usize) {
    let m = matrix(n);
    let mut acc = 0.0;
    let start = Instant::now();
    for _ in 0..reps {
        for i in 0..n {
            for j in 0..n {
                acc += m[i * n + j];
            }
        }
    }
    report("element_sum", n, start, acc);
}

fn element_fill(n: usize, reps: usize) {
    let mut m = vec![0.0_f64; n * n];
    let start = Instant::now();
    for rep in 0..reps {
        for i in 0..n {
            for j in 0..n {
                m[i * n + j] = ((i + j + rep) % 7) as f64;
            }
        }
        black_box(&mut m);
    }
    report("element_fill", n, start, m.iter().sum());
}

fn stencil_2d(n: usize, reps: usize) {
    let mut grid = vec![0.0_f64; n * n];
    let mut next = vec![0.0_f64; n * n];
    for i in 0..n {
        grid[i * n] = 100.0;
    }
    let start = Instant::now();
    for _ in 0..reps {
        for i in 1..n - 1 {
            for j in 1..n - 1 {
                next[i * n + j] = 0.25
                    * (grid[(i - 1) * n + j]
                        + grid[(i + 1) * n + j]
                        + grid[i * n + j - 1]
                        + grid[i * n + j + 1]);
            }
        }
        for i in 1..n - 1 {
            for j in 1..n - 1 {
                grid[i * n + j] = next[i * n + j];
            }
        }
    }
    report("stencil_2d", n, start, grid.iter().sum());
}

fn center_columns(n: usize, reps: usize) {
    let m = matrix(n);
    let mut acc = 0.0;
    let start = Instant::now();
    for _ in 0..reps {
        let mut means = vec![0.0_f64; n];
        for row in m.chunks(n) {
            for (mean, value) in means.iter_mut().zip(row) {
                *mean += value;
            }
        }
        for mean in &mut means {
            *mean /= n as f64;
        }
        let mut centered = vec![0.0_f64; n * n];
        for (out, row) in centered.chunks_mut(n).zip(m.chunks(n)) {
            for ((out, value), mean) in out.iter_mut().zip(row).zip(&means) {
                *out = value - mean;
            }
        }
        acc += black_box(&centered)[(n - 1) * n];
    }
    report("center_columns", n, start, acc);
}

fn mask_choose(n: usize, reps: usize) {
    let m = matrix(n);
    let mut acc = 0.0;
    let mut count = 0_usize;
    let start = Instant::now();
    for _ in 0..reps {
        let large: Vec<bool> = m.iter().map(|value| *value > 12.0).collect();
        count += black_box(&large).iter().filter(|flag| **flag).count();
        let chosen: Vec<f64> = large
            .iter()
            .zip(&m)
            .map(|(flag, value)| if *flag { *value } else { 12.0 })
            .collect();
        acc += black_box(&chosen)[n * n - 1];
    }
    report("mask_choose", n, start, acc + count as f64);
}

fn sqrt_exp(n: usize, reps: usize) {
    let m = matrix(n);
    let mut acc = 0.0;
    let start = Instant::now();
    for _ in 0..reps {
        let roots: Vec<f64> = m.iter().map(|value| value.sqrt() * 0.125).collect();
        let powers: Vec<f64> = black_box(&roots).iter().map(|value| value.exp()).collect();
        acc += black_box(&powers).iter().sum::<f64>();
    }
    report("sqrt_exp", n, start, acc);
}

fn batched_matmul(batch: usize, n: usize, reps: usize) {
    let stack: Vec<f64> = (0..batch * n * n)
        .map(|i| (i % 13) as f64 * 0.5 - 2.0)
        .collect();
    let weights = matrix(n);
    let mut acc = 0.0;
    let start = Instant::now();
    for _ in 0..reps {
        let mut product = vec![0.0_f64; batch * n * n];
        for (out, left) in product.chunks_mut(n * n).zip(stack.chunks(n * n)) {
            for i in 0..n {
                for k in 0..n {
                    let scale = left[i * n + k];
                    let row = &weights[k * n..(k + 1) * n];
                    for (out, value) in out[i * n..(i + 1) * n].iter_mut().zip(row) {
                        *out += scale * value;
                    }
                }
            }
        }
        acc += black_box(&product)[batch * n * n - 1];
    }
    report("batched_matmul", format!("{batch}x{n}"), start, acc);
}

fn lane_dot(n: usize, reps: usize) {
    let xs = matrix(n);
    let ys: Vec<f64> = matrix(n).iter().map(|value| value * 0.5).collect();
    let mut acc = 0.0;
    let start = Instant::now();
    for _ in 0..reps {
        let mut lanes = [0.0_f64; 4];
        for (x, y) in xs.chunks_exact(4).zip(ys.chunks_exact(4)) {
            for lane in 0..4 {
                lanes[lane] += x[lane] * y[lane];
            }
        }
        acc += black_box(lanes).iter().sum::<f64>();
    }
    report("lane_dot", n, start, acc);
}

fn shared_tasks(n: usize, reps: usize) {
    let m = std::sync::Arc::new(matrix(n));
    let mut acc = 0.0;
    let start = Instant::now();
    for _ in 0..reps {
        let handles: Vec<_> = (0..4)
            .map(|_| {
                let m = std::sync::Arc::clone(&m);
                std::thread::spawn(move || {
                    let mut partial = [0.0_f64; 8];
                    for chunk in m.chunks_exact(8) {
                        for lane in 0..8 {
                            partial[lane] += chunk[lane];
                        }
                    }
                    partial.iter().sum::<f64>()
                })
            })
            .collect();
        for handle in handles {
            acc += handle.join().unwrap();
        }
    }
    report("shared_tasks", n, start, acc);
}

fn main() {
    let size = black_box(1000);
    element_sum(size, 100);
    element_fill(size, 100);
    stencil_2d(size, 50);
    center_columns(size, 100);
    mask_choose(size, 100);
    sqrt_exp(size, 20);
    batched_matmul(black_box(512), 32, 20);
    lane_dot(size, 100);
    shared_tasks(black_box(2000), 100);
}
