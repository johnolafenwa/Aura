# Randomness Module

The `random` module keeps two kinds of randomness apart. A seeded `random.Rng` gives a reproducible pseudo-random stream. The `secure_*` functions draw unpredictable values from the operating system. Construct an explicitly seeded `random.Rng` when results must repeat. Use the `secure_*` functions only when results must be unpredictable.

| API | Signature | Contract |
| --- | --- | --- |
| `random.Rng` | `Rng(seed: int64) -> random.Rng` | Creates one deterministic xoshiro256** stream from the exact signed seed bit pattern. |
| `random.Rng.next_int` | `next_int(lo: int64, hi: int64) -> int64` | Advances the stream and returns a uniform value in `[lo, hi)`. |
| `random.Rng.next_float` | `next_float() -> float64` | Advances the stream and returns a binary64 value in `[0.0, 1.0)`. |
| `random.Rng.shuffle` | `shuffle[T](values: mut list[T]) -> None` | Advances the stream and shuffles `values` in place. |
| `random.secure_int` | `secure_int(lo: int64, hi: int64) -> int64` | Returns a uniform value in `[lo, hi)` from the operating system's secure source. |
| `random.secure_bytes` | `secure_bytes(n: int64) -> list[uint8]` | Returns exactly `n` bytes from the operating system's secure random source. |

The deterministic generator is reproducible. It is not cryptographically secure. Never use `random.Rng` for keys, tokens, nonces, salts, session identifiers, or anything else where predictability could affect security.

The secure calls never use, seed, or advance a deterministic `Rng` stream.

## Example

```aura
import random

def main() -> int32:
    mut rng = random.Rng(42)
    print(rng.next_int(0, 10))
    print(rng.next_int(-5, 6))

    mut values: list[int64] = [0, 1, 2, 3, 4, 5]
    mut shuffle_rng = random.Rng(42)
    shuffle_rng.shuffle(values)
    print(values)
    return 0
```

This prints `2`, `2`, and `[3, 5, 4, 1, 2, 0]` on separate lines. The maintained program is `examples/randomness/deterministic_rng.au`.

## Deterministic Algorithm

This section is normative. It is enough to rebuild Aura's seeded stream without reading the compiler source.

Conventions for this section:

- Every value is an unsigned 64-bit word.
- `+`, `*`, `<<`, and exclusive-or wrap or truncate to 64 bits.
- `rotl(x, k)` rotates `x` left by `k` bits.

### Seed Expansion

First, the signed `int64` seed is reinterpreted as its two's-complement unsigned 64-bit pattern. Set `split_state = seed_bits`. Then run four SplitMix64 steps. Each step does this:

1. `split_state = split_state + 0x9E3779B97F4A7C15`.
2. `z = split_state`.
3. `z = (z xor (z >> 30)) * 0xBF58476D1CE4E5B9`.
4. `z = (z xor (z >> 27)) * 0x94D049BB133111EB`.
5. The step output is `z xor (z >> 31)`.

The four outputs, in order, become `s0`, `s1`, `s2`, and `s3`.

### Raw Draw

One xoshiro256** raw draw computes its result and updates the state in exactly this order:

1. `result = rotl(s1 * 5, 7) * 9`.
2. `t = s1 << 17`.
3. `s2 = s2 xor s0`.
4. `s3 = s3 xor s1`.
5. `s1 = s1 xor s2`.
6. `s0 = s0 xor s3`.
7. `s2 = s2 xor t`.
8. `s3 = rotl(s3, 45)`.

### next_int

`next_int(lo, hi)` first requires `lo < hi`.

1. Let `span` be the exact unsigned mathematical difference `hi - lo`. It is in `1..=2^64 - 1`.
2. Let `threshold = 2^64 mod span`. The equivalent unsigned-wrapping expression is `(-span) mod span`.
3. Draw raw words until one is at least `threshold`.
4. Return the exact signed value `lo + (raw mod span)`.

This rejection step makes every result equally likely. An interval with one value still consumes one raw draw.

### next_float

`next_float()` consumes one raw word and returns `float64(raw >> 11) * 2^-53`. A binary64 value represents the 53-bit integer exactly. So the result is always at least `0.0` and less than `1.0`. It is one of the `2^53` evenly spaced values in that interval.

### shuffle

`shuffle(values)` uses descending Fisher-Yates. For `i` from `len - 1` down to `1` inclusive, it gets `j` by the same rule as `next_int(0, i + 1)` and swaps positions `i` and `j`.

A list of length zero or one is unchanged and consumes no raw draws. A longer shuffle consumes one accepted index draw per iteration, plus any raw draws that the unbiased range mapping rejects.

## Conformance Oracles

These values, the mapping rules above, and the no-draw rule for lists of length zero or one are compatibility tests. They are not only illustrations.

For seed `42`, the initialized state words are:

| Word | Hexadecimal value |
| --- | --- |
| `s0` | `bdd732262feb6e95` |
| `s1` | `28efe333b266f103` |
| `s2` | `47526757130f9f52` |
| `s3` | `581ce1ff0e4ae394` |

The first five raw xoshiro256** results, in order, are:

1. `1546998764402558742`
2. `6990951692964543102`
3. `12544586762248559009`
4. `17057574109182124193`
5. `18295552978065317476`

Fresh seed-42 generators give these public results:

- Consecutive integer calls give `next_int(0, 10) == 2`, `next_int(-5, 6) == 2`, `next_int(-9223372036854775808, 9223372036854775807) == 3321214725393783201`, and `next_int(7, 8) == 7`.
- Consecutive float calls give `0.08386297105988216`, `0.3789802506626686`, and `0.6800434110281394`.
- Shuffling `[0, 1, 2, 3, 4, 5]` gives `[3, 5, 4, 1, 2, 0]`.

## Secure Randomness

`random.secure_int(lo, hi)` samples the half-open interval `[lo, hi)` without modulo bias. It uses fresh bytes from the operating system's cryptographically secure random source. It has no seed and no reproducibility guarantee.

`random.secure_bytes(n)` requires `0 <= n <= 2147483647`. The upper bound is a fixed per-call ceiling on resources and safety for allocation and operating-system entropy requests. It is separate from Aura's public `list` length domain.

- The function allocates a fresh `list[uint8]` and fills it from the same OS source.
- `secure_bytes(0)` returns an empty list without contacting the entropy source.
- A count above the ceiling fails with `AU4005` before any allocation or entropy request.
- For any accepted positive count, Aura returns exactly that many initialized bytes, or fails. It never returns a short list and never substitutes deterministic data.

The exact secure outputs are unspecified by design. Their distribution, length, failure category, and no-fallback rule are specified. Host entropy and allocation availability are external conditions.

## Grammar

The module adds no grammar. `import random`, qualified names, constructor calls, mutable bindings, method calls, named arguments, generic `list[T]`, and ordinary module functions use the forms defined elsewhere in this Manual. There is no random literal and no implicit process-global generator.

## Typing Rules

The signatures in the opening table are normative.

- Seeds, integer bounds, and secure byte counts are `int64`.
- `next_float` returns `float64`.
- Secure bytes use the ordinary bytes representation, `list[uint8]`.
- Bounds are half-open. At run time they must satisfy `lo < hi`.

`random.Rng` is a builtin module type. It is non-copy, and it is not a resource. Its three methods have mutable receivers.

`shuffle` is generic over every element type `T`. It needs no copy, clone, equality, ordering, or user-trait bound, because it only swaps owned list positions in place. The argument must be a mutable `list[T]` place. A supported mutable field projection counts.

### Clone Safety

A generator cannot be cloned, and the rule is transitive. Suppose a type contains `random.Rng` through a collection, a user class, an enum payload, or another ordinary wrapper. Then no operation that would clone the contained generator may be used on it. The checker rejects:

- direct `random.Rng.clone()` calls
- clone-producing collection operations when the produced value would contain an `Rng`: `list.copy`, `list.get`, `dict.copy`, `dict.get`, `dict.keys`, `dict.values`, `dict.items`, and `set.copy`
- clone-producing task observations when the produced value would contain an `Rng`: `Task.result`, `Task.poll`, `Task.result_or`, `wait_any`, and `wait_all`

A polymorphic clone-producing operation over an unresolved type parameter infers a clone-safety obligation instead. The generic declaration stays valid. The obligation propagates through generic-to-generic calls and imports. The checker rejects an unsafe concrete specialization with `AU3007`.

Task and Queue handles are clone barriers. Copying a handle, where that is allowed, does not clone its stored payload. This is a statement about clone safety only. A separate rule forbids `random.Rng` at task-result and Queue-payload boundaries, with `AU3008`. A Task that carries a non-repeatable result is not copyable.

Operations that transfer one owned value within one owning task stay valid. Examples include `list.pop`, `dict.remove`, ordinary moves, and shuffling a `list[random.Rng]` in place.

Clone-safety obligations are part of callable and trait method contracts:

- An obligation inferred by a trait default body is substituted through `Self` and the trait's type arguments, for both concrete and bound-based dispatch.
- An explicit implementation cannot quietly require clone safety that its trait method does not. The checker rejects that strengthening with `AU3007`.
- Operator-trait and `From` dispatch enforce the same contract.

### No Error Type

The module has no `random.Error` enum. Secure operations return plain values. Invalid requests and unavailable host facilities produce runtime diagnostics.

## Runtime Semantics

Constructing an `Rng` applies the exact seed expansion above. Each successful method that consumes state advances that one stream in the specified order.

- `next_int` consumes extra raw words only when rejection sampling needs them.
- `next_float` consumes exactly one.
- `shuffle` consumes according to its loop and rejection rules.
- The secure functions never read or change an `Rng`.

`Rng` defines no equality. The checker rejects `==`, `!=`, membership, list equality searches, set insertion, and use as a dictionary key, with `AU2008`. Generator identity and the four state words cannot be observed.

Rendering through `print` or f-string interpolation gives exactly `<rng>` and does not advance the stream. There is no public way to export or import state. There is also no public way to clone the generator, including through a collection or task-result alias that would clone it indirectly.

Invalid ranges and counts are checked before any value is returned. A secure entropy or allocation failure ends the operation with the diagnostic listed below. Partial output and deterministic fallback output are forbidden.

## Ownership And Evaluation Order

Assigning or passing an `Rng` through an owned position moves it. A moved source cannot be reused.

`next_int`, `next_float`, and `shuffle` need a mutable generator place. An ordinary immutable binding is not enough. A function that should advance a caller's stream takes `rng: mut random.Rng`.

Moving a generator into or out of a collection keeps its single owner. Cloning an enclosing value would not, so the transitive clone rules in [Typing Rules](#typing-rules) apply even when the generator is nested several type layers deep.

An allowed copy of a Task or Queue handle is different. It copies the handle's identity, not the `Rng` value behind it, so it does not duplicate generator state. Queue and Task handle state is synchronized for use across workers. The `Rng` itself is not `Transfer` and stays on its owning task. `Transfer` is the property of a value that may cross a task or Queue boundary. See [Concurrency](/manual/concurrency).

`shuffle(values: mut list[T])` borrows the caller's list exclusively, mutates that same place, and returns `None`. It does not move, clone, or replace the list or its elements. A projected mutable list gets the same writeback behavior as a root binding.

Ordinary call order applies. The receiver is evaluated before the supplied arguments. The supplied arguments are evaluated in call-site source order, even when named. Generator state changes at the position of the method call.

The secure functions share no generator state. Each call makes its own OS request, except for the zero-byte fast path described above.

## Diagnostics

| Code | Cause |
| --- | --- |
| `AU2001` | The `random` name is unavailable, or a member is unknown. |
| `AU2002` | A seed, bound, byte count, receiver, list, or return type does not match. |
| `AU2004` | Invalid arity, argument names, or positional and named binding. |
| `AU2006` | A trait method on `random.Rng` has the same name as a builtin `Rng` method. |
| `AU2008` | Equality, membership, set insertion, or dictionary-key use on an `Rng`. |
| `AU3001` | Reuse of a moved generator. |
| `AU3002` | Conflicting borrows of the mutable receiver or the shuffled list. Includes passing an immutable list place to `shuffle`, and shuffling a list while its exclusive borrow is unavailable. |
| `AU3003` | A state-consuming method called through an immutable generator place. |
| `AU3007` | Direct or transitive use of a clone-producing operation on a value that contains, or may contain, `random.Rng` state. Also an unsafe generic specialization, a concrete clone requirement that cannot be proved, or a trait implementation that strengthens its declared contract. |
| `AU3008` | `random.Rng` at a task-result or Queue-payload boundary. |
| `AU4003` | `lo >= hi` in either integer function, or a negative `secure_bytes` count. |
| `AU4005` | A byte count above the fixed secure-random request ceiling of `2147483647`, a failure to get secure operating-system entropy, or a failure to allocate or fill the requested secure byte list. |

The over-limit `AU4005` comes before any allocation or entropy request. The public return types are plain values, so these runtime conditions are diagnostics, not `Result` or `random.Error` values.

## Backend Support

The MIR runtime and the direct native backend implement the same deterministic algorithm. MIR is the compiler's mid-level intermediate representation. They match on seed reinterpretation, the rejection threshold, float mapping, shuffle order, rendering, ownership, and diagnostics. For one seed and call sequence, deterministic output MUST be bit-for-bit identical across backends and supported hosts.

Secure output is not compared byte for byte between runs or backends. Both backends MUST:

- use the host's secure random facility
- keep the exact length and half-open uniformity contracts
- use the same diagnostic categories
- never fall back to the deterministic generator

## Limits And Implementation-Defined Behavior

The deterministic API has one stream algorithm. It has none of these:

- a process-global generator
- a reseeding method
- state serialization
- jump or substream operations
- a distribution library
- a random choice helper
- a public clone route

Integer sampling covers only `int64` half-open ranges. Float sampling covers only uniform `float64` values in `[0.0, 1.0)`. Shuffle works only on mutable `list[T]` values.

The secure byte count is an `int64`, but each request is capped at `2147483647`. This is a fixed secure-random ceiling for resources and safety. It does not define or narrow the public `list` length domain or the result of `list.len()`. Within that ceiling, the allocation must also fit the host address space and allocator. Either failure reports `AU4005`.

The operating system chooses the secure entropy implementation and the actual values returned. Secure calls have no deterministic ordering across calls, tasks, backends, processes, or hosts.

The deterministic algorithm and seeded results are not implementation-defined. They stay fixed, as specified above, throughout the Aura 0.3.x series. They are unsuitable for cryptography even when the seed is secret.

## Status

The constructor, deterministic methods, secure functions, move-only ownership, backend parity, and the diagnostics on this page are maintained Aura 0.3 API. The exact algorithm, mapping, compatibility window, identity and rendering policy, and secure-failure boundary are accepted design decisions.

Aura 0.3 has no other random distributions, no secure float function, no global generator, no derived sampling trait, and no `random.Error` type.

Design records: [ADR-0020: Randomness algorithm and security boundary](https://github.com/johnolafenwa/Aura/blob/main/architecture_docs/decisions/0020-randomness-algorithm-and-security-boundary.md) and [ADR-0033: Structural Transfer and task-result consumption](https://github.com/johnolafenwa/Aura/blob/main/architecture_docs/decisions/0033-structural-transfer-and-task-results.md).
