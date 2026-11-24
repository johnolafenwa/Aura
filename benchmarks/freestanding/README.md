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
| `crc32` | 44.8 ms | 141.3 ms | 46.3 ms | 45.7 ms | 0.97× |
| `collatz` | 169.0 ms | 122.4 ms | 115.1 ms | 212.9 ms | 1.47× |
| `isqrt_sum` | 133.9 ms | 116.8 ms | 119.1 ms | 128.9 ms | 1.15× |
| `gcd_sum` | 173.3 ms | 121.1 ms | 121.7 ms | 122.3 ms | 1.43× |
| `orbit` | 881.6 ms | 1009.7 ms | 858.6 ms | 878.9 ms | 1.03× |
| `ring_mix` | 13.9 ms | 17.4 ms | 17.5 ms | 17.0 ms | 0.80× |

| Build | Code and read-only data | Writable data |
| --- | ---: | ---: |
| Aura | 3,412 bytes | 0 bytes |
| C | 1,081 bytes | 0 bytes |
| Rust | 2,189 bytes | 48 bytes |
| Rust with overflow checks | 2,345 bytes | 168 bytes |
