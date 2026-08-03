# S1 MIR-runtime coverage classification

## Goal

Classify the 55 logical functions reported missing in `mir_runtime.rs` by the
S1 coverage baseline. LLVM represents these functions as closure regions, so
the raw JSON's 264 uncovered instantiations was grouped by the first source
region to recover the file summary's exact 55-function count.

## Public behavior

Ten closures are public collection resource behavior: baseline lines 3584,
3590, 3600, 3611, 5567, 5574, 5829, 5833, 6116, and 6123. The public-source
regression `canonical_collection_capacity_failures_have_stable_runtime_diagnostics`
pins negative and failed-capacity behavior for `list`, `dict`, and `set`
constructors and `reserve`. Successful reserve behavior remains in the
canonical collection run-pass fixture.

The public numeric regression added in this pass pins exact diagnostics and
source-span behavior for unary negation, `abs`, floating and integer power,
operator shifts, and integer shift methods. A trait-dispatch regression also
confirms mutable argument writeback through the resolved public MIR path.

## Defensive closures removed

The following baseline closure regions were redundant wrappers around an
already validated invariant and were rewritten as direct branches or eager
fallbacks: 2355, 2959, 3007, 3011, 3085, 3687, 4147, 4161, 4195, 4209, 4320,
4411, 4435, 6213, 6230, 8370, 8420, 8457, 8472, and 8923.

The rewrite preserves every fallback value and diagnostic. It does not remove
validation: malformed receivers, operands, trait bodies, indexes, and numeric
types still fail at the same runtime boundary if unchecked MIR reaches it.

The compiler coverage denominator changes are:

- functions: 6164 to 6144 (-20)
- lines: 94587 to 94563 (-24)
- regions: 141485 to 141424 (-61)

Within `mir_runtime.rs`, functions change from 510 to 492, lines from 8208 to
8186, and regions from 13605 to 13548.

## Justified retained closures

The remaining missed closures must not receive synthetic malformed-MIR tests:

- Checked-MIR invariants: 835, 1359, 1666, 3084, 3570, 4056, 4401, 4420,
  7271, 9106, 9111, 9137, 9331, 9363, 9365, 9451, 9468, and 9469. These cover
  states excluded by place capabilities, exact static types, generated
  initializer and method bodies, bound argument arity, or exact byte/integer
  collection metadata. Public trait calls lower through resolved dispatch, so
  the generic fallback at 4420 is not the public argument-writeback path.
- Platform or physical-size limits: 4799 and 6221 are 32-bit-only conversion
  failures; 8144 and 8191 require an in-memory task list longer than
  `i64::MAX`.
- Genuine non-S1 product edges retained for their owning suites: negative
  `read_exact` counts at 7497 and 8064, and a null opaque FFI return at 8794.
  They are not malformed states and their diagnostics remain intact.

No coverage-only malformed MIR was executed, and no unreachable branch was
counted by constructing invalid runtime values.

## Verification

Focused tests passed for public numeric diagnostics, trait dispatch and mutable
writeback, try conversion, index normalization, duration arithmetic, round,
and divmod. Focused instrumented coverage passed for both new public tests.

