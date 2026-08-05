# Resource Management

When you open a file, a network connection, or a task group, you need to ensure it gets cleaned up even if something goes wrong. Aura's `with` statement provides deterministic scoped cleanup -- the resource is always closed when the block exits, whether by normal completion or early `return`.

If you are coming from Python, this works like Python's `with` statement and context managers.

## `with` Binds A Scoped Resource

```aura check-pass
import fs
import io

def load_text(path: str) -> Result[str, io.Error]:
    with file = try fs.open(path):
        return file.read_all()
    # file.close() is called automatically here, even on early return
```

The bound resource:

- is available inside the block as a mutable local binding
- always runs `close(mut self)` when the block exits
- cleanup runs on normal fallthrough, on early `return`, and after `try` propagation

See [examples/resources/with_resource.au](../examples/resources/with_resource.au).

## The Resource Protocol

In the current compiler, a `with` resource may be:

- a user-defined class with:

```aura fragment
def close(mut self):
```

- a builtin `fs.File`
- a builtin `net.TcpStream`
- a builtin `net.TcpListener`
- a `TaskGroup`

For user-defined classes, `close(...)` must take `mut self`, no extra parameters, and return `None`.

## `with ... as ...` For Task Groups

`TaskGroup()` is the one non-class value that supports `with`:

```aura fragment
with TaskGroup() as group:
    group.start(worker, out)
    group.start(worker, out)
# leaving the block waits for children and cancels only unbounded waits
# for which no live task can provide a wakeup
```

Task groups tie child tasks to a lexical scope. When the `with` block ends,
Aura waits for child tasks to finish. A child blocked in a queue wait is
cancelled only when no live task can wake it; temporary queue backpressure does
not become cancellation merely because the host is busy. A true deadlock with
no reachable sender, receiver, or queue closer is cancelled so shutdown does
not hang forever. You can also cancel early with `group.cancel()`. Queue
iteration with `for value in queue:` inside the same `with TaskGroup()` scope
observes that cancellation and exits cleanly.

See [examples/concurrency/task_group_queue_sum.au](../examples/concurrency/task_group_queue_sum.au) and [examples/concurrency/task_group_cancel.au](../examples/concurrency/task_group_cancel.au).

## Current Limits

- builtin resources use the fixed file/TCP/task-group surface; there is no broader enter/exit protocol yet
- user-defined resources still require `close(mut self)` with no extra parameters
- no borrowed resource bindings
