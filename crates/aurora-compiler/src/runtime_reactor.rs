//! Persistent polling primitive for the lightweight-task scheduler.
//!
//! The scheduler integration lands separately. Keep the primitive independent
//! of Aurora values so it can be exercised without constructing runtime state.
#![allow(dead_code)]

use mio::{Events, Poll, Token, Waker};
use std::cmp::Reverse;
use std::collections::{BinaryHeap, HashMap, HashSet, VecDeque};
use std::io;
use std::ops::BitOr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

#[cfg(unix)]
use mio::unix::SourceFd;
#[cfg(unix)]
use std::collections::{BTreeMap, BTreeSet};
#[cfg(unix)]
use std::os::fd::RawFd;

const WAKER_TOKEN: Token = Token(0);
const FIRST_FD_TOKEN: usize = 1;
static NEXT_REACTOR_ID: AtomicU64 = AtomicU64::new(1);
const MIN_TIMER_HEAP_COMPACTION_THRESHOLD: usize = 64;
const TIMER_HEAP_LIVE_MULTIPLIER: usize = 2;
const TIMER_HEAP_STALE_ALLOWANCE: usize = 16;

/// Identifies one suspension of a task. A later epoch for the same task makes
/// every source registered under an older key stale.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub(crate) struct WaitKey(pub(crate) u64, pub(crate) u64);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct IoInterest(u8);

impl IoInterest {
    pub(crate) const READABLE: Self = Self(0b01);
    pub(crate) const WRITABLE: Self = Self(0b10);

    fn contains(self, other: Self) -> bool {
        self.0 & other.0 != 0
    }

    #[cfg(unix)]
    fn as_mio(self) -> mio::Interest {
        match self.0 {
            0b01 => mio::Interest::READABLE,
            0b10 => mio::Interest::WRITABLE,
            0b11 => mio::Interest::READABLE | mio::Interest::WRITABLE,
            _ => unreachable!("an empty or unknown I/O interest must not be registered"),
        }
    }
}

impl BitOr for IoInterest {
    type Output = Self;

    fn bitor(self, rhs: Self) -> Self::Output {
        Self(self.0 | rhs.0)
    }
}

enum InboxCommand {
    Wake(WaitKey),
    Deadline(WaitKey, Instant),
    Cancel(WaitKey),
}

struct ReactorInbox {
    id: u64,
    state: Mutex<ReactorInboxState>,
    waker: Waker,
}

struct ReactorInboxState {
    commands: VecDeque<InboxCommand>,
    control_pending: bool,
    wake_armed: bool,
}

/// Cloneable, thread-safe completion path. Commands enter the durable queue
/// before the wake syscall, so a reported wake failure never discards them.
#[derive(Clone)]
pub(crate) struct ReactorHandle {
    inbox: Arc<ReactorInbox>,
}

impl ReactorHandle {
    pub(crate) fn wake(&self, key: WaitKey) -> io::Result<()> {
        self.submit(InboxCommand::Wake(key))
    }

    pub(crate) fn add_deadline(&self, key: WaitKey, deadline: Instant) -> io::Result<()> {
        self.submit(InboxCommand::Deadline(key, deadline))
    }

    pub(crate) fn cancel_wait(&self, key: WaitKey) -> io::Result<()> {
        self.submit(InboxCommand::Cancel(key))
    }

    /// Wakes the owning scheduler so it can inspect durable worker-control
    /// state such as new task admission or shutdown. Control notifications
    /// are coalesced independently of keyed wait commands.
    pub(crate) fn notify_control(&self) -> io::Result<()> {
        let mut state = lock_unpoisoned(&self.inbox.state);
        state.control_pending = true;
        self.wake_if_needed(&mut state)
    }

    pub(crate) fn same_reactor(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.inbox, &other.inbox)
    }

    fn submit(&self, command: InboxCommand) -> io::Result<()> {
        let mut state = lock_unpoisoned(&self.inbox.state);
        state.commands.push_back(command);
        self.wake_if_needed(&mut state)
    }

    fn wake_if_needed(&self, state: &mut ReactorInboxState) -> io::Result<()> {
        if state.wake_armed {
            return Ok(());
        }
        // Leave `wake_armed` false on failure. The command or control flag is
        // already durable under the mutex, so a later notification can retry.
        self.inbox.waker.wake()?;
        state.wake_armed = true;
        Ok(())
    }
}

/// A source-facing keyed notifier. It intentionally has no `Drop`
/// cancellation: several runtime sources can hold subscriptions for the same
/// select, and dropping any one of them must not retire the whole wait.
#[derive(Clone)]
pub(crate) struct ReactorSubscription {
    key: WaitKey,
    handle: ReactorHandle,
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub(crate) struct ReactorSubscriptionKey(u64, WaitKey);

impl ReactorSubscription {
    pub(crate) fn new(key: WaitKey, handle: ReactorHandle) -> Self {
        Self { key, handle }
    }

    pub(crate) fn key(&self) -> WaitKey {
        self.key
    }

    pub(crate) fn wake(&self) -> io::Result<()> {
        self.handle.wake(self.key)
    }

    pub(crate) fn cancel_wait(&self) -> io::Result<()> {
        self.handle.cancel_wait(self.key)
    }

    pub(crate) fn same_wait(&self, other: &Self) -> bool {
        self.key == other.key && self.handle.same_reactor(&other.handle)
    }

    pub(crate) fn identity(&self) -> ReactorSubscriptionKey {
        ReactorSubscriptionKey(self.handle.inbox.id, self.key)
    }
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct TimerEntry {
    deadline: Instant,
    sequence: u64,
    key: WaitKey,
}

#[cfg(unix)]
struct FdEntry {
    token: Token,
    waiters: BTreeMap<WaitKey, IoInterest>,
    interest: IoInterest,
}

/// Single-owner persistent poller. Registration methods are called by the
/// scheduler thread; other threads signal it through [`ReactorHandle`].
pub(crate) struct RuntimeReactor {
    poll: Poll,
    events: Events,
    inbox: Arc<ReactorInbox>,
    active_epochs: HashMap<u64, u64>,
    deadlines: BinaryHeap<Reverse<TimerEntry>>,
    timer_versions: HashMap<WaitKey, u64>,
    next_timer_sequence: u64,
    ready: VecDeque<WaitKey>,
    ready_set: HashSet<WaitKey>,
    #[cfg(unix)]
    fds: HashMap<RawFd, FdEntry>,
    #[cfg(unix)]
    token_fds: HashMap<Token, RawFd>,
    #[cfg(unix)]
    key_fds: HashMap<WaitKey, BTreeSet<RawFd>>,
    #[cfg(unix)]
    next_fd_token: usize,
}

impl RuntimeReactor {
    pub(crate) fn new() -> io::Result<Self> {
        let poll = Poll::new()?;
        let inbox = Arc::new(ReactorInbox {
            id: NEXT_REACTOR_ID.fetch_add(1, Ordering::Relaxed),
            state: Mutex::new(ReactorInboxState {
                commands: VecDeque::new(),
                control_pending: false,
                wake_armed: false,
            }),
            waker: Waker::new(poll.registry(), WAKER_TOKEN)?,
        });
        Ok(Self {
            poll,
            events: Events::with_capacity(128),
            inbox,
            active_epochs: HashMap::new(),
            deadlines: BinaryHeap::new(),
            timer_versions: HashMap::new(),
            next_timer_sequence: 0,
            ready: VecDeque::new(),
            ready_set: HashSet::new(),
            #[cfg(unix)]
            fds: HashMap::new(),
            #[cfg(unix)]
            token_fds: HashMap::new(),
            #[cfg(unix)]
            key_fds: HashMap::new(),
            #[cfg(unix)]
            next_fd_token: FIRST_FD_TOKEN,
        })
    }

    pub(crate) fn handle(&self) -> ReactorHandle {
        ReactorHandle {
            inbox: Arc::clone(&self.inbox),
        }
    }

    /// Starts a new suspension and atomically retires the previous epoch for
    /// this task, including all of that suspension's losing sources.
    pub(crate) fn begin_wait(&mut self, key: WaitKey) -> io::Result<()> {
        if let Some(old_epoch) = self.active_epochs.get(&key.0).copied() {
            let old_key = WaitKey(key.0, old_epoch);
            self.remove_from_ready(old_key);
            self.cleanup_sources(old_key)?;
        }
        self.active_epochs.insert(key.0, key.1);
        Ok(())
    }

    pub(crate) fn is_waiting(&self, key: WaitKey) -> bool {
        self.active_epochs.get(&key.0) == Some(&key.1)
    }

    /// Adds or replaces this suspension's timer. A heap entry is immutable;
    /// the version map makes the replaced entry cheap, safely stale garbage.
    pub(crate) fn add_deadline(&mut self, key: WaitKey, deadline: Instant) -> io::Result<()> {
        if !self.is_waiting(key) {
            return Ok(());
        }
        let sequence = self.next_timer_sequence;
        self.next_timer_sequence = self
            .next_timer_sequence
            .checked_add(1)
            .ok_or_else(|| io::Error::other("runtime reactor timer sequence exhausted"))?;
        self.timer_versions.insert(key, sequence);
        self.deadlines.push(Reverse(TimerEntry {
            deadline,
            sequence,
            key,
        }));
        self.compact_stale_timers_if_needed();
        Ok(())
    }

    /// Retires a suspension. This is also the select-loser cleanup operation:
    /// it removes the timer and every descriptor registration for this key.
    pub(crate) fn cancel_wait(&mut self, key: WaitKey) -> io::Result<()> {
        if self.is_waiting(key) {
            self.active_epochs.remove(&key.0);
        }
        self.remove_from_ready(key);
        self.cleanup_sources(key)
    }

    /// Blocks until one or more keys are ready, a control notification asks
    /// the scheduler to inspect its worker state, or `max_wait` elapses.
    /// Timer-update waker events continue polling rather than spuriously
    /// returning; a control notification intentionally returns an empty key
    /// list without disturbing any keyed waits.
    pub(crate) fn poll(&mut self, max_wait: Option<Duration>) -> io::Result<Vec<WaitKey>> {
        let caller_deadline = max_wait.and_then(|duration| Instant::now().checked_add(duration));
        loop {
            let control_notified = self.drain_inbox()?;
            self.mark_expired_timers(Instant::now());
            if !self.ready.is_empty() {
                return self.finish_ready();
            }
            if control_notified {
                return Ok(Vec::new());
            }
            if caller_deadline.is_some_and(|deadline| deadline <= Instant::now()) {
                return Ok(Vec::new());
            }

            let timeout = self.poll_timeout(caller_deadline);
            self.poll_mio_once(timeout)?;

            let control_notified = self.drain_inbox()?;
            self.mark_expired_timers(Instant::now());
            if !self.ready.is_empty() {
                return self.finish_ready();
            }
            if control_notified {
                return Ok(Vec::new());
            }
            if caller_deadline.is_some_and(|deadline| deadline <= Instant::now()) {
                return Ok(Vec::new());
            }
        }
    }

    /// Admits every source that is ready now without yielding the scheduler
    /// thread. The zero-time poll is deliberately performed even when the
    /// inbox or timer heap has already produced work, so a perpetually nonempty
    /// scheduler ready queue cannot starve kernel events.
    pub(crate) fn poll_nonblocking(&mut self) -> io::Result<Vec<WaitKey>> {
        self.drain_inbox()?;
        self.mark_expired_timers(Instant::now());
        self.poll_mio_once(Some(Duration::ZERO))?;
        self.drain_inbox()?;
        self.mark_expired_timers(Instant::now());
        if self.ready.is_empty() {
            Ok(Vec::new())
        } else {
            self.finish_ready()
        }
    }

    /// Admits thread-safe source notifications and expired deadlines without
    /// entering the platform poller. The scheduler uses this cheap path while
    /// runnable tasks remain, interleaving less-frequent zero-time descriptor
    /// polls for fd fairness.
    pub(crate) fn poll_local_nonblocking(&mut self) -> io::Result<Vec<WaitKey>> {
        self.drain_inbox()?;
        self.mark_expired_timers(Instant::now());
        if self.ready.is_empty() {
            Ok(Vec::new())
        } else {
            self.finish_ready()
        }
    }

    fn poll_mio_once(&mut self, timeout: Option<Duration>) -> io::Result<()> {
        self.events.clear();
        retry_on_interrupt(|| self.poll.poll(&mut self.events, timeout))?;

        #[cfg(unix)]
        let fd_events: Vec<_> = self
            .events
            .iter()
            .filter(|event| event.token() != WAKER_TOKEN)
            .map(|event| {
                let closed = event.is_read_closed() || event.is_write_closed();
                (
                    event.token(),
                    event.is_readable() || event.is_read_closed() || event.is_priority(),
                    event.is_writable() || event.is_write_closed(),
                    event.is_error() || closed,
                )
            })
            .collect();
        #[cfg(unix)]
        for (token, readable, writable, error) in fd_events {
            self.dispatch_fd_event(token, readable, writable, error);
        }
        Ok(())
    }

    fn poll_timeout(&mut self, caller_deadline: Option<Instant>) -> Option<Duration> {
        let deadline = match (self.next_deadline(), caller_deadline) {
            (Some(timer), Some(caller)) => Some(timer.min(caller)),
            (Some(timer), None) => Some(timer),
            (None, caller) => caller,
        };
        deadline.map(|deadline| deadline.saturating_duration_since(Instant::now()))
    }

    fn drain_inbox(&mut self) -> io::Result<bool> {
        let (commands, control_notified) = {
            let mut state = lock_unpoisoned(&self.inbox.state);
            state.wake_armed = false;
            let commands: Vec<_> = state.commands.drain(..).collect();
            let control_notified = std::mem::take(&mut state.control_pending);
            (commands, control_notified)
        };
        for command in commands {
            match command {
                InboxCommand::Wake(key) => self.mark_ready(key),
                InboxCommand::Deadline(key, deadline) => self.add_deadline(key, deadline)?,
                InboxCommand::Cancel(key) => self.cancel_wait(key)?,
            }
        }
        Ok(control_notified)
    }

    fn mark_ready(&mut self, key: WaitKey) {
        if self.is_waiting(key) && self.ready_set.insert(key) {
            self.ready.push_back(key);
        }
    }

    fn remove_from_ready(&mut self, key: WaitKey) {
        if self.ready_set.remove(&key) {
            self.ready.retain(|queued| *queued != key);
        }
    }

    fn finish_ready(&mut self) -> io::Result<Vec<WaitKey>> {
        let ready: Vec<_> = self.ready.drain(..).collect();
        self.ready_set.clear();
        let mut first_error = None;
        for key in &ready {
            if self.is_waiting(*key) {
                self.active_epochs.remove(&key.0);
            }
            if let Err(error) = self.cleanup_sources(*key) {
                first_error.get_or_insert(error);
            }
        }
        if let Some(error) = first_error {
            Err(error)
        } else {
            Ok(ready)
        }
    }

    fn mark_expired_timers(&mut self, now: Instant) {
        self.prune_stale_timers();
        while let Some(Reverse(entry)) = self.deadlines.peek().copied() {
            if entry.deadline > now {
                break;
            }
            self.deadlines.pop();
            if self.timer_entry_is_current(entry) {
                self.timer_versions.remove(&entry.key);
                self.mark_ready(entry.key);
            }
            self.prune_stale_timers();
        }
    }

    fn next_deadline(&mut self) -> Option<Instant> {
        self.prune_stale_timers();
        self.deadlines.peek().map(|entry| entry.0.deadline)
    }

    fn timer_entry_is_current(&self, entry: TimerEntry) -> bool {
        self.is_waiting(entry.key) && self.timer_versions.get(&entry.key) == Some(&entry.sequence)
    }

    fn prune_stale_timers(&mut self) {
        while self
            .deadlines
            .peek()
            .is_some_and(|entry| !self.timer_entry_is_current(entry.0))
        {
            self.deadlines.pop();
        }
    }

    fn compact_stale_timers_if_needed(&mut self) {
        let live_timer_count = self.timer_versions.len();
        let threshold = live_timer_count
            .saturating_mul(TIMER_HEAP_LIVE_MULTIPLIER)
            .saturating_add(TIMER_HEAP_STALE_ALLOWANCE)
            .max(MIN_TIMER_HEAP_COMPACTION_THRESHOLD);
        if self.deadlines.len() <= threshold {
            return;
        }

        let active_epochs = &self.active_epochs;
        let timer_versions = &self.timer_versions;
        self.deadlines.retain(|entry| {
            let entry = entry.0;
            active_epochs.get(&entry.key.0) == Some(&entry.key.1)
                && timer_versions.get(&entry.key) == Some(&entry.sequence)
        });
    }

    fn cleanup_sources(&mut self, key: WaitKey) -> io::Result<()> {
        self.timer_versions.remove(&key);
        self.compact_stale_timers_if_needed();
        #[cfg(unix)]
        {
            let descriptors = self.key_fds.get(&key).cloned().unwrap_or_default();
            let mut first_error = None;
            for fd in descriptors {
                if let Err(error) = self.remove_fd_waiter(key, fd) {
                    first_error.get_or_insert(error);
                }
            }
            if let Some(error) = first_error {
                return Err(error);
            }
        }
        Ok(())
    }

    #[cfg(unix)]
    pub(crate) fn add_fd(
        &mut self,
        key: WaitKey,
        fd: RawFd,
        interest: IoInterest,
    ) -> io::Result<()> {
        if !self.is_waiting(key) {
            return Ok(());
        }
        if interest.0 == 0 || interest.0 & !0b11 != 0 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "runtime reactor requires readable and/or writable interest",
            ));
        }

        if let Some(entry) = self.fds.get(&fd) {
            let mut waiters = entry.waiters.clone();
            waiters
                .entry(key)
                .and_modify(|current| *current = *current | interest)
                .or_insert(interest);
            let combined = combined_interest(&waiters);
            if combined != entry.interest {
                let mut source = SourceFd(&fd);
                retry_on_interrupt(|| {
                    self.poll
                        .registry()
                        .reregister(&mut source, entry.token, combined.as_mio())
                })?;
            }
            let entry = self.fds.get_mut(&fd).expect("checked above");
            entry.waiters = waiters;
            entry.interest = combined;
        } else {
            let token = self.allocate_fd_token()?;
            let mut source = SourceFd(&fd);
            retry_on_interrupt(|| {
                self.poll
                    .registry()
                    .register(&mut source, token, interest.as_mio())
            })?;
            let mut waiters = BTreeMap::new();
            waiters.insert(key, interest);
            self.fds.insert(
                fd,
                FdEntry {
                    token,
                    waiters,
                    interest,
                },
            );
            self.token_fds.insert(token, fd);
        }
        self.key_fds.entry(key).or_default().insert(fd);
        Ok(())
    }

    #[cfg(unix)]
    fn remove_fd_waiter(&mut self, key: WaitKey, fd: RawFd) -> io::Result<()> {
        let Some(entry) = self.fds.get(&fd) else {
            self.unlink_key_fd(key, fd);
            return Ok(());
        };
        if !entry.waiters.contains_key(&key) {
            self.unlink_key_fd(key, fd);
            return Ok(());
        }

        let token = entry.token;
        let old_interest = entry.interest;
        let mut waiters = entry.waiters.clone();
        waiters.remove(&key);
        let mut source = SourceFd(&fd);
        if waiters.is_empty() {
            let result = retry_on_interrupt(|| self.poll.registry().deregister(&mut source));
            self.retire_fd_registration(fd);
            return match result {
                Ok(()) => Ok(()),
                Err(error) if fd_registration_is_gone(&error) => Ok(()),
                Err(error) => Err(error),
            };
        } else {
            let interest = combined_interest(&waiters);
            if interest != old_interest {
                let result = retry_on_interrupt(|| {
                    self.poll
                        .registry()
                        .reregister(&mut source, token, interest.as_mio())
                });
                if let Err(error) = result {
                    // A failed narrowing leaves the kernel registration state
                    // unknowable. Retire the token locally so queued events
                    // cannot alias later fd reuse, and wake the surviving
                    // waiters so none can remain parked on an untracked fd.
                    let surviving_keys = self.retire_fd_registration(fd);
                    for surviving_key in surviving_keys {
                        if surviving_key != key {
                            self.mark_ready(surviving_key);
                        }
                    }
                    return if fd_registration_is_gone(&error) {
                        Ok(())
                    } else {
                        Err(error)
                    };
                }
            }
            let entry = self.fds.get_mut(&fd).expect("checked above");
            entry.waiters = waiters;
            entry.interest = interest;
        }
        self.unlink_key_fd(key, fd);
        Ok(())
    }

    #[cfg(unix)]
    fn retire_fd_registration(&mut self, fd: RawFd) -> Vec<WaitKey> {
        let Some(entry) = self.fds.remove(&fd) else {
            return Vec::new();
        };
        self.token_fds.remove(&entry.token);
        let keys: Vec<_> = entry.waiters.into_keys().collect();
        for key in &keys {
            self.unlink_key_fd(*key, fd);
        }
        keys
    }

    #[cfg(unix)]
    fn unlink_key_fd(&mut self, key: WaitKey, fd: RawFd) {
        if let Some(descriptors) = self.key_fds.get_mut(&key) {
            descriptors.remove(&fd);
            if descriptors.is_empty() {
                self.key_fds.remove(&key);
            }
        }
    }

    #[cfg(unix)]
    fn dispatch_fd_event(&mut self, token: Token, readable: bool, writable: bool, error: bool) {
        let Some(fd) = self.token_fds.get(&token).copied() else {
            return;
        };
        let Some(entry) = self.fds.get(&fd) else {
            return;
        };
        if entry.token != token {
            return;
        }
        let ready: Vec<_> = entry
            .waiters
            .iter()
            .filter_map(|(key, interest)| {
                (error
                    || (readable && interest.contains(IoInterest::READABLE))
                    || (writable && interest.contains(IoInterest::WRITABLE)))
                .then_some(*key)
            })
            .collect();
        for key in ready {
            self.mark_ready(key);
        }
    }

    #[cfg(unix)]
    fn allocate_fd_token(&mut self) -> io::Result<Token> {
        let token = Token(self.next_fd_token);
        self.next_fd_token = self
            .next_fd_token
            .checked_add(1)
            .ok_or_else(|| io::Error::other("runtime reactor descriptor token exhausted"))?;
        Ok(token)
    }

    #[cfg(test)]
    fn expire_timers(&mut self, now: Instant) -> Vec<WaitKey> {
        self.mark_expired_timers(now);
        self.finish_ready().unwrap()
    }

    #[cfg(test)]
    fn take_ready(&mut self) -> Vec<WaitKey> {
        self.finish_ready().unwrap()
    }

    #[cfg(all(test, unix))]
    fn fd_token(&self, fd: RawFd) -> Option<Token> {
        self.fds.get(&fd).map(|entry| entry.token)
    }

    #[cfg(all(test, unix))]
    fn fd_interest(&self, fd: RawFd) -> Option<IoInterest> {
        self.fds.get(&fd).map(|entry| entry.interest)
    }
}

#[cfg(unix)]
fn combined_interest(waiters: &BTreeMap<WaitKey, IoInterest>) -> IoInterest {
    waiters
        .values()
        .copied()
        .reduce(BitOr::bitor)
        .expect("a registered descriptor has at least one waiter")
}

#[cfg(unix)]
fn fd_registration_is_gone(error: &io::Error) -> bool {
    error.kind() == io::ErrorKind::NotFound
        || matches!(error.raw_os_error(), Some(libc::EBADF) | Some(libc::ENOENT))
}

fn retry_on_interrupt<T>(mut operation: impl FnMut() -> io::Result<T>) -> io::Result<T> {
    loop {
        match operation() {
            Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
            result => return result,
        }
    }
}

fn lock_unpoisoned<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io;
    use std::thread;
    use std::time::{Duration, Instant};

    #[test]
    fn timers_fire_at_the_earliest_deadline_and_preserve_equal_deadline_order() {
        let mut reactor = RuntimeReactor::new().unwrap();
        let now = Instant::now();
        let later = WaitKey(1, 1);
        let first_equal = WaitKey(2, 1);
        let second_equal = WaitKey(3, 1);

        for key in [later, first_equal, second_equal] {
            reactor.begin_wait(key).unwrap();
        }
        reactor
            .add_deadline(later, now + Duration::from_secs(2))
            .unwrap();
        reactor
            .add_deadline(first_equal, now + Duration::from_secs(1))
            .unwrap();
        reactor
            .add_deadline(second_equal, now + Duration::from_secs(1))
            .unwrap();

        assert_eq!(
            reactor.expire_timers(now + Duration::from_secs(1)),
            vec![first_equal, second_equal]
        );
        assert_eq!(reactor.next_deadline(), Some(now + Duration::from_secs(2)));
    }

    #[test]
    fn a_new_earlier_timer_wakes_poll_and_recomputes_its_timeout() {
        let mut reactor = RuntimeReactor::new().unwrap();
        let slow = WaitKey(1, 1);
        let fast = WaitKey(2, 1);
        reactor.begin_wait(slow).unwrap();
        reactor.begin_wait(fast).unwrap();
        reactor
            .add_deadline(slow, Instant::now() + Duration::from_secs(10))
            .unwrap();

        let handle = reactor.handle();
        let sender = thread::spawn(move || {
            thread::sleep(Duration::from_millis(20));
            handle
                .add_deadline(fast, Instant::now() + Duration::from_millis(10))
                .unwrap();
        });
        let ready = reactor.poll(Some(Duration::from_secs(2))).unwrap();
        sender.join().unwrap();

        assert_eq!(ready, vec![fast]);
        assert!(reactor.is_waiting(slow));
    }

    #[test]
    fn a_superseded_epoch_makes_its_old_timer_stale() {
        let mut reactor = RuntimeReactor::new().unwrap();
        let now = Instant::now();
        let stale = WaitKey(7, 1);
        let current = WaitKey(7, 2);
        reactor.begin_wait(stale).unwrap();
        reactor.add_deadline(stale, now).unwrap();
        reactor.begin_wait(current).unwrap();

        assert!(reactor.expire_timers(now).is_empty());
        assert!(!reactor.is_waiting(stale));
        assert!(reactor.is_waiting(current));
    }

    #[test]
    fn cancelled_long_deadlines_are_compacted_without_disturbing_live_order() {
        let mut reactor = RuntimeReactor::new().unwrap();
        let now = Instant::now();
        let live_first = WaitKey(10, 1);
        let live_second = WaitKey(11, 1);
        reactor.begin_wait(live_first).unwrap();
        reactor.begin_wait(live_second).unwrap();
        reactor
            .add_deadline(live_first, now + Duration::from_secs(1))
            .unwrap();
        reactor
            .add_deadline(live_second, now + Duration::from_secs(2))
            .unwrap();

        for epoch in 1..=1_000 {
            let cancelled = WaitKey(1, epoch);
            reactor.begin_wait(cancelled).unwrap();
            reactor
                .add_deadline(cancelled, now + Duration::from_secs(60))
                .unwrap();
            reactor.cancel_wait(cancelled).unwrap();
        }

        assert!(
            reactor.deadlines.len() <= 64,
            "stale timer heap grew to {} entries",
            reactor.deadlines.len()
        );
        assert_eq!(
            reactor.expire_timers(now + Duration::from_secs(1)),
            vec![live_first]
        );
        assert_eq!(reactor.next_deadline(), Some(now + Duration::from_secs(2)));
    }

    #[test]
    fn superseding_an_epoch_discards_already_queued_stale_readiness() {
        let mut reactor = RuntimeReactor::new().unwrap();
        let stale = WaitKey(7, 1);
        let current = WaitKey(7, 2);
        reactor.begin_wait(stale).unwrap();
        reactor.mark_ready(stale);
        reactor.begin_wait(current).unwrap();

        assert!(reactor.take_ready().is_empty());
        assert!(reactor.is_waiting(current));
    }

    #[test]
    fn subscriptions_identify_and_wake_only_their_own_reactor_wait() {
        let mut first_reactor = RuntimeReactor::new().unwrap();
        let second_reactor = RuntimeReactor::new().unwrap();
        let key = WaitKey(1, 1);
        let other_key = WaitKey(2, 1);
        first_reactor.begin_wait(key).unwrap();
        let first = ReactorSubscription::new(key, first_reactor.handle());
        let duplicate = first.clone();
        let different_wait = ReactorSubscription::new(other_key, first_reactor.handle());
        let other = ReactorSubscription::new(key, second_reactor.handle());

        assert!(first.same_wait(&duplicate));
        assert!(!first.same_wait(&different_wait));
        assert!(!first.same_wait(&other));
        assert_eq!(first.key(), key);
        first.wake().unwrap();
        assert_eq!(first_reactor.poll(Some(Duration::ZERO)).unwrap(), vec![key]);
    }

    #[test]
    fn subscription_cancellation_retires_queued_wake_and_deadline_sources() {
        let mut reactor = RuntimeReactor::new().unwrap();
        let key = WaitKey(1, 1);
        reactor.begin_wait(key).unwrap();
        let subscription = ReactorSubscription::new(key, reactor.handle());

        subscription.wake().unwrap();
        reactor.handle().add_deadline(key, Instant::now()).unwrap();
        subscription.cancel_wait().unwrap();

        assert!(reactor.poll_local_nonblocking().unwrap().is_empty());
        assert!(!reactor.is_waiting(key));
        assert_eq!(reactor.next_deadline(), None);
    }

    #[test]
    fn cancelling_a_stale_epoch_through_a_handle_preserves_the_current_wait() {
        let mut reactor = RuntimeReactor::new().unwrap();
        let stale = WaitKey(1, 1);
        let current = WaitKey(1, 2);
        reactor.begin_wait(stale).unwrap();
        reactor.begin_wait(current).unwrap();

        reactor.handle().cancel_wait(stale).unwrap();

        assert!(reactor.poll_local_nonblocking().unwrap().is_empty());
        assert!(reactor.is_waiting(current));
    }

    #[test]
    fn local_nonblocking_poll_admits_thread_wakes_and_expired_deadlines() {
        let mut reactor = RuntimeReactor::new().unwrap();
        let notified = WaitKey(1, 1);
        let expired = WaitKey(2, 1);
        reactor.begin_wait(notified).unwrap();
        reactor.begin_wait(expired).unwrap();
        reactor.handle().wake(notified).unwrap();
        reactor.add_deadline(expired, Instant::now()).unwrap();

        assert_eq!(
            reactor.poll_local_nonblocking().unwrap(),
            vec![notified, expired]
        );
        assert!(reactor.poll_local_nonblocking().unwrap().is_empty());
    }

    #[test]
    fn zero_duration_poll_returns_without_retiring_an_unready_wait() {
        let mut reactor = RuntimeReactor::new().unwrap();
        let key = WaitKey(1, 1);
        reactor.begin_wait(key).unwrap();

        assert!(reactor.poll(Some(Duration::ZERO)).unwrap().is_empty());
        assert!(reactor.is_waiting(key));
    }

    #[test]
    fn waker_coalescing_does_not_lose_inbox_entries_and_ready_is_deduplicated() {
        let mut reactor = RuntimeReactor::new().unwrap();
        let first = WaitKey(1, 1);
        let second = WaitKey(2, 1);
        reactor.begin_wait(first).unwrap();
        reactor.begin_wait(second).unwrap();
        let handle = reactor.handle();

        handle.wake(first).unwrap();
        handle.wake(first).unwrap();
        handle.wake(second).unwrap();

        assert_eq!(
            reactor.poll(Some(Duration::ZERO)).unwrap(),
            vec![first, second]
        );
    }

    #[test]
    fn control_notification_before_poll_is_durable_without_a_wait_key() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<ReactorHandle>();

        let mut reactor = RuntimeReactor::new().unwrap();
        reactor.handle().notify_control().unwrap();

        let started = Instant::now();
        assert!(reactor
            .poll(Some(Duration::from_secs(1)))
            .unwrap()
            .is_empty());
        assert!(
            started.elapsed() < Duration::from_millis(250),
            "a durable control notification should make the next poll return promptly"
        );
    }

    #[test]
    fn control_notification_during_poll_wakes_without_fabricating_readiness() {
        let mut reactor = RuntimeReactor::new().unwrap();
        let handle = reactor.handle();
        let sender = thread::spawn(move || {
            thread::sleep(Duration::from_millis(20));
            handle.notify_control().unwrap();
        });

        let started = Instant::now();
        assert!(reactor
            .poll(Some(Duration::from_secs(2)))
            .unwrap()
            .is_empty());
        sender.join().unwrap();
        assert!(
            started.elapsed() < Duration::from_millis(500),
            "control notification should wake a blocked poll"
        );
    }

    #[test]
    fn repeated_control_notifications_coalesce_into_one_scheduler_turn() {
        let mut reactor = RuntimeReactor::new().unwrap();
        let handle = reactor.handle();
        for _ in 0..32 {
            handle.notify_control().unwrap();
        }
        assert!(reactor
            .poll(Some(Duration::from_secs(1)))
            .unwrap()
            .is_empty());

        let sender = thread::spawn(move || {
            thread::sleep(Duration::from_millis(30));
            handle.notify_control().unwrap();
        });
        let started = Instant::now();
        assert!(reactor
            .poll(Some(Duration::from_secs(1)))
            .unwrap()
            .is_empty());
        sender.join().unwrap();
        assert!(
            started.elapsed() >= Duration::from_millis(10),
            "coalesced notifications must not leave extra control turns pending"
        );
        assert!(
            started.elapsed() < Duration::from_millis(500),
            "a fresh notification should still wake the next poll"
        );
    }

    #[test]
    fn shutdown_control_wake_preserves_existing_keyed_waits() {
        let mut reactor = RuntimeReactor::new().unwrap();
        let parked = WaitKey(1, 1);
        reactor.begin_wait(parked).unwrap();
        reactor
            .add_deadline(parked, Instant::now() + Duration::from_secs(60))
            .unwrap();

        reactor.handle().notify_control().unwrap();

        assert!(reactor
            .poll(Some(Duration::from_secs(1)))
            .unwrap()
            .is_empty());
        assert!(reactor.is_waiting(parked));
        assert!(reactor.next_deadline().is_some());
    }

    #[test]
    fn readiness_from_multiple_sources_is_reported_once_and_cleans_losers() {
        let mut reactor = RuntimeReactor::new().unwrap();
        let key = WaitKey(1, 1);
        let now = Instant::now();
        reactor.begin_wait(key).unwrap();
        reactor.add_deadline(key, now).unwrap();
        reactor.handle().wake(key).unwrap();

        assert_eq!(reactor.poll(Some(Duration::ZERO)).unwrap(), vec![key]);
        assert!(!reactor.is_waiting(key));
        assert_eq!(reactor.next_deadline(), None);
    }

    #[test]
    fn interrupted_operations_are_retried() {
        let mut calls = 0;
        let result = retry_on_interrupt(|| {
            calls += 1;
            if calls == 1 {
                Err(io::Error::from(io::ErrorKind::Interrupted))
            } else {
                Ok(42)
            }
        })
        .unwrap();

        assert_eq!(result, 42);
        assert_eq!(calls, 2);
    }

    #[cfg(unix)]
    mod unix {
        use super::*;
        use std::io::Write;
        use std::os::fd::{AsRawFd, RawFd};
        use std::os::unix::net::UnixStream;

        // These tests deliberately close a descriptor while mio still owns its
        // registration. A high duplicate prevents unrelated parallel tests
        // from immediately reusing the process-wide fd number.
        static CLOSED_FD_TEST_LOCK: Mutex<()> = Mutex::new(());

        fn duplicate_high_fd(fd: RawFd) -> RawFd {
            let duplicate = unsafe { libc::fcntl(fd, libc::F_DUPFD_CLOEXEC, 10_000) };
            assert!(
                duplicate >= 10_000,
                "failed to reserve a high test descriptor: {}",
                io::Error::last_os_error()
            );
            duplicate
        }

        #[test]
        fn one_persistent_fd_registration_aggregates_waiters_and_narrows_interest() {
            let mut reactor = RuntimeReactor::new().unwrap();
            let (stream, _peer) = UnixStream::pair().unwrap();
            let fd = stream.as_raw_fd();
            let reader = WaitKey(1, 1);
            let writer = WaitKey(2, 1);
            reactor.begin_wait(reader).unwrap();
            reactor.begin_wait(writer).unwrap();

            reactor.add_fd(reader, fd, IoInterest::READABLE).unwrap();
            let token = reactor.fd_token(fd).unwrap();
            reactor.add_fd(writer, fd, IoInterest::WRITABLE).unwrap();
            assert_eq!(
                reactor.fd_interest(fd),
                Some(IoInterest::READABLE | IoInterest::WRITABLE)
            );
            assert_eq!(reactor.fd_token(fd), Some(token));

            reactor.cancel_wait(writer).unwrap();
            assert_eq!(reactor.fd_interest(fd), Some(IoInterest::READABLE));
            assert_eq!(reactor.fd_token(fd), Some(token));
            reactor.cancel_wait(reader).unwrap();
            assert_eq!(reactor.fd_token(fd), None);
        }

        #[test]
        fn inactive_and_invalid_descriptor_registrations_are_rejected_safely() {
            let mut reactor = RuntimeReactor::new().unwrap();
            let (stream, _peer) = UnixStream::pair().unwrap();
            let fd = stream.as_raw_fd();
            let inactive = WaitKey(1, 1);

            reactor.add_fd(inactive, fd, IoInterest::READABLE).unwrap();
            assert_eq!(reactor.fd_token(fd), None);

            reactor.begin_wait(inactive).unwrap();
            for interest in [IoInterest(0), IoInterest(0b100)] {
                let error = reactor.add_fd(inactive, fd, interest).unwrap_err();
                assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
                assert_eq!(
                    error.to_string(),
                    "runtime reactor requires readable and/or writable interest"
                );
            }
            assert_eq!(reactor.fd_token(fd), None);
        }

        #[test]
        fn descriptor_events_wake_only_waiters_with_matching_interests() {
            let mut reactor = RuntimeReactor::new().unwrap();
            let (stream, _peer) = UnixStream::pair().unwrap();
            let fd = stream.as_raw_fd();
            let reader = WaitKey(1, 1);
            let writer = WaitKey(2, 1);
            reactor.begin_wait(reader).unwrap();
            reactor.begin_wait(writer).unwrap();
            reactor.add_fd(reader, fd, IoInterest::READABLE).unwrap();
            reactor.add_fd(writer, fd, IoInterest::WRITABLE).unwrap();
            let token = reactor.fd_token(fd).unwrap();

            reactor.dispatch_fd_event(token, true, false, false);
            assert_eq!(reactor.take_ready(), vec![reader]);
            assert!(reactor.is_waiting(writer));

            reactor.dispatch_fd_event(token, false, true, false);
            assert_eq!(reactor.take_ready(), vec![writer]);
        }

        #[test]
        fn stale_tokens_cannot_wake_a_waiter_after_fd_reuse() {
            let mut reactor = RuntimeReactor::new().unwrap();
            let (stream, _peer) = UnixStream::pair().unwrap();
            let fd = stream.as_raw_fd();
            let old = WaitKey(1, 1);
            reactor.begin_wait(old).unwrap();
            reactor.add_fd(old, fd, IoInterest::READABLE).unwrap();
            let stale_token = reactor.fd_token(fd).unwrap();
            reactor.cancel_wait(old).unwrap();

            let current = WaitKey(2, 1);
            reactor.begin_wait(current).unwrap();
            reactor.add_fd(current, fd, IoInterest::READABLE).unwrap();
            let current_token = reactor.fd_token(fd).unwrap();
            assert_ne!(stale_token, current_token);

            reactor.dispatch_fd_event(stale_token, true, false, false);
            assert!(reactor.take_ready().is_empty());
            assert!(reactor.is_waiting(current));
        }

        #[test]
        fn error_readiness_wakes_every_waiter_on_the_source_and_deduplicates() {
            let mut reactor = RuntimeReactor::new().unwrap();
            let (stream, _peer) = UnixStream::pair().unwrap();
            let fd = stream.as_raw_fd();
            let reader = WaitKey(1, 1);
            let writer = WaitKey(2, 1);
            reactor.begin_wait(reader).unwrap();
            reactor.begin_wait(writer).unwrap();
            reactor.add_fd(reader, fd, IoInterest::READABLE).unwrap();
            reactor.add_fd(reader, fd, IoInterest::READABLE).unwrap();
            reactor.add_fd(writer, fd, IoInterest::WRITABLE).unwrap();
            let token = reactor.fd_token(fd).unwrap();

            reactor.dispatch_fd_event(token, false, false, true);
            assert_eq!(reactor.take_ready(), vec![reader, writer]);
        }

        #[test]
        fn shared_descriptor_readiness_wakes_each_registered_waiter_once() {
            let mut reactor = RuntimeReactor::new().unwrap();
            let (stream, mut peer) = UnixStream::pair().unwrap();
            let fd = stream.as_raw_fd();
            let first = WaitKey(1, 1);
            let second = WaitKey(2, 1);
            reactor.begin_wait(first).unwrap();
            reactor.begin_wait(second).unwrap();
            reactor.add_fd(first, fd, IoInterest::READABLE).unwrap();
            reactor.add_fd(second, fd, IoInterest::READABLE).unwrap();

            peer.write_all(b"ready").unwrap();

            assert_eq!(
                reactor.poll(Some(Duration::from_secs(1))).unwrap(),
                vec![first, second]
            );
            assert_eq!(reactor.fd_token(fd), None);
        }

        #[test]
        fn nonblocking_poll_admits_fd_readiness_even_when_inbox_work_is_already_ready() {
            let mut reactor = RuntimeReactor::new().unwrap();
            let (stream, mut peer) = UnixStream::pair().unwrap();
            let inbox_key = WaitKey(1, 1);
            let fd_key = WaitKey(2, 1);
            reactor.begin_wait(inbox_key).unwrap();
            reactor.begin_wait(fd_key).unwrap();
            reactor
                .add_fd(fd_key, stream.as_raw_fd(), IoInterest::READABLE)
                .unwrap();
            reactor.handle().wake(inbox_key).unwrap();
            peer.write_all(b"ready").unwrap();

            assert_eq!(reactor.poll_nonblocking().unwrap(), vec![inbox_key, fd_key]);
        }

        #[test]
        fn cancelling_a_closed_last_descriptor_retires_every_internal_registration() {
            let _closed_fd_guard = lock_unpoisoned(&CLOSED_FD_TEST_LOCK);
            let mut reactor = RuntimeReactor::new().unwrap();
            let (stream, _peer) = UnixStream::pair().unwrap();
            let fd = duplicate_high_fd(stream.as_raw_fd());
            let key = WaitKey(1, 1);
            reactor.begin_wait(key).unwrap();
            reactor.add_fd(key, fd, IoInterest::READABLE).unwrap();
            let token = reactor.fd_token(fd).unwrap();
            assert_eq!(unsafe { libc::close(fd) }, 0);

            reactor.cancel_wait(key).unwrap();

            assert!(!reactor.fds.contains_key(&fd));
            assert!(!reactor.token_fds.contains_key(&token));
            assert!(!reactor.key_fds.contains_key(&key));
        }

        #[test]
        fn failed_interest_narrowing_retires_the_fd_and_wakes_surviving_waiters() {
            let _closed_fd_guard = lock_unpoisoned(&CLOSED_FD_TEST_LOCK);
            let mut reactor = RuntimeReactor::new().unwrap();
            let (stream, _peer) = UnixStream::pair().unwrap();
            let fd = duplicate_high_fd(stream.as_raw_fd());
            let reader = WaitKey(1, 1);
            let writer = WaitKey(2, 1);
            reactor.begin_wait(reader).unwrap();
            reactor.begin_wait(writer).unwrap();
            reactor.add_fd(reader, fd, IoInterest::READABLE).unwrap();
            reactor.add_fd(writer, fd, IoInterest::WRITABLE).unwrap();
            let token = reactor.fd_token(fd).unwrap();
            assert_eq!(unsafe { libc::close(fd) }, 0);

            reactor.cancel_wait(writer).unwrap();

            assert!(!reactor.fds.contains_key(&fd));
            assert!(!reactor.token_fds.contains_key(&token));
            assert!(!reactor.key_fds.contains_key(&reader));
            assert!(!reactor.key_fds.contains_key(&writer));
            assert_eq!(reactor.take_ready(), vec![reader]);
        }
    }
}
