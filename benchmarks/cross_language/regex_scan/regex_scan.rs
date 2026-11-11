// Regular expressions over generated log lines.
//
// The same work as regex_scan.au, on the `regex` crate Aura's runtime links.
use regex::Regex;

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
    let Some(n) = args.get(1).and_then(|text| text.parse::<i64>().ok()) else {
        println!("usage: regex_scan <lines>");
        std::process::exit(2);
    };
    let level_re = Regex::new(r"\b(?:ERROR|WARN)\b").unwrap();
    let pair_re = Regex::new(r"(?P<key>[a-z]+)=(\d+)").unwrap();
    let number_re = Regex::new(r"\d+").unwrap();
    let address_re = Regex::new(r"([a-z]+)@([a-z]+)\.example").unwrap();
    let separator_re = Regex::new(r"[,;]\s*").unwrap();

    let mut rng = Rng { state: 97 };
    let levels = ["INFO", "DEBUG", "WARN", "INFO", "ERROR", "INFO", "TRACE", "INFO"];
    let names = ["ada", "grace", "linus", "barbara", "ken", "margaret", "dennis", "frances"];
    let hosts = ["mail", "corp", "lab", "dev"];
    let (mut flagged, mut pairs, mut pair_sum, mut numbers) = (0i64, 0i64, 0i64, 0i64);
    let (mut spans, mut rewritten_len, mut piece_count, mut checksum) = (0i64, 0i64, 0i64, 0i64);
    for i in 0..n {
        let level = levels[(rng.next() % 8) as usize];
        let name = names[(rng.next() % 8) as usize];
        let host = hosts[(rng.next() % 4) as usize];
        let line = format!(
            "{i} {level} user={} took={}ms from {name}@{host}.example; tags a{}, b{};c",
            rng.next() % 5000,
            rng.next() % 900,
            rng.next() % 10,
            rng.next() % 10,
        );
        if level_re.is_match(&line) {
            flagged += 1;
        }

        if let Some(groups) = pair_re.captures(&line) {
            pairs += 1;
            pair_sum += groups[2].parse::<i64>().unwrap();
            checksum = (checksum * 31 + groups["key"].chars().count() as i64) % 1000000007;
        }

        for found in number_re.find_iter(&line) {
            numbers += 1;
            // The lines are ASCII, so byte offsets are character offsets.
            spans += (found.end() - found.start()) as i64;
            checksum = (checksum * 31 + found.start() as i64) % 1000000007;
        }

        let rewritten = address_re.replace_all(&line, "$2/$1");
        rewritten_len += rewritten.chars().count() as i64;

        let pieces: Vec<&str> = separator_re.split(&line).collect();
        piece_count += pieces.len() as i64;
        checksum = (checksum * 31 + pieces[pieces.len() - 1].chars().count() as i64) % 1000000007;
    }

    println!(
        "regex_scan lines={n} flagged={flagged} pairs={pairs} pair_sum={pair_sum} \
         numbers={numbers} spans={spans} rewritten={rewritten_len} pieces={piece_count} \
         checksum={checksum}"
    );
}
