# Randomness

"Give me a random number" hides a question you have to answer before
you write any code: random for what? A test that shuffles its input wants
the *same* shuffle every time it runs, so that a failure can be reproduced.
A session token wants a value nobody on Earth can guess, including someone
who has read your source. Those are opposite requirements, and a library
that serves both from one function will sooner or later hand the first kind
of number to code that needed the second.

Aura's `random` module keeps the two apart by name. `random.Rng` is a
generator you seed yourself, and its output is completely determined by the
seed. The functions whose names start with `secure_` ask the operating
system for unpredictable bytes. There is no global generator and no
default, so every call site shows which kind it is using.

## Unpredictable values

`courier` needs an id for each envelope, so that a receiver can notice the
same envelope arriving twice. The id has to be unique, and it should not be
guessable, or someone could mark ids as "seen" before the real messages
arrive. That makes it the secure kind:

*Filename: src/main.au*

```aura
import bytes
import random

def new_id() -> str:
    return bytes.hex_encode(random.secure_bytes(8))

class Mailbox:
    seen: set[str]

    def accept(mut self, id: str) -> bool:
        if id in self.seen:
            return false
        self.seen.add(id.clone())
        return true

def main():
    first = new_id()
    second = new_id()
    print(first)
    print(second)

    mut mailbox = Mailbox(seen=set[str]())
    print(mailbox.accept(first))
    print(mailbox.accept(second))
    print(mailbox.accept(first))
```

<span class="caption">Listing 20-10: Secure random ids and a mailbox that refuses replays</span>

```console
$ aura run src/main.au
0e0412d7b386fa86
1c4f5e92401ef006
true
true
false
```

Your two ids will be different from ours and different on every run. That
is the whole point. `random.secure_bytes(8)` returns eight bytes from the
operating system's secure source as a `list[uint8]`, and `hex_encode` from
the first section of this chapter turns them into sixteen readable
characters. Eight random bytes give 2⁶⁴ possible ids, far more than enough
to make a collision between honest envelopes a non-event. For a secret,
such as a signing key or a session token, use at least sixteen bytes.

`Mailbox` remembers the ids it has accepted in a set and refuses one it has
seen before, which is the replay check that the last section left open. In
the real tool, `seal` would put the id on a line beside the timestamp,
inside the signed data, and the mailbox would only need to remember ids for
as long as an envelope can stay fresh.

The module has one more secure function. `random.secure_int(lo, hi)` returns
an unpredictable integer from `lo` up to but not including `hi`, for
choosing a one-time code or picking a winner fairly.

The secure functions return plain values, not `Result`s. If the operating
system cannot supply entropy, which is very rare, the call traps. It never
falls back to something weaker, because a token that is silently
predictable is worse than a program that stops.

## Reproducible values

For everything that is not a secret (test data, simulations, sampling,
shuffling a playlist) you want a sequence you can run again. Create a
`random.Rng` with a seed:

*Filename: src/main.au*

```aura
import random

def main():
    mut rng = random.Rng(2026)
    print(rng.next_int(1, 7))
    print(rng.next_int(1, 7))
    print(rng.next_float())

    mut again = random.Rng(2026)
    print(again.next_int(1, 7))
    print(again.next_int(1, 7))

    mut ids = ["7f3a", "91bc", "c04d", "02ee", "5d19"]
    rng.shuffle(ids)
    print(ids)
```

<span class="caption">Listing 20-11: A seeded generator gives the same sequence every time</span>

```console
$ aura run src/main.au
6
5
0.8125094267576175
6
5
[02ee, 7f3a, 5d19, c04d, 91bc]
```

This time your output is the same as ours, digit for digit, and it will be
the same on macOS, Linux, and Windows, under `aura run` and in a built
binary. `again` was created with the same seed as `rng`, so its first two
rolls are the same two rolls. Aura documents the exact algorithm in the
Manual and has kept its output fixed in every release since 0.3, so that a
seed written into a test keeps producing the same data.

`next_int(lo, hi)` returns an integer in the half-open range from `lo` up to
but not including `hi`, so `next_int(1, 7)` is a die. `next_float()` returns
a `float64` from 0.0 up to but not including 1.0. `shuffle` rearranges a
mutable list in place. That is the entire API. There is no `choice`, but
`items[rng.next_int(0, items.len())]` does the job, and no normal
distribution or other shapes.

Every one of those methods changes the generator, because producing a
number advances its internal state. So the binding has to be `mut`, and a
function that draws from a caller's generator takes it as
`rng: mut random.Rng`. Forget either and the compiler says so:

```aura check-fail:AU3003
import random

def main():
    rng = random.Rng(1)
    print(rng.next_int(0, 10))
```

```console
$ aura check src/main.au
error[AU3003]: method `next_int` requires a mutable receiver
 --> src/main.au:5:15
  |
5 |     print(rng.next_int(0, 10))
  |               ^
  = help: declare the receiver place with `mut` before calling a mutating method
```

A generator also cannot be copied. `rng.clone()` is a compile error, and so
is any operation that would clone a value containing one. The reason is
that two copies of one generator produce the same "random" numbers from
that point on, which is a bug that can go unnoticed for a long time: two
workers that were meant to behave independently quietly make identical
choices. For the same reason an `Rng` cannot be sent to another task. When
several tasks need random numbers, give each its own generator with its own
seed, for example the base seed plus the worker's index.

## A seeded generator in practice: retry jitter

Here is a small, real use. When many clients retry a failed request after
exactly the same delay, they all come back at the same moment and knock the
server over again. The standard cure is *jitter*: add a random amount to
each delay so the retries spread out. The delays do not need to be secret,
and in a test we would like them to be predictable:

*Filename: src/main.au*

```aura
import random

def backoff(rng: mut random.Rng, attempt: int64) -> Duration:
    base = 100 * (1 << attempt)
    return Duration.ms(base + rng.next_int(0, base))

def schedule(seed: int64) -> list[Duration]:
    mut rng = random.Rng(seed)
    mut delays: list[Duration] = []
    for attempt in range(4):
        delays.append(backoff(rng, attempt))
    return delays

def main():
    print(schedule(1))
    print(schedule(2))
    print(schedule(1))
```

<span class="caption">Listing 20-12: Exponential backoff with reproducible jitter</span>

```console
$ aura run src/main.au
[157ms, 322ms, 500ms, 1383ms]
[175ms, 282ms, 789ms, 1213ms]
[157ms, 322ms, 500ms, 1383ms]
```

`backoff` doubles a base delay on each attempt, 100, 200, 400, and 800
milliseconds, and adds a random amount up to the base again. Two clients
with different seeds get different schedules. One seed always gets the same
schedule, so a test can assert on the exact delays. In production each
process would pick its seed once at startup, and this is the one place
where the two kinds of randomness meet: `random.Rng(random.secure_int(0,
1000000))` seeds a fast, reproducible generator from an unpredictable
number.

The rule to take away is short. If someone guessing the value would be a
problem, use `secure_bytes` or `secure_int`. Otherwise, use a seeded `Rng`
and write the seed down. The [Randomness Module](/manual/randomness) page
specifies the generator's algorithm exactly, for the rare case where you
need to reproduce its output in another language.

`courier` can now seal, stamp, identify, and verify. What it cannot yet do
is tell anyone what it has been up to.
