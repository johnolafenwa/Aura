# Batch 1: Pre-reference Punch List and Phase 2 Reference Freeze

## Batch boundary

- Batch boundary: do not begin Batch 2 or Phase 3; V6 is deferred to Batch 2 with Phase 4.

## Goal

Complete P1-P5, introduce stable structured diagnostics and MIR runtime call/task backtraces, turn the Manual into the fully executable normative post-Phase-1.5 reference, freeze that reference, pass a full gate before every independently revertable commit, and perform the one authorized downward-truncated coverage re-ratchet at the checkpoint.

## Work completed

- Added the compiler-owned stable diagnostic model: append-only banded `AU####`
  codes, primary and labeled secondary spans, notes, help, and
  machine-applicable edits; human rendering now includes the code.
- Added schema-versioned JSON diagnostic output for `aura check`, `run`, and
  `build` compile failures and carried the same structure through compiler
  analysis into the LSP bridge instead of creating a parallel editor path.
- Replaced the stale diagnostics chapter with the normative code registry,
  JSON and LSP contracts, coordinate rules, ownership-guidance contract,
  Python-migration policy, runtime-backtrace contract, and stability policy.
- Added the dedicated Python-migration fixture family covering all Batch 1
  hint categories. Lexer, parser, and checker diagnostics now pin each message
  and stable code, including the new double-quoted f-string and
  `mut self` guidance.
- P1: implemented exact float-context typing for positive and negative
  unsuffixed integer literals in annotated bindings, binary operands, concrete
  call arguments, defaults, and returns. Inexact `float32`/`float64` cases are
  static `AU2002` errors naming the value and both explicit-rounding exits;
  explicit `as float32`/`as float64` remains an exactness-checked cast rather
  than being absorbed into contextual typing. MIR lowering emits floating
  operands only for concrete floating expectations, preserving generic
  argument inference and the existing specialized-trait dispatch surface.
- P2: replaced the lossy float32 round-trip heuristic used for float64 display
  with source-type-specific shortest-roundtrip rendering. MIR `print` selects
  the renderer from the argument's static type and direct codegen calls
  distinct float32/float64 runtime entry points. The stdout audit corrected the
  baked-in rounded `2^63` cast oracle to `9.223372036854776e18`.
- P3 preferred path: reproduced the confirmed divergence where semantic checking and direct execution accepted a user trait method on `Queue[T]` or `Task[T]`, but MIR runtime dispatch intercepted the call as a builtin-handle method and rejected the unknown member. Because lowered MIR already retained the static receiver type and the sema-resolved trait implementation, the fix remained contained to MIR runtime dispatch: non-builtin Queue/Task member names now fall back to that resolved user implementation, preserving builtin precedence and diagnostics. Added generic Queue and Task run-pass fixtures so both cases are automatically included in the forced MIR/direct parity matrix; no checker-rejection fallback was required.
- P4: recorded the intentional ownership-spelling asymmetry in ADR-0006 and the normative Manual: parameters place `own` in the type position, parallel to ``, while loops prefix the iterable because they have no type position.
- Completed the normative contract pass for CLI/tooling, control-plane, filesystem, standard I/O, networking, packages, and processes. Each feature page now has the required grammar, typing, runtime, ownership/evaluation, stable-diagnostic, backend, limit, and status sections; implemented behavior, known defects, host-defined limits, and unavailable future work are explicitly separated. Existing example fences were preserved unchanged. The pass also aligned the CLI analysis-field reference with the shared structured diagnostic schema.
- Added MIR runtime trap backtraces. Each propagating diagnostic is annotated once with the innermost-first Aurora function chain and declaration spans; a failing structured child also carries its task entry plus exact spawning function/call-site ancestry. The diagnostic notes flow through structured output and human rendering. Native direct-backend frame capture is deliberately deferred to Batch 3, so the forced parity comparator temporarily removes only `Aurora call chain`, `Aurora task entry`, and `Aurora task ancestry` note lines while continuing to compare the complete primary trap code/message/span.
- Corrected the diagnostics-band gap for invalid UTF-8 returned through `process.Completed.stdout()` or `stderr()`: both MIR and direct paths now construct `AU4005` explicitly rather than falling through to `AU2999`, with the existing message unchanged.
- Added provisional ADR-0014 after the normative collections pass exposed an
  underdetermined Map contract. Map literals now evaluate entries left to
  right and let the last equal key's value win without moving the first key's
  insertion slot; non-copy indexed reads are rejected in favor of explicit
  `get`/`remove`; and missing indexed keys trap as `AU4003` on both backends.
  Existing String-valued indexed reads in the maintained control-plane fixture
  and HTTP example now use explicit `get`/`match` handling.
- Added provisional ADR-0015 after the reference audit exposed the missing
  relative order between supplied arguments and omitted defaults. Functions,
  methods, class construction, and named enum construction now evaluate every
  supplied expression in call-site order before omitted defaults in declaration
  order. Named enum payloads are then bound into declaration slots. MIR and
  direct execution now agree on both observable order and payload positions.
- Added provisional ADR-0016 after the left-to-right evaluation audit exposed
  a representation-sensitive gap for non-copy places. Copy places are captured
  at their sequence point; f-string interpolations render immediately; and a
  non-copy place selected as a binary operand, index base, method receiver, or
  indexed-assignment target remains borrowed through the operation inputs.
  Overlapping mutation or consumption is rejected as `AU3002` instead of being
  implemented with a hidden deep clone or backend-divergent alias behavior.
- Added provisional ADR-0017 after the loop reference pass exposed an
  underdetermined source-selection point. ADR-0006 defines loop ownership modes
  and Queue's receive carve-out but does not say whether rebinding the iterable
  place can retarget an active loop. Aurora now selects the iterable once at
  loop entry: `own` Vec/Set iteration moves into a loop-private source and bare
  Queue iteration copies its handle. This supplements rather than amends the
  accepted ownership modes, and closes a contained MIR/direct divergence.
- Corrected MIR sequence-point lowering so copy operands, literal elements,
  call arguments, and constructor arguments preserve the selected value even
  when a later expression mutates the source place. Compound assignment now
  captures the old target before evaluating its right-hand side, including
  projected targets, and f-string interpolation observes each value at its
  source position without cloning non-copy values.
- Routed compound assignment through the same builtin-or-trait operator
  lowering as its binary-expression counterpart. User-defined `Add`
  implementations now execute for both root and projected `+=` targets on MIR
  and direct backends; non-copy targets remain retained rather than being
  copied into an implicit MIR snapshot.
- Rejected compound indexed assignment through `Map[K, V]` when `V` is
  non-copy, because the read-modify-write step would otherwise imply a hidden
  copy. The diagnostic points users to `get` or `remove` followed by an
  explicit write-back.
- Under provisional ADR-0017, corrected Queue iteration to capture its copy handle once when the loop is
  entered. Rebinding the source variable inside the body no longer switches
  subsequent receives to a different Queue, while the handle remains
  intentionally unfrozen under the receive-based G2 carve-out.
- Under provisional ADR-0017, corrected `own` Vec and Set iteration to move the selected collection into a
  loop-private source once at entry. Reinitializing the consumed source binding
  inside the body no longer switches or truncates the active iteration.
- Made declaration-stable parameter passing drive both directional call-boundary
  checking and MIR argument lowering. Explicit or generic-specialized shared
  and mutable borrows remain retained even for copy types; value slots capture
  copy places or move non-copy places; and a prior exclusive borrow or move now
  rejects a later overlapping plain place read as well as a later borrow or
  move. TaskGroup arguments retain their distinct outer value-capture boundary.
- Applied the resolved receiver and right-parameter modes of user operator
  traits to unary, binary, and compound expressions. `own` operands move,
  `mut ` receivers require mutable places and write back, shared copy
  receivers remain borrows, and legal value-mode copy receivers snapshot before
  later operand effects.
- Extended retained indexed-assignment checking through both the key/index and
  value sequence points, including non-copy key consumption that overlaps the
  selected mutable container target.
- Completed ownership-diagnostic provenance through the checker, compiler
  analysis, JSON model, and LSP bridge. Invalid later uses retain the move,
  borrow, or loop-freeze origin as a labeled secondary span and carry teaching
  help plus machine edits only for safe source shapes.
- Reconciled accepted ADR-0010 with the parser: equality, ordering, and mixed
  comparison chains are all rejected with the current `and` migration
  guidance, while explicit parenthesized boolean comparisons remain distinct.
- Pinned the existing positive-`int32` Queue-capacity contract with zero and
  negative run-fail fixtures on both backends and normalized the canonical
  diagnostic spelling to `Queue(capacity=...)`.
- Normalized all errors crossing MIR or direct runtime boundaries into the
  runtime diagnostic band while retaining explicitly precise `AU40xx` codes.
  Spanless direct-runtime rendering now includes its stable code as well.
- Audited all 29 Manual pages. All 19 feature pages now contain the exact eight
  required normative sections and at least one verified executable example.
  The strengthened fail-closed integrity gate inventories all 226 fences,
  hash-pins every classification, safely verifies 101 contracts, and requires
  a specific reason for each of the 125 illustrative blocks.
- Kept the richer compiler diagnostic value below Clippy's large-error
  threshold by boxing its supplemental spans, guidance, edits, and render
  context behind one internal detail object. Existing field access and the
  public structured JSON schema are unchanged. The Queue/Task trait-dispatch
  fallback inputs are likewise grouped in a private context object, leaving
  the strict `-D warnings` gate green without adding lint allowances.

## Verification

- `cargo test -p aurora-compiler --test fixtures run_pass_fixtures_match_expected_stdout -- --exact` passes with the new P1/P2 fixtures and the corrected float oracle.
- `cargo test -p aura --test cli float_context_integer_literals` passes its forced MIR/direct behavioral pin, including exact `float32`/`float64` boundaries, both operand orders, negative literals, calls, defaults, returns, and the required floor/remainder cases.
- `cargo test -p aura --test cli shortest_roundtrip_float_printing` passes its forced MIR/direct oracle for `2^53`, `2^53 +/- 1` through `.to_float()`, `1e300`, `1e-300`, `-0.0`, `0.1 + 0.2`, and float32 source precision.
- `cargo test -p aurora-compiler --test fixtures run_fail_fixtures_match_expected_diagnostics -- --exact` passes, including the regression proving an inexact integer-literal `as float64` remains a runtime cast rather than contextual literal typing.
- `cargo test -p aurora-compiler --test fixtures run_pass_fixtures_match_expected_stdout -- --nocapture` passes, including the generic Queue and Task trait-dispatch regressions under MIR execution.
- Focused `aura build --backend direct` builds and executions of both new fixtures pass with `queue trait` and `task trait` respectively.
- `cargo test -p aura --test backend_parity -- --ignored --test-threads=1 --nocapture` passes the repository-wide forced MIR/direct runtime-fixture matrix with both new fixtures auto-enumerated (1 passed in 271.96s).
- `cargo test -p aurora-compiler --test mir_backtraces -- --nocapture` passes two focused regressions covering a three-function synchronous trap and an unobserved TaskGroup child trap (2 passed).
- `cargo test -p aura --test mir_backtraces -- --nocapture` passes the CLI human-rendering regression for both call-chain and task-ancestry notes (1 passed).
- `cargo test -p aura --test backend_parity primary_runtime_diagnostic_normalization_ignores_only_deferred_mir_backtrace_notes -- --nocapture` passes and proves the temporary comparator exception retains every non-backtrace note.
- `cargo test -p aurora-compiler --test fixtures run_fail_fixtures_match_expected_diagnostics -- --nocapture` passes all runtime-trap oracles with only the three documented supplemental MIR note families normalized.
- Focused MIR task-detection and native-codegen task-start/type-inference unit tests pass after adding the serialized task spawn span; `cargo test -p aurora-compiler --lib --no-run` also passes.
- `cargo test -p aura --test process_diagnostic_codes -- --nocapture` passes a parity-focused product regression for invalid UTF-8 from both `process.Completed.stdout()` and `stderr()` under MIR and forced direct execution (1 passed in 99.33s).
- `cargo test -p aura --test packages deps_update_preserves_the_compiler_diagnostic_code -- --exact` passes, pinning stable human diagnostic codes on compiler-owned package resolver failures.
- `cargo test -p aurora-compiler --test fixtures -- --nocapture` passes all
  seven fixture families after the lexical/parser migration-code pass, the 21
  dedicated Python hints, and provisional ADR-0014 fixtures.
- Focused direct builds reproduce ADR-0014 on the native backend: duplicate
  literal keys print `2`, `30`, `1`, `2` (unique length, last value, first-slot
  order), and a missing indexed key reports `AU4003` at the key span.
- `cargo test -p aurora-compiler --test fixtures -- --nocapture` passes all seven fixture families after the final ownership, comparison, Map, Queue, and argument-order integration.
- The rebuilt MIR and direct products both print the ADR-0015 order
  `explicit-third`, `explicit-first`, then `default-second`; the zero and
  negative Queue-capacity products both fail with the exact `AU4001` contract.
- `python3 scripts/test_reference_integrity.py` passes all nine behavioral gate tests; `python3 scripts/reference_integrity.py` passes the full 29-page, 226-fence executable inventory with 101 verified and 125 justified illustrative blocks.
- `bash scripts/check-reference.sh` and `npm run docs:build` pass on the complete reference; only the existing non-fatal EBNF highlighter and chunk-size warnings remain.
- `cargo fmt --all`, Python byte-compilation, and `git diff --check` pass on the integrated tree.
- The complete compiler library sweep passes at 549 tests after correcting the
  target-width compound-literal regression, the retained-call `Option.None`
  inference regression, and the maintained HTTP self-read-before-mutation
  examples exposed by the first full sweep.
- The full CLI target passes at 242 tests after making retained-call analysis
  classify builtin, user, specialized, and module-qualified enum constructors
  as constructor rvalues rather than instance places. A positive constructor
  matrix and a negative nested-payload overlap fixture pin both sides of that
  boundary.
- Corrected sequence-point snapshot typing so a real place keeps its inferred
  type instead of being retagged with an opposite equality operand's contextual
  hint. The forced parity gate exposed the malformed `Unit` metadata for
  `option_value == None`; a focused MIR assertion now pins the `Option[int32]`
  snapshot type in addition to the existing 15-result runtime fixture.
- The final behavior-focused coverage closure added observable operator
  writeback, non-copy borrowed-local alias, shared-self mutation, mutable-match
  scrutinee, and invalid process-text decoding regressions. After the
  Clippy-required compact diagnostic representation, the complete instrumented
  workspace passes at 53,769/55,971 lines (96.065820%), 3,324/3,434 functions
  (96.796738%), and 78,212/83,064 regions (94.158721%). No synthetic-coverage
  test or coverage exclusion was added; the one-time checkpoint floors are
  downward-truncated to 96.06/96.79/94.15.
- The exact full `npm run ci` checkpoint gate passes on the final tree. This
  includes formatting; the complete normal Rust suites (242 CLI tests and 552
  compiler library tests); the fallback-disabled MIR/direct parity matrix (1/1
  in 223.67s); 53/53 LSP tests; 8/8 extension tests; compiler and 100% LSP
  coverage; the 29-page reference-integrity gate; docs build; dependency audit;
  Clippy with `-D warnings`; and repository hygiene.

## Follow-up

- Batch 1 is complete at the reference-freeze checkpoint. ADR-0014 through
  ADR-0017 remain provisional for ratification review; no Batch 2 or Phase 3
  work began.
- V6 remains deferred to Batch 2 with Phase 4. Native direct-backend call/task
  backtraces remain deferred to Batch 3 frame work.
