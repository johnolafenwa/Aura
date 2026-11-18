use std::collections::HashSet;
use std::hint::black_box;
use std::time::Instant;

fn report(name: &str, start: Instant, checksum: usize) {
    println!(
        "{name} {} {checksum}",
        start.elapsed().as_secs_f64() * 1000.0
    );
}

fn gcd(mut left: usize, mut right: usize) -> usize {
    while right != 0 {
        (left, right) = (right, left % right);
    }
    left
}

fn main() {
    let mut checksum = 0;
    let start = Instant::now();
    for i in 0..500_000 {
        checksum += gcd(i % 997 + 41, 1533);
    }
    report("gcd", start, checksum);

    let text = black_box("abracadabra");
    checksum = 0;
    let start = Instant::now();
    for i in 0..400_000 {
        if text.as_bytes()[i % 11] == b'a' {
            checksum += 1;
        }
    }
    report("ascii_index", start, checksum);

    let left: HashSet<i64> = (0..128).collect();
    let right: HashSet<i64> = (64..192).collect();
    checksum = 0;
    let start = Instant::now();
    for _ in 0..2000 {
        let union: HashSet<_> = left.union(&right).copied().collect();
        checksum += black_box(union).len();
    }
    report("set_union", start, checksum);

    let message = "café-😀|".repeat(32);
    checksum = 0;
    let start = Instant::now();
    for _ in 0..4000 {
        let bytes: Vec<u8> = black_box(message.as_str())
            .encode_utf16()
            .flat_map(u16::to_le_bytes)
            .collect();
        let decoded: String = char::decode_utf16(
            bytes
                .chunks_exact(2)
                .map(|pair| u16::from_le_bytes([pair[0], pair[1]])),
        )
        .collect::<Result<_, _>>()
        .expect("valid UTF-16");
        checksum += black_box(decoded).chars().count();
    }
    report("utf16_round_trip", start, checksum);
}
