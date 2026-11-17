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
