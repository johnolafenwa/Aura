//! The host part of the `sync` module: state several tasks share.
//!
//! A `sync.Cell` is a lock and the one value it guards. Taking the lock
//! moves the value out to the task, and releasing it moves the value back,
//! so the value is never reachable from two tasks at once. A `sync.Atom` is
//! one atomic 64-bit word. `std/sync.au` builds `Mutex`, `AtomicInt`,
//! `AtomicBool`, and `Map` on the two.
//!
//! A task that finds a cell locked spins briefly, then waits through the
//! scheduler on the cell's wake queue, so it never blocks its worker thread.
//!
//! A task that ends while it holds a cell cannot put the value back: its
//! stack is gone. The task keeps weak references to its held cells so its
//! exit can poison each one and wake its waiters.

use super::*;
use std::cell::UnsafeCell;
use std::sync::atomic::{AtomicI64, AtomicU8};

const UNLOCKED: u8 = 0;
const LOCKED: u8 = 1;
const CONTENDED: u8 = 2;
const POISONED: u8 = 3;

/// How many times a taker checks a locked cell before it waits through the
/// scheduler. A critical section is usually shorter than a park and a wake.
const SPIN_LIMIT: u32 = 96;

/// The active prefix contains the cells this task holds in acquisition
/// order. Keeping slots up to the maximum nesting depth lets another
/// acquisition of the same lock reuse its weak reference.
#[derive(Default)]
pub(crate) struct HolderState {
    token: Cell<u64>,
    held: RefCell<Vec<HeldCell>>,
    epoch: Cell<u64>,
    depth: Cell<usize>,
}

struct HeldCell {
    cell: Weak<CellState>,
    epoch: u64,
}

/// Identifies the cells acquired inside a recovery call on this task.
#[derive(Clone, Copy)]
pub struct HeldCellMark(u64);

impl HolderState {
    fn acquired(&self, cell: &Arc<CellState>) {
        let depth = self.depth.get();
        let mut held = self.held.borrow_mut();
        let epoch = self.epoch.get();
        if let Some(slot) = held.get_mut(depth) {
            if !std::ptr::eq(slot.cell.as_ptr(), Arc::as_ptr(cell)) {
                slot.cell = Arc::downgrade(cell);
            }
            slot.epoch = epoch;
        } else {
            held.push(HeldCell {
                cell: Arc::downgrade(cell),
                epoch,
            });
        }
        self.depth.set(depth + 1);
        cell.holder.store(self.token(), Ordering::Relaxed);
    }

    fn released(&self, cell: &Arc<CellState>) {
        let last = self.depth.get() - 1;
        let mut held = self.held.borrow_mut();
        if !std::ptr::eq(held[last].cell.as_ptr(), Arc::as_ptr(cell)) {
            // Host calls can return two cells in a different order.
            let position = held[..last]
                .iter()
                .position(|slot| std::ptr::eq(slot.cell.as_ptr(), Arc::as_ptr(cell)))
                .expect("the current holder registered this cell");
            held[position..=last].rotate_left(1);
        }
        self.depth.set(last);
    }

    fn abandon_since(&self, mark: HeldCellMark) {
        let end = self.depth.get();
        if end == 0 {
            return;
        }
        let held = self.held.borrow();
        let active = &held[..end];
        let keep = active.partition_point(|entry| entry.epoch < mark.0);
        self.depth.set(keep);
        for cell in active[keep..]
            .iter()
            .filter_map(|entry| entry.cell.upgrade())
        {
            if cell.state.swap(POISONED, Ordering::Release) == CONTENDED {
                cell.wake_one();
            }
        }
    }

    fn token(&self) -> u64 {
        let token = self.token.get();
        if token != 0 {
            return token;
        }
        static NEXT_HOLDER_TOKEN: AtomicU64 = AtomicU64::new(1);
        let token = NEXT_HOLDER_TOKEN.fetch_add(1, Ordering::Relaxed);
        self.token.set(token);
        token
    }
}

thread_local! {
    /// The holder of code that runs outside any lightweight task.
    static THREAD_HOLDER: HolderState = HolderState::default();
}

fn with_current_holder<T>(f: impl FnOnce(&HolderState) -> T) -> T {
    CURRENT_LIGHTWEIGHT_TASK_CONTEXT.with(|slot| match slot.borrow().as_deref() {
        Some(context) => f(&context.sync_holder),
        None => THREAD_HOLDER.with(|holder| f(holder)),
    })
}

/// Poisons every cell still held at task completion.
pub(crate) fn holder_ended(holder: &HolderState) {
    holder.abandon_since(HeldCellMark(0));
}

/// Marks the start of a recovery call on this task. A caller can release
/// a pre-existing cell during the call, so a count alone cannot mark it.
pub fn mark_held_cells() -> HeldCellMark {
    with_current_holder(|holder| {
        if holder.depth.get() == 0 {
            return HeldCellMark(0);
        }
        let epoch = holder
            .epoch
            .get()
            .checked_add(1)
            .expect("lock recovery epoch exhausted");
        holder.epoch.set(epoch);
        HeldCellMark(epoch)
    })
}

/// Poisons only the cells acquired since `mark` on this task.
pub fn abandon_held_cells_since(mark: HeldCellMark) {
    with_current_holder(|holder| holder.abandon_since(mark));
}

/// Poisons every lock held by the current task.
pub fn abandon_held_cells() {
    abandon_held_cells_since(HeldCellMark(0));
}

/// A native caller can leave a unique generated-code box intact between
/// critical sections. Interpreter and host callers move the value itself.
pub(crate) enum GuardedValue {
    Value(Value),
    Native(crate::native_runtime::GuardedNativeBox),
}

impl GuardedValue {
    pub(crate) fn into_value(self) -> Value {
        match self {
            Self::Value(value) => value,
            Self::Native(guarded) => guarded.into_value(),
        }
    }
}

struct CellState {
    state: AtomicU8,
    /// The token of the holder, while locked.
    holder: AtomicU64,
    /// The guarded value. Only the holder reads or writes it. `None` while
    /// the holder has it, and after poisoning.
    value: UnsafeCell<Option<GuardedValue>>,
    /// Made on first contention. A token in it wakes one waiter.
    wake: OnceLock<ChannelValue>,
}

// SAFETY: `value` is reached only by the task that holds the lock, and a
// `Value` that is `Transfer` may move between worker threads, as a queue
// item does.
unsafe impl Send for CellState {}
unsafe impl Sync for CellState {}

impl CellState {
    fn wake_queue(&self) -> &ChannelValue {
        self.wake
            .get_or_init(|| ChannelValue::with_options(Some(1), true))
    }

    fn wake_one(&self) {
        let _ = self.wake_queue().try_send(Value::Unit);
    }

    fn try_lock(&self) -> bool {
        self.state
            .compare_exchange(UNLOCKED, LOCKED, Ordering::Acquire, Ordering::Relaxed)
            .is_ok()
    }

    fn check_poisoned(&self) -> bool {
        self.state.load(Ordering::Acquire) == POISONED
    }
}

fn poisoned_error() -> Diagnostic {
    Diagnostic::coded(
        "AU4001",
        "the lock is poisoned: a task failed while it held this `sync.Mutex`, and the value it held is lost",
    )
}

fn relock_error() -> Diagnostic {
    Diagnostic::coded(
        "AU4001",
        "this task already holds the lock: a `sync.Mutex` cannot be locked again by its holder",
    )
}

/// A `sync.Cell` or a `sync.Atom`.
#[derive(Clone)]
pub struct SyncValue(SyncHandle);

#[derive(Clone)]
enum SyncHandle {
    Cell(Arc<CellState>),
    Atom(Arc<AtomicI64>),
}

impl std::fmt::Debug for SyncValue {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.type_name())
    }
}

impl PartialEq for SyncValue {
    fn eq(&self, other: &Self) -> bool {
        match (&self.0, &other.0) {
            (SyncHandle::Cell(left), SyncHandle::Cell(right)) => Arc::ptr_eq(left, right),
            (SyncHandle::Atom(left), SyncHandle::Atom(right)) => Arc::ptr_eq(left, right),
            _ => false,
        }
    }
}

impl SyncValue {
    /// A cell that holds `value`, unlocked.
    pub fn cell(value: Value) -> Self {
        Self(SyncHandle::Cell(Arc::new(CellState {
            state: AtomicU8::new(UNLOCKED),
            holder: AtomicU64::new(0),
            value: UnsafeCell::new(Some(GuardedValue::Value(value))),
            wake: OnceLock::new(),
        })))
    }

    /// An atomic word that holds `value`.
    pub fn atom(value: i64) -> Self {
        Self(SyncHandle::Atom(Arc::new(AtomicI64::new(value))))
    }

    /// The Aura type of the handle.
    pub fn type_name(&self) -> &'static str {
        match self.0 {
            SyncHandle::Cell(_) => "sync.Cell",
            SyncHandle::Atom(_) => "sync.Atom",
        }
    }

    fn as_cell(&self, call: &str) -> Result<&Arc<CellState>> {
        match &self.0 {
            SyncHandle::Cell(cell) => Ok(cell),
            SyncHandle::Atom(_) => Err(wrong_handle(call, "sync.Cell", "sync.Atom")),
        }
    }

    fn as_atom(&self, call: &str) -> Result<&AtomicI64> {
        match &self.0 {
            SyncHandle::Atom(atom) => Ok(atom),
            SyncHandle::Cell(_) => Err(wrong_handle(call, "sync.Atom", "sync.Cell")),
        }
    }

    /// Locks the cell and moves its value out, waiting while another task
    /// holds it.
    pub fn take(&self) -> Result<Value> {
        self.take_guarded().map(GuardedValue::into_value)
    }

    pub(crate) fn take_guarded(&self) -> Result<GuardedValue> {
        let cell = self.as_cell("sync.__aura_cell_take")?;
        if !cell.try_lock() {
            // The scheduler changes the current task while a waiter yields.
            let token = with_current_holder(HolderState::token);
            lock_contended(cell, token)?;
        }
        Ok(Self::take_locked(cell))
    }

    /// Takes the value without yielding, allowing a native caller to borrow
    /// the handle in place while the box is accessible.
    pub(crate) fn try_take_guarded(&self) -> Result<Option<GuardedValue>> {
        let cell = self.as_cell("sync.__aura_cell_take")?;
        Ok(cell.try_lock().then(|| Self::take_locked(cell)))
    }

    #[inline]
    fn take_locked(cell: &Arc<CellState>) -> GuardedValue {
        with_current_holder(|holder| holder.acquired(cell));
        // SAFETY: this task just acquired the lock.
        unsafe { (*cell.value.get()).take() }.expect("an unlocked cell holds its value")
    }

    /// Moves `value` into the cell and unlocks it.
    pub fn put(&self, value: Value) -> Result<()> {
        self.put_guarded(GuardedValue::Value(value))
    }

    pub(crate) fn put_guarded(&self, value: GuardedValue) -> Result<()> {
        let cell = self.as_cell("sync.__aura_cell_put")?;
        let owned = with_current_holder(|holder| {
            if cell.holder.load(Ordering::Relaxed) != holder.token()
                || !matches!(cell.state.load(Ordering::Relaxed), LOCKED | CONTENDED)
            {
                return false;
            }
            holder.released(cell);
            true
        });
        if !owned {
            return Err(Diagnostic::coded(
                "AU4001",
                "internal error: a `sync.Mutex` was released by a task that does not hold it",
            ));
        }
        // SAFETY: this task holds the lock.
        unsafe {
            *cell.value.get() = Some(value);
        }
        cell.holder.store(0, Ordering::Relaxed);
        if cell.state.swap(UNLOCKED, Ordering::Release) == CONTENDED {
            cell.wake_one();
        }
        Ok(())
    }

    /// Whether a holder failed while it held the cell.
    pub fn is_poisoned(&self) -> Result<bool> {
        Ok(self.as_cell("sync.__aura_cell_poisoned")?.check_poisoned())
    }

    pub fn load(&self) -> Result<i64> {
        Ok(self
            .as_atom("sync.__aura_atom_load")?
            .load(Ordering::SeqCst))
    }

    pub fn store(&self, value: i64) -> Result<()> {
        self.as_atom("sync.__aura_atom_store")?
            .store(value, Ordering::SeqCst);
        Ok(())
    }

    /// Adds `delta` and returns the new value. An overflow traps after the
    /// stored word has wrapped.
    pub fn add(&self, delta: i64) -> Result<i64> {
        let previous = self
            .as_atom("sync.__aura_atom_add")?
            .fetch_add(delta, Ordering::SeqCst);
        previous.checked_add(delta).ok_or_else(|| {
            Diagnostic::coded(
                "AU4002",
                "integer overflow in `sync.AtomicInt.add`: the stored value has wrapped",
            )
        })
    }

    pub fn swap(&self, value: i64) -> Result<i64> {
        Ok(self
            .as_atom("sync.__aura_atom_swap")?
            .swap(value, Ordering::SeqCst))
    }

    /// Stores `new` when the word holds `expected`, and says whether it did.
    pub fn compare_and_set(&self, expected: i64, new: i64) -> Result<bool> {
        Ok(self
            .as_atom("sync.__aura_atom_compare_and_set")?
            .compare_exchange(expected, new, Ordering::SeqCst, Ordering::SeqCst)
            .is_ok())
    }
}

fn wrong_handle(call: &str, expected: &str, found: &str) -> Diagnostic {
    Diagnostic::coded(
        "AU4001",
        format!("`{call}` expected a `{expected}`, found `{found}`"),
    )
}

/// Takes a cell another holder has, or reports why it cannot be taken.
#[cold]
fn lock_contended(cell: &Arc<CellState>, token: u64) -> Result<()> {
    if cell.holder.load(Ordering::Relaxed) == token && !cell.check_poisoned() {
        return Err(relock_error());
    }
    for _ in 0..SPIN_LIMIT {
        if cell.state.load(Ordering::Relaxed) == UNLOCKED && cell.try_lock() {
            return Ok(());
        }
        std::hint::spin_loop();
    }
    let wake = cell.wake_queue();
    loop {
        // The holder's release observes CONTENDED in the same atomic word
        // it unlocks. A wake token sent before we park stays in the queue.
        match cell.state.load(Ordering::Relaxed) {
            POISONED => {
                cell.wake_one();
                return Err(poisoned_error());
            }
            UNLOCKED => {
                if cell
                    .state
                    .compare_exchange(UNLOCKED, CONTENDED, Ordering::Acquire, Ordering::Relaxed)
                    .is_ok()
                {
                    return Ok(());
                }
            }
            LOCKED => {
                let _ = cell.state.compare_exchange(
                    LOCKED,
                    CONTENDED,
                    Ordering::Relaxed,
                    Ordering::Relaxed,
                );
            }
            _ => {
                let _ = wait_for_runtime_scheduler(
                    vec![wake.clone()],
                    false,
                    Vec::new(),
                    Vec::new(),
                    None,
                    None,
                );
                let _ = wake.try_recv();
            }
        }
    }
}

fn int(value: i64) -> Value {
    Value::Int(IntegerValue::from_i64(value))
}

fn int_arg(args: &[Value], index: usize, name: &str) -> Result<i64> {
    match &args[index] {
        Value::Int(value) => value.as_i128().and_then(|value| i64::try_from(value).ok()),
        _ => None,
    }
    .ok_or_else(|| {
        Diagnostic::coded(
            "AU4001",
            format!(
                "`{}` expects argument {} to be `int64`",
                name.replace("::", "."),
                index + 1
            ),
        )
    })
}

fn handle_arg<'a>(args: &'a [Value], name: &str) -> Result<&'a SyncValue> {
    match &args[0] {
        Value::Sync(handle) => Ok(handle),
        _ => Err(Diagnostic::coded(
            "AU4001",
            format!("`{}` expects a `sync` handle", name.replace("::", ".")),
        )),
    }
}

pub fn host_cell_new(mut args: Vec<Value>, _program_args: Option<&[String]>) -> Result<Value> {
    host_expect_arity("sync::__aura_cell_new", &args, 1)?;
    Ok(Value::Sync(SyncValue::cell(args.remove(0))))
}

pub fn host_cell_take(args: Vec<Value>, _program_args: Option<&[String]>) -> Result<Value> {
    let name = "sync::__aura_cell_take";
    host_expect_arity(name, &args, 1)?;
    handle_arg(&args, name)?.take()
}

pub fn host_cell_put(mut args: Vec<Value>, _program_args: Option<&[String]>) -> Result<Value> {
    let name = "sync::__aura_cell_put";
    host_expect_arity(name, &args, 2)?;
    let value = args.remove(1);
    handle_arg(&args, name)?.put(value).map(|()| Value::Unit)
}

pub fn host_cell_poisoned(args: Vec<Value>, _program_args: Option<&[String]>) -> Result<Value> {
    let name = "sync::__aura_cell_poisoned";
    host_expect_arity(name, &args, 1)?;
    handle_arg(&args, name)?.is_poisoned().map(Value::Bool)
}

pub fn host_atom_new(args: Vec<Value>, _program_args: Option<&[String]>) -> Result<Value> {
    let name = "sync::__aura_atom_new";
    host_expect_arity(name, &args, 1)?;
    Ok(Value::Sync(SyncValue::atom(int_arg(&args, 0, name)?)))
}

pub fn host_atom_load(args: Vec<Value>, _program_args: Option<&[String]>) -> Result<Value> {
    let name = "sync::__aura_atom_load";
    host_expect_arity(name, &args, 1)?;
    handle_arg(&args, name)?.load().map(int)
}

pub fn host_atom_store(args: Vec<Value>, _program_args: Option<&[String]>) -> Result<Value> {
    let name = "sync::__aura_atom_store";
    host_expect_arity(name, &args, 2)?;
    handle_arg(&args, name)?
        .store(int_arg(&args, 1, name)?)
        .map(|()| Value::Unit)
}

pub fn host_atom_add(args: Vec<Value>, _program_args: Option<&[String]>) -> Result<Value> {
    let name = "sync::__aura_atom_add";
    host_expect_arity(name, &args, 2)?;
    handle_arg(&args, name)?
        .add(int_arg(&args, 1, name)?)
        .map(int)
}

pub fn host_atom_swap(args: Vec<Value>, _program_args: Option<&[String]>) -> Result<Value> {
    let name = "sync::__aura_atom_swap";
    host_expect_arity(name, &args, 2)?;
    handle_arg(&args, name)?
        .swap(int_arg(&args, 1, name)?)
        .map(int)
}

pub fn host_atom_compare_and_set(
    args: Vec<Value>,
    _program_args: Option<&[String]>,
) -> Result<Value> {
    let name = "sync::__aura_atom_compare_and_set";
    host_expect_arity(name, &args, 3)?;
    handle_arg(&args, name)?
        .compare_and_set(int_arg(&args, 1, name)?, int_arg(&args, 2, name)?)
        .map(Value::Bool)
}

/// The `sync` host function named `name`.
pub fn sync_host_function(name: &str) -> Option<host_builtins::HostBuiltinFn> {
    Some(match name {
        "sync::__aura_cell_new" => host_cell_new,
        "sync::__aura_cell_take" => host_cell_take,
        "sync::__aura_cell_put" => host_cell_put,
        "sync::__aura_cell_poisoned" => host_cell_poisoned,
        "sync::__aura_atom_new" => host_atom_new,
        "sync::__aura_atom_load" => host_atom_load,
        "sync::__aura_atom_store" => host_atom_store,
        "sync::__aura_atom_add" => host_atom_add,
        "sync::__aura_atom_swap" => host_atom_swap,
        "sync::__aura_atom_compare_and_set" => host_atom_compare_and_set,
        _ => return None,
    })
}

#[cfg(test)]
#[path = "sync_host_tests.rs"]
mod tests;
