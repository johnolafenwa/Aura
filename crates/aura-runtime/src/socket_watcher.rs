//! Socket readiness for reactors on Windows.
//!
//! A Unix reactor registers a waiting socket with its own `epoll` or
//! `kqueue` instance. Windows gives no such registration for a socket the
//! runtime owns outside the poller, so its reactors share one watcher
//! thread instead. A reactor hands the watcher a socket, the events to wait
//! for, and the wait's key. The thread polls every watched socket in one
//! `WSAPoll` call and, when a socket has a result, drops that watch and
//! wakes the wait through the reactor's inbox.
//!
//! A watch fires at most once. A wake for a wait that has already ended is
//! ignored by the reactor, so the watcher never needs to know whether the
//! wait is still current, and a socket closed while watched only wakes its
//! waiter early.
//!
//! One poll call scans every watched socket, so a wake costs time in
//! proportion to how many sockets are waiting.
//!
//! The module holds no Windows call of its own: it builds on
//! [`crate::host_io`], and its tests run on every host.

use std::io;
use std::net::UdpSocket;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread;
use std::time::{Duration, Instant};

use crate::host_io::{self, AsRawFd, RawIo};
use crate::runtime_reactor::{ReactorHandle, WaitKey};

/// How long a socket wait stays with its own reactor before it moves to
/// the watcher thread. A reply on a busy connection arrives well inside
/// this, and a wait answered here costs no thread wake-up at all.
pub const LOCAL_SOCKET_WAIT: Duration = Duration::from_micros(200);

struct LocalWait {
    key: WaitKey,
    socket: RawIo,
    events: i16,
    since: Instant,
}

/// One reactor's socket waits.
///
/// A new wait is first polled by the reactor's own thread. A wait that is
/// still quiet after [`LOCAL_SOCKET_WAIT`] moves to the shared watcher, so
/// the reactor's thread never scans a long list of idle sockets and is free
/// to sleep.
#[derive(Default)]
pub struct LocalSocketWaits {
    fresh: Vec<LocalWait>,
    /// The keys with at least one watch on the watcher thread.
    moved: std::collections::HashSet<WaitKey>,
    entries: Vec<host_io::PollEntry>,
}

impl LocalSocketWaits {
    /// Whether any wait is still polled by the reactor's own thread.
    pub fn is_empty(&self) -> bool {
        self.fresh.is_empty()
    }

    pub fn add(&mut self, key: WaitKey, socket: RawIo, events: i16, now: Instant) {
        self.fresh.push(LocalWait {
            key,
            socket,
            events,
            since: now,
        });
    }

    /// Returns the keys whose socket has a result now, and moves every
    /// other wait older than [`LOCAL_SOCKET_WAIT`] to `watcher`.
    pub fn poll(
        &mut self,
        now: Instant,
        watcher: &SocketWatcher,
        handle: &ReactorHandle,
    ) -> io::Result<Vec<WaitKey>> {
        if self.fresh.is_empty() {
            return Ok(Vec::new());
        }
        self.entries.clear();
        self.entries.extend(
            self.fresh
                .iter()
                .map(|wait| host_io::poll_entry(wait.socket, wait.events)),
        );
        let polled = loop {
            match host_io::poll(&mut self.entries, 0) {
                Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
                other => break other,
            }
        };
        let mut fired = Vec::new();
        let mut index = 0;
        let mut moved = Ok(());
        for position in 0..self.entries.len() {
            // A poll the host refused wakes every waiter, which retries
            // its own operation and reports that socket's error.
            let has_result = polled.is_err() || self.entries[position].revents != 0;
            if has_result {
                let wait = self.fresh.remove(index);
                if !fired.contains(&wait.key) {
                    fired.push(wait.key);
                }
            } else if now.saturating_duration_since(self.fresh[index].since) >= experiment_wait() {
                let wait = self.fresh.remove(index);
                self.moved.insert(wait.key);
                if let Err(error) =
                    watcher.watch(wait.socket, wait.events, handle.clone(), wait.key)
                {
                    moved = Err(error);
                }
            } else {
                index += 1;
            }
        }
        moved.map(|()| fired)
    }

    /// Drops every wait of `key`, here and on `watcher`.
    pub fn remove(&mut self, key: WaitKey, watcher: &SocketWatcher, handle: &ReactorHandle) {
        self.fresh.retain(|wait| wait.key != key);
        if self.moved.remove(&key) {
            watcher.unwatch(handle, key);
        }
    }
}

// EXPERIMENT ONLY, removed before merge: the local wait from the environment.
fn experiment_wait() -> Duration {
    static WAIT: std::sync::OnceLock<Duration> = std::sync::OnceLock::new();
    *WAIT.get_or_init(|| {
        std::env::var("AURA_EXPERIMENT_SOCKET_WAIT_US")
            .ok()
            .and_then(|text| text.parse().ok())
            .map_or(LOCAL_SOCKET_WAIT, Duration::from_micros)
    })
}

#[derive(Clone)]
struct Watch {
    socket: RawIo,
    events: i16,
    handle: ReactorHandle,
    key: WaitKey,
}

impl Watch {
    fn is(&self, other: &Watch) -> bool {
        self.socket == other.socket
            && self.events == other.events
            && self.key == other.key
            && self.handle.same_reactor(&other.handle)
    }
}

struct Shared {
    watches: Mutex<Vec<Watch>>,
    /// Interrupts the poll when the watch list changes.
    wake_sender: UdpSocket,
    /// Set while a wake datagram is on its way, so a burst of changes sends
    /// one.
    wake_pending: AtomicBool,
    stopping: AtomicBool,
}

impl Shared {
    fn watches(&self) -> MutexGuard<'_, Vec<Watch>> {
        self.watches
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    fn interrupt(&self) -> io::Result<()> {
        if !self.wake_pending.swap(true, Ordering::AcqRel) {
            if let Err(error) = self.wake_sender.send(&[1]) {
                self.wake_pending.store(false, Ordering::Release);
                return Err(error);
            }
        }
        Ok(())
    }
}

/// The watcher thread's handle. Dropping it stops the thread.
pub struct SocketWatcher {
    shared: Arc<Shared>,
}

impl SocketWatcher {
    /// Starts a watcher thread.
    pub fn start() -> io::Result<Self> {
        let wake_receiver = UdpSocket::bind("127.0.0.1:0")?;
        wake_receiver.set_nonblocking(true)?;
        let wake_sender = UdpSocket::bind("127.0.0.1:0")?;
        wake_sender.connect(wake_receiver.local_addr()?)?;
        let shared = Arc::new(Shared {
            watches: Mutex::new(Vec::new()),
            wake_sender,
            wake_pending: AtomicBool::new(false),
            stopping: AtomicBool::new(false),
        });
        let thread_shared = Arc::clone(&shared);
        thread::Builder::new()
            .name("aura-socket-watcher".to_string())
            .spawn(move || watch_sockets(&thread_shared, &wake_receiver))?;
        Ok(Self { shared })
    }

    /// The watcher every reactor of the process shares, started on first
    /// use.
    pub fn global() -> io::Result<&'static SocketWatcher> {
        static GLOBAL: std::sync::OnceLock<SocketWatcher> = std::sync::OnceLock::new();
        if let Some(watcher) = GLOBAL.get() {
            return Ok(watcher);
        }
        // Two threads may both start one; the loser's stops when dropped.
        let started = Self::start()?;
        Ok(GLOBAL.get_or_init(|| started))
    }

    /// Wakes `key` on `handle`'s reactor once `socket` has one of `events`,
    /// an error, or a closed peer.
    pub fn watch(
        &self,
        socket: RawIo,
        events: i16,
        handle: ReactorHandle,
        key: WaitKey,
    ) -> io::Result<()> {
        self.shared.watches().push(Watch {
            socket,
            events,
            handle,
            key,
        });
        self.shared.interrupt()
    }

    /// Drops every watch of `key` on `handle`'s reactor.
    pub fn unwatch(&self, handle: &ReactorHandle, key: WaitKey) {
        let mut watches = self.shared.watches();
        let before = watches.len();
        watches.retain(|watch| !(watch.key == key && watch.handle.same_reactor(handle)));
        let changed = watches.len() != before;
        drop(watches);
        if changed {
            // The thread may be polling the dropped socket. A lost
            // interrupt only delays its next look at the list.
            let _ = self.shared.interrupt();
        }
    }

    #[cfg(test)]
    pub(crate) fn watch_count(&self) -> usize {
        self.shared.watches().len()
    }
}

impl Drop for SocketWatcher {
    fn drop(&mut self) {
        self.shared.stopping.store(true, Ordering::Release);
        self.shared.wake_pending.store(false, Ordering::Release);
        let _ = self.shared.interrupt();
    }
}

fn watch_sockets(shared: &Shared, wake_receiver: &UdpSocket) {
    let mut entries = Vec::new();
    while !shared.stopping.load(Ordering::Acquire) {
        let snapshot = shared.watches().clone();
        entries.clear();
        entries.push(host_io::poll_entry(
            wake_receiver.as_raw_fd(),
            host_io::POLLIN,
        ));
        entries.extend(
            snapshot
                .iter()
                .map(|watch| host_io::poll_entry(watch.socket, watch.events)),
        );
        let fired = match host_io::poll(&mut entries, -1) {
            Ok(_) => {
                if entries[0].revents != 0 {
                    // Drain before clearing the flag. While the flag is
                    // set no one sends, so nothing arrives between the
                    // two steps, and a change made after the flag clears
                    // sends a fresh datagram. Cleared first, a datagram
                    // sent in between would be drained here and leave the
                    // flag set with nothing on its way: every later change
                    // would then go unseen.
                    let mut datagram = [0u8; 8];
                    while wake_receiver.recv(&mut datagram).is_ok() {}
                    shared.wake_pending.store(false, Ordering::Release);
                }
                let mut watches = shared.watches();
                let mut fired = Vec::new();
                for (entry, watch) in entries[1..].iter().zip(&snapshot) {
                    if entry.revents == 0 {
                        continue;
                    }
                    // A watch dropped since the snapshot wakes no one.
                    if let Some(index) = watches.iter().position(|current| current.is(watch)) {
                        fired.push(watches.swap_remove(index));
                    }
                }
                fired
            }
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(_) => {
                // The host refused the whole poll without naming a socket.
                // Wake every waiter, which retries its own operation and
                // reports that socket's error, and pause so a poll that
                // keeps failing cannot spin.
                let fired = std::mem::take(&mut *shared.watches());
                thread::sleep(Duration::from_millis(1));
                fired
            }
        };
        for watch in fired {
            // The reactor queues the wake before it signals, so a failed
            // signal loses nothing.
            let _ = watch.handle.wake(watch.key);
        }
    }
}

#[cfg(test)]
#[path = "socket_watcher_tests.rs"]
mod tests;
