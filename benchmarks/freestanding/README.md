# Freestanding routines

Five routines over machine words, written in Aura, C, and Rust without
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

x86-64 Linux, best of nine runs:

| Routine | Aura | C | Rust | Rust with overflow checks |
| --- | --- | --- | --- | --- |
| `crc32` | 50 ms | 195 ms | 50 ms | 54 ms |
| `collatz` | 231 ms | 157 ms | 139 ms | 302 ms |
| `isqrt_sum` | 98 ms | 105 ms | 145 ms | 164 ms |
| `gcd_sum` | 201 ms | 131 ms | 156 ms | 175 ms |
| `orbit` | 832 ms | 882 ms | 778 ms | 756 ms |

| Build | Code and read-only data |
| --- | --- |
| Aura | 3,218 bytes |
| C | 914 bytes |
| Rust | 2,020 bytes |
| Rust with overflow checks | 2,176 bytes |

LLVM 22 turns the `crc32` bit loop into a lookup in a 2,048-byte table,
which Aura and Rust carry and clang 18 does not. Without the table, the
five Aura functions are 993 bytes, and the object also holds 115 bytes of
memory routines.
