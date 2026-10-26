//! Databases: a small SQL engine with a tokenizer, parser, planner, and executor.
//!
//! Runs N INSERT statements into a users table, then N more statements mixing
//! point SELECTs, UPDATEs and DELETEs by id (planned onto a hash index), new
//! INSERTs, filtered range scans, and GROUP BY aggregates. Every statement arrives
//! as SQL text, so each one is tokenized, parsed into a statement value, and
//! executed against row storage with an id index. Reports the rows affected and
//! a checksum of everything the SELECTs returned.
use std::collections::HashMap;

const CITIES: [&str; 8] = [
    "paris", "lagos", "tokyo", "lima", "oslo", "cairo", "quito", "perth",
];
const SYLLABLES: [&str; 8] = ["ka", "lo", "mi", "ra", "to", "ne", "su", "vi"];
const WORD: u8 = 0;
const NUMBER: u8 = 1;
const TEXT: u8 = 2;
const SYMBOL: u8 = 3;
const END: u8 = 4;
const EQ: i64 = 0;
const NE: i64 = 1;
const LT: i64 = 2;
const LE: i64 = 3;
const GT: i64 = 4;
const COLUMN: i64 = 0;
const COUNT: i64 = 1;
const SUM: i64 = 2;
const MAX: i64 = 3;
const MOD: i64 = 1000000007;

struct Rng {
    state: i64,
}

impl Rng {
    fn next(&mut self) -> i64 {
        self.state = self.state * 48271 % 2147483647;
        self.state
    }
}

#[derive(Clone, PartialEq)]
enum Value {
    Int(i64),
    Text(String),
}

struct Condition {
    column: usize,
    op: i64,
    value: Value,
}

enum Statement {
    Insert(Vec<Value>),
    Select(Vec<(i64, usize)>, Vec<Condition>, i64),
    Update(usize, i64, Value, Vec<Condition>),
    Delete(Vec<Condition>),
}

struct Tokens {
    kinds: Vec<u8>,
    texts: Vec<String>,
    numbers: Vec<i64>,
}

fn is_word_byte(c: u8) -> bool {
    c.is_ascii_alphabetic() || c == 95
}

fn tokenize(sql: &str) -> Tokens {
    let data = sql.as_bytes();
    let mut t = Tokens {
        kinds: Vec::new(),
        texts: Vec::new(),
        numbers: Vec::new(),
    };
    let n = data.len();
    let mut i = 0;
    while i < n {
        let c = data[i];
        if c == 32 {
            i += 1;
        } else if c.is_ascii_digit() {
            let mut value = 0i64;
            while i < n && data[i].is_ascii_digit() {
                value = value * 10 + (data[i] - 48) as i64;
                i += 1;
            }
            t.kinds.push(NUMBER);
            t.texts.push(String::new());
            t.numbers.push(value);
        } else if c == 39 {
            let start = i + 1;
            i = start;
            while data[i] != 39 {
                i += 1;
            }
            t.kinds.push(TEXT);
            t.texts.push(sql[start..i].to_string());
            t.numbers.push(0);
            i += 1;
        } else if is_word_byte(c) {
            let start = i;
            while i < n && is_word_byte(data[i]) {
                i += 1;
            }
            t.kinds.push(WORD);
            t.texts.push(sql[start..i].to_string());
            t.numbers.push(0);
        } else {
            t.kinds.push(SYMBOL);
            if (c == 60 || c == 62 || c == 33) && i + 1 < n && data[i + 1] == 61 {
                t.texts.push(sql[i..i + 2].to_string());
                i += 2;
            } else {
                t.texts.push(sql[i..i + 1].to_string());
                i += 1;
            }
            t.numbers.push(0);
        }
    }
    t.kinds.push(END);
    t.texts.push(String::new());
    t.numbers.push(0);
    t
}

fn op_code(text: &str) -> i64 {
    match text {
        "=" => 0,
        "!=" => 1,
        "<" => 2,
        "<=" => 3,
        ">" => 4,
        _ => 5,
    }
}

struct Parser<'a> {
    tokens: Tokens,
    pos: usize,
    columns: &'a HashMap<String, usize>,
}

impl<'a> Parser<'a> {
    fn word(&mut self) -> String {
        self.pos += 1;
        self.tokens.texts[self.pos - 1].clone()
    }

    fn is_word(&self, text: &str) -> bool {
        self.tokens.kinds[self.pos] == WORD && self.tokens.texts[self.pos] == text
    }

    fn column(&mut self) -> usize {
        let name = self.word();
        self.columns[&name]
    }

    fn literal(&mut self) -> Value {
        self.pos += 1;
        if self.tokens.kinds[self.pos - 1] == NUMBER {
            Value::Int(self.tokens.numbers[self.pos - 1])
        } else {
            Value::Text(self.tokens.texts[self.pos - 1].clone())
        }
    }

    fn conditions(&mut self) -> Vec<Condition> {
        let mut result = Vec::new();
        if self.is_word("WHERE") {
            self.pos += 1;
            loop {
                let column = self.column();
                let op = op_code(&self.word());
                let value = self.literal();
                result.push(Condition { column, op, value });
                if self.is_word("AND") {
                    self.pos += 1;
                } else {
                    break;
                }
            }
        }
        result
    }

    fn statement(&mut self) -> Statement {
        let verb = self.word();
        if verb == "INSERT" {
            self.pos += 4;
            let mut values = vec![self.literal()];
            while self.tokens.texts[self.pos] == "," {
                self.pos += 1;
                values.push(self.literal());
            }
            return Statement::Insert(values);
        }
        if verb == "SELECT" {
            let mut items = Vec::new();
            loop {
                let name = self.word();
                if name == "COUNT" {
                    self.pos += 3;
                    items.push((COUNT, 0));
                } else if name == "SUM" || name == "MAX" {
                    self.pos += 1;
                    let column = self.column();
                    self.pos += 1;
                    items.push((if name == "SUM" { SUM } else { MAX }, column));
                } else {
                    items.push((COLUMN, self.columns[&name]));
                }
                if self.tokens.texts[self.pos] == "," {
                    self.pos += 1;
                } else {
                    break;
                }
            }
            self.pos += 2;
            let conditions = self.conditions();
            let mut group = -1;
            if self.is_word("GROUP") {
                self.pos += 2;
                group = self.column() as i64;
            }
            return Statement::Select(items, conditions, group);
        }
        if verb == "UPDATE" {
            self.pos += 2;
            let target = self.column();
            self.pos += 1;
            let (source, delta) = if self.tokens.kinds[self.pos] == WORD {
                let source = self.column() as i64;
                self.pos += 1;
                (source, self.literal())
            } else {
                (-1, self.literal())
            };
            return Statement::Update(target, source, delta, self.conditions());
        }
        self.pos += 2;
        Statement::Delete(self.conditions())
    }
}

fn compare(cell: &Value, op: i64, value: &Value) -> bool {
    match (cell, value) {
        (Value::Int(a), Value::Int(b)) => match op {
            EQ => a == b,
            NE => a != b,
            LT => a < b,
            LE => a <= b,
            GT => a > b,
            _ => a >= b,
        },
        (Value::Text(a), Value::Text(b)) => match op {
            EQ => a == b,
            NE => a != b,
            _ => false,
        },
        _ => false,
    }
}

fn matches(row: &[Value], conditions: &[Condition]) -> bool {
    conditions
        .iter()
        .all(|c| compare(&row[c.column], c.op, &c.value))
}

fn value_hash(value: &Value) -> i64 {
    match value {
        Value::Int(v) => *v,
        Value::Text(s) => s.len() as i64 * 131 + s.as_bytes()[0] as i64,
    }
}

fn int_of(value: &Value) -> i64 {
    match value {
        Value::Int(v) => *v,
        Value::Text(_) => 0,
    }
}

struct Engine {
    columns: HashMap<String, usize>,
    rows: Vec<Vec<Value>>,
    live: Vec<bool>,
    index: HashMap<i64, usize>,
    inserted: i64,
    selected: i64,
    groups: i64,
    updated: i64,
    deleted: i64,
    scans: i64,
    checksum: i64,
}

impl Engine {
    fn candidates(&mut self, conditions: &[Condition]) -> Vec<usize> {
        // The planner: an equality on id uses the index, anything else scans.
        for c in conditions {
            if c.column == 0 && c.op == EQ {
                return match self.index.get(&int_of(&c.value)) {
                    Some(&slot) => vec![slot],
                    None => Vec::new(),
                };
            }
        }
        self.scans += 1;
        (0..self.rows.len())
            .filter(|&slot| self.live[slot])
            .collect()
    }

    fn execute(&mut self, sql: &str) {
        let tokens = tokenize(sql);
        let statement = Parser {
            tokens,
            pos: 0,
            columns: &self.columns,
        }
        .statement();
        match statement {
            Statement::Insert(values) => {
                self.index.insert(int_of(&values[0]), self.rows.len());
                self.rows.push(values);
                self.live.push(true);
                self.inserted += 1;
            }
            Statement::Select(items, conditions, group) => {
                if group < 0 {
                    for slot in self.candidates(&conditions) {
                        let row = &self.rows[slot];
                        if matches(row, &conditions) {
                            self.selected += 1;
                            for &(_, column) in &items {
                                self.checksum =
                                    (self.checksum * 31 + value_hash(&row[column])) % MOD;
                            }
                        }
                    }
                    return;
                }
                let mut keys: Vec<Value> = Vec::new();
                let mut totals: Vec<Vec<i64>> = Vec::new();
                let mut positions: HashMap<String, usize> = HashMap::new();
                for slot in self.candidates(&conditions) {
                    let row = &self.rows[slot];
                    if !matches(row, &conditions) {
                        continue;
                    }
                    let key = row[group as usize].clone();
                    let name = match &key {
                        Value::Text(s) => s.clone(),
                        Value::Int(v) => v.to_string(),
                    };
                    let position = *positions.entry(name).or_insert_with(|| {
                        keys.push(key);
                        totals.push(vec![0; items.len()]);
                        totals.len() - 1
                    });
                    let state = &mut totals[position];
                    for (i, &(kind, column)) in items.iter().enumerate() {
                        if kind == COUNT {
                            state[i] += 1;
                        } else if kind == SUM {
                            state[i] += int_of(&row[column]);
                        } else if kind == MAX {
                            state[i] = state[i].max(int_of(&row[column]));
                        }
                    }
                }
                for (key, state) in keys.iter().zip(&totals) {
                    self.groups += 1;
                    for &value in state {
                        self.checksum = (self.checksum + value_hash(key) * value) % MOD;
                    }
                }
            }
            Statement::Update(target, source, delta, conditions) => {
                for slot in self.candidates(&conditions) {
                    if matches(&self.rows[slot], &conditions) {
                        let value = if source >= 0 {
                            Value::Int(int_of(&self.rows[slot][source as usize]) + int_of(&delta))
                        } else {
                            delta.clone()
                        };
                        self.rows[slot][target] = value;
                        self.updated += 1;
                    }
                }
            }
            Statement::Delete(conditions) => {
                for slot in self.candidates(&conditions) {
                    if matches(&self.rows[slot], &conditions) {
                        self.live[slot] = false;
                        let id = int_of(&self.rows[slot][0]);
                        self.index.remove(&id);
                        self.deleted += 1;
                    }
                }
            }
        }
    }
}

fn insert_sql(rng: &mut Rng, row_id: i64) -> String {
    let r = rng.next();
    let name = format!(
        "{}{}{}",
        SYLLABLES[(r % 8) as usize],
        SYLLABLES[((r / 8) % 8) as usize],
        SYLLABLES[((r / 64) % 8) as usize]
    );
    format!(
        "INSERT INTO users VALUES ({row_id}, '{name}', {}, '{}')",
        18 + (r / 512) % 60,
        CITIES[((r / 30720) % 8) as usize]
    )
}

fn main() {
    let n: i64 = std::env::args().nth(1).unwrap().parse().unwrap();
    let mut rng = Rng { state: 89 };
    let mut columns = HashMap::new();
    for (i, name) in ["id", "name", "age", "city"].iter().enumerate() {
        columns.insert(name.to_string(), i);
    }
    let mut engine = Engine {
        columns,
        rows: Vec::new(),
        live: Vec::new(),
        index: HashMap::new(),
        inserted: 0,
        selected: 0,
        groups: 0,
        updated: 0,
        deleted: 0,
        scans: 0,
        checksum: 0,
    };
    for row_id in 0..n {
        let sql = insert_sql(&mut rng, row_id);
        engine.execute(&sql);
    }
    let mut next_id = n;
    for _ in 0..n {
        let r = rng.next();
        let kind = r % 1000;
        let target = (r / 1000) % next_id;
        if kind < 500 {
            engine.execute(&format!(
                "SELECT name, age, city FROM users WHERE id = {target}"
            ));
        } else if kind < 700 {
            engine.execute(&format!(
                "UPDATE users SET age = age + 1 WHERE id = {target}"
            ));
        } else if kind < 800 {
            engine.execute(&format!("DELETE FROM users WHERE id = {target}"));
        } else if kind < 993 {
            let sql = insert_sql(&mut rng, next_id);
            engine.execute(&sql);
            next_id += 1;
        } else if kind < 998 {
            let low = 18 + (r / 1000) % 60;
            engine.execute(&format!(
                "SELECT id, name FROM users WHERE age >= {low} AND age < {} AND city = '{}'",
                low + 3,
                CITIES[((r / 7) % 8) as usize]
            ));
        } else {
            engine.execute(
                "SELECT city, COUNT(*), SUM(age), MAX(age) FROM users WHERE age > 20 GROUP BY city",
            );
        }
    }
    println!(
        "sql_engine statements={} inserted={} selected={} groups={} updated={} deleted={} scans={} live={} checksum={}",
        2 * n,
        engine.inserted,
        engine.selected,
        engine.groups,
        engine.updated,
        engine.deleted,
        engine.scans,
        engine.index.len(),
        engine.checksum
    );
}
