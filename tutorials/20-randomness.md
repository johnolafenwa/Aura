# Deterministic And Secure Randomness

Aura makes you choose which promise you need. A seeded `random.Rng` gives a
repeatable sequence for tests, simulations, generated fixtures, and retry
jitter. The module-level `random.secure_int` and `random.secure_bytes`
functions ask the operating system for unpredictable values.

## A Repeatable Stream

Import the module and keep the generator in a mutable binding:

```python
import random

mut rng = random.Rng(42)
print(rng.next_int(0, 10))
print(rng.next_int(-5, 6))
```

This prints `2` and `2`. Reconstructing a generator with seed `42` starts the
same stream again. A different pattern of calls consumes the stream
differently, so reproducibility depends on both the seed and call order.

`next_int(lo, hi)` uses a half-open interval: `lo` can be returned and `hi`
cannot. The bounds are `int64`, may be negative, and must satisfy `lo < hi`.
Aura uses rejection sampling internally, giving every integer in the interval
equal probability and avoiding remainder bias.

`next_float()` returns a `float64` in `[0.0, 1.0)`. It can return zero and can
never return one.

## Shuffling In Place

`shuffle` mutably borrows a list and rearranges its existing elements:

```python
import random

mut rng = random.Rng(42)
mut values: list[int64] = [0, 1, 2, 3, 4, 5]
rng.shuffle(values)
print(values)
```

The result is `[3, 5, 4, 1, 2, 0]`. The list stays owned by the caller. The
method neither clones nor moves its elements, so it works with move-only
element types too. Empty and one-element vectors are unchanged and do not
advance the stream.

## The Generator Is A Move Value

An `Rng` contains evolving state. It is deliberately non-copy and has no
public clone route:

```python
import random

def take_rng(rng: own random.Rng):
    pass

mut rng = random.Rng(7)
take_rng(rng)
# rng.next_float() would be rejected because rng moved.
```

The three generator methods need a mutable receiver. If a helper should
advance the caller's stream without taking ownership, give it a mutable borrow:

```python
import random

def roll(rng: mut random.Rng) -> int64:
    return rng.next_int(1, 7)
```

This makes state flow visible at the same call boundary as any other mutation.

Wrapping the generator does not make it cloneable. Aura rejects collection
copies and cloned collection reads that would duplicate an `Rng`, even when it
is nested inside a class or enum. Moving or removing a generator from a
collection within one owning task remains valid. An `Rng` is not `Transfer`,
so it cannot be a task result or Queue payload: those boundaries fail with
`AU3008`. Queue handles remain copy values for admitted payload types, while a
Task handle is copyable only when its result is repeatable.

Generic code is not rejected merely because its element type is unresolved.
If a body copies `list[T]` or performs another clone-producing operation, Aura
infers that `T` must be clone-safe. The requirement propagates through other
generic calls and imports, then a concrete `random.Rng` specialization fails
with `AU3007`. A trait default body can establish the same contract; an
explicit implementation cannot add a stronger hidden requirement. Operator
traits and `From` conversions enforce the selected method's contract too.

## Use OS Randomness For Secrets

The deterministic generator is predictable and must not create secrets. Use
the secure functions for tokens, nonces, salts, keys, and session identifiers:

```python
import random

die_roll = random.secure_int(1, 7)
token_bytes = random.secure_bytes(32)
print(token_bytes.len())
```

Secure calls have no seed and no reproducible sequence. They use only the
operating system's cryptographically secure source; Aura never falls back to
`random.Rng`, a clock, or a process identifier. `secure_bytes(0)` returns an
empty list without requesting entropy.

The `secure_bytes` count is `int64`. Each call accepts at most `2147483647`
bytes as a fixed resource and safety ceiling independent of the public `list`
length domain. A larger count traps with `AU4005` before Aura requests either
allocation or entropy.

Invalid or unavailable requests trap because these functions return plain
values: an empty/reversed integer interval or negative byte count is `AU4003`,
while a secure byte count above the ceiling, OS entropy failure, or allocation
failure is `AU4005`. Aura 0.2 has no `random.Error` type.

## Compatibility And The Full Contract

Aura 0.2 fixes xoshiro256** plus its SplitMix64 seed expansion, integer
mapping, floating mapping, and shuffle order for the complete 0.2.x series.
That promise makes seeded tests portable across the MIR and direct backends.
It does not make xoshiro secure.

Run the maintained example:

```bash
cargo run -p aura -- run examples/randomness/deterministic_rng.au
```

For the constants, state transition, seed-42 conformance vectors, ownership
rules, and secure failure boundary, read the normative
[Randomness Module](../docs/manual/randomness.md) chapter.
