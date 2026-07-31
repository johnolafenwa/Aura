# Batch 6 claims and positioning audit

## Goal

Audit maintained README and documentation claims about performance,
concurrency, and safety; remove unsupported marketing language; and state the
Aurora 0.2 technical-preview position against Mojo, Nim, Go, and free-threaded
Python 3.13+ without turning exact measurements into portable claims.

## Work completed

- Added `docs/positioning.md` as the maintained positioning page and linked it
  from the site navigation, home sidebar, documentation hero, and root README.
- Aligned the README and documentation hero on the narrow wedge:
  deterministic ownership, structured concurrency, and typed failure for agent
  control planes.
- Defined “deterministic ownership” as a value-access, transfer, and cleanup
  contract. The page explicitly separates that contract from scheduler order,
  which remains unspecified.
- Removed the unsupported README comparison to “the memory safety of Rust.”
  The replacement names implemented Aurora mechanisms and links to their
  normative contracts instead of borrowing another project's safety claim.
- Published the exact Batch 6 medians for four protocol workloads, the V6
  fixed-width integer loops, and the separate NumPy Array comparison. Each
  table identifies its measurement boundary and says that the result is not a
  portable promise or release gate.
- Recorded post-reboot host, interpreter, commit, raw-evidence SHA-256, and
  summary SHA-256 provenance. The page states plainly that CPython 3.9.6—not a
  free-threaded Python 3.13+ build—was measured.
- Documented the 20-listener TCP topology required by Aurora 0.2's `AU3008`
  transfer boundary, the 10,000-task creation/join window, startup-subtracted
  V6 estimate, 10/11 valid `int64` adjusted observations, and initial Array
  limitations.

## Claims audit

The maintained surface was inventoried with:

```bash
rg -n -i '\b(performance|performant|faster|fastest|speedup|speed|benchmark|concurren|parallel|memory safe|safety|safe|deterministic ownership|no garbage collector|multicore)\b' \
  README.md docs --glob '*.md' \
  --glob '!aurora_language_proposal.md' \
  --glob '!ml_systems_support_plan.md' \
  --glob '!testing_strategy.md'
```

The excluded files are explicitly historical or forward-looking and are not
part of the built VitePress surface. The 228 maintained matches were reviewed
by claim class:

| claim class | disposition and evidence |
| --- | --- |
| New measured snapshot | Exact Batch 6 post-reboot observations, hardware, interpreter, measurement boundary, evidence hashes, and caveats are present together in `docs/positioning.md`. |
| Numeric Array measurements | Exact measurements and no-gate/no-portability language already live in `docs/manual/numeric-arrays.md`; the positioning page repeats the same medians and points back to that contract. |
| Multicore task execution | Kept as a narrow execution guarantee, never a general speedup claim. `docs/manual/conformance.md` maps it to focused scheduler, cross-worker, CLI parity, and runtime tests; the concurrency and current-limits pages explicitly disclaim preemption, work stealing, ordering, and universal speedup. |
| Ownership and cleanup | Kept as normative language semantics backed by compiler fixtures, runtime/native cleanup tests, backend parity, and the conformance ledger. “Deterministic” is limited to ownership effects, not scheduling. |
| Typed failure and safe/checked operations | Kept where the term names a specified result, diagnostic, bounds check, resource ceiling, or FFI boundary. These are Manual contracts with their fixtures and gates indexed by `docs/manual/conformance.md`, not claims of universal program safety. |
| Clone-safe terminology | Kept. “Clone-safe” is a defined Aurora static-semantic term rather than marketing language. Its specialization and diagnostic tests are mapped in the conformance ledger. |
| Broad cross-language safety or performance language | Removed or avoided. The README no longer claims Rust-equivalent memory safety; the new comparison page makes no unmeasured Go, Mojo, Nim, or free-threaded-Python performance statement. |

## External source verification

Competitor descriptions were checked on 31 July 2026 against official primary
documentation only:

- Mojo roadmap and Ownership:
  <https://mojolang.org/docs/roadmap/> and
  <https://mojolang.org/docs/manual/values/ownership/>
- Nim official site, memory-management guide, and typed threads:
  <https://nim-lang.org/>, <https://nim-lang.org/2.2.6/mm.html>, and
  <https://nim-lang.org/docs/typedthreads.html>
- Effective Go, Go's errors-as-values article, and the Go GC guide:
  <https://go.dev/doc/effective_go#concurrency>,
  <https://go.dev/blog/errors-are-values>, and
  <https://go.dev/doc/gc-guide>
- CPython free-threading guide:
  <https://docs.python.org/3/howto/free-threading-python.html>

The comparison deliberately reports what those projects document, then states
Aurora's difference in scope. It does not rank maturity, ecosystem size,
safety, or performance.

## Verification

Focused verification on the scoped change:

- positioning structural assertions: passed (all competitor sections, exact
  medians, ratios, evidence hashes, scheduling disclaimer, two navigation
  entries, and removal of the former Rust comparison)
- README/home/positioning four-row table parity: passed
- `git diff --check` over the five scoped files: passed
- `npm run docs:build`: passed; `positioning.html` was emitted
- maintained-claims grep: completed with 228 matches classified in the ledger
  above
- `npm run check:reference`: the first scoped run reached an unrelated
  concurrent release-stamping inconsistency: the gate still required the old
  phrase `stable throughout the Aurora 0.1.x` in `docs/manual/randomness.md`.
  The release/version pass owns that file and gate pin; the final Batch 6 gate
  must rerun reference integrity after those concurrent edits converge.

## Follow-up

The release-preparation pass owns version stamping outside this scoped slice.
Future benchmark changes must update the raw and summary evidence first, then
change all three public tables together; a new language comparison must cite a
current first-party source and must not inherit claims from the measured
CPython 3.9.6 baseline.
