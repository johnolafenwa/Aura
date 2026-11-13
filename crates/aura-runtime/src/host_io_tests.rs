use super::*;
use std::io::Write;
use std::net::{TcpListener, TcpStream};

#[test]
fn a_socket_polls_readable_once_its_peer_writes() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let mut client = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
    let (server, _) = listener.accept().unwrap();
    assert!(!poll_one(server.as_raw_fd(), POLLIN, 0).unwrap());
    assert!(poll_one(client.as_raw_fd(), POLLOUT, 1_000).unwrap());
    client.write_all(b"x").unwrap();
    assert!(poll_one(server.as_raw_fd(), POLLIN, 5_000).unwrap());
}

#[test]
fn a_listener_polls_readable_once_a_client_connects() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    assert!(!poll_one(listener.as_raw_fd(), POLLIN, 0).unwrap());
    let _client = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
    assert!(poll_one(listener.as_raw_fd(), POLLIN, 5_000).unwrap());
}

#[test]
fn a_closed_peer_polls_readable() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let client = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
    let (server, _) = listener.accept().unwrap();
    drop(client);
    assert!(poll_one(server.as_raw_fd(), POLLIN, 5_000).unwrap());
}
