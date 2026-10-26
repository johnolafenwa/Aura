//! Text search: a small regular-expression engine run over generated log lines.
//!
//! Compiles ten patterns (literals, `.`, `*`, `+`, `?`, `^`, `$`, `[...]`
//! classes, `\d`, `\w`, `\s`) into a token program with one 256-entry byte
//! table per token, then runs each pattern over N log lines with a Thompson NFA
//! simulation (a Pike VM without captures). It counts which lines each pattern
//! matches. The engine is written out so all three languages do the same work.

const ONE: u8 = 0;
const OPTIONAL: u8 = 1;
const STAR: u8 = 2;

const PATTERNS: [&str; 10] = [
    "ERROR",
    "^\\d+-\\d+-\\d+ \\d+:\\d+:\\d+ WARN",
    "user=[a-z]+\\d+ ",
    "latency=\\d\\d\\d\\d+ms",
    "GET /api/v\\d/items/\\d+ ok$",
    "[A-Z][a-z]+Exception$",
    "id=[0-9a-f]+-[0-9a-f]+",
    "^.*timeout.*retry \\d$",
    "\\s[a-z]+\\s[a-z]+\\s[a-z]+$",
    "co?u?nn?ection re[a-z]*t",
];
const LEVELS: [&str; 4] = ["INFO", "WARN", "ERROR", "DEBUG"];
const COMPONENTS: [&str; 5] = ["auth", "db", "cache", "api", "queue"];
const NAMES: [&str; 6] = ["alice", "bob", "carol", "dave", "erin", "frank"];
const HEX_DIGITS: &[u8] = b"0123456789abcdef";

struct Rng {
    state: i64,
}

impl Rng {
    fn next(&mut self) -> i64 {
        self.state = self.state * 48271 % 2147483647;
        self.state
    }
}

#[derive(Default)]
struct Program {
    tables: Vec<bool>,
    quants: Vec<u8>,
    count: usize,
    anchored_start: bool,
    anchored_end: bool,
}

fn to_hex(mut value: i64) -> String {
    if value == 0 {
        return "0".to_string();
    }
    let mut digits = Vec::new();
    while value > 0 {
        digits.push(HEX_DIGITS[(value % 16) as usize]);
        value /= 16;
    }
    digits.reverse();
    String::from_utf8(digits).unwrap()
}

fn pad2(value: i64) -> String {
    if value < 10 {
        format!("0{value}")
    } else {
        value.to_string()
    }
}

fn make_line(rng: &mut Rng) -> String {
    let r1 = rng.next();
    let r2 = rng.next();
    let r3 = rng.next();
    let day = 1 + r1 % 28;
    let hour = (r1 / 28) % 24;
    let minute = (r1 / 672) % 60;
    let second = r2 % 60;
    let level = LEVELS[((r2 / 60) % 4) as usize];
    let component = COMPONENTS[((r2 / 240) % 5) as usize];
    let mut name = NAMES[(r3 % 6) as usize].to_string();
    if r3 % 3 == 0 {
        name += &((r3 / 6) % 100).to_string();
    }
    let latency = (r3 / 7) % 2000;
    let kind = (r3 / 14000) % 6;
    let a = 1 + r1 % 3;
    let b = r2 % 10000;
    let message = match kind {
        0 => format!("GET /api/v{a}/items/{b} ok"),
        1 => format!("timeout after {b}ms, retry {a}"),
        2 => format!(
            "cache miss for id={}-{}",
            to_hex(b * 7919),
            to_hex(r3 % 65536)
        ),
        3 => format!(
            "raised {}Exception",
            ["Value", "Key", "Io"][(a - 1) as usize]
        ),
        4 => format!("POST /api/v{a}/orders/{b} created"),
        _ => "connection reset by peer".to_string(),
    };
    format!(
        "2026-10-{} {}:{}:{} {level} {component} user={name} latency={latency}ms {message}",
        pad2(day),
        pad2(hour),
        pad2(minute),
        pad2(second)
    )
}

fn class_table(program: &mut Program) -> usize {
    let base = program.count * 256;
    program.tables.extend(std::iter::repeat(false).take(256));
    program.count += 1;
    base
}

fn set_range(program: &mut Program, base: usize, low: u8, high: u8) {
    for b in low..=high {
        program.tables[base + b as usize] = true;
    }
}

fn compile_pattern(pattern: &str) -> Program {
    let p = pattern.as_bytes();
    let mut program = Program::default();
    let mut i = 0;
    if p[0] == b'^' {
        program.anchored_start = true;
        i = 1;
    }
    while i < p.len() {
        let c = p[i];
        if c == b'$' && i == p.len() - 1 {
            program.anchored_end = true;
            break;
        }
        let base = class_table(&mut program);
        if c == b'.' {
            set_range(&mut program, base, 0, 255);
            program.tables[base + b'\n' as usize] = false;
        } else if c == b'\\' {
            i += 1;
            let e = p[i];
            if e == b'd' {
                set_range(&mut program, base, b'0', b'9');
            } else if e == b'w' {
                set_range(&mut program, base, b'0', b'9');
                set_range(&mut program, base, b'a', b'z');
                set_range(&mut program, base, b'A', b'Z');
                program.tables[base + b'_' as usize] = true;
            } else if e == b's' {
                program.tables[base + b' ' as usize] = true;
                program.tables[base + b'\t' as usize] = true;
            } else {
                program.tables[base + e as usize] = true;
            }
        } else if c == b'[' {
            i += 1;
            while p[i] != b']' {
                if p[i + 1] == b'-' && p[i + 2] != b']' {
                    set_range(&mut program, base, p[i], p[i + 2]);
                    i += 3;
                } else {
                    program.tables[base + p[i] as usize] = true;
                    i += 1;
                }
            }
        } else {
            program.tables[base + c as usize] = true;
        }
        i += 1;
        let mut quant = ONE;
        if i < p.len() {
            if p[i] == b'?' {
                quant = OPTIONAL;
                i += 1;
            } else if p[i] == b'*' {
                quant = STAR;
                i += 1;
            } else if p[i] == b'+' {
                // x+ is x followed by x*.
                program.quants.push(ONE);
                let copy = class_table(&mut program);
                for b in 0..256 {
                    program.tables[copy + b] = program.tables[base + b];
                }
                quant = STAR;
                i += 1;
            }
        }
        program.quants.push(quant);
    }
    program
}

/// Visited marks shared by every pattern; the generation only grows.
struct Matcher {
    marks: Vec<i64>,
    generation: i64,
}

impl Matcher {
    fn add_state(&mut self, program: &Program, states: &mut Vec<usize>, start: usize) {
        let mut s = start;
        while self.marks[s] != self.generation {
            self.marks[s] = self.generation;
            states.push(s);
            if s == program.count || program.quants[s] == ONE {
                return;
            }
            s += 1;
        }
    }

    fn search(&mut self, program: &Program, data: &[u8]) -> bool {
        let accept = program.count;
        self.generation += 1;
        let mut current = Vec::new();
        self.add_state(program, &mut current, 0);
        let mut pos = 0;
        loop {
            if self.marks[accept] == self.generation && (!program.anchored_end || pos == data.len())
            {
                return true;
            }
            if pos == data.len() || current.is_empty() {
                return false;
            }
            let byte = data[pos] as usize;
            self.generation += 1;
            let mut following = Vec::new();
            for &s in &current {
                if s < accept && program.tables[s * 256 + byte] {
                    if program.quants[s] == STAR {
                        self.add_state(program, &mut following, s);
                    } else {
                        self.add_state(program, &mut following, s + 1);
                    }
                }
            }
            pos += 1;
            if !program.anchored_start {
                self.add_state(program, &mut following, 0);
            }
            current = following;
        }
    }
}

fn main() {
    let n: i64 = std::env::args().nth(1).unwrap().parse().unwrap();
    let mut rng = Rng { state: 31 };
    let programs: Vec<Program> = PATTERNS
        .iter()
        .map(|pattern| compile_pattern(pattern))
        .collect();
    let size = programs.iter().map(|program| program.count).max().unwrap() + 1;
    let mut matcher = Matcher {
        marks: vec![0; size],
        generation: 0,
    };
    let mut counts = vec![0i64; programs.len()];
    let mut checksum = 0i64;
    let mut total_bytes = 0;
    for line_number in 0..n {
        let line = make_line(&mut rng);
        let data = line.as_bytes();
        total_bytes += data.len();
        let mut mask = 0i64;
        for k in 0..programs.len() {
            if matcher.search(&programs[k], data) {
                counts[k] += 1;
                mask |= 1 << k;
            }
        }
        checksum = (checksum * 31 + mask * (line_number % 13 + 1)) % 1000000007;
    }
    let tokens: usize = programs.iter().map(|program| program.count).sum();
    let counts: Vec<String> = counts.iter().map(|c| c.to_string()).collect();
    println!(
        "regex_match lines={n} bytes={total_bytes} tokens={tokens} counts={} checksum={checksum}",
        counts.join(",")
    );
}
