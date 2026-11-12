// The Rust side of the lane value benchmark: the same loops over lane
// structs with one field for each lane, which is how stable Rust writes
// them without a vendor intrinsic. Each load and store keeps the bounds
// check of its slice.
use std::hint::black_box;
use std::time::Instant;

#[derive(Clone, Copy)]
struct F64x4([f64; 4]);

impl F64x4 {
    #[inline(always)]
    fn splat(value: f64) -> Self {
        Self([value; 4])
    }
    #[inline(always)]
    fn load(values: &[f64], at: usize) -> Self {
        let lanes = &values[at..at + 4];
        Self([lanes[0], lanes[1], lanes[2], lanes[3]])
    }
    #[inline(always)]
    fn store(self, values: &mut [f64], at: usize) {
        values[at..at + 4].copy_from_slice(&self.0);
    }
    #[inline(always)]
    fn zip(self, other: Self, op: impl Fn(f64, f64) -> f64) -> Self {
        Self([
            op(self.0[0], other.0[0]),
            op(self.0[1], other.0[1]),
            op(self.0[2], other.0[2]),
            op(self.0[3], other.0[3]),
        ])
    }
    #[inline(always)]
    fn map(self, op: impl Fn(f64) -> f64) -> Self {
        Self([op(self.0[0]), op(self.0[1]), op(self.0[2]), op(self.0[3])])
    }
    #[inline(always)]
    fn sum(self) -> f64 {
        (self.0[0] + self.0[1]) + (self.0[2] + self.0[3])
    }
}

#[derive(Clone, Copy)]
struct F32x8([f32; 8]);

impl F32x8 {
    #[inline(always)]
    fn load(values: &[f32], at: usize) -> Self {
        let mut lanes = [0.0; 8];
        lanes.copy_from_slice(&values[at..at + 8]);
        Self(lanes)
    }
    #[inline(always)]
    fn store(self, values: &mut [f32], at: usize) {
        values[at..at + 8].copy_from_slice(&self.0);
    }
    #[inline(always)]
    fn zip(self, other: Self, op: impl Fn(f32, f32) -> f32) -> Self {
        let mut lanes = [0.0; 8];
        for lane in 0..8 {
            lanes[lane] = op(self.0[lane], other.0[lane]);
        }
        Self(lanes)
    }
    #[inline(always)]
    fn sum(self) -> f32 {
        let l = self.0;
        ((l[0] + l[1]) + (l[2] + l[3])) + ((l[4] + l[5]) + (l[6] + l[7]))
    }
}

#[inline(never)]
fn dot(xs: &[f64], ys: &[f64]) -> f64 {
    let mut acc = F64x4::splat(0.0);
    let mut i = 0;
    while i + 3 < xs.len() {
        let product = F64x4::load(xs, i).zip(F64x4::load(ys, i), |x, y| x * y);
        acc = acc.zip(product, |a, p| a + p);
        i += 4;
    }
    acc.sum()
}

#[inline(never)]
fn saxpy(a: f64, xs: &[f64], ys: &mut [f64]) {
    let mut i = 0;
    while i + 3 < xs.len() {
        F64x4::load(xs, i)
            .zip(F64x4::load(ys, i), |x, y| x * a + y)
            .store(ys, i);
        i += 4;
    }
}

#[inline(never)]
fn norms(xs: &[f64], ys: &[f64], out: &mut [f64]) {
    let mut i = 0;
    while i + 3 < xs.len() {
        F64x4::load(xs, i)
            .zip(F64x4::load(ys, i), |x, y| x * x + y * y)
            .map(f64::sqrt)
            .store(out, i);
        i += 4;
    }
}

#[inline(never)]
fn clamp(xs: &mut [f64], low: f64, high: f64) {
    let mut i = 0;
    while i + 3 < xs.len() {
        F64x4::load(xs, i)
            .map(|x| if x > low { x } else { low })
            .map(|x| if x < high { x } else { high })
            .store(xs, i);
        i += 4;
    }
}

#[inline(never)]
fn absdiff(xs: &[f32], ys: &[f32], out: &mut [f32]) {
    let mut i = 0;
    while i + 7 < xs.len() {
        F32x8::load(xs, i)
            .zip(F32x8::load(ys, i), |x, y| (x - y).abs())
            .store(out, i);
        i += 8;
    }
}

#[inline(never)]
fn sum8(xs: &[f32]) -> f32 {
    let mut acc = F32x8([0.0; 8]);
    let mut i = 0;
    while i + 7 < xs.len() {
        acc = acc.zip(F32x8::load(xs, i), |a, x| a + x);
        i += 8;
    }
    acc.sum()
}

fn main() {
    let n = 1024;
    let reps = 400000;
    let xs: Vec<f64> = (0..n).map(|i| (i % 1000) as f64 * 0.5 + 1.0).collect();
    let mut ys: Vec<f64> = (0..n).map(|i| (i % 1000) as f64 * 0.25 - 3.0).collect();
    let mut out = vec![0.0f64; n];
    let fx: Vec<f32> = (0..n).map(|i| ((i % 100) as f64 * 0.5) as f32).collect();
    let fy: Vec<f32> = (0..n).map(|i| ((i % 70) as f64 * 0.25) as f32).collect();
    let mut fo = vec![0.0f32; n];

    let mut acc = 0.0;
    let start = Instant::now();
    for _ in 0..reps {
        acc += dot(black_box(&xs), black_box(&ys));
    }
    println!("dot {n} {} {acc:?}", start.elapsed().as_millis());

    let start = Instant::now();
    for _ in 0..reps {
        saxpy(0.0000001, black_box(&xs), black_box(&mut ys));
    }
    println!("saxpy {n} {} {:?}", start.elapsed().as_millis(), ys[7]);

    let start = Instant::now();
    for _ in 0..reps {
        norms(black_box(&xs), black_box(&ys), black_box(&mut out));
    }
    println!("norms {n} {} {:?}", start.elapsed().as_millis(), out[7]);

    let start = Instant::now();
    for _ in 0..reps {
        clamp(black_box(&mut out), 3.0, 200.0);
    }
    println!(
        "clamp {n} {} {:?} {:?}",
        start.elapsed().as_millis(),
        out[7],
        out[1000]
    );

    let start = Instant::now();
    for _ in 0..reps {
        absdiff(black_box(&fx), black_box(&fy), black_box(&mut fo));
    }
    println!("absdiff {n} {} {:?}", start.elapsed().as_millis(), fo[7]);

    let mut total = 0.0f32;
    let start = Instant::now();
    for _ in 0..reps {
        total += sum8(black_box(&fx));
    }
    println!("sum8 {n} {} {total:?}", start.elapsed().as_millis());
}
