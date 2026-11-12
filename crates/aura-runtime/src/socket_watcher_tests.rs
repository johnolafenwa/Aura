use super::*;
use crate::runtime_reactor::RuntimeReactor;
use std::io::Write;
use std::net::{TcpListener, TcpStream};
use std::time::Instant;

fn pair() -> (TcpStream, TcpStream) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let client = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
    let (server, _) = listener.accept().unwrap();
    (client, server)
}

fn poll_until(reactor: &mut RuntimeReactor, key: WaitKey) -> bool {
    let deadline = Instant::now() + Duration::from_secs(10);
    while Instant::now() < deadline {
        let ready = reactor.poll(Some(Duration::from_millis(50))).unwrap();
        if ready.contains(&key) {
            return true;
        }
    }
    false
}

fn wait_for_no_watches(watcher: &SocketWatcher) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while watcher.watch_count() != 0 {
        assert!(Instant::now() < deadline, "watch was not dropped");
        thread::sleep(Duration::from_millis(1));
    }
}

#[test]
fn a_watch_wakes_its_wait_when_the_socket_becomes_readable() {
    let watcher = SocketWatcher::start().unwrap();
    let mut reactor = RuntimeReactor::new().unwrap();
    let (mut client, server) = pair();
    let key = WaitKey(1, 1);
    reactor.begin_wait(key).unwrap();
    watcher
        .watch(server.as_raw_fd(), host_io::POLLIN, reactor.handle(), key)
        .unwrap();
    assert!(reactor
        .poll(Some(Duration::from_millis(20)))
        .unwrap()
        .is_empty());
    client.write_all(b"x").unwrap();
    assert!(poll_until(&mut reactor, key));
    wait_for_no_watches(&watcher);
}

#[test]
fn a_watch_on_a_writable_socket_fires_at_once() {
    let watcher = SocketWatcher::start().unwrap();
    let mut reactor = RuntimeReactor::new().unwrap();
    let (client, _server) = pair();
    let key = WaitKey(2, 1);
    reactor.begin_wait(key).unwrap();
    watcher
        .watch(client.as_raw_fd(), host_io::POLLOUT, reactor.handle(), key)
        .unwrap();
    assert!(poll_until(&mut reactor, key));
}

#[test]
fn a_closed_peer_wakes_a_reader() {
    let watcher = SocketWatcher::start().unwrap();
    let mut reactor = RuntimeReactor::new().unwrap();
    let (client, server) = pair();
    let key = WaitKey(3, 1);
    reactor.begin_wait(key).unwrap();
    watcher
        .watch(server.as_raw_fd(), host_io::POLLIN, reactor.handle(), key)
        .unwrap();
    drop(client);
    assert!(poll_until(&mut reactor, key));
}

#[test]
fn an_unwatched_socket_wakes_no_one() {
    let watcher = SocketWatcher::start().unwrap();
    let mut reactor = RuntimeReactor::new().unwrap();
    let (mut client, server) = pair();
    let key = WaitKey(4, 1);
    reactor.begin_wait(key).unwrap();
    watcher
        .watch(server.as_raw_fd(), host_io::POLLIN, reactor.handle(), key)
        .unwrap();
    watcher.unwatch(&reactor.handle(), key);
    assert_eq!(watcher.watch_count(), 0);
    // Unwatching a key that has no watch changes nothing.
    watcher.unwatch(&reactor.handle(), key);
    client.write_all(b"x").unwrap();
    assert!(reactor
        .poll(Some(Duration::from_millis(100)))
        .unwrap()
        .is_empty());
}

#[test]
fn unwatch_leaves_the_watches_of_other_keys_and_reactors() {
    let watcher = SocketWatcher::start().unwrap();
    let mut first = RuntimeReactor::new().unwrap();
    let mut second = RuntimeReactor::new().unwrap();
    let (mut client, server) = pair();
    let key = WaitKey(5, 1);
    let other = WaitKey(6, 1);
    first.begin_wait(key).unwrap();
    first.begin_wait(other).unwrap();
    second.begin_wait(key).unwrap();
    for (reactor, key) in [(&first, key), (&first, other), (&second, key)] {
        watcher
            .watch(server.as_raw_fd(), host_io::POLLIN, reactor.handle(), key)
            .unwrap();
    }
    watcher.unwatch(&first.handle(), key);
    assert_eq!(watcher.watch_count(), 2);
    client.write_all(b"x").unwrap();
    assert!(poll_until(&mut first, other));
    assert!(poll_until(&mut second, key));
}

#[test]
fn many_sockets_each_wake_their_own_wait() {
    let watcher = SocketWatcher::start().unwrap();
    let mut reactor = RuntimeReactor::new().unwrap();
    let mut pairs: Vec<_> = (0..40).map(|_| pair()).collect();
    for (index, (_, server)) in pairs.iter().enumerate() {
        let key = WaitKey(100 + index as u64, 1);
        reactor.begin_wait(key).unwrap();
        watcher
            .watch(server.as_raw_fd(), host_io::POLLIN, reactor.handle(), key)
            .unwrap();
    }
    for index in [7usize, 23, 39] {
        pairs[index].0.write_all(b"x").unwrap();
        assert!(poll_until(&mut reactor, WaitKey(100 + index as u64, 1)));
    }
    let deadline = Instant::now() + Duration::from_secs(10);
    while watcher.watch_count() != 37 {
        assert!(Instant::now() < deadline);
        thread::sleep(Duration::from_millis(1));
    }
}

#[test]
fn a_handle_that_is_no_socket_wakes_its_wait() {
    let watcher = SocketWatcher::start().unwrap();
    let mut reactor = RuntimeReactor::new().unwrap();
    let key = WaitKey(7, 1);
    reactor.begin_wait(key).unwrap();
    // No open handle has this number, and no later one will take it while
    // the test runs.
    let stale = 0x3fff_fff0 as RawIo;
    watcher
        .watch(stale, host_io::POLLIN, reactor.handle(), key)
        .unwrap();
    assert!(poll_until(&mut reactor, key));
    wait_for_no_watches(&watcher);
}

#[test]
fn the_global_watcher_is_one_instance() {
    let first = SocketWatcher::global().unwrap() as *const SocketWatcher;
    let second = SocketWatcher::global().unwrap() as *const SocketWatcher;
    assert_eq!(first, second);
}

#[test]
fn dropping_a_watcher_stops_its_thread() {
    let watcher = SocketWatcher::start().unwrap();
    let shared = Arc::clone(&watcher.shared);
    drop(watcher);
    let deadline = Instant::now() + Duration::from_secs(10);
    // The thread holds the only other reference.
    while Arc::strong_count(&shared) != 1 {
        assert!(Instant::now() < deadline, "watcher thread did not stop");
        thread::sleep(Duration::from_millis(1));
    }
}

#[test]
fn watches_added_in_quick_succession_each_fire() {
    // Each watch interrupts the poll. An interrupt that arrives while the
    // thread handles the one before it must still be seen.
    let watcher = SocketWatcher::start().unwrap();
    let mut reactor = RuntimeReactor::new().unwrap();
    let (client, _server) = pair();
    for round in 0..2_000u64 {
        let key = WaitKey(1_000 + round, 1);
        reactor.begin_wait(key).unwrap();
        watcher
            .watch(client.as_raw_fd(), host_io::POLLOUT, reactor.handle(), key)
            .unwrap();
        assert!(poll_until(&mut reactor, key), "round {round} never fired");
    }
}
