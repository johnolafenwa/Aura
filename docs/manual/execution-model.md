# Execution Model

Aurora source is statically checked, lowered, and executed with deterministic single-expression sequencing plus scheduler-controlled concurrency and external I/O. This chapter defines observable behavior shared by `aura run` and built programs.

## Maintained Execution Paths

Aurora 0.1 maintains one checked source language and two runtime representations:

- `aura run` lowers the entry package to MIR and executes it in the MIR runtime.
- native direct builds lower MIR-compatible program structure to native code linked with the direct runtime.

`aura build --backend direct` requires direct emission and fails if the program cannot be emitted. The default `--backend auto` first attempts direct emission and may fall back to a native launcher containing serialized MIR plus the MIR runtime when direct emission fails. This fallback is a packaging choice, not a third language semantics.

Both runtime representations MUST agree on maintained observable behavior. Backend parity tests compare the eligible runtime fixture corpus.

Duration values remain signed 128-bit nanosecond counts across both paths.
Direct code passes a Duration literal as its exact low and high 64-bit
two's-complement limbs, and the native runtime reconstructs the same i128
value. This ABI transport never narrows through milliseconds or a host timer
type.

## Entry Module Execution

After successful checking, an entry module runs in one of two modes:

1. If it has executable top-level statements, those statements execute in their stored source order. The file cannot also declare a local `main`.
2. Otherwise, a local `main()` is called when present. It returns `None` or `int32`.

An imported function named `main` is not an entrypoint. Imported module top-level statements do not execute as import side effects in Aurora 0.1.

For `aura run`, a returned `int32` is passed to the host process as the requested exit status; `None` means success. A built native program follows the same entry result contract.

## Evaluation Order

Except for short-circuit boolean operators and control-flow constructs, subexpressions are evaluated left-to-right:

- a binary expression evaluates its left operand before its right operand
- collection literal elements are evaluated in source order
- a map evaluates each key before its value and entries in source order
- f-string interpolations are evaluated from left to right
- an index evaluates its base before its index
- a receiver is evaluated before call arguments
- every supplied call or constructor argument is evaluated in call-site source order

Evaluating a copy place captures its copied value at that point. A non-copy
place selected as a binary left operand, index base, method receiver, or
indexed-assignment target remains borrowed until that operation has consumed
all of its inputs. A later shared borrow is permitted, but an overlapping
mutable borrow or consumption is rejected with `AU3002`; the diagnostic points
to both the conflicting access and the retained-borrow origin. This applies to
name roots and projected member places, and no backend may insert a hidden deep
clone. Each f-string interpolation is converted to its rendered `String` at
its own position before the next interpolation begins.

All supplied arguments complete before any omitted default is evaluated. Each
supplied function/method argument or class-field expression is fully evaluated
before the next supplied expression begins. Its copy or move result is captured
in the destination slot; a borrow-mode selection is established without a
clone and remains subject to the retained-borrow overlap rule above. Later side
effects cannot cause an earlier captured argument or field value to be re-read.
Omitted function parameters and class fields then evaluate their defaults in
declaration order when the call or construction occurs. Binding supplied values
to declaration slots never reorders them, and a supplied slot suppresses its
default. Each omission causes a fresh evaluation; a mutable default value is
not a process-global singleton. A shared-borrow parameter's default temporary lives until the call completes. An `own` parameter consumes its fresh default
temporary. Mutable-borrow defaults are statically rejected.

Enum-variant constructor arguments also evaluate in call-site source order.
Named arguments are then bound by name to the variant's declaration-order
payload slots; declaration-slot order never reorders their evaluation.

When two evaluated keys in one map literal compare equal, the later value
replaces the earlier value and the key keeps its first insertion position.

`and` evaluates the right operand only when the left value is `true`. `or` evaluates the right operand only when the left value is `false`. Both operands have static type `bool`.

An assertion evaluates its condition exactly once. A true condition skips the
optional message and falls through. A false condition evaluates the message
exactly once, then establishes the assertion failure before cleanup begins. A
trap in either operand occurs first. Assertion-triggered cleanup follows the
ordinary reverse-order rule, and a cleanup trap does not replace the assertion
diagnostic.

## Calls And Returns

A call evaluates and binds arguments, then transfers control to the target body or runtime builtin. Explicitly owned non-copy arguments have been moved at the call boundary; default-mode non-copy and explicitly borrowed arguments remain owned by the caller and are constrained for the duration of the call.

`return value` evaluates `value`, performs any required borrow-source or move operation, runs active lexical cleanups, and returns to the caller. Reaching the end of a `None` function returns `None`. A non-`None` function cannot pass static checking if a reachable path falls through.

A recursive Aurora call consumes one logical call-depth unit. The maintained runtime rejects execution after 256 nested Aurora calls with a source diagnostic rather than allowing the host stack to overflow.

## Operators

Arithmetic is checked under the selected concrete numeric type.

- integer addition, subtraction, multiplication, floor division, remainder, negation, and casts reject overflow
- builtin integer `/` and `/=` do not reach execution because static checking rejects them
- for integers with nonzero divisor `b`, `q = a // b` is the mathematical quotient rounded toward negative infinity and `r = a % b` satisfies `a == q * b + r`; a nonzero `r` has `b`'s sign
- integer `//` or `%` by zero is a runtime failure; an unrepresentable floor quotient, including the signed minimum divided by `-1`, is integer overflow
- floating `/` is ordinary true division, except that a zero divisor is an explicit runtime failure rather than IEEE infinity or NaN
- floating `//` and `%` use the CPython-compatible divmod correction: start from the host remainder and `(a - remainder) / b`; when a nonzero remainder's sign differs from `b`, add `b` to the remainder and subtract one from the provisional quotient; give a zero remainder `b`'s sign; for a nonzero quotient, take its floor and add one when the provisional quotient minus that floor is greater than `0.5`; preserve the quotient's division-result signed zero when it is zero
- floating `//` and `%` by either signed zero are runtime failures
- ordinary floating operations otherwise use host IEEE-754 `float32`/`float64` behavior, including possible runtime NaN results from operations such as square root of a negative value
- integer `.to_float()` converts to `float64` with IEEE-754 round-to-nearest, ties-to-even and may round; integer `as float32` or `as float64` retains its exactness check and fails instead of rounding
- Duration addition, subtraction, and multiplication operate on signed 128-bit nanoseconds and reject overflow with `AU4002`
- `Duration // int64` returns a Duration whose signed nanosecond count is the mathematical quotient rounded toward negative infinity; a zero divisor is `AU4004` and the signed-minimum divided by `-1` is `AU4002`
- string `+` creates a new concatenated `String`

Trait-backed operators invoke the selected trait implementation method with
ordinary receiver, argument, move, borrow, and runtime-error behavior. `/` may
invoke `Div.div` for an applicable non-numeric user type. `//` and `//=` invoke
`FloorDiv.floor_div` when no builtin numeric or `Duration // int64` rule
applies.

`==` and `!=` perform structural equality for maintained plain values and collections. Resource/handle identity is not a portable substitute for an application identifier; programs should use documented resource data rather than depend on equality of runtime handles.

More precisely:

- numbers, booleans, strings, durations, ranges, enum values, classes, datagrams, and HTTP responses compare by represented value
- vectors compare element-by-element in order
- maps and sets compare by contents and ignore insertion order
- floating equality follows IEEE behavior, so a NaN value is not equal to itself
- queue/task handles, random generators, and live file, process, listener, stream, exchange, supervisor, and WebSocket values compare by shared runtime identity

Equality is defined only after static typing has established compatible operand types.

## Value Rendering

`print`, f-string interpolation, and scalar `.to_string()` use Aurora's maintained value rendering where applicable. Strings render as their contents without quotes and `None` renders as the empty string. A directly printed `float32` or `float64` uses the shortest decimal spelling that round-trips to the same value in its source type. Integral finite values retain a decimal marker, scientific notation is used when it is shorter, and signed zero remains `-0.0`. A Duration renders as an exact decimal millisecond value with an `ms` suffix, using at most six fractional digits and trimming trailing fractional zeros; for example, `2s` renders as `2000ms` and `1ms // 3` renders as `0.333333ms`. This rendering policy is Provisional under ADR-0019.

Vectors render as `[a, b]`, sets as `Set{a, b}`, and maps as `{key: value}` in their maintained insertion order. Class values render as `Class(field=value, ...)`; enum values render as `Enum.Variant(...)`. Nested strings remain unquoted, so this display form is for people and is not a round-trippable serialization format. A deterministic random generator renders exactly `<rng>` without exposing or advancing its state. Live resources render opaque labels such as `<file>` or `<tcp-stream>` rather than host identifiers.

## Assignment And Mutation

A simple assignment ordinarily evaluates the right side before creating or
updating the target. Simple Map indexed assignment is the deliberate exception:
it evaluates and captures its owned key, consuming it when non-copy, before
evaluating and consuming the assigned value. A side effect in the value
therefore cannot retarget the write. Simple Map assignment accepts any `V`.
Reassignment preserves the target's type. A compound assignment selects its
target place once and uses exactly the corresponding binary operator dispatch,
including an applicable user-defined operator trait for a root or projected
target. For a copy target, it captures the current copied value before
evaluating the right operand and stores the operator result into the originally
selected place, so right-side effects neither change the left operand nor
retarget the store. A non-copy root or projected target remains borrowed across
the right operand; overlapping mutable borrow or consumption is rejected with
`AU3002`. Direct indexed compound assignment requires a copy `Vec` element or
`Map` value. A non-copy indexed element is rejected rather than implicitly
cloned or destructively moved, with `AU3006`. A non-copy direct indexed read is
rejected with `AU3005`.

Field and index assignment mutate the selected place. Vector indices are
zero-based. Simple Map assignment replaces an equal existing key or adds a new
entry; an absent key is therefore not a simple-assignment failure. Compound Map
assignment requires an existing key and traps with `AU4003` if its initial read
finds none. Failed checked mutation leaves the operation incomplete and
produces its documented runtime failure or typed error.

Moving a field marks that field unavailable while leaving disjoint fields usable. Reassigning the exact moved place reinitializes it.

## Collections And Iteration

`Vec` preserves element order. `Map` uses insertion order for its `keys()`,
`values()`, `items()`, and `entries()` projections. Replacing an equal Map key,
including from a later literal entry, retains that key's existing slot. `Set`
uses an insertion-oriented runtime representation, but its public order is not
promised. Algorithms should rely on ordering only where the relevant API
promises it.

Bare iteration over a `Vec` or `Set` retains and freezes the selected collection
for the loop and yields shared element access. `own` iteration moves the
collection into a loop-private source once at entry and yields owned elements;
reinitializing the consumed source binding in the body does not switch or
truncate the active iteration. That one-time source selection is accepted
under ADR-0017 and does not alter ADR-0006's ownership modes. Explicit `borrow` and `borrow mut` iteration
retain the collection as allowed by the static rules. Range iteration yields `int32` values from
`start` inclusive to `end` exclusive; its currently accepted modifiers do not
change behavior and remain a tracked language-design follow-up.

Queue iteration receives items until the queue closes, cancellation is observed, registered producers complete cleanly with no more items, or an unread sibling-task failure ends the surrounding group. It is a scheduler operation rather than iteration over a snapshot. Each item arrives already owned by the loop binding; explicit `own`, `borrow`, and `borrow mut` modifiers are rejected because neither the received value nor the copyable Queue handle has a place-iteration ownership mode to modify. Under accepted ADR-0017, the bare form evaluates and copies its Queue handle once at loop entry. This does not freeze the source binding: rebinding that variable in the body is allowed, but later iterations continue receiving through the captured handle. ADR-0006's ownership carve-out is otherwise unchanged.

## Pattern Matching

The scrutinee is evaluated exactly once. Arms are considered in source order. The first matching arm executes.

- by-value match consumes a non-copy scrutinee place when ownership rules require it
- `match borrow` leaves the scrutinee owned and exposes shared payload borrows for non-copy data
- `match borrow mut` permits payload mutation and writes the reconstructed enum value back to the matched mutable place on normal arm exit
- literal patterns compare against the scrutinee value
- `_` always matches and binds nothing

A match expression evaluates only its selected arm and produces that arm's value. Static exhaustiveness ensures a checked match has a selected arm for every permitted input.

## Conditional Expressions

For `value if condition else alternative`, the runtime evaluates `condition`
first and exactly once. A true result evaluates and produces `value`; a false
result evaluates and produces `alternative`. The unselected arm performs no
calls, moves, mutations, I/O, allocation, or runtime failures.

Static checking still analyzes both arms and merges their ownership effects.
This conservative merge prevents later use of a non-copy value that may have
been moved on the selected path. MIR and direct lowering use an explicit
condition branch and a single typed join value; a backend must not eagerly
evaluate either arm. When the surrounding operation takes a shared borrow, the
join does not consume the selected source value, so both source owners remain
available after the borrow ends.

## `try`

`try expression` evaluates one `Result[T, E]` value:

- `Ok(value)` produces `value` and continues the enclosing expression
- `Err(error)` returns immediately from the enclosing function

When the enclosing function uses a different error type, the implementation invokes the applicable `From` trait conversion before returning the error. Active `with` scopes are cleaned up during this early return.

## Resource Lifetime And Cleanup

`with` creates an active cleanup registration after its resource expression succeeds. Leaving the body invokes `close(borrow mut self) -> None` exactly once through that registration.

Cleanup runs on:

- normal fallthrough
- `return`
- `break` or `continue` that exits the scope
- `try` error propagation
- a maintained Aurora runtime failure

Nested active cleanups run in reverse registration order. If a body is already failing and cleanup also fails, the original body diagnostic remains primary. Resource-specific `close()` behavior is defined in its API chapter.

Explicitly closing a resource before scope exit is permitted only where the resource contract makes repeated close harmless; otherwise programs should let the lexical owner perform cleanup.

## Tasks And Scheduler

Aurora lightweight tasks run on one cooperative coroutine scheduler thread per program. Aurora 0.1 does not execute Aurora task bodies in parallel. Operations such as queue waits, task waits, sleep, nonblocking sockets, and scheduler-integrated I/O yield instead of creating one OS thread per Aurora task. The bounded blocking-worker pool may execute host calls concurrently, but those workers do not run Aurora code.

The scheduler is not preemptive and does not inject fuel checks into ordinary loops. A task that keeps executing CPU code without calling `cancelled()` or reaching another scheduler-aware operation can starve every other Aurora task. Each lightweight task reserves a fixed 1 MiB coroutine stack. Readiness discovery scans the waiting-task set and constructs the host `poll` set, so its current cost is linear in the number of waiting tasks/descriptors.

Scheduling order among multiple ready tasks is not specified. Programs coordinate through queues, task results, cancellation, and other documented synchronization rather than timing assumptions.

`Task[T]` and `Queue[T]` are copy handles to shared runtime state. Copying a handle does not duplicate the underlying task or queue.

Starting a task first copies or moves every argument into task-owned capture
storage. The child then applies the target's declared parameter ABI to that
capture: a default non-copy or explicit shared parameter borrows it, and an
`own` parameter consumes it. Mutable-borrow targets are rejected statically.

A task stores its completed result. Repeated result observation clones the stored runtime value. For ordinary copy data this produces another ordinary value. A result containing an exclusive runtime resource is single-observer-only in 0.1; the checker does not yet enforce that restriction, and a second observation can alias the same host resource through shared handles. Repeated observation is supported only for copy data or explicitly shared synchronized handles.

## Task Groups And Failure Observation

`TaskGroup` owns children started within its scope.

- normal scope exit waits for children that are making bounded progress
- a child blocked in an unbounded group-owned wait may be cancelled so cleanup cannot deadlock forever
- explicit `cancel()` signals cancellation and wakes scheduler-aware waits
- a task failure observed through its `Task` result does not also abort the group as unread
- an unread child failure aborts the group scope and wakes queue iteration/waits that depend on that group

Cancellation is cooperative. Pure CPU code observes cancellation at maintained task boundaries/yields, while scheduler-aware blocking operations receive cancellation context directly.

## Host I/O And Cancellation

Socket-backed network resources use nonblocking descriptors and scheduler/poll integration. Their timeout and cancellation outcomes are documented per operation.

Converting a Duration to a host wait is a checked boundary. Negative values,
values outside the host timer range, and durations whose addition to the
current instant would overflow are invalid inputs. Deadline overflow never
silently becomes an unlimited wait. An API with an `io.Error` carrier reports
`InvalidInput`; a process-error carrier reports
`process.Error.Io(io.Error.InvalidInput)`; an API without either typed carrier
traps with `AU4001`. This host-boundary classification is Provisional under
ADR-0019.

Filesystem operations and some host operations run on a bounded blocking worker pool. Cancelling the Aurora task cancels its wait for the worker result; it cannot forcibly stop an operating-system call already executing on a worker. A cancelled write or other side-effecting operation may therefore complete in the host after Aurora has stopped waiting. Programs requiring transactional cancellation must write to a temporary artifact and commit it explicitly.

Process cancellation and close operations signal/terminate according to the process API. Group-enabled processes extend those operations to the maintained host process group behavior.

## Standard Streams

`print` and `io.write` preserve call order within one task. Concurrent writes may interleave at operation boundaries; no global record transaction is implied unless the application serializes output.

`aura run` streams standard output while the program runs. If a later runtime failure occurs, already written output remains observable and the diagnostic is written to standard error. A broken stdout pipe is treated as clean early termination by the CLI.

## Runtime Limits

The maintained resource size, header, frame, timeout, and platform limits are normative for Aurora 0.1 and are collected in [Current Limits](/manual/current-limits). An implementation MUST reject or return a typed error when a limit is exceeded; it must not allocate without bound or hang indefinitely where the API supplies a deadline.

## Determinism

Pure expression evaluation, ordinary control flow, and collection operations are deterministic for the same values. The following are external or scheduler-dependent and therefore not generally deterministic:

- task interleaving among simultaneously ready tasks
- wall/monotonic clock readings
- process identifiers, exit timing, and host scheduling
- network arrival order and peer behavior
- filesystem enumeration supplied by the host
- operating-system secure random output
- the exact wording of host operating-system errors

An explicitly seeded `random.Rng` is deterministic rather than part of that
list: its xoshiro256** sequence, integer/float mapping, and shuffle order are
fixed for Aurora 0.1.x and specified in [Randomness Module](/manual/randomness).
Secure random calls are external effects and never draw from that stream.
Aurora converts host effects into typed values and ordering primitives where
practical, but does not pretend the host environment is deterministic.
