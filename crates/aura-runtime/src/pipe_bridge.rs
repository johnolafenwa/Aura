//! Child process pipes as sockets, for Windows.
//!
//! The runtime reads and writes a child's pipes without blocking: it waits
//! for the handle to be ready, then moves bytes. Unix can wait on a pipe.
//! Windows cannot: an anonymous pipe has no readiness to poll. So on
//! Windows each pipe is bridged to a loopback socket. A thread copies bytes
//! between the child's pipe and one end of a connected socket pair, and
//! the runtime keeps the other end, which waits, times out and cancels
//! like any socket.
//!
//! The bridge keeps the pipe's meaning. The end of the child's output is
//! the end of the socket's input. Dropping the runtime's end of a bridged
//! input closes the child's input, and a child that closed its input makes
//! later writes fail with a broken pipe.
//!
//! The module holds no Windows call: it copies between `Read` and `Write`
//! values, and its tests run on every host.

use std::io::{self, Read, Write};
use std::net::{Ipv4Addr, Shutdown, TcpListener, TcpStream};
use std::thread;

use crate::host_io::{AsRawFd, RawIo};

/// The runtime's end of a bridged pipe.
pub struct BridgedPipe {
    socket: TcpStream,
}

impl BridgedPipe {
    pub fn set_nonblocking(&self, enabled: bool) -> io::Result<()> {
        self.socket.set_nonblocking(enabled)
    }
}

impl Read for BridgedPipe {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        self.socket.read(buffer)
    }
}

impl Write for BridgedPipe {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.socket.write(bytes).map_err(broken_pipe)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.socket.flush().map_err(broken_pipe)
    }
}

impl AsRawFd for BridgedPipe {
    fn as_raw_fd(&self) -> RawIo {
        self.socket.as_raw_fd()
    }
}

/// A socket whose peer went away is a pipe whose reader went away.
fn broken_pipe(error: io::Error) -> io::Error {
    match error.kind() {
        io::ErrorKind::ConnectionReset
        | io::ErrorKind::ConnectionAborted
        | io::ErrorKind::NotConnected => io::Error::new(io::ErrorKind::BrokenPipe, error),
        _ => error,
    }
}

/// A connected loopback socket pair. Another local process may connect to
/// the listener first, so the pair is the accepted connection whose peer
/// is this function's own client.
fn socket_pair() -> io::Result<(TcpStream, TcpStream)> {
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))?;
    let near = TcpStream::connect(listener.local_addr()?)?;
    let near_address = near.local_addr()?;
    loop {
        let (far, peer) = listener.accept()?;
        if peer == near_address {
            near.set_nodelay(true)?;
            far.set_nodelay(true)?;
            return Ok((near, far));
        }
    }
}

/// Bridges a child's output: the returned end reads what `reader` yields,
/// and ends when it does.
pub fn bridge_reader<R>(mut reader: R) -> io::Result<BridgedPipe>
where
    R: Read + Send + 'static,
{
    let (near, mut far) = socket_pair()?;
    thread::Builder::new()
        .name("aura-pipe-reader".to_string())
        .spawn(move || {
            let mut buffer = vec![0u8; 64 * 1024];
            loop {
                match reader.read(&mut buffer) {
                    Ok(0) => break,
                    Ok(count) => {
                        // The runtime's end closed: stop reading, which
                        // closes the pipe when `reader` drops.
                        if far.write_all(&buffer[..count]).is_err() {
                            break;
                        }
                    }
                    Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
                    Err(_) => break,
                }
            }
            let _ = far.shutdown(Shutdown::Write);
        })?;
    Ok(BridgedPipe { socket: near })
}

/// Bridges a child's input: bytes written to the returned end reach
/// `writer`, and dropping that end drops `writer`.
pub fn bridge_writer<W>(mut writer: W) -> io::Result<BridgedPipe>
where
    W: Write + Send + 'static,
{
    let (near, mut far) = socket_pair()?;
    thread::Builder::new()
        .name("aura-pipe-writer".to_string())
        .spawn(move || {
            let mut buffer = vec![0u8; 64 * 1024];
            loop {
                match far.read(&mut buffer) {
                    Ok(0) => break,
                    Ok(count) => {
                        // The child closed its input: closing the socket
                        // makes the runtime's next write fail.
                        if writer
                            .write_all(&buffer[..count])
                            .and_then(|()| writer.flush())
                            .is_err()
                        {
                            break;
                        }
                    }
                    Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
                    Err(_) => break,
                }
            }
            drop(writer);
            let _ = far.shutdown(Shutdown::Both);
        })?;
    Ok(BridgedPipe { socket: near })
}

#[cfg(test)]
#[path = "pipe_bridge_tests.rs"]
mod tests;
