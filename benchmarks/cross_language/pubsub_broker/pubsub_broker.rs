//! Messaging: a publish/subscribe broker with mailboxes over loopback TCP.
//!
//! One server task owns the broker and serves client sessions one after another,
//! like the key-value server. Clients send SUB and UNSUB to manage topic
//! subscriptions, PUB to fan a message out to the mailbox of every subscriber, and
//! POLL to drain up to K waiting messages, which arrive as MSG lines followed by
//! END. Each session sends 250 commands from 40 clients over 16 topics and waits
//! for every reply. Reports the messages fanned out and received, and a checksum.
use std::collections::HashMap;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{TcpListener, TcpStream};

const COMMANDS_PER_SESSION: i64 = 250;
const CLIENTS: i64 = 40;
const TOPICS: i64 = 16;

struct Rng {
    state: i64,
}

impl Rng {
    fn next(&mut self) -> i64 {
        self.state = self.state * 48271 % 2147483647;
        self.state
    }
}

struct Broker {
    topics: HashMap<String, Vec<usize>>,
    mailboxes: Vec<Vec<String>>,
    heads: Vec<usize>,
    delivered: i64,
}

impl Broker {
    fn new() -> Self {
        Broker {
            topics: HashMap::new(),
            mailboxes: vec![Vec::new(); CLIENTS as usize],
            heads: vec![0; CLIENTS as usize],
            delivered: 0,
        }
    }

    fn execute(&mut self, line: &str) -> String {
        let parts: Vec<&str> = line.split(' ').collect();
        if parts[0] == "PUB" {
            let Some(subscribers) = self.topics.get(parts[1]) else {
                return "SENT 0".to_string();
            };
            let message = format!("MSG {} {}", parts[1], parts[2]);
            for &client in subscribers {
                self.mailboxes[client].push(message.clone());
            }
            self.delivered += subscribers.len() as i64;
            return format!("SENT {}", subscribers.len());
        }
        let client: usize = parts[1][1..].parse().unwrap();
        match parts[0] {
            "SUB" => {
                let subscribers = self.topics.entry(parts[2].to_string()).or_default();
                if !subscribers.contains(&client) {
                    subscribers.push(client);
                }
                "OK".to_string()
            }
            "UNSUB" => {
                if let Some(subscribers) = self.topics.get_mut(parts[2]) {
                    if let Some(index) = subscribers.iter().position(|&c| c == client) {
                        subscribers.remove(index);
                        return "OK".to_string();
                    }
                }
                "NONE".to_string()
            }
            _ => {
                let limit: usize = parts[2].parse().unwrap();
                let mailbox = &self.mailboxes[client];
                let head = self.heads[client];
                let end = mailbox.len().min(head + limit);
                let mut reply = String::new();
                for message in &mailbox[head..end] {
                    reply.push_str(message);
                    reply.push('\n');
                }
                reply.push_str("END");
                self.heads[client] = end;
                reply
            }
        }
    }
}

async fn serve(listener: TcpListener, sessions: i64) -> (i64, i64) {
    let mut broker = Broker::new();
    let mut commands = 0;
    for _ in 0..sessions {
        let (stream, _) = listener.accept().await.unwrap();
        let (read, mut write) = stream.into_split();
        let mut lines = BufReader::new(read).lines();
        while let Some(line) = lines.next_line().await.unwrap() {
            if line == "QUIT" {
                write.write_all(b"BYE\n").await.unwrap();
                break;
            }
            let mut reply = broker.execute(&line);
            reply.push('\n');
            write.write_all(reply.as_bytes()).await.unwrap();
            commands += 1;
        }
    }
    (commands, broker.delivered)
}

async fn run(n: i64) {
    let sessions = n / COMMANDS_PER_SESSION;
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(serve(listener, sessions));
    let mut rng = Rng { state: 97 };
    let (mut published, mut received, mut checksum, mut sequence) = (0i64, 0i64, 0i64, 0i64);
    for _ in 0..sessions {
        let stream = TcpStream::connect(address).await.unwrap();
        let (read, mut write) = stream.into_split();
        let mut lines = BufReader::new(read).lines();
        for _ in 0..COMMANDS_PER_SESSION {
            let r = rng.next();
            let op = r % 100;
            let client = (r / 100) % CLIENTS;
            let topic = (r / 10000) % TOPICS;
            let request = if op < 12 {
                format!("SUB c{client} t{topic}\n")
            } else if op < 15 {
                format!("UNSUB c{client} t{topic}\n")
            } else if op < 70 {
                sequence += 1;
                format!("PUB t{topic} m{sequence}-{}\n", r % 1000)
            } else {
                format!("POLL c{client} {}\n", 1 + r % 20)
            };
            write.write_all(request.as_bytes()).await.unwrap();
            if op >= 70 {
                loop {
                    let reply = lines.next_line().await.unwrap().unwrap();
                    if reply == "END" {
                        break;
                    }
                    received += 1;
                    checksum = (checksum * 31 + reply.len() as i64) % 1000000007;
                }
            } else {
                let reply = lines.next_line().await.unwrap().unwrap();
                if let Some(count) = reply.strip_prefix("SENT ") {
                    published += count.parse::<i64>().unwrap();
                }
            }
        }
        write.write_all(b"QUIT\n").await.unwrap();
        assert_eq!(lines.next_line().await.unwrap().as_deref(), Some("BYE"));
    }
    let (commands, delivered) = server.await.unwrap();
    println!(
        "pubsub_broker sessions={sessions} commands={commands} published={published} \
         delivered={delivered} received={received} checksum={checksum}"
    );
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
