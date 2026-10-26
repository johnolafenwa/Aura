//! Web rendering: a Mustache-style template engine producing HTML pages.
//!
//! Compiles one page template with escaped variables ({{name}}), raw variables
//! ({{&name}}), and a repeated section ({{#rows}}...{{/rows}}) into segments, then
//! renders N pages, each from its own context with 20 table rows. Text that goes
//! through {{name}} is HTML-escaped. Reports the pages, rows, bytes rendered, the
//! escapes written, and a checksum of every page's length.
use std::collections::HashMap;

const TEMPLATE: &str = concat!(
    "<!doctype html>\n<html><head><title>{{title}}</title></head>\n",
    "<body><h1>{{title}}</h1>\n<p class=\"greeting\">Welcome back, {{user}}!</p>\n",
    "<table>\n{{#rows}}<tr><td>{{name}}</td><td class=\"price\">${{price}}</td>",
    "<td>{{note}}</td><td>{{user}}</td></tr>\n{{/rows}}</table>\n",
    "<footer>{{&footer}}</footer>\n</body></html>\n"
);
const LITERAL: u8 = 0;
const ESCAPED: u8 = 1;
const RAW: u8 = 2;
const SECTION: u8 = 3;
const END: u8 = 4;
const SYLLABLES: [&str; 8] = ["ka", "lo", "mi", "ra", "to", "ne", "su", "vi"];
const NOTES: [&str; 6] = [
    "fresh & local",
    "<limited>",
    "\"best\" seller",
    "it's new",
    "plain",
    "a < b > c",
];
const MOD: i64 = 1000000007;

type Context = HashMap<String, String>;

struct Rng {
    state: i64,
}

impl Rng {
    fn next(&mut self) -> i64 {
        self.state = self.state * 48271 % 2147483647;
        self.state
    }
}

fn compile_template(source: &str) -> (Vec<u8>, Vec<String>) {
    let mut kinds = Vec::new();
    let mut texts = Vec::new();
    let mut parts = source.split("{{");
    kinds.push(LITERAL);
    texts.push(parts.next().unwrap().to_string());
    for part in parts {
        let (tag, literal) = part.split_once("}}").unwrap();
        if let Some(name) = tag.strip_prefix('#') {
            kinds.push(SECTION);
            texts.push(name.to_string());
        } else if let Some(name) = tag.strip_prefix('/') {
            kinds.push(END);
            texts.push(name.to_string());
        } else if let Some(name) = tag.strip_prefix('&') {
            kinds.push(RAW);
            texts.push(name.to_string());
        } else {
            kinds.push(ESCAPED);
            texts.push(tag.to_string());
        }
        kinds.push(LITERAL);
        texts.push(literal.to_string());
    }
    (kinds, texts)
}

fn escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

fn lookup<'a>(name: &str, row: &'a Context, context: &'a Context) -> &'a str {
    match row.get(name) {
        Some(value) => value,
        None => &context[name],
    }
}

#[allow(clippy::too_many_arguments)]
fn render_range(
    kinds: &[u8],
    texts: &[String],
    start: usize,
    end: usize,
    row: &Context,
    context: &Context,
    rows: &[Context],
    out: &mut Vec<String>,
) {
    let mut i = start;
    while i < end {
        let kind = kinds[i];
        if kind == LITERAL {
            out.push(texts[i].clone());
        } else if kind == ESCAPED {
            out.push(escape(lookup(&texts[i], row, context)));
        } else if kind == RAW {
            out.push(lookup(&texts[i], row, context).to_string());
        } else if kind == SECTION {
            let mut close = i + 1;
            while kinds[close] != END {
                close += 1;
            }
            for item in rows {
                render_range(kinds, texts, i + 1, close, item, context, rows, out);
            }
            i = close;
        }
        i += 1;
    }
}

fn make_page(rng: &mut Rng, page: i64) -> (Context, Vec<Context>) {
    let mut context = Context::new();
    context.insert(
        "title".to_string(),
        format!("Catalog page {page} <{}>", SYLLABLES[(page % 8) as usize]),
    );
    context.insert(
        "user".to_string(),
        format!("user<{}>&co", rng.next() % 1000),
    );
    context.insert(
        "footer".to_string(),
        format!("<small>Generated page {page}</small>"),
    );
    let mut rows = Vec::new();
    for _ in 0..20 {
        let r = rng.next();
        let cents = r % 100000;
        let mut row = Context::new();
        row.insert(
            "name".to_string(),
            format!(
                "{}{}{}",
                SYLLABLES[(r % 8) as usize],
                SYLLABLES[((r / 8) % 8) as usize],
                SYLLABLES[((r / 64) % 8) as usize]
            ),
        );
        row.insert(
            "price".to_string(),
            format!("{}.{:02}", cents / 100, cents % 100),
        );
        row.insert(
            "note".to_string(),
            NOTES[((r / 512) % 6) as usize].to_string(),
        );
        rows.push(row);
    }
    (context, rows)
}

fn main() {
    let n: i64 = std::env::args().nth(1).unwrap().parse().unwrap();
    let mut rng = Rng { state: 61 };
    let (kinds, texts) = compile_template(TEMPLATE);
    let empty = Context::new();
    let (mut total_bytes, mut escapes, mut checksum) = (0, 0, 0i64);
    for page in 0..n {
        let (context, rows) = make_page(&mut rng, page);
        let mut out = Vec::new();
        render_range(
            &kinds,
            &texts,
            0,
            kinds.len(),
            &empty,
            &context,
            &rows,
            &mut out,
        );
        let html = out.concat();
        total_bytes += html.len();
        escapes += html.matches('&').count();
        checksum = (checksum * 31 + html.len() as i64) % MOD;
    }
    println!(
        "template_render pages={n} segments={} rows={} bytes={total_bytes} escapes={escapes} checksum={checksum}",
        kinds.len(),
        n * 20
    );
}
