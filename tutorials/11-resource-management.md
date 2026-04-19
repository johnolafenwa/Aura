# Resource Management

When you open a file, a network connection, or a task group, you need to ensure it gets cleaned up even if something goes wrong. Aurora's `with` statement provides deterministic scoped cleanup -- the resource is always closed when the block exits, whether by normal completion or early `return`.

If you are coming from Python, this works like Python's `with` statement and context managers.

## `with` Binds A Scoped Resource

```python
import fs
import io

def load_text(path: String) -> Result[String, io.Error]:
    with file = try fs.open(path):
        return file.read_all()
    # file.close() is called automatically here, even on early return
```

The bound resource:

- is available inside the block as a mutable local binding
- always runs `close(borrow mut self)` when the block exits
- cleanup runs on normal fallthrough, on early `return`, and after `try` propagation

See [examples/resources/with_resource.au](../examples/resources/with_resource.au).

## The Resource Protocol

In the current compiler, a `with` resource may be:

- a user-defined class with:

```python
def close(borrow mut self):
```

- a builtin `fs.File`
- a builtin `net.TcpStream`
- a builtin `net.TcpListener`
- a `TaskGroup`

For user-defined classes, `close(...)` must take `borrow mut self`, no extra parameters, and return `None`.

## `with ... as ...` For Task Groups

`TaskGroup()` is the one non-class value that supports `with`:

```python
with TaskGroup() as group:
    group.start(worker, out)
    group.start(worker, out)
# leaving the block waits for all child tasks
```

Task groups tie child tasks to a lexical scope. When the `with` block ends, any still-running child tasks are joined. You can also cancel early with `group.cancel()`.

See [examples/concurrency/task_group_queue_sum.au](../examples/concurrency/task_group_queue_sum.au) and [examples/concurrency/task_group_cancel.au](../examples/concurrency/task_group_cancel.au).

## Current Limits

- builtin resources use the fixed file/TCP/task-group surface; there is no broader enter/exit protocol yet
- user-defined resources still require `close(borrow mut self)` with no extra parameters
- no borrowed resource bindings
