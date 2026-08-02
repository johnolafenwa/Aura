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
  94.62% regions. Re-ratchet once, downward-truncated, at checkpoint.
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

### S3 test runner foundation

- `aura test -k` filtering, schema-versioned JSON output, source-order
  discovery, per-test setup/teardown, teardown-after-trap behavior, structured
  secondary teardown failures, parametrized registration, stdout capture, and
  canonical paths are implemented.
- Each source file is checked and lowered exactly once into an opaque checked
  MIR module reused by registration, hooks, and test bodies. Source-rewrite and
  manifest-authorized FFI regressions prove that later phases do not re-read or
  re-check the file.
- The runner remains on its current execution backend. Backend parity for the
  pending assertion-introspection work will be proved through forced diagnostic
  fixtures; no runner backend selector is introduced.

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
- LSP and extension — 101/101 and 20/20.
- Reference gate — green across 38 Manual pages, 261 fences, 203 Aura blocks,
  and 123 compiler-verified Aura blocks.
- Focused integration — 15/15 coverage-surface tests and 12/12 FFI frontend
  tests.
- Formatting, owned-file diff hygiene, and build-artifact/disk checks are the
  final pre-commit checkpoint steps.

The opening full-gate attempt stopped at the expected old identity guard that
classified “Aura 0.3” as future narration. That guard now advances to 0.4 and
its focused identity suite is green. Per the user's updated gate policy, the
full gate will next run when the coordinated S1/S2 migration family is ready.

## Follow-up

Commit the coordinated S1/S2 family and merge the now-merged documentation/CI
pull request from `origin/main` locally. Then complete S3 assertion
introspection and S4. At the checkpoint, report migration counts, zero-cast
backend parity, V6 numbers, assertion introspection, S4 evidence, provisional
decisions, final coverage, and three hosted run links, then stop.
