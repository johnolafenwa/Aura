# Concurrency

Aurora provides Go-style concurrency with typed channels, spawned tasks, task groups, and `select`. This chapter walks through each primitive with inline examples.

## Channels

A channel is a typed pipe for sending values between tasks. Create one with `channel()` and an explicit type annotation:

```python
ch: Channel[int32] = channel()
```

The bootstrap compiler requires the type annotation -- a bare `channel()` without an expected `Channel[T]` type is rejected.

Send and receive:

```python
ch: Channel[int32] = channel()
ch.send(42)
msg = ch.recv()    # returns Option[int32]
```

`recv()` returns `Option[T]`:

- `Option.Some(value)` when a value is available
- `Option.None` when the channel is closed and empty

`send(value)` returns `Result[None, SendError[T]]`:

- `Result.Ok(None)` on success
- `Result.Err(SendError.Closed(value))` when the channel is already closed, returning the unsent value

Close a channel with `ch.close()`. After closing, no more values can be sent, but existing values can still be received.

Channels are move types (see [06-ownership-and-borrowing.md](06-ownership-and-borrowing.md)). To share a channel between tasks, clone it:

```python
sender = ch.clone()
```

Each clone is an independent handle to the same underlying channel. Cloning is cheap -- it does not copy messages.

### Iterating Over A Channel

You can iterate over a channel with `for`. The loop runs until the channel is closed and empty:

```python
jobs: Channel[int32] = channel()
jobs.send(1)
jobs.send(2)
jobs.close()

for job in jobs:
    print(job)
```

See [examples/concurrency/channel_iteration.au](../examples/concurrency/channel_iteration.au).

## Spawning Tasks

Use `spawn` with a named function call to run work concurrently:

```python
def producer(out: Channel[int32]):
    out.send(2)
    out.send(4)
    out.close()

ch: Channel[int32] = channel()
task = spawn producer(ch.clone())
```

`spawn` returns a `Task[T]`. Call `.join()` to wait for the task to complete.

`Task[T]` also supports `.clone()` for sharing a handle between multiple consumers.

### Fire-And-Forget With `spawn detached`

Use `spawn detached` when you do not need to join the result:

```python
spawn detached producer(ch.clone())
```

Detached tasks do not return a `Task[T]` handle.

## Structured Task Groups

Task groups tie child tasks to a lexical scope using `with`:

```python
with task_group() as group:
    group.spawn(worker, jobs.clone(), results.clone())
    group.spawn(worker, jobs.clone(), results.clone())
# leaving the block joins all child tasks
```

When the `with` block ends, any still-running children are joined. This ensures spawned work does not outlive its parent scope.

### Cooperative Cancellation

Call `group.cancel()` to signal all tasks in the group to stop. Inside a task, call `cancelled()` to check whether cancellation was requested:

```python
def worker(out: Channel[int32]):
    mut i: int32 = 0
    while i < 100:
        if cancelled():
            return        # exit cleanly
        out.send(i)
        i += 1
```

Cancellation is cooperative -- `cancelled()` returns `true` after `group.cancel()` is called, but the task must check it and decide to stop. Aurora does not forcibly kill tasks.

See [examples/concurrency/task_group_cancel.au](../examples/concurrency/task_group_cancel.au).

## `select`

`select` waits on multiple channel operations or a timer, executing whichever is ready first:

```python
select:
    case value = inbox.recv():
        match value:
            case Option.Some(message):
                print(message)
            case Option.None:
                print("closed")
    case after(100ms):
        print("timeout")
```

### Supported Arms

- `case binding = channel.recv():` -- receive and bind
- `case channel.recv():` -- receive and discard
- `case binding = channel.send(value):` -- send and bind the result
- `case channel.send(value):` -- send and discard the result
- `case after(100ms):` -- timeout after a duration
- `case after(duration=100ms):` -- named form

When a `select` mixes `recv()` arms with an `after(...)` arm, a closed-and-empty channel does not starve the timer. The timeout arm still fires as an escape path.

Duration literals include `ms` (milliseconds), `s` (seconds), and `m` (minutes).

## `sleep`

A simple blocking delay:

```python
sleep(100ms)
```

See [examples/concurrency/sleep_builtin.au](../examples/concurrency/sleep_builtin.au).

## Full Example: Producer-Consumer

```python
def producer(out: Channel[int32]):
    out.send(2)
    out.send(4)
    out.close()

def main() -> int32:
    ch: Channel[int32] = channel()
    task = spawn producer(ch.clone())

    while true:
        match ch.recv():
            case Option.Some(value):
                print(value)
            case Option.None:
                break

    task.join()
    return 0
```

See:

- [examples/concurrency/channels_spawn.au](../examples/concurrency/channels_spawn.au)
- [examples/concurrency/send_result.au](../examples/concurrency/send_result.au)
- [examples/concurrency/spawn_detached.au](../examples/concurrency/spawn_detached.au)
- [examples/concurrency/select_send.au](../examples/concurrency/select_send.au)
- [examples/concurrency/task_group_select.au](../examples/concurrency/task_group_select.au)
- [examples/concurrency/task_group_cancel.au](../examples/concurrency/task_group_cancel.au)
- [examples/concurrency/select_timeout.au](../examples/concurrency/select_timeout.au)

## Current Limits

The bootstrap concurrency runtime does not yet provide:

- network or socket integration
- general async I/O APIs
- detached-task ownership restrictions from the full proposal
