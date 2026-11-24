# Several numbers at once with lane values

The last stop in this chapter is for the loops that spend their whole life
doing arithmetic on a list of numbers. Aura already makes most of them fast.
`Array` methods run tuned kernels, and a plain counted loop that reads
`xs[i]` and writes `ys[i]` is turned into vector instructions by the native
compiler without any help. One common kind of loop gets neither benefit,
and it is the one we will speed up here.

Suppose we have a second of audio as a list of samples between -1.0 and 1.0,
and we want its *energy*, the sum of the squares of the samples:

```aura
def energy(samples: list[float64]) -> float64:
    mut total = 0.0
    for i in range(len(samples)):
        total += samples[i] * samples[i]
    return total

def main():
    print(energy([0.5, -0.25, 1.0, 0.75, -0.5]))
```

```console
$ aura run src/main.au
2.125
```

A modern CPU can add four `float64` values to four others with one
instruction. It would like to keep four running totals side by side, add
four squares to them on each step, and combine the four totals at the end.
But that adds the numbers in a different order from the one we wrote, and
with floating point the order of additions can change the last digits of
the result. Aura promises that a program prints the same floats on every
machine and under both `aura run` and `aura build`, so the compiler adds
exactly in the order of the source: one element at a time.

The `simd` module is how you tell Aura that a different order is fine, by
writing that order yourself. (SIMD stands for "single instruction, multiple
data".)

## Lane values

A *lane value* holds a fixed number of numbers side by side. Each slot is
called a lane. `simd.F64x4` has four `float64` lanes:

*Filename: src/main.au*

```aura
from simd import F64x4

def main():
    a = F64x4(l0=1.0, l1=2.0, l2=3.0, l3=4.0)
    b = F64x4.splat(10.0)
    print((a + b).to_list())
    print((a * a).to_list())
    print(a.max(F64x4.splat(2.5)).to_list())
    print(a.l2)
    print(a.sum())
```

<span class="caption">Listing 8-34: Building lane values and operating on every lane</span>

```console
$ aura run src/main.au
[11.0, 12.0, 13.0, 14.0]
[1.0, 4.0, 9.0, 16.0]
[2.5, 2.5, 3.0, 4.0]
3.0
10.0
```

An `F64x4` is an ordinary copy class with four public fields, `l0` to `l3`,
so you can build one by naming its fields and read a lane with `a.l2`.
`splat` fills every lane with one number. The operators `+`, `-`, `*`, and
`/` work lane by lane: lane 0 of the result comes from lane 0 of each
operand, and so on. So do the methods `min`, `max`, `abs`, and `sqrt`.
`to_list()` copies the lanes into a new list, and `sum()` adds them into one
number.

Both operands of an operator have to be the same lane class. `a * 2.0` is a
compile error, because `2.0` is a `float64` and not an `F64x4`. Write
`a * F64x4.splat(2.0)`. That is the same rule as everywhere else in Aura:
nothing converts implicitly.

There are six lane classes. `F64x2` and `F64x4` hold `float64`, `F32x4` and
`F32x8` hold `float32`, and `I64x2` and `I64x4` hold `int64`.

## A lane loop

To use lanes on a list we need to get list elements into them.
`F64x4.load(samples, i)` reads the four elements starting at index `i`. With
it, the energy loop becomes:

*Filename: src/main.au*

```aura
from simd import F64x4

def energy(samples: list[float64]) -> float64:
    mut total = 0.0
    for i in range(len(samples)):
        total += samples[i] * samples[i]
    return total

def energy_lanes(samples: list[float64]) -> float64:
    mut acc = F64x4.splat(0.0)
    for i in range(0, len(samples) - 3, 4):
        lanes = F64x4.load(samples, i)
        acc = acc + lanes * lanes
    mut total = acc.sum()
    for i in range(len(samples) - len(samples) % 4, len(samples)):
        total += samples[i] * samples[i]
    return total

def main():
    samples = [0.5, -0.25, 1.0, 0.75, -0.5, 0.125]
    print(energy(samples))
    print(energy_lanes(samples))
```

<span class="caption">Listing 8-35: Summing squares four samples at a time</span>

```console
$ aura run src/main.au
2.140625
2.140625
```

`energy_lanes` has three parts, and every lane loop you write will have the
same three.

The main loop steps by four. Its bound is `len(samples) - 3`, so it runs
only while four whole elements are left. Each step loads four samples,
squares all four with one `*`, and adds the four squares to `acc`. After the
loop, `acc` holds four separate running totals: lane 0 has the squares of
samples 0, 4, 8 and so on, lane 1 those of samples 1, 5, 9, and so on.

`acc.sum()` combines the four totals into one number.

The last loop is the *tail*. A list of six samples has one group of four and
two left over, and `load` cannot read a group that hangs off the end of the
list: like any index, that traps with `AU4003`. `len(samples) - len(samples)
% 4` is where the main loop stopped, and the tail finishes the remaining
zero to three samples one at a time.

The two functions printed the same number here because every value involved
is exact in binary. On real data they agree to about fifteen digits and can
differ after that, which is the price of the new order. We will see it in
the next listing.

## Measuring it

Is it faster? Let's time both functions on the same data. `time.monotonic()`
reads a clock that only moves forward, and subtracting two readings gives a
`Duration`. We will meet the `time` module properly in
[Chapter 20](/book/ch20-02-time-and-dates). Add `import time` at the top of
Listing 8-35, add a function that makes test data, and replace `main`:

```aura fragment
import time

def make_samples(count: int64) -> list[float64]:
    mut samples: list[float64] = []
    for i in range(count):
        samples.append(((i * 37) % 201 - 100) as float64 / 100.0)
    return samples

def main():
    samples = make_samples(4099)

    mut plain = 0.0
    started = time.monotonic()
    for round in range(100000):
        plain = energy(samples)
    plain_ms = (time.monotonic() - started).to_ms()

    mut fast = 0.0
    restarted = time.monotonic()
    for round in range(100000):
        fast = energy_lanes(samples)
    lanes_ms = (time.monotonic() - restarted).to_ms()

    print(plain)
    print(fast)
    print(f"plain loop: {plain_ms} ms")
    print(f"lanes:      {lanes_ms} ms")
```

<span class="caption">Listing 8-36: Timing the two loops</span>

Lane values only pay off in native code, so build the program instead of
running it with `aura run`:

```console
$ aura build -o energy src/main.au
aura: built `energy` with direct backend
$ ./energy
1380.4646999999986
1380.4647000000002
plain loop: 281.072563 ms
lanes:      71.337671 ms
```

That run was on a Linux x86-64 cloud machine. Your times will differ, but
the ratio should be similar: the lane loop does the same work in about a
quarter of the time. In the native build each `F64x4` lives in the CPU's
vector registers, the calls to `load`, `*`, and `+` are copied into the loop,
and the range of every `load` is checked once before the loop starts and not
on each step.

Look at the two results too. They differ by about 0.0000000000016, because
the lane loop added the same 4,099 squares in a different order. Neither is wrong. Each is the exact result of its own sequence of
additions, and each will come out the same on every machine. If your program
compares floats for equality or prints all their digits, decide which order
you want and stay with it.

Under `aura run` the picture is reversed. The MIR interpreter computes each
lane as a separate float, so the lane loop gives the same answer and runs
slower than the plain one. That is fine while you are testing for
correctness. Measure with `aura build` or `aura run --backend direct`.

## Writing lanes back

Lanes can also change a list in place. `store` is the opposite of `load`: it
writes the lanes over four elements of a mutable list. Let's turn the volume
up. `normalize` multiplies every sample by a gain and then holds the result
between -1.0 and 1.0, so that a loud passage is clipped at the limit and does
not wrap around:

*Filename: src/main.au*

```aura
from simd import F64x4

def normalize(samples: mut list[float64], gain: float64):
    scale = F64x4.splat(gain)
    low = F64x4.splat(-1.0)
    high = F64x4.splat(1.0)
    for i in range(0, len(samples) - 3, 4):
        (F64x4.load(samples, i) * scale).max(low).min(high).store(samples, i)
    for i in range(len(samples) - len(samples) % 4, len(samples)):
        sample = samples[i]
        samples[i] = max(-1.0, min(1.0, sample * gain))

def main():
    mut samples = [0.1, -0.2, 0.45, 0.6, -0.7, 0.05]
    normalize(samples, 2.0)
    print(samples)
```

<span class="caption">Listing 8-37: Scaling and clamping samples in place</span>

```console
$ aura run src/main.au
[0.2, -0.4, 0.9, 1.0, -1.0, 0.1]
```

The body of the main loop is one expression that reads left to right: load
four samples, multiply them by the gain, raise anything below -1.0 up to it,
lower anything above 1.0 down to it, and store the four results where they
came from. The fourth and fifth samples, 1.2 and -1.4 after scaling, were
clipped. The last two samples were handled by the tail loop, which does the
same arithmetic with the scalar `max` and `min`.

`samples` is a `mut list[float64]` parameter, because `store` writes to it.
Nothing about ownership changes with lanes: a lane value is a copy value
that you loaded from the list, and `store` is an ordinary write through
mutable access.

## Integer lanes

Integer arithmetic in Aura is checked: a sum that does not fit in `int64`
stops the program instead of wrapping around. The checks are the reason a
plain loop over integers is not vectorized. `I64x2` and `I64x4` keep the
checks and do them a whole lane value at a time:

*Filename: src/main.au*

```aura
from simd import I64x4

def total(values: list[int64]) -> int64:
    mut acc = I64x4.splat(0)
    for i in range(0, len(values) - 3, 4):
        acc = acc + I64x4.load(values, i)
    mut sum = acc.sum()
    for i in range(len(values) - len(values) % 4, len(values)):
        sum += values[i]
    return sum

def main():
    print(total([1, 2, 3, 4, 5, 6, 7, 8, 9]))
    print(total([9223372036854775807, 0, 0, 0, 1, 0, 0, 0]))
```

<span class="caption">Listing 8-38: A checked integer sum in four lanes</span>

```console
$ aura run src/main.au
45
error[AU4002]: integer value `9223372036854775808` does not fit in `int64`
 --> <std>/simd.au:570:30
  = note: Aura call chain (innermost first): Add for simd.I64x4.add at 569:5 -> total at 3:1 -> main at 12:1
```

The second call puts the largest `int64` in lane 0 and then adds 1 to that
lane. The trap is the same `AU4002` a scalar `+` would raise, with the value
that did not fit. Its position is inside the `simd` module's own source,
which is where the lane addition is written, and the call chain leads back
to `total` and `main`. Integer sums have no rounding, so when nothing
overflows the lane loop and the plain loop always agree exactly.

Those checks stay run-time checks. `aura verify` doesn't model lane values
today, so it reports the loads and sums of a lane loop as unproved, where the
plain loop with an `invariant` can be proved.

There are no `int32` lanes. Aura computes an `int32` in a 64-bit register
and checks its range afterward, and when the project measured eight `int32`
lanes they ran no faster than the plain loop, so they were left out.

## Choosing the CPU

By default `aura build` produces code for the baseline of your architecture,
so the binary runs on any x86-64 or any arm64 machine. On baseline x86-64 an
`F64x4` is kept in two 128-bit registers. Newer x86-64 CPUs have 256-bit
registers that hold all four lanes at once, and `aura build --target-cpu
x86-64-v3` (or `--target-cpu native`, for whatever the build machine has)
lets the compiler use them:

```console
$ aura build --target-cpu x86-64-v3 -o energy src/main.au
aura: built `energy` with direct backend
```

The results do not change, because every lane operation is defined in terms
of the scalar one. The speed may or may not. The energy program above ran in
the same time with both settings on our machine, while the Manual records
that a checked `I64x4` sum goes from about 1.2 times the speed of the plain
loop to more than twice its speed with the wider registers. The cost is
portability: a binary built for `x86-64-v3` stops with an illegal
instruction on an older CPU.
[Chapter 17](/book/ch17-04-building-for-other-machines) comes back to this
option.

## When to reach for lanes

Lane values are the third tool to try, not the first. If the numbers are in
an `Array`, its methods are already vectorized, and `fast_sum`, `fast_mean`,
and `dot` do reordered float sums for you. If the loop reads and writes list
elements without carrying a float total from one step to the next, write it
plainly and let the compiler vectorize it. Lanes are for what is left: float
reductions such as sums and dot products over a list, and checked `int64`
arithmetic, where you are willing to write the order of operations yourself
in exchange for the speed.

The [SIMD Module](/manual/simd) page of the Manual lists every operation,
including `mul_add`, and `load_array` and `store_array` for one-dimensional
arrays, and it states the exact rounding and trap behavior of each.

## Summary

You now know the collections that most Aura programs are built from. Lists
keep values in order and give you indexing, slicing, sorting, and in-place
changes through `mut`. Strings are owned UTF-8 text that counts characters as
Unicode scalars, slices without ever splitting one, and offers methods for
the splitting, searching, and formatting that text processing needs.
Dictionaries map keys to values in insertion order, sets answer membership
questions, and both make you be explicit about absence with `Lookup` or
`in`. Comprehensions and builtins such as `sum`, `max`, and `sorted` turn
most of the loops you'd write over these into one expression, and `Array[T]`
handles rectangular tables of numbers with whole-array arithmetic,
broadcasting, and masks. Views give a name to an element or a range without
copying it, and lane values do arithmetic on several list elements with one
instruction.

Running through all of it is ownership. A collection owns its elements, so
storing a value moves it in, reading one gives you shared access or a copy,
and the compiler tells you when a string or a list would end up with two
owners. You've also met a lot of traps along the way: indexes past the end,
missing keys, `remove` of a value that isn't there, the `max` of an empty
list. In the [next chapter](/book/ch09-00-error-handling) we'll look at
errors properly: when a trap is the right response, and how to use `Result`
to report a problem to the caller and let it decide what to do.
