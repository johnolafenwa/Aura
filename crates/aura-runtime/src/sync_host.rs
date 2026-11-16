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
//! stack is gone. Each holder counts the cells it holds. When a task ends
//! with a count above zero, its holder token joins the dead holders, the
//! waiting cells are woken, and a cell whose holder is dead is poisoned:
//! every later take traps.

use super::*;
use std::cell::UnsafeCell;
use std::collections::HashSet;
use std::sync::atomic::{AtomicI64, AtomicU8, AtomicUsize};

const UNLOCKED: u8 = 0;
const LOCKED: u8 = 1;
const POISONED: u8 = 2;

/// How many times a taker checks a locked cell before it waits through the
/// scheduler. A critical section is usually shorter than a park and a wake.
const SPIN_LIMIT: u32 = 96;

/// What a task or thread needs to hold cells: a token no other live holder
/// has, and how many cells it holds now.
#[derive(Default)]
pub(crate) struct HolderState {
    token: Cell<u64>,
    held: Cell<u32>,
}

impl HolderState {
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

/// The tokens of holders that ended while they held a cell.
fn dead_holders() -> &'static Mutex<HashSet<u64>> {
    static DEAD: OnceLock<Mutex<HashSet<u64>>> = OnceLock::new();
    DEAD.get_or_init(|| Mutex::new(HashSet::new()))
}

/// How many tokens `dead_holders` has. Zero in a program where no task has
/// failed inside a lock, which lets a waiter skip the set's lock.
static DEAD_HOLDER_COUNT: AtomicUsize = AtomicUsize::new(0);

/// The cells some task is waiting on, to wake when a holder dies.
fn waited_cells() -> &'static Mutex<Vec<Weak<CellState>>> {
    static WAITED: OnceLock<Mutex<Vec<Weak<CellState>>>> = OnceLock::new();
    WAITED.get_or_init(|| Mutex::new(Vec::new()))
}

/// Called when a lightweight task has ended, by return, trap, or
/// cancellation. A task that still holds cells poisons them.
pub(crate) fn holder_ended(holder: &HolderState) {
    if holder.held.get() == 0 {
        return;
    }
    holder.held.set(0);
    lock_mutex(dead_holders()).insert(holder.token());
    // A holder that lives on, such as a thread that runs one program after
    // another, takes its next lock under a fresh token.
    holder.token.set(0);
    DEAD_HOLDER_COUNT.fetch_add(1, Ordering::SeqCst);
    let waited = lock_mutex(waited_cells())
        .iter()
        .filter_map(Weak::upgrade)
        .collect::<Vec<_>>();
    for cell in waited {
        cell.wake_one();
    }
}

/// Poisons every cell the running task or thread holds. Code that stops a
/// trap from ending its task calls this, because the frames that would
/// have released the locks are gone.
pub fn abandon_held_cells() {
    with_current_holder(holder_ended);
}

struct CellState {
    state: AtomicU8,
    /// The token of the holder, while locked.
    holder: AtomicU64,
    waiters: AtomicUsize,
    /// The guarded value. Only the holder reads or writes it. `None` while
    /// the holder has it, and after poisoning.
    value: UnsafeCell<Option<Value>>,
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

    /// Whether the cell is poisoned, marking it so when its holder is dead.
    fn check_poisoned(&self) -> bool {
        match self.state.load(Ordering::SeqCst) {
            POISONED => return true,
            UNLOCKED => return false,
            _ => {}
        }
        if DEAD_HOLDER_COUNT.load(Ordering::SeqCst) == 0 {
            return false;
        }
        let holder = self.holder.load(Ordering::SeqCst);
        if !lock_mutex(dead_holders()).contains(&holder) {
            return false;
        }
        // The dead holder never unlocks, so the cell is still locked by it
        // unless another waiter marked it first.
        if self.holder.load(Ordering::SeqCst) == holder {
            let _ = self.state.compare_exchange(
                LOCKED,
                POISONED,
                Ordering::SeqCst,
                Ordering::SeqCst,
            );
        }
        self.state.load(Ordering::SeqCst) == POISONED
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
            waiters: AtomicUsize::new(0),
            value: UnsafeCell::new(Some(value)),
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
        let cell = self.as_cell("sync.__aura_cell_take")?;
        // The holder is not kept borrowed across the wait: a waiting task
        // yields, and the scheduler then changes the current task.
        let token = with_current_holder(HolderState::token);
        if !cell.try_lock() {
            lock_contended(cell, token)?;
        }
        cell.holder.store(token, Ordering::Relaxed);
        with_current_holder(|holder| holder.held.set(holder.held.get() + 1));
        // SAFETY: this task holds the lock.
        match unsafe { (*cell.value.get()).take() } {
            Some(value) => Ok(value),
            None => unreachable!("an unlocked cell holds its value"),
        }
    }

    /// Moves `value` into the cell and unlocks it. The caller is the holder
    /// that `take` gave the previous value to.
    pub fn put(&self, value: Value) -> Result<()> {
        let cell = self.as_cell("sync.__aura_cell_put")?;
        let token = with_current_holder(|holder| {
            holder.held.set(holder.held.get().saturating_sub(1));
            holder.token()
        });
        if cell.state.load(Ordering::Relaxed) != LOCKED
            || cell.holder.load(Ordering::Relaxed) != token
        {
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
        cell.state.store(UNLOCKED, Ordering::SeqCst);
        if cell.waiters.load(Ordering::SeqCst) != 0 {
            cell.wake_one();
        }
        Ok(())
    }

    /// Whether a holder failed while it held the cell.
    pub fn is_poisoned(&self) -> Result<bool> {
        Ok(self.as_cell("sync.__aura_cell_poisoned")?.check_poisoned())
    }

    pub fn load(&self) -> Result<i64> {
        Ok(self.as_atom("sync.__aura_atom_load")?.load(Ordering::SeqCst))
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
    if cell.state.load(Ordering::SeqCst) == LOCKED
        && cell.holder.load(Ordering::SeqCst) == token
    {
        return Err(relock_error());
    }
    for _ in 0..SPIN_LIMIT {
        if cell.state.load(Ordering::Relaxed) == UNLOCKED && cell.try_lock() {
            return Ok(());
        }
        std::hint::spin_loop();
    }
    cell.waiters.fetch_add(1, Ordering::SeqCst);
    lock_mutex(waited_cells()).push(Arc::downgrade(cell));
    let outcome = loop {
        if cell.try_lock() {
            break Ok(());
        }
        if cell.check_poisoned() {
            // Each waiter that finds the poison wakes the next one.
            cell.wake_one();
            break Err(poisoned_error());
        }
        let wake = cell.wake_queue();
        let _ = wait_for_runtime_scheduler(
            vec![wake.clone()],
            false,
            Vec::new(),
            Vec::new(),
            None,
            None,
        );
        let _ = wake.try_recv();
    };
    {
        let mut waited = lock_mutex(waited_cells());
        if let Some(index) = waited
            .iter()
            .position(|waiting| std::ptr::eq(waiting.as_ptr(), Arc::as_ptr(cell)))
        {
            waited.swap_remove(index);
        }
    }
    cell.waiters.fetch_sub(1, Ordering::SeqCst);
    outcome
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

pub fn host_atom_compare_and_set(args: Vec<Value>, _program_args: Option<&[String]>) -> Result<Value> {
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
