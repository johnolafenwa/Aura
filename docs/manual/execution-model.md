# Execution Model

This chapter defines the runtime behavior that `aura run` and built programs share. Aura checks source statically, lowers it, and runs it. Evaluation within one expression is deterministic. The scheduler controls concurrency, and external I/O depends on the host.

## Maintained Execution Paths

Aura 0.3 has one checked source language and two runtime representations:

- `aura run` lowers the entry package to MIR, Aura's mid-level intermediate representation, and executes it in the MIR runtime.
- A native direct build lowers MIR-compatible program structure to native code linked with the direct runtime.

`aura build` selects the representation with `--backend`:

| Option | Behavior |
| --- | --- |
| `--backend direct` | Requires direct emission. The build fails if the program cannot be emitted. |
| `--backend auto` | The default. Tries direct emission first. If that fails, it may fall back to a native launcher that contains serialized MIR plus the MIR runtime. |

The fallback is a packaging choice. It is not a third language semantics.

Both runtime representations MUST agree on maintained observable behavior. Backend parity tests compare the eligible runtime fixture corpus.

Duration values are signed 128-bit nanosecond counts on both paths. Direct code passes a Duration literal as its exact low and high 64-bit two's-complement limbs. The native runtime reconstructs the same i128 value. This ABI transport never narrows through milliseconds or a host timer type. ABI means application binary interface.

## Entry Module Execution

After checking succeeds, an entry module runs in one of two modes:

1. If it has executable top-level statements, those statements execute in their stored source order. The file cannot also declare a local `main`.
2. Otherwise, a local `main()` is called when present. It returns `None` or `int32`.

An imported function named `main` is not an entrypoint. In Aura 0.3, importing a module does not execute its top-level statements as a side effect.

For `aura run`, a returned `int32` is passed to the host process as the requested exit status. `None` means success. A built native program follows the same entry result contract.

## Evaluation Order

Subexpressions evaluate left to right. Short-circuit boolean operators and control-flow constructs are the exceptions.

- A binary expression evaluates its left operand before its right operand.
- Collection literal elements evaluate in source order.
- A dictionary evaluates each key before its value, and its entries in source order.
- F-string interpolations evaluate from left to right.
- An index evaluates its base before its index.
- A slice evaluates its base, written start, and written end exactly once, from left to right. An omitted endpoint evaluates nothing.
- A receiver evaluates before call arguments.
- Every supplied call or constructor argument evaluates in call-site source order.
- A lambda acquires every capture from left to right when the lambda expression is evaluated. Implicit and `own` captures copy or move. Explicit bare and `mut` entries begin loans before any later sibling expression.
- A comprehension allocates its result first. It then evaluates clause iterables and filters in nested outer-major order, and evaluates its textually leading output last. A dictionary output evaluates its key before its value.

### Retained Borrows

Evaluating a copy place captures its copied value at that point.

A non-copy place stays borrowed until the operation has consumed all of its inputs. This applies when the place is a binary left operand, an index base, a method receiver, or an indexed-assignment target. It applies to name roots and to projected member places.

A later shared borrow is permitted. An overlapping mutable borrow or consumption is rejected with `AU3002`. The diagnostic points to both the conflicting access and the retained-borrow origin. No backend may insert a hidden deep clone.

### F-Strings

Each f-string interpolation is converted to its rendered `str` at its own position, before the next interpolation begins.

Each append preflights the maintained 64 MiB constructed-string limit. An oversized result reports `AU4005` and does not evaluate a later interpolation. The partial output is released through ordinary failure cleanup.

### Slices

A `list` or `str` slice captures its base, then any written start, then any written end. A non-Copy base stays retained while the endpoint expressions run.

After those expressions complete:

1. Negative endpoints are normalized once.
2. Both effective bounds are checked against `0..=len`.
3. The `start <= end` check runs.

A failure traps with `AU4003` and returns no partial value. A successful slice copies the selected range, in source order, into a fresh owned `list` or `str`.

String endpoints count Unicode scalar values, and locating them is O(n). No maintained backend may substitute Python-style endpoint clamping or a view into source storage.

### Arguments And Defaults

All supplied arguments complete before any omitted default is evaluated.

Each supplied function or method argument, and each class-field expression, is fully evaluated before the next supplied expression begins. Its copy or move result is captured in the destination slot. A borrow-mode selection is established without a clone and follows the retained-borrow rule above. A later side effect cannot cause an earlier captured argument or field value to be re-read.

Omitted function parameters and class fields then evaluate their defaults in declaration order, when the call or construction occurs.

- Binding supplied values to declaration slots never reorders them.
- A supplied slot suppresses its default.
- Each omission evaluates the default afresh. A mutable default value is not a process-global singleton.
- A shared-borrow parameter's default temporary lives until the call completes.
- An `own` parameter consumes its fresh default temporary.
- Mutable-borrow defaults are statically rejected.

Enum-variant constructor arguments also evaluate in call-site source order. Named arguments are then bound by name to the variant's payload slots, which are in declaration order. Slot order never reorders their evaluation.

### Dictionary Literal Keys

When two evaluated keys in one dictionary literal compare equal, the later value replaces the earlier value. The key keeps its first insertion position.

### Short-Circuit Operators

`and` evaluates the right operand only when the left value is `true`. `or` evaluates the right operand only when the left value is `false`. Both operands have static type `bool`.

### Lambda Calls

A lambda call evaluates its arguments under its contextual structural function type.

- A read-only or shared-loan closure may be called repeatedly.
- A mutable-loan closure may be called sequentially through a mutable closure place. It writes directly to its captured source.
- A closure whose body consumes a non-Copy owned capture is consumed by the call.

Closure environments release owned values and loan registrations exactly once on both maintained backends, whether or not the closure was called.

### Comparison Chains

A comparison chain evaluates its operand expressions from left to right, each at most once. It evaluates each adjacent link after obtaining that link's right operand. It stops at the first false link and evaluates no remaining operand.

Chains that contain tuple `==` or `!=` follow the same rule. A tuple used as a middle operand is evaluated once and read by both adjacent links.

### Assertions

An [assertion](/manual/assertions) evaluates its condition exactly once. A true condition skips the optional message and falls through. A false condition evaluates the message exactly once, then establishes the assertion failure before cleanup begins.

A trap in either operand occurs first. Assertion-triggered cleanup follows the ordinary reverse-order rule. A cleanup trap does not replace the assertion diagnostic.

## Calls And Returns

A call evaluates and binds its arguments, then transfers control to the target body or runtime builtin. An explicitly owned non-copy argument has already moved at the call boundary. A bare shared argument stays owned by the caller and is constrained for the duration of the call.

`return value` evaluates `value`, copies or moves it into the owned result, runs active lexical cleanups, and returns to the caller. Reaching the end of a `None` function returns `None`. A non-`None` function fails static checking if a reachable path falls through.

`return view [mut] place` hands one checked loan to the caller instead. The declaration's `from` slot fixes the loan's origin. The caller continues the exact selected projection with no unlock and relock, and no clone. Every other callee loan ends before control returns. A trap before the handoff transfers no loan.

Each recursive Aura call consumes one logical call-depth unit. The maintained runtime rejects execution after 256 nested Aura calls with a source diagnostic, so the host stack does not overflow.

## Loan Lifetime And Cleanup

A local view begins a shared or mutable loan over one place and storage generation. Both maintained backends resolve reads and writes through that identity. Mutable writes happen immediately.

Static final-use analysis may end the loan before its lexical scope ends. Branch joins and loop edges extend loan regions conservatively. Every iteration releases its iteration-local loans.

Exit actions form one stack, in reverse acquisition order. The stack holds:

- loan ends
- closure environment drops
- mutable writebacks
- match and iteration reconstruction
- resource cleanup

Normal fallthrough, return, escaping loop control, `try` propagation, maintained traps, and cancellation each drain the required suffix of this stack. So a view into a resource ends before `close`, and an inner reborrow ends before its parent.

A task may keep a loan to its own storage across a scheduler suspension. No descriptor or loan closure may cross to another task or worker.

## Foreign Calls

A direct extern call evaluates its arguments left to right. It marshals them only after ordinary call binding succeeds, then synchronously invokes the matching process-global C symbol. The current Aura worker stays occupied until the foreign function returns.

A missing symbol or a marshalling failure before the call prevents entry. Return validation happens after the foreign function returns. It cannot roll back native side effects or mutable-byte writeback.

Byte views pass as follows:

| Argument | Passed as |
| --- | --- |
| Empty `str`, `list[uint8]`, or `mut list[uint8]` view | A null pointer with length zero. |
| Non-empty shared view | A valid const pointer and the byte length. |
| Non-empty mutable byte view | A same-length scratch buffer. The initial bytes are copied in. After the foreign function returns, exactly that length is copied back, even when later return-value validation produces an Aura runtime error. |

The callee must not retain a pointer, and must not read or write outside the supplied length.

Aura diagnostics do not unwind through a foreign frame. A native abort, signal, memory fault, or foreign unwind is not caught and may terminate the process. [FFI v0](/manual/ffi) has the complete type, ownership, package, and safety contract. FFI means foreign function interface.

## Operators

Arithmetic is checked under the selected concrete numeric type.

### Integer Arithmetic

- Addition, subtraction, multiplication, power, floor division, remainder, checked left shift, negation, and casts reject overflow.
- Builtin integer `/` and `/=` never execute, because static checking rejects them.
- For a nonzero divisor `b`, `q = a // b` is the mathematical quotient rounded toward negative infinity. `r = a % b` satisfies `a == q * b + r`. A nonzero `r` has `b`'s sign.
- Integer `//` or `%` by zero is a runtime failure. An unrepresentable floor quotient, including the signed minimum divided by `-1`, is integer overflow.
- Integer `**` is checked. It defines `x ** 0` as `1`, including `0 ** 0`. A runtime negative exponent is rejected with `AU4001`.
- Ordinary integer scalar and Array arithmetic stays checked. The explicit `wrapping_*` methods use fixed-width modular arithmetic. The `saturating_*` methods clamp to the declared width.

### Floating Arithmetic

- Floating `/` is ordinary true division. A zero divisor is an explicit runtime failure, not IEEE infinity or NaN.
- Floating `//` and `%` by either signed zero are runtime failures.
- Floating `**` follows the maintained floating power domain and overflow classification.
- Other floating operations use host IEEE-754 `float32` and `float64` behavior. That includes possible runtime NaN results, such as the square root of a negative value.

Floating `//` and `%` use the CPython-compatible divmod correction:

1. Start from the host remainder and the provisional quotient `(a - remainder) / b`.
2. When a nonzero remainder's sign differs from `b`, add `b` to the remainder and subtract one from the provisional quotient.
3. Give a zero remainder `b`'s sign.
4. For a nonzero quotient, take its floor. Add one when the provisional quotient minus that floor is greater than `0.5`.
5. When the quotient is zero, keep the signed zero from the division result.

### Numeric Builtins And Conversions

- `divmod(a, b)` computes the same corrected quotient and remainder together, from one evaluation of each operand. A zero divisor is `AU4004`.
- `round(integer)` keeps the exact integer type.
- `round(float)` rounds ties to even and returns `int64`. NaN, infinity, and out-of-range results are classified as `AU4002`.
- Integer `.to_float()` converts to `float64` with IEEE-754 round-to-nearest, ties-to-even, and may round.
- Integer `as float32` or `as float64` keeps its exactness check and fails instead of rounding.

### Bitwise Operators And Shifts

- `&`, `|`, `^`, and `~` operate on the declared fixed-width integer bit representation.
- Every shift count requires `0 <= count < width`.
- Signed `>>` is arithmetic. Unsigned `>>` is logical.
- `<<` is checked. `wrapping_shl` discards high bits. `saturating_shl` clamps at the declared bounds.
- `wrapping_shr` and `saturating_shr` give the same result as ordinary `>>`, after the common count check.

### Duration Arithmetic

- Duration addition, subtraction, and multiplication operate on signed 128-bit nanoseconds. Overflow is rejected with `AU4002`.
- `Duration // int64` returns a Duration whose signed nanosecond count is the mathematical quotient rounded toward negative infinity. A zero divisor is `AU4004`. The signed minimum divided by `-1` is `AU4002`.

### Strings And Arrays

- String `+` creates a new concatenated `str`.
- Numeric Array `+`, `-`, and `*` traverse exact-shape row-major buffers and return fresh owned storage. Float Arrays also support `/`.
- Floating Array reductions visit row-major elements left to right, with deterministic dtype rounding and NaN propagation. `mean` accumulates in `float64`. No reassociation or vectorized reduction order is promised.

Array failures use these codes:

| Code | Cause |
| --- | --- |
| `AU4007` | Rank-zero or negative-dimension construction, `from_list` count mismatch, exact-shape or rank mismatch, and empty reductions. |
| `AU4005` | Shape-product or element-count overflow, and allocation failure. |
| `AU4003` | Direct coordinate and first-axis-slice bounds failures. |

### Trait-Backed Operators

A trait-backed operator invokes the selected trait implementation method. It has the ordinary receiver, argument, move, borrow, and runtime-error behavior of a method call.

- `/` may invoke `Div.div` for an applicable non-numeric user type.
- `//` and `//=` invoke `FloorDiv.floor_div` when no builtin numeric rule and no `Duration // int64` rule applies.

### Equality

`==` and `!=` perform structural equality for maintained plain values and collections.

| Operands | Compared by |
| --- | --- |
| Numbers, booleans, strings, durations, ranges, enum values, classes, datagrams, and HTTP responses | Represented value. |
| Tuples | Corresponding element values from left to right, using ordinary equality and recursing into nested tuples. Comparison stops at the first unequal element. |
| Vectors | Element by element, in order. |
| Maps and sets | Contents. Insertion order is ignored. |
| Floating values | IEEE behavior. A NaN value is not equal to itself. |
| Queue and task handles, random generators, and live file, process, listener, stream, exchange, supervisor, and WebSocket values | Shared runtime identity. |

Resource and handle identity is not a portable substitute for an application identifier. Use documented resource data instead of depending on equality of runtime handles.

Equality is defined only after static typing establishes compatible operand types. Tuple equality requires the same static tuple type. It reads both complete operands and consumes neither, even when the tuples have non-copy elements.

Tuple equality does not compare runtime element-type, transport, or backend metadata carried with a tuple value. That metadata cannot change the recursively determined result. Operand expressions keep their ordinary ownership effects. The equality operation itself moves neither tuple.

Tuple `!=` is the logical negation of tuple `==`. Tuple `<`, `<=`, `>`, and `>=` are static errors. Aura defines no lexicographic or metadata-based tuple ordering.

## Value Rendering

`print`, f-string interpolation, and scalar `.to_string()` use Aura's maintained value rendering where it applies.

| Value | Renders as |
| --- | --- |
| `str` | Its contents, without quotes. |
| `None` | The empty string. |
| Directly printed `float32` or `float64` | The shortest decimal spelling that round-trips to the same value in its source type. An integral finite value keeps a decimal marker. Scientific notation is used when it is shorter. Signed zero stays `-0.0`. |
| Duration | An exact decimal millisecond value with an `ms` suffix. It uses at most six fractional digits and trims trailing fractional zeros. `2s` renders as `2000ms`, and `1ms // 3` renders as `0.333333ms`. |
| List | `[a, b]` |
| Non-empty set | `{a, b}` |
| Empty set | `set()` |
| Dictionary | `{key: value}`, in its defined order. |
| Class value | `Class(field=value, ...)` |
| Enum value | `Enum.Variant(...)` |
| Deterministic random generator | Exactly `<rng>`. Rendering does not expose or advance its state. |
| Live resource | An opaque label such as `<file>` or `<tcp-stream>`, with no host identifiers. |
| FFI opaque handle | `<opaque TypeName>`, using its canonical Aura type name. The foreign pointer address is never shown. |

Nested strings stay unquoted. This display form is for people. It is not a round-trippable serialization format.

## Assignment And Mutation

A simple assignment evaluates the right side before it creates or updates the target. Reassignment preserves the target's type.

Simple dict indexed assignment is the deliberate exception. It evaluates and captures its owned key first, consuming the key when it is non-copy. Then it evaluates and consumes the assigned value. So a side effect in the value cannot retarget the write. Simple dict assignment accepts any `V`.

A compound assignment selects its target place once. It uses exactly the corresponding binary operator dispatch, including an applicable user-defined operator trait for a root or projected target.

- **Copy target.** The current copied value is captured before the right operand is evaluated. The operator result is stored into the originally selected place. Right-side effects neither change the left operand nor retarget the store.
- **Non-copy root or projected target.** The target stays borrowed across the right operand. An overlapping mutable borrow or consumption is rejected with `AU3002`.
- **Indexed target.** Direct indexed compound assignment requires a copy `list` element or `dict` value. A non-copy indexed element is rejected with `AU3006`. It is not implicitly cloned or destructively moved.

A non-copy element used where a place is borrowed is lent in place for the statement. Moving it out of its collection is rejected with `AU3005`.

Field and index assignment mutate the selected place. List indices are zero-based.

Simple dict assignment replaces an equal existing key or adds a new entry. An absent key is not a simple-assignment failure. Compound dict assignment requires an existing key and traps with `AU4003` if its initial read finds none.

A failed checked mutation leaves the operation incomplete. It produces its documented runtime failure or typed error.

Moving a field marks that field unavailable and leaves disjoint fields usable. Reassigning the exact moved place reinitializes it.

## Collections And Iteration

Collection order:

- `list` preserves element order.
- `dict` uses insertion order for its `keys()`, `values()`, and `items()` projections. Replacing an equal key, including from a later literal entry, keeps that key's existing slot.
- `set` uses an insertion-oriented runtime representation, but its public order is not promised.

Rely on ordering only where the relevant API promises it.

`list.map` and `list.filter` traverse from the first element to the last and produce their eager result in that order. Their shared receiver stays unchanged. `map` invokes its callback once per source element, and the new list owns each returned value. `filter` invokes its predicate once per source element and clones accepted elements into the new list.

Natural and keyed `list.sort` calls mutate their receiver into a stable order. Equal elements or keys keep their prior relative order. Keyed sorting evaluates its key function exactly once for every element, from first to last. It stores all keys before it moves any receiver element. So a trap during key evaluation propagates before mutation and leaves the source order unchanged.

Iteration over a `list` or `set` depends on the loop form:

| Form | Behavior |
| --- | --- |
| Bare | Retains and freezes the selected collection for the loop. Yields shared element access. |
| `own` | Moves the collection into a loop-private source once, at entry. Yields owned elements. Reinitializing the consumed source binding in the body does not switch or truncate the active iteration. |
| `mut` | Over a mutable list, binds each element as a mutable element view that writes through at once, and retains the collection. Mutable set iteration is rejected. |

Range iteration yields independent `int64` values from `start` inclusive to `end` exclusive. Explicit `mut` and `own` Range modifiers are rejected with `AU3004`, because there is no place access or ownership transfer to modify. Use the bare form.

Queue iteration receives items until one of these happens:

- the queue closes
- cancellation is observed
- registered producers complete cleanly with no more items
- an unread sibling-task failure ends the surrounding group

Queue iteration is a scheduler operation, not iteration over a snapshot. Each item arrives already owned by the loop binding. Explicit `own` and `mut` modifiers are rejected. Neither the received value nor the copyable Queue handle has a place-iteration ownership mode to modify.

The bare form evaluates and copies its Queue handle once, at loop entry. This does not freeze the source binding. The body may rebind that variable, and later iterations keep receiving through the captured handle.

### Comprehensions

A list, set, or dictionary comprehension creates a fresh empty owned result. It then executes its clauses like nested bare loops.

1. The first source is selected once.
2. For each selected item, filters execute left to right. The first `false` stops that item.
3. Each inner source is selected once for every combination that survives the earlier filters.
4. At an innermost surviving combination, the list or set element, or the dictionary key and value, is evaluated and inserted.

Traversal is outer-major. The complete inner traversal for one outer item finishes before the next outer item begins. The output is written first in source but executes last. A dictionary captures its key before it evaluates its value. Set deduplication and dictionary equal-key replacement follow their literal and storage contracts.

Every clause inherits the bare-loop behavior above:

- List and set sources stay shared and frozen through downstream filters, sources, and output.
- Range targets are copy `int64`.
- `enumerate` and `zip` keep their lockstep rules.
- Queue copies its handle for the clause and yields each received item owned. A Queue comprehension ends only when ordinary Queue iteration ends.

Insertion owns its output. Copy values copy, and owned non-Copy values move. A shared non-Copy list or set element needs an explicit clone-safe clone. Each lambda creation that is reached follows the capture rules in [Evaluation Order](#evaluation-order).

A trap or `try` propagation destroys the partial result and all active temporary sources exactly once. Then the ordinary failure or early-return path continues.

## Pattern Matching

The scrutinee is evaluated exactly once. Arms are considered in source order, and the first matching arm executes.

- `match own` consumes a non-copy scrutinee place.
- Bare `match` leaves the scrutinee owned and exposes shared payload borrows for non-copy data.
- `match mut` permits payload mutation. It writes the reconstructed enum value back on normal arm exit, `return`, `break`, `continue`, and `try` propagation.
- Literal patterns compare against the scrutinee value.
- `_` always matches and binds nothing.

A match expression evaluates only its selected arm and produces that arm's value. Static exhaustiveness ensures a checked match has a selected arm for every permitted input.

## Conditional Expressions

For `value if condition else alternative`, the runtime evaluates `condition` first, exactly once. A true result evaluates and produces `value`. A false result evaluates and produces `alternative`. The unselected arm performs no calls, moves, mutations, I/O, allocation, or runtime failures.

Static checking still analyzes both arms and merges their ownership effects. This conservative merge prevents later use of a non-copy value that may have been moved on the selected path.

MIR and direct lowering use an explicit condition branch and a single typed join value. A backend must not eagerly evaluate either arm. When the surrounding operation takes a shared borrow, the join does not consume the selected source value. Both source owners stay available after the borrow ends.

## `try`

`try expression` evaluates one `Result[T, E]` value:

- `Ok(value)` produces `value` and continues the enclosing expression.
- `Err(error)` returns immediately from the enclosing function.

When the enclosing function uses a different error type, the applicable `From` trait conversion runs before the error is returned. Active `with` scopes are cleaned up during this early return.

## Resource Lifetime And Cleanup

`with` creates an active cleanup registration after its resource expression succeeds. Leaving the body invokes `close(mut self) -> None` exactly once through that registration.

Cleanup runs on:

- normal fallthrough
- `return`
- `break` or `continue` that exits the scope
- `try` error propagation
- a maintained Aura runtime failure

Nested active cleanups run in reverse registration order. If the body is already failing and cleanup also fails, the original body diagnostic stays primary. Each resource's API chapter defines its `close()` behavior.

Closing a resource explicitly before scope exit is permitted only where the resource contract makes a repeated close harmless. Otherwise, let the lexical owner perform cleanup.

These rules apply when Aura control flow or a maintained runtime failure exits the task through the language cleanup machinery. Internal scheduler abandonment is different. It is a last-resort containment path, used when the whole scheduler stops while a child is still suspended, such as after root completion or a fatal reactor failure.

Scheduler abandonment marks the remaining task cancelled and releases scheduler-owned and direct-runtime host state. It does not invoke arbitrary Aura cleanup thunks. A direct generated stack may be reset on this path, because it cannot be safely Rust-unwound across Cranelift frames. Use structured `TaskGroup` scopes. Do not depend on scheduler abandonment as a cleanup mechanism.

## Tasks And Scheduler

### Workers

Aura lightweight tasks run on cooperative scheduler workers, and each task is pinned to one worker. By default, the runtime uses the available parallelism the host reports. The `AURA_WORKERS=<positive integer>` environment override selects an explicit count.

Each child receives a stable worker assignment when it is spawned. Its coroutine stack never migrates, and the runtime does not steal tasks between workers.

Queue waits, task waits, sleep, nonblocking sockets, and scheduler-integrated I/O yield. They do not create one OS thread per Aura task. A task can also yield explicitly with `yield_now()`. The generic blocking-I/O pool may execute host calls concurrently, but its service workers do not run Aura code.

### Ordering And Cross-Worker Data

Scheduling order among multiple ready tasks is not specified. Neither is completion order among independent tasks, or program-output order. Coordinate through queues, task results, cancellation, and other documented synchronization, not timing assumptions.

Aura exposes no worker identity or affinity API. Task execution is multicore. Preemption and work stealing are unavailable, and speedup depends on the workload.

Queue and Task handles are the maintained cross-worker communication surface. Every other capture and result is owned `Transfer` data, which keeps the boundary share-nothing. Cancellation and diagnostic context are installed per task and stay isolated across workers.

### Loop Safepoints

The compiler inserts a cooperative scheduling check on every semantic loop backedge. Reaching the ordinary tail of a `while` or `for` body passes the backedge, and so does `continue`. `break`, `return`, and other exits that leave the loop do not. These checks let a tight loop eventually return to the scheduler, so ready timers, Queue operations, and socket work are not starved indefinitely.

A loop safepoint is not preemption, and it does not inspect cancellation. A single long iteration can still delay every sibling pinned to the same worker until the body reaches its backedge. Long straight-line CPU work with no loop or scheduler operation can do the same.

- Use `cancelled()` when the task must observe cancellation.
- Use `yield_now()` when the program needs an explicit scheduling point between chosen chunks.

Neither automatic nor explicit yielding specifies which ready local task runs next.

MIR execution amortizes the cooperative yield with 8 units of function-local loop fuel. Direct native code uses 4,096 units and replenishes the fuel after yielding. A program proven to have no possible sibling Aura task elides the runtime check entirely. These backend strategies may produce different valid interleavings. Scheduling order is not observable language order.

### `yield_now()`

`yield_now()` places the current lightweight task back in its worker's ready set. It returns when that worker selects the task again. Other runnable local tasks get a chance to proceed without waiting for an event or deadline.

`yield_now()` does not migrate the coroutine or steal work. It does not guarantee that a different task runs, and it does not specify a ready-task order. With no current schedulable lightweight task, it returns without effect.

### Task Stacks

An ordinary lightweight task requests 768 KiB of writable coroutine stack. The `TaskGroup.start_with_stack` and `start_soon_with_stack` methods accept an exact `int64` request from 256 KiB through 64 MiB inclusive.

Accepted requests are rounded up to the host page size and guard-protected. Out-of-range requests are rejected, not clamped. This surface is provisional.

The 256 KiB lower bound is an explicit minimum for measured shallow tasks. It is not a general default. The complete compiled Aura HTTP example, including its MIR and direct language-execution frames, was unsafe with a 256 KiB default task stack and succeeded with 512 KiB. The default is 768 KiB.

A separate, isolated runtime round trip forces protocol callers to 256 KiB. It shows that service workers own the deep host protocol frames. It does not measure the full compiled task stack.

### Event Reactor

The scheduler owns a persistent event reactor:

- Nonblocking descriptors stay registered across scheduler turns.
- Deadlines are ordered in a timer heap.
- Queue, task-completion, and blocking-pool events notify the responsible ready queue directly, including across workers.

Registration uses a check-subscribe-recheck protocol with wait epochs. A readiness edge that races with suspension is not lost, and a stale wakeup does not resume a later wait. If no task is ready, the scheduler blocks until the next event or deadline. There is no periodic park tick.

### `select`

`select(source, ...)` evaluates its Queue, Task, and relative-Duration sources once, from left to right. It then uses one composite wait under the same protocol.

Current-task cancellation wins. Otherwise, each arbitration probes sources by their original zero-based index and commits the first ready source. A wake is only a request to arbitrate. So if another consumer takes a Queue item before the selecting task resumes, `select` does not produce a false outcome.

The winner commit consumes at most one Queue item or selected Task result, and it removes every losing registration. All Duration sources share one base instant, established after evaluation and validation.

### `control.retry`

`control.retry` invokes its worker immediately for the first attempt. An `Ok` returns immediately. Every `Err` is retryable.

An `Err` is kept only until the helper knows whether another attempt exists. When one does, the helper waits for the current backoff unless it is zero, then invokes the next attempt. It doubles the delay only when another retry could still use it. The final permitted `Err` is returned exactly, with no extra sleep or multiplication.

Worker traps and checked Duration overflow propagate as runtime diagnostics. Cancellation of the current task propagates through the helper and its scheduler-aware delay. It is not represented as the most recent `Err`.

### Starting Tasks

Starting a task first copies or moves every argument into task-owned capture storage. The child then applies the target's declared parameter capability to that capture. A bare parameter borrows it, and an `own` parameter consumes it. Mutable targets are rejected statically.

Task captures, results, and Queue payloads must also pass the structural `Transfer` check before the child is admitted to its spawn-time pinned worker.

Starting a child from a running task does not mutate the live scheduler through an alias. The runtime first prepares the child's guarded stack and task state. Then it transfers that prepared request to the scheduler for admission. If preparation fails, the start fails synchronously before a handle is returned, and no child is admitted.

A task may wait on a successfully returned child handle immediately, including inside a nested `TaskGroup`. The admission broker keeps its own FIFO request order, but that is an internal safety property. Child execution order stays unspecified.

### Task Handles And Results

`Queue[T]` is a copy handle to shared runtime state. A `Task[T]` handle is copyable only when its result is repeatable. Every task handle is transferable. Copying an allowed handle does not duplicate the underlying task or queue.

A task stores its completed result.

- Copy results, Queue handles, and recursively repeatable Task handles permit repeated observation.
- Every other transferable result has a unique observation right. Each direct result call consumes it on every outcome.
- Multi-task waits consume the complete task list. `wait_any` abandons the unchosen rights.

Queue and Task handle state is synchronized for cross-worker notification and observation. Every other value that crosses the boundary stays owned and share-nothing.

The runtime also protects a non-repeatable stored result with an atomic one-winner claim. A failed second claim traps with `AU4001` and `task result has already been observed; non-repeatable task results allow exactly one observing attempt`. This is defense in depth against backend defects or foreign handles. It does not replace static ownership diagnostics.

`TaskGroup` scope cleanup joins, abandons, or accounts for a child without observing its successful result. Cleanup does not claim the observation right or make the value available to another observer.

### Service Pools

The runtime has three process-global service pools besides the scheduler workers.

**Protocol-step service.** Deep HTTP parsing and construction, TLS operations, and maintained Unix WebSocket protocol steps run on this distinct, bounded service. Its two named workers have 2 MiB native stacks and share a 64-job queue.

A job owns its protocol state for one bounded, nonblocking library step. The coroutine waits for the state to return before it observes cancellation or waits for descriptor readiness again. So a protocol state is never abandoned with two owners, and no resource mutex stays held across the worker wait. Reactor readiness, absolute deadlines, and cancellation stay scheduler-side concerns.

The protocol-step pool is lazily initialized and shared by every lightweight scheduler. Its workers intentionally live until process exit. Aura 0.3 has no protocol-pool shutdown or join surface. The non-Unix WebSocket fallback keeps its compatibility path.

**Generic blocking-I/O pool.** Resolver, listener-bind, and file reads use this pool. TLS asset bytes are read here before PEM parsing and rustls construction run on protocol workers. PEM is the text encoding used for certificates and keys.

| Variable | Effect |
| --- | --- |
| `AURA_BLOCKING_WORKERS=<positive integer>` | Selects the exact worker count, without clamping. When unset, the pool uses host parallelism, with fallback `4` and a derived `2..=8` clamp. |
| `AURA_BLOCKING_QUEUE_CAPACITY=<positive integer>` | Optionally bounds accepted pending jobs. Capacity excludes running jobs and callers waiting for admission. When unset, the queue is unbounded. |

The first runtime preflight reads these settings once. They stay fixed for the process lifetime. That preflight starts no worker. The first blocking submission creates the complete configured set of workers, which production reuses until process exit. There is no Aura shutdown or join surface.

**JSON parse service.** Dynamic `json.parse` uses a third, independent service with two 2 MiB-stack workers and a total in-flight capacity of two.

A task reserves capacity before it makes the fallible owned copy of its parse source. When the service is saturated, a lightweight task parks through the scheduler instead of spinning. Once admitted, synchronous `json.parse` waits through codec completion. Cancellation is observed at the task's next ordinary cancellation boundary. The codec job is not abandoned.

The dependency-owned recursive parser runs on the service stack. Runtime materialization, JSON-aware cloning and rendering, and dump conversion and emission use iterative traversals. The direct backend waits for admission without value-table access. It then holds read access only long enough to copy the source, and releases it before submission and completion waiting.

The bounded `json.is_valid` and `json.parse_string_map` operations stay caller-side and do not use this service. Codec workers are process-lifetime. Aura 0.3 has no shutdown or configuration surface for them.

## Task Groups And Failure Observation

`TaskGroup` owns the children started within its scope.

- Normal scope exit waits for children that are making bounded progress.
- A child blocked in an unbounded group-owned wait is cancelled only when the runtime's live wait graph has no reachable waker.
- Explicit `cancel()` signals cancellation and wakes scheduler-aware waits.
- A task failure observed through its `Task` result does not also abort the group as unread.
- An unread child failure aborts the group scope. It wakes Queue iteration and waits that depend on that group.

Cancellation is cooperative. Pure CPU code observes cancellation through `cancelled()`. `yield_now()` is a scheduling point but does not inspect cancellation, and neither do compiler-inserted loop safepoints. Scheduler-aware blocking operations receive cancellation context directly.

Queue reachability is based on live tasks known to hold `Queue` handles. It does not use an elapsed-time threshold.

- A sender parked on a full open queue stays joinable while a live receiver can drain it.
- A receiver parked on an empty queue stays joinable while a live sender, or another live owner of the open queue, can send or close it.
- The task performing the join does not count as its own child's waker, because it cannot use its queue handle until the join returns.
- Cycles made only of mutually blocked waits have no reachable waker and are cancelled.

## Host I/O And Cancellation

Socket-backed network resources use nonblocking descriptors with persistent reactor registration. Each operation documents its timeout and cancellation outcomes.

Converting a Duration to a host wait is a checked boundary. These inputs are invalid:

- negative values
- values outside the host timer range
- durations whose addition to the current instant would overflow

Deadline overflow never silently becomes an unlimited wait. The error depends on the API's typed carrier:

| API | Result |
| --- | --- |
| Has an `io.Error` carrier | `InvalidInput` |
| Has a process-error carrier | `process.Error.Io(io.Error.InvalidInput)` |
| Has neither typed carrier | Traps with `AU4001` |

Filesystem operations and some host operations run on the generic blocking-I/O pool. When its optional pending-queue bound is full, Aura tasks wait for admission through the scheduler in FIFO order. They do not block a pinned worker.

Cancellation or deadline expiry before queue insertion prevents the operation from being submitted. Once inserted, the operation cannot be retracted. Cancelling the Aura task cancels its wait, not an operating-system call that is already pending or executing. So a cancelled write or other side-effecting operation may complete in the host after Aura stops waiting, and its late result is discarded. A program that needs transactional cancellation must write to a temporary artifact and commit it explicitly.

Bounding accepted pending jobs does not bound admission waiters. It does not guarantee progress for unrelated blocking I/O while every configured worker stays stuck.

Process cancellation and close operations signal or terminate according to the process API. Group-enabled processes extend those operations to the maintained host process group behavior.

## Standard Streams

`print` and `io.write` preserve call order within one task. Concurrent writes may interleave at operation boundaries. No global record transaction is implied unless the application serializes output.

`aura run` streams standard output while the program runs. If a runtime failure occurs later, output already written stays observable, and the diagnostic is written to standard error. The CLI treats a broken stdout pipe as clean early termination.

## Runtime Limits

The maintained resource size, header, frame, timeout, and platform limits are normative for Aura 0.3. [Current Limits](/manual/current-limits) collects them. An implementation MUST reject or return a typed error when a limit is exceeded. It must not allocate without bound, and it must not hang indefinitely where the API supplies a deadline.

## Determinism

Pure expression evaluation, ordinary control flow, and collection operations are deterministic for the same values. The following are external or scheduler-dependent, so they are not generally deterministic:

- task interleaving among simultaneously ready tasks
- wall and monotonic clock readings
- process identifiers, exit timing, and host scheduling
- network arrival order and peer behavior
- filesystem enumeration supplied by the host
- operating-system secure random output
- the exact wording of host operating-system errors

An explicitly seeded `random.Rng` is deterministic. Its xoshiro256** sequence, integer and float mapping, and shuffle order are fixed for Aura 0.3.x. [Randomness Module](/manual/randomness) specifies them. Secure random calls are external effects and never draw from that stream.

Aura converts host effects into typed values and ordering primitives where practical. It does not pretend the host environment is deterministic.

Design record: `architecture_docs/decisions/0006-parameter-and-loop-ownership-defaults.md` and `architecture_docs/decisions/0017-iteration-source-selection.md` for iteration ownership, `architecture_docs/decisions/0019-duration-conversion-and-timer-policy.md` for Duration rendering and host waits, `architecture_docs/decisions/0032-guarded-lightweight-task-stacks.md` for task stacks, `architecture_docs/decisions/0033-structural-transfer-and-task-results.md` for task handles and results, `architecture_docs/decisions/0035-configurable-blocking-io-pool.md` for the blocking-I/O pool, and `architecture_docs/decisions/0037-expression-closures-and-value-capture.md` for lambda capture.
