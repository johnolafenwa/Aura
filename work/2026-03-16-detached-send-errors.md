# 2026-03-16 Detached Tasks and Send Errors

## Scope

Completed the next concurrency slice after task groups:

- `spawn detached`
- `Channel.send() -> Result[None, SendError[T]]`
- built-in `SendError[T]` with `SendError.Closed(T)`
- `select` send arms in addition to receive and timer arms

## Compiler and Runtime

- Extended the AST, lexer, and parser for `spawn detached`.
- Updated semantic checking so:
  - detached spawn expressions type-check as `None`
  - `SendError[T]` is a built-in generic enum-like type
  - channel `send()` returns `Result[None, SendError[T]]`
  - `select` accepts `channel.send(value)` arms
- Updated the interpreter so:
  - detached tasks run without returning a `Task[T]` handle
  - detached tasks do not inherit the current cancellation context
  - closed-channel sends return `Result.Err(SendError.Closed(value))`
  - successful sends return `Result.Ok(None)`
  - `select` can choose send, receive, or timer arms

## Examples and Fixtures

Added:

- `examples/concurrency/send_result.au`
- `examples/concurrency/spawn_detached.au`
- `examples/concurrency/select_send.au`

Added matching parse/check/run fixtures plus a new negative fixture:

- `check-fail/spawn_detached_task_type.au`

## Tooling

- Updated the Aurora language server for:
  - `detached` keyword completions
  - `SendError` builtin enum support
  - `Channel.send()` result-type inference
  - hover for send-result bindings
- Updated VS Code syntax highlighting and snippets for the new concurrency forms.

## Verification

- `cargo test`
- `cargo run -p aura -- run examples/concurrency/send_result.au`
- `cargo run -p aura -- run examples/concurrency/spawn_detached.au`
- `cargo run -p aura -- run examples/concurrency/select_send.au`
- `cargo run -p aura -- mir examples/concurrency/select_send.au`
- `npm run test:lsp`
- `npm run check:extension`
- `npm run test:extension`
- `npm run package:extension`
