//! Document database: schemaless JSON records with a primary and a secondary index.
//!
//! Inserts N documents, runs point reads, an indexed filter query, in-place
//! updates, deletes, a per-city aggregate, then saves every live document as
//! JSON lines and loads the file back into a fresh database.
use serde_json::{json, Map, Value};
use std::collections::HashMap;
use std::fs;
use std::path::Path;

const CITIES: [&str; 8] = [
    "Lagos", "Abuja", "Accra", "Nairobi", "Cairo", "Dakar", "Kigali", "Tunis",
];

struct Rng {
    state: i64,
}

impl Rng {
    fn next(&mut self) -> i64 {
        self.state = self.state * 48271 % 2147483647;
        self.state
    }
}

type Doc = Map<String, Value>;

#[derive(Default)]
struct Database {
    docs: HashMap<i64, Doc>,
    by_city: HashMap<String, Vec<i64>>,
}

fn int_field(doc: &Doc, name: &str) -> i64 {
    doc.get(name).and_then(Value::as_i64).unwrap()
}

impl Database {
    fn insert(&mut self, doc: Doc) {
        let id = int_field(&doc, "id");
        let city = doc["city"].as_str().unwrap().to_string();
        self.docs.insert(id, doc);
        self.by_city.entry(city).or_default().push(id);
    }

    fn get(&self, id: i64) -> Option<&Doc> {
        self.docs.get(&id)
    }

    fn delete(&mut self, id: i64) -> bool {
        self.docs.remove(&id).is_some()
    }

    fn count_where(&self, city: &str, min_age: i64) -> i64 {
        let mut matched = 0;
        for id in self.by_city.get(city).map(Vec::as_slice).unwrap_or(&[]) {
            if let Some(doc) = self.docs.get(id) {
                if doc["active"].as_bool().unwrap() && int_field(doc, "age") >= min_age {
                    matched += 1;
                }
            }
        }
        matched
    }
}

fn make_doc(i: i64, r: i64) -> Doc {
    let value = json!({
        "id": i,
        "name": format!("user-{i}"),
        "age": 18 + r % 60,
        "city": CITIES[((r / 60) % 8) as usize],
        "score": (r / 480) % 1000,
        "active": i % 3 != 0,
        "tags": [format!("t{}", r % 7), format!("t{}", r % 5)],
    });
    match value {
        Value::Object(map) => map,
        _ => unreachable!(),
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let n: i64 = args[1].parse().unwrap();
    let workdir = &args[2];
    let mut rng = Rng { state: 42 };
    let mut db = Database::default();
    for i in 0..n {
        let r = rng.next();
        db.insert(make_doc(i, r));
    }

    let (mut hits, mut misses, mut score_total) = (0i64, 0i64, 0i64);
    for _ in 0..n * 2 {
        match db.get(rng.next() % (n + n / 10)) {
            Some(doc) => {
                hits += 1;
                score_total += int_field(doc, "score");
            }
            None => misses += 1,
        }
    }

    let mut matched = 0;
    for city in CITIES {
        matched += db.count_where(city, 40);
    }

    for id in (0..n).step_by(10) {
        let doc = db.docs.get_mut(&id).unwrap();
        let score = int_field(doc, "score");
        doc.insert("score".to_string(), Value::from(score + 7));
        doc.get_mut("tags")
            .unwrap()
            .as_array_mut()
            .unwrap()
            .push(Value::from("updated"));
    }

    let mut deleted = 0;
    for id in (1..n).step_by(20) {
        if db.delete(id) {
            deleted += 1;
        }
    }

    let mut city_scores: HashMap<&str, i64> = HashMap::new();
    let mut city_counts: HashMap<&str, i64> = HashMap::new();
    for doc in db.docs.values() {
        let city = doc["city"].as_str().unwrap();
        *city_scores.entry(city).or_default() += int_field(doc, "score");
        *city_counts.entry(city).or_default() += 1;
    }
    let mut average_total = 0;
    for city in CITIES {
        let count = city_counts.get(city).copied().unwrap_or(0);
        if count > 0 {
            average_total += city_scores[city] * 100 / count;
        }
    }

    let path = Path::new(workdir).join("docdb.jsonl");
    let mut text = String::new();
    for doc in db.docs.values() {
        text.push_str(&serde_json::to_string(doc).unwrap());
        text.push('\n');
    }
    fs::write(&path, &text).unwrap();
    let saved_bytes = text.len();

    let loaded = fs::read_to_string(&path).unwrap();
    let mut reloaded = Database::default();
    for line in loaded.lines() {
        let doc: Doc = serde_json::from_str(line).unwrap();
        reloaded.insert(doc);
    }
    fs::remove_file(&path).unwrap();

    let mut checksum = 0;
    for doc in reloaded.docs.values() {
        checksum += int_field(doc, "score")
            + int_field(doc, "age")
            + doc["tags"].as_array().unwrap().len() as i64;
    }
    println!(
        "docdb docs={n} live={} hits={hits} misses={misses} score={score_total} matched={matched} \
         deleted={deleted} averages={average_total} bytes={saved_bytes} checksum={checksum}",
        reloaded.docs.len()
    );
}
