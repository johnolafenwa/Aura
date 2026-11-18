# Shared state benchmarks

Run the native Aura program, its Rust equivalent, and its Python equivalent from the repository root.

```sh
cargo build --release --locked -p aura
bash benchmarks/shared_state/run.sh
```

The script builds both native executables, runs Aura with four scheduler workers, and checks that all three programs produce the same results. Set `AURA` to select an installed compiler. Each program measures elapsed wall time inside the process, excluding startup and compilation.

| Workload | Work performed | Expected result |
| --- | --- | --- |
| `lock` | One task acquires an integer mutex, increments the value, and releases it four million times. | 4,000,000 |
| `atomic` | One task makes twenty million sequentially consistent increments. Python uses a lock around each increment. | 20,000,000 |
| `dict` | One task writes four million entries under a mutex, cycling through 1,024 keys. | 1,024 entries with values totaling 4,095,475,200 |
| `lock_shared` | Four tasks share the four million increments of one mutex-protected counter. | 4,000,000 |
| `atomic_shared` | Four tasks share the twenty million increments of one atomic counter. | 20,000,000 |
| `cache_shared` | Four tasks each run one million repeatable lookups. On a miss, the task stores three times the key. | 1,024 entries and a sum of 6,135,895,677 across the four tasks |

The cache lookups and stores take separate locks in all three programs. Two simultaneous misses for a key therefore write the same value.

## Linux x86-64 results

The table gives median wall time in milliseconds over three runs on an
Intel Xeon Platinum 8573C. The native Aura compiler and runtime came
from the `4f5b3dff` Linux release build. Rust used `rustc 1.95.0` and
Python used CPython 3.12.14. All programs used the same four CPUs.
The Aura scheduler used four workers. Build and test jobs on the worker
were paused for each measured run. The order rotated between rounds.
All checksums matched.

| Workload | Aura (ms) | Rust (ms) | Python (ms) | Aura time / Rust time | Python time / Aura time |
| --- | ---: | ---: | ---: | ---: | ---: |
| `lock` | 146 | 82 | 829 | 1.78× | 5.68× |
| `atomic` | 380 | 252 | 4,484 | 1.51× | 11.80× |
| `dict` | 233 | 90 | 1,007 | 2.59× | 4.32× |
| `lock_shared` | 293 | 266 | 822 | 1.10× | 2.81× |
| `atomic_shared` | 540 | 318 | 4,160 | 1.70× | 7.70× |
| `cache_shared` | 352 | 289 | 25,828 | 1.22× | 73.38× |

The uncontended integer lock took 36.5 ns per increment, including
acquire and release. The four-task cache took 1.22 times the Rust
time for the same four million lookups.

## Dedicated integration runners

Each column gives median milliseconds in Aura / Rust / Python over
three runs on its own Linux x86-64 runner. Aura used four scheduler
workers. All programs produced matching checksums. The third release
backs off while borrowing a contended native mutex handle.

| Workload | `248ffb7e` | `5f111af1` | `1033d521` |
| --- | ---: | ---: | ---: |
| `lock` | 74 / 54 / 503 | 80 / 20 / 860 | 82 / 22 / 897 |
| `atomic` | 203 / 148 / 2,522 | 257 / 47 / 4,306 | 238 / 53 / 4,447 |
| `dict` | 140 / 51 / 588 | 199 / 51 / 950 | 201 / 69 / 957 |
| `lock_shared` | 823 / 288 / 494 | 502 / 104 / 838 | 158 / 97 / 822 |
| `atomic_shared` | 595 / 430 / 2,511 | 388 / 344 / 4,179 | 324 / 311 / 4,107 |
| `cache_shared` | 876 / 348 / 30,755 | 261 / 318 / 38,821 | 267 / 360 / 37,684 |

The four-task mutex finished behind Python on the first runner. On the
third runner, it completed four million increments in 158 ms, 1.63 times
the Rust time and 5.20 times faster than CPython. The uncontended lock
took 20.5 ns per update. The uncontended lock and atomic counter took
3.73 and 4.49 times the Rust time on that runner. The guarded dictionary
and the three four-task workloads each took less than three times the
Rust time. [First run](https://github.com/johnolafenwa/Aura/actions/runs/44207713216/job/134487634681) · [Second run](https://github.com/johnolafenwa/Aura/actions/runs/44212212238/job/134502108609) · [Third run](https://github.com/johnolafenwa/Aura/actions/runs/44219534330/job/134523303042)

Local results varied with other builds on the machine. In ten
alternating runs on four CPUs, the median four-task mutex wall time
went from 685 to 698 ms and process CPU from 264.5 to 327 ms when
comparing the prior scoped Linux release with the local backoff
runtime. The cache wall time went from 994.5 to 959 ms and process CPU
from 439 to 363.5 ms. These local runs used the same native frontend.

A second comparison rebuilt the code immediately before and after the
backoff with the same local release toolchain. In 15 alternating runs
on four CPUs, four-task mutex wall time fell from 595 to 488 ms and
process CPU from 286 to 231 ms. Four-task cache wall time fell from
917 to 845 ms and process CPU from 386 to 350 ms. The uncontended
integer lock used 184 ms of process CPU in both builds. All checksums
matched.

## Checked atomic addition

The native atomic addition keeps sequentially consistent ordering. The
runtime inlines the checked addition and constructs its diagnostic on
the error path. A 20-million-increment comparison on one logical CPU
used the same compiler and release settings for both runtimes. Eleven
rotated runs measured 512 ms of process CPU before the change and
443 ms afterward for the public `AtomicInt.add` loop. The direct host
entry took 416 and 359 ms respectively.

Callgrind counted 124.0 instructions per public increment before the
change and 106.0 after it, using the difference between 10,000 and 20,000
iterations of the same executable. In a separate five-round comparison,
the prior public loop took 486 ms of process CPU, the new loop took
418 ms, Rust took 228 ms and Python took 19,113 ms. All four produced
20,000,000. These CPU measurements exclude time spent waiting to run.

## Combined release at `92535bb9`

The dedicated Linux runner used the same inputs for Aura, Rust and Python.
The first three timing columns are medians of three runs. A separate
comparison alternated two Aura releases five times on the same runner.
Every program produced its expected checksum. Times are in milliseconds.
[Benchmark job](https://github.com/johnolafenwa/Aura/actions/runs/44225018396/job/134547497863)

| Workload | Aura | Rust | Python | Paired Aura `5f111af1` | Paired Aura `92535bb9` |
| --- | ---: | ---: | ---: | ---: | ---: |
| `lock` | 78 | 58 | 565 | 80 | 78 |
| `atomic` | 194 | 160 | 2,829 | 219 | 193 |
| `dict` | 149 | 55 | 655 | 150 | 149 |
| `lock_shared` | 411 | 307 | 546 | 780 | 343 |
| `atomic_shared` | 579 | 419 | 2,719 | 604 | 472 |
| `cache_shared` | 436 | 504 | 11,473 | 956 | 434 |

The uncontended lock took 19.5 ns per update. The new four-task mutex
finished sooner in all five pairs, with a median paired reduction of 52.6%.
