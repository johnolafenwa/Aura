# Phase 4 content-addressed native artifact cache

## Goal

Add the content-addressed cache for `aura run`'s native path, keyed by
compiler and runtime version, target, backend, source, and dependencies, and
record the cold-compile and warm-launch benchmarks.

## Work Completed

- Keyed each cached binary by a SHA-256 over the cache format tag, this
  compiler's version, the host architecture and OS, the backend, the runtime
  archive's identity, and the complete lowered program. Lowering already
  incorporates the entry source and every resolved dependency source, so
  hashing the module covers the whole dependency set exactly rather than
  re-walking it and risking a missed input.
- Made the key fail closed: if any input cannot be captured, the key is `None`
  and the run compiles normally rather than reusing a binary under a key that
  omits an input.
- Identified the runtime archive by content, memoized against its cheap
  `(length, mtime)` stamp. Modification time alone is unusable here because a
  direct build re-runs the runtime's `cargo` invocation and can restamp an
  archive whose bytes did not change, which would invalidate every cached
  program on the next run. The memo keeps the expensive read to the rare case
  where the archive genuinely changed.
- Published entries atomically: each binary is written under a unique staged
  name and renamed into place, so a concurrent run sees either no entry or a
  complete one and never launches a half-written executable. A publish failure
  is not a build failure; the run continues with the binary it already has.
- Separated program entries from the cache's own bookkeeping by placing
  binaries under `programs/`, so the runtime-identity memo can never be
  mistaken for a content key.
- Honored `AURORA_CACHE_DIR`, defaulting to `~/.cache/aurora/native`, so a
  sandbox or a test keeps its own cache.

## Benchmarks

Measured on the development workstation with a hello-world program, after the
runtime archive had settled. Each figure is the median of three runs.

| Path | Wall clock |
| --- | --- |
| `--backend mir` | 0.00s |
| `--backend direct`, cold compile and link | 1.31s |
| `--backend direct`, warm launch, first touch of a fresh binary | 0.81s |
| `--backend direct`, warm launch, binary resident | 0.01s |

The cache removes the compile and link entirely on a hit. What remains on a
first touch is loading the binary itself: a direct hello-world executable is
about 57 MB because it statically links the whole runtime, so its first launch
is dominated by page-cache misses. Once resident it matches the MIR runtime.

## Decision: the default still stays `mir`

The cache removes the steady-state cost but not the first-run cost. A cold
miss is still about 1.3 seconds, and both CI and the test suites are dominated
by programs each seen once, so every one of them would pay a cold miss. The
default therefore remains `mir`, and the named constant carrying that decision
now records the cache measurements alongside the original reasoning.

The remaining blocker for a native default is binary size rather than compile
time: until a direct binary is small enough that its first launch is cheap,
`auto` cannot be the default for one-shot programs.

## Verification

- A CLI product test settles the runtime archive, clears the cache, and then
  pins that a cold run publishes exactly one entry under a non-staged name,
  that a warm run reuses it rather than publishing another, that changing the
  program keys to a second entry rather than launching the stale binary, and
  that the runtime-identity memo is recorded outside the program directory.
- The complete `npm run ci` gate is green before the commit, with compiler
  coverage at 64,396/67,022 lines (96.08188356062189%), 4,155/4,292 functions
  (96.80801491146319%), and 94,455/100,165 regions (94.29940598013278%), above
  the frozen 96.06/96.79/94.15 floors. No synthetic coverage test or exclusion
  was added.
- Those figures are transcribed from that gate into this note and the task
  board, and the resulting documentation-only delta was re-verified with
  `cargo fmt --all --check`, `git diff --check`, `npm run check:reference`,
  `npm run docs:build`, `npm run check:hygiene`, and `npm audit`.

## Follow-Up

- Revisit the default if direct binaries become small enough for a cheap first
  launch.
