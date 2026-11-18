use num_bigint::{BigInt, BigUint};
use num_complex::Complex64;
use rust_decimal::Decimal;
use std::{fmt::Display, hint::black_box, time::Instant};

fn product(low: u64, high: u64) -> BigUint {
    if (high - low + 1) * u64::from(high.ilog2() + 1) <= 64 {
        return BigUint::from((low..=high).product::<u64>());
    }
    let mid = low + (high - low) / 2;
    product(low, mid) * product(mid + 1, high)
}

fn timed<T: Display>(name: &str, f: impl FnOnce() -> T) {
    let start = Instant::now();
    let checksum = black_box(f());
    println!(
        "{name} {:.3} {checksum}",
        start.elapsed().as_secs_f64() * 1000.0
    );
}

fn main() {
    timed("factorial", || {
        (4000..4100).map(|n| product(2, n).bits()).sum::<u64>()
    });
    timed("fibonacci", || {
        let (mut a, mut b) = (BigInt::from(0), BigInt::from(1));
        for _ in 0..30000 {
            let next = &a + &b;
            a = b;
            b = next;
        }
        a.bits()
    });
    timed("small_integers", || {
        let mut total = BigInt::from(0);
        for i in 0..500000 {
            total += i;
        }
        total
    });
    timed("modular_powers", || {
        let exponent = BigInt::from(65537);
        let modulus = BigInt::from(1000000007);
        (1..20001)
            .map(|i| BigInt::from(i).modpow(&exponent, &modulus))
            .sum::<BigInt>()
    });
    timed("decimal_money", || {
        let price = black_box(Decimal::new(1995, 2));
        let quantity = black_box(Decimal::from(3));
        let mut total = Decimal::ZERO;
        for _ in 0..200000 {
            total += price * quantity;
        }
        format!("{total:.2}")
    });
    timed("mandelbrot", || {
        let mut total = 0;
        for y in 0..240 {
            for x in 0..320 {
                let c = Complex64::new(f64::from(x) / 128.0 - 2.0, f64::from(y) / 120.0 - 1.0);
                let mut z = Complex64::new(0.0, 0.0);
                let mut iteration = 0;
                while iteration < 50 && z.norm_sqr() <= 4.0 {
                    z = z * z + c;
                    iteration += 1;
                }
                total += iteration;
            }
        }
        total
    });
}
