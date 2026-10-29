# Rust Speed Step 4c: HTTP

Date: 2026-10-29

## Problem

Every HTTP/1.1 step (parsing a request or response head, decoding a
chunked body, writing a response, building a client request) ran as a
"protocol step" on the runtime's service thread. Each step copied its
input buffer, handed it across threads, and woke the calling task when it
finished. The steps themselves take microseconds, so the hand-off was the
cost: `http_api` serving 1,000 requests made 21,636 `futex` calls and
3,440 eventfd `write` calls, and ran in 3.2x Rust's wall time.

Client connects to a literal address also went to the blocking pool.

## Changes

- `run_http_step_before` checks the deadline and cancellation, runs the
  HTTP step inline on the calling task, and checks again. It replaces the
  protocol-step hand-off for request and response head parsing, chunked
  body decoding, response writes, and client request building. None of
  these copies its buffer any more. TLS and WebSocket protocol steps stay
  on the service thread.
- On unix, `TcpStreamValue::connect_before` connects a literal
  `host:port` address with a nonblocking `connect` and waits for
  writability on the reactor, reporting any socket error the connect left.
  Names that need DNS still resolve on the blocking pool.

With 1,000 requests, strace shows 39 `futex` calls (from 21,636) and
almost no eventfd writes.

## Measurements

Wall time at the bench size, Linux container, median of 7:

| Program | Rust ms | Before (Aura / Rust) | After (Aura / Rust) |
| --- | ---: | ---: | ---: |
| `http_api` | 288 | 3.2x | 1.1x |

Callgrind instructions at check size, base = step 4b. All 30 programs
print the same output.

| Program | Before | After | Change |
| --- | ---: | ---: | ---: |
| `http_api` | 26.5M | 19.2M | -27.6% |

No other program moves by more than 0.7%.

## Gate

`http_api` meets 4c's gate (at most 1.25x Rust).
