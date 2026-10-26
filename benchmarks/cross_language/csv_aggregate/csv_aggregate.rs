//! CSV analytics: parse a sales export and aggregate it by region, product, and month.
//!
//! Generates N rows of CSV text, then parses every line, computes discounted
//! revenue, groups it three ways, and ranks the five best-selling products.
use std::collections::HashMap;

const REGIONS: [&str; 6] = ["north", "south", "east", "west", "central", "coastal"];

struct Rng {
    state: i64,
}

impl Rng {
    fn next(&mut self) -> i64 {
        self.state = self.state * 48271 % 2147483647;
        self.state
    }
}

fn two_digits(value: i64) -> String {
    if value < 10 {
        format!("0{value}")
    } else {
        format!("{value}")
    }
}

fn generate(rng: &mut Rng, n: i64) -> String {
    let mut lines = vec!["date,region,product,quantity,unit_price_cents,discount_pct".to_string()];
    for _ in 0..n {
        let r = rng.next();
        let s = rng.next();
        let month = two_digits(1 + r % 12);
        let day = two_digits(1 + (r / 12) % 28);
        let region = REGIONS[((r / 336) % 6) as usize];
        let quantity = 1 + (s / 500) % 50;
        let price = 100 + (s / 25000) % 99900;
        let discount = (r / 2016) % 31;
        lines.push(format!(
            "2026-{month}-{day},{region},SKU-{},{quantity},{price},{discount}",
            s % 500
        ));
    }
    lines.join("\n") + "\n"
}

/// Adds to a counter, keeping the key's first-seen position for stable ranking.
fn add_to(
    totals: &mut HashMap<String, usize>,
    order: &mut Vec<(String, i64)>,
    key: &str,
    amount: i64,
) {
    match totals.get(key) {
        Some(&slot) => order[slot].1 += amount,
        None => {
            totals.insert(key.to_string(), order.len());
            order.push((key.to_string(), amount));
        }
    }
}

fn main() {
    let n: i64 = std::env::args().nth(1).unwrap().parse().unwrap();
    let text = generate(&mut Rng { state: 17 }, n);
    let (mut region_slots, mut regions) = (HashMap::new(), Vec::new());
    let (mut product_slots, mut products) = (HashMap::new(), Vec::new());
    let (mut month_slots, mut months) = (HashMap::new(), Vec::new());
    let (mut rows, mut total_revenue) = (0i64, 0i64);
    for line in text.lines().skip(1) {
        let fields: Vec<&str> = line.split(',').collect();
        let month = fields[0].split('-').nth(1).unwrap();
        let quantity: i64 = fields[3].parse().unwrap();
        let price: i64 = fields[4].parse().unwrap();
        let discount: i64 = fields[5].parse().unwrap();
        let revenue = quantity * price * (100 - discount) / 100;
        rows += 1;
        total_revenue += revenue;
        add_to(&mut region_slots, &mut regions, fields[1], revenue);
        add_to(&mut product_slots, &mut products, fields[2], quantity);
        add_to(&mut month_slots, &mut months, month, 1);
    }
    let mut ranked = products.clone();
    ranked.sort_by_key(|entry| -entry.1);
    let top: Vec<String> = ranked[..5]
        .iter()
        .map(|(product, quantity)| format!("{product}:{quantity}"))
        .collect();
    let region_totals: Vec<String> = REGIONS
        .iter()
        .map(|region| {
            let revenue = region_slots.get(*region).map_or(0, |&slot| regions[slot].1);
            format!("{region}:{revenue}")
        })
        .collect();
    let busiest = months.iter().map(|entry| entry.1).max().unwrap();
    println!(
        "csv_aggregate rows={rows} bytes={} revenue={total_revenue} products={} busiest_month={busiest} regions={} top={}",
        text.len(),
        products.len(),
        region_totals.join(" "),
        top.join(" ")
    );
}
