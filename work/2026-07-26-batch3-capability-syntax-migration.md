# Batch 3 defect closure and capability-syntax migration

## Goal

Complete Batch 3 of 6 at its requested checkpoint: close B3.0-a through
B3.0-e in separate test-first commits, ratify and implement ADR-0022 as one
coordinated capability-syntax migration, run the post-migration gates and
coverage re-ratchet, and stop without beginning Phase 5.

## Authorized order

1. Verify cached native artifacts by their own content before execution.
2. Fix heterogeneous `enumerate`/`zip` direct binding-slot reuse.
3. Add structural tuple equality and inequality.
4. Unify the maintained collection/string length-and-count surface on `int64`.
5. Land the four required diagnostic and comment polish items.
6. Publish the ADR-0022 source inventory before the syntax flip.
7. Implement and migrate bare shared, `mut`, and `own` capability syntax.
8. Run the checkpoint parity, reference, coverage, LSP, extension, docs,
   audit, Clippy, hygiene, and complete repository gates.

Phase 5 is outside this target and must not begin.

## Entry decisions

- ADR-0018, ADR-0019, ADR-0020, ADR-0021, ADR-0023, ADR-0024, ADR-0025,
  ADR-0027, and ADR-0028 are Accepted as implemented.
- ADR-0026 becomes Accepted with the B3.0-c tuple-equality amendment.
- ADR-0029 remains Provisional until B3.0-b closes.
- ADR-0030 becomes Accepted with the B3.0-d length-unification amendment.
- ADR-0031 remains Accepted.
- ADR-0022 is ratified by the Batch 3 brief but remains marked Proposed until
  its binding decisions and cross-ADR amendments are recorded with the
  migration change family.

## Current verification

- Entry worktree: clean at `4929bab`.
- Repository-hygiene prerequisite `18b7f00` removed the committed trailing
  whitespace exposed by the first full gate. Part-0 ratifications are isolated
  in `19a10f4`; the remaining worktree is B3.0-a plus this active tracking.
- Batch 2 checkpoint gate: green at the recorded
  `64,409/67,039` lines, `4,158/4,295` functions, and
  `94,472/100,184` regions, with enforced floors
  `96.07/96.81/94.29` and LSP coverage at 100%.
- Old coverage-only artifacts were cleaned before Batch 3; source and
  dependency caches were preserved.

## B3.0-a: native artifact-cache integrity

- Failing repro: the strengthened CLI cache regression failed because the old
  entry was a bare executable with no recorded artifact digest.
- Each v3 cache entry now atomically publishes `program`, `program.sha256`, and
  a key-bound unique `entry-id`. A hit uses bounded no-follow reads, verifies
  the entry identity and artifact SHA-256, validates regular-file structure,
  requires the cached artifact's execute permission, enforces a 512 MiB cache
  bound, and checks the current platform's native executable magic.
- The verified bytes, rather than the shared cache pathname, are materialized
  in a private per-launch directory and invoked through raw Unix `execv`.
  Replacing a cache entry after verification therefore cannot substitute the
  launched bytes, and malformed native-looking bytes return an execution error
  rather than receiving macOS's `execvp` shell fallback.
- A missing or mismatched digest, truncated artifact, non-regular member, lost
  execute permission, or digest-matching file rejected for executable format
  or architecture is quarantined by exact entry identity and handled as a
  cache miss. A stale invalidator cannot delete a concurrently published
  replacement. Temporary-directory, process-resource, `noexec`, and other
  environmental launch failures preserve verified cache state and follow the
  selected backend's normal error/fallback policy.
- Cache roots are an explicit trust boundary. Unix requires current-user
  ownership, rejects group/world write access, and tightens accepted
  directories to `0700`, including every component created under a permissive
  umask. Private launch copies are removed after normal exit. An inherited
  exclusive lease keeps an interrupted parent from allowing cleanup to unlink
  a still-running native child's executable. Memo, publication, quarantine,
  and launch leftovers are collected only after exact-name, owner-liveness,
  age, or lease checks establish that they are not live. The runtime-archive
  memo now includes Unix file identity and ctime as well as length/mtime.
- A second red product repro removed the old settle-and-delete workaround:
  after a cold source-checkout build refreshed the runtime archive, the
  immediate poisoned-linker warm run missed because the entry had been stored
  under the pre-build archive identity. Cold publication now keys on the exact
  archive bytes and ordered native link arguments that reached the linker,
  writes that authoritative identity for Cargo-free warm lookup, and packaged
  runs select the sibling installed runtime rather than an unrelated workspace
  archive.
- The CLI regression observes the stored digest itself, a verified hit without
  a rewrite, a digest-only mismatch rebuild, a truncated artifact rebuild, a
  lost-execute-permission rebuild, a matching-digest malformed-native rebuild,
  changed-program keying, and separation of runtime bookkeeping from program
  entries. Additional behavioral regressions pin symlink and FIFO rejection
  without blocking, successful launch-temp cleanup, preservation after
  unusable-TMPDIR failure, current-user cache-root permissions, exact-entry
  invalidation under replacement, and same-size/same-mtime runtime-archive
  replacement. The malformed-native repro failed first by proving that the old
  path passed the bytes to a shell on macOS.
- The `sha2` dependency is optimized in the development profile because every
  cache hit must hash a large artifact. With one hash and no redundant
  post-copy hash, three resident verified hits measured `0.81s` each on this
  workstation; the first verified hit measured `2.80s`. These replace the old
  pre-verification resident-cache assumption, while still avoiding a native
  compile and link.
- The first full gate passed behavior, forced parity, LSP, extension, compiler
  and LSP coverage, reference integrity, docs, audits, and Clippy. It exposed
  committed trailing whitespace in `personal/file_ops.au` through the final
  hygiene check. The non-semantic baseline repair is isolated in prerequisite
  commit `18b7f00`, because the hygiene gate checks committed `HEAD`; B3.0-a
  remains a separate commit after its strengthened exact-tree rerun.
- That coverage pass remained above the frozen floors at 64,410/67,039 lines,
  4,158/4,295 functions, and 94,473/100,184 regions. Coverage-only output was
  then cleaned; because the remaining accumulated build tree still exceeded
  20 GiB, `cargo clean` reclaimed 26.6 GiB before the next full gate.
- Focused verification:
  `cargo test -p aura --test cli native_run_cache_verifies_artifacts_rebuilds_invalid_entries_and_keys_on_the_program -- --test-threads=1`
  plus the non-regular-member, environmental-launch-failure, entry-replacement,
  cache-root trust and permissive-umask behavior, runtime-memo replacement,
  exact runtime-input identity, inherited child leases, exact stale-stage
  cleanup, and backend-selector regressions pass. The no-settle cold-to-warm
  run and valid-but-wrong-key `entry-id` repair are both pinned by a poisoned
  linker warm hit.
- The strengthened exact-tree `npm run ci` decision gate passes: 265 CLI
  integration tests, 897 compiler unit tests, every fixture and package suite,
  the forced MIR/direct runtime-fixture matrix, all 70 language-server tests,
  all 13 extension tests, compiler and LSP coverage, reference integrity, docs
  build, npm and Rust audits, Clippy with warnings denied, and repository
  hygiene are green. The first sandboxed run's 24 loopback failures were an
  execution-environment restriction; the authorized exact same tree passed all
  TCP, UDP, HTTP, TLS, Unix-socket, and WebSocket cases.
- Final B3.0-a compiler coverage is `64,410/67,039` lines (96.08%),
  `4,158/4,295` functions (96.81%), and `94,473/100,184` regions (94.30%),
  above the frozen `96.07/96.81/94.29` floors. LSP statements, branches,
  functions, and lines remain at 100%. No synthetic coverage test, exclusion,
  or coverage-only production branch was added.
- Post-gate artifact hygiene found `target/` at 14 GiB with 157 GiB free, so
  neither repository cleanup threshold was crossed and the reusable profiles
  were retained for the next ticket.

## Follow-up

Proceed to B3.0-b. Continue recording failing repros, implementation details,
focused verification, exact full-gate evidence, coverage measurements,
source-inventory counts, migration results, and checkpoint disposition here as
the batch advances.
