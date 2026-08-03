# Aura Batch S1: 0.3 Python-surface program

## Goal and boundaries

Complete the source-incompatible 0.3 surface migration as one
coordinated program: S2 index-domain unification, S1 collection naming and
methods, S3 testing, S4 Python polish, and five ratification-ready design-only
ADRs. Stop at the requested checkpoint. ADR-0038 loans, the P1 performance
batch, publishing, and 0.4 implementation are outside this authorization.

Nothing from this batch is pushed without explicit user authorization.

## Preconditions

- Extension publication and the F1/F2/F3 hosted-CI stability work are complete.
- Hosted CI runs
  [30738666230](https://github.com/johnolafenwa/Aura/actions/runs/30738666230),
  [30738666191](https://github.com/johnolafenwa/Aura/actions/runs/30738666191),
  and
  [30738666169](https://github.com/johnolafenwa/Aura/actions/runs/30738666169)
  each passed Ubuntu 24.04 and macOS 15 at commit `cd93f221`.
- The batch is isolated on local branch `codex/batch-s1-python-surface`, stacked
  on the pending landing-page and CI-routing work. It has no remote branch.
- The pre-batch build tree is 10 GiB and the workstation has 88 GiB available.

## Standing batch rules

- Test first for every behavior change.
- Keep ADR, Manual, fixtures, examples, tutorials, and Python hints atomic with
  each semantic change.
- Run focused behavioral gates throughout. Reserve the full local gate for a
  completed logical migration family and the final checkpoint; do not replay
  the multi-hour gate after every small commit.
- Freeze compiler coverage floors at 96.28% lines, 97.20% functions, and
  94.62% regions during implementation. The single checkpoint re-ratchet is
  now complete at 96.30% lines, 97.21% functions, and 94.71% regions.
- Use inventory → migrator → atomic flip → identity gate for every rename.
- Use provisional decisions only for genuine gaps and record P1–P6 evidence.

## User amendment: no backward compatibility

Aura has no users yet, so the 0.3 surface treats the prior syntax and methods
as though they never existed:

- There are no aliases, shims, reserved old spellings, specialized retirement
  diagnostics, fix-its, grace periods, or user-facing migration guidance. An
  old name is only an ordinary unknown type, name, or member.
- `list.remove(x)` activates immediately for every equatable element type,
  including integer lists. The prompt's one-release integer containment is
  removed. Maintained old index-removal calls migrate directly to `pop(index)`.
- Internal migration tooling exists only to flip this repository atomically.
  It is not part of the language, CLI, release, or documentation surface.

## Progress

### 0.3 development identity

- Failing tests first pinned `0.3.0` across Cargo/npm workspace manifests and
  locks, the CLI `aura 0.3.0-dev (<commit>)` stamp, the language-server identity,
  the VS Code extension, and the Manual's development channel.
- The implementation now satisfies those focused contracts. The shipped
  v0.2.0-preview installer, archive names, and release workflow remain
  historical release behavior.

### Coordinated S1/S2 migration inventory

The atomic source inventory covered 1,921 maintained `.au` files. Of those,
790 contained candidate syntax. The classifier reviewed 2,218 candidate
occurrences, applied 2,006 semantic rewrites across 733 files, classified 212
occurrences as legitimate user names, enum variants, comments, or private host
identities, and left zero unresolved source occurrences. The exact migrated
surface included collection type annotations and constructors, list methods,
the Array list constructor, and capability syntax; receiver-sensitive
`.remove(...)` calls were classified before editing so list index removal became
`pop(index)` without changing dict or set behavior.

The final one-shot manifest had SHA-256
`f5e99b455b39109e2b2992231c70135f7935847663428044521eafe94cbe2f6d`.
Its exact found/rewrite ledger was:

| Inventory rule | Found | Rewritten |
| --- | ---: | ---: |
| `Vec` type/constructor to `list` | 305 | 305 |
| `String` type to `str` | 1,442 | 1,442 |
| `Map` type/constructor to `dict` | 46 | 46 |
| `Set` type/constructor to `set` | 41 | 41 |
| `Set{...}` literal to `{...}` | 3 | 3 |
| list `push` to `append` | 72 | 72 |
| list `sort_by` to `sort(key = ...)` | 9 | 9 |
| collection `clone` to `copy` | 16 | 16 |
| collection `contains` to membership | 18 | 18 |
| dict `contains_key` to membership | 4 | 4 |
| dict `entries` to `items` | 2 | 2 |
| dict `extend` to `update` | 3 | 3 |
| set `insert` to `add` | 9 | 9 |
| dict `set` call to indexed assignment | 7 | 7 |
| set `remove` with ignored result to `discard` | 1 | 1 |
| contract-statement rewrites for `insert` | 10 | 10 |
| contract-statement rewrites for `items` | 2 | 2 |
| contract-statement rewrites for `pop` | 3 | 3 |
| contract-statement rewrites for `remove` | 6 | 6 |
| contract-statement rewrites for `set` | 5 | 5 |
| contract-statement rewrites for set literals | 1 | 1 |
| contract-statement rewrites for `swap` | 1 | 1 |
| Legitimate homonyms and private identities | 212 | 0 |
| **Total** | **2,218** | **2,006** |

This ledger is internal repository evidence. There is no public migration
guide, compatibility mode, alias, old-spelling diagnostic, or fix-it. The
prompt's one-release integer `remove` containment is superseded and is not
applicable. Immediate value removal for integer lists is pinned by
`canonical_collection_surface.au`, where `values.remove(1)` removes the value,
and `list_remove_missing.au`, where an absent integer value traps with `AU4008`
and membership-precheck guidance. Index removal is independently pinned by the
same success fixture's `values.pop()` call.

S2 did not produce a per-occurrence migration manifest. The exact pre-flip
found count by semantic index subfamily therefore cannot be reconstructed from
an authoritative ledger. The strongest reproducible textual evidence is the
word diff from pre-batch `8dffd9d` to coordinated S1/S2 commit `2d604f8`,
excluding work notes and ADRs: it contains 245 direct `int32` to `int64` token
substitutions in 244 replacement pairs across 51 maintained files, plus 24
direct internal `i32` to `i64` replacement pairs. This does not count newly
added implementation or test lines and is not presented as the missing
pre-flip inventory. Semantic coverage is instead pinned directly for collection
positions, slice endpoints, range bounds and yields, Array coordinates, and
concurrent result indices by the fixtures and focused tests listed below.

The repository identity guard now scans maintained Aura files, documentation
fences, diagnostic oracles, editor rules, and embedded Aura programs inside
both integration tests and Rust `*_tests.rs` unit-test siblings. It masks host
language syntax and permits user-defined homonyms. Exact structural exemptions
exist only for `json.Value.String(str)` and one private native ABI collection
tag. The expanded scan also avoids quadratic host-string extraction on large
test modules.

The first broad compiler-library replay exposed 354 stale embedded test
programs or expected hover strings after 1,145 tests passed. The compiler was
correctly rejecting the removed names; the permanent gate had omitted
`*_tests.rs`. That gate defect and the complete missed test surface are now
closed. The resulting audit also removed surviving public collection-member
aliases and repaired private dict-membership dispatch without restoring any
source-visible compatibility path.

### S2 unified index domain

- Accepted ADR-0043 makes `int64` the one position type for collection
  indices and assignment, slices, range bounds and yields, enumeration,
  Array coordinates, and concurrent source-result indices.
- The compiler permits lossless widening from `int8`, `int16`, `int32`,
  `uint8`, `uint16`, and `uint32` only at those positions. `intsize`,
  `uintsize`, and wider integer domains are rejected target-stably. Ordinary
  assignments and calls retain exact typing.
- Semantic checking, MIR casts and traversal counters, interpreter runtime,
  direct code generation/runtime, public builtin metadata, analysis/LSP
  completion details, examples, tutorials, and the Manual now share the
  contract. At this temporary pre-S1 checkpoint, Array coordinate containers
  are internally `Vec[int64]`; the coordinated S1 flip immediately replaces
  that spelling with canonical `list[int64]`.
- Behavioral fixtures pin the three cast-free idioms, all six widening source
  types, pointer-sized and wider rejection, `i64` boundaries, Array
  coordinates, and non-index conversion rejection.

### Post-S2 V6 measurement

The maintained schema-4 V6 protocol was replayed at clean integrated commit
`face52e3900f775a3284df56a2519622d8381d60` at
`2026-08-02T20:13:03.022427+00:00`. The host was a
Mac14,9 MacBook Pro with an Apple M2 Pro, 10 cores, and 16 GiB memory. A fresh
locked release build produced `aura 0.3.0-dev (face52e3900f)`, SHA-256
`1f7e3281e574d2bd07d666735238833349f1854bd1c32d5684a3a8545a8cfb10`.

The focused replay used the established `startup.au`, `int32_loop.au`, and
`int64_loop.au` direct-native workloads: one excluded warmup each, five
measured repetitions, rotating startup/int32/int64 order, exact stdout checks,
and same-repetition whole-process-minus-startup loop estimates. Both quiet
process checks were empty and the report is contractual.

| V6 lane | Median | MAD | p95 | Best |
| --- | ---: | ---: | ---: | ---: |
| startup, whole process | 6.570375 ms | 0.243542 ms | 9.216708 ms | 6.326833 ms |
| `int32`, whole process | 36.222917 ms | 0.467584 ms | 45.228209 ms | 35.254792 ms |
| `int64`, whole process | 14.673875 ms | 0.315334 ms | 16.303083 ms | 14.193708 ms |
| `int32`, paired loop estimate | 29.305958 ms | 0.590126 ms | 36.011501 ms | 28.684417 ms |
| `int64`, paired loop estimate | 7.744333 ms | 0.657958 ms | 8.418834 ms | 7.085334 ms |

All five startup-adjusted pairs were valid for both widths. Against the
accepted post-reboot whole-process baseline of 36.691666 ms / 14.837417 ms,
the post-S2 medians are 1.28% / 1.10% lower. The `int32`/`int64` median ratio is
2.469x versus the baseline's 2.473x. The index-domain migration therefore shows
no V6 regression and does not begin the separately authorized P1 optimization
work. Raw evidence is `/tmp/aura-s1-post-s2-v6-face52e.json`, SHA-256
`491d1268398c46b0c55393d7542d63a93804034ba6e8b128be67565f93fcdf64`.

### S1 canonical collection surface

- Aura source and public diagnostics now use one collection vocabulary:
  `list`, `dict`, `set`, and `str`. The former words have no lexer, parser,
  checker, completion, or diagnostic significance and remain available as
  ordinary user identifiers where grammar permits.
- `list.append`, value-based `remove`, `index`, `count`, Python-shaped
  `pop(index = -1)`, stable `sort(key = ..., reverse = ...)`, clamping
  `insert`, and capacity control are implemented across semantic checking,
  MIR execution, and the direct backend. Integer-list removal has the same
  immediate value semantics as every other equatable element type.
- Dict and set names and methods are canonicalized, including indexed dict
  assignment, `copy`, `update`, set `add`, loud set `remove`, non-trapping
  `discard`, and capacity control. Missing list/set values produce the pinned
  AU4008 diagnostic with the membership pre-check guidance.
- Empty `{}` is a dict. Empty sets use the typed `set[T]()` constructor; bare
  `set()` reports the canonical type-argument diagnostic. Set rendering is
  `{...}` and `set()` for the empty value on both backends.
- The public Array constructor is `Array[T].from_list`. The private Rust value
  helper and native ABI symbol retain implementation names that are not Aura
  source surface.
- The maintained Manual, Learn track, examples, tutorials, reference hashes,
  LSP recovery/completions, TextMate grammar, and VS Code tests use the
  canonical surface. No alias, shim, transition diagnostic, fix-it, grace
  period, or public migration instruction was added.

### S3 testing framework

- `aura test -k` filtering, schema-versioned JSON output, source-order
  discovery, per-test setup/teardown, teardown-after-trap behavior, structured
  secondary teardown failures, parametrized registration, stdout capture, and
  canonical paths are implemented.
- Each source file is checked and lowered exactly once into an opaque checked
  MIR module reused by registration, hooks, and test bodies. Source-rewrite and
  manifest-authorized FFI regressions prove that later phases do not re-read or
  re-check the file.
- Top-level `==`, `!=`, `<`, `<=`, `>`, `>=`, and positive builtin membership
  assertions capture and report two typed operand values. Whole-condition
  grouping preserves introspection; chains, `not in`, boolean combinations,
  calls, and consuming custom dispatch retain ordinary assertion diagnostics.
- Operand evaluation remains exactly once and left to right. Rendered captures
  are produced on the failure edge before the lazy message runs, bounded to
  4,096 UTF-8 bytes each, and carried in optional schema-1
  `assertion_operands` records. The MIR and direct runtime paths use the same
  labels, types, values, truncation state, and source span.
- The runner remains on its current execution backend. Focused forced MIR and
  direct assertion failures are byte-identical; no runner backend selector is
  introduced. ADR-0045 implementation is complete and remains Provisional only
  until its registration API and full completion matrix are ratified at the
  Batch S1 checkpoint.

### S4 Python polish: numeric and import foundation

- Module and from-import aliases are implemented for builtin, local, and
  dependency-package imports. Only the alias binds, while nominal identity,
  visibility, hover, completion, and definition behavior remain intact.
- Integer literals accept binary, octal, hexadecimal, and between-digit
  separators. Power and the complete bitwise precedence ladder preserve exact
  integer widths.
- Checked shifts reject negative or width-reaching counts and reject left-shift
  overflow. Every fixed-width signed and unsigned integer exposes
  `wrapping_shl`, `wrapping_shr`, `saturating_shl`, and `saturating_shr` with
  exact-type counts. Tests cover all 12 widths, arithmetic versus logical right
  shift, both saturation bounds, and MIR/direct diagnostics.
- `round` uses ties-to-even, preserves integer types, and returns `int64` for
  floats. `divmod` preserves the exact matching numeric type and shares checked
  floor-division behavior, including zero and signed-minimum overflow traps.
- Floating power is computed at the destination width through one shared
  runtime helper. A regression pins float32 overflow that remains finite as
  float64, eliminating backend double-rounding and overflow-classification
  drift.
- The maintained bit-packing example and executable Manual blocks prove the
  binary-protocol surface. The semantic compiler/editor interface is now
  schema version 5 after the complete S4 AST, analysis, and constant-symbol
  expansion. Compiler, CLI cache, language-server bridge, extension protocol,
  Manual, and pinned semantic-interface tests use the same value.
- The broad run-pass harness initially exposed a stack-guard SIGBUS in an
  existing imported-builtin-default fixture. Nested MIR checked-dispatch frames
  exceeded the 512 KiB lightweight-task stack before a blocking host call; the
  default is now 768 KiB and the full run-pass family is green.

### S4 canonical paths and design adoption

- Five public examples and 64 fixture stems now use canonical `list`, `dict`,
  `set`, shared, and mutable terminology. The rewrite replaced 126 fixture and
  oracle paths while preserving four legitimate Array `map` names.
- Exact README, tutorial, Manual, compiler-test, CLI-test, and diagnostic-path
  references moved with the files. The clean-slate identity suite now rejects
  a reintroduced noncanonical example or fixture path.
- ADR-0052 through ADR-0056 each record implementation dependencies, cache or
  schema adoption, and completion conditions. The sections contain no alias,
  shim, retired-spelling, compatibility-period, or old-to-new migration story.
- Focused verification passed all 11 identity tests, all nine fixture
  categories, example smoke/direct-codegen coverage, and 15 collection-focused
  CLI example tests.

### S4 scalar math functions

- The builtin `math` module exposes exact `float64` contracts for `floor`,
  `ceil`, `trunc`, `pow`, `exp`, `log`, `log2`, `log10`, `sin`, `cos`, and
  `tan`. No implicit numeric conversion is introduced.
- Shared runtime helpers classify finite overflow as `AU4002` and domain
  failures as `AU4001`, while preserving the accepted NaN, infinity, signed
  zero, and identity cases from ADR-0048. Integer-returning conversions enforce
  the exact `int64` boundary despite binary64 rounding of `i64::MAX`.
- Compiler analysis supplies module completion, exact signatures, and hover.
  Focused unit tests, the maintained success fixture, and the domain-failure
  fixture are byte-identical between MIR and direct execution.
- `math.pi`, `math.e`, `math.inf`, and `math.nan` are exposed as exact
  `float64` module constants over the generic constant foundation. Fixtures,
  the maintained scalar-math example, the Manual, the API index, analysis,
  and both runtime backends pin their exact IEEE identities and behavior.

### S4 strings and format specifications

- Accepted ADR-0046 is implemented across lexing, parsing, checking, MIR,
  direct code generation/runtime, analysis, the Manual, tutorial, maintained
  example, language server, and editor grammar/snippets.
- Single- and double-quoted raw strings and triple-quoted multiline strings
  preserve their exact contents under the documented delimiter, escape, and
  indentation rules. Tabs outside continuation/string contexts retain their
  lexical rejection and tabs inside triple-quoted content remain data.
- Static f-string specifications cover fill/alignment, sign, width, grouping,
  precision, and the accepted `d`/`f`/`e`/`x`/`X`/`b`/`o`/`%`/`s` type codes.
  Formatting is type-directed, bounded and fallible, preserves exact integer
  digits without a binary64 round trip, and shares observable behavior across
  both backends.
- Focused lexer/parser, semantic, runtime-value, MIR, native, compiler-bridge,
  extension, fixture, formatter-preservation, Unicode, boundary, IEEE, and
  allocation-limit regressions are present. The maintained success fixture and
  focused failures pin accepted output and `AU1101`/`AU2002`/`AU4001`/`AU4005`
  behavior.

### S4 match guards and or-patterns

- Guards and or-patterns from ADR-0049 are implemented for statement and
  expression matches. Guards require exact `bool`; alternatives test left to
  right, bind identical names/types/capabilities, and guarded arms do not
  contribute to exhaustiveness.
- `match own` delays non-Copy extraction until a true guard commits its arm.
  `match mut` writes candidate mutations back before false continuation,
  propagated failure, runtime trap, and each selected-arm exit path. Later
  alternatives and later arms observe the reconstructed value.
- Focused parse/check/run fixtures pin false continuation, first-match order,
  guarded wildcard fallthrough, guarded enum/bool/tuple/open-domain
  exhaustiveness, owned-candidate move rejection, or-pattern binding rules,
  mutable false/trap/`try` writeback, and normal/`return`/`break`/`continue`
  exits. Compiler analysis and language-server coverage accompany the
  maintained example and Manual text.
- Class patterns are not part of Aura 0.3's implemented surface. Their
  match-exposure and capability design remains a provisional ADR-0049
  disposition for explicit checkpoint review; positional and named
  call-shaped forms are rejected by focused fixtures.

### S4 module constants

- Accepted ADR-0050 is implemented generically: inferred, annotated, and
  `public` module constants may coexist with imports, declarations, executable
  entry statements, and `main`.
- The shared initialization plan is dependency-first, first-import ordered,
  source ordered within each module, eager, once-only across diamonds, guarded
  against re-entry, and cleaned up in reverse order. MIR and direct execution
  preserve one defining storage identity; Copy reads copy and non-Copy reads
  provide shared access without an implicit deep clone.
- Assignment, mutable access, and moves from non-Copy constant storage are
  rejected. Analysis and the language server expose visibility, hover,
  completion, and definitions for local, qualified, and from-imported
  constants.
- Focused proof is exact: `cargo test -p aura-compiler module_constant --
  --nocapture` passed 5 tests; the fixture harness passed all 9 families; the
  focused package dependency test passed; local, imported, stateful-shared,
  and indirect-re-entry fixtures produced byte-equivalent MIR/direct output or
  diagnostics; 2 LSP integration tests passed; the language-server check
  passed; and reference integrity executed 127 verified blocks across all 27
  feature pages.

### Testing reference freeze and semantic interface

- ADR-0045's implemented runner and assertion surface is now described by the
  CLI Manual, conformance map, Tutorial 23, and maintained assertions example.
  The completion checklist is fully checked except for the checkpoint-wide
  forced parity matrix and final local/hosted gates.
- The reference script's long wrapped assertions no longer depend on source
  line wrapping, and the maintained reference assertions cover the current
  numeric and testing surface. Production warning-denied Clippy passed for the
  integrated feature commits; the final repository-wide Clippy replay remains
  part of the checkpoint gate.
- Semantic compiler/editor protocol schema 5 is pinned consistently in the
  compiler, CLI cache metadata, language-server bridge, extension tests, and
  Manual. There is no schema-4 acceptance path.

## Provisional decisions P1-P6

These are provisional checkpoint answers to ADR-0045's six ratification
questions. They record the implemented contract and remain provisional until
the complete forced-backend matrix plus final local and hosted gates pass.

| Decision | Provisional checkpoint answer | Evidence/status |
| --- | --- | --- |
| P1 | Keep parameter registration as `list[(str, def() -> None)]`, expanded once in registration order into independently reported cases. | Implemented and covered by valid, empty, duplicate-label, invalid-signature, capture-rejection, trap, ordering, and filtering tests. ADR-0045 remains Provisional. |
| P2 | Keep literal case-sensitive substring matching for `aura test -k`, applied after parameter expansion; selecting zero cases succeeds. | Implemented and pinned for ordinary/parameterized names, zero matches, and usage errors. ADR-0045 remains Provisional. |
| P3 | Keep the 4,096-byte UTF-8 bound independently for each rendered assertion operand, with explicit truncation state. | Implemented in human and schema-1 structured diagnostics with Unicode-boundary tests. ADR-0045 remains Provisional. |
| P4 | Keep the first lifecycle failure primary and report a teardown failure secondarily; teardown runs after a case failure or trap. | Implemented for setup, case, teardown, dual-failure, timeout, and phase-order paths. ADR-0045 remains Provisional. |
| P5 | Keep setup, case, and teardown as isolated lifecycle phases and require registered case function values to be capture-free. | Implemented with checked-module reuse, external-order evidence, and capture rejection. ADR-0045 remains Provisional. |
| P6 | Keep JSON result schema 1: one ordered document with summary and test records, optional discovery/stdout/diagnostic/reason fields, and status 0/1 after a completed run. | Implemented and pinned for ordering, durations, output separation, runner/diagnostic failures, and exit status. ADR-0045 remains Provisional. |

ADR-0049 has a separate provisional checkpoint question. Guards and
or-patterns are accepted and implemented, while class patterns remain
unimplemented pending a future match-exposure protocol. ADR-0051 accepts
import aliases and deliberately defers keyword-only parameters because the
current structural callable type cannot preserve that restriction. No
provisional decision creates backward compatibility: previous spellings and
methods have no reserved status, compatibility path, tailored diagnostic, or
public migration surface.

## Coverage closure and checkpoint ratchet

The first exact combined coverage replay passed all 361 CLI tests, 1,654
compiler-library tests, and every instrumented integration suite. Its only
red gate was the frozen ratchet at 90,966/94,569 lines (96.19%),
5,968/6,149 functions (97.06%), and 133,808/141,427 regions (94.61%). Under
the standing rule, the floors stayed fixed and the gap was closed with
observable behavior tests and removal or consolidation of formally
unreachable defensive wrappers.

The closure found and fixed three product defects:

- compiler analysis inferred `range` loop bindings as `int32` after S2; they
  now use the canonical `int64` index domain;
- native list traps could occur while a collection write lock was held,
  poisoning the collection for later tasks; diagnostics now occur after the
  guard is released; and
- MIR argument lowering widened `list.remove(value)` as though it were an
  index while failing to widen narrow `list.pop(index)` arguments. Only
  `pop` now uses the lossless index-domain widening rule.

Behavioral closure covers the canonical collection ABI, semantic and editor
diagnostics, match guards and or-patterns, scalar numeric boundaries, module
constants, formatting, package locks, direct runtime ABI contracts, Array
allocation failure, process/supervisor behavior, HTTP/HTTPS/TCP/UDP/WebSocket
boundaries, public compiler facades, and exact MIR/direct parity paths.
Unreachable-branch work is recorded in
`work/2026-08-03-s1-mir-runtime-coverage-classification.md` and
`work/2026-08-03-s1-mir-runtime-public-coverage.md`; it includes duplicated
string-byte dispatch, impossible task-index conversion failures, infallible
Duration construction overflow wrappers, redundant private call-target
fallbacks, and generic-only builtin lowering closures whose public registry
cannot construct the old path.

The dedicated closure coverage replay passed all 361 CLI tests, 1,659
compiler-library tests, and every instrumented integration suite at:

- 91,003/94,493 lines (96.306604722043%);
- 5,973/6,144 functions (97.216796875%); and
- 133,861/141,324 regions (94.719226741389%).

The one-time downward-truncated checkpoint ratchet is therefore
96.30%/97.21%/94.71%. No synthetic coverage test, coverage exclusion, or
malformed internal compiler-state test was added.

The final `npm run ci` replay at `5ee64cdfc73197bd9084b26c270407df8d2daf9a`
also passed the ratchet. Its independent profile merge recorded
91,001/94,493 lines (96.304488163144%), 5,973/6,144 functions
(97.216796875%), and 133,859/141,324 regions (94.717811553593%). The two
replays differ by two nondeterministic process-path lines while both remain
above the downward-truncated checkpoint floors.

## Verification

Current focused version-stamp evidence:

- `python3 -m unittest scripts/test_release_metadata.py` — 5 passed.
- `node --test docs/.vitepress/release-metadata.test.mjs` — 6 passed.
- `npm --prefix tools/vscode-aura test` — 20 passed.
- `cargo test -p aura --test cli version_flags_exit_successfully -- --exact` —
  passed.
- S2 run-pass, check-pass, and check-fail fixture harnesses — passed.
- S2 cast-free and lossless-widening fixtures under forced direct execution —
  passed with exact output.
- Focused semantic, MIR, MIR-runtime, native-codegen/runtime, analysis, and
  typed select/wait tests — passed.
- `python3 scripts/reference_integrity.py --inventory-only` — passed after
  reviewing and updating the changed executable Collections block hash.
- Canonical collection run-pass, run-fail, check-fail, check-pass, and
  Python-hint fixture families — passed before the expanded embedded-unit-test
  scan.
- `cargo test -p aura-compiler --test coverage_surface` — 15 passed.
- `cargo test -p aura-compiler --test ffi_frontend` — 12 passed.
- S3 runner focused suite — 17 passed; Aura-only Clippy passed.
- S3 assertion compiler partition — 11/11 passed across semantic eligibility,
  MIR shape and sequencing, diagnostic rendering, MIR runtime, native runtime,
  and direct code generation.
- S3 runtime partitions — MIR runtime 130/130 and native runtime 182/182 passed.
- S3 CLI evidence — the once-only human diagnostic test is green with
  byte-identical focused MIR/direct output, and the schema-1 JSON runner test
  pins the typed `assertion_operands` records. A second JSON integration test
  proves the same operand records and primary span under MIR and direct
  execution.
- S3 run-fail evidence — both comparison and membership fixtures pass the
  fixture harness and match their `.diag` oracles byte-for-byte under forced
  MIR and direct execution.
- S3 compiler library replay — 1,512/1,512 passed after the complete
  assertion-diagnostic, MIR, runtime, and native-codegen integration.
- S3 editor integration — 102/102 LSP tests, 100% LSP statement/branch/
  function/line coverage, and 21/21 extension tests passed. Non-empty
  `assertion_operands` records survive the compiler-to-editor boundary and are
  omitted from ordinary diagnostics.
- Reference inventory — 38 Manual pages, 261 blocks, 203 Aura blocks, 123
  compiler-verified blocks; reference and release-metadata tests passed.
- Clean-slate identity suite — 11 passed before expanding the embedded-source
  path classifier; the expanded suite is now 11/11 green after scanning Rust
  unit-test siblings and removing the final public alias dispatches.
- Compiler library replay after the coordinated migration and final diagnostic
  and private-membership closure — 1,500/1,500 passed.
- Canonical semantic/call/analysis partitions — 303/303, 21/21, and 97/97.
- Complete fixture harness — 9/9 families, including parse, check, run,
  diagnostics, package paths, and Python-shaped accepted forms.
- S1/S2 checkpoint LSP and extension baseline — 101/101 and 20/20; the S3
  editor replay advances those suites to 102/102 and 21/21.
- Reference gate — green across 38 Manual pages, 261 fences, 203 Aura blocks,
  and 123 compiler-verified Aura blocks.
- Focused integration — 15/15 coverage-surface tests and 12/12 FFI frontend
  tests.
- The coordinated S1/S2 family passed formatting and owned-file diff hygiene,
  was committed, and was merged locally with the landed documentation/CI pull
  request before S3 began.
- S4 import-alias parser/module/package/analysis/LSP fixtures pass. Numeric
  unit partitions and all parse/check/run fixture families pass; the maintained
  numeric example and focused float32-power failure are byte-identical on MIR
  and direct backends. Reference integrity is green across 38 Manual pages,
  264 fences, 206 Aura blocks, and 124 compiler-verified blocks; the docs build
  is green.
- Canonical path closure — 5/5 public examples and 126/126 fixture/oracle paths
  migrated; all identity, fixture, example, and focused CLI checks passed.
- Scalar math function closure — five focused compiler tests passed; the
  maintained success and failure programs match across forced MIR/direct
  execution; production warning-denied Clippy passed.
- String/format closure — focused lexer, parser, semantic, runtime-value, MIR,
  native, fixture, CLI, compiler-bridge, and extension tests passed; the
  maintained success and failure fixtures pin exact formatting, diagnostics,
  Unicode boundaries, exact wide integers, float32/IEEE cases, formatter
  preservation, and allocation caps.
- Match closure — focused parser/checker/MIR/analysis tests and the complete
  guard/or-pattern fixture family passed. Forced MIR/direct runs agree for
  accepted selection and mutable writeback, owned-candidate behavior, and
  trap/propagation diagnostics. The final corpus-wide parity matrix passed.
- Module-constant closure — 5 focused compiler tests, all 9 fixture families,
  the package dependency test, 2 LSP integrations, the language-server check,
  focused MIR/direct parity, and 127 verified Manual blocks passed.
- Testing/reference closure — ADR-0045's reference completion matrix is checked
  through every focused item; the checkpoint-wide matrix and full local gate
  also pass. Semantic interface schema 5 and its cache/editor identities are
  aligned. Wrapped reference assertions and production warning-denied Clippy
  passed on their focused commits.
- Checkpoint-documentation proof — reference inventory passes at 39 pages, 270
  fenced blocks, 131 verified blocks, and 28 feature pages with no missing
  normative section or executable example; the focused reference unit suite is
  10/10; the clean-surface identity suite is 11/11; the production VitePress
  build and link resolution pass; and `git diff --check` passes. The complete
  reference wrapper and regenerated `llms.txt`/`llms-full.txt` are green after
  the four math constants finalized the source reference.

The final exact-tree `npm run ci` gate is green at
`5ee64cdfc73197bd9084b26c270407df8d2daf9a`: formatting, benchmark and release
regressions, identity, all Rust tests, the forced-backend parity matrix, LSP,
extension packaging/tests, compiler and LSP coverage, reference integrity,
the documentation build, npm and Rust audits, warning-denied Clippy, and
hygiene all passed. The Rust audit retains the already-allowed
`rustls-pemfile` unmaintained warning. `target/` finished at 19 GiB with 37 GiB
free, below both cleanup thresholds. Three hosted runs remain before checkpoint
sign-off.

## Follow-up

Obtain explicit user authorization before pushing this branch. After three
consecutive hosted runs pass on both operating systems, ratify ADR-0045's
P1-P6 answers and ADR-0049's class-pattern deferment, publish the checkpoint
report, and stop. Do not begin loans, the P1 performance batch, publishing, or
any 0.4 implementation.
