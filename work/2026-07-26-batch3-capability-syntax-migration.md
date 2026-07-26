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
- ADR-0029 is Accepted with the B3.0-b function-wide per-loop binding-slot
  isolation amendment. This ratifies the decision; it does not by itself claim
  that the implementation or gates are complete.
- ADR-0030 becomes Accepted with the B3.0-d length-unification amendment.
- ADR-0031 remains Accepted.
- ADR-0022 is ratified by the Batch 3 brief but remains marked Proposed until
  its binding decisions and cross-ADR amendments are recorded with the
  migration change family.

## Current verification

- Batch 3 entry worktree: clean at `4929bab`.
- Repository-hygiene prerequisite `18b7f00` removed the committed trailing
  whitespace exposed by the first full gate. Part-0 ratifications are isolated
  in `19a10f4`, completed B3.0-a is isolated in `6afe47c`, and completed
  B3.0-b is isolated in `fc22696`. Completed B3.0-c is isolated in `e05c5e6`;
  B3.0-d is the active worktree and ticket.
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

## B3.0-b: heterogeneous `enumerate`/`zip` direct binding-slot reuse

- Decision: ADR-0029 is Accepted. The lowering rule applies to every `for`
  branch: each `Range`, `Vec`, `Set`, `Queue`, `enumerate`, or `zip` loop
  occurrence owns distinct typed target identities for its loop-body scope.
  Sequential loops may therefore reuse the same source binding names even when
  their element types differ. The mandated acceptance case is
  `zip(numbers, words)` followed by `zip(words, numbers)`.
- Required red repro: extend
  `crates/aurora-compiler/tests/fixtures/run-pass/enumerate_and_zip.au` with the
  reversed heterogeneous `zip` loop while reusing `number, word`. Before the
  fix, MIR runs the fixture but forced direct execution traps with `AU4001`
  because the old lowering collapses both loops onto function-wide
  source-named typed slots.
- Implementation: every lockstep and ordinary `for` branch now allocates fresh
  typed `%tN` leaves for its target and maps source names through a loop-local
  `scoped_names` frame only while lowering target initialization and the body.
  The iterable is evaluated first. Range terminators, Queue/Vec/Set payload
  extraction, recursive tuple leaves, and mutable-Vec writeback all carry the
  fresh physical slot rather than the repeated source spelling.
- Focused verification: the original forced-direct repro exited 1 with
  `AU4001` (`expected int32, found String`) while the forced-MIR run succeeded.
  Post-fix, 55 focused MIR tests pass, and forced MIR and forced direct both
  match the exact stdout for `enumerate_and_zip`,
  `tuple_for_pattern_queue`, and `vec_borrow_mut_iteration`. The last fixture
  pins fallthrough, `continue`, `break`, and explicit-return element writeback.
  Formatting and `git diff --check` pass.
- Coverage verification is green at `64,476/67,106` lines (96.08%),
  `4,162/4,299` functions (96.81%), and `94,558/100,270` regions (94.30%),
  above the frozen `96.07/96.81/94.29` floors. No synthetic coverage test or
  exclusion was added. Two genuinely unreachable defensive tuple-shape returns
  were restructured as guarded traversal, and the obsolete native fallback
  that invented an untyped Range binding was removed because every generated
  Range terminator now names its registered typed slot.
- Post-full-gate artifact hygiene found `target/` at 18 GiB with 152 GiB free,
  so neither repository cleanup threshold was crossed and the reusable
  profiles were retained.
- The exact full-repository `npm run ci` decision gate is green: 265 CLI tests,
  900 compiler tests, forced MIR/direct parity, all 70 language-server tests,
  all 13 extension tests, compiler and 100% LSP coverage, reference integrity,
  the documentation build, dependency audits, Clippy with warnings denied, and
  repository hygiene all passed. The Rust audit retains its already-allowed
  `rustls-pemfile` unmaintained warning and reports no vulnerability failure.
- Adjacent findings retained for the ADR-0022 scoping/exit-routing work:
  propagated `?` errors currently bypass mutable-Vec element writeback; nested
  mutable-Vec return redirects with an intervening `with` can omit an inner
  cleanup; and unrelated locals declared under separate loop bodies can still
  collide by raw source name. None is caused by the fresh target-slot patch,
  and none is represented as closed by B3.0-b.
- Status: complete. ADR-0029 is Accepted, all scoped binding-slot behavior is
  implemented and documented, and the exact full decision gate is green.

## B3.0-c: structural tuple equality and inequality

- Red evidence first pinned four defects or missing contracts: the checker
  rejected tuple `==` and `!=` with AU2003; runtime tuple equality compared
  transport type metadata as well as values; tuple ordering still emitted the
  obsolete generic comparison diagnostic; and a tuple comparison chain
  accepted a later mutable borrow of a non-copy tuple place still retained by
  the preceding link.
- The checker now symmetrically derives a move-free operand hint before actual
  left-to-right typing. Nested tuple literals therefore adopt the peer's exact
  recursive `Option`, integer-width, and floating-width types in either
  direction. After contextual typing, `==` and `!=` require one identical
  static tuple type and produce `bool`; bound tuples are never widened.
- Equality is builtin and non-consuming. Corresponding values compare
  recursively from left to right and `!=` is the negation. Tuple `<`, `<=`,
  `>`, and `>=` remain AU2003 with teaching guidance toward equality or
  explicit element comparisons. Different tuple types report AU2002.
- MIR lowering gives both equality operands one shared recursive tuple type
  rather than crossing independently inferred literal metadata. The same
  compile-time hint is carried through equality links without evaluating an
  operand early; the existing comparison-chain CFG still evaluates each
  operand at most once, left to right, and skips the suffix after the first
  false link.
- Chain checking now retains each non-copy builtin left operand through the
  following operand. Dedicated check-fail fixtures reject mutation of both the
  first tuple place and a middle tuple place while the relevant equality link
  still holds a shared read.
- `TupleValue` equality compares ordered payload elements only. Static tuple
  typing remains in `element_types` for checking and dispatch, but runtime,
  generic-specialization, transport, and backend metadata cannot change
  language value equality.
- The maintained run fixture covers nested and singleton values, both
  operators, symmetric nested `Option[int32]`/`float32` literals, exact
  non-copy operand reuse, successful and first-false chains, a non-copy
  `(String,)` middle operand, and the generic float32 metadata-divergence case.
  Forced MIR and forced direct execution produce the same exact stdout.
- Focused compiler equality, ordering, mismatch, runtime-value, check-pass,
  check-fail, and run-pass tests pass. All 71 language-server tests pass with
  bool hover/definition/reuse and exact ordering-diagnostic coverage. The
  executable reference inventory and all verified Manual examples pass against
  `target/debug/aura`; the tuple fence's stdout is unchanged and its one
  content hash was refreshed.
- ADR-0026 is Accepted with the B3.0-c amendment. The Manual, conformance and
  status pages, maintained example, root/example READMEs, and tuple tutorials
  now teach same-static-type recursive equality, non-consuming reads, symmetric
  literal context, chain behavior, metadata independence, and continued
  ordering rejection.
- Compiler coverage is green at `64,588/67,216` lines (96.09%),
  `4,176/4,313` functions (96.82%), and `94,731/100,444` regions (94.31%),
  above the frozen `96.07/96.81/94.29` floors. No synthetic coverage test,
  justified exclusion, or coverage-only branch was added.
- The exact full-repository `npm run ci` decision gate is green: 265 CLI tests,
  905 compiler tests, forced MIR/direct backend parity, all 71 language-server
  tests, all 13 extension tests, compiler coverage, 100% LSP coverage,
  reference integrity, the documentation build, dependency audits, Clippy
  with warnings denied, and repository hygiene all passed. The Rust audit
  retains its allowed `rustls-pemfile` unmaintained warning and reports no
  vulnerability failure.
- Post-full-gate artifact hygiene found `target/` at 19 GiB with 149 GiB free,
  so neither repository cleanup threshold was crossed and the reusable
  profiles are retained for B3.0-d.
- Status: complete and ready for the isolated B3.0-c decision commit.

## B3.0-d: `int64` length-surface unification

- Red evidence first established that an `int64` annotation rejected each
  member length as `int32`, while builtin `len(...)` was already `int64`.
  Focused fixtures then pinned all five member results, free/member equality,
  Unicode scalar versus UTF-8 byte counts, checked `int32` narrowing, and
  MIR/direct parity.
- `String.len()`, `String.byte_len()`, `Vec.len()`, `Map.len()`, and
  `Set.len()` now return `int64` consistently through call metadata, semantic
  checking, analysis and completion, MIR typing/runtime, direct lowering, and
  the LSP. The direct backend no longer emits an implicit `int32` range check
  for these results; an explicit `as int32` conversion remains checked.
- A late independent review found that MIR member dispatch cloned a complete
  place receiver before reading a length. Six clone-count regressions failed
  with one snapshot apiece for the five member forms and source-lowered free
  `len(...)`. A borrowed place fast path now reads every length without cloning
  or moving the receiver, preserving `String.byte_len()`'s O(1) contract and
  the existing no-argument diagnostics.
- The same review exposed duplicate builtin-member completion rows. A
  uniqueness regression first reproduced duplicate `Set.len`, then the
  broader Vec/Map/Set hardcoded-plus-catalog overlap. Catalog entries are now
  appended only when that member name is not already present. Compiler and LSP
  tests require unique observable completion rows and exact `int64` details.
- ADR-0030 is Accepted with the B3.0-d amendment. ADR-0004 records the two
  `int64` String length APIs, ADR-0020 describes the secure-random request
  ceiling, and ADR-0023 preserves the independent byte-codec output ceiling
  and `bytes.Error` `int32` payload ABI. Manual, Learn, README, examples, and
  tutorials now teach free/member type-and-value equality, Unicode scalar and
  byte counts, and explicit checked narrowing at retained `int32` boundaries.
  The reference gate positively freezes those contracts and rejects stale
  `int32` length signatures or prose.
- Resource limits were not widened. The codec implementation now uses
  `MAX_CODEC_OUTPUT_LEN` and `checked_codec_output_len`; secure randomness uses
  `RequestExceedsCeiling`. Oversized `bytes.Error` metadata retains a precise
  `AU4005` trap. Renamed run-fail fixtures pin the secure-random ceiling without
  describing it as a maximum representable Vec length.
- Count/index audit disposition:
  - already `int64`: free `len(...)` and `enumerate` positions;
  - migrated here: the five public String/collection member lengths;
  - deliberately retained as `int32`: Range endpoints and yields, Vec index
    inputs and internal cursors, and `WaitAny`/`WaitAll` indices, which form one
    coordinated index domain and use explicit checked casts from lengths;
  - deliberately retained as `int32`: `bytes.Error` offsets/lengths and JSON
    error line/column/depth payloads, which are bounded diagnostic ABIs;
  - deliberately retained: process supervisor restart counts/configuration,
    Queue capacity, bounded network/process byte-count inputs, HTTP status,
    process exit/signal codes, and `main` exit codes. Codes are not collection
    counts; the bounded operational parameters require their own coordinated
    API decision. Unlimited supervisor restart-count overflow remains a
    separate long-horizon audit item rather than an unreviewed breaking change.
- Focused verification is green: 916 compiler unit tests include the six
  zero-clone regressions; all nine fixture categories pass with local-network
  access; MIR and direct produce the same `len_and_str` oracle; all 72 LSP
  tests pass at 100% statements, branches, functions, and lines; all 13
  extension tests pass; reference integrity, executable fence hashes, docs
  build, formatting, and diff hygiene pass. Three independent final reviews
  report no remaining P0-P2 finding.
- Compiler coverage is green at `64,612/67,239` lines (96.09%),
  `4,179/4,315` functions (96.85%), and `94,761/100,470` regions (94.32%),
  above the frozen `96.07/96.81/94.29` floors. Every added test pins observable
  type, value, diagnostic, completion, allocation/ownership, or backend
  behavior; no synthetic coverage test, exclusion, or coverage-only branch was
  added.
- An earlier full-repository `npm run ci` attempt passed formatting, all Rust
  suites, the forced-backend parity matrix, all 72 LSP tests, all 13 extension
  tests, and both compiler and LSP coverage gates. It then exposed a reference
  guard whose fixed-string assertion crossed a Markdown line wrap. The guard now
  pins the same normative statement without depending on its line wrapping.
- The exact full-repository `npm run ci` decision gate is green on the committed
  tree: `cargo fmt --all --check`, 916 compiler unit tests, 265 CLI integration
  tests, every fixture and package suite, the forced MIR/direct runtime-fixture
  parity matrix in 516.80 seconds, all 72 language-server tests, all 13 extension
  tests, compiler coverage, LSP coverage at 100% statements, branches, functions,
  and lines, executable reference integrity, the documentation build, the npm and
  Rust audits, Clippy with warnings denied, and repository hygiene all passed. The
  Rust audit retains its already-allowed `rustls-pemfile` unmaintained warning and
  reports no vulnerability failure.
- The gate's compiler coverage is `64,612/67,239` lines
  (96.09304124094648%), `4,179/4,315` functions (96.84820393974508%), and
  `94,761/100,470` regions (94.31770677814274%), above the frozen
  `96.07/96.81/94.29` floors, which are not raised during the migration.
- Coverage-only output was cleaned before the gate; `target/` was 18.9 GiB with
  about 146 GiB free, so neither repository cleanup threshold was crossed.
- Status: complete and ready for the isolated B3.0-d decision commit.

## Follow-up

B3.0-e diagnostic and comment polish is next, followed by the ADR-0022
inventory, migrator, and capability-syntax migration. Continue recording the
syntax-aware source-inventory counts, migration results, and checkpoint
disposition here as the batch advances.
