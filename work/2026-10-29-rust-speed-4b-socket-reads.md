# Rust Speed Step 4b: Socket Reads

Date: 2026-10-29

## Problem

`read_line` on a `net` TCP or Unix stream read one byte per `recv`, so a
20-byte request line cost 21 system calls, the last one reporting that it
would block. Each wait on a socket then added the descriptor to the
runtime's poller and removed it again on wake: two `epoll_ctl` calls per
wait. `kv_server` ran in 1.6x Rust's wall time and `pubsub_broker` in
2.4x. The Rust versions (tokio) make one `recv` and one `send` per
message and no poller calls.

The sockets were already nonblocking and their waits already went through
the reactor, so the step did not need a new I/O path.

## Changes

- `TcpStreamState` and `UnixStreamState` hold a `ReadAheadBuffer`.
  `read_line` reads up to 4 KB into it and returns the first line,
  enforcing the same length limit as before. The `ReadAhead` adapter
  serves those bytes first to `read_bytes`, `read_exact`, and `read_all`,
  so no read sees bytes out of order. `close` drops them.
- A read that returns fewer bytes than asked marks the buffer drained. The
  next `read_line` that needs more bytes waits for readiness first instead
  of reading into `EAGAIN`. This is only a hint: the wait checks readiness
  when it registers.
- The reactor keeps a descriptor registered when its last waiter leaves.
  The next wait re-arms it with `reregister` (one `epoll_ctl`), which also
  reports readiness the socket already has. If the descriptor was closed
  meanwhile, `reregister` fails, and the descriptor is registered afresh
  under a new token, so events for the old file cannot wake the new
  waiter.

Per message, `kv_server` now makes one `recv`, one `send`, and one
`epoll_ctl` (strace, 10,000 commands: 20,077 `recvfrom`, 20,040
`sendto`, 20,099 `epoll_ctl`).

## Measurements

Wall time at the bench size, Linux container, median of 7 (5 before):

| Program | Rust ms | Before (Aura / Rust) | After (Aura / Rust) |
| --- | ---: | ---: | ---: |
| `kv_server` | 1,632 | 1.6x | 1.2x |
| `pubsub_broker` | 1,836 | 2.4x | 1.4x to 1.5x |

Callgrind instructions at check size, base = step 4a. All 30 programs
print the same output.

| Program | Before | After | Change |
| --- | ---: | ---: | ---: |
| `pubsub_broker` | 68.5M | 61.8M | -9.8% |
| `kv_server` | 44.5M | 40.8M | -8.4% |
| `http_api` | 26.5M | 26.4M | -0.6% |

No other program moves by more than 0.3%.

## Gate

`kv_server` meets 4b's gate (at most 1.25x). `pubsub_broker` misses it:
its syscalls now match Rust's, and what remains is the broker's own string
splitting, list appends, and joins, which Phase 5 addresses.
