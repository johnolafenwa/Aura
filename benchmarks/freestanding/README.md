# Freestanding routines

Six routines over machine words, written in Aura, C, and Rust without
its standard library. Each is built as an object with no runtime and
linked with the same C harness, which times it and prints its result.

```sh
benchmarks/freestanding/run.sh target/release/aura
```

The script needs `clang`, `rustc`, and `size`.

| Build | Command |
| --- | --- |
| `aura` | `aura build --freestanding` |
| `c` | `clang -O2 -ffreestanding -fno-builtin` |
| `rs` | `rustc -C opt-level=3 -C panic=abort` |
| `rsc` | the same with `-C overflow-checks=on` |

Aura keeps its overflow and divisor checks, so `rsc` is the closest match.

## Results

On x86-64 Linux, each executable ran on the same dedicated CPU core. The table gives the best elapsed time from nine runs per routine with the same C harness. The ratio compares Aura with the faster of C and Rust with overflow checks off. All four builds returned the same results.

| Routine | Aura | C | Rust | Rust with overflow checks | Aura / fastest C or Rust |
| --- | ---: | ---: | ---: | ---: | ---: |
| `crc32` | 45.6 ms | 144.8 ms | 45.2 ms | 46.1 ms | 1.01× |
| `collatz` | 158.9 ms | 122.3 ms | 121.4 ms | 228.1 ms | 1.31× |
| `isqrt_sum` | 120.0 ms | 119.8 ms | 139.9 ms | 129.0 ms | 1.00× |
| `gcd_sum` | 171.7 ms | 120.2 ms | 121.7 ms | 136.4 ms | 1.43× |
| `orbit` | 880.5 ms | 1007.6 ms | 883.6 ms | 884.7 ms | 1.00× |
| `ring_mix` | 19.5 ms | 17.9 ms | 19.7 ms | 16.5 ms | 1.09× |

| Build | Code and read-only data | Writable data |
| --- | ---: | ---: |
| Aura | 3,412 bytes | 0 bytes |
| C | 1,081 bytes | 0 bytes |
| Rust | 2,189 bytes | 48 bytes |
| Rust with overflow checks | 2,345 bytes | 168 bytes |
