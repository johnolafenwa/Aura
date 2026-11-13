// The routines of `kernels.au` in Rust with no standard library. Built
// twice: with wrapping arithmetic (the release default) and with
// `-C overflow-checks=on`, which is the closer match to Aura's checks.
#![no_std]

#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    loop {}
}

#[derive(Clone, Copy)]
struct Body {
    x: f64,
    y: f64,
    vx: f64,
    vy: f64,
}

extern "C" {
    fn sqrt(value: f64) -> f64;
}

fn step(body: Body, dt: f64) -> Body {
    let r2 = body.x * body.x + body.y * body.y;
    let inverse = 1.0 / (r2 * unsafe { sqrt(r2) });
    let vx = body.vx - body.x * inverse * dt;
    let vy = body.vy - body.y * inverse * dt;
    Body { x: body.x + vx * dt, y: body.y + vy * dt, vx, vy }
}

#[no_mangle]
pub extern "C" fn rs_crc32(count: u32) -> u32 {
    let mut crc: u32 = 0xffff_ffff;
    let mut i: u32 = 0;
    while i < count {
        crc ^= i & 255;
        let mut bit = 0;
        while bit < 8 {
            crc = if crc & 1 == 1 { (crc >> 1) ^ 0xedb8_8320 } else { crc >> 1 };
            bit += 1;
        }
        i += 1;
    }
    crc ^ 0xffff_ffff
}

#[no_mangle]
pub extern "C" fn rs_collatz(limit: u64) -> u64 {
    let mut total: u64 = 0;
    let mut start: u64 = 1;
    while start < limit {
        let mut n = start;
        while n != 1 {
            n = if n % 2 == 0 { n / 2 } else { 3 * n + 1 };
            total += 1;
        }
        start += 1;
    }
    total
}

#[no_mangle]
pub extern "C" fn rs_isqrt_sum(limit: u64) -> u64 {
    let mut total: u64 = 0;
    let mut value: u64 = 1;
    while value < limit {
        let mut guess = value;
        let mut next = (guess + 1) / 2;
        while next < guess {
            guess = next;
            next = (guess + value / guess) / 2;
        }
        total += guess;
        value += 1;
    }
    total
}

#[no_mangle]
pub extern "C" fn rs_gcd_sum(limit: i64) -> i64 {
    let mut total: i64 = 0;
    let mut a: i64 = 1;
    while a < limit {
        let mut x = a;
        let mut y = limit - a;
        while y != 0 {
            let rest = x % y;
            x = y;
            y = rest;
        }
        total += x;
        a += 1;
    }
    total
}

#[no_mangle]
pub extern "C" fn rs_orbit(steps: i64) -> f64 {
    let mut body = Body { x: 1.0, y: 0.0, vx: 0.0, vy: 1.0 };
    let mut i: i64 = 0;
    while i < steps {
        body = step(body, 0.001);
        i += 1;
    }
    body.x * body.x + body.y * body.y
}
