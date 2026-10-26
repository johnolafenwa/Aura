//! HTTP JSON API over loopback: an item service and a client that drives it.
//!
//! The server answers POST /items/<id>, GET /items/<id>, and GET /stats with
//! JSON bodies. The client sends one request per connection, as the Aura HTTP
//! client does, and checks every reply. HTTP/1.1 framing is written by hand on
//! tokio streams, as in the retrying_worker reference.
use serde_json::{json, Value};
use std::collections::HashMap;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::{TcpListener, TcpStream};

struct Rng {
    state: i64,
}

impl Rng {
    fn next(&mut self) -> i64 {
        self.state = self.state * 48271 % 2147483647;
        self.state
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

fn route(items: &mut HashMap<i64, Value>, method: &str, path: &str, body: &str) -> (u16, String) {
    if let Some(id) = path.strip_prefix("/items/") {
        let id: i64 = id.parse().unwrap();
        if method == "POST" {
            let item: Value = serde_json::from_str(body).unwrap();
            let reply = json!({"id": item["id"], "ok": true}).to_string();
            items.insert(id, item);
            return (201, reply);
        }
        return match items.get(&id) {
            Some(item) => (200, item.to_string()),
            None => (404, json!({"error": "not found"}).to_string()),
        };
    }
    if method == "GET" && path == "/stats" {
        return (200, json!({"count": items.len()}).to_string());
    }
    (400, json!({"error": "bad request"}).to_string())
}

async fn serve(listener: TcpListener, requests: i64) -> usize {
    let mut items = HashMap::new();
    for _ in 0..requests {
        let (stream, _) = listener.accept().await.unwrap();
        let mut reader = BufReader::new(stream);
        let (start, body) = read_message(&mut reader).await;
        let mut fields = start.split(' ');
        let (method, path) = (fields.next().unwrap(), fields.next().unwrap());
        let (status, payload) = route(&mut items, method, path, &body);
        let response = format!(
            "HTTP/1.1 {status} OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{payload}",
            payload.len()
        );
        reader
            .get_mut()
            .write_all(response.as_bytes())
            .await
            .unwrap();
    }
    items.len()
}

async fn request(
    address: std::net::SocketAddr,
    method: &str,
    path: &str,
    body: &str,
) -> (i64, String) {
    let stream = TcpStream::connect(address).await.unwrap();
    let mut reader = BufReader::new(stream);
    let message = format!(
        "{method} {path} HTTP/1.1\r\nHost: {address}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    reader
        .get_mut()
        .write_all(message.as_bytes())
        .await
        .unwrap();
    let (start, reply) = read_message(&mut reader).await;
    (start.split(' ').nth(1).unwrap().parse().unwrap(), reply)
}

async fn run(n: i64) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(serve(listener, n));
    let mut rng = Rng { state: 11 };
    let (mut statuses, mut quantities, mut body_bytes) = (0i64, 0i64, 0usize);
    for i in 0..n {
        let r = rng.next();
        let (status, reply) = if i % 50 == 49 {
            let (status, reply) = request(address, "GET", "/stats", "").await;
            let value: Value = serde_json::from_str(&reply).unwrap();
            quantities += value["count"].as_i64().unwrap();
            (status, reply)
        } else if i % 3 != 2 {
            let item = json!({"id": i, "name": format!("item-{i}"), "qty": r % 100, "tags": ["new", format!("batch-{}", r % 7)]});
            let (status, reply) =
                request(address, "POST", &format!("/items/{i}"), &item.to_string()).await;
            let value: Value = serde_json::from_str(&reply).unwrap();
            assert_eq!(value["id"].as_i64(), Some(i));
            (status, reply)
        } else {
            let (status, reply) =
                request(address, "GET", &format!("/items/{}", r % (i + 1)), "").await;
            let value: Value = serde_json::from_str(&reply).unwrap();
            if status == 200 {
                quantities += value["qty"].as_i64().unwrap();
            }
            (status, reply)
        };
        statuses += status;
        body_bytes += reply.len();
    }
    let stored = server.await.unwrap();
    println!("http_api requests={n} stored={stored} statuses={statuses} quantities={quantities} body_bytes={body_bytes}");
}

fn main() {
    let n: i64 = std::env::args().nth(1).unwrap().parse().unwrap();
    tokio::runtime::Builder::new_multi_thread()
        .worker_threads(std::thread::available_parallelism().map_or(1, usize::from))
        .enable_all()
        .build()
        .unwrap()
        .block_on(run(n));
}
