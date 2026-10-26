//! Key-value server: a line protocol over loopback TCP.
//!
//! One server task owns the store and serves client sessions one after another.
//! The client opens a session per 500 commands and sends SET, GET, DEL, and INCR
//! requests, waiting for each reply before sending the next request.
use std::collections::HashMap;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{TcpListener, TcpStream};

const COMMANDS_PER_SESSION: i64 = 500;

struct Rng {
    state: i64,
}

impl Rng {
    fn next(&mut self) -> i64 {
        self.state = self.state * 48271 % 2147483647;
        self.state
    }
}

fn execute(store: &mut HashMap<String, String>, line: &str) -> String {
    let parts: Vec<&str> = line.split(' ').collect();
    match (parts[0], parts.len()) {
        ("SET", 3) => {
            store.insert(parts[1].to_string(), parts[2].to_string());
            "OK".to_string()
        }
        ("GET", 2) => match store.get(parts[1]) {
            Some(value) => format!("VALUE {value}"),
            None => "NIL".to_string(),
        },
        ("DEL", 2) => match store.remove(parts[1]) {
            Some(_) => "DELETED 1".to_string(),
            None => "DELETED 0".to_string(),
        },
        ("INCR", 2) => {
            let current = store.get(parts[1]).map_or("0", String::as_str);
            match current.parse::<i64>() {
                Ok(number) => {
                    let next = number + 1;
                    store.insert(parts[1].to_string(), next.to_string());
                    format!("INT {next}")
                }
                Err(_) => "ERR not an integer".to_string(),
            }
        }
        _ => "ERR unknown command".to_string(),
    }
}

async fn serve(listener: TcpListener, sessions: i64) -> i64 {
    let mut store = HashMap::new();
    let mut served = 0;
    for _ in 0..sessions {
        let (stream, _) = listener.accept().await.unwrap();
        let (read, mut write) = stream.into_split();
        let mut lines = BufReader::new(read).lines();
        while let Some(line) = lines.next_line().await.unwrap() {
            if line == "QUIT" {
                write.write_all(b"BYE\n").await.unwrap();
                break;
            }
            let mut reply = execute(&mut store, &line);
            reply.push('\n');
            write.write_all(reply.as_bytes()).await.unwrap();
            served += 1;
        }
    }
    served
}

async fn run(n: i64) {
    let sessions = n / COMMANDS_PER_SESSION;
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(serve(listener, sessions));
    let mut rng = Rng { state: 7 };
    let (mut hits, mut nils, mut ints, mut checksum) = (0i64, 0i64, 0i64, 0i64);
    for _ in 0..sessions {
        let stream = TcpStream::connect(address).await.unwrap();
        let (read, mut write) = stream.into_split();
        let mut lines = BufReader::new(read).lines();
        for _ in 0..COMMANDS_PER_SESSION {
            let r = rng.next();
            let op = r % 10;
            let key = (r / 10) % 1000;
            let request = if op < 4 {
                format!("GET key-{key}\n")
            } else if op < 7 {
                format!("SET key-{key} v{}\n", r / 10000)
            } else if op == 7 {
                format!("DEL key-{key}\n")
            } else {
                format!("INCR counter-{}\n", key % 50)
            };
            write.write_all(request.as_bytes()).await.unwrap();
            let reply = lines.next_line().await.unwrap().unwrap();
            checksum += reply.len() as i64;
            if reply.starts_with("VALUE ") {
                hits += 1;
            } else if reply == "NIL" {
                nils += 1;
            } else if let Some(number) = reply.strip_prefix("INT ") {
                ints += number.parse::<i64>().unwrap();
            }
        }
        write.write_all(b"QUIT\n").await.unwrap();
        assert_eq!(lines.next_line().await.unwrap().as_deref(), Some("BYE"));
    }
    let served = server.await.unwrap();
    println!("kv_server sessions={sessions} commands={served} hits={hits} nils={nils} ints={ints} checksum={checksum}");
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
