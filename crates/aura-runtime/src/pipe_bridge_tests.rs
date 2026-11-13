use super::*;
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// A reader that yields its chunks one read at a time, then ends.
struct Chunks(Vec<Vec<u8>>);

impl Read for Chunks {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        if self.0.is_empty() {
            return Ok(0);
        }
        let chunk = self.0.remove(0);
        buffer[..chunk.len()].copy_from_slice(&chunk);
        Ok(chunk.len())
    }
}

/// A writer that records what it receives and reports when it is dropped.
struct Recorder {
    bytes: Arc<Mutex<Vec<u8>>>,
    dropped: mpsc::Sender<()>,
    fail_after: Option<usize>,
}

impl Write for Recorder {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        let mut recorded = self.bytes.lock().unwrap();
        if self.fail_after.is_some_and(|limit| recorded.len() >= limit) {
            return Err(io::Error::new(io::ErrorKind::BrokenPipe, "child closed"));
        }
        recorded.extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl Drop for Recorder {
    fn drop(&mut self) {
        let _ = self.dropped.send(());
    }
}

fn recorder(fail_after: Option<usize>) -> (Recorder, Arc<Mutex<Vec<u8>>>, mpsc::Receiver<()>) {
    let bytes = Arc::new(Mutex::new(Vec::new()));
    let (dropped, was_dropped) = mpsc::channel();
    (
        Recorder {
            bytes: Arc::clone(&bytes),
            dropped,
            fail_after,
        },
        bytes,
        was_dropped,
    )
}

#[test]
fn a_bridged_reader_yields_the_bytes_and_then_the_end() {
    let mut pipe = bridge_reader(Chunks(vec![b"hello ".to_vec(), b"world".to_vec()])).unwrap();
    let mut text = String::new();
    pipe.read_to_string(&mut text).unwrap();
    assert_eq!(text, "hello world");
}

#[test]
fn a_bridged_reader_carries_more_than_one_buffer() {
    let chunk: Vec<u8> = (0..60_000u32).map(|index| index as u8).collect();
    let mut pipe = bridge_reader(Chunks(vec![chunk.clone(); 8])).unwrap();
    let mut bytes = Vec::new();
    pipe.read_to_end(&mut bytes).unwrap();
    assert_eq!(bytes.len(), 480_000);
    assert_eq!(&bytes[420_000..], &chunk[..]);
}

#[test]
fn a_nonblocking_bridged_reader_reports_would_block_until_bytes_arrive() {
    let (sender, receiver) = mpsc::channel::<Vec<u8>>();
    struct Waiting(mpsc::Receiver<Vec<u8>>);
    impl Read for Waiting {
        fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
            match self.0.recv() {
                Ok(chunk) => {
                    buffer[..chunk.len()].copy_from_slice(&chunk);
                    Ok(chunk.len())
                }
                Err(_) => Ok(0),
            }
        }
    }
    let mut pipe = bridge_reader(Waiting(receiver)).unwrap();
    pipe.set_nonblocking(true).unwrap();
    let mut buffer = [0u8; 8];
    assert_eq!(
        pipe.read(&mut buffer).unwrap_err().kind(),
        io::ErrorKind::WouldBlock
    );
    sender.send(b"x".to_vec()).unwrap();
    assert!(crate::host_io::poll_one(pipe.as_raw_fd(), crate::host_io::POLLIN, 10_000).unwrap());
    assert_eq!(pipe.read(&mut buffer).unwrap(), 1);
    drop(sender);
    assert!(crate::host_io::poll_one(pipe.as_raw_fd(), crate::host_io::POLLIN, 10_000).unwrap());
    assert_eq!(pipe.read(&mut buffer).unwrap(), 0);
}

#[test]
fn dropping_a_bridged_reader_drops_the_source() {
    struct Endless(mpsc::Sender<()>);
    impl Read for Endless {
        fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
            buffer.fill(7);
            Ok(buffer.len())
        }
    }
    impl Drop for Endless {
        fn drop(&mut self) {
            let _ = self.0.send(());
        }
    }
    let (dropped, was_dropped) = mpsc::channel();
    let pipe = bridge_reader(Endless(dropped)).unwrap();
    drop(pipe);
    was_dropped.recv_timeout(Duration::from_secs(30)).unwrap();
}

#[test]
fn a_failing_source_ends_the_bridged_reader() {
    struct Failing(bool);
    impl Read for Failing {
        fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
            if self.0 {
                return Err(io::Error::other("pipe failed"));
            }
            self.0 = true;
            buffer[0] = b'a';
            Ok(1)
        }
    }
    let mut pipe = bridge_reader(Failing(false)).unwrap();
    let mut bytes = Vec::new();
    pipe.read_to_end(&mut bytes).unwrap();
    assert_eq!(bytes, b"a");
}

#[test]
fn an_interrupted_source_read_is_retried() {
    struct Interrupted(u8);
    impl Read for Interrupted {
        fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
            self.0 += 1;
            match self.0 {
                1 => Err(io::Error::from(io::ErrorKind::Interrupted)),
                2 => {
                    buffer[0] = b'b';
                    Ok(1)
                }
                _ => Ok(0),
            }
        }
    }
    let mut pipe = bridge_reader(Interrupted(0)).unwrap();
    let mut bytes = Vec::new();
    pipe.read_to_end(&mut bytes).unwrap();
    assert_eq!(bytes, b"b");
}

#[test]
fn a_bridged_writer_delivers_the_bytes_and_drops_the_sink_when_closed() {
    let (sink, bytes, was_dropped) = recorder(None);
    let mut pipe = bridge_writer(sink).unwrap();
    pipe.write_all(b"one two").unwrap();
    pipe.flush().unwrap();
    drop(pipe);
    was_dropped.recv_timeout(Duration::from_secs(30)).unwrap();
    assert_eq!(&*bytes.lock().unwrap(), b"one two");
}

#[test]
fn a_write_after_the_sink_fails_is_a_broken_pipe() {
    let (sink, _bytes, was_dropped) = recorder(Some(0));
    let mut pipe = bridge_writer(sink).unwrap();
    pipe.write_all(b"x").unwrap();
    was_dropped.recv_timeout(Duration::from_secs(30)).unwrap();
    let deadline = Instant::now() + Duration::from_secs(30);
    let error = loop {
        match pipe.write_all(b"more").and_then(|()| pipe.flush()) {
            Err(error) => break error,
            Ok(()) => {
                assert!(Instant::now() < deadline, "writes kept succeeding");
                thread::sleep(Duration::from_millis(5));
            }
        }
    };
    assert_eq!(error.kind(), io::ErrorKind::BrokenPipe);
}

#[test]
fn only_socket_peer_errors_become_broken_pipes() {
    for kind in [
        io::ErrorKind::ConnectionReset,
        io::ErrorKind::ConnectionAborted,
        io::ErrorKind::NotConnected,
    ] {
        assert_eq!(
            broken_pipe(io::Error::from(kind)).kind(),
            io::ErrorKind::BrokenPipe
        );
    }
    assert_eq!(
        broken_pipe(io::Error::from(io::ErrorKind::WouldBlock)).kind(),
        io::ErrorKind::WouldBlock
    );
}

#[test]
fn a_socket_pair_skips_a_connection_from_elsewhere() {
    // The pair's own ends talk to each other.
    let (mut near, mut far) = socket_pair().unwrap();
    near.write_all(b"n").unwrap();
    let mut byte = [0u8; 1];
    far.read_exact(&mut byte).unwrap();
    assert_eq!(&byte, b"n");
    assert_eq!(near.local_addr().unwrap(), far.peer_addr().unwrap());
}
