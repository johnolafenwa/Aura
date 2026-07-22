# Process Module

The `process` module runs child processes without a shell by default. Commands are explicit `Vec[String]` argv values. That means `["/bin/echo", "hello world"]` runs exactly one executable with one argument; Aurora does not split strings or expand shell syntax.

```python
import process
```

Use `process.run(...)` for "start, wait, collect" workflows. Use `process.start(...)` when the parent needs pipes or a long-running child handle. Use `process.supervisor()` when the parent owns a named set of child processes and wants restart/event behavior.

## Stdio Configuration

| API | Signature | Contract |
| --- | --- | --- |
| `process.inherit` | `inherit() -> process.Stdio` | Connects the child stream to the parent process stream. |
| `process.null` | `null() -> process.Stdio` | Connects the child stream to the null device. |
| `process.pipe` | `pipe() -> process.Stdio` | Creates a pipe that can be captured or accessed through `process.Pipe`. |

`process.Stdio` variants:

| Variant | Meaning |
| --- | --- |
| `process.Stdio.Inherit` | Use the parent stream. |
| `process.Stdio.Null` | Discard output or provide EOF input. |
| `process.Stdio.Pipe` | Create a pipe. |

Prefer the functions (`process.pipe()`, `process.null()`, `process.inherit()`) in normal code.

## process.run

Signature: `process.run(command: Vec[String], cwd: Option[String] = None, env: Map[String, String] = {}, stdin: process.Stdio = process.null(), stdout: process.Stdio = process.pipe(), stderr: process.Stdio = process.pipe(), timeout: Duration = ..., group: bool = false) -> Result[process.Completed, process.Error]`

`process.run(...)` starts a child, waits for it, and returns a `process.Completed` value. By default, stdin is null and stdout/stderr are captured. Omitting `timeout` uses an internal absence marker and supplies no caller deadline. No Duration value is that marker: an explicit negative timeout is invalid rather than unlimited.

The `env` map augments the inherited host environment and replaces inherited values with matching names. Aurora never invokes a shell for `run` or `start`. Capture occurs only for streams configured with `process.pipe()` and each captured stream is capped at 64 MiB.

```python
def run_echo() -> Result[None, process.Error]:
    command = ["/bin/echo", "aurora"]
    completed = try process.run(command, stdout=process.pipe(), stderr=process.pipe(), timeout=1s)

    try completed.check()
    print(completed.stdout().trim())
    return Result.Ok(None)
```

Set `group=true` when the child may spawn descendants and the parent should clean up the whole process group on maintained Unix hosts.

## process.start

Signature: `process.start(command: Vec[String], cwd: Option[String] = None, env: Map[String, String] = {}, stdin: process.Stdio = process.null(), stdout: process.Stdio = process.inherit(), stderr: process.Stdio = process.inherit(), group: bool = false) -> Result[process.Child, process.Error]`

`process.start(...)` returns a live `process.Child`. The default is interactive-friendly: stdout and stderr inherit the parent's streams unless you ask for pipes.

```python
def start_cat() -> Result[process.Child, process.Error]:
    command = ["/bin/cat"]
    child = try process.start(command, stdin=process.pipe(), stdout=process.pipe(), stderr=process.pipe(), group=true)
    return Result.Ok(child)
```

The caller is responsible for waiting, killing, terminating, or closing the child.

## process.Child

| API | Signature | Contract |
| --- | --- | --- |
| `stdin` | `stdin() -> Option[process.Pipe]` | Returns the child's piped stdin when `stdin=process.pipe()` was used. |
| `stdout` | `stdout() -> Option[process.Pipe]` | Returns the child's piped stdout when `stdout=process.pipe()` was used. |
| `stderr` | `stderr() -> Option[process.Pipe]` | Returns the child's piped stderr when `stderr=process.pipe()` was used. |
| `wait` | `wait(timeout: Duration = ...) -> process.Wait` | Waits for exit and returns an exit, timeout, cancellation, or failure outcome. |
| `wait_or_none` | `wait_or_none(timeout: Duration = ...) -> Result[Option[process.ExitStatus], process.Error]` | Returns `Ok(Some(status))` on exit, `Ok(None)` on timeout, and `Err(...)` for cancellation or wait failure. |
| `wait_ok` | `wait_ok(timeout: Duration = ...) -> Result[process.ExitStatus, process.Error]` | Returns the exit status only for successful exits; non-zero status and wait failures become `process.Error`. |
| `kill` | `kill() -> Result[None, process.Error]` | Kills the child immediately. With `group=true`, targets the process group on maintained Unix hosts. |
| `terminate` | `terminate() -> Result[None, process.Error]` | Requests graceful termination. With `group=true`, targets the process group on maintained Unix hosts. |
| `close` | `close() -> None` | Closes the child resource, terminating it if still running. |

`process.Wait` variants:

| Variant | Meaning |
| --- | --- |
| `Exited(status: own process.ExitStatus)` | The child exited or was signaled. |
| `TimedOut` | The wait timeout expired. |
| `Cancelled` | Cancellation interrupted the wait. |
| `Failed(error: own process.Error)` | Waiting failed. |

`process.ExitStatus` variants:

| Variant | Meaning |
| --- | --- |
| `Exited(code: own int32)` | The process exited with a numeric code. |
| `Signaled(signal: own int32)` | The process was terminated by a signal on platforms that expose signal status. |

## process.Pipe

| API | Signature | Contract |
| --- | --- | --- |
| `read_all` | `read_all() -> Result[String, process.Error]` | Reads remaining strict UTF-8 text until EOF, capped at 64 MiB. Use byte APIs for arbitrary output. |
| `read_line` | `read_line(timeout: Duration = ...) -> Result[Option[String], process.Error]` | Reads one strict UTF-8 line without its trailing LF/CRLF, `Ok(None)` only on EOF, or an error. |
| `read_bytes` | `read_bytes(max_bytes: int32, timeout: Duration = ...) -> Result[Option[Vec[uint8]], process.Error]` | Reads up to `max_bytes` raw bytes and returns `Ok(None)` only at EOF. `max_bytes` must be in `1..=67108864`. |
| `write_all` | `write_all(text: String, timeout: Duration = ...) -> Result[None, process.Error]` | Writes all text. |
| `write_bytes` | `write_bytes(bytes: Vec[uint8], timeout: Duration = ...) -> Result[None, process.Error]` | Writes all bytes. |
| `flush` | `flush() -> Result[None, process.Error]` | Flushes buffered pipe output. |
| `close` | `close() -> None` | Closes the pipe handle. |

A pipe deadline expires as `Err(process.Error.TimedOut)`; cancellation becomes `Err(process.Error.Cancelled)`. Neither outcome is reported as `Ok(None)`. `read_bytes(0, ...)` and requests above 64 MiB return `process.Error.Io(io.Error.InvalidInput)`.

Close a child's stdin pipe when the child expects EOF:

```python
def close_stdin(child: process.Child) -> Result[None, process.Error]:
    match child.stdin():
        case Option.Some(pipe):
            try pipe.write_all("hello\n")
            pipe.close()
        case Option.None:
            pass
    return Result.Ok(None)
```

## process.Completed

`process.Completed` is returned by `process.run(...)`.

| API | Signature | Contract |
| --- | --- | --- |
| `status` | `status() -> process.ExitStatus` | Returns the captured exit status. |
| `success` | `success() -> bool` | Returns `true` when the status is exit code `0`. |
| `stdout` | `stdout() -> String` | Returns captured stdout decoded as strict UTF-8. Invalid UTF-8 raises a runtime diagnostic; use `stdout_bytes` for untrusted output. |
| `stdout_bytes` | `stdout_bytes() -> Vec[uint8]` | Returns captured stdout as raw bytes. |
| `stderr` | `stderr() -> String` | Returns captured stderr decoded as strict UTF-8. Invalid UTF-8 raises a runtime diagnostic; use `stderr_bytes` for untrusted output. |
| `stderr_bytes` | `stderr_bytes() -> Vec[uint8]` | Returns captured stderr as raw bytes. |
| `check` | `check() -> Result[None, process.Error]` | Returns `Ok(None)` for successful exit status, otherwise `Err(...)`. |

Use `check` when a command failure should stop the current `Result`-returning function:

```python
def must_succeed() -> Result[None, process.Error]:
    completed = try process.run(["/bin/false"], timeout=1s)
    try completed.check()
    return Result.Ok(None)
```

Use byte methods for tools that may emit binary or non-UTF-8 output.

## process.supervisor

```python
process.supervisor() -> process.Supervisor
```

A supervisor is a resource that owns named child process specs and emits lifecycle events. Bind it with `with` whenever possible:

```python
def wait_for_worker() -> Result[process.SupervisorWait, process.Error]:
    with supervisor = process.supervisor():
        try supervisor.start(name="worker", command=["/bin/sleep", "1"])
        return Result.Ok(supervisor.wait(timeout=2s))
```

## process.Supervisor

| API | Signature | Contract |
| --- | --- | --- |
| `start` | `start(name: own String, command: own Vec[String], cwd: own Option[String] = ..., env: own Map[String, String] = ..., stdin: own process.Stdio = ..., stdout: own process.Stdio = ..., stderr: own process.Stdio = ..., restart: own process.RestartPolicy = ..., backoff: own Duration = ..., max_restarts: own int32 = ..., group: own bool = ...) -> Result[None, process.Error]` | Starts a named child under supervision and retains the owned configuration needed for restarts. Names must be unique within the supervisor. |
| `wait` | `wait(timeout: Duration = ...) -> process.SupervisorWait` | Waits for the next supervisor event, timeout, or cancellation. |
| `wait_or_none` | `wait_or_none(timeout: Duration = ...) -> Result[Option[process.SupervisorEvent], process.Error]` | Returns `Ok(Some(event))`, `Ok(None)` on timeout, or `Err(...)` on cancellation or wait failure. |
| `stop` | `stop() -> Result[None, process.Error]` | Stops every supervised child and clears the supervisor. |
| `is_empty` | `is_empty() -> bool` | Returns `true` when no services are running or pending restart. |
| `close` | `close() -> None` | Closes the supervisor, stopping all managed children. |

Runtime defaults for `Supervisor.start(...)` are:

| Parameter | Default |
| --- | --- |
| `cwd` | `None` |
| `env` | empty map |
| `stdin` | `process.null()` |
| `stdout` | `process.inherit()` |
| `stderr` | `process.inherit()` |
| `restart` | `process.RestartPolicy.OnFailure` |
| `backoff` | `100ms` |
| `max_restarts` | unlimited when omitted; `-1` is accepted as unlimited |
| `group` | `true` |

When restart is enabled, `backoff` must be at least `10ms`.

`process.RestartPolicy` variants:

| Variant | Meaning |
| --- | --- |
| `Never` | Do not restart. |
| `OnFailure` | Restart only when the child exits unsuccessfully. |
| `Always` | Restart after every exit while restart limits allow it. |

`process.SupervisorEvent` variants:

| Variant | Meaning |
| --- | --- |
| `Exited(name: own String, status: own process.ExitStatus, restart_count: own int32)` | A child exited and was not restarted. |
| `Restarted(name: own String, status: own process.ExitStatus, restart_count: own int32)` | A child exited and a replacement was started. |
| `Failed(name: own String, error: own process.Error, restart_count: own int32)` | A child failed to start or restart. |

`process.SupervisorWait` variants:

| Variant | Meaning |
| --- | --- |
| `Event(event: own process.SupervisorEvent)` | A supervisor event is available. |
| `TimedOut` | No event arrived before timeout. |
| `Cancelled` | Cancellation interrupted the wait. |

An invalid `Supervisor.wait` timer cannot be returned directly as
`process.Error` because `wait` returns `process.SupervisorWait`. It maps
exactly to
`process.SupervisorWait.Event(process.SupervisorEvent.Failed("<supervisor>", process.Error.Io(io.Error.InvalidInput), 0))`:
the synthetic name is `<supervisor>` and the synthetic restart count is zero.
The `wait_or_none` return type has an error carrier, so the same invalid timer
returns `Result.Err(process.Error.Io(io.Error.InvalidInput))` instead.

## process.Error

| Variant | Meaning |
| --- | --- |
| `NoCommand` | The command vector was empty. |
| `TimedOut` | A process operation timed out. |
| `Cancelled` | Cancellation interrupted the operation. |
| `Io(error: own io.Error)` | The operation failed with an I/O error. |
| `Spawn(message: own String)` | The child could not be spawned. |
| `Other(message: own String)` | A process-specific failure not covered by another variant. |

## Cleanup Rules

Child, pipe, and supervisor values are resources. Prefer `with` for supervisors and call `close()` on children and pipes when ownership is not scoped.

For child processes, `close()` terminates a still-running child. With `group=true`, cleanup targets the process group on maintained Unix hosts.

When `process.run` times out or its Aurora task is cancelled, the runtime terminates the child and waits for cleanup; with `group=true` it applies that policy to the process group on maintained Unix hosts. As with all host I/O, cancellation cannot retroactively undo side effects already performed by the child.

## Grammar

The process module adds no source-language grammar. Commands are ordinary `Vec[String]` expressions passed to ordinary calls; Aurora does not parse shell syntax, split one command string, expand variables, interpret redirections, or construct pipelines. Named arguments, `Duration` literals, `Result`, `Option`, `try`, `match`, and `with` use their general grammar.

An omitted parameter displayed with `= ...` selects the documented builtin default. The ellipsis is reference notation, not a source expression. Process and standard-I/O variants use ordinary qualified enum construction and pattern syntax.

For `process.run`, the omitted timeout is represented internally rather than
by a sentinel Duration. Explicit zero is a real immediate deadline, and an
explicit negative value is invalid input. This distinction is Provisional
under ADR-0019.

## Typing Rules

The function and method signatures above are normative. Commands are `Vec[String]`, environment overlays are `Map[String, String]`, working directories are `Option[String]`, and timeout parameters are `Duration`. Fallible start/run/pipe/control operations use `process.Error`; wait APIs deliberately distinguish enum, `Option`, and `Result` outcomes as shown in their tables.

`process.Child`, `process.Pipe`, and `process.Supervisor` are non-copy resources. Kill, terminate, pipe write/flush/close, and supervisor start/stop/close operations require mutable receiver places. `Supervisor.start` consumes every configuration argument marked `own`, because the supervisor retains that configuration for possible restart. `Completed.stdout()` and `stderr()` are trapping text accessors; the byte accessors are total over captured bytes.

## Runtime Semantics

`run` and `start` invoke exactly the executable and argument vector supplied, inherit the host environment, then apply `env` entries as replacements or additions. `run` waits and captures only streams configured as pipes. `start` returns immediately with a live child and any configured pipe endpoints. Repeated child pipe accessors return handles to the same underlying endpoint, so cursor state and close state are shared.

`Child.wait` reports exit, timeout, cancellation, or failure without automatically terminating a still-live child. By contrast, timeout or cancellation of `process.run` terminates the child and waits for cleanup. A negative, host-unrepresentable, or deadline-overflowing timeout/backoff is `process.Error.Io(io.Error.InvalidInput)` wherever the declared process outcome can carry that error; deadline overflow never becomes an unlimited wait. `Completed.check` converts a non-success status into `process.Error`; invalid captured UTF-8 in `stdout()` or `stderr()` is a runtime diagnostic, while the byte accessors return the original bytes. Supervisor restarts, counts, events, defaults, and minimum backoff follow the tables above.

## Ownership And Evaluation Order

Arguments are evaluated left to right before process creation. `run` and `start` share their Aurora arguments for the call and copy the required command, environment, and path data into host process state; they do not retain Aurora borrows after returning. A supervisor takes ownership of retained configuration. Child, pipe, completed-output, status, error, and event values returned from an operation are owned by the caller.

Moving a resource invalidates the source binding. `with` closes a supervisor on every scope exit; explicit child and pipe `close()` operations close shared handle state, and child close terminates a process still running. Cleanup is ordered after body evaluation but cannot undo child filesystem, network, or other external side effects already performed.

## Diagnostics

Unknown process members use `AU2001`, type mismatches use `AU2002`, invalid argument binding uses `AU2004`, and remaining static rejections use `AU2999`. Use after moving a process resource uses `AU3001`, borrow conflicts use `AU3002`, and calling a mutating method through an immutable place uses `AU3003`.

Empty commands, spawn failures, timeouts, cancellation, invalid byte counts,
invalid timeout/backoff values or deadlines, closed pipes, non-zero status
checked through `check`, and ordinary host I/O failures are typed
`process.Error` values. Invalid timer inputs use
`process.Error.Io(io.Error.InvalidInput)`. Decoding invalid captured bytes
through `Completed.stdout()` or `stderr()` is deliberately a runtime trap with
code `AU4005`; use `stdout_bytes()` or `stderr_bytes()` when output encoding is
not guaranteed.

## Backend Support

Process creation, capture, pipes, waiting, supervisor behavior, typed errors, and cleanup are implemented in the MIR runtime and direct native backend. Command-vector handling, environment overlay, capture bytes, timeout outcomes, and ownership are backend-parity requirements.

Process-group creation and signaling are maintained on Unix hosts. On unsupported hosts, requesting group behavior returns a typed process error rather than silently weakening cleanup. Executable lookup, signals, and exit-status details otherwise follow host process facilities.

## Limits And Implementation-Defined Behavior

Each `process.run` captured stream and each whole-pipe read is capped at 64 MiB; bounded pipe byte reads accept `1..=67108864`. This stream ceiling is independent of the larger filesystem whole-read limit. Text access is strict UTF-8. Supervisor restart backoff must be at least 10 ms when restart is enabled; omitted or `-1` maximum restarts means unlimited. There is no shell, command-string parser, pipeline builder, pseudo-terminal API, daemon manager, sandbox, resource-limit API, or portable signal-number abstraction.

Executable discovery, path syntax, inherited environment, signal availability, numeric exit behavior, graceful-termination meaning, scheduling, and side effects are host-dependent. Timeouts and cancellation bound Aurora's wait but cannot retract child actions that already occurred. Group cleanup of descendants is a maintained Unix contract, not a portable guarantee for every host process tree.

## Status

One-shot execution, live children, standard-I/O configuration, pipes, completed output, status checking, supervisor restart/event behavior, typed failures, and Unix process-group cleanup are implemented and maintained in Aurora 0.1. The fixed stream-cap policy recorded by ADR-0018 remains Provisional pending the Batch 2 checkpoint review, and the omitted-timeout and invalid host-timer policy recorded by ADR-0019 remains Provisional pending the Phase 3 checkpoint review; no other process semantics on this page are provisional.

Shell evaluation, pipelines, pseudo-terminals, Windows process groups, portable signal control, sandboxing, and operating-system service management are unavailable. They are future, non-normative facilities rather than implicit behavior of the current API.
