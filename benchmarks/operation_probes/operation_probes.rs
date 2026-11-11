// Per-operation probes: the Rust counterpart of operation_probes.au.
//
// `operation_probes <operation> <count> run` sets up the operation's data and
// then performs the operation `count` times. `... setup` only sets up. Each
// operation is written the way Rust code would do the same work, and prints
// the same line as the Aura version.
use std::collections::HashMap;
use std::hint::black_box;

struct Point {
    x: f64,
    y: f64,
}

struct Counter {
    name: String,
    count: i64,
}

enum Shape {
    Circle(f64),
    Rect(f64, f64),
}

fn area(shape: &Shape) -> f64 {
    match shape {
        Shape::Circle(r) => r * r,
        Shape::Rect(w, h) => w * h,
    }
}

fn while_step(n: i64, run: bool) -> i64 {
    let mut total = 0i64;
    if run {
        let mut i = 0;
        while i < n {
            total += i;
            i += 1;
        }
    }
    total
}

fn range_step(n: i64, run: bool) -> i64 {
    let mut total = 0i64;
    if run {
        for i in 0..n {
            total += i;
        }
    }
    total
}

fn floats(n: i64) -> Vec<f64> {
    let mut xs = Vec::new();
    for i in 0..n {
        xs.push((i % 7) as f64);
    }
    xs
}

fn float_read(n: i64, run: bool) -> i64 {
    let xs = floats(n);
    let mut total = 0.0;
    if run {
        let mut i = 0;
        while i < xs.len() {
            total += xs[i];
            i += 1;
        }
    }
    total as i64
}

fn float_write(n: i64, run: bool) -> i64 {
    let mut xs = floats(n);
    if run {
        let mut i = 0;
        while i < xs.len() {
            xs[i] *= 2.0;
            i += 1;
        }
    }
    xs[n as usize - 1] as i64
}

fn int32_read(n: i64, run: bool) -> i64 {
    let mut xs: Vec<i32> = Vec::new();
    for i in 0..n {
        xs.push((i % 100) as i32);
    }
    let mut total = 0i32;
    if run {
        let mut i = 0;
        while i < xs.len() {
            total += xs[i];
            i += 1;
        }
    }
    total as i64
}

fn u8_cast(n: i64, run: bool) -> i64 {
    let mut bytes: Vec<u8> = Vec::new();
    for i in 0..n {
        bytes.push((i % 200) as u8);
    }
    let mut total = 0u32;
    if run {
        let mut i = 0;
        while i < bytes.len() {
            total += bytes[i] as u32;
            i += 1;
        }
    }
    total as i64
}

fn sort_ints(n: i64, run: bool) -> i64 {
    let mut xs: Vec<i64> = Vec::new();
    let mut seed = 12345i64;
    while (xs.len() as i64) < n {
        seed = seed * 48271 % 2147483647;
        xs.push(seed);
    }
    if run {
        xs.sort();
    }
    xs[0] + xs[n as usize - 1]
}

fn str_slice(n: i64, run: bool) -> i64 {
    let mut text = String::new();
    while text.len() < 10000 {
        text += "abcdefghij";
    }
    let mut total = 0i64;
    if run {
        let mut i = 0;
        while i < n {
            let part = text[5000..5010].to_string();
            total += black_box(&part).len() as i64;
            i += 1;
        }
    }
    total
}

fn dict_remove(n: i64, run: bool) -> i64 {
    let mut table: HashMap<i64, i64> = HashMap::new();
    for i in 0..n {
        table.insert(i, i);
    }
    if run {
        for i in 0..n {
            table.remove(&i);
        }
    }
    table.len() as i64
}

fn dict_get_str(n: i64, run: bool) -> i64 {
    let mut keys: Vec<String> = Vec::new();
    let mut table: HashMap<String, i64> = HashMap::new();
    for i in 0..1000 {
        keys.push(format!("key{i}"));
        table.insert(format!("key{i}"), i);
    }
    let mut total = 0i64;
    if run {
        for i in 0..n {
            if let Some(value) = table.get(&keys[(i % 1000) as usize]) {
                total += value;
            }
        }
    }
    total
}

fn list_push(n: i64, run: bool) -> i64 {
    let mut xs: Vec<f64> = Vec::new();
    let mut ps: Vec<Point> = Vec::new();
    if run {
        for _ in 0..n {
            xs.push(1.5);
            ps.push(Point { x: 1.0, y: 2.0 });
        }
    }
    black_box(&ps);
    (xs.len() + ps.len()) as i64
}

fn class_field_read(n: i64, run: bool) -> i64 {
    let mut ps: Vec<Point> = Vec::new();
    for i in 0..n {
        ps.push(Point {
            x: (i % 7) as f64,
            y: 2.0,
        });
    }
    let mut total = 0.0;
    if run {
        let mut i = 0;
        while i < ps.len() {
            total += ps[i].x;
            i += 1;
        }
    }
    black_box(ps[0].y);
    total as i64
}

fn field_write(n: i64, run: bool) -> i64 {
    let mut counters: Vec<Counter> = Vec::new();
    for i in 0..n {
        counters.push(Counter {
            name: "c".to_string(),
            count: i,
        });
    }
    if run {
        let mut i = 0;
        while i < counters.len() {
            counters[i].count += 1;
            i += 1;
        }
    }
    black_box(&counters[0].name);
    counters[n as usize - 1].count
}

fn enum_match(n: i64, run: bool) -> i64 {
    let mut shapes: Vec<Shape> = Vec::new();
    for i in 0..n {
        if i % 2 == 0 {
            shapes.push(Shape::Circle(2.0));
        } else {
            shapes.push(Shape::Rect(1.0, 3.0));
        }
    }
    let mut total = 0.0;
    if run {
        let mut i = 0;
        while i < shapes.len() {
            total += area(&shapes[i]);
            i += 1;
        }
    }
    total as i64
}

fn add_scaled(total: i64, i: i64) -> i64 {
    total + i * 3
}

fn call(n: i64, run: bool) -> i64 {
    let mut total: i64 = 0;
    if run {
        for i in 0..n {
            total = add_scaled(total, std::hint::black_box(i));
        }
    } else {
        for i in 0..n {
            total = total + std::hint::black_box(i) * 3;
        }
    }
    total
}

struct Upto {
    i: i64,
    n: i64,
}

impl Iterator for Upto {
    type Item = i64;

    fn next(&mut self) -> Option<i64> {
        if self.i == self.n {
            return None;
        }
        self.i += 1;
        Some(self.i - 1)
    }
}

fn iterator_next(n: i64, run: bool) -> i64 {
    let mut total = 0i64;
    if run {
        for i in (Upto { i: 0, n: black_box(n) }) {
            total += i;
        }
    }
    total
}

// Rust has no stable generators; a closure iterator is the usual stand-in.
fn upto(n: i64) -> impl Iterator<Item = i64> {
    let mut i = 0;
    std::iter::from_fn(move || {
        if i < n {
            i += 1;
            Some(i - 1)
        } else {
            None
        }
    })
}

fn generator_next(n: i64, run: bool) -> i64 {
    let mut total = 0i64;
    if run {
        for i in upto(black_box(n)) {
            total += i;
        }
    }
    total
}

fn generator_start(n: i64, run: bool) -> i64 {
    let mut total = 0i64;
    if run {
        for i in 0..n {
            for x in upto(black_box(1)) {
                total += x + i;
            }
        }
    } else {
        for i in 0..n {
            total += black_box(i);
        }
    }
    total
}

fn sum_items(source: impl Iterator<Item = i64>) -> i64 {
    let mut total = 0i64;
    for x in source {
        total += x;
    }
    total
}

fn generator_value(n: i64, run: bool) -> i64 {
    if run {
        return sum_items(upto(black_box(n)));
    }
    0
}

fn doubled(source: impl Iterator<Item = i64>) -> impl Iterator<Item = i64> {
    source.map(|x| x * 2)
}

fn generator_pipeline(n: i64, run: bool) -> i64 {
    if run {
        return sum_items(doubled(upto(black_box(n))));
    }
    0
}

fn generator_list(n: i64, run: bool) -> i64 {
    let mut xs: Vec<i64> = Vec::new();
    for i in 0..n {
        xs.push(i);
    }
    let xs = black_box(xs);
    if run {
        let mut total = 0i64;
        for x in xs.into_iter() {
            total += x;
        }
        total
    } else {
        xs.len() as i64
    }
}

fn walk(n: i64) -> impl Iterator<Item = i64> {
    let mut at = Counter { name: "walker".to_string(), count: 0 };
    std::iter::from_fn(move || {
        if at.count < n {
            at.count += 1;
            Some(at.count - 1)
        } else {
            let _ = &at.name;
            None
        }
    })
}

fn generator_class(n: i64, run: bool) -> i64 {
    let mut total = 0i64;
    if run {
        for x in walk(black_box(n)) {
            total += x;
        }
    }
    total
}

fn str_append(n: i64, run: bool) -> i64 {
    let mut text = String::new();
    if run {
        for _ in 0..black_box(n) {
            text.push_str(black_box("ab"));
        }
    }
    text.len() as i64
}

fn str_format(n: i64, run: bool) -> i64 {
    use std::fmt::Write;
    let mut text = String::new();
    if run {
        for i in 0..black_box(n) {
            write!(text, "{i},").unwrap();
        }
    }
    text.len() as i64
}

trait Solid {
    fn volume(&self) -> i64;
}

struct Cube {
    side: i64,
}

struct Slab {
    width: i64,
    depth: i64,
}

struct Rod {
    length: i64,
    step: i64,
}

impl Solid for Cube {
    fn volume(&self) -> i64 {
        self.side * self.side
    }
}

impl Solid for Slab {
    fn volume(&self) -> i64 {
        self.width * self.depth
    }
}

impl Solid for Rod {
    fn volume(&self) -> i64 {
        self.length + self.step
    }
}

fn trait_object_call(n: i64, run: bool) -> i64 {
    let mut solids: Vec<Box<dyn Solid>> = Vec::new();
    for i in 0..n {
        match i % 3 {
            0 => solids.push(Box::new(Cube { side: i % 7 })),
            1 => solids.push(Box::new(Slab {
                width: i % 5,
                depth: i % 11,
            })),
            _ => solids.push(Box::new(Rod {
                length: i % 13,
                step: 1,
            })),
        }
    }
    let mut total = 0i64;
    if run {
        for solid in black_box(&solids) {
            total += solid.volume();
        }
    }
    total + solids.len() as i64
}

fn probe(operation: &str, n: i64, run: bool) -> i64 {
    match operation {
        "while_step" => while_step(n, run),
        "range_step" => range_step(n, run),
        "float_read" => float_read(n, run),
        "float_write" => float_write(n, run),
        "int32_read" => int32_read(n, run),
        "u8_cast" => u8_cast(n, run),
        "sort_ints" => sort_ints(n, run),
        "str_slice" => str_slice(n, run),
        "dict_remove" => dict_remove(n, run),
        "dict_get_str" => dict_get_str(n, run),
        "list_push" => list_push(n, run),
        "class_field_read" => class_field_read(n, run),
        "field_write" => field_write(n, run),
        "enum_match" => enum_match(n, run),
        "call" => call(n, run),
        "iterator_next" => iterator_next(n, run),
        "generator_next" => generator_next(n, run),
        "generator_start" => generator_start(n, run),
        "generator_value" => generator_value(n, run),
        "generator_pipeline" => generator_pipeline(n, run),
        "generator_list" => generator_list(n, run),
        "generator_class" => generator_class(n, run),
        "str_append" => str_append(n, run),
        "str_format" => str_format(n, run),
        "trait_object_call" => trait_object_call(n, run),
        _ => -1,
    }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let usage = "usage: operation_probes <operation> <count> run|setup";
    if args.len() != 3 {
        println!("{usage}");
        return;
    }
    match args[1].parse::<i64>() {
        Ok(count) => println!("{} {}", args[0], probe(&args[0], count, args[2] == "run")),
        Err(_) => println!("{usage}"),
    }
}
