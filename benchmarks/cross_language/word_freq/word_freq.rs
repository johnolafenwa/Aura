//! Text processing: tokenize a generated corpus and count words and bigrams.
//!
//! Builds a corpus of N words with mixed case and punctuation, then splits it
//! into lines and tokens, normalizes each token, counts words and adjacent word
//! pairs, and ranks the ten most frequent words.
use std::collections::HashMap;

const SYLLABLES: [&str; 12] = [
    "ka", "lo", "mi", "ra", "zu", "te", "no", "bi", "sa", "de", "gu", "pe",
];
const VOCABULARY: i64 = 2000;

struct Rng {
    state: i64,
}

impl Rng {
    fn next(&mut self) -> i64 {
        self.state = self.state * 48271 % 2147483647;
        self.state
    }
}

fn build_corpus(rng: &mut Rng, n: i64) -> String {
    let mut vocabulary = Vec::new();
    for _ in 0..VOCABULARY {
        let r = rng.next();
        let mut word = String::new();
        for k in 0..1 + r % 4 {
            word.push_str(SYLLABLES[((r / (k * 12 + 4)) % 12) as usize]);
        }
        vocabulary.push(word);
    }
    let mut lines = Vec::new();
    let mut line: Vec<String> = Vec::new();
    for i in 0..n {
        let a = rng.next() % VOCABULARY;
        let b = rng.next() % VOCABULARY;
        let r = rng.next();
        let mut word = vocabulary[(a * b / VOCABULARY) as usize].clone();
        if r % 7 == 0 {
            word = word.to_uppercase();
        }
        if r % 11 == 0 {
            word.push(',');
        } else if r % 13 == 0 {
            word.push('.');
        }
        line.push(word);
        if i % 12 == 11 {
            lines.push(line.join(" "));
            line.clear();
        }
    }
    if !line.is_empty() {
        lines.push(line.join(" "));
    }
    lines.join("\n") + "\n"
}

/// Counts a key, keeping its first-seen position for stable ranking.
fn bump(slots: &mut HashMap<String, usize>, counts: &mut Vec<(String, i64)>, key: String) {
    match slots.get(&key) {
        Some(&slot) => counts[slot].1 += 1,
        None => {
            slots.insert(key.clone(), counts.len());
            counts.push((key, 1));
        }
    }
}

fn main() {
    let n: i64 = std::env::args().nth(1).unwrap().parse().unwrap();
    let corpus = build_corpus(&mut Rng { state: 3 }, n);
    let (mut slots, mut counts) = (HashMap::new(), Vec::new());
    let mut bigrams: HashMap<String, i64> = HashMap::new();
    let mut tokens = 0;
    for line in corpus.lines() {
        let mut previous: Option<String> = None;
        for token in line.split_whitespace() {
            let mut word = token.to_lowercase();
            if word.ends_with(',') || word.ends_with('.') {
                word.pop();
            }
            tokens += 1;
            bump(&mut slots, &mut counts, word.clone());
            if let Some(previous) = &previous {
                *bigrams.entry(format!("{previous} {word}")).or_default() += 1;
            }
            previous = Some(word);
        }
    }
    let mut ranked = counts.clone();
    ranked.sort_by_key(|entry| -entry.1);
    let top: Vec<String> = ranked[..10]
        .iter()
        .map(|(word, count)| format!("{word}:{count}"))
        .collect();
    let weight: i64 = counts
        .iter()
        .map(|(word, count)| count * word.len() as i64)
        .sum();
    println!(
        "word_freq tokens={tokens} bytes={} distinct={} bigrams={} weight={weight} top={}",
        corpus.len(),
        counts.len(),
        bigrams.len(),
        top.join(" ")
    );
}
