# Stored Closure Calls

Run `bash benchmarks/closures/run.sh` from a checkout with a release
build of `aura`, a Rust compiler and Python 3. The programs build
equivalent closures and check the result of each loop. Each row reports
the number of calls, elapsed milliseconds and the checksum. Setup is
outside the timed loop, except for the two cases that create a closure
on every iteration.

## Linux x86-64

The following measurements used the stored-call branch at `c20db550`,
Rust 1.95 and a release Aura compiler. The programs ran sequentially
on the same logical CPU on a shared build worker. Each workload
produced the same checksum in all three languages.

| Workload | Calls | Aura (ms) | Rust (ms) | Python (ms) | Checksum |
| --- | ---: | ---: | ---: | ---: | ---: |
| Iterate a list of shared closures | 32,000,000 | 1,779 | 103 | 6,918 | 79,872 |
| Call a closure field | 30,000,000 | 762 | 115 | 7,451 | 107,264 |
| Iterate a list of mutable closures | 16,000,000 | 7,336 | 33 | 5,326 | 475,648 |
| Call a mutable local | 30,000,000 | 871 | 88 | 8,414 | 105,000,000 |
| Create and call a shared closure | 10,000,000 | 28 | 300 | 5,835 | 551,104 |
| Pass a fresh closure to a function | 10,000,000 | 33 | 36 | 5,593 | 54,464 |

The Rust programs use `Box<dyn Fn>` and `Box<dyn FnMut>`. The
last two cases include optimizer decisions about inlining and
allocation. Their timings describe the entire loop.
