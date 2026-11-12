//! Streamed HTTP: a client that reads long replies as they arrive.
//!
//! A server thread speaks HTTP/1.1 by hand over loopback TCP and keeps its
//! connection open between requests. The client, also written by hand on
//! the standard library, reads N log lines from a chunked body, N/4
//! server-sent events one at a time, then sends N/100 small requests, all
//! over one kept connection. See http_stream.au.
use std::fmt::Write as _;
use std::io::{BufRead, BufReader, BufWriter, Read, Write};
use std::net::{TcpListener, TcpStream};

struct Rng {
    state: i64,
}

impl Rng {
    fn next(&mut self) -> i64 {
        self.state = self.state * 48271 % 2147483647;
        self.state
    }
}

const WORDS: [&str; 8] = [
    "connect", "read", "write", "close", "retry", "timeout", "accept", "flush",
];

fn send_chunk(out: &mut impl Write, piece: &str) {
    write!(out, "{:x}\r\n{piece}\r\n", piece.len()).unwrap();
    out.flush().unwrap();
}

fn send_lines(out: &mut impl Write, count: i64) {
    let mut rng = Rng { state: 41 };
    out.write_all(
        b"HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nTransfer-Encoding: chunked\r\n\r\n",
    )
    .unwrap();
    let mut piece = String::new();
    for i in 0..count {
        let word = WORDS[(rng.next() % 8) as usize];
        let worker = rng.next() % 64;
        let value = rng.next() % 100000;
        writeln!(piece, "{i} {word} worker-{worker} {value}").unwrap();
        if i % 64 == 63 {
            send_chunk(out, &piece);
            piece.clear();
        }
    }
    if !piece.is_empty() {
        send_chunk(out, &piece);
    }
    out.write_all(b"0\r\n\r\n").unwrap();
    out.flush().unwrap();
}

fn send_events(out: &mut impl Write, count: i64) {
    let mut rng = Rng { state: 43 };
    out.write_all(
        b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nTransfer-Encoding: chunked\r\n\r\n",
    )
    .unwrap();
    for i in 0..count {
        let token = rng.next() % 5000;
        send_chunk(
            out,
            &format!("event: delta\nid: {i}\ndata: {{\"index\":{i},\"text\":\"tok-{token}\"}}\n\n"),
        );
    }
    send_chunk(out, "event: done\ndata: [DONE]\n\n");
    out.write_all(b"0\r\n\r\n").unwrap();
    out.flush().unwrap();
}

fn send_text(out: &mut impl Write, body: &str, close: bool) {
    let mut head = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nContent-Length: {}\r\n",
        body.len()
    );
    if close {
        head.push_str("Connection: close\r\n");
    }
    write!(out, "{head}\r\n{body}").unwrap();
    out.flush().unwrap();
}

fn serve(listener: TcpListener) {
    let mut accepted = 0;
    loop {
        let (stream, _) = listener.accept().unwrap();
        stream.set_nodelay(true).unwrap();
        accepted += 1;
        let mut reader = BufReader::new(stream.try_clone().unwrap());
        let mut out = BufWriter::new(stream);
        let mut line = String::new();
        loop {
            line.clear();
            if reader.read_line(&mut line).unwrap() == 0 {
                break;
            }
            let target = line.split(' ').nth(1).unwrap().to_string();
            loop {
                line.clear();
                if reader.read_line(&mut line).unwrap() == 0 || line == "\r\n" || line == "\n" {
                    break;
                }
            }
            if target == "/quit" {
                send_text(&mut out, &accepted.to_string(), true);
                return;
            }
            if let Some(count) = target.strip_prefix("/lines/") {
                send_lines(&mut out, count.parse().unwrap());
            } else if let Some(count) = target.strip_prefix("/events/") {
                send_events(&mut out, count.parse().unwrap());
            } else {
                send_text(&mut out, &format!("item {target}"), false);
            }
        }
    }
}

/// How the rest of a response body is framed.
enum Framing {
    Length(usize),
    /// Bytes left in the current chunk; `None` before a chunk-size line.
    Chunked(Option<usize>),
    Done,
}

/// One kept connection and the body of the response being read from it.
struct Client {
    reader: BufReader<TcpStream>,
    writer: TcpStream,
    framing: Framing,
}

impl Client {
    fn connect(address: &str) -> Self {
        let stream = TcpStream::connect(address).unwrap();
        stream.set_nodelay(true).unwrap();
        Self {
            reader: BufReader::with_capacity(16 * 1024, stream.try_clone().unwrap()),
            writer: stream,
            framing: Framing::Done,
        }
    }

    /// Sends a GET request and reads the response head.
    fn get(&mut self, host: &str, path: &str) {
        let request = format!(
            "GET {path} HTTP/1.1\r\nHost: {host}\r\nAccept: */*\r\nConnection: keep-alive\r\n\r\n"
        );
        self.writer.write_all(request.as_bytes()).unwrap();
        let mut line = String::new();
        self.reader.read_line(&mut line).unwrap();
        assert!(line.starts_with("HTTP/1.1 200"));
        self.framing = Framing::Done;
        loop {
            line.clear();
            self.reader.read_line(&mut line).unwrap();
            let header = line.trim_end();
            if header.is_empty() {
                break;
            }
            let (name, value) = header.split_once(':').unwrap();
            if name.eq_ignore_ascii_case("content-length") {
                self.framing = Framing::Length(value.trim().parse().unwrap());
            } else if name.eq_ignore_ascii_case("transfer-encoding") {
                self.framing = Framing::Chunked(None);
            }
        }
    }

    /// The next decoded bytes of the body; empty at its end.
    fn fill(&mut self) -> &[u8] {
        loop {
            match self.framing {
                Framing::Done => return &[],
                Framing::Length(0) => self.framing = Framing::Done,
                Framing::Length(left) => {
                    let available = self.reader.fill_buf().unwrap().len().min(left);
                    return &self.reader.buffer()[..available];
                }
                Framing::Chunked(None) => {
                    let mut line = String::new();
                    self.reader.read_line(&mut line).unwrap();
                    let size = usize::from_str_radix(line.trim_end(), 16).unwrap();
                    if size == 0 {
                        self.reader.read_line(&mut line).unwrap();
                        self.framing = Framing::Done;
                    } else {
                        self.framing = Framing::Chunked(Some(size));
                    }
                }
                Framing::Chunked(Some(0)) => {
                    let mut end = [0u8; 2];
                    self.reader.read_exact(&mut end).unwrap();
                    self.framing = Framing::Chunked(None);
                }
                Framing::Chunked(Some(left)) => {
                    let available = self.reader.fill_buf().unwrap().len().min(left);
                    return &self.reader.buffer()[..available];
                }
            }
        }
    }

    fn consume(&mut self, count: usize) {
        self.reader.consume(count);
        match &mut self.framing {
            Framing::Length(left) | Framing::Chunked(Some(left)) => *left -= count,
            _ => {}
        }
    }

    /// Reads the next line of the body into `line`, without its line ending.
    fn read_line(&mut self, line: &mut Vec<u8>) -> bool {
        line.clear();
        loop {
            let available = self.fill();
            if available.is_empty() {
                return !line.is_empty();
            }
            match available.iter().position(|&byte| byte == b'\n') {
                Some(end) => {
                    line.extend_from_slice(&available[..end]);
                    self.consume(end + 1);
                    return true;
                }
                None => {
                    let count = available.len();
                    line.extend_from_slice(available);
                    self.consume(count);
                }
            }
        }
    }

    fn read_all(&mut self) -> Vec<u8> {
        let mut body = Vec::new();
        loop {
            let available = self.fill();
            if available.is_empty() {
                return body;
            }
            let count = available.len();
            body.extend_from_slice(available);
            self.consume(count);
        }
    }
}

fn main() {
    let n: i64 = std::env::args().nth(1).unwrap().parse().unwrap();
    let event_count = n / 4;
    let request_count = n / 100;
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let host = listener.local_addr().unwrap().to_string();
    let server = std::thread::spawn(move || serve(listener));
    let mut client = Client::connect(&host);
    let mut line = Vec::new();

    let (mut lines, mut value_sum, mut retries) = (0i64, 0i64, 0i64);
    client.get(&host, &format!("/lines/{n}"));
    while client.read_line(&mut line) {
        let text = std::str::from_utf8(&line).unwrap();
        let fields: Vec<&str> = text.split(' ').collect();
        lines += 1;
        value_sum += fields[3].parse::<i64>().unwrap();
        if fields[1] == "retry" {
            retries += 1;
        }
    }

    let (mut events, mut event_bytes) = (0i64, 0usize);
    let mut last_id = String::new();
    let mut done_id = String::new();
    let mut event = String::from("message");
    let mut data = String::new();
    let mut has_data = false;
    client.get(&host, &format!("/events/{event_count}"));
    while client.read_line(&mut line) {
        let text = std::str::from_utf8(&line).unwrap();
        let text = text.strip_suffix('\r').unwrap_or(text);
        if text.is_empty() {
            if has_data {
                if event == "done" {
                    done_id.clone_from(&last_id);
                } else {
                    events += 1;
                    event_bytes += data.len();
                }
            }
            event.clear();
            event.push_str("message");
            data.clear();
            has_data = false;
            continue;
        }
        if text.starts_with(':') {
            continue;
        }
        let (name, value) = text.split_once(':').unwrap_or((text, ""));
        let value = value.strip_prefix(' ').unwrap_or(value);
        match name {
            "event" => {
                event.clear();
                event.push_str(value);
            }
            "data" => {
                if has_data {
                    data.push('\n');
                }
                data.push_str(value);
                has_data = true;
            }
            "id" => {
                last_id.clear();
                last_id.push_str(value);
            }
            _ => {}
        }
    }

    let mut body_bytes = 0usize;
    for i in 0..request_count {
        client.get(&host, &format!("/item/{i}"));
        body_bytes += client.read_all().len();
    }

    client.get(&host, "/quit");
    let connections = String::from_utf8(client.read_all()).unwrap();
    server.join().unwrap();
    println!(
        "http_stream lines={lines} value_sum={value_sum} retries={retries} events={events} event_bytes={event_bytes} last_id={done_id} requests={request_count} body_bytes={body_bytes} connections={connections}"
    );
}
