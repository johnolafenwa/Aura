# Concurrency

Aurora now has a bootstrap concurrency surface with typed channels, spawned tasks, detached tasks, task groups, `select`, send-result errors, and cooperative cancellation.

## Channels

Create a channel with an explicit annotation:

```aurora
ch: Channel[int32] = channel()
```

In the bootstrap compiler, `channel()` needs that surrounding type context. A bare `channel()` call without an expected `Channel[T]` type is rejected.

The current bootstrap supports:

- `clone()`
- `send(value)`
- `recv()`
- `close()`
- `for value in jobs:` iteration over a channel until it closes

`recv()` returns `Option[T]`:

- `Option.Some(value)` when a value is available
- `Option.None` when the channel is closed and empty

`send(value)` returns `Result[None, SendError[T]]`:

- `Result.Ok(None)` when the value was queued successfully
- `Result.Err(SendError.Closed(value))` when the channel was already closed

Channels can also act as `for` iterables:

```aurora
jobs: Channel[int32] = channel()
jobs.send(1)
jobs.send(2)
jobs.close()

for job in jobs:
    print(job)
```

See [examples/concurrency/channel_iteration.au](../examples/concurrency/channel_iteration.au).

## Spawning work

Use `spawn` with a named function call:

```aurora
task = spawn producer(ch.clone())
```

The result is a `Task[T]`. Call `join()` to wait for completion.

`Task[T]` also supports `clone()` for sharing a handle.

Use `spawn detached` for explicit fire-and-forget work that is not joined through a task handle:

```aurora
spawn detached producer(ch.clone())
```

Detached tasks do not return a `Task[T]` handle in the bootstrap runtime.

## Structured task groups

Use `with task_group() as group:` to keep child tasks tied to a lexical scope:

```aurora
with task_group() as group:
    group.spawn(worker, jobs.clone(), results.clone())
```

Leaving the `with` block joins the remaining child tasks. Call `group.cancel()` to signal cooperative cancellation early.

Inside spawned work, `cancelled()` reports whether the current task has been cancelled:

```aurora
def worker(out: Channel[int32]):
    mut i: int32 = 0
    while i < 100:
        if cancelled():
            return
        out.send(i)
        i += 1
```

## Select and timeouts

Use `select` to wait on multiple channel receives or timer arms:

```aurora
select:
    case value = inbox.recv():
        match value:
            case Option.Some(message):
                print(message)
            case Option.None:
                print("closed")
    case after(100ms):
        print("waiting")
```

Bootstrap `select` currently supports:

- `case binding = channel.recv():`
- `case channel.recv():`
- `case binding = channel.send(value):`
- `case channel.send(value):`
- `case after(100ms):`
- `case after(duration=100ms):`

Duration literals currently include `ms`, `s`, and `m`.

`after(...)` is a builtin helper that turns a `Duration` into a timeout arm.

When a `select` mixes `recv()` arms with at least one `after(...)` arm, a closed-and-empty channel
does not starve the timer. The timeout arm still gets a chance to fire.

Aurora also provides a simple blocking sleep helper for the current runtime:

```aurora
sleep(100ms)
```

See [examples/concurrency/sleep_builtin.au](../examples/concurrency/sleep_builtin.au).

The minute suffix is fully supported in the current compiler:

```aurora
print(2m)
```

See [examples/concurrency/minute_duration.au](../examples/concurrency/minute_duration.au).

The timer helper also supports a named form:

```aurora
select:
    case after(duration=100ms):
        print("waiting")
```

## Full example

```aurora
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
- [examples/concurrency/select_timeout_named.au](../examples/concurrency/select_timeout_named.au)
- [examples/concurrency/sleep_builtin.au](../examples/concurrency/sleep_builtin.au)
- [examples/concurrency/minute_duration.au](../examples/concurrency/minute_duration.au)

## Current Limits

The bootstrap concurrency runtime does not yet provide:

- network or socket integration
- general async I/O APIs
- detached-task ownership restrictions from the full proposal
