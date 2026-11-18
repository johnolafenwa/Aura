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

## Dedicated integration runner

The combined Linux x86-64 release at `248ffb7e` produced the following
median wall times in three runs. Each implementation returned the same
checksums as the four-core run above. The Aura scheduler used four workers.

| Workload | Aura (ms) | Rust (ms) | Python (ms) |
| --- | ---: | ---: | ---: |
| `lock` | 74 | 54 | 503 |
| `atomic` | 203 | 148 | 2,522 |
| `dict` | 140 | 51 | 588 |
| `lock_shared` | 823 | 288 | 494 |
| `atomic_shared` | 595 | 430 | 2,511 |
| `cache_shared` | 876 | 348 | 30,755 |

The four-task mutex took 2.86 times the Rust time and 1.67 times the
Python time on this runner. The other five workloads finished sooner
in Aura than in Python. [GitHub Actions measurements](https://github.com/johnolafenwa/Aura/actions/runs/44207713216/job/134487634681)

A second dedicated x86-64 job at `5f111af1` also took three measurements
per workload with four Aura workers. The Rust compiler was 1.95.0 and
Python was 3.12.3. These medians give the three times on that runner.

| Workload | Aura (ms) | Rust (ms) | Python (ms) |
| --- | ---: | ---: | ---: |
| `lock` | 80 | 20 | 860 |
| `atomic` | 257 | 47 | 4,306 |
| `dict` | 199 | 51 | 950 |
| `lock_shared` | 502 | 104 | 838 |
| `atomic_shared` | 388 | 344 | 4,179 |
| `cache_shared` | 261 | 318 | 38,821 |

On this runner Aura finished all six workloads ahead of Python. Against
Rust, the first four took 4.00, 5.47, 3.90 and 4.83 times as long respectively.
[GitHub Actions measurements](https://github.com/johnolafenwa/Aura/actions/runs/44212212238/job/134502108609)
