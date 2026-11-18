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
    held: RefCell<Vec<HeldCell>>,
    epoch: Cell<u64>,
    depth: Cell<usize>,
    /// Installed on acquisition. Task retirement in programs without
    /// mutexes can link without the channel wake and poisoning code.
    on_exit: Cell<Option<fn(&HolderState)>>,
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
            self.on_exit.set(Some(Self::abandon_all));
            held.push(HeldCell {
                cell: Arc::downgrade(cell),
                epoch,
            });
        }
        self.depth.set(depth + 1);
    }

    fn holds(&self, cell: &Arc<CellState>) -> bool {
        self.held.borrow()[..self.depth.get()]
            .iter()
            .any(|slot| std::ptr::eq(slot.cell.as_ptr(), Arc::as_ptr(cell)))
    }

    fn released(&self, cell: &Arc<CellState>) -> bool {
        let Some(last) = self.depth.get().checked_sub(1) else {
            return false;
        };
        let mut held = self.held.borrow_mut();
        if !std::ptr::eq(held[last].cell.as_ptr(), Arc::as_ptr(cell)) {
            // Host calls can return two cells in a different order.
            let Some(position) = held[..last]
                .iter()
                .position(|slot| std::ptr::eq(slot.cell.as_ptr(), Arc::as_ptr(cell)))
            else {
                return false;
            };
            held[position..=last].rotate_left(1);
        }
        self.depth.set(last);
        true
    }

    fn abandon_all(&self) {
        self.abandon_since(HeldCellMark(0));
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
            cell.finish(POISONED);
        }
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
    if let Some(poison) = holder.on_exit.get() {
        poison(holder);
    }
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
    Int64(i64),
    Bool(bool),
}

impl GuardedValue {
    pub(crate) fn into_value(self) -> Value {
        match self {
            Self::Value(value) => value,
            Self::Native(guarded) => guarded.into_value(),
            Self::Int64(word) => int(word),
            Self::Bool(flag) => Value::Bool(flag),
        }
    }

    /// Borrows a map under its cell's lock. Native readers only inspect
    /// the dictionary and can leave its box in the cell.
    pub(crate) fn copy_map_entry(
        slot: &Option<Self>,
        find: impl FnOnce(&MapValue) -> Option<&Value>,
    ) -> Result<Option<Value>> {
        find(Self::map(slot)?)
            .map(try_clone_array_containing_value)
            .transpose()
    }

    fn map(slot: &Option<Self>) -> Result<&MapValue> {
        match slot {
            Some(Self::Value(Value::Map(map))) => Some(map),
            Some(Self::Native(boxed)) => boxed.as_map(),
            _ => None,
        }
        .ok_or_else(map_type_error)
    }

    pub(crate) fn take(slot: &mut Option<Self>) -> Self {
        slot.take().expect("an unlocked cell holds its value")
    }
}

struct CellState {
    state: AtomicU8,
    /// Only the holder accesses the value. A lock block takes it out
    /// until release; a map lookup reads it in place.
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

    fn finish(&self, state: u8) {
        if self.state.swap(state, Ordering::Release) == CONTENDED {
            self.wake_one();
        }
    }

    /// This read runs entirely in Rust without suspending the task or invoking
    /// Aura code. The result owns its word before the cell wakes another reader.
    fn read_locked_map_word(&self, key: i64) -> Result<Option<i64>> {
        struct ReadGuard<'a> {
            cell: &'a CellState,
            exit: u8,
        }
        impl Drop for ReadGuard<'_> {
            fn drop(&mut self) {
                self.cell.finish(self.exit);
            }
        }

        let mut guard = ReadGuard {
            cell: self,
            exit: POISONED,
        };
        // SAFETY: the caller acquired the cell; the guard releases it.
        let map = GuardedValue::map(unsafe { &*self.value.get() })?;
        let word = match map.entries.value_of_i64(key) {
            None => None,
            Some(Value::Int(integer)) => Some(
                integer
                    .as_i128()
                    .and_then(|n| i64::try_from(n).ok())
                    .ok_or_else(|| {
                        Diagnostic::coded("AU4001", "sync.Map.get expected an int64 value")
                    })?,
            ),
            Some(_) => {
                return Err(Diagnostic::coded(
                    "AU4001",
                    "sync.Map.get expected an int64 value",
                ))
            }
        };
        guard.exit = UNLOCKED;
        Ok(word)
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

fn map_type_error() -> Diagnostic {
    Diagnostic::coded(
        "AU4001",
        "sync.Map.get expected a cell containing a dictionary",
    )
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

/// Standard library wrapper classes whose native representation is the
/// handle itself. Local classes keep their declared fields and rendering.
pub fn is_handle_class(name: &str) -> bool {
    matches!(
        name,
        "sync.Mutex" | "sync.Locked" | "sync.Map" | "sync.AtomicInt" | "sync.AtomicBool"
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
        self.take_with(|slot| GuardedValue::take(slot).into_value())
    }

    pub(crate) fn take_with<T>(
        &self,
        read: impl FnOnce(&mut Option<GuardedValue>) -> T,
    ) -> Result<T> {
        let cell = self.as_cell("sync.__aura_cell_take")?;
        if !cell.try_lock() {
            // The scheduler changes the current task while a waiter yields.
            lock_contended(cell)?;
        }
        Ok(Self::take_locked(cell, read))
    }

    /// Takes the value without yielding, allowing a native caller to borrow
    /// the handle in place while the box is accessible.
    pub(crate) fn try_take_with<T>(
        &self,
        read: impl FnOnce(&mut Option<GuardedValue>) -> T,
    ) -> Result<Option<T>> {
        let cell = self.as_cell("sync.__aura_cell_take")?;
        Ok(cell.try_lock().then(|| Self::take_locked(cell, read)))
    }

    #[inline]
    fn take_locked<T>(
        cell: &Arc<CellState>,
        read: impl FnOnce(&mut Option<GuardedValue>) -> T,
    ) -> T {
        with_current_holder(|holder| holder.acquired(cell));
        // SAFETY: this task just acquired the lock.
        read(unsafe { &mut *cell.value.get() })
    }

    /// The outer option distinguishes a contended cell from a missing key.
    pub(crate) fn try_read_map_word(&self, key: i64) -> Result<Option<Option<i64>>> {
        let cell = self.as_cell("sync.__aura_map_get")?;
        cell.try_lock()
            .then(|| cell.read_locked_map_word(key))
            .transpose()
    }

    pub(crate) fn read_map_word(&self, key: i64) -> Result<Option<i64>> {
        let cell = self.as_cell("sync.__aura_map_get")?;
        if !cell.try_lock() {
            lock_contended(cell)?;
        }
        cell.read_locked_map_word(key)
    }

    /// Moves `value` into the cell and unlocks it.
    pub fn put(&self, value: Value) -> Result<()> {
        self.put_with(|slot| *slot = Some(GuardedValue::Value(value)))
    }

    pub(crate) fn put_with(&self, write: impl FnOnce(&mut Option<GuardedValue>)) -> Result<()> {
        let cell = self.as_cell("sync.__aura_cell_put")?;
        let owned = with_current_holder(|holder| holder.released(cell));
        if !owned {
            return Err(Diagnostic::coded(
                "AU4001",
                "internal error: a `sync.Mutex` was released by a task that does not hold it",
            ));
        }
        // SAFETY: this task holds the lock.
        write(unsafe { &mut *cell.value.get() });
        cell.finish(UNLOCKED);
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
fn lock_contended(cell: &Arc<CellState>) -> Result<()> {
    if with_current_holder(|holder| holder.holds(cell)) {
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

pub fn host_map_get(args: Vec<Value>, _program_args: Option<&[String]>) -> Result<Value> {
    let name = "sync::__aura_map_get";
    host_expect_arity(name, &args, 2)?;
    let handle = handle_arg(&args, name)?;
    let found = handle.take_with(|slot| {
        GuardedValue::copy_map_entry(slot, |map| map.entries.value_of(&args[1]))
    })??;
    handle.put_with(|_| {})?;
    Ok(found.map(lookup_found).unwrap_or_else(lookup_missing))
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
        "sync::__aura_map_get" => host_map_get,
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
