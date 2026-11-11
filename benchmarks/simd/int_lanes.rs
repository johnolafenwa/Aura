// The Rust side of the integer lane benchmark: the same kernels over a
// lane struct whose sum checks each lane for overflow, as
// Aura's do, and the same kernels as plain checked loops.
use std::hint::black_box;
use std::time::Instant;

#[derive(Clone, Copy)]
struct I64x4([i64; 4]);

impl I64x4 {
    #[inline(always)]
    fn splat(value: i64) -> Self {
        Self([value; 4])
    }
    #[inline(always)]
    fn load(values: &[i64], at: usize) -> Self {
        let lanes = &values[at..at + 4];
        Self([lanes[0], lanes[1], lanes[2], lanes[3]])
    }
    #[inline(always)]
    fn store(self, values: &mut [i64], at: usize) {
        values[at..at + 4].copy_from_slice(&self.0);
    }
    #[inline(always)]
    fn zip(self, other: Self, op: impl Fn(i64, i64) -> i64) -> Self {
        Self([
            op(self.0[0], other.0[0]),
            op(self.0[1], other.0[1]),
            op(self.0[2], other.0[2]),
            op(self.0[3], other.0[3]),
        ])
    }
    #[inline(always)]
    fn add(self, other: Self) -> Self {
        self.zip(other, |a, b| a.checked_add(b).expect("overflow"))
    }
    #[inline(always)]
    fn sum(self) -> i64 {
        let low = self.0[0].checked_add(self.0[1]).expect("overflow");
        let high = self.0[2].checked_add(self.0[3]).expect("overflow");
        low.checked_add(high).expect("overflow")
    }
}

#[inline(never)]
fn add(xs: &[i64], ys: &[i64], out: &mut [i64]) {
    let mut i = 0;
    while i + 4 <= xs.len() {
        I64x4::load(xs, i).add(I64x4::load(ys, i)).store(out, i);
        i += 4;
    }
}

#[inline(never)]
fn add_scalar(xs: &[i64], ys: &[i64], out: &mut [i64]) {
    for i in 0..xs.len() {
        out[i] = xs[i].checked_add(ys[i]).expect("overflow");
    }
}

#[inline(never)]
fn total(xs: &[i64]) -> i64 {
    let mut acc = I64x4::splat(0);
    let mut i = 0;
    while i + 4 <= xs.len() {
        acc = acc.add(I64x4::load(xs, i));
        i += 4;
    }
    acc.sum()
}

#[inline(never)]
fn total_scalar(xs: &[i64]) -> i64 {
    let mut sum = 0i64;
    for &x in xs {
        sum = sum.checked_add(x).expect("overflow");
    }
    sum
}

fn main() {
    let n = 1024usize;
    let reps = 400_000;
    let xs: Vec<i64> = (0..n as i64).map(|i| (i % 1000) * 3 + 1).collect();
    let ys: Vec<i64> = (0..n as i64).map(|i| (i % 700) * 5 - 3).collect();
    let mut out = vec![0i64; n];

    let start = Instant::now();
    for _ in 0..reps {
        add(black_box(&xs), black_box(&ys), &mut out);
    }
    println!("add {n} {} {}", start.elapsed().as_millis(), out[7]);
    let start = Instant::now();
    for _ in 0..reps {
        add_scalar(black_box(&xs), black_box(&ys), &mut out);
    }
    println!("add_scalar {n} {} {}", start.elapsed().as_millis(), out[7]);

    let mut acc = 0i64;
    let start = Instant::now();
    for _ in 0..reps {
        acc += total(black_box(&xs));
    }
    println!("total {n} {} {acc}", start.elapsed().as_millis());
    acc = 0;
    let start = Instant::now();
    for _ in 0..reps {
        acc += total_scalar(black_box(&xs));
    }
    println!("total_scalar {n} {} {acc}", start.elapsed().as_millis());
}
