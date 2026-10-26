//! Office software: a spreadsheet with formulas, a dependency graph, and recalculation.
//!
//! Builds a sheet of N rows by 8 columns where about half the cells hold numbers
//! and the rest hold formulas such as =B12+C9*3, =SUM(D4:D13), or
//! =MAX(A2:A8)-MIN(A2:A8) that refer to earlier rows. It tokenizes and parses every
//! formula into a syntax tree, links each referenced cell to its dependents, orders
//! the cells topologically (Kahn's algorithm), evaluates them, then applies N / 4
//! edits to number cells, recomputing only the cells downstream of each edit.
//! Values wrap at 1,000,003 so they stay integers.

const COLS: i64 = 8;
const LETTERS: [&str; 8] = ["A", "B", "C", "D", "E", "F", "G", "H"];
const MOD: i64 = 1000003;
const NUMBER: u8 = 0;
const CELL: u8 = 1;
const FUNCTION: u8 = 2;
const SYMBOL: u8 = 3;
const END: u8 = 4;
const SUM: i64 = 0;
const MAX: i64 = 1;
const MIN: i64 = 2;

struct Rng {
    state: i64,
}

impl Rng {
    fn next(&mut self) -> i64 {
        self.state = self.state * 48271 % 2147483647;
        self.state
    }
}

enum Expr {
    Num(i64),
    Ref(usize),
    Func(i64, usize, usize),
    Neg(Box<Expr>),
    Bin(i64, Box<Expr>, Box<Expr>),
}

fn wrap(value: i64) -> i64 {
    (value % MOD + MOD) % MOD
}

fn tokenize(text: &str) -> (Vec<u8>, Vec<i64>) {
    let data = text.as_bytes();
    let (mut kinds, mut values) = (Vec::new(), Vec::new());
    let mut i = 0;
    while i < data.len() {
        let c = data[i];
        if c.is_ascii_digit() {
            let mut value = 0i64;
            while i < data.len() && data[i].is_ascii_digit() {
                value = value * 10 + (data[i] - 48) as i64;
                i += 1;
            }
            kinds.push(NUMBER);
            values.push(value);
        } else if c.is_ascii_uppercase() {
            let start = i;
            while i < data.len() && data[i].is_ascii_uppercase() {
                i += 1;
            }
            if i < data.len() && data[i].is_ascii_digit() {
                let mut row = 0i64;
                while i < data.len() && data[i].is_ascii_digit() {
                    row = row * 10 + (data[i] - 48) as i64;
                    i += 1;
                }
                kinds.push(CELL);
                values.push((row - 1) * COLS + (data[start] - 65) as i64);
            } else {
                kinds.push(FUNCTION);
                values.push(if data[start] == 83 {
                    SUM
                } else if data[start + 1] == 65 {
                    MAX
                } else {
                    MIN
                });
            }
        } else {
            if c != 32 {
                kinds.push(SYMBOL);
                values.push(c as i64);
            }
            i += 1;
        }
    }
    kinds.push(END);
    values.push(0);
    (kinds, values)
}

struct Parser {
    kinds: Vec<u8>,
    values: Vec<i64>,
    pos: usize,
    refs: Vec<usize>,
}

impl Parser {
    fn symbol_is(&self, code: i64) -> bool {
        self.kinds[self.pos] == SYMBOL && self.values[self.pos] == code
    }

    fn expr(&mut self) -> Expr {
        let mut left = self.term();
        while self.symbol_is(43) || self.symbol_is(45) {
            let op = self.values[self.pos];
            self.pos += 1;
            left = Expr::Bin(op, Box::new(left), Box::new(self.term()));
        }
        left
    }

    fn term(&mut self) -> Expr {
        let mut left = self.factor();
        while self.symbol_is(42) {
            self.pos += 1;
            left = Expr::Bin(42, Box::new(left), Box::new(self.factor()));
        }
        left
    }

    fn factor(&mut self) -> Expr {
        let kind = self.kinds[self.pos];
        let value = self.values[self.pos];
        self.pos += 1;
        if kind == NUMBER {
            return Expr::Num(value);
        }
        if kind == CELL {
            self.refs.push(value as usize);
            return Expr::Ref(value as usize);
        }
        if kind == FUNCTION {
            self.pos += 1;
            let first = self.values[self.pos] as usize;
            self.pos += 2;
            let last = self.values[self.pos] as usize;
            self.pos += 2;
            let mut cell = first;
            while cell <= last {
                self.refs.push(cell);
                cell += COLS as usize;
            }
            return Expr::Func(value, first, last);
        }
        if value == 45 {
            return Expr::Neg(Box::new(self.factor()));
        }
        let inner = self.expr();
        self.pos += 1;
        inner
    }
}

fn evaluate(node: &Expr, values: &[i64]) -> i64 {
    match node {
        Expr::Num(value) => *value,
        Expr::Ref(cell) => values[*cell],
        Expr::Func(kind, first, last) => {
            let mut result = if *kind == SUM { 0 } else { values[*first] };
            let mut cell = *first;
            while cell <= *last {
                if *kind == SUM {
                    result = wrap(result + values[cell]);
                } else if *kind == MAX {
                    result = result.max(values[cell]);
                } else {
                    result = result.min(values[cell]);
                }
                cell += COLS as usize;
            }
            result
        }
        Expr::Neg(operand) => wrap(-evaluate(operand, values)),
        Expr::Bin(op, left, right) => {
            let left = evaluate(left, values);
            let right = evaluate(right, values);
            if *op == 43 {
                wrap(left + right)
            } else if *op == 45 {
                wrap(left - right)
            } else {
                wrap(left * right)
            }
        }
    }
}

fn cell_name(cell: i64) -> String {
    format!("{}{}", LETTERS[(cell % COLS) as usize], cell / COLS + 1)
}

fn pick(low: i64, row: i64, salt: i64) -> i64 {
    (low + salt % (row - low)) * COLS + (salt / 7) % COLS
}

fn make_formula(rng: &mut Rng, row: i64) -> String {
    let r = rng.next();
    let low = 0.max(row - 12);
    let kind = r % 4;
    if kind == 0 {
        return format!(
            "={}+{}*{}",
            cell_name(pick(low, row, r / 4)),
            cell_name(pick(low, row, r / 97)),
            r % 9 + 1
        );
    }
    let first_row = low + (r / 4) % (row - low);
    let last_row = (row - 1).min(first_row + 9);
    let column = LETTERS[((r / 13) % COLS) as usize];
    let span = format!("{column}{}:{column}{}", first_row + 1, last_row + 1);
    if kind == 1 {
        return format!("=SUM({span})");
    }
    if kind == 2 {
        return format!("=MAX({span})-MIN({span})");
    }
    format!(
        "=({}-{})*{}+7",
        cell_name(pick(low, row, r / 5)),
        cell_name(pick(low, row, r / 211)),
        r % 5 + 2
    )
}

fn main() {
    let n: i64 = std::env::args().nth(1).unwrap().parse().unwrap();
    let cells = (n * COLS) as usize;
    let mut rng = Rng { state: 79 };
    let mut values = vec![0i64; cells];
    let mut formulas: Vec<Option<Expr>> = (0..cells).map(|_| None).collect();
    let mut dependents: Vec<Vec<usize>> = vec![Vec::new(); cells];
    let mut indegree = vec![0i64; cells];
    let (mut formula_count, mut formula_bytes) = (0, 0);
    for row in 0..n {
        for col in 0..COLS {
            let cell = (row * COLS + col) as usize;
            let r = rng.next();
            if row == 0 || r % 2 == 0 {
                values[cell] = r % 1000;
                continue;
            }
            let text = make_formula(&mut rng, row);
            formula_bytes += text.len();
            let (kinds, token_values) = tokenize(&text[1..]);
            let mut parser = Parser {
                kinds,
                values: token_values,
                pos: 0,
                refs: Vec::new(),
            };
            formulas[cell] = Some(parser.expr());
            formula_count += 1;
            for &reference in &parser.refs {
                dependents[reference].push(cell);
                indegree[cell] += 1;
            }
        }
    }
    let mut order: Vec<usize> = (0..cells).filter(|&cell| indegree[cell] == 0).collect();
    let mut head = 0;
    while head < order.len() {
        let cell = order[head];
        head += 1;
        for i in 0..dependents[cell].len() {
            let dependent = dependents[cell][i];
            indegree[dependent] -= 1;
            if indegree[dependent] == 0 {
                order.push(dependent);
            }
        }
    }
    for &cell in &order {
        if let Some(formula) = &formulas[cell] {
            values[cell] = evaluate(formula, &values);
        }
    }
    let mut first_total = 0;
    for &value in &values {
        first_total = (first_total + value) % MOD;
    }

    let mut dirty = vec![-1i64; cells];
    let mut recomputed = 0;
    let edits = n / 4;
    for edit in 0..edits {
        let mut target = (rng.next() % cells as i64) as usize;
        while formulas[target].is_some() {
            target = (target + 1) % cells;
        }
        values[target] = rng.next() % 1000;
        let mut stack = vec![target];
        dirty[target] = edit;
        while let Some(cell) = stack.pop() {
            for &dependent in &dependents[cell] {
                if dirty[dependent] != edit {
                    dirty[dependent] = edit;
                    stack.push(dependent);
                }
            }
        }
        for &cell in &order {
            if dirty[cell] == edit {
                if let Some(formula) = &formulas[cell] {
                    values[cell] = evaluate(formula, &values);
                    recomputed += 1;
                }
            }
        }
    }
    let (mut total, mut checksum) = (0, 0i64);
    for &value in &values {
        total = (total + value) % MOD;
        checksum = (checksum * 31 + value) % 1000000007;
    }
    println!(
        "spreadsheet cells={cells} formulas={formula_count} formula_bytes={formula_bytes} ordered={} \
         first_total={first_total} edits={edits} recomputed={recomputed} total={total} checksum={checksum}",
        order.len()
    );
}
