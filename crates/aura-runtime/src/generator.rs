//! Generator frames (Batch 9 step 9c).
//!
//! A generator owns a coroutine stack of its own. `resume` runs the body on
//! that stack, on the thread and task that call it, until the body yields an
//! item, returns, or traps. Both backends share this machinery: each one
//! supplies the body as a closure and, for the direct backend, hooks that
//! move the body's frame-stack entries out while it is suspended.
//!
//! A scheduler wait inside a body does not suspend the task from the
//! generator stack. The body suspends the generator with the wait instead,
//! and `resume`, back on the caller's stack, performs the wait through the
//! enclosing task (or enclosing generator, recursively) and resumes the
//! body with the wake reason. The task coroutine therefore stays the only
//! context that ever suspends to the scheduler.

use super::{
    enter_task_stack_bounds, task_stack_bounds, Diagnostic, RuntimeSchedulerWakeReason,
    TaskStackBounds, TaskYield, Value,
};
use corosensei::stack::DefaultStack;
use corosensei::{Coroutine, CoroutineResult, Yielder};
use std::cell::{Cell, RefCell};
use std::panic::{self, AssertUnwindSafe};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread::ThreadId;

/// What a resumed body receives from `yield` or from a wait it handed out.
pub(super) enum GeneratorInput {
    /// Produce the next item.
    Next,
    /// Stop: `yield` returns false so the body runs its cleanup and returns.
    Close,
    /// A wait the body handed out finished, with the enclosing context's
    /// answer.
    Woke(Option<RuntimeSchedulerWakeReason>),
}

/// What a suspended body hands back to `resume`.
pub(super) enum GeneratorSignal {
    Item(Value),
    Wait(TaskYield),
}

type GeneratorBody = Box<dyn FnOnce() -> Result<(), Diagnostic>>;
type GeneratorCoroutine =
    Coroutine<GeneratorInput, GeneratorSignal, Result<(), Diagnostic>, DefaultStack>;
type GeneratorYielder = Yielder<GeneratorInput, GeneratorSignal>;

/// Moves a body's per-task state out while it is suspended and back in
/// when it resumes. The direct backend moves its frame-stack entries.
pub trait GeneratorHooks {
    /// Called on the caller's stack just before the body runs again.
    fn before_resume(&mut self);
    /// Called on the caller's stack right after the body suspended.
    fn after_suspend(&mut self);
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum GeneratorStatus {
    /// Created; the body has not started.
    Fresh,
    /// Suspended at a `yield`.
    Suspended,
    /// Running on its stack right now.
    Running,
    /// Returned or closed.
    Done,
    /// The body trapped. Resuming reports the failure again.
    Failed,
}

struct GeneratorState {
    status: GeneratorStatus,
    body: Option<GeneratorBody>,
    coroutine: Option<GeneratorCoroutine>,
    bounds: Option<TaskStackBounds>,
    stack_bytes: usize,
    item: Option<Value>,
    failure: Option<Diagnostic>,
    hooks: Option<Box<dyn GeneratorHooks>>,
    /// Whether dropping a suspended body may unwind its Rust frames. Direct
    /// backend frames cannot be unwound by the coroutine library, so its
    /// generators reset the stack instead and leave owned values to the
    /// task's ledger.
    unwind_on_drop: bool,
    /// The body's yielder, set when it first runs.
    yielder: Arc<YielderCell>,
    owner: ThreadId,
}

// SAFETY: a generator is not Transfer, so the compiler never lets one leave
// the task that created it, and tasks never migrate between threads.
// `resume` checks the owning thread before touching the coroutine.
unsafe impl Send for GeneratorState {}

/// Holds the body's yielder address once the body starts.
struct YielderCell(Cell<*const GeneratorYielder>);

// SAFETY: as for `GeneratorState`: the cell is only read on the owning
// thread.
unsafe impl Send for YielderCell {}
unsafe impl Sync for YielderCell {}

impl Drop for GeneratorState {
    fn drop(&mut self) {
        if let Some(mut coroutine) = self.coroutine.take() {
            if coroutine.started() && !coroutine.done() {
                if self.unwind_on_drop {
                    coroutine.force_unwind();
                } else {
                    // SAFETY: the body's direct frames hold only words the
                    // task's ledger owns; nothing on the stack is used again.
                    unsafe { coroutine.force_reset() };
                }
            }
        }
    }
}

/// A runtime generator value.
#[derive(Clone)]
pub struct GeneratorValue {
    inner: Arc<Mutex<GeneratorState>>,
}

impl std::fmt::Debug for GeneratorValue {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("<generator>")
    }
}

impl PartialEq for GeneratorValue {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.inner, &other.inner)
    }
}

impl Drop for GeneratorValue {
    /// Dropping the last handle of a suspended generator closes it, so the
    /// body's `with` exits run. A failure there has no caller to reach: the
    /// direct backend reports a trap itself, and the interpreter drops it.
    fn drop(&mut self) {
        if Arc::strong_count(&self.inner) != 1 || std::thread::panicking() {
            return;
        }
        let suspended = {
            let state = lock(&self.inner);
            state.status == GeneratorStatus::Suspended && state.owner == std::thread::current().id()
        };
        if suspended {
            let _ = self.close();
        }
    }
}

thread_local! {
    /// The yielders of the generator bodies running on this thread,
    /// innermost last.
    static RUNNING_GENERATORS: RefCell<Vec<Arc<YielderCell>>> = const { RefCell::new(Vec::new()) };
}

/// The default reserved stack of a generator body run by the direct
/// backend. It is committed lazily.
pub const DIRECT_GENERATOR_STACK_BYTES: usize = 1024 * 1024;

/// The reserved stack of a generator body run by the interpreter, whose
/// Aura calls cost far more stack.
pub const INTERPRETED_GENERATOR_STACK_BYTES: usize = 16 * 1024 * 1024;

fn lock(state: &Mutex<GeneratorState>) -> MutexGuard<'_, GeneratorState> {
    state
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn generator_error(message: impl Into<String>) -> Diagnostic {
    Diagnostic::coded("AU4010", message.into())
}

impl GeneratorValue {
    /// A generator that runs `body` on its first resume.
    pub fn new(
        body: impl FnOnce() -> Result<(), Diagnostic> + 'static,
        stack_bytes: usize,
        unwind_on_drop: bool,
        hooks: Option<Box<dyn GeneratorHooks>>,
    ) -> Self {
        Self {
            inner: Arc::new(Mutex::new(GeneratorState {
                status: GeneratorStatus::Fresh,
                body: Some(Box::new(body)),
                coroutine: None,
                bounds: None,
                stack_bytes,
                item: None,
                failure: None,
                hooks,
                unwind_on_drop,
                yielder: Arc::new(YielderCell(Cell::new(std::ptr::null()))),
                owner: std::thread::current().id(),
            })),
        }
    }

    /// Runs the body until it yields (true, with the item ready for
    /// [`GeneratorValue::take_item`]) or ends (false).
    pub fn resume(&self) -> Result<bool, Diagnostic> {
        self.drive(GeneratorInput::Next)
    }

    /// Closes the generator: a suspended body sees its `yield` return false
    /// and runs its cleanup. Closing an ended generator does nothing.
    pub fn close(&self) -> Result<(), Diagnostic> {
        let status = lock(&self.inner).status;
        match status {
            GeneratorStatus::Fresh => {
                let mut state = lock(&self.inner);
                state.body = None;
                state.status = GeneratorStatus::Done;
                Ok(())
            }
            GeneratorStatus::Suspended => {
                if self.drive(GeneratorInput::Close)? {
                    let mut state = lock(&self.inner);
                    state.item = None;
                    state.status = GeneratorStatus::Failed;
                    let failure = generator_error("a generator yielded while it was being closed");
                    state.failure = Some(failure.clone());
                    return Err(failure);
                }
                Ok(())
            }
            GeneratorStatus::Running => Err(generator_error(
                "a generator cannot close itself while it is running",
            )),
            GeneratorStatus::Done | GeneratorStatus::Failed => Ok(()),
        }
    }

    /// The item the last successful [`GeneratorValue::resume`] produced.
    pub fn take_item(&self) -> Result<Value, Diagnostic> {
        lock(&self.inner)
            .item
            .take()
            .ok_or_else(|| generator_error("a generator has no item ready"))
    }

    fn drive(&self, mut input: GeneratorInput) -> Result<bool, Diagnostic> {
        let mut coroutine = {
            let mut state = lock(&self.inner);
            if state.owner != std::thread::current().id() {
                return Err(generator_error(
                    "a generator was resumed outside the task that created it",
                ));
            }
            match state.status {
                GeneratorStatus::Running => {
                    return Err(generator_error("a generator cannot resume itself"))
                }
                GeneratorStatus::Done => return Ok(false),
                GeneratorStatus::Failed => {
                    return Err(state.failure.clone().unwrap_or_else(|| {
                        generator_error("a generator that failed cannot resume")
                    }))
                }
                GeneratorStatus::Fresh => {
                    if matches!(input, GeneratorInput::Close) {
                        state.body = None;
                        state.status = GeneratorStatus::Done;
                        return Ok(false);
                    }
                    let body = state
                        .body
                        .take()
                        .ok_or_else(|| generator_error("a generator lost its body"))?;
                    let stack = DefaultStack::new(state.stack_bytes).map_err(|error| {
                        generator_error(format!("cannot allocate a generator stack: {error}"))
                    })?;
                    state.bounds = Some(task_stack_bounds(&stack));
                    let yielder_cell = state.yielder.clone();
                    Coroutine::with_stack(stack, move |yielder: &GeneratorYielder, _input| {
                        yielder_cell.0.set(yielder as *const _);
                        body()
                    })
                }
                GeneratorStatus::Suspended => state
                    .coroutine
                    .take()
                    .ok_or_else(|| generator_error("a generator lost its stack"))?,
            }
        };
        let (bounds, yielder) = {
            let mut state = lock(&self.inner);
            state.status = GeneratorStatus::Running;
            (state.bounds, state.yielder.clone())
        };
        loop {
            self.with_hooks(|hooks| hooks.before_resume());
            RUNNING_GENERATORS.with(|running| running.borrow_mut().push(yielder.clone()));
            let bounds_guard = bounds.map(enter_task_stack_bounds);
            let outcome = panic::catch_unwind(AssertUnwindSafe(|| coroutine.resume(input)));
            drop(bounds_guard);
            RUNNING_GENERATORS.with(|running| {
                running.borrow_mut().pop();
            });
            self.with_hooks(|hooks| hooks.after_suspend());
            match outcome {
                Ok(CoroutineResult::Yield(GeneratorSignal::Item(item))) => {
                    let mut state = lock(&self.inner);
                    state.item = Some(item);
                    state.coroutine = Some(coroutine);
                    state.status = GeneratorStatus::Suspended;
                    return Ok(true);
                }
                Ok(CoroutineResult::Yield(GeneratorSignal::Wait(wait))) => {
                    // Perform the wait in the enclosing context, then hand
                    // the answer back to the body.
                    input = GeneratorInput::Woke(super::yield_current_lightweight_task(wait));
                }
                Ok(CoroutineResult::Return(Ok(()))) => {
                    let mut state = lock(&self.inner);
                    state.status = GeneratorStatus::Done;
                    return Ok(false);
                }
                Ok(CoroutineResult::Return(Err(error))) => {
                    let mut state = lock(&self.inner);
                    state.status = GeneratorStatus::Failed;
                    state.failure = Some(error.clone());
                    return Err(error);
                }
                Err(payload) => {
                    let mut state = lock(&self.inner);
                    state.status = GeneratorStatus::Failed;
                    drop(state);
                    panic::resume_unwind(payload);
                }
            }
        }
    }

    fn with_hooks(&self, action: impl FnOnce(&mut dyn GeneratorHooks)) {
        let hooks = lock(&self.inner).hooks.take();
        if let Some(mut hooks) = hooks {
            action(hooks.as_mut());
            lock(&self.inner).hooks = Some(hooks);
        }
    }
}

/// Suspends the running generator body with `item`. Returns true when the
/// body should continue and false when the generator is being closed.
pub fn generator_yield(item: Value) -> Result<bool, Diagnostic> {
    let yielder = RUNNING_GENERATORS
        .with(|running| running.borrow().last().cloned())
        .ok_or_else(|| generator_error("`yield` ran outside a generator body"))?;
    let pointer = yielder.0.get();
    if pointer.is_null() {
        return Err(generator_error("`yield` ran outside a generator body"));
    }
    // SAFETY: the pointer names the yielder of the body running on this
    // stack, which lives as long as that body.
    let yielder = unsafe { &*pointer };
    match yielder.suspend(GeneratorSignal::Item(item)) {
        GeneratorInput::Next => Ok(true),
        GeneratorInput::Close => Ok(false),
        GeneratorInput::Woke(_) => Err(generator_error(
            "a generator resumed with a wake it did not wait for",
        )),
    }
}

/// Hands a scheduler wait from a running generator body to the context that
/// resumed it. Returns `None` when no generator body is running, so the
/// caller suspends the task itself.
// The wait goes back to the caller unchanged, so it is not boxed.
#[allow(clippy::result_large_err)]
pub(super) fn generator_wait(
    wait: TaskYield,
) -> Result<Option<RuntimeSchedulerWakeReason>, TaskYield> {
    let Some(yielder) = RUNNING_GENERATORS.with(|running| running.borrow().last().cloned()) else {
        return Err(wait);
    };
    let pointer = yielder.0.get();
    if pointer.is_null() {
        return Err(wait);
    }
    // SAFETY: as in `generator_yield`.
    let yielder = unsafe { &*pointer };
    match yielder.suspend(GeneratorSignal::Wait(wait)) {
        GeneratorInput::Woke(reason) => Ok(reason),
        GeneratorInput::Next | GeneratorInput::Close => Ok(None),
    }
}

#[cfg(test)]
#[path = "generator_tests.rs"]
mod tests;
