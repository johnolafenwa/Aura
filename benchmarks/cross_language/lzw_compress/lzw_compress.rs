//! Compression: LZW with 12-bit codes and run-length coding, both round-tripped.
//!
//! Generates N bytes of word-like text, compresses it with LZW (dictionary reset
//! at 4,096 entries), decompresses it, and checks the result. It also run-length
//! encodes a sparse byte stream and decodes it again.
use std::collections::HashMap;

const SYLLABLES: [&str; 10] = [
    "the", "and", "data", "core", "sys", "net", "run", "io", "map", "set",
];
const MAX_CODES: i64 = 4096;

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
    let mut text = String::new();
    while text.len() < n {
        let r = rng.next();
        text.push_str(SYLLABLES[(r % 10) as usize]);
        text.push_str(SYLLABLES[((r / 10) % 10) as usize]);
        text.push(if r % 9 == 0 { '\n' } else { ' ' });
    }
    text.truncate(n);
    text.into_bytes()
}

fn compress(data: &[u8]) -> Vec<i64> {
    let mut table: HashMap<i64, i64> = HashMap::new();
    let mut next_code = 256;
    let mut codes = Vec::new();
    let mut prefix = data[0] as i64;
    for &byte in &data[1..] {
        let key = prefix * 256 + byte as i64;
        match table.get(&key) {
            Some(&code) => prefix = code,
            None => {
                codes.push(prefix);
                if next_code < MAX_CODES {
                    table.insert(key, next_code);
                    next_code += 1;
                } else {
                    table.clear();
                    next_code = 256;
                }
                prefix = byte as i64;
            }
        }
    }
    codes.push(prefix);
    codes
}

fn expand(prefixes: &[i64], suffixes: &[u8], mut code: i64) -> Vec<u8> {
    let mut bytes = Vec::new();
    while code >= 256 {
        bytes.push(suffixes[(code - 256) as usize]);
        code = prefixes[(code - 256) as usize];
    }
    bytes.push(code as u8);
    bytes.reverse();
    bytes
}

fn decompress(codes: &[i64]) -> Vec<u8> {
    let mut prefixes: Vec<i64> = Vec::new();
    let mut suffixes: Vec<u8> = Vec::new();
    let mut out = Vec::new();
    let mut previous = -1;
    for &code in codes {
        let entry = if code < 256 {
            vec![code as u8]
        } else if ((code - 256) as usize) < prefixes.len() {
            expand(&prefixes, &suffixes, code)
        } else {
            let mut entry = expand(&prefixes, &suffixes, previous);
            entry.push(entry[0]);
            entry
        };
        out.extend_from_slice(&entry);
        if previous >= 0 {
            if (prefixes.len() as i64) + 256 < MAX_CODES {
                prefixes.push(previous);
                suffixes.push(entry[0]);
            } else {
                prefixes.clear();
                suffixes.clear();
            }
        }
        previous = code;
    }
    out
}

fn sparse_stream(rng: &mut Rng, n: usize) -> Vec<u8> {
    let mut data = Vec::new();
    while data.len() < n {
        let r = rng.next();
        let run = 1 + (r % 300) as usize;
        let value = if r % 3 != 0 { 0 } else { ((r / 3) % 256) as u8 };
        let count = run.min(n - data.len());
        data.extend(std::iter::repeat(value).take(count));
    }
    data
}

fn rle_encode(data: &[u8]) -> Vec<i64> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < data.len() {
        let value = data[i];
        let mut run = 1;
        while i + run < data.len() && run < 255 && data[i + run] == value {
            run += 1;
        }
        out.push(run as i64);
        out.push(value as i64);
        i += run;
    }
    out
}

fn rle_decode(pairs: &[i64]) -> Vec<u8> {
    let mut out = Vec::new();
    for pair in pairs.chunks(2) {
        out.extend(std::iter::repeat(pair[1] as u8).take(pair[0] as usize));
    }
    out
}

fn main() {
    let n: usize = std::env::args().nth(1).unwrap().parse().unwrap();
    let mut rng = Rng { state: 29 };
    let data = generate(&mut rng, n);
    let codes = compress(&data);
    let restored = decompress(&codes);
    let mut checksum = 0i64;
    for (i, &code) in codes.iter().enumerate() {
        checksum = (checksum * 31 + code * (i as i64 % 7 + 1)) % 1000000007;
    }
    let sparse = sparse_stream(&mut rng, n);
    let pairs = rle_encode(&sparse);
    let rle_ok = rle_decode(&pairs) == sparse;
    println!(
        "lzw_compress bytes={} codes={} restored={} checksum={checksum} rle_pairs={} rle_restored={rle_ok}",
        data.len(),
        codes.len(),
        restored == data,
        pairs.len() / 2
    );
}
