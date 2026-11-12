// The Rust side of the Array kernel benchmark. Each kernel is written the
// way a Rust program writes it first: iterator sums and plain loops over
// slices, with no reordering the language does not allow.
use std::hint::black_box;
use std::time::Instant;

fn vector(n: usize, scale: f64, shift: f64) -> Vec<f64> {
    (0..n).map(|i| (i % 1000) as f64 * scale + shift).collect()
}

fn report(name: &str, size: usize, start: Instant, acc: f64) {
    println!("{name} {size} {} {acc}", start.elapsed().as_millis());
}

fn reductions(n: usize, reps: usize) {
    let a = vector(n, 0.5, 1.0);
    let b = vector(n, 0.25, -3.0);

    let mut acc = 0.0;
    let start = Instant::now();
    for _ in 0..reps {
        acc += black_box(&a).iter().sum::<f64>();
    }
    report("sum", n, start, acc);

    // Rust has no reordered sum in the standard library, so the eight
    // partial sums are written by hand.
    let mut acc = 0.0;
    let start = Instant::now();
    for _ in 0..reps {
        let a = black_box(&a);
        let mut partial = [0.0f64; 8];
        let chunks = a.chunks_exact(8);
        let tail = chunks.remainder();
        for chunk in chunks {
            for lane in 0..8 {
                partial[lane] += chunk[lane];
            }
        }
        acc += partial.iter().sum::<f64>() + tail.iter().sum::<f64>();
    }
    report("fast_sum", n, start, acc);

    let mut acc = 0.0;
    let start = Instant::now();
    for _ in 0..reps {
        acc += black_box(&a)
            .iter()
            .zip(black_box(&b))
            .map(|(x, y)| x * y)
            .sum::<f64>();
    }
    report("dot", n, start, acc);

    let mut acc = 0.0;
    let start = Instant::now();
    for _ in 0..reps {
        let a = black_box(&a);
        let low = a.iter().copied().fold(f64::INFINITY, f64::min);
        let high = a.iter().copied().fold(f64::NEG_INFINITY, f64::max);
        acc += low + high;
    }
    report("min_max", n, start, acc);

    let mut acc = 0.0;
    let start = Instant::now();
    for _ in 0..reps {
        let c: Vec<f64> = black_box(&a)
            .iter()
            .zip(black_box(&b))
            .map(|(x, y)| x + y)
            .collect();
        acc += c[n - 1];
    }
    report("add", n, start, acc);
}

fn matmul(left: &[f64], right: &[f64], m: usize) -> Vec<f64> {
    let mut out = vec![0.0; m * m];
    for i in 0..m {
        let row = &mut out[i * m..(i + 1) * m];
        for k in 0..m {
            let scale = left[i * m + k];
            let other = &right[k * m..(k + 1) * m];
            for (cell, value) in row.iter_mut().zip(other) {
                *cell += scale * value;
            }
        }
    }
    out
}

fn products(m: usize, reps: usize) {
    let p: Vec<f64> = (0..m * m).map(|i| (i % 97) as f64 * 0.01).collect();
    let mut acc = 0.0;
    let start = Instant::now();
    for _ in 0..reps {
        let q = matmul(black_box(&p), black_box(&p), m);
        acc += q[m * m - 1];
    }
    report("matmul", m, start, acc);
}

fn main() {
    reductions(1024, 400000);
    reductions(1000000, 300);
    products(64, 2000);
    products(512, 4);
}
