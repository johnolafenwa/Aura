//! Developer tools: a line diff with the Myers O(ND) algorithm.
//!
//! Generates a text of N lines and an edited copy (about 4% of lines deleted, 4%
//! inserted, 4% changed), interns every distinct line as an integer, finds a
//! shortest edit script with Myers' greedy algorithm (keeping one snapshot of the
//! frontier per edit distance for the backtrack), and renders it as diff lines
//! prefixed with ' ', '-', or '+'. Reports the edit distance, the operations, the
//! hunks, and a checksum of the rendered diff.
use std::collections::HashMap;

const WORDS: [&str; 12] = [
    "let", "value", "return", "if", "else", "for", "item", "count", "total", "name", "}", "{",
];
const KEEP: usize = 0;
const DELETE: usize = 1;
const INSERT: usize = 2;

struct Rng {
    state: i64,
}

impl Rng {
    fn next(&mut self) -> i64 {
        self.state = self.state * 48271 % 2147483647;
        self.state
    }
}

fn make_line(rng: &mut Rng) -> String {
    let r = rng.next();
    format!(
        "{} {} {}",
        WORDS[(r % 12) as usize],
        WORDS[((r / 12) % 12) as usize],
        (r / 144) % 60
    )
}

fn intern(lines: &[String], table: &mut HashMap<String, i64>) -> Vec<i64> {
    let mut ids = Vec::new();
    for line in lines {
        let next = table.len() as i64;
        ids.push(*table.entry(line.clone()).or_insert(next));
    }
    ids
}

fn myers(a: &[i64], b: &[i64]) -> Vec<usize> {
    let n = a.len() as i64;
    let m = b.len() as i64;
    let limit = n + m;
    let offset = limit + 1;
    let mut v = vec![0i64; (2 * limit + 3) as usize];
    let mut trace: Vec<Vec<i64>> = Vec::new();
    for d in 0..=limit {
        trace.push(v[(offset - d) as usize..(offset + d + 1) as usize].to_vec());
        let mut k = -d;
        while k <= d {
            let mut x = if k == -d
                || (k != d && v[(offset + k - 1) as usize] < v[(offset + k + 1) as usize])
            {
                v[(offset + k + 1) as usize]
            } else {
                v[(offset + k - 1) as usize] + 1
            };
            let mut y = x - k;
            while x < n && y < m && a[x as usize] == b[y as usize] {
                x += 1;
                y += 1;
            }
            v[(offset + k) as usize] = x;
            if x >= n && y >= m {
                return backtrack(&trace, n, m);
            }
            k += 2;
        }
    }
    Vec::new()
}

fn backtrack(trace: &[Vec<i64>], n: i64, m: i64) -> Vec<usize> {
    let mut ops = Vec::new();
    let (mut x, mut y) = (n, m);
    for d in (0..trace.len() as i64).rev() {
        let snapshot = &trace[d as usize];
        let k = x - y;
        let previous_k = if k == -d
            || (k != d && snapshot[(k - 1 + d) as usize] < snapshot[(k + 1 + d) as usize])
        {
            k + 1
        } else {
            k - 1
        };
        let previous_x = if d > 0 {
            snapshot[(previous_k + d) as usize]
        } else {
            0
        };
        let previous_y = previous_x - previous_k;
        while x > previous_x && y > previous_y {
            x -= 1;
            y -= 1;
            ops.push(KEEP);
        }
        if d > 0 {
            ops.push(if x == previous_x { INSERT } else { DELETE });
        }
        x = previous_x;
        y = previous_y;
    }
    ops.reverse();
    ops
}

fn main() {
    let n: usize = std::env::args().nth(1).unwrap().parse().unwrap();
    let mut rng = Rng { state: 73 };
    let old: Vec<String> = (0..n).map(|_| make_line(&mut rng)).collect();
    let mut new = Vec::new();
    for line in &old {
        let r = rng.next() % 100;
        if r < 4 {
            continue;
        }
        if r < 8 {
            new.push(make_line(&mut rng));
        }
        if (8..12).contains(&r) {
            new.push(make_line(&mut rng));
            continue;
        }
        new.push(line.clone());
    }
    let mut table = HashMap::new();
    let a = intern(&old, &mut table);
    let b = intern(&new, &mut table);
    let ops = myers(&a, &b);
    let mut out = Vec::new();
    let (mut i, mut j, mut hunks, mut previous) = (0, 0, 0, KEEP);
    let mut counts = [0usize; 3];
    for &op in &ops {
        counts[op] += 1;
        if op != KEEP && previous == KEEP {
            hunks += 1;
        }
        previous = op;
        if op == KEEP {
            out.push(format!(" {}", old[i]));
            i += 1;
            j += 1;
        } else if op == DELETE {
            out.push(format!("-{}", old[i]));
            i += 1;
        } else {
            out.push(format!("+{}", new[j]));
            j += 1;
        }
    }
    let text = out.join("\n");
    let mut checksum = 0i64;
    for line in &out {
        checksum = (checksum * 31 + line.len() as i64) % 1000000007;
    }
    println!(
        "text_diff old={n} new={} distinct={} distance={} kept={} deleted={} inserted={} hunks={hunks} bytes={} checksum={checksum}",
        new.len(),
        table.len(),
        counts[1] + counts[2],
        counts[0],
        counts[1],
        counts[2],
        text.len()
    );
}
