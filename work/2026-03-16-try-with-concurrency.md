# 2026-03-16: Try, With, Mutating Receivers, And Bootstrap Concurrency

## Summary

Completed the requested language slice across the compiler, runtime, examples, tutorials, and editor tooling:

- fuller mutating receiver semantics
- `try expr`
- `with`
- bootstrap channels and spawned tasks

## Compiler And Runtime

- Added member-target assignment in the AST and parser.
- Added checker/runtime support for assignment through field paths such as `self.value = ...` and `self.value += ...`.
- Made `mut self` methods mutate the original receiver place instead of a throwaway copy.
- Added `try expr` as a real expression that unwraps `Result.Ok(...)` and returns early on `Result.Err(...)`.
- Added `with name = expr:` with deterministic cleanup through `close(mut self)`.
- Added built-in generic `Channel[T]` and `Task[T]` handling in the checker.
- Added runtime support for:
  - `channel()`
  - `send`
  - `recv`
  - `close`
  - `spawn`
  - `join()`

## Tests

Added fixture coverage for:

- mutating methods
- invalid mutable receiver use
- `try` success and propagation
- invalid `try` outside `Result` return contexts
- `with` cleanup on early return
- invalid `with` resources without `close`
- typed channel creation and spawned producer tasks
- invalid `channel()` usage without an expected type

## Examples And Tutorials

Added or updated:

- `examples/classes/mutating_methods.au`
- `examples/error_handling/try_result.au`
- `examples/resources/with_resource.au`
- `examples/concurrency/channels_spawn.au`
- `tutorials/05-classes-and-data.md`
- `tutorials/10-resource-management.md`
- `tutorials/11-error-propagation.md`
- `tutorials/12-concurrency.md`

## Tooling

- Updated the language server with:
  - `channel` builtin completion
  - `Channel` and `Task` member completions
  - `with`-bound local tracking
  - `try` / `spawn` expression inference
- Added regression tests against the new examples.

## Verification

- `cargo test`
- `cargo run -p aura -- run examples/error_handling/try_result.au`
- `cargo run -p aura -- run examples/resources/with_resource.au`
- `cargo run -p aura -- run examples/concurrency/channels_spawn.au`
- `npm run test:lsp`
- `npm run check:extension`
- `npm run test:extension`
