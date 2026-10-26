//! JSON parse and serialize: an API-style payload parsed, walked, and dumped N times.
//!
//! Builds a 1,000-record order payload, serializes it once, then each round
//! parses the text, walks the tree for statistics, and serializes it again.
use serde_json::{json, Value};

const RECORDS: i64 = 1000;

struct Rng {
    state: i64,
}

impl Rng {
    fn next(&mut self) -> i64 {
        self.state = self.state * 48271 % 2147483647;
        self.state
    }
}

fn build_payload(rng: &mut Rng) -> Value {
    let mut orders = Vec::new();
    for i in 0..RECORDS {
        let r = rng.next();
        let note = if r % 4 == 0 {
            Value::from(format!("gift \"wrap\" \\ {i}\n"))
        } else {
            Value::Null
        };
        let items: Vec<Value> = (0..1 + r % 3)
            .map(|_| json!({"sku": format!("SKU-{}", (r / 11) % 900), "units": 1 + (r / 13) % 5}))
            .collect();
        orders.push(json!({
            "id": i,
            "customer": format!("customer-{}", r % 500),
            "price": (2 * (r % 50000) + 1) as f64 / 1000.0,
            "quantity": 1 + (r / 7) % 20,
            "express": r % 3 == 0,
            "note": note,
            "items": items,
            "address": {"city": format!("city-{}", r % 40), "zip": 10000 + r % 90000},
        }));
    }
    json!({"version": 3, "source": "benchmark", "orders": orders})
}

#[derive(Default)]
struct Stats {
    ints: i64,
    floats: i64,
    strings: i64,
    string_bytes: i64,
    nulls: i64,
    bools: i64,
    containers: i64,
}

fn walk(value: &Value, stats: &mut Stats) {
    match value {
        Value::Null => stats.nulls += 1,
        Value::Bool(_) => stats.bools += 1,
        Value::Number(number) => match number.as_i64() {
            Some(int) => stats.ints += int,
            None => stats.floats += (number.as_f64().unwrap() * 1000.0).round_ties_even() as i64,
        },
        Value::String(text) => {
            stats.strings += 1;
            stats.string_bytes += text.len() as i64;
        }
        Value::Array(items) => {
            stats.containers += 1;
            for item in items {
                walk(item, stats);
            }
        }
        Value::Object(fields) => {
            stats.containers += 1;
            for item in fields.values() {
                walk(item, stats);
            }
        }
    }
}

fn main() {
    let rounds: i64 = std::env::args().nth(1).unwrap().parse().unwrap();
    let text = build_payload(&mut Rng { state: 5 }).to_string();
    let mut stats = Stats::default();
    let mut dumped_bytes = 0;
    for _ in 0..rounds {
        let value: Value = serde_json::from_str(&text).unwrap();
        walk(&value, &mut stats);
        let again = value.to_string();
        assert!(again == text, "round trip changed the text");
        dumped_bytes += again.len();
    }
    println!(
        "json_roundtrip rounds={rounds} text_bytes={} ints={} floats={} strings={} string_bytes={} nulls={} bools={} containers={} dumped_bytes={dumped_bytes}",
        text.len(),
        stats.ints,
        stats.floats,
        stats.strings,
        stats.string_bytes,
        stats.nulls,
        stats.bools,
        stats.containers
    );
}
