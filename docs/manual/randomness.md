# Randomness Module

Aurora separates reproducible pseudo-random streams from security-sensitive
operating-system randomness. Import `random`, construct an explicitly seeded
`random.Rng` when results must repeat, and use the module's `secure_*`
functions only when results must be unpredictable.

| API | Signature | Contract |
| --- | --- | --- |
| `random.Rng` | `Rng(seed: int64) -> random.Rng` | Creates one deterministic xoshiro256** stream from the exact signed seed bit pattern. |
| `random.Rng.next_int` | `next_int(lo: int64, hi: int64) -> int64` | Advances the stream and returns a uniform value in `[lo, hi)`. |
| `random.Rng.next_float` | `next_float() -> float64` | Advances the stream and returns a binary64 value in `[0.0, 1.0)`. |
| `random.Rng.shuffle` | `shuffle[T](values: mut Vec[T]) -> None` | Advances the stream while shuffling `values` in place. |
| `random.secure_int` | `secure_int(lo: int64, hi: int64) -> int64` | Returns an OS-secure uniform value in `[lo, hi)`. |
| `random.secure_bytes` | `secure_bytes(n: int64) -> Vec[uint8]` | Returns exactly `n` bytes from the operating system's secure random source. |

The deterministic generator is reproducible, not cryptographically secure.
Never use `random.Rng` for keys, tokens, nonces, salts, session identifiers, or
anything whose predictability could affect security. Secure calls do not use,
seed, or advance any deterministic `Rng` stream.

## Deterministic Algorithm

This section is normative and is sufficient to reconstruct Aurora's seeded
stream without consulting the compiler implementation. Let every value in this
section be an unsigned 64-bit word, let `+`, `*`, `<<`, and exclusive-or wrap or
truncate to 64 bits, and let `rotl(x, k)` rotate `x` left by `k` bits.

The signed `int64` seed is first reinterpreted as its two's-complement unsigned
64-bit pattern. Starting with `split_state = seed_bits`, each of four
SplitMix64 steps performs:

1. `split_state = split_state + 0x9E3779B97F4A7C15`.
2. `z = split_state`.
3. `z = (z xor (z >> 30)) * 0xBF58476D1CE4E5B9`.
4. `z = (z xor (z >> 27)) * 0x94D049BB133111EB`.
5. The step output is `z xor (z >> 31)`.

The four consecutive outputs become `s0`, `s1`, `s2`, and `s3`. One
xoshiro256** raw draw then returns and transitions in this exact order:

1. `result = rotl(s1 * 5, 7) * 9`.
2. `t = s1 << 17`.
3. `s2 = s2 xor s0`.
4. `s3 = s3 xor s1`.
5. `s1 = s1 xor s2`.
6. `s0 = s0 xor s3`.
7. `s2 = s2 xor t`.
8. `s3 = rotl(s3, 45)`.

`next_int(lo, hi)` first requires `lo < hi`. Let `span` be the exact unsigned
mathematical difference `hi - lo`, which is in `1..=2^64 - 1`. Let
`threshold = 2^64 mod span`, equivalently the unsigned-wrapping expression
`(-span) mod span`. Draw raw words until one is at least `threshold`, then
return the exact signed value `lo + (raw mod span)`. This rejection makes every
result equally likely. A one-value interval still consumes one raw draw.

`next_float()` consumes one raw word and returns
`float64(raw >> 11) * 2^-53`. The 53-bit integer is exactly representable as a
binary64 value, so the result is always at least `0.0`, always less than `1.0`,
and is selected from the `2^53` evenly spaced values in that interval.

`shuffle(values)` uses descending Fisher-Yates. For `i` from `len - 1` down to
`1`, inclusive, it obtains `j` through the same `next_int(0, i + 1)` rule and
swaps positions `i` and `j`. A vector of length zero or one is unchanged and
consumes no raw draws. Longer shuffles consume one accepted index draw per
iteration plus any raw draws rejected by the unbiased range mapping.

## Conformance Oracles

For seed `42`, the initialized state words are:

| Word | Hexadecimal value |
| --- | --- |
| `s0` | `bdd732262feb6e95` |
| `s1` | `28efe333b266f103` |
| `s2` | `47526757130f9f52` |
| `s3` | `581ce1ff0e4ae394` |

The first five raw xoshiro256** results are, in order:

1. `1546998764402558742`
2. `6990951692964543102`
3. `12544586762248559009`
4. `17057574109182124193`
5. `18295552978065317476`

Fresh seed-42 generators produce these public results:

- consecutive integer calls produce `next_int(0, 10) == 2`,
  `next_int(-5, 6) == 2`,
  `next_int(-9223372036854775808, 9223372036854775807) == 3321214725393783201`,
  and `next_int(7, 8) == 7`
- consecutive floating calls produce `0.08386297105988216`,
  `0.3789802506626686`, and `0.6800434110281394`
- shuffling `[0, 1, 2, 3, 4, 5]` produces `[3, 5, 4, 1, 2, 0]`

These values, the mapping rules above, and the no-draw rule for zero/one-length
shuffles are compatibility tests, not merely illustrative examples.

## Secure Randomness

`random.secure_int(lo, hi)` samples the half-open interval `[lo, hi)` without
modulo bias using fresh bytes from the operating system's cryptographically
secure random source. It has no seed and no reproducibility guarantee.

`random.secure_bytes(n)` requires `0 <= n <= 2147483647`. The upper bound is a
fixed per-call resource and safety ceiling for allocation and operating-system
entropy requests, independently of Aurora's public `Vec` length domain. The
function allocates a fresh `Vec[uint8]` and fills it from that same OS source.
`secure_bytes(0)` returns an empty vector without contacting the entropy source.
A count above the secure-random request ceiling fails with `AU4005` before
allocation or entropy is requested. For any accepted positive count, Aurora
either returns exactly that many initialized bytes or fails; it never returns
a short vector and never substitutes deterministic data.

The exact secure outputs are intentionally unspecified. Their distribution,
length, failure category, and no-fallback rule are specified. Host entropy and
allocation availability remain external conditions.

## Example

```python
import random

def main() -> int32:
    mut rng = random.Rng(42)
    print(rng.next_int(0, 10))
    print(rng.next_int(-5, 6))

    mut values: Vec[int64] = [0, 1, 2, 3, 4, 5]
    mut shuffle_rng = random.Rng(42)
    shuffle_rng.shuffle(values)
    print(values)
    return 0
```

This prints `2`, `2`, and `[3, 5, 4, 1, 2, 0]` on separate lines. The maintained
program is `examples/randomness/deterministic_rng.au`.

## Grammar

The module adds no source-language grammar. `import random`, qualified names,
constructor calls, mutable bindings, method calls, named arguments, generic
`Vec[T]`, and ordinary module functions use the forms defined elsewhere in
this Manual. There is no random literal and no implicit process-global
generator.

## Typing Rules

The signatures in the opening table are normative. Seeds, integer bounds, and
secure byte counts are `int64`; `next_float` returns `float64`; secure bytes
are the ordinary bytes representation `Vec[uint8]`. Bounds are half-open and
must satisfy `lo < hi` at runtime.

`random.Rng` is a non-copy, non-resource builtin module type. Its three
methods have mutable receivers. `shuffle` is generic over every element type
`T`; it requires no copy, clone, equality, ordering, or user-trait bound
because it only exchanges owned vector positions in place. The argument must
be a mutable `Vec[T]` place, including a supported mutable field projection.

The no-clone rule is transitive. A type that contains `random.Rng`, whether
through a collection, user class, enum payload, or another ordinary value
wrapper, cannot be used with an operation that would clone the contained
generator.
This rejects direct `random.Rng.clone()` calls and clone-producing collection
or task observations such as `Vec.clone`, `Vec.get`, `Map.clone`, `Map.get`,
`Map.keys`, `Map.values`, `Map.items`, `Map.entries`, `Set.clone`,
`Task.result`, `Task.result_or_none`, `Task.result_or`, `wait_any`, and
`wait_all` when the produced value would contain an `Rng`. A polymorphic
clone-producing operation over an unresolved type parameter instead infers a
clone-safety obligation. The generic declaration remains valid, the obligation
propagates through generic-to-generic calls and imports, and an unsafe concrete
specialization is rejected with `AU3007`.

Task and Queue handles are clone barriers: copying a `Task[random.Rng]` or
`Queue[random.Rng]` handle does not clone the
stored or queued generator and remains valid, including when those handles are
elements of a cloned collection. Operations that transfer one owned value
instead of duplicating it also remain valid: examples include `Vec.pop`,
`Vec.remove`, `Map.remove`, ordinary moves, queue receive operations, and
shuffling a `Vec[random.Rng]` in place.

Clone-safety obligations are part of callable and trait method contracts. An
obligation inferred by a trait default body is substituted through `Self` and
the trait's type arguments for concrete and bound-based dispatch. An explicit
implementation cannot silently require clone safety that its trait method does
not require; such strengthening is rejected with `AU3007`. Operator-trait and
`From` dispatch enforce the same contract.

The module exposes no `random.Error` enum. Secure operations return plain
values and use runtime diagnostics for invalid requests or unavailable host
facilities.

## Runtime Semantics

Constructing an `Rng` applies the exact seed expansion above. Each successful
state-consuming method advances that one stream in the specified order.
`next_int` may consume additional raw words only when rejection sampling
requires them; `next_float` consumes exactly one; shuffle consumes according
to its loop and rejection rules. Secure functions do not observe or mutate an
`Rng`.

Two `Rng` values compare equal only when they designate the same generator
identity; equality does not compare or expose their four state words. Human
rendering through `print` or f-string interpolation is exactly `<rng>` and
does not advance the stream. No public state export or import exists. There is
also no public operation that clones the generator, including a collection or
task-result alias that would clone it indirectly.

Invalid ranges and counts are checked before producing a return value. Secure
entropy or allocation failure terminates the operation with the diagnostic
specified below; partial or deterministic fallback output is forbidden.

## Ownership And Evaluation Order

Assigning or passing an `Rng` through an owned position moves it. A moved
source cannot be reused. `next_int`, `next_float`, and `shuffle` require a
mutable generator place; an ordinary immutable binding is insufficient. A
function that should advance a caller's stream takes
`rng: mut random.Rng`.

Moving a generator into or out of a collection preserves its single owner.
Cloning an enclosing value would not, so the transitive clone restrictions in
the typing rules apply even when the generator is nested several type layers
deep. A copied task or queue handle is different: it aliases the synchronization
handle, not the `Rng` value behind that handle, and therefore does not duplicate
generator state.

`shuffle(values: mut Vec[T])` borrows the caller's vector exclusively,
mutates that same place, and returns `None`; it does not move, clone, or replace
the vector or its elements. Projected mutable vectors receive the same
writeback semantics as root bindings.

Ordinary call order applies: the receiver is evaluated before supplied
arguments, and supplied arguments are evaluated in call-site source order even
when named. Generator state changes occur at the method-call position. The
secure functions have no shared generator state; each call performs its own
OS request except the specified zero-byte fast path.

## Diagnostics

`AU2001` reports an unavailable `random` name or unknown member. `AU2002`
reports seed, bound, byte-count, receiver, vector, or return-type mismatches.
`AU2004` reports invalid arity, argument names, or positional/named binding.
`AU2006` rejects a trait method on `random.Rng` whose name collides with a
builtin `Rng` method.

`AU3001` reports reuse of a moved generator. `AU3002` reports conflicting
borrows involving the mutable receiver or shuffled vector, including passing
an immutable vector place to `shuffle` or trying to shuffle a vector while its
exclusive borrow is unavailable. `AU3003` reports a state-consuming method
called through an immutable generator place. `AU3007` rejects direct or
transitive use of a clone-producing operation when its produced value
contains, or may contain, non-cloneable `random.Rng` state. It also reports an
unsafe generic specialization, an unprovable concrete clone requirement, or a
trait implementation that would strengthen its declared contract.

`AU4003` reports `lo >= hi` for either integer function and a negative
`secure_bytes` count. `AU4005` reports a byte count above the fixed
secure-random request ceiling of `2147483647`, failure to obtain secure
operating-system entropy, or failure to allocate/fill the requested secure byte
vector. The over-limit diagnostic is emitted before any allocation or entropy
request. Because the public return types are plain values, these runtime
conditions are diagnostics, not `Result` or `random.Error` values.

## Backend Support

The MIR runtime and direct native backend implement the same deterministic
algorithm, seed reinterpretation, rejection threshold, float mapping, shuffle
order, identity equality, rendering, ownership, and diagnostics. For one seed
and call sequence, deterministic output MUST be bit-for-bit identical across
backends and supported hosts.

Secure output is not compared byte-for-byte between executions or backends.
Both backends MUST use the host's secure random facility, preserve the exact
length and half-open uniformity contracts, use the same diagnostic categories,
and provide no fallback to the deterministic generator.

## Limits And Implementation-Defined Behavior

The deterministic surface has only one stream algorithm and no process-global
generator, reseeding method, state serialization, jump/substream operation,
distribution library, random choice helper, or public clone route. Integer
sampling is limited to `int64` half-open ranges, floating sampling to uniform
`float64` values in `[0.0, 1.0)`, and shuffle to mutable `Vec[T]` values.

Secure byte count is an `int64`, but each request is capped at `2147483647` as
a fixed secure-random resource and safety ceiling. The ceiling does not define
or narrow the public `Vec` length domain or the result of `Vec.len()`. Within
that request ceiling, the allocation must also fit the host address space and
allocator. Either failure reports `AU4005`. The operating system chooses the
secure entropy implementation and actual returned values. No deterministic
ordering relationship exists between secure calls, tasks, backends, processes,
or hosts.

The deterministic algorithm and seeded results are not
implementation-defined: they are stable throughout the Aurora 0.1.x series as
fixed above. They remain unsuitable for cryptography regardless of seed
secrecy.

## Status

The constructor, deterministic methods, secure functions, move-only ownership,
backend parity, and documented diagnostics are maintained Aurora 0.1 surface.
The exact algorithm, mapping, compatibility window, identity/rendering policy,
and secure-failure boundary are accepted under ADR-0020.

No other random distributions, secure floating function, global generator,
derived sampling trait, or `random.Error` type is part of Aurora 0.1.
