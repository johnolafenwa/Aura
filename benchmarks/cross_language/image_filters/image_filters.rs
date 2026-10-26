//! Image processing: blur, edge detection, histogram, downscale, PGM encoding.
//!
//! Generates an N x N grayscale image (a gradient plus noise), applies a 3 x 3 box
//! blur with clamped edges, runs a Sobel edge detector on the result, builds a
//! 16-bin histogram of edge strength, halves the image with 2 x 2 averaging, and
//! encodes the small image as a binary PGM file in memory. Pixels live in flat
//! integer lists indexed by y * width + x.

const MOD: i64 = 1000000007;

struct Rng {
    state: i64,
}

impl Rng {
    fn next(&mut self) -> i64 {
        self.state = self.state * 48271 % 2147483647;
        self.state
    }
}

fn blur(pixels: &[i64], n: i64) -> Vec<i64> {
    let mut out = vec![0; (n * n) as usize];
    for y in 0..n {
        for x in 0..n {
            let mut total = 0;
            for dy in -1..2 {
                let yy = (n - 1).min(0.max(y + dy));
                for dx in -1..2 {
                    let xx = (n - 1).min(0.max(x + dx));
                    total += pixels[(yy * n + xx) as usize];
                }
            }
            out[(y * n + x) as usize] = total / 9;
        }
    }
    out
}

fn sobel(pixels: &[i64], n: i64) -> Vec<i64> {
    let mut out = vec![0; (n * n) as usize];
    let n = n as usize;
    for y in 1..n - 1 {
        for x in 1..n - 1 {
            let i = y * n + x;
            let gx = (pixels[i - n + 1] + 2 * pixels[i + 1] + pixels[i + n + 1])
                - (pixels[i - n - 1] + 2 * pixels[i - 1] + pixels[i + n - 1]);
            let gy = (pixels[i + n - 1] + 2 * pixels[i + n] + pixels[i + n + 1])
                - (pixels[i - n - 1] + 2 * pixels[i - n] + pixels[i - n + 1]);
            out[i] = 255.min(gx.abs() + gy.abs());
        }
    }
    out
}

fn downscale(pixels: &[i64], n: i64) -> Vec<i64> {
    let half = n / 2;
    let n = n as usize;
    let mut out = Vec::new();
    for y in 0..half as usize {
        for x in 0..half as usize {
            let i = 2 * y * n + 2 * x;
            out.push((pixels[i] + pixels[i + 1] + pixels[i + n] + pixels[i + n + 1]) / 4);
        }
    }
    out
}

fn main() {
    let n: i64 = std::env::args().nth(1).unwrap().parse().unwrap();
    let mut rng = Rng { state: 71 };
    let mut pixels = Vec::new();
    for y in 0..n {
        for x in 0..n {
            pixels.push((x * x + 3 * y * y + rng.next() % 32) % 256);
        }
    }
    let blurred = blur(&pixels, n);
    let edges = sobel(&blurred, n);
    let mut histogram = [0i64; 16];
    let mut strong = 0;
    for &value in &edges {
        histogram[(value / 16) as usize] += 1;
        if value > 128 {
            strong += 1;
        }
    }
    let small = downscale(&blurred, n);
    let half = n / 2;
    let mut pgm = format!("P5\n{half} {half}\n255\n").into_bytes();
    for &value in &small {
        pgm.push(value as u8);
    }
    let mut blur_check = 0i64;
    for &value in &blurred {
        blur_check = (blur_check * 31 + value) % MOD;
    }
    let mut pgm_check = 0i64;
    for &value in &pgm {
        pgm_check = (pgm_check * 131 + value as i64) % MOD;
    }
    let histogram: Vec<String> = histogram.iter().map(|h| h.to_string()).collect();
    println!(
        "image_filters size={n} blur={blur_check} strong={strong} histogram={} pgm_bytes={} pgm={pgm_check}",
        histogram.join(","),
        pgm.len()
    );
}
