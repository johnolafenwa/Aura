# Phase 3.5 `len` and `str` builtins

## Goal

Implement the last two Batch 2 expression-kernel builtins as one freeze-rule
packet: `len(x)` delegating to `.len()` and `str(x)` producing the print and
f-string rendering, retiring the `python_len` and `python_str` hints to
acceptance.

## Test-First Evidence

- The `python_len` and `python_str` fixtures were converted to acceptance
  before either builtin existed, so the hint family failed until both Python
  spellings type-checked.
- The `len_requires_a_len_member` check-fail fixture was written against the
  delegation-shaped diagnostic before the checker produced it.

## Work Completed

- Registered `len` and `str` as maintained builtin functions alongside `print`,
  `abs`, and `parse_int64`, with the same one-argument binding rules, hover
  detail, and documentation metadata.
- Typed `len(value)` by requiring a builtin `len()` member on the value's type
  and producing `int64`. The domain is defined by that member rather than an
  enumerated list, and the rejection names the member the call would have
  delegated to.
- Typed `str(value)` as total over the renderable surface, producing `String`.
- Lowered both by delegation rather than to new runtime entry points: `len` to
  the receiver's `len()` member call and `str` to a one-part format string. The
  direct backend needed no change, so parity follows from machinery both
  backends already had.
- Added Provisional ADR-0030, the normative Expressions section and API-index
  entries, the maintained `examples/basics/len_and_str.au` example with its
  index entry and smoke oracle, the tutorial surface listings, the conformance
  map, a verified reference-integrity block, and a language-server bridge test.
- Recorded the source-compatibility consequence on the status page: both names
  are now reserved, so a program that declared its own `def len(...)` or
  `def str(...)` is rejected the same way redefining `print` is. ADR-0030
  records why this differs from the `enumerate`/`zip` loop forms, which a user
  declaration does shadow.
- Removed the two now-unreachable `AU2005` hint arms from the call-target
  diagnostic, since both names now resolve as builtins.

## Verification

- The focused compiler test pins delegation over `String`, `Vec`, `Map`, and
  `Set`, the `int64` result, rendering equality between `str(x)` and `f"{x}"`,
  both rejection categories, and the reservation of both names.
- The nine-category fixture suite passes, including the new check-fail oracle,
  the new run fixture, and the two retired hints now asserting acceptance.
- The run fixture's exact stdout is identical through MIR and a forced-direct
  binary.
- `npm run check:reference` passes with the new verified Manual block, and the
  70-test language-server suite and its enforced 100% coverage gate stay green.
- The 896-test compiler library suite and the 259-test CLI product suite pass.
- The complete `npm run ci` gate is green before the commit, with compiler
  coverage at 64,387/67,014 lines (96.07992359805414%), 4,154/4,291 functions
  (96.80727103239339%), and 94,439/100,150 regions (94.29755366949576%), above
  the frozen 96.06/96.79/94.15 floors. The closure is observable behavior only;
  no synthetic coverage test or exclusion was added.
- Those figures are transcribed from that gate into this note and the task
  board, and the resulting documentation-only delta was re-verified with
  `cargo fmt --all --check`, `git diff --check`, `npm run check:reference`,
  `npm run docs:build`, `npm run check:hygiene`, and `npm audit`.

## Follow-Up

- Present Provisional ADR-0030 at the Batch 2 checkpoint.
- Phase 3.5 is complete. The Phase 4 backend selector is the next ordered
  ticket.
