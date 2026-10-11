# Resource Management

A `with` statement runs a block with one or more context managers and exits each one when the block ends. Use it for files, network connections, task groups, and your own classes. The exit runs whether the block finishes normally or leaves early. It works like Python's `with` statement and context managers.

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

## Builtin Resources

These builtin types work in `with` and close themselves:

- `fs.File`
- the `net` stream, listener, socket, and HTTP types
- `process.Child`, `process.Pipe`, and `process.Supervisor`
- `TaskGroup`

## Writing A Context Manager

A class becomes a context manager when it declares `__exit__(mut self)`. An optional `__enter__(mut self)` runs before the body:

```aura
class Timer:
    label: str

    def __enter__(mut self) -> None:
        print("start " + self.label)

    def __exit__(mut self) -> None:
        print("stop " + self.label)

def main():
    with timer = Timer(label="load"):
        print("loading")
```

This prints `start load`, `loading`, and `stop load`.

A class with only `close(mut self) -> None` also works. Its `close` is the exit:

```aura fragment
def close(mut self):
```

Both methods take `mut self` and no other parameters. The checker reports `AU2017` for any other shape.

## Entering With A View

`__enter__` can return a view of the manager. The `with` name then binds the view, and the manager stays out of reach until the exit:

```aura
class Lock:
    values: list[int64]

    def __enter__(mut self) -> view mut list[int64] from self:
        return view mut self.values

    def __exit__(mut self) -> None:
        print("unlocked with " + str(len(self.values)) + " values")

def main():
    with Lock(values=[]) as values:
        values.append(1)
```

The view cannot leave the block. Returning or storing it reports `AU3010`.

## Exits That Can Fail

`__exit__` may return `Result[None, E]`. On a normal exit, an `Err` returns from the enclosing function like `try`, so that function must return a compatible `Result`. If the block is already failing, the block's own error wins.

## Several Managers In One Header

List several items separated by commas. They enter left to right and exit right to left. A later item can use an earlier binding:

```aura fragment
with input = try fs.open(source), output = try fs.create(target):
    try output.write_all(try input.read_all())
```

See [examples/resources/context_managers.au](../examples/resources/context_managers.au) for all of these together.

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

- `__exit__` receives no exception information. It cannot inspect or suppress the failure that ended the block.
- `__enter__` returns `None` or a view of the manager. It cannot return an owned value or a `Result`.
- If the whole scheduler stops while a task is suspended, that task's exits do not run. Use `TaskGroup` scopes so tasks finish inside their block.
