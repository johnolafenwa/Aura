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
