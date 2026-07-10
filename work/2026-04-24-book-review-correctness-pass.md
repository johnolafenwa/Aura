# April 24 Book Review Correctness Pass

## Goal

Respond to the external newcomer-focused review of the Aurora VitePress book and bring the highest-traffic examples and reference contracts back in line with the current compiler.

## Work Completed

- Removed invalid call-site `borrow` and `borrow mut` from examples and reference text. Borrowing remains declared at parameter, receiver, loop, and match positions where the compiler supports it.
- Collapsed runnable Aurora calls and collection literals that used unsupported multi-line continuation.
- Replaced short-form `case Some(...)` / `case None` examples with `Option.Some(...)` / `Option.None`.
- Rewrote top-level `try` snippets into function bodies or explicit `match` handling, because `try` is only valid inside functions.
- Corrected `Vec.insert` and `Vec.swap` contracts to document runtime traps on out-of-bounds indexes.
- Rewrote the install chapter around `git clone`, `cargo build --release -p aura`, and a reusable `$AURA` release binary.
- Added a concrete homepage definition of Aurora and updated quick commands to use the release binary.
- Expanded `manual/current-limits.md` with continuation, match-arm, loop-shadowing, duration, floating-point, HTTP-cap, and detached-task limits.
- Removed the positive `spawn detached` documentation and replaced it with the current structured `TaskGroup` guidance.
- Switched Aurora source fences in the book from plain `text` to `python` highlighting until an Aurora grammar exists.

## Verification

- Ran the homepage, Learn overview, Small Programs, and log-analyzer snippets through `./target/debug/aura run`.
- Ran the revised process examples through `./target/debug/aura check`.
- Verified `Vec.insert` and `Vec.swap` out-of-bounds behavior traps at runtime.
- `npm run docs:build`
- `npm audit --audit-level=moderate`
- `git diff --check`
- checked book Markdown files for trailing whitespace

## Follow-up

- The book now avoids the systematic invalid syntax from the review. A future pass should add an automated docs-snippet harness that distinguishes complete Aurora programs from illustrative fragments so snippet drift is caught by CI instead of external review.
