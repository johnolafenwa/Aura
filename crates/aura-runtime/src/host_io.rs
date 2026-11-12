//! The host's raw socket handles and its one-handle readiness poll.
//!
//! The runtime waits on sockets by handle: a task registers the handle with
//! its reactor, and a thread outside a task polls it directly. Unix names
//! a socket by file descriptor and polls it with `poll`. Windows names it
//! by `SOCKET` and polls it with `WSAPoll`, which takes the same request
//! and result bits under other values. This module gives both hosts one
//! handle type, one trait to read it, and one poll call, so the socket code
//! above it is the same on each.

use std::io;

/// A socket handle: a file descriptor on Unix, a `SOCKET` on Windows.
#[cfg(unix)]
pub type RawIo = std::os::fd::RawFd;
/// A socket handle: a file descriptor on Unix, a `SOCKET` on Windows.
#[cfg(windows)]
pub type RawIo = std::os::windows::io::RawSocket;

#[cfg(unix)]
pub use std::os::fd::AsRawFd;

/// Reads a socket's handle. It carries the name of the Unix trait so the
/// socket code calls `as_raw_fd` on either host.
#[cfg(windows)]
pub trait AsRawFd {
    fn as_raw_fd(&self) -> RawIo;
}

#[cfg(windows)]
macro_rules! socket_handle {
    ($($socket:ty),*) => {$(
        impl AsRawFd for $socket {
            fn as_raw_fd(&self) -> RawIo {
                std::os::windows::io::AsRawSocket::as_raw_socket(self)
            }
        }
    )*};
}

#[cfg(windows)]
socket_handle!(
    std::net::TcpStream,
    std::net::TcpListener,
    std::net::UdpSocket,
    uds_windows::UnixStream,
    uds_windows::UnixListener
);

#[cfg(windows)]
impl<T: AsRawFd> AsRawFd for &T {
    fn as_raw_fd(&self) -> RawIo {
        (**self).as_raw_fd()
    }
}

#[cfg(windows)]
impl<T: AsRawFd> AsRawFd for &mut T {
    fn as_raw_fd(&self) -> RawIo {
        (**self).as_raw_fd()
    }
}

/// Asks whether the socket has bytes to read, a connection to accept, or a
/// closed peer.
#[cfg(unix)]
pub const POLLIN: i16 = libc::POLLIN;
/// Asks whether the socket has urgent bytes to read.
#[cfg(unix)]
pub const POLLPRI: i16 = libc::POLLPRI;
/// Asks whether the socket can take bytes, or has finished connecting.
#[cfg(unix)]
pub const POLLOUT: i16 = libc::POLLOUT;

/// Asks whether the socket has bytes to read, a connection to accept, or a
/// closed peer.
#[cfg(windows)]
pub const POLLIN: i16 = windows_sys::Win32::Networking::WinSock::POLLRDNORM;
/// `WSAPoll` rejects a request for urgent bytes, so the bit is empty here.
#[cfg(windows)]
pub const POLLPRI: i16 = 0;
/// Asks whether the socket can take bytes, or has finished connecting.
#[cfg(windows)]
pub const POLLOUT: i16 = windows_sys::Win32::Networking::WinSock::POLLWRNORM;

/// One entry of a readiness poll.
#[cfg(unix)]
pub type PollEntry = libc::pollfd;
/// One entry of a readiness poll.
#[cfg(windows)]
pub type PollEntry = windows_sys::Win32::Networking::WinSock::WSAPOLLFD;

/// An entry that asks for `events` on `handle`.
pub fn poll_entry(handle: RawIo, events: i16) -> PollEntry {
    PollEntry {
        fd: handle as _,
        events,
        revents: 0,
    }
}

/// Polls `entries` for up to `timeout_ms` milliseconds, or without limit
/// when it is negative. Returns how many entries have a result.
pub fn poll(entries: &mut [PollEntry], timeout_ms: i32) -> io::Result<usize> {
    #[cfg(unix)]
    // SAFETY: the pointer and length name the caller's live slice.
    let result = unsafe { libc::poll(entries.as_mut_ptr(), entries.len() as _, timeout_ms) };
    #[cfg(windows)]
    // SAFETY: the pointer and length name the caller's live slice.
    let result = unsafe {
        windows_sys::Win32::Networking::WinSock::WSAPoll(
            entries.as_mut_ptr(),
            entries.len() as u32,
            timeout_ms,
        )
    };
    if result < 0 {
        #[cfg(unix)]
        return Err(io::Error::last_os_error());
        #[cfg(windows)]
        // SAFETY: reads this thread's last socket error.
        return Err(io::Error::from_raw_os_error(unsafe {
            windows_sys::Win32::Networking::WinSock::WSAGetLastError()
        }));
    }
    Ok(result as usize)
}

/// Polls one handle. Returns whether it has a result: one of `events`, an
/// error, or a closed peer.
pub fn poll_one(handle: RawIo, events: i16, timeout_ms: i32) -> io::Result<bool> {
    let mut entry = [poll_entry(handle, events)];
    Ok(poll(&mut entry, timeout_ms)? > 0)
}

#[cfg(test)]
#[path = "host_io_tests.rs"]
mod tests;
