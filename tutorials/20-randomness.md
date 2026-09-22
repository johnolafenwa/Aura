# Deterministic And Secure Randomness

Aura has two kinds of randomness, and you pick the one whose promise you need:

- A seeded `random.Rng` gives a repeatable sequence. Use it for tests,
  simulations, generated fixtures, and retry jitter.
- The module-level `random.secure_int` and `random.secure_bytes` functions ask
  the operating system for unpredictable values. Use them for secrets.

## A Repeatable Stream

Import the module and keep the generator in a mutable binding:

```aura check-pass
import random

mut rng = random.Rng(42)
print(rng.next_int(0, 10))
print(rng.next_int(-5, 6))
```

This prints `2` and `2`. A new generator with seed `42` starts the same stream
again. Reproducibility depends on both the seed and the call order, because a
different pattern of calls consumes the stream differently.

`next_int(lo, hi)` uses a half-open interval. It can return `lo` but never
`hi`. The bounds are `int64`, may be negative, and must satisfy `lo < hi`.
Aura uses rejection sampling, so every integer in the interval has equal
probability and there is no remainder bias.

`next_float()` returns a `float64` in `[0.0, 1.0)`. It can return zero but
never one.

## Shuffling In Place

`shuffle` takes a mutable borrow of a list and rearranges its elements:

```aura check-pass
import random

mut rng = random.Rng(42)
mut values: list[int64] = [0, 1, 2, 3, 4, 5]
rng.shuffle(values)
print(values)
```

This prints `[3, 5, 4, 1, 2, 0]`. The caller still owns the list. `shuffle`
neither clones nor moves the elements, so it works with move-only element
types too. An empty or one-element list stays unchanged and does not advance
the stream.

## The Generator Is A Move Value

An `Rng` holds evolving state. It is not a copy value, and it has no public
way to clone it:

```aura check-pass
import random

def take_rng(rng: own random.Rng):
    pass

mut rng = random.Rng(7)
take_rng(rng)
# rng.next_float() would be rejected because rng moved.
```

All three generator methods need a mutable receiver. When a helper should
advance the caller's stream without taking ownership, give it a mutable
borrow:

```aura check-pass
import random

def roll(rng: mut random.Rng) -> int64:
    return rng.next_int(1, 7)
```

The state change is then visible at the call, like any other mutation.

Wrapping the generator does not make it cloneable. Aura rejects any collection
copy or cloned collection read that would duplicate an `Rng`, even when the
`Rng` is nested in a class or enum. Moving or removing a generator from a
collection inside one owning task is still valid.

An `Rng` is not `Transfer`, so it cannot be a task result or a Queue payload.
Both of those boundaries fail with `AU3008`. A Queue handle is a copy value
whenever its payload type is admitted. A Task handle is copyable only when its
result is repeatable.

### Generic Code And Clone Safety

The checker does not reject generic code only because its element type is
still unresolved. Instead, it infers requirements:

- If a body copies `list[T]` or does another clone-producing operation, Aura
  infers that `T` must be clone-safe.
- That requirement propagates through other generic calls and through
  imports.
- A concrete specialization with `random.Rng` then fails with `AU3007`.

A trait default body can set up the same requirement. An explicit
implementation cannot add a stronger hidden requirement. Operator traits and
`From` conversions also enforce the contract of the method they select.

## Use OS Randomness For Secrets

The seeded generator is predictable, so it must not create secrets. Use the
secure functions for tokens, nonces, salts, keys, and session identifiers:

```aura check-pass
import random

die_roll = random.secure_int(1, 7)
token_bytes = random.secure_bytes(32)
print(token_bytes.len())
```

Secure calls have no seed and no reproducible sequence. They use only the
operating system's cryptographically secure source. Aura never falls back to
`random.Rng`, a clock, or a process identifier. `secure_bytes(0)` returns an
empty list without requesting entropy.

The `secure_bytes` count is an `int64`. One call accepts at most `2147483647`
bytes. This is a fixed resource and safety ceiling, separate from the public
`list` length domain. A larger count traps with `AU4005` before Aura requests
any allocation or entropy.

These functions return plain values, so an invalid or unavailable request
traps. Aura 0.3 has no `random.Error` type.

| Code | Cause |
| --- | --- |
| `AU4003` | An empty or reversed integer interval, or a negative byte count. |
| `AU4005` | A byte count above the ceiling, an OS entropy failure, or an allocation failure. |

## Compatibility And The Full Contract

Aura 0.3 fixes the algorithm for the whole 0.3.x series: xoshiro256**, its
SplitMix64 seed expansion, the integer mapping, the floating mapping, and the
shuffle order. That makes seeded tests portable across the MIR and direct
backends. It does not make xoshiro secure.

Run the maintained example:

```bash
cargo run -p aura -- run examples/randomness/deterministic_rng.au
```

The normative [Randomness Module](../docs/manual/randomness.md) chapter has
the constants, the state transition, the seed-42 conformance vectors, the
ownership rules, and the secure failure boundary.
