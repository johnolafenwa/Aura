# B2.0-b generalized builtin method collisions

## Goal

Close the remaining part of the ratified B2.0-b rule. The original fix rejected
a trait method that shadows a builtin member only on `Queue[T]`, `Task[T]`, and
`TaskGroup`, later extended to `random.Rng`. The ruling is about builtin member
names being reserved, not about which four targets were named in the repro, so
every builtin target needed the same rejection.

## Test-First Evidence

- Three new check-fail fixtures were added before the checker changed:
  `builtin_vec_trait_method_collision`, `builtin_string_trait_method_collision`,
  and `builtin_file_trait_method_collision`. The first fixture run failed with
  "should fail to type-check", because all three were accepted.
- The accepted programs were reproduced end to end first. `impl Sized for
  Vec[int32]` with a `len` method checked clean and then printed `1` on both
  `aura run` and a forced-direct binary rather than the trait body's `99`;
  `impl Probe for String` with a `contains` method printed the builtin's `true`
  rather than the trait body's `false`. The trait body was silently unreachable
  at every call site on both backends, so the program did something other than
  what its source said.

## Work Completed

- Replaced the four-name allowlist in the checker with the general builtin-type
  predicate, so an `impl` for any builtin target — the runtime handles plus the
  builtin value and scalar types — is rejected when an explicit or inherited
  trait method name resolves to a builtin member of that target.
- Generalized the `AU2006` message and guidance from "builtin handle method" to
  "builtin method"; the code, its span behavior, and its inherited-default
  secondary span are unchanged.
- Confirmed the direct backend's existing precedence guard already generalizes:
  it consults `BuiltinMember::resolve` for any receiver base, so builtin
  resolution is honored even if the checker rule were ever bypassed.
- Added a focused compiler regression across `Vec`, `String`, `Map`, `fs.File`,
  and a scalar target, which also pins that a noncolliding trait method still
  implements and dispatches on a builtin target.
- Added the maintained `examples/traits/builtin_target_traits.au`, its example
  index entry and smoke-test oracle, and a traits-tutorial section that shows
  the accepted form and the rejected one.
- Updated the normative rule and backend-support paragraph in
  `generics-and-traits.md`, the `AU2006` category text and code table in
  `diagnostics.md`, the conformance map, and the executable reference guard.

## Verification

- The nine-category fixture suite passes, including the three new check-fail
  oracles and the three original handle oracles under the generalized wording.
- The 889-test compiler library suite passes, including the new cross-target
  regression and the new example smoke oracle.
- `npm run check:reference` passes with the added guard for the generalized
  normative sentence.
- The complete `npm run ci` gate is green before the commit, with compiler
  coverage at 63,750/66,358 lines (96.06980318876398%), 4,137/4,268 functions
  (96.9306466729147%), and 93,474/99,154 regions (94.27153720475219%), above the
  frozen 96.06/96.79/94.15 floors. No synthetic coverage test or exclusion was
  added.
- Those figures are transcribed from that gate into this note and the task
  board, and the resulting documentation-only delta was re-verified with
  `cargo fmt --all --check`, `git diff --check`, `npm run check:reference`,
  `npm run docs:build`, `npm run check:hygiene`, and `npm audit`.

## Follow-Up

None. B2.0-b is fully closed at the ratified scope.
