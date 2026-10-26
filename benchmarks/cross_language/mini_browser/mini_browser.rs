//! Web browser: fetch pages over HTTP, parse HTML and CSS, lay out text, follow links.
//!
//! A server task generates a site of N HTML pages, each with a style sheet,
//! headings, paragraphs, lists, inline links, and hidden ad blocks. The browser
//! crawls the site breadth first from /page/0, one request per connection. For
//! each page it tokenizes the HTML into a DOM tree, parses the <style> sheet,
//! computes each element's style with tag and class selectors and specificity,
//! and lays the visible text out into lines with word wrapping, indents, margins,
//! and numbered link markers, like a text-mode browser. Links to missing pages get
//! 404 replies. Reports crawl counts and a checksum over every rendered line.
use std::collections::{HashSet, VecDeque};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::{TcpListener, TcpStream};

const WORDS: [&str; 32] = [
    "alpha", "bravo", "cache", "delta", "engine", "fabric", "gamma", "harbor", "index", "jolly",
    "kernel", "lambda", "matrix", "nectar", "orbit", "packet", "quartz", "router", "socket",
    "tensor", "update", "vector", "widget", "xenon", "yield", "zephyr", "browser", "layout",
    "render", "stream", "token", "cursor",
];
const STYLE: &str = "body { width: 72 }\n\
h1 { transform: upper; margin: 1 }\n\
h2 { margin: 1 }\n\
p { indent: 2 }\n\
ul { margin: 1 }\n\
li { indent: 4 }\n\
.note { indent: 6; width: 56 }\n\
.wide { width: 96 }\n\
.hidden, .ad { display: none }\n\
a.ext { transform: upper }\n";
const NOT_FOUND: &str = "<html><body><h1>Not found</h1></body></html>";
const HIDDEN_TAGS: [&str; 4] = ["head", "title", "style", "script"];
const BLOCK_TAGS: [&str; 8] = ["html", "body", "h1", "h2", "p", "div", "ul", "li"];
const VOID_TAGS: [&str; 4] = ["br", "img", "meta", "hr"];
const PROPERTIES: [&str; 5] = ["width", "indent", "margin", "transform", "display"];

struct Rng {
    state: i64,
}

impl Rng {
    fn next(&mut self) -> i64 {
        self.state = self.state * 48271 % 2147483647;
        self.state
    }
}

fn words(rng: &mut Rng, count: i64) -> String {
    let out: Vec<&str> = (0..count)
        .map(|_| WORDS[(rng.next() % 32) as usize])
        .collect();
    out.join(" ")
}

fn inline_text(rng: &mut Rng, total: i64, count: i64) -> String {
    let mut parts = Vec::new();
    for _ in 0..count {
        let r = rng.next();
        let word = WORDS[((r / 20) % 32) as usize];
        parts.push(match r % 20 {
            0 => {
                let target = (r / 640) % (total + total / 10 + 1);
                format!(
                    "<a href=\"/page/{target}\">{word} {}</a>",
                    WORDS[((r / 7) % 32) as usize]
                )
            }
            1 => format!("<a class=\"ext\" href=\"http://example.com/{word}\">{word}</a>"),
            2 => format!("<b>{word}</b>"),
            3 => format!("<em>{word}</em>"),
            4 => format!("{word} &amp; co"),
            5 => format!("&lt;{word}&gt;"),
            _ => word.to_string(),
        });
    }
    parts.join(" ")
}

fn page_html(page: i64, total: i64) -> String {
    let mut rng = Rng {
        state: (page + 1) * 7919,
    };
    let mut out = format!("<!doctype html>\n<html><head><title>Page {page}</title>\n<style>\n{STYLE}</style></head>\n<body>\n");
    out += &format!("<h1>Page {page}: {}</h1>\n", words(&mut rng, 3));
    let sections = 3 + rng.next() % 4;
    for _ in 0..sections {
        let count = 2 + rng.next() % 3;
        out += &format!("<h2>{}</h2>\n", words(&mut rng, count));
        let paragraphs = 1 + rng.next() % 3;
        for _ in 0..paragraphs {
            let r = rng.next();
            let classes = [
                "",
                " class=\"note\"",
                " class=\"wide\"",
                " class=\"note wide\"",
            ][(r % 4) as usize];
            let count = 20 + (r / 4) % 40;
            out += &format!("<p{classes}>{}</p>\n", inline_text(&mut rng, total, count));
        }
        let r = rng.next();
        if r % 3 == 0 {
            let items = 2 + (r / 3) % 4;
            out += "<ul>\n";
            for _ in 0..items {
                let count = 3 + rng.next() % 6;
                out += &format!("<li>{}</li>\n", inline_text(&mut rng, total, count));
            }
            out += "</ul>\n";
        }
        if r % 5 == 1 {
            out += "<div class=\"ad\">Buy <b>now</b> &amp; save</div>\n";
        }
        if r % 7 == 2 {
            out += "<!-- generated section -->\n<p>see<br>also</p>\n";
        }
    }
    out += "</body></html>\n";
    out
}

struct Node {
    tag: String,
    classes: Vec<String>,
    href: String,
    text: String,
    children: Vec<usize>,
}

impl Node {
    fn element(tag: String, classes: Vec<String>, href: String) -> Self {
        Node {
            tag,
            classes,
            href,
            text: String::new(),
            children: Vec::new(),
        }
    }

    fn text(text: String) -> Self {
        Node {
            tag: String::new(),
            classes: Vec::new(),
            href: String::new(),
            text,
            children: Vec::new(),
        }
    }
}

fn add_child(nodes: &mut Vec<Node>, parent: usize, node: Node) -> usize {
    nodes.push(node);
    let index = nodes.len() - 1;
    nodes[parent].children.push(index);
    index
}

fn decode_entities(text: &str) -> String {
    if !text.contains('&') {
        return text.to_string();
    }
    text.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&amp;", "&")
}

fn parse_tag(inner: &str) -> (String, Vec<String>, String) {
    let (name, rest) = inner.split_once(' ').unwrap_or((inner, ""));
    let mut classes = Vec::new();
    let mut href = String::new();
    let parts: Vec<&str> = rest.split('"').collect();
    let mut k = 0;
    while k + 1 < parts.len() {
        let attribute = parts[k].trim();
        let attribute = attribute
            .strip_suffix('=')
            .unwrap_or(attribute)
            .trim()
            .to_lowercase();
        if attribute == "class" {
            classes = parts[k + 1]
                .split_whitespace()
                .map(str::to_string)
                .collect();
        } else if attribute == "href" {
            href = parts[k + 1].to_string();
        }
        k += 2;
    }
    (name.to_lowercase(), classes, href)
}

fn parse_html(html: &str) -> Vec<Node> {
    let mut nodes = vec![Node::element(
        "#document".to_string(),
        Vec::new(),
        String::new(),
    )];
    let mut stack = vec![0usize];
    let n = html.len();
    let mut i = 0;
    while i < n {
        if html.as_bytes()[i] != b'<' {
            let end = html[i..].find('<').map_or(n, |offset| i + offset);
            let text = &html[i..end];
            i = end;
            if !text.trim().is_empty() {
                add_child(
                    &mut nodes,
                    *stack.last().unwrap(),
                    Node::text(decode_entities(text)),
                );
            }
            continue;
        }
        if html[i..].starts_with("<!--") {
            i = html[i + 4..]
                .find("-->")
                .map_or(n, |offset| i + 4 + offset + 3);
            continue;
        }
        let Some(offset) = html[i..].find('>') else {
            break;
        };
        let end = i + offset;
        let inner = &html[i + 1..end];
        i = end + 1;
        if inner.starts_with('!') {
            continue;
        }
        if let Some(name) = inner.strip_prefix('/') {
            let name = name.trim().to_lowercase();
            if let Some(depth) = (1..stack.len())
                .rev()
                .find(|&depth| nodes[stack[depth]].tag == name)
            {
                stack.truncate(depth);
            }
            continue;
        }
        let (tag, classes, href) = parse_tag(inner);
        let parent = *stack.last().unwrap();
        let raw = tag == "style" || tag == "script";
        let void = VOID_TAGS.contains(&tag.as_str());
        let closing = format!("</{tag}");
        let index = add_child(&mut nodes, parent, Node::element(tag, classes, href));
        if raw {
            let end = html[i..].find(&closing).map_or(n, |offset| i + offset);
            add_child(&mut nodes, index, Node::text(html[i..end].to_string()));
            i = end;
            stack.push(index);
        } else if !void {
            stack.push(index);
        }
    }
    nodes
}

struct Rule {
    tag: String,
    class_name: String,
    specificity: i64,
    prop: usize,
    value: i64,
}

fn property_value(prop: usize, text: &str) -> i64 {
    match prop {
        3 => (text == "upper") as i64,
        4 => match text {
            "none" => 0,
            "block" => 1,
            _ => 2,
        },
        _ => text.parse().unwrap(),
    }
}

fn parse_css(text: &str) -> Vec<Rule> {
    let mut rules = Vec::new();
    for block in text.split('}') {
        let Some((selectors, body)) = block.split_once('{') else {
            continue;
        };
        let mut declarations = Vec::new();
        for declaration in body.split(';') {
            let Some((name, value)) = declaration.split_once(':') else {
                continue;
            };
            if let Some(prop) = PROPERTIES.iter().position(|&p| p == name.trim()) {
                declarations.push((prop, property_value(prop, value.trim())));
            }
        }
        for selector in selectors.split(',') {
            let (tag, class_name) = selector
                .trim()
                .split_once('.')
                .unwrap_or((selector.trim(), ""));
            let specificity =
                if tag.is_empty() { 0 } else { 1 } + if class_name.is_empty() { 0 } else { 10 };
            for &(prop, value) in &declarations {
                rules.push(Rule {
                    tag: tag.to_string(),
                    class_name: class_name.to_string(),
                    specificity,
                    prop,
                    value,
                });
            }
        }
    }
    rules
}

#[derive(Clone, Copy)]
struct Style {
    width: i64,
    indent: i64,
    margin: i64,
    upper: i64,
    display: i64,
}

fn compute_style(node: &Node, parent: &Style, rules: &[Rule]) -> Style {
    let mut values = [parent.width, parent.indent, 0, parent.upper, 2];
    if HIDDEN_TAGS.contains(&node.tag.as_str()) {
        values[4] = 0;
    } else if BLOCK_TAGS.contains(&node.tag.as_str()) {
        values[4] = 1;
    }
    let mut best = [-1i64; 5];
    for rule in rules {
        if !rule.tag.is_empty() && rule.tag != node.tag {
            continue;
        }
        if !rule.class_name.is_empty() && !node.classes.contains(&rule.class_name) {
            continue;
        }
        if rule.specificity >= best[rule.prop] {
            best[rule.prop] = rule.specificity;
            values[rule.prop] = rule.value;
        }
    }
    Style {
        width: values[0],
        indent: values[1],
        margin: values[2],
        upper: values[3],
        display: values[4],
    }
}

struct Layout {
    lines: Vec<String>,
    words: Vec<String>,
    line_len: i64,
    width: i64,
    indent: i64,
    links: Vec<String>,
    word_count: i64,
}

impl Layout {
    fn flush(&mut self) {
        if !self.words.is_empty() {
            let mut line = " ".repeat(self.indent as usize);
            line += &self.words.join(" ");
            self.lines.push(line);
            self.words.clear();
            self.line_len = 0;
        }
    }

    fn add_word(&mut self, word: String) {
        let available = (self.width - self.indent).max(1);
        if !self.words.is_empty() && self.line_len + 1 + word.len() as i64 > available {
            self.flush();
        }
        if !self.words.is_empty() {
            self.line_len += 1;
        }
        self.line_len += word.len() as i64;
        self.words.push(word);
        self.word_count += 1;
    }
}

fn render(nodes: &[Node], index: usize, parent: &Style, rules: &[Rule], layout: &mut Layout) {
    let node = &nodes[index];
    if node.tag.is_empty() {
        for word in node.text.split_whitespace() {
            layout.add_word(if parent.upper == 1 {
                word.to_uppercase()
            } else {
                word.to_string()
            });
        }
        return;
    }
    let style = compute_style(node, parent, rules);
    if style.display == 0 {
        return;
    }
    if node.tag == "br" {
        layout.flush();
        return;
    }
    let block = style.display == 1;
    let (saved_width, saved_indent) = (layout.width, layout.indent);
    if block {
        layout.flush();
        for _ in 0..style.margin {
            layout.lines.push(String::new());
        }
        layout.width = style.width;
        layout.indent = style.indent;
    }
    for &child in &node.children {
        render(nodes, child, &style, rules, layout);
    }
    if node.tag == "a" && !node.href.is_empty() {
        layout.links.push(node.href.clone());
        let marker = format!("[{}]", layout.links.len());
        layout.add_word(marker);
    }
    if block {
        layout.flush();
        layout.width = saved_width;
        layout.indent = saved_indent;
    }
}

struct Page {
    nodes: usize,
    title_bytes: usize,
    layout: Layout,
}

fn browse(html: &str) -> Page {
    let nodes = parse_html(html);
    let mut sheet = String::new();
    let mut title_bytes = 0;
    for node in &nodes {
        if node.tag == "style" {
            for &child in &node.children {
                sheet += &nodes[child].text;
            }
        } else if node.tag == "title" {
            for &child in &node.children {
                title_bytes += nodes[child].text.len();
            }
        }
    }
    let rules = parse_css(&sheet);
    let mut layout = Layout {
        lines: Vec::new(),
        words: Vec::new(),
        line_len: 0,
        width: 80,
        indent: 0,
        links: Vec::new(),
        word_count: 0,
    };
    let root = Style {
        width: 80,
        indent: 0,
        margin: 0,
        upper: 0,
        display: 1,
    };
    render(&nodes, 0, &root, &rules, &mut layout);
    layout.flush();
    Page {
        nodes: nodes.len(),
        title_bytes,
        layout,
    }
}

/// Reads a start line, the headers, and a Content-Length body.
async fn read_message<R: AsyncBufReadExt + Unpin>(reader: &mut R) -> (String, String) {
    let mut start = String::new();
    reader.read_line(&mut start).await.unwrap();
    let mut length = 0;
    loop {
        let mut line = String::new();
        reader.read_line(&mut line).await.unwrap();
        let line = line.trim_end_matches(['\r', '\n']);
        if line.is_empty() {
            break;
        }
        let (name, value) = line.split_once(':').unwrap();
        if name.trim().eq_ignore_ascii_case("content-length") {
            length = value.trim().parse().unwrap();
        }
    }
    let mut body = vec![0; length];
    reader.read_exact(&mut body).await.unwrap();
    (
        start.trim_end().to_string(),
        String::from_utf8(body).unwrap(),
    )
}

async fn serve(listener: TcpListener, total: i64) -> i64 {
    let mut served = 0;
    loop {
        let (stream, _) = listener.accept().await.unwrap();
        let mut reader = BufReader::new(stream);
        let (start, _) = read_message(&mut reader).await;
        let path = start.split(' ').nth(1).unwrap().to_string();
        let (status, payload) = if path == "/quit" {
            (200, "bye".to_string())
        } else {
            served += 1;
            let page: i64 = path.strip_prefix("/page/").unwrap().parse().unwrap();
            if page < total {
                (200, page_html(page, total))
            } else {
                (404, NOT_FOUND.to_string())
            }
        };
        let response = format!(
            "HTTP/1.1 {status} OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{payload}",
            payload.len()
        );
        reader
            .get_mut()
            .write_all(response.as_bytes())
            .await
            .unwrap();
        if path == "/quit" {
            return served;
        }
    }
}

async fn fetch(address: std::net::SocketAddr, path: &str) -> (i64, String) {
    let stream = TcpStream::connect(address).await.unwrap();
    let mut reader = BufReader::new(stream);
    let message = format!(
        "GET {path} HTTP/1.1\r\nHost: {address}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
    );
    reader
        .get_mut()
        .write_all(message.as_bytes())
        .await
        .unwrap();
    let (start, body) = read_message(&mut reader).await;
    (start.split(' ').nth(1).unwrap().parse().unwrap(), body)
}

async fn run(total: i64) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(serve(listener, total));
    let mut queue = VecDeque::from(["/page/0".to_string()]);
    let mut seen: HashSet<String> = queue.iter().cloned().collect();
    let (mut fetched, mut not_found, mut html_bytes, mut nodes, mut links, mut external) =
        (0, 0, 0, 0, 0, 0);
    let (mut lines, mut words_seen, mut title_bytes, mut checksum) = (0, 0i64, 0, 0i64);
    while let Some(path) = queue.pop_front() {
        let (status, body) = fetch(address, &path).await;
        fetched += 1;
        if status != 200 {
            not_found += 1;
            continue;
        }
        html_bytes += body.len();
        let page = browse(&body);
        nodes += page.nodes;
        title_bytes += page.title_bytes;
        words_seen += page.layout.word_count;
        for line in &page.layout.lines {
            lines += 1;
            for &byte in line.as_bytes() {
                checksum = (checksum * 31 + byte as i64) % 1000000007;
            }
            checksum = (checksum * 31 + 10) % 1000000007;
        }
        for href in page.layout.links {
            links += 1;
            if href.starts_with('/') {
                if seen.insert(href.clone()) {
                    queue.push_back(href);
                }
            } else {
                external += 1;
            }
        }
    }
    fetch(address, "/quit").await;
    let served = server.await.unwrap();
    println!(
        "mini_browser site={total} fetched={fetched} served={served} not_found={not_found} html_bytes={html_bytes} \
         nodes={nodes} links={links} external={external} lines={lines} words={words_seen} \
         title_bytes={title_bytes} checksum={checksum}"
    );
}

fn main() {
    let total: i64 = std::env::args().nth(1).unwrap().parse().unwrap();
    tokio::runtime::Builder::new_multi_thread()
        .worker_threads(std::thread::available_parallelism().map_or(1, usize::from))
        .enable_all()
        .build()
        .unwrap()
        .block_on(run(total));
}
