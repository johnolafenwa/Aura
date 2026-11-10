//! Streaming I/O: a log read in line batches and in chunks, with gzip and HMAC.
//!
//! Writes N log lines, reads them back line by line and parses each, then reads
//! the file again in 64 KiB chunks. Each chunk is compressed with gzip,
//! decompressed again, and folded into a running HMAC-SHA256, so the program
//! never holds the whole file. Takes the line count and a scratch directory.
use std::fs;
use std::io::{BufRead, BufReader, BufWriter, Read, Write};
use std::path::Path;

use flate2::read::GzDecoder;
use flate2::write::GzEncoder;
use flate2::Compression;
use hmac::{Hmac, Mac};
use sha2::Sha256;

struct Rng {
    state: i64,
}

impl Rng {
    fn next(&mut self) -> i64 {
        self.state = self.state * 48271 % 2147483647;
        self.state
    }
}

/// Reads until `buffer` is full or the file ends, as Python's `read(n)` does.
fn read_chunk(file: &mut fs::File, buffer: &mut [u8]) -> usize {
    let mut filled = 0;
    while filled < buffer.len() {
        match file.read(&mut buffer[filled..]).unwrap() {
            0 => break,
            count => filled += count,
        }
    }
    filled
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let n: i64 = args[1].parse().unwrap();
    let workdir = Path::new(&args[2]);
    let mut rng = Rng { state: 41 };
    let words = [
        "connect", "read", "write", "close", "retry", "timeout", "accept", "flush",
    ];

    let log_path = workdir.join("stream.log");
    {
        let mut log = BufWriter::new(fs::File::create(&log_path).unwrap());
        for i in 0..n {
            let word = words[(rng.next() % 8) as usize];
            write!(
                log,
                "{i} {word} worker-{} {}\n",
                rng.next() % 64,
                rng.next() % 100000
            )
            .unwrap();
        }
        log.flush().unwrap();
    }

    let (mut lines, mut value_sum, mut retries) = (0i64, 0i64, 0i64);
    for line in BufReader::new(fs::File::open(&log_path).unwrap()).lines() {
        let line = line.unwrap();
        let fields: Vec<&str> = line.split(' ').collect();
        lines += 1;
        value_sum += fields[3].parse::<i64>().unwrap();
        if fields[1] == "retry" {
            retries += 1;
        }
    }

    let (mut chunks, mut restored, mut smaller) = (0i64, 0usize, 0i64);
    let mut mac: Vec<u8> = b"stream_io".to_vec();
    let mut file = fs::File::open(&log_path).unwrap();
    let mut buffer = vec![0u8; 65536];
    loop {
        let filled = read_chunk(&mut file, &mut buffer);
        if filled == 0 {
            break;
        }
        let chunk = &buffer[..filled];
        chunks += 1;
        let mut encoder = GzEncoder::new(Vec::new(), Compression::new(6));
        encoder.write_all(chunk).unwrap();
        let packed = encoder.finish().unwrap();
        if packed.len() < chunk.len() {
            smaller += 1;
        }
        let mut plain = Vec::new();
        GzDecoder::new(&packed[..]).read_to_end(&mut plain).unwrap();
        restored += plain.len();
        let mut hmac = Hmac::<Sha256>::new_from_slice(&mac).unwrap();
        hmac.update(&plain);
        mac = hmac.finalize().into_bytes().to_vec();
    }

    fs::remove_file(&log_path).unwrap();
    let hex: String = mac.iter().map(|byte| format!("{byte:02x}")).collect();
    println!(
        "stream_io lines={lines} value_sum={value_sum} retries={retries} chunks={chunks} smaller={smaller} restored={restored} mac={hex}"
    );
}
