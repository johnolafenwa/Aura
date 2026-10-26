//! Tree-walking interpreter: lex, parse, and run a small imperative language.
//!
//! The guest program sums Collatz sequence lengths for 1..N and counts primes
//! below N by trial division. Variables live in one string-keyed environment.
use std::collections::HashMap;

const SOURCE: &str = r#"
let total = 0;
let i = 1;
while (i <= LIMIT) {
    let x = i;
    let steps = 0;
    while (x != 1) {
        if (x % 2 == 0) { x = x / 2; } else { x = 3 * x + 1; }
        steps = steps + 1;
    }
    total = total + steps;
    i = i + 1;
}
print total;
let primes = 0;
let k = 2;
while (k < LIMIT) {
    let d = 2;
    let prime = 1;
    while (d * d <= k && prime) {
        if (k % d == 0) { prime = 0; }
        d = d + 1;
    }
    primes = primes + prime;
    k = k + 1;
}
print primes;
print -(total % 1000) + !primes;
"#;

#[derive(Clone, Copy, PartialEq)]
enum Kind {
    Number,
    Name,
    Symbol,
    End,
}

struct Token {
    kind: Kind,
    text: String,
}

fn tokenize(source: &str) -> Vec<Token> {
    let bytes = source.as_bytes();
    let mut tokens = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        let c = bytes[i];
        let start = i;
        let kind = if c == b' ' || c == b'\n' || c == b'\t' || c == b'\r' {
            i += 1;
            continue;
        } else if c.is_ascii_digit() {
            while i < bytes.len() && bytes[i].is_ascii_digit() {
                i += 1;
            }
            Kind::Number
        } else if c.is_ascii_alphabetic() || c == b'_' {
            while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') {
                i += 1;
            }
            Kind::Name
        } else if i + 1 < bytes.len()
            && ["==", "!=", "<=", ">=", "&&", "||"].contains(&&source[i..i + 2])
        {
            i += 2;
            Kind::Symbol
        } else {
            i += 1;
            Kind::Symbol
        };
        tokens.push(Token {
            kind,
            text: source[start..i].to_string(),
        });
    }
    tokens.push(Token {
        kind: Kind::End,
        text: String::new(),
    });
    tokens
}

#[derive(Clone, Copy)]
enum Op {
    Or,
    And,
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    Add,
    Sub,
    Mul,
    Div,
    Rem,
    Neg,
    Not,
}

enum Expr {
    Num(i64),
    Var(String),
    Unary(Op, Box<Expr>),
    Binary(Op, Box<Expr>, Box<Expr>),
}

enum Stmt {
    Assign(String, Expr),
    While(Expr, Vec<Stmt>),
    If(Expr, Vec<Stmt>, Vec<Stmt>),
    Print(Expr),
}

fn eval(expr: &Expr, env: &HashMap<String, i64>) -> i64 {
    match expr {
        Expr::Num(value) => *value,
        Expr::Var(name) => env[name],
        Expr::Unary(op, operand) => {
            let value = eval(operand, env);
            match op {
                Op::Neg => -value,
                _ => (value == 0) as i64,
            }
        }
        Expr::Binary(Op::And, left, right) => {
            (eval(left, env) != 0 && eval(right, env) != 0) as i64
        }
        Expr::Binary(Op::Or, left, right) => (eval(left, env) != 0 || eval(right, env) != 0) as i64,
        Expr::Binary(op, left, right) => {
            let a = eval(left, env);
            let b = eval(right, env);
            match op {
                Op::Add => a + b,
                Op::Sub => a - b,
                Op::Mul => a * b,
                Op::Div => a / b,
                Op::Rem => a % b,
                Op::Eq => (a == b) as i64,
                Op::Ne => (a != b) as i64,
                Op::Lt => (a < b) as i64,
                Op::Le => (a <= b) as i64,
                Op::Gt => (a > b) as i64,
                _ => (a >= b) as i64,
            }
        }
    }
}

fn run(body: &[Stmt], env: &mut HashMap<String, i64>, out: &mut Vec<i64>) {
    for statement in body {
        match statement {
            Stmt::Assign(name, value) => {
                let value = eval(value, env);
                match env.get_mut(name) {
                    Some(slot) => *slot = value,
                    None => {
                        env.insert(name.clone(), value);
                    }
                }
            }
            Stmt::While(condition, body) => {
                while eval(condition, env) != 0 {
                    run(body, env, out);
                }
            }
            Stmt::If(condition, then_body, else_body) => {
                if eval(condition, env) != 0 {
                    run(then_body, env, out);
                } else {
                    run(else_body, env, out);
                }
            }
            Stmt::Print(value) => out.push(eval(value, env)),
        }
    }
}

const LEVELS: [&[(&str, Op)]; 6] = [
    &[("||", Op::Or)],
    &[("&&", Op::And)],
    &[("==", Op::Eq), ("!=", Op::Ne)],
    &[("<", Op::Lt), ("<=", Op::Le), (">", Op::Gt), (">=", Op::Ge)],
    &[("+", Op::Add), ("-", Op::Sub)],
    &[("*", Op::Mul), ("/", Op::Div), ("%", Op::Rem)],
];

struct Parser {
    tokens: Vec<Token>,
    pos: usize,
    nodes: i64,
}

impl Parser {
    fn peek(&self) -> &str {
        &self.tokens[self.pos].text
    }

    fn take(&mut self) -> String {
        self.pos += 1;
        self.tokens[self.pos - 1].text.clone()
    }

    fn expect(&mut self, text: &str) {
        assert_eq!(self.take(), text, "expected {text}");
    }

    fn program(&mut self) -> Vec<Stmt> {
        let mut statements = Vec::new();
        while self.tokens[self.pos].kind != Kind::End {
            statements.push(self.statement());
        }
        statements
    }

    fn block(&mut self) -> Vec<Stmt> {
        self.expect("{");
        let mut statements = Vec::new();
        while self.peek() != "}" {
            statements.push(self.statement());
        }
        self.expect("}");
        statements
    }

    fn statement(&mut self) -> Stmt {
        self.nodes += 1;
        let word = self.take();
        match word.as_str() {
            "let" => {
                let name = self.take();
                self.expect("=");
                let value = self.expression(0);
                self.expect(";");
                Stmt::Assign(name, value)
            }
            "while" => {
                self.expect("(");
                let condition = self.expression(0);
                self.expect(")");
                Stmt::While(condition, self.block())
            }
            "if" => {
                self.expect("(");
                let condition = self.expression(0);
                self.expect(")");
                let then_body = self.block();
                let mut else_body = Vec::new();
                if self.peek() == "else" {
                    self.take();
                    else_body = self.block();
                }
                Stmt::If(condition, then_body, else_body)
            }
            "print" => {
                let value = self.expression(0);
                self.expect(";");
                Stmt::Print(value)
            }
            _ => {
                self.expect("=");
                let value = self.expression(0);
                self.expect(";");
                Stmt::Assign(word, value)
            }
        }
    }

    fn expression(&mut self, level: usize) -> Expr {
        if level == LEVELS.len() {
            return self.unary();
        }
        let mut left = self.expression(level + 1);
        while let Some(&(_, op)) = LEVELS[level].iter().find(|(text, _)| *text == self.peek()) {
            self.take();
            let right = self.expression(level + 1);
            self.nodes += 1;
            left = Expr::Binary(op, Box::new(left), Box::new(right));
        }
        left
    }

    fn unary(&mut self) -> Expr {
        self.nodes += 1;
        if self.peek() == "-" || self.peek() == "!" {
            let op = if self.take() == "-" { Op::Neg } else { Op::Not };
            return Expr::Unary(op, Box::new(self.unary()));
        }
        let kind = self.tokens[self.pos].kind;
        let text = self.take();
        match kind {
            Kind::Number => Expr::Num(text.parse().unwrap()),
            Kind::Name => Expr::Var(text),
            _ => {
                let value = self.expression(0);
                self.expect(")");
                value
            }
        }
    }
}

fn main() {
    let limit: i64 = std::env::args().nth(1).unwrap().parse().unwrap();
    let tokens = tokenize(&SOURCE.replace("LIMIT", &limit.to_string()));
    let token_count = tokens.len();
    let mut parser = Parser {
        tokens,
        pos: 0,
        nodes: 0,
    };
    let program = parser.program();
    let mut env = HashMap::new();
    let mut out = Vec::new();
    run(&program, &mut env, &mut out);
    let printed: Vec<String> = out.iter().map(i64::to_string).collect();
    println!(
        "interpreter tokens={token_count} nodes={} variables={} output={}",
        parser.nodes,
        env.len(),
        printed.join(",")
    );
}
