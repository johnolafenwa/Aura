# Batch 2 checkpoint report

Batch 2 of 5 is complete at its requested checkpoint. Phase 5 was not started.

## Accepted checkpoint amendment

ADR-0031 accepts `mir` as the default for the interactive `aura run` command
and keeps `auto` as the default for artifact-producing `aura build`. This
explicitly amends the original Phase 4 interim-`auto` clause. Forced
MIR/direct parity remains an independent supported-platform release gate.

## Commits

Every semantic commit passed the complete `npm run ci` gate before admission.

| Commit | Ticket |
| --- | --- |
| `24c69a2` | Close the `brace-expansion` advisory |
| `f50b206` | Conditional expressions and the reference cleanup (ADR-0027) |
| `2acdbb1` | B2.0-b generalized to every builtin target |
| `a96f115` | Membership operators and comparison chains (ADR-0028) |
| `9303d78` | Record the resume point |
| `575962c` | `enumerate` and `zip` loop forms (ADR-0029) |
| `6b56d1a` | `len` and `str` builtins (ADR-0030) |
| `7898659` | `aura run` backend selector |
| `6ffbe40` | Content-addressed native artifact cache |
| `9eca65b` | Function-level `aura test` discovery |
| `8fbb2c7` | V6 narrow-integer range-check cost |

## B2.0 disposition and repro results

B2.0-a and B2.0-c were closed in the earlier Batch 2 work at `8bca972` and
`19d8de6` and are unchanged.

B2.0-b was **reopened and closed at the ratified scope**. The original fix
applied the no-shadowing rule only to the four targets named in the repro, so
the same unsoundness stayed reachable everywhere else. Both original repro
shapes were re-run against the current tree and now fail at check time as the
ruling requires, and two further shapes were reproduced end to end before the
fix:

- `impl Sized for Vec[int32]` with a `len` method type-checked clean, then
  printed `1` through both `aura run` and a forced-direct binary instead of the
  trait body's `99`.
- `impl Probe for String` with a `contains` method printed the builtin's `true`
  instead of the trait body's `false`.

In both cases the trait body was silently unreachable at every call site on both
backends: the program did something other than what its source said. Both are
now `AU2006` at check time, and the rule is stated by the builtin-type predicate
rather than a list, so it covers the runtime handles and the builtin value and
scalar types alike.

## Evidence by phase

**Phase 3** was already complete on entry and is unchanged.

**Phase 3.5** is complete. Conditional expressions, membership operators and
comparison chains, `enumerate` and `zip`, and `len` and `str` all landed with
fixtures, MIR/direct parity, language-server coverage, maintained examples,
tutorials, normative Manual and Grammar sections, and ADRs in the same logical
commits.

The conditional-expression packet needed a correctness repair before it could be
admitted, and that is the most important finding in this batch. The inherited
"corrected" ownership replay had three reachable regressions in previously green
behavior, each reproduced before it was fixed:

- Every member expression was routed through branch-aware result consumption, so
  enum-variant and module-qualified paths such as `io.Error.NotFound`,
  `json.Value.Null`, and `process.RestartPolicy.OnFailure` were type-checked as
  field reads of a value object and rejected with `AU2002`.
- Call-argument place collection resolved module-rooted paths as places, so
  `json.dumps(json.Value.Null)` failed while the named-argument form passed.
- Call-argument place collection dropped every copy-typed access, losing the
  retained `mut int32` access, and the new source-ordered rejection
  displaced the parameter-aware same-level overlap diagnostic.

The prior focused verification — 32 conditional tests, `cargo check`, and one
fixture's MIR and direct output — passed through all three. Only the full CLI
and fixture suites caught them. Focused verification is not a substitute for the
full suites on a change that touches shared ownership machinery.

**Phase 4** is complete. `aura run --backend mir|direct|auto` exists, with
`direct` reporting build and launch failures rather than degrading, so a parity
or benchmark caller cannot silently measure the other backend. Both MIR legs of
`backend_parity.rs` now pass `--backend mir` explicitly, which is the recorded
V4 invariant. The native path is content-addressed, and `aura test` reports one
result per `def test_*()` function while file-level tests keep working.

## Provisional ADRs for review

Four were added in this batch and need checkpoint disposition:

- **ADR-0027** conditional expressions — precedence, contextual-literal
  behavior, and the conservative ownership merge should be ratified together.
- **ADR-0028** membership operators and comparison chains — the single
  precedence level, the four supported containers, the at-most-once evaluation
  rule, and the conservative treatment of short-circuited operands.
- **ADR-0029** `enumerate` and `zip` loop forms — loop-form-only status, the
  `Vec`/`Set` operand domain, the `int64` position type, and the borrow default.
- **ADR-0030** `len` and `str` builtins — the member-defined `len` domain, the
  `int64` result, the total `str` domain, and the reservation of both names.

Eight earlier Provisional ADRs remain open from prior batches: 0018, 0019, 0020,
0021, 0023, 0024, 0025, and 0026.

ADR-0031, the CLI backend-default checkpoint amendment, is Accepted.

ADR-0029 and ADR-0030 make deliberately different shadowing choices, and the
difference is the thing to ratify or reject together: a user declaration shadows
`enumerate` and `zip`, because those are loop forms with no value meaning, while
`len` and `str` are reserved, because they are ordinary callables competing for
the same namespace. Reserving `len` and `str` is a source-compatibility change
and is recorded on the status page.

## Retired Python hints

Seven hints are retired to pass-through acceptance. Each keeps its fixture,
which now asserts that the Python spelling type-checks, through a new `.accept`
marker in the hint family:

`python_in`, `python_chained_comparison`, `python_chained_equality`,
`python_mixed_comparison_equality_first`,
`python_mixed_comparison_ordering_first`, `python_len`, `python_str`.

Three of those fixtures were rewritten to well-typed chains of the same shape,
because Aurora's exact types make `1 == 1 == true` a type error rather than
Python's truthiness coincidence. Fourteen hints remain active.

## Retrying worker example

`examples/agents/retrying_network_worker.au`, unchanged in this batch and still
covered by its maintained CLI regression through both backends.

## Backend default decision

**Direct did not become `aura run`'s default. ADR-0031 accepts `mir` as the
default.**

This is an explicit amendment to the original interim-`auto` roadmap clause,
not an undocumented exception. The blocker is not correctness. Forced-direct
correctness is gated on every CI run by the full parity matrix over every
run-pass and run-fail fixture. The blocker is product cost, measured on this
workstation with a hello-world program:

| Path | Wall clock |
| --- | --- |
| `--backend mir` | 0.00s |
| `--backend direct`, cold compile and link | 1.31s |
| `--backend direct`, warm launch, first touch | 0.81s |
| `--backend direct`, warm launch, resident | 0.01s |

The artifact cache removes compiling and linking entirely on a hit. What remains
on a first touch is loading the binary: a direct hello-world executable is about
57 MB of statically linked runtime. A cold miss still costs about 1.3s, and both
CI and the test suites are dominated by programs seen once.

So the remaining blocker for a native `run` default is **binary size**, not
compile time. Promotion now requires ADR-0031's supported-platform parity,
launch-cost, artifact-size, cache-reliability, and separate-ratification
criteria. `aura build` remains native-oriented and defaults to `auto`.

## V6 findings

The direct `int32` ten-million iteration loop was about six times slower than
`int64`, and the cause was not code-generation quality in the loop.

Every integer is a 64-bit runtime value. At `int64`, arithmetic lowers to
Cranelift's overflow-producing form, so the overflow flag falls out of the one
native instruction. A narrow width has no such flag, so the backend computed in
64 bits and then range-checked the result with a two-sided signed comparison:
two constants, two compares, an `or`, and a branch, on the result of every
`int32` operation.

The check is now one biased unsigned comparison — one `iadd_imm`, one
`icmp_imm`, the same branch.

| Width | Before | After |
| --- | --- | --- |
| `int32` | 0.0697s | 0.0327s |
| `int64` | 0.0115s | 0.0111s |
| `int32` / `int64` | 6.05x | 2.95x |

Both numbers are retained in `benchmarks/direct_integer_loops/README.md` and the
benchmark is runnable as `npm run bench:direct-integer-loops`. The residual gap
is the separate branch itself; closing it means giving narrow widths their own
arithmetic width, which is a backend representation change rather than a
contained fix.

Recording the first, wrong reproduction as well: summing `0..10_000_000` into an
`int32` accumulator overflows and traps, so it measures a trap rather than a
loop. The workload has to be a counter loop.

## Coverage per logical decision commit

| Commit | Lines | Functions | Regions |
| --- | --- | --- | --- |
| `f50b206` conditional expressions | 63,752/66,360 | 4,137/4,268 | 93,478/99,158 |
| `2acdbb1` B2.0-b generalization | 63,750/66,358 | 4,137/4,268 | 93,474/99,154 |
| `a96f115` membership and chains | 64,028/66,649 | 4,145/4,281 | 93,930/99,630 |
| `575962c` `enumerate` and `zip` | 64,313/66,939 | 4,154/4,291 | 94,351/100,058 |
| `6b56d1a` `len` and `str` | 64,387/67,014 | 4,154/4,291 | 94,439/100,150 |
| `7898659` backend selector | 64,388/67,014 | 4,154/4,291 | 94,440/100,150 |
| `6ffbe40` artifact cache | 64,396/67,022 | 4,155/4,292 | 94,455/100,165 |
| `9eca65b` test discovery | 64,413/67,042 | 4,158/4,295 | 94,490/100,201 |
| `8fbb2c7` V6 | 64,409/67,039 | 4,158/4,295 | 94,472/100,184 |

The floors stayed frozen at 96.06/96.79/94.15 for the whole batch.

## Final re-ratcheted floors

One downward-truncated re-ratchet. The corrected checkpoint baseline uses the
lower repeatable measurement from the V6 and independent verification gates:
64,409/67,039 lines (96.07691045510822%), 4,158/4,295 functions
(96.81024447031432%), and 94,472/100,184 regions (94.29849077697038%). The
original report recorded one additional covered line and region; that
run-to-run difference does not change any truncated floor:

| Metric | Old floor | New floor |
| --- | --- | --- |
| Lines | 96.06 | **96.07** |
| Functions | 96.79 | **96.81** |
| Regions | 94.15 | **94.29** |

The language-server gate remains enforced at 100%.

## No synthetic coverage tests

No synthetic coverage test and no coverage exclusion was added anywhere in this
batch. Every coverage closure is observable behavior: container coverage and
rejections, evaluation order and short-circuiting, literal adoption and range
checking, unresolved-operand diagnostics, operands inside f-strings,
default-argument parameter references, argument-read conflicts, analysis hover
coverage, AST JSON shapes, and backend parity.

## Justified restructuring

Three sets of provably unreachable branches were removed rather than covered,
each with its invariant stated in the source:

1. The ownership replay walk no longer restates type rules its own precondition
   already proved. A replay only runs after the same expression was accepted
   under the same expected type, and typing does not depend on move state.
2. The direct consumption walk no longer carries a second, non-branch-aware copy
   of composite, conditional, match, cast, and `try` handling; those shapes
   delegate to the single branch-aware walk.
3. In a comparison chain, the right operand is already typed under the left
   operand's type, and every comparison operator produces `bool` — builtin or
   through an operator trait whose declaration fixes that return type — so
   neither restatement was reachable.

## Recommended movements between Batches 3 to 5

- **Narrow-width native arithmetic** (from V6) into the batch that owns backend
  representation. It is the remaining V6 lead and is not a local fix.
- **Direct binary size** into the same batch. It is now the single blocker for a
  native `aura run` default, and it gates the value of the artifact cache.
- **`enumerate` and `zip` over lazy or user-defined iterables** into whichever
  batch introduces an iterator protocol. The current `Vec`/`Set` domain is the
  honest limit of a position-indexed lowering.
- **Structured diagnostic frame lists** remains in Batch 3 frame work, as
  already recorded.

## Phase 5

Not started.
