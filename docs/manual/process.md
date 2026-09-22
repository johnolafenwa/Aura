# Process Module

The `process` module runs child processes. By default it uses no shell. A command is an explicit `list[str]` argument vector, or argv. So `["/bin/echo", "hello world"]` runs one executable with one argument. Aura does not split strings or expand shell syntax.

```aura
import process
```

Pick the entry point by what the parent needs:

- `process.run(...)` starts a child, waits for it, and collects its output.
- `process.start(...)` returns a live child handle, for pipes or a long-running child.
- `process.supervisor()` owns a named set of children, with restarts and lifecycle events.

## Stdio Configuration

| API | Signature | Contract |
| --- | --- | --- |
| `process.inherit` | `inherit() -> process.Stdio` | Connects the child stream to the parent's stream. |
| `process.null` | `null() -> process.Stdio` | Connects the child stream to the null device. |
| `process.pipe` | `pipe() -> process.Stdio` | Creates a pipe that the parent can capture or use through `process.Pipe`. |

`process.Stdio` variants:

| Variant | Meaning |
| --- | --- |
| `process.Stdio.Inherit` | Use the parent's stream. |
| `process.Stdio.Null` | Discard output, or give EOF as input. |
| `process.Stdio.Pipe` | Create a pipe. |

In normal code, use the functions `process.pipe()`, `process.null()`, and `process.inherit()` rather than the variants.

The functions are also capture-free function values. For example, `factory: def() -> process.Stdio = process.pipe` followed by `factory()` gives the same result as the qualified direct call.

## process.run

Signature: `process.run(command: list[str], cwd: str | None = None, env: dict[str, str] = {}, stdin: process.Stdio = process.null(), stdout: process.Stdio = process.pipe(), stderr: process.Stdio = process.pipe(), timeout: Duration = ..., group: bool = false) -> Result[process.Completed, process.Error]`

`process.run(...)` starts a child, waits for it, and returns a `process.Completed` value. By default, stdin is null and stdout and stderr are captured.

Omitting `timeout` means no caller deadline. The runtime marks the absence internally, and no `Duration` value stands for it. An explicit negative timeout is invalid. It does not mean unlimited.

```aura
def run_echo() -> Result[None, process.Error]:
    command = ["/bin/echo", "aura"]
    completed = try process.run(command, stdout=process.pipe(), stderr=process.pipe(), timeout=1s)

    try completed.check()
    print(completed.stdout().trim())
    return Result.Ok(None)
```

The `env` dictionary adds to the inherited host environment. An entry with the same name as an inherited variable replaces it.

Aura never invokes a shell for `run` or `start`. Only streams configured with `process.pipe()` are captured. Each captured stream is capped at 64 MiB.

Set `group=true` when the child may spawn descendants. On maintained Unix hosts, the parent then cleans up the whole process group.

Like other builtin module functions, `process.run` is a capture-free first-class function value:

- A direct alias such as `runner = process.run` keeps the parameter names and defaults. So `try runner(command)` still uses null stdin, captured stdout and stderr, and no caller deadline.
- Storing the value behind a structural function annotation, a class field, or a mutable collection drops those call-site extras. Every call then needs every positional argument.

## process.start

Signature: `process.start(command: list[str], cwd: str | None = None, env: dict[str, str] = {}, stdin: process.Stdio = process.null(), stdout: process.Stdio = process.inherit(), stderr: process.Stdio = process.inherit(), group: bool = false) -> Result[process.Child, process.Error]`

`process.start(...)` returns a live `process.Child`. The defaults suit interactive use: stdout and stderr inherit the parent's streams unless you ask for pipes.

```aura
def start_cat() -> Result[process.Child, process.Error]:
    command = ["/bin/cat"]
    child = try process.start(command, stdin=process.pipe(), stdout=process.pipe(), stderr=process.pipe(), group=true)
    return Result.Ok(child)
```

The caller must wait for, kill, terminate, or close the child.

## process.Child

| API | Signature | Contract |
| --- | --- | --- |
| `stdin` | `stdin() -> process.Pipe \| None` | Returns the child's stdin pipe if the child started with `stdin=process.pipe()`. Otherwise returns `None`. |
| `stdout` | `stdout() -> process.Pipe \| None` | Returns the child's stdout pipe if the child started with `stdout=process.pipe()`. Otherwise returns `None`. |
| `stderr` | `stderr() -> process.Pipe \| None` | Returns the child's stderr pipe if the child started with `stderr=process.pipe()`. Otherwise returns `None`. |
| `wait` | `wait(timeout: Duration = ...) -> process.Wait` | Waits for exit. Returns an exit, timeout, cancellation, or failure outcome. |
| `wait_or_none` | `wait_or_none(timeout: Duration = ...) -> Result[process.ExitStatus \| None, process.Error]` | Returns `Ok(status)` on exit and `Ok(None)` on timeout. Returns `Err(...)` for cancellation or a wait failure. |
| `wait_ok` | `wait_ok(timeout: Duration = ...) -> Result[process.ExitStatus, process.Error]` | Returns the exit status only for a successful exit. A non-zero status or a wait failure becomes `process.Error`. |
| `kill` | `kill() -> Result[None, process.Error]` | Kills the child at once. With `group=true`, targets the process group on maintained Unix hosts. |
| `terminate` | `terminate() -> Result[None, process.Error]` | Asks the child to terminate gracefully. With `group=true`, targets the process group on maintained Unix hosts. |
| `close` | `close() -> None` | Closes the child resource. Terminates the child if it is still running. |

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
| `Signaled(signal: own int32)` | A signal terminated the process. Only on platforms that expose signal status. |

## process.Pipe

| API | Signature | Contract |
| --- | --- | --- |
| `read_all` | `read_all() -> Result[str, process.Error]` | Reads the remaining strict UTF-8 text until EOF. Capped at 64 MiB. Use the byte APIs for arbitrary output. |
| `read_line` | `read_line(timeout: Duration = ...) -> Result[str \| None, process.Error]` | Reads one strict UTF-8 line without its trailing LF or CRLF. Returns `Ok(None)` only at EOF, or an error. |
| `read_bytes` | `read_bytes(max_bytes: int32, timeout: Duration = ...) -> Result[list[uint8] \| None, process.Error]` | Reads up to `max_bytes` raw bytes. Returns `Ok(None)` only at EOF. `max_bytes` must be in `1..=67108864`. |
| `write_all` | `write_all(text: str, timeout: Duration = ...) -> Result[None, process.Error]` | Writes all the text. |
| `write_bytes` | `write_bytes(bytes: list[uint8], timeout: Duration = ...) -> Result[None, process.Error]` | Writes all the bytes. |
| `flush` | `flush() -> Result[None, process.Error]` | Flushes buffered pipe output. |
| `close` | `close() -> None` | Closes the pipe handle. |

Timeouts and cancellation are errors, never `Ok(None)`:

- An expired pipe deadline returns `Err(process.Error.TimedOut)`.
- Cancellation returns `Err(process.Error.Cancelled)`.
- `read_bytes(0, ...)` and requests above 64 MiB return `process.Error.Io(io.Error.InvalidInput)`.

Close a child's stdin pipe when the child expects EOF:

```aura
def close_stdin(child: process.Child) -> Result[None, process.Error]:
    match own child.stdin():
        case process.Pipe as pipe:
            try pipe.write_all("hello\n")
            pipe.close()
        case None:
            pass
    return Result.Ok(None)
```

The accessor returns an owned `process.Pipe | None`. `match own` moves the pipe out of that result, so the arm holds an owned resource that it can write, flush, and close. A bare `match` would bind only a shared view of the pipe.

## process.Completed

`process.run(...)` returns a `process.Completed`.

| API | Signature | Contract |
| --- | --- | --- |
| `status` | `status() -> process.ExitStatus` | Returns the captured exit status. |
| `success` | `success() -> bool` | Returns `true` when the status is exit code `0`. |
| `stdout` | `stdout() -> str` | Returns captured stdout decoded as strict UTF-8. Invalid UTF-8 raises a runtime diagnostic. Use `stdout_bytes` for untrusted output. |
| `stdout_bytes` | `stdout_bytes() -> list[uint8]` | Returns captured stdout as raw bytes. |
| `stderr` | `stderr() -> str` | Returns captured stderr decoded as strict UTF-8. Invalid UTF-8 raises a runtime diagnostic. Use `stderr_bytes` for untrusted output. |
| `stderr_bytes` | `stderr_bytes() -> list[uint8]` | Returns captured stderr as raw bytes. |
| `check` | `check() -> Result[None, process.Error]` | Returns `Ok(None)` for a successful exit status. Otherwise returns `Err(...)`. |

Use `check` when a failed command should stop the current `Result`-returning function:

```aura
def must_succeed() -> Result[None, process.Error]:
    completed = try process.run(["/bin/false"], timeout=1s)
    try completed.check()
    return Result.Ok(None)
```

Use the byte methods for tools that may emit binary or non-UTF-8 output.

## process.supervisor

```aura
process.supervisor() -> process.Supervisor
```

A supervisor is a resource that owns named child process specs and emits lifecycle events. Bind it with `with` where you can:

```aura
def wait_for_worker() -> Result[process.SupervisorWait, process.Error]:
    with supervisor = process.supervisor():
        try supervisor.start(name="worker", command=["/bin/sleep", "1"])
        return Result.Ok(supervisor.wait(timeout=2s))
```

## process.Supervisor

| API | Signature | Contract |
| --- | --- | --- |
| `start` | `start(name: own str, command: own list[str], cwd: own (str \| None) = ..., env: own dict[str, str] = ..., stdin: own process.Stdio = ..., stdout: own process.Stdio = ..., stderr: own process.Stdio = ..., restart: own process.RestartPolicy = ..., backoff: own Duration = ..., max_restarts: own int32 = ..., group: own bool = ...) -> Result[None, process.Error]` | Starts a named child under supervision. Keeps the owned configuration it needs for restarts. Each name must be unique within the supervisor. |
| `wait` | `wait(timeout: Duration = ...) -> process.SupervisorWait` | Waits for the next supervisor event, a timeout, or cancellation. |
| `wait_or_none` | `wait_or_none(timeout: Duration = ...) -> Result[process.SupervisorEvent \| None, process.Error]` | Returns `Ok(event)`, or `Ok(None)` on timeout. Returns `Err(...)` on cancellation or a wait failure. |
| `stop` | `stop() -> Result[None, process.Error]` | Stops every supervised child and clears the supervisor. |
| `is_empty` | `is_empty() -> bool` | Returns `true` when no services are running or waiting to restart. |
| `close` | `close() -> None` | Closes the supervisor and stops all the children it manages. |

Runtime defaults for `Supervisor.start(...)`:

| Parameter | Default |
| --- | --- |
| `cwd` | `None` |
| `env` | empty dictionary |
| `stdin` | `process.null()` |
| `stdout` | `process.inherit()` |
| `stderr` | `process.inherit()` |
| `restart` | `process.RestartPolicy.OnFailure` |
| `backoff` | `100ms` |
| `max_restarts` | unlimited when omitted. `-1` is also accepted as unlimited. |
| `group` | `true` |

When restart is enabled, `backoff` must be at least `10ms`.

`process.RestartPolicy` variants:

| Variant | Meaning |
| --- | --- |
| `Never` | Do not restart. |
| `OnFailure` | Restart only when the child exits unsuccessfully. |
| `Always` | Restart after every exit, while the restart limit allows. |

`process.SupervisorEvent` variants:

| Variant | Meaning |
| --- | --- |
| `Exited(name: own str, status: own process.ExitStatus, restart_count: own int32)` | A child exited and was not restarted. |
| `Restarted(name: own str, status: own process.ExitStatus, restart_count: own int32)` | A child exited and a replacement started. |
| `Failed(name: own str, error: own process.Error, restart_count: own int32)` | A child failed to start or restart. |

`process.SupervisorWait` variants:

| Variant | Meaning |
| --- | --- |
| `Event(event: own process.SupervisorEvent)` | A supervisor event is available. |
| `TimedOut` | No event arrived before the timeout. |
| `Cancelled` | Cancellation interrupted the wait. |

An invalid timer passed to `Supervisor.wait` cannot come back as a `process.Error`, because `wait` returns `process.SupervisorWait`. It maps to exactly this value:

`process.SupervisorWait.Event(process.SupervisorEvent.Failed("<supervisor>", process.Error.Io(io.Error.InvalidInput), 0))`

The synthetic name is `<supervisor>` and the synthetic restart count is zero. `wait_or_none` has an error carrier in its return type, so the same invalid timer returns `Result.Err(process.Error.Io(io.Error.InvalidInput))` instead.

## process.Error

| Variant | Meaning |
| --- | --- |
| `NoCommand` | The command list was empty. |
| `TimedOut` | A process operation timed out. |
| `Cancelled` | Cancellation interrupted the operation. |
| `Io(error: own io.Error)` | The operation failed with an I/O error. |
| `Spawn(message: own str)` | The child could not be spawned. |
| `Other(message: own str)` | A process failure that no other variant covers. |

## Cleanup Rules

Child, pipe, and supervisor values are resources. Use `with` for supervisors. Call `close()` on children and pipes when their ownership is not scoped.

`close()` on a child terminates it if it is still running. With `group=true`, cleanup targets the process group on maintained Unix hosts.

When `process.run` times out, or its Aura task is cancelled, the runtime terminates the child and waits for cleanup. With `group=true`, it does this to the process group on maintained Unix hosts. As with all host I/O, cancellation cannot undo side effects that the child already performed.

## Grammar

The process module adds no grammar. A command is an ordinary `list[str]` expression passed to an ordinary call. Aura does not parse shell syntax, split a command string, expand variables, interpret redirections, or build pipelines. Named arguments, `Duration` literals, `Result`, `T | None` unions, `try`, `match`, and `with` use their general grammar.

A parameter shown with `= ...` takes the builtin default documented on this page. The ellipsis is reference notation, not a source expression. Process and standard-I/O variants use ordinary qualified enum construction and pattern syntax.

For `process.run`, an omitted timeout is marked internally, not by a sentinel `Duration`. An explicit zero is a real, immediate deadline. An explicit negative value is invalid input.

## Typing Rules

The function and method signatures above are normative.

- Commands are `list[str]`.
- Environment overlays are `dict[str, str]`.
- Working directories are `str | None`. `None` inherits the parent's directory.
- Timeout parameters are `Duration`.

Fallible start, run, pipe, and control operations use `process.Error`. The wait APIs return an enum, a `T | None`, or a `Result` by design, as their tables show.

`process.Child`, `process.Pipe`, and `process.Supervisor` are non-copy resources. These operations need a mutable receiver place: kill, terminate, pipe write, flush, and close, and supervisor start, stop, and close.

`Supervisor.start` consumes every configuration argument marked `own`, because the supervisor keeps that configuration for possible restarts.

`Completed.stdout()` and `stderr()` are text accessors that can trap. The byte accessors work for any captured bytes.

## Runtime Semantics

`run` and `start` invoke exactly the executable and argument list you supply. They inherit the host environment, then apply the `env` entries as replacements or additions.

- `run` waits, and captures only the streams configured as pipes.
- `start` returns at once with a live child and any configured pipe endpoints.
- Repeated calls to a child's pipe accessor return handles to the same endpoint. The handles share cursor state and close state.

`Child.wait` reports exit, timeout, cancellation, or failure. It does not terminate a child that is still running. In contrast, a timeout or cancellation of `process.run` terminates the child and waits for cleanup.

A timeout or backoff is invalid if it is negative, cannot be represented by the host, or overflows the deadline. Where the declared outcome can carry an error, it becomes `process.Error.Io(io.Error.InvalidInput)`. Deadline overflow never becomes an unlimited wait.

`Completed.check` turns a non-success status into a `process.Error`. Invalid captured UTF-8 in `stdout()` or `stderr()` is a runtime diagnostic. The byte accessors return the original bytes.

Supervisor restarts, counts, events, defaults, and the minimum backoff follow the tables above.

## Ownership And Evaluation Order

Arguments are evaluated left to right before the process is created. `run` and `start` share their Aura arguments for the call. They copy the command, environment, and path data they need into host process state. They keep no Aura borrows after they return.

A supervisor takes ownership of the configuration it keeps. The caller owns every child, pipe, completed output, status, error, and event value that an operation returns.

Moving a resource invalidates the source binding. `with` closes a supervisor on every scope exit. An explicit `close()` on a child or pipe closes the shared handle state. Closing a child also terminates it if it is still running.

Cleanup runs after the body. It cannot undo filesystem, network, or other external side effects that the child already performed.

## Diagnostics

| Code | Cause |
| --- | --- |
| `AU2001` | Unknown process member. |
| `AU2002` | Type mismatch. |
| `AU2004` | Invalid argument binding. |
| `AU3001` | Use of a process resource after it moved. |
| `AU3002` | Borrow conflict. |
| `AU3003` | A mutating method called through an immutable place. |
| `AU2999` | Any other static rejection. |

These failures are typed `process.Error` values:

- empty commands
- spawn failures
- timeouts and cancellation
- invalid byte counts
- invalid timeout or backoff values, or invalid deadlines
- closed pipes
- a non-zero status checked through `check`
- ordinary host I/O failures

Invalid timer inputs use `process.Error.Io(io.Error.InvalidInput)`.

Decoding invalid captured bytes through `Completed.stdout()` or `stderr()` is a runtime trap with code `AU4005`, by design. Use `stdout_bytes()` or `stderr_bytes()` when the output encoding is not guaranteed.

## Backend Support

The MIR runtime and the direct native backend implement process creation, capture, pipes, waiting, supervisor behavior, typed errors, and cleanup. MIR is the compiler's mid-level intermediate representation. Both backends must match on command-list handling, the environment overlay, captured bytes, timeout outcomes, and ownership.

Process-group creation and signaling are maintained on Unix hosts. On other hosts, asking for group behavior returns a typed process error. Cleanup is never silently weakened. Executable lookup, signals, and exit-status details otherwise follow the host's process facilities.

## Limits And Implementation-Defined Behavior

- Each captured stream from `process.run`, and each whole-pipe read, is capped at 64 MiB. This cap is separate from the larger filesystem whole-read limit.
- Bounded pipe byte reads accept `1..=67108864`.
- Text access is strict UTF-8.
- When restart is enabled, supervisor restart backoff must be at least 10 ms.
- An omitted maximum restart count, or `-1`, means unlimited.

The module has no shell, command-string parser, pipeline builder, pseudo-terminal API, daemon manager, sandbox, resource-limit API, or portable signal-number abstraction.

The host determines executable discovery, path syntax, the inherited environment, signal availability, numeric exit behavior, the meaning of graceful termination, scheduling, and side effects. Timeouts and cancellation bound how long Aura waits. They cannot take back child actions that already happened. Group cleanup of descendants is a contract on maintained Unix hosts. It is not a portable guarantee for every host process tree.

## Status

Aura 0.3 implements one-shot execution, live children, standard-I/O configuration, pipes, completed output, status checking, supervisor restarts and events, typed failures, and Unix process-group cleanup. The fixed stream cap is an accepted design decision. So is the policy for omitted timeouts and invalid host timers.

These are not available: shell evaluation, pipelines, pseudo-terminals, Windows process groups, portable signal control, sandboxing, and operating-system service management. The current API does not provide any of them implicitly.

Design records: [ADR-0018: Fixed resource read limits](https://github.com/johnolafenwa/Aura/blob/main/architecture_docs/decisions/0018-fixed-resource-read-limits.md) and [ADR-0019: Duration conversion and timer policy](https://github.com/johnolafenwa/Aura/blob/main/architecture_docs/decisions/0019-duration-conversion-and-timer-policy.md).
