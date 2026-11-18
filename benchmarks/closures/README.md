# Stored Closure Calls

Run `bash benchmarks/closures/run.sh` from a checkout with a release
build of `aura`, a Rust compiler and Python 3. The programs build
equivalent closures and check the result of each loop. Each row reports
the number of calls, elapsed milliseconds and the checksum. Setup is
outside the timed loop, except for the two cases that create a closure
on every iteration.

## Linux x86-64

Native code from the Aura release compiler at `aa9f498c`, Rust 1.95.0 and Python 3.12.14 ran on an Intel Xeon Platinum 8573C worker. The table gives the median of three runs per language pinned to the same logical CPU. Each language ran separately, rotating the order across runs. Every workload produced the same checksum.

| Workload | Calls | Aura (ms) | Rust (ms) | Python (ms) | Aura / Rust | Checksum |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| Iterate a list of shared closures | 32,000,000 | 499 | 219 | 11,849 | 2.28× | 79,872 |
| Call a closure field | 30,000,000 | 466 | 170 | 14,022 | 2.74× | 107,264 |
| Iterate a list of mutable closures | 16,000,000 | 250 | 114 | 10,559 | 2.19× | 475,648 |
| Call a mutable local | 30,000,000 | 299 | 269 | 16,064 | 1.11× | 105,000,000 |
| Create and call a shared closure | 10,000,000 | 85 | 564 | 9,628 | 0.15× | 551,104 |
| Pass a fresh closure to a function | 10,000,000 | 57 | 86 | 8,011 | 0.66× | 54,464 |

The original stored-call branch at `c20db550` took 6,765 ms for the mutable-list workload using the same protocol. A list now lends its callable environment to the call, and the compiler can forward a mutable receiver to a leaf method with frame records deferred until an overflow. The benchmark checks the final state across all 16 million calls.

On this shared worker, scheduling affected individual wall times. A separate run timed each full workload in its own process, alternating Aura and Rust and rotating the workloads. The CPU columns give the median user plus system time across seven runs, including process startup. Callgrind counted the instruction difference between 16,000 and 32,000 calls, divided by 16,000. The shorter workloads also matched the Rust checksums.

| Workload | Aura CPU (ms) | Rust CPU (ms) | Aura / Rust CPU | Aura instructions/call | Rust instructions/call |
| --- | ---: | ---: | ---: | ---: | ---: |
| Iterate a list of shared closures | 173.3 | 79.0 | 2.19× | 40.04 | 11.11 |
| Call a closure field | 170.4 | 69.2 | 2.46× | 40.94 | 11.00 |
| Iterate a list of mutable closures | 109.6 | 43.9 | 2.49× | 51.54 | 14.44 |
| Call a mutable local | 126.6 | 74.4 | 1.70× | 34.27 | 12.00 |
| Create and call a shared closure | 30.8 | 191.4 | 0.16× | 24.88 | 139.00 |
| Pass a fresh closure to a function | 33.6 | 38.1 | 0.88× | 23.78 | 12.00 |

The Rust programs use `Box<dyn Fn>` and `Box<dyn FnMut>`. The
last two cases include optimizer decisions about inlining and
allocation. Their timings describe the entire loop.
