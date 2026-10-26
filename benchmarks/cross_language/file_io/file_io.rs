//! File I/O: a line log, a binary record file, and a directory of small files.
//!
//! Writes N log lines one write call at a time, reads the log back and parses it,
//! writes N four-byte little-endian records and decodes them, then creates N / 100
//! small files in a subdirectory, lists and reads them, replaces a manifest
//! atomically (write a temporary file, rename it over the target), and deletes
//! everything it wrote. Takes the line count and a scratch directory.
use std::fs;
use std::io::{BufWriter, Write};
use std::path::Path;

struct Rng {
    state: i64,
}

impl Rng {
    fn next(&mut self) -> i64 {
        self.state = self.state * 48271 % 2147483647;
        self.state
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let n: i64 = args[1].parse().unwrap();
    let workdir = Path::new(&args[2]);
    let mut rng = Rng { state: 37 };

    let log_path = workdir.join("events.log");
    {
        // Python's text files are buffered too.
        let mut log = BufWriter::new(fs::File::create(&log_path).unwrap());
        for i in 0..n {
            write!(log, "event {i} value {}\n", rng.next() % 100000).unwrap();
        }
        log.flush().unwrap();
    }
    let text = fs::read_to_string(&log_path).unwrap();
    let (mut lines, mut value_sum) = (0i64, 0i64);
    for line in text.split('\n') {
        if line.is_empty() {
            continue;
        }
        let fields: Vec<&str> = line.split(' ').collect();
        lines += 1;
        value_sum += fields[3].parse::<i64>().unwrap();
    }

    let mut records = Vec::new();
    for _ in 0..n {
        let v = rng.next();
        records.push((v & 255) as u8);
        records.push(((v >> 8) & 255) as u8);
        records.push(((v >> 16) & 255) as u8);
        records.push(((v >> 24) & 255) as u8);
    }
    let binary_path = workdir.join("records.bin");
    fs::write(&binary_path, &records).unwrap();
    let loaded = fs::read(&binary_path).unwrap();
    let mut checksum = 0i64;
    for chunk in loaded.chunks(4) {
        let v = chunk[0] as i64
            | (chunk[1] as i64) << 8
            | (chunk[2] as i64) << 16
            | (chunk[3] as i64) << 24;
        checksum = (checksum * 31 + v) % 1000000007;
    }

    let shard = workdir.join("shard");
    fs::create_dir_all(&shard).unwrap();
    let files = (n / 100).max(1);
    for k in 0..files {
        fs::write(
            shard.join(format!("part-{k}.txt")),
            format!("shard {k}\n").repeat((k % 10 + 1) as usize),
        )
        .unwrap();
    }
    let mut names: Vec<String> = fs::read_dir(&shard)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().into_string().unwrap())
        .collect();
    names.sort();
    let mut file_bytes = 0;
    for name in &names {
        file_bytes += fs::read_to_string(shard.join(name)).unwrap().len();
    }
    let manifest = workdir.join("manifest.txt");
    let temporary = workdir.join("manifest.txt.tmp");
    {
        let mut out = fs::File::create(&temporary).unwrap();
        out.write_all(names.join("\n").as_bytes()).unwrap();
        out.sync_all().unwrap();
    }
    fs::rename(&temporary, &manifest).unwrap();
    let manifest_bytes = fs::read_to_string(&manifest).unwrap().len();

    for name in &names {
        fs::remove_file(shard.join(name)).unwrap();
    }
    fs::remove_file(&manifest).unwrap();
    fs::remove_file(&binary_path).unwrap();
    fs::remove_file(&log_path).unwrap();
    let left = fs::read_dir(&shard).unwrap().count();
    println!(
        "file_io lines={lines} value_sum={value_sum} record_bytes={} checksum={checksum} files={files} listed={} \
         file_bytes={file_bytes} manifest_bytes={manifest_bytes} left={left}",
        loaded.len(),
        names.len()
    );
}
