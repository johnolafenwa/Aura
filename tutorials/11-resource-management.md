# Resource Management

A `with` statement closes a resource when its block exits. Use it for files, network connections, and task groups. Cleanup runs whether the block finishes normally or leaves early with `return`. It works like Python's `with` statement and context managers.

## `with` Binds A Scoped Resource

```aura check-pass
import fs
import io

def load_text(path: str) -> Result[str, io.Error]:
    with file = try fs.open(path):
        return file.read_all()
    # file.close() is called automatically here, even on early return
```

Inside the block, `file` is a mutable local binding. When the block exits, Aura calls the resource's `close(mut self)`. This happens:

- when the block falls through normally
- on an early `return`
- after `try` propagates an error

See [examples/resources/with_resource.au](../examples/resources/with_resource.au).

## The Resource Protocol

A `with` resource can be any of these:

- a user-defined class with a `close` method:

```aura fragment
def close(mut self):
```

- a builtin `fs.File`
- a builtin `net.TcpStream`
- a builtin `net.TcpListener`
- a `TaskGroup`

A user-defined `close` must take `mut self`, take no other parameters, and return `None`.

## `with ... as ...` For Task Groups

A `TaskGroup` ties child tasks to a lexical scope. Open one with `with ... as ...`:

```aura fragment
with TaskGroup() as group:
    group.start(worker, out)
    group.start(worker, out)
# leaving the block waits for children and cancels only unbounded waits
# for which no live task can provide a wakeup
```

When the block ends, Aura waits for the child tasks to finish. A child blocked on a queue is cancelled only when no live task can wake it:

- Temporary queue backpressure is not cancelled, even when the host is busy.
- A true deadlock, with no reachable sender, receiver, or queue closer, is cancelled so shutdown does not hang forever.

To cancel early, call `group.cancel()`. A `for value in queue:` loop inside the same `with TaskGroup()` scope sees the cancellation and exits cleanly.

See [examples/concurrency/task_group_queue_sum.au](../examples/concurrency/task_group_queue_sum.au) and [examples/concurrency/task_group_cancel.au](../examples/concurrency/task_group_cancel.au). [13-concurrency.md](13-concurrency.md) covers task groups in full.

## Current Limits

- Builtin resources are limited to the file, TCP, and task-group types above. There is no general enter/exit protocol.
- A user-defined resource must have `close(mut self)` with no other parameters.
- A `with` binding cannot borrow its resource.
