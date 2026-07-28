# ADR-0020: Randomness algorithm and security boundary

- Status: Accepted
- Date: 2026-07-22
- Amended: 2026-07-26 (B3.0-d secure-byte resource ceiling clarification)
- Amended: 2026-07-28 by Provisional ADR-0033 for task/Queue boundary availability
- Roadmap decision: Phase 3 Randomness gap-fill policy

> **Phase 5.6 boundary amendment.** The clone-safety barrier remains true: an
> allowed Task or Queue handle copy does not clone its payload. Provisional
> ADR-0033 separately makes `random.Rng` non-Transfer, so a task may not return
> it and a Queue may not transport it. Those boundary rejections use `AU3008`;
> the older examples below do not provide an escape from Transfer checking.

## Context

The Phase 3 roadmap requires a deterministic seeded generator with integer,
floating-point, and in-place shuffle operations, plus separate operating-system
secure integer and byte generation. It deliberately leaves the deterministic
algorithm, seed mapping, range sampling, stream ownership, compatibility
window, secure-failure policy, and exact shuffle draw order to a documented
gap-fill decision.

Those choices are observable. Changing any of them can change simulations,
tests, retry jitter, generated data, and every shuffled order even when source
and seed stay unchanged. Treating a deterministic generator as secure would
also create a serious API-category error.

## Accepted decision

- `random.Rng(seed: int64)` initializes xoshiro256** state from the seed's
  two's-complement 64-bit pattern. Four consecutive SplitMix64 outputs become
  the four xoshiro state words. All generator arithmetic is wrapping unsigned
  64-bit arithmetic.
- The xoshiro256** raw result and state transition are the published
  `rotl(s1 * 5, 7) * 9` variant. The complete constants, shifts, transition,
  range-rejection rule, and conformance vectors are normative in
  `docs/manual/randomness.md`.
- `next_int(lo, hi)` samples the half-open interval `[lo, hi)` uniformly. It
  rejects `lo >= hi`; accepted draws use modulo only after rejecting the short
  low prefix of the 64-bit domain, so modulo bias is impossible.
- `next_float()` uses the high 53 bits of one raw word and divides by `2^53`,
  producing a binary64 value in `[0.0, 1.0)`.
- `shuffle(values)` is descending Fisher-Yates. For each index from `len - 1`
  through `1`, it draws an integer from `[0, index + 1)` and swaps those two
  positions. Length-zero and length-one vectors consume no draws.
- `random.Rng` is a non-copy, mutable, opaque value. Assignment moves it,
  state-consuming methods require a mutable receiver, equality compares
  generator identity, and human rendering is exactly `<rng>`. No public
  operation may clone it directly or through a containing collection, user
  class, enum, generic value, or clone-producing task-result observation.
  Such an operation is rejected with `AU3007`.
- A clone-producing operation over unresolved generic types infers a
  clone-safety obligation instead of rejecting the generic declaration. The
  obligation is part of the callable contract, propagates through
  generic-to-generic calls and imports, and is discharged after concrete
  substitution. Task and Queue handles stop structural traversal because
  copying them does not observe their payload.
- Task and Queue handles are clone-safety barriers: an allowed handle copy
  does not observe or duplicate its payload. Under Provisional ADR-0033,
  `random.Rng` is not available as a task result or Queue payload, and a Task
  holding a non-repeatable result is not copyable. Moving or removing an `Rng`
  and shuffling owned generator elements in one owning task remain permitted
  because they transfer or rearrange ownership without cloning state.
- The deterministic stream, integer mapping, floating mapping, and shuffle
  order are stable throughout the Aurora 0.1.x compatibility series. A future
  series may select another generator only through an explicit compatibility
  decision and documentation update.
- `random.secure_int(lo, hi)` and `random.secure_bytes(n)` obtain randomness
  exclusively from the host operating system. They never fall back to
  xoshiro256**, clocks, process identifiers, or any other deterministic or
  weak source. `secure_bytes(0)` returns an empty vector without requesting
  entropy.
- `secure_bytes` accepts at most `2147483647` bytes as a fixed per-call
  secure-random request and resource ceiling. This ceiling bounds allocation
  and operating-system entropy work; it is independent of the public `Vec`
  length domain and the result reported by `Vec.len()`. A larger count traps
  with `AU4005` before allocation or entropy is requested. Invalid integer
  intervals and negative byte counts trap with `AU4003`; host-entropy and
  allocation failures trap with `AU4005`. These functions return plain values
  and there is no `random.Error` type.
- Builtin class behavior is attached to compiler-synthesized declaration
  origin, not merely the string pair `random` and `Rng`. A user entry module
  named `random` and an imported user module whose file is named `random.au`
  may define ordinary `Rng` classes without acquiring builtin construction,
  analysis, lowering, or clone-safety behavior.
- The deterministic and secure surfaces are separate: secure calls neither use
  nor advance an `Rng`, and an `Rng` is not suitable for keys, tokens, nonces,
  salts, session identifiers, or other security-sensitive values.
- Trait implementations for `random.Rng` may not define or inherit a method
  whose name collides with one of its builtin methods. Such a collision is
  rejected with `AU2006`, matching the existing builtin-handle dispatch
  protection.
- A clone-safety obligation inferred by a trait default body is part of that
  method's contract and is substituted through `Self` and trait/method type
  arguments. An explicit implementation may satisfy the contract but may not
  strengthen it with an obligation absent from the trait method; strengthening
  is rejected with `AU3007`. Operator-trait and `From` dispatch enforce the same
  contract.

These choices were accepted at the Batch 3 entry checkpoint.

The 2026-07-26 B3.0-d amendment preserves the exact `secure_bytes` upper bound
while classifying it as a per-call resource and safety ceiling independent of
collection-length semantics.

## Completion tests

- Pure generator tests pin SplitMix64 initialization, raw xoshiro outputs,
  signed-seed bit preservation, integer rejection sampling, 53-bit floats, and
  zero/one-length shuffle draw behavior.
- MIR and direct run-pass fixtures pin the seed-42 integer, float, and shuffle
  vectors, named arguments, projected-vector writeback, qualified and imported
  construction, generator identity, exact `<rng>` rendering, and backend
  equality.
- Static checker tests pin move-only ownership, `AU3003` for an immutable
  generator receiver, `AU3002` for an unavailable exclusive shuffle borrow,
  and `AU3007` across direct, nested, generic, collection-observer, and
  task-observer clone routes. Generic-to-generic and imported propagation,
  qualified nominal identity, trait/default/associated/operator/`From`
  dispatch, impl no-strengthening, and terminating recursive type inspection
  are pinned as part of the same contract. Tests also pin the safe handle-copy
  and value-transfer boundaries. The `random_transitive_clone_rejected` fixture
  provides maintained source-level coverage of the transitive rejection.
- Secure-function tests inject entropy success/failure and pin invalid bounds,
  negative counts, the secure-byte request ceiling before allocation/entropy,
  zero-byte no-entropy behavior, returned byte length, and the absence of a
  deterministic fallback. Run-fail fixtures cover invalid deterministic and
  secure bounds, a negative byte count, the maximum accepted secure-byte
  request boundary, and secure resource failures with `AU4005`.
- Path-level checking, analysis, MIR, direct-object, and execution tests pin
  user `Rng` classes in both an entry `random.au` and an imported user
  `random.au` module.
- Analysis/LSP tests pin the `random` module, `random.Rng`, constructor,
  methods, parameter names, return types, and builtin-member completions.
