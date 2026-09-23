# Case Study: A Supervised Process Runner

This case study builds a small service supervisor. It manages named child processes, watches what happens to them, and guarantees cleanup when the surrounding program exits. The exit may be normal or a runtime error unwinding through the scope.

It uses `process.supervisor()`, named children, a restart policy, process groups, and the `process.SupervisorWait` and `process.SupervisorEvent` enums.

## When To Use A Supervisor

Aura has three levels of process management:

| API | Use it for |
| --- | --- |
| `process.run(...)` | One command that runs to completion and produces a `Completed` record. |
| `process.start(...)` | One child whose lifetime overlaps the parent's, with pipes the parent talks to. |
| `process.supervisor()` | A set of named children with a lifecycle policy. |

Choose a supervisor when the program must reject duplicate names, restart failed children under a policy, and observe exits as a stream of events.

## Starting A Child

```aura
import process

with supervisor = process.supervisor():
    match supervisor.start(name="worker", command=["/bin/sleep", "1"], stdout=process.null(), stderr=process.inherit(), restart=process.RestartPolicy.Never, group=true):
        case Result.Ok(_):
            print("started")
        case Result.Err(error):
            print(error)
```

`group=true` puts the child in its own process group on supported Unix hosts. Termination then reaches the leader and every descendant, so cleanup is reliable even when a child starts more processes.

Names are unique within a supervisor. Starting another child with the same name returns `Result.Err` and keeps the existing child. Every later lifecycle operation relies on names staying unique.

## Waiting For Events

A supervisor produces events as children start, exit, and restart. `wait` returns a structured `SupervisorWait` outcome:

```aura fragment
match supervisor.wait(timeout=2s):
    case process.SupervisorWait.Event(process.SupervisorEvent.Exited(name, status, restart_count)):
        print(name)
    case process.SupervisorWait.Event(process.SupervisorEvent.Restarted(name, status, restart_count)):
        print(name)
    case process.SupervisorWait.Event(process.SupervisorEvent.Failed(name, error, restart_count)):
        print(name)
    case process.SupervisorWait.TimedOut:
        print("still running")
    case process.SupervisorWait.Cancelled:
        print("cancelled")
```

Each event carries the child's name, its status or error, and the number of restarts so far. That covers most log-style reporting and any retry policy the program's own logic needs.

When a timeout and "no event" can share a branch, use `wait_or_none`. It maps a timeout to `None` inside a `Result`:

```aura fragment
match own supervisor.wait_or_none(timeout=500ms):
    case Result.Ok(process.SupervisorEvent as event):
        print(event)
    case Result.Ok(None):
        print("no event")
    case Result.Err(error):
        print(error)
```

The payload is `process.SupervisorEvent | None`. The nested type pattern `process.SupervisorEvent as event` selects an event, and `None` selects the timeout. `match own` consumes the returned `Result`, so the arm owns the event it prints.

## Restart Policy

`process.RestartPolicy` has three variants:

| Policy | Behaviour |
| --- | --- |
| `Never` | Do not restart. |
| `OnFailure` | Restart only after an unsuccessful exit. |
| `Always` | Restart after any exit while restart limits allow it. |

The supervisor's `start` method also accepts:

| Parameter | Meaning |
| --- | --- |
| `backoff` | The delay before a restart. The minimum is `10ms` when restarts are enabled. |
| `max_restarts` | Omit it for unlimited restarts. `-1` is also accepted as an explicit "unlimited". |
| `group` | Defaults to `true` for supervised children. |

Each policy has a cost. `Always` can retry a config error a hundred times. `Never` can turn a transient failure into an outage. Pick the policy that fits each child, not the supervisor as a whole.

## Scoped Cleanup

Prefer this shape:

```aura fragment
with supervisor = process.supervisor():
    try supervisor.start(name="worker", command=["/bin/sleep", "60"], group=true)
    match supervisor.wait(timeout=1s):
        case process.SupervisorWait.TimedOut:
            print("still alive")
        case process.SupervisorWait.Event(event):
            print(event)
        case process.SupervisorWait.Cancelled:
            print("cancelled")
```

When execution leaves the `with` block, the supervisor closes and stops every child it manages. One scoped rule covers normal returns and runtime errors, so no return path needs its own `close()` call.

Call `supervisor.stop()` when stopping the whole managed set is a real branch in the program, such as a user-requested pipeline shutdown. Scope exit handles ordinary cleanup.

## A Template To Copy

This is the smallest template that still has the right shape:

```aura
import process

def run() -> Result[None, process.Error]:
    with supervisor = process.supervisor():
        try supervisor.start(name="service", command=["/path/to/bin"], restart=process.RestartPolicy.OnFailure, backoff=200ms, max_restarts=5, group=true)

        loop_until_event(supervisor)
        return Result.Ok(None)

def loop_until_event(supervisor: process.Supervisor) -> Result[None, process.Error]:
    match supervisor.wait(timeout=5s):
        case process.SupervisorWait.Event(event):
            print(event)
            return Result.Ok(None)
        case process.SupervisorWait.TimedOut:
            return Result.Ok(None)
        case process.SupervisorWait.Cancelled:
            return Result.Ok(None)
```

The important facts are all in the types:

- The `with` block bounds the resource's lifetime.
- The wait outcome is a structured enum.
- Recoverable failures are returned.
- Cleanup runs on normal returns and on runtime unwinding.

This is the worker-pool shape from the previous case study, applied to subprocesses. Once it is familiar, make it your default.
