# Rust baselines

This is a standalone Cargo project, excluded from the Aura workspace. It is
pinned to Rust 1.95.0 and tokio `=1.53.1`, with its own committed lockfile.
Release settings are `opt-level=3`, `lto="fat"`, and `codegen-units=1`.

| Program | Equivalent work and output |
| --- | --- |
| `fib30` | Naive recursive fib(30), checksum 832040, same READY/GO/DONE records. |
| `tasks_10000` | Create and join 10,000 tokio tasks, sum indexes to 49995000, same protocol. |
| `tcp_fanout` | 20 pre-bound ephemeral loopback listeners, 20 clients, ping/pong and 100 ms handlers; checksum 80. No single-listener variant. |
| `retrying_worker` | Same local HTTP fixture and path/status schedule as existing Aura/CPython harness sources: 16 cycles, statuses 503/200/503/429/503/503/503, delays 4/6/3/5 ms per cycle, 112 requests, checksum 18112. |
| `int32_loop`, `int64_loop` | Ten million checked increments in the declared width; overflow fails, never wraps. Existing Aura whole-process output is `10000000` with a newline, with no handshake. |
| `startup` | Empty whole-process control. |
| `float64_add` | One million elements; fresh `Vec<f64>` each of 512 additions, checksum 2048.0; same warmup and protocol. |
| `float64_sum` | One million elements, 1024 sequential left-to-right sums, checksum 4096000000.0; same warmup and protocol. |

## Equivalence rules

- Concurrent programs use tokio's multi-thread runtime, with workers equal to
  host parallelism. They own and join their server and task work before
  `DONE`. The runtime is initialized before `READY`.
- The Rust local HTTP server keeps the existing fixture's wire contract. The
  runner does not substitute a remote service.
- `black_box` prevents constant folding and elimination of fib inputs, checked
  loops, and Array kernels.
- Integer loops expose each counter update to the optimizer barrier. This
  overhead is part of the disclosed Rust reference.
- Array addition keeps full fresh allocation. Reduction does not use parallel
  or reassociated summation.

## Recorded results

The [7 September measurement record](../../docs/manual/performance.md)
contains contractual control-plane and Array ratios, and diagnostic standalone
integer ratios. No Rust workload was excluded. The Aura/Rust protocol ratios of
medians are:

| Workload | Aura / Rust |
| --- | ---: |
| fib | 29.664884 |
| tasks | 29.068501 |
| TCP | 0.996856 |
| retry | 0.907338 |

Release sources use the same approved two-cast correction at both compiler
bases. The measurement record also covers the benchmark sources and the
optimizer-barrier qualifications. The session manifest is
`work/2026-09-07-foundations-measurements/SHA256SUMS`. Its SHA-256 is
`911dc4a7901357b33679ad260923c56d0c8216440b200a0eb12e426ea60cac67`.

## Build and protocol check

Build the references and run the protocol-only check from the repository root:

```bash
cargo +1.95.0 build --manifest-path benchmarks/rust_baselines/Cargo.toml --release --locked --bins
python3 scripts/rust_baselines.py
```

The smoke tool verifies exact output and checksums once per binary and prints
only PASS records. Hosted `npm run ci` includes the build and the smoke
contract without recording durations.

The three timing runners build references in temporary separate targets, keep
provenance, then remove the targets. They clear profile environment overrides
for the references, so an Aura profile experiment cannot change the Rust
baseline unnoticed.
