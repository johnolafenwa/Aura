# Batch 6 Fresh-Eyes Corpus

- Date: 2026-07-31
- Status: complete
- Entry commit: `86ff95a`

## Goal

Write 30 new programs that a Python developer could reasonably attempt during
days one through five with Aurora. Exercise classes, strings, files, typed
failure, math, collections, comprehensions, slices, arrays, structured
concurrency, retry, and a small HTTP worker. Run every program through forced
MIR and forced direct execution.

## Method

- Programs are written from the maintained README, Manual, and tutorial track.
- Existing compiler tests, test fixtures, and maintained examples are not
  consulted while authoring the corpus.
- Each program is deterministic and records forced-backend status, stdout, and
  stderr.
- A success requires both backends to pass with identical observable output.
- Every failure is minimized and classified as a product bug, documented
  limitation, or documentation/usability friction.
- Product bugs are closed test-first. Accepted limitations receive an explicit
  migration hint.

## Work Allocation

- Programs 01-08: language foundations, classes, traits, ownership, and a
  small collection workflow.
- Programs 09-16: strings, files, bytes, JSON, process execution, `Result` /
  `try`, and data cleaning.
- Programs 17-23: comprehensions, slices, collection processing, and scalar
  numeric modes.
- Programs 24-30: numeric arrays, structured tasks and Queue work, retry, and
  loopback HTTP.

## Verification

The final consolidated replay used the repository `target/debug/aura` and ran
all 30 programs through:

```text
aura check <program>
aura fmt --check <program>
aura run --backend mir <program>
aura run --backend direct <program>
```

Results:

- 30/30 programs passed `check`.
- 30/30 programs passed `fmt --check`.
- 60/60 forced-backend executions exited successfully.
- MIR and direct stdout were byte-identical for all 30 programs.
- No program emitted a language diagnostic on its final replay.
- The loopback HTTP program used an ephemeral `127.0.0.1:0` listener and
  cleaned up successfully.

The consolidated machine-readable report is
`/private/tmp/aurora-fresh-eyes-final-20260731/results.tsv`, SHA-256
`0ff7a962c32116050c78685d881e1d3159d54ce19e74f3d39009c796dddbe13d`.
The complete program sources and lane-level stdout hashes are retained under
`work/fresh-eyes-corpus/`.

Focused closure gates:

- `cargo fmt --all -- --check`
- `cargo test -p aura --test cli
  fixed_width_integer_methods_match_forced_mir_and_direct_backends -- --exact
  --nocapture`
- `cargo test -p aura --test cli
  native_run_cache_verifies_artifacts_rebuilds_invalid_entries_and_keys_on_the_program
  -- --exact --nocapture`
- `npm run check:reference`
- `npm run docs:build`
- `npm run check:clippy`

All passed. The exact repository Clippy gate is production-targeted. An
additional, non-gating `--all-targets` probe exposed pre-existing test-only
lint debt under the installed Rust toolchain; none of those findings are in
the corpus changes or the release Clippy command.

## Findings and closure

### Documented ownership friction

Directly indexing `Map[int32, String]` produced `AU3005`, because a retained
map cannot implicitly copy or move its non-copy String value. The diagnostic
named both valid migrations (`get` for a cloned optional read and `remove` for
ownership transfer), and the final program followed the `get` path. Two
programs also initially tried to move non-copy enum payloads from a bare
shared match; the existing diagnostic and ownership documentation led directly
to `match own`. These are intentional ownership boundaries, not defects.

### String ordering documentation gap

`Vec[String].sort()` is not supported in Aurora 0.2 because String has no
built-in `Ord[String]`. The reference previously said only "orderable" and was
too easy for a Python reader to overgeneralize. The Manual, API index, Learn
guide, current-surface tutorial, and current-limits page now enumerate the
built-in ordered types and give three migrations: retain insertion order,
`sort_by` an ordered application key/index, or define a nominal type with a
domain-specific `Ord`.

### Narrow-integer MIR panic

The original program 23 made a checked `int16` method call with a contextual
integer literal:

```aurora
high: int16 = 30000
print(high.wrapping_add(10000))
```

Direct execution correctly printed `-25536`; MIR panicked because the
materialized literal retained its default `int64` runtime tag after static
checking had established an `int16` call. The repair was made test-first. MIR
now reapplies the statically checked receiver width to both operands, and an
unexpected mismatch returns controlled `AU4001` rather than panicking. The
parity regression pins all six wrapping/saturating `int16` boundary methods.

### Native-cache progress wording

The 30 direct runs initially printed `aura: rebuilding native runtime...`.
That was not a cache loop: they were 30 distinct cold program keys. Independent
proof serialized the same program's MIR byte-identically in 200 processes,
and an isolated cache produced one entry: the first run built it and the next
two reused it with empty stderr and identical stdout SHA-256
`e33a675b71c9dc9a7e32ee9f78ca92dff2496b92691cf246e81996c68106a70a`.
The apparent same-program repeats coincided with concurrent Cargo commands
replacing the canonical runtime archive with a different valid unit and SHA;
the runtime SHA is intentionally part of the content key.

The cache implementation is unchanged. The misleading progress text is fixed
test-first: a cold key now reports `aura: building native program...`.
Human, JSON, installed-runtime, concurrent-build, README, Manual, changelog,
and historical work-note contracts use the accurate wording.

## Follow-up

Proceed to the consolidated post-reboot performance story: fib(30), V6 loops,
10,000 tasks, TCP fan-out, retrying worker, and Array operations against
CPython/NumPy on the rebooted baseline host.
