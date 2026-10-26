//! Checksums and content-defined chunking over a byte buffer.
//!
//! Generates N bytes from 64 random 200-byte blocks (some with one byte changed),
//! then computes FNV-1a (32-bit), CRC-32 (table driven), and Adler-32 over the
//! whole buffer. It also splits the buffer into chunks where a rolling hash over a
//! 16-byte window hits a boundary, as backup and sync tools do, and counts the
//! distinct chunks by their FNV-1a hash.
use std::collections::HashSet;

const WINDOW: usize = 16;
const BASE: i64 = 257;
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

fn generate(rng: &mut Rng, n: usize) -> Vec<u8> {
    let mut blocks = Vec::new();
    for _ in 0..64 {
        let block: Vec<u8> = (0..200).map(|_| (rng.next() % 256) as u8).collect();
        blocks.push(block);
    }
    let mut data = Vec::new();
    while data.len() < n {
        let r = rng.next();
        if r % 5 == 0 {
            let mut block = blocks[(r % 64) as usize].clone();
            block[((r / 5) % 200) as usize] = ((r / 1000) % 256) as u8;
            data.extend_from_slice(&block);
        } else {
            data.extend_from_slice(&blocks[(r % 64) as usize]);
        }
    }
    data.truncate(n);
    data
}

fn fnv1a(data: &[u8], start: usize, end: usize) -> i64 {
    let mut h: i64 = 2166136261;
    for &b in &data[start..end] {
        h = ((h ^ b as i64) * 16777619) % 4294967296;
    }
    h
}

fn crc_table() -> Vec<i64> {
    let mut table = Vec::new();
    for i in 0..256 {
        let mut c: i64 = i;
        for _ in 0..8 {
            if c & 1 != 0 {
                c = 0xEDB88320 ^ (c >> 1);
            } else {
                c >>= 1;
            }
        }
        table.push(c);
    }
    table
}

fn crc32(table: &[i64], data: &[u8]) -> i64 {
    let mut c: i64 = 0xFFFFFFFF;
    for &b in data {
        c = table[((c ^ b as i64) & 255) as usize] ^ (c >> 8);
    }
    c ^ 0xFFFFFFFF
}

fn adler32(data: &[u8]) -> i64 {
    let (mut a, mut b) = (1i64, 0i64);
    for &x in data {
        a = (a + x as i64) % 65521;
        b = (b + a) % 65521;
    }
    b * 65536 + a
}

fn main() {
    let n: usize = std::env::args().nth(1).unwrap().parse().unwrap();
    let mut rng = Rng { state: 43 };
    let data = generate(&mut rng, n);
    let whole = fnv1a(&data, 0, data.len());
    let crc = crc32(&crc_table(), &data);
    let adler = adler32(&data);

    let mut power = 1;
    for _ in 0..WINDOW - 1 {
        power = power * BASE % MOD;
    }
    let mut seen = HashSet::new();
    let (mut chunks, mut chunk_sum, mut start, mut h) = (0, 0i64, 0usize, 0i64);
    for i in 0..data.len() {
        if i >= WINDOW {
            h = (h + MOD - data[i - WINDOW] as i64 * power % MOD) % MOD;
        }
        h = (h * BASE + data[i] as i64) % MOD;
        let size = i + 1 - start;
        if size >= 32 && (h % 128 == 0 || size >= 1024) {
            let digest = fnv1a(&data, start, i + 1);
            seen.insert(digest);
            chunks += 1;
            chunk_sum = (chunk_sum + digest % 1000003 * size as i64) % MOD;
            start = i + 1;
        }
    }
    if start < data.len() {
        let digest = fnv1a(&data, start, data.len());
        seen.insert(digest);
        chunks += 1;
        chunk_sum = (chunk_sum + digest % 1000003 * (data.len() - start) as i64) % MOD;
    }
    println!(
        "hashing bytes={} fnv={whole} crc32={crc} adler32={adler} chunks={chunks} unique={} chunk_sum={chunk_sum}",
        data.len(),
        seen.len()
    );
}
