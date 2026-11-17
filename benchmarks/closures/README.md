# Stored Closure Calls

Run `bash benchmarks/closures/run.sh` from a checkout with a release
build of `aura`, a Rust compiler and Python 3. The programs build
equivalent closures and check the result of each loop. Each row reports
the number of calls, elapsed milliseconds and the checksum. Setup is
outside the timed loop, except for the two cases that create a closure
on every iteration.

## Linux x86-64

The following measurements used native code from the release Aura compiler
at `827a064d`, a separately compiled Rust 1.95 reference and Python 3. The programs ran sequentially
on the same logical CPU on a shared build worker. The table gives the median
of three runs per language, rotating the language order between runs. Each workload
produced the same checksum in all three languages.

| Workload | Calls | Aura (ms) | Rust (ms) | Python (ms) | Checksum |
| --- | ---: | ---: | ---: | ---: | ---: |
| Iterate a list of shared closures | 32,000,000 | 1,377 | 386 | 17,033 | 79,872 |
| Call a closure field | 30,000,000 | 700 | 240 | 19,148 | 107,264 |
| Iterate a list of mutable closures | 16,000,000 | 1,133 | 128 | 13,794 | 475,648 |
| Call a mutable local | 30,000,000 | 1,105 | 242 | 21,540 | 105,000,000 |
| Create and call a shared closure | 10,000,000 | 97 | 645 | 12,615 | 551,104 |
| Pass a fresh closure to a function | 10,000,000 | 73 | 109 | 8,528 | 54,464 |

The original `c20db550` mutable-list loop took 6,765 ms using the same
protocol. The optimized loop invokes the callable through its existing
element loan when the argument computes a bit mask. Its median falls to
1,133 ms. The shared list, mutable list and mutable local remain more
than three times the Rust wall time on this worker.

The Rust programs use `Box<dyn Fn>` and `Box<dyn FnMut>`. The
last two cases include optimizer decisions about inlining and
allocation. Their timings describe the entire loop.

## Direct local calls

`local_functions.au` applies the same captured operation 48,000,000 times
through a local `def` and a typed lambda. Build it with `aura build
--backend direct benchmarks/closures/local_functions.au` and run the
resulting executable. Both loops print checksum 473,089.

On the Linux worker above, five runs of native code from `d1d76285`
gave a median of 448 ms for the local function and 487 ms for the lambda.
The debug-built compiler emitted optimized native code. Both invoke
adapters have the same arithmetic and control flow, including the checked
multiply and add and their cold error paths. Keeping a recursive name in
a runtime slot had previously forced a frame on every successful call,
with respective medians of 1,079 and 459 ms.
