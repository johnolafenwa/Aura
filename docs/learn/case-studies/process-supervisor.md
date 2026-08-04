# Case Study: A Supervised Process Runner

This case study builds a small service supervisor. The goal is to manage named child processes, watch what happens to them, and guarantee cleanup when the surrounding program exits — whether that exit is normal or a runtime error unwinding through the scope.

The program uses `process.supervisor()`, named children, a restart policy, process groups, and the `process.SupervisorWait` / `process.SupervisorEvent` enums.

## When To Use A Supervisor

Three levels of process management in Aura, roughly from simplest to richest:

- `process.run(...)` for a single command that should execute to completion and produce a `Completed` record.
- `process.start(...)` for a single child whose lifetime overlaps with the parent and whose pipes the parent needs to interact with.
- `process.supervisor()` for a set of named children with a lifecycle policy.

When the program needs to reject duplicate names, restart failed children according to a policy, and observe exits as an event stream, `supervisor` is the right primitive.

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

`group=true` puts the child in its own process group on supported Unix hosts.
Termination can then reach the leader and every descendant, which makes
cleanup reliable when a child starts more processes.

Names are unique within a supervisor. Starting another child with the same name
returns `Result.Err` and preserves the existing child. Name integrity is a
correctness property for every lifecycle operation that follows.

## Waiting For Events

A supervisor produces events while children start, exit, and restart. The `wait` method returns a structured `SupervisorWait` outcome:

```aura
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

Each event carries the child's name, its status or error, and how many restarts have happened. That is enough for most log-style reporting and for retry policies that only a program's own logic would understand.

When "timed out or no event" can collapse to the same branch, `wait_or_none` maps a timeout to `Option.None` inside a `Result`:

```aura
match supervisor.wait_or_none(timeout=500ms):
    case Result.Ok(Option.Some(event)):
        print(event)
    case Result.Ok(Option.None):
        print("no event")
    case Result.Err(error):
        print(error)
```

## Restart Policy

`process.RestartPolicy` has three variants:

| Policy | Behaviour |
| --- | --- |
| `Never` | Do not restart. |
| `OnFailure` | Restart only after an unsuccessful exit. |
| `Always` | Restart after any exit while restart limits allow it. |

The supervisor's `start` method also accepts:

- `backoff`, the delay before restarting (minimum `10ms` when restarts are enabled)
- `max_restarts`, where omitting it means unlimited and `-1` is accepted as an explicit "unlimited"
- `group`, which defaults to `true` for supervised children

Restart policy is a question with a real answer. Letting `Always` retry a config error a hundred times is a cost; letting `Never` turn a transient failure into an outage is a different cost. Pick the policy that fits the child, not the supervisor.

## Scoped Cleanup

Prefer this shape:

```aura
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

When execution leaves the `with` block, the supervisor closes and stops every
managed child. One scoped rule covers normal returns and runtime errors, so no
return path needs a separate `close()` call.

Explicitly call `supervisor.stop()` when stopping the whole managed set is a
meaningful branch inside the program, such as a user-requested pipeline
shutdown. Scope exit handles ordinary cleanup.

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

Everything important is in the type: the resource's lifetime is bound to the
`with` block; the wait outcome is a structured enum; recoverable failures are
returned; and cleanup runs on normal returns and runtime unwinding.

This applies the worker-pool shape from the previous case study to
subprocesses. When it becomes familiar, it becomes the default.
