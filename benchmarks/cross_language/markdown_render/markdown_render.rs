//! Documentation tools: render Markdown to HTML.
//!
//! Generates a Markdown document of N blocks (headings, paragraphs, bulleted and
//! numbered lists, fenced code, and quotes) whose text uses **bold**, *italic*,
//! `code`, and [links](url), then renders it to HTML in one pass over the lines.
//! Inline markup is scanned byte by byte, text is HTML-escaped, and the output is
//! built in a byte buffer. Reports the input and output sizes, the elements
//! written, and a checksum of the HTML.

const WORDS: [&str; 12] = [
    "aura", "fast", "safe", "compiler", "runtime", "task", "queue", "list", "native", "build",
    "docs", "test",
];
const NONE: u8 = 0;
const PARAGRAPH: u8 = 1;
const BULLETS: u8 = 2;
const NUMBERS: u8 = 3;

struct Rng {
    state: i64,
}

impl Rng {
    fn next(&mut self) -> i64 {
        self.state = self.state * 48271 % 2147483647;
        self.state
    }
}

fn inline_text(rng: &mut Rng, count: i64) -> String {
    let mut parts = Vec::new();
    for _ in 0..count {
        let r = rng.next();
        let word = WORDS[(r % 12) as usize];
        parts.push(match (r / 12) % 10 {
            0 => format!("**{word}**"),
            1 => format!("*{word}*"),
            2 => format!("`{word}<T>`"),
            3 => format!("[{word}](https://example.com/{word}?a=1&b=2)"),
            4 => format!("{word} & co"),
            _ => word.to_string(),
        });
    }
    parts.join(" ")
}

fn generate(rng: &mut Rng, blocks: i64) -> String {
    let mut lines = Vec::new();
    for _ in 0..blocks {
        let r = rng.next();
        match r % 6 {
            0 => {
                let level = (1 + (r / 6) % 3) as usize;
                lines.push(format!(
                    "{} {}",
                    "#".repeat(level),
                    inline_text(rng, 3 + (r / 18) % 4)
                ));
            }
            1 => {
                for _ in 0..1 + (r / 6) % 3 {
                    lines.push(inline_text(rng, 6 + (r / 18) % 6));
                }
            }
            2 => {
                for _ in 0..2 + (r / 6) % 4 {
                    lines.push(format!("- {}", inline_text(rng, 4)));
                }
            }
            3 => {
                for i in 0..2 + (r / 6) % 4 {
                    lines.push(format!("{}. {}", i + 1, inline_text(rng, 4)));
                }
            }
            4 => {
                lines.push("```".to_string());
                for i in 0..1 + (r / 6) % 4 {
                    lines.push(format!("if a < b && c > {i}: return \"x\""));
                }
                lines.push("```".to_string());
            }
            _ => lines.push(format!("> {}", inline_text(rng, 5))),
        }
        lines.push(String::new());
    }
    lines.join("\n")
}

fn escape_into(out: &mut Vec<u8>, data: &[u8], start: usize, end: usize) {
    for &c in &data[start..end] {
        match c {
            38 => out.extend_from_slice(b"&amp;"),
            60 => out.extend_from_slice(b"&lt;"),
            62 => out.extend_from_slice(b"&gt;"),
            34 => out.extend_from_slice(b"&quot;"),
            _ => out.push(c),
        }
    }
}

fn find(data: &[u8], start: usize, end: usize, c: u8) -> Option<usize> {
    (start..end).find(|&i| data[i] == c)
}

fn render_inline(out: &mut Vec<u8>, data: &[u8], start: usize, end: usize) {
    let mut i = start;
    while i < end {
        let c = data[i];
        if c == 42 && i + 1 < end && data[i + 1] == 42 {
            if let Some(close) = find(data, i + 2, end, 42) {
                if close + 1 < end && data[close + 1] == 42 {
                    out.extend_from_slice(b"<strong>");
                    escape_into(out, data, i + 2, close);
                    out.extend_from_slice(b"</strong>");
                    i = close + 2;
                    continue;
                }
            }
        } else if c == 42 {
            if let Some(close) = find(data, i + 1, end, 42) {
                out.extend_from_slice(b"<em>");
                escape_into(out, data, i + 1, close);
                out.extend_from_slice(b"</em>");
                i = close + 1;
                continue;
            }
        } else if c == 96 {
            if let Some(close) = find(data, i + 1, end, 96) {
                out.extend_from_slice(b"<code>");
                escape_into(out, data, i + 1, close);
                out.extend_from_slice(b"</code>");
                i = close + 1;
                continue;
            }
        } else if c == 91 {
            if let Some(label_end) = find(data, i + 1, end, 93) {
                if label_end + 1 < end && data[label_end + 1] == 40 {
                    if let Some(url_end) = find(data, label_end + 2, end, 41) {
                        out.extend_from_slice(b"<a href=\"");
                        escape_into(out, data, label_end + 2, url_end);
                        out.extend_from_slice(b"\">");
                        escape_into(out, data, i + 1, label_end);
                        out.extend_from_slice(b"</a>");
                        i = url_end + 1;
                        continue;
                    }
                }
            }
        }
        escape_into(out, data, i, i + 1);
        i += 1;
    }
}

fn close_block(out: &mut Vec<u8>, state: u8) -> u8 {
    match state {
        PARAGRAPH => out.extend_from_slice(b"</p>\n"),
        BULLETS => out.extend_from_slice(b"</ul>\n"),
        NUMBERS => out.extend_from_slice(b"</ol>\n"),
        _ => {}
    }
    NONE
}

fn render(text: &str) -> (Vec<u8>, [i64; 6]) {
    let mut out = Vec::new();
    let mut counts = [0i64; 6];
    let mut state = NONE;
    let mut in_code = false;
    for line in text.split('\n') {
        let data = line.as_bytes();
        let n = data.len();
        if n >= 3 && data[0] == 96 && data[1] == 96 && data[2] == 96 {
            if in_code {
                out.extend_from_slice(b"</code></pre>\n");
                in_code = false;
            } else {
                state = close_block(&mut out, state);
                out.extend_from_slice(b"<pre><code>");
                in_code = true;
                counts[4] += 1;
            }
            continue;
        }
        if in_code {
            escape_into(&mut out, data, 0, n);
            out.push(10);
            continue;
        }
        if n == 0 {
            state = close_block(&mut out, state);
            continue;
        }
        if data[0] == 35 {
            state = close_block(&mut out, state);
            let mut level = 0;
            while level < n && data[level] == 35 {
                level += 1;
            }
            out.extend_from_slice(b"<h");
            out.push(48 + level as u8);
            out.push(62);
            render_inline(&mut out, data, level + 1, n);
            out.extend_from_slice(b"</h");
            out.push(48 + level as u8);
            out.extend_from_slice(b">\n");
            counts[0] += 1;
        } else if n >= 2 && data[0] == 45 && data[1] == 32 {
            if state != BULLETS {
                close_block(&mut out, state);
                out.extend_from_slice(b"<ul>\n");
                state = BULLETS;
            }
            out.extend_from_slice(b"<li>");
            render_inline(&mut out, data, 2, n);
            out.extend_from_slice(b"</li>\n");
            counts[2] += 1;
        } else if data[0].is_ascii_digit() {
            let dot = find(data, 0, n, 46).unwrap();
            if state != NUMBERS {
                close_block(&mut out, state);
                out.extend_from_slice(b"<ol>\n");
                state = NUMBERS;
            }
            out.extend_from_slice(b"<li>");
            render_inline(&mut out, data, dot + 2, n);
            out.extend_from_slice(b"</li>\n");
            counts[3] += 1;
        } else if data[0] == 62 {
            state = close_block(&mut out, state);
            out.extend_from_slice(b"<blockquote>");
            render_inline(&mut out, data, 2, n);
            out.extend_from_slice(b"</blockquote>\n");
            counts[5] += 1;
        } else {
            if state != PARAGRAPH {
                close_block(&mut out, state);
                out.extend_from_slice(b"<p>");
                state = PARAGRAPH;
                counts[1] += 1;
            } else {
                out.push(10);
            }
            render_inline(&mut out, data, 0, n);
        }
    }
    close_block(&mut out, state);
    (out, counts)
}

fn main() {
    let n: i64 = std::env::args().nth(1).unwrap().parse().unwrap();
    let mut rng = Rng { state: 83 };
    let text = generate(&mut rng, n);
    let (html, counts) = render(&text);
    let mut checksum = 0i64;
    for &c in &html {
        checksum = (checksum * 31 + c as i64) % 1000000007;
    }
    println!(
        "markdown_render blocks={n} markdown_bytes={} html_bytes={} headings={} paragraphs={} items={} code={} \
         quotes={} checksum={checksum}",
        text.len(),
        html.len(),
        counts[0],
        counts[1],
        counts[2] + counts[3],
        counts[4],
        counts[5]
    );
}
