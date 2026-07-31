# Fresh-eyes corpus lane C: programs 17–23

Date: 2026-07-31

## Method

These seven programs were written from the maintained reader documentation,
without consulting compiler tests, fixture directories, or existing example
programs. The principal sources were:

- `tutorials/04-control-flow.md` (`enumerate`, `zip`, and comprehensions)
- `tutorials/07-strings-and-numbers.md` (numeric operations)
- `docs/manual/collections.md` (comprehensions, owned slices, sorting, maps,
  and sets)
- `docs/manual/expressions.md` (comprehensions and exact-width arithmetic)
- `docs/manual/cli-and-tooling.md` (forced backend commands)

Each program was executed with both commands:

```text
target/debug/aura run --backend mir <program>
target/debug/aura run --backend direct <program>
```

The binary reported `aura 0.1.0`; the initial repository revision was
`86ff95a9ffcf`. Each direct run had a cold program-content key, so it wrote the
CLI advisory `aura: building native program...` to stderr. That advisory is
recorded below but is not Aurora-program output. MIR success stderr was empty.

## Results

| Program | Reader workflow | MIR | Direct | Program-output parity |
| --- | --- | --- | --- | --- |
| `17-filtered-list-comprehension.au` | Convert selected temperatures with two filters | exit 0 | exit 0 | yes |
| `18-set-and-map-comprehensions.au` | Deduplicate residues and index odd-square results | exit 0 | exit 0 | yes |
| `19-nested-comprehension.au` | Build outer-major inventory coordinates | exit 0 | exit 0 | yes |
| `20-owned-slices.au` | Slice a Vec and Unicode String with omitted and negative bounds | exit 0 | exit 0 | yes |
| `21-enumerate-and-zip-processing.au` | Label names and total paired work hours | exit 0 | exit 0 | yes |
| `22-sort-and-deduplicate.au` | Sort a retained copy and deduplicate with a Set | exit 0 | exit 0 | yes |
| `23-fixed-width-arithmetic.au` | Wrap an overflowing `int16` addition | exit 1, internal panic | exit 0 | **no: compiler bug** |

Programs 17–22 produced identical stdout on both backends:

```text
# 17
[69, 75, 71]
[18, 21, 24, 17, 30, 22]

# 18
2
true
false
true
9
25
121
false

# 19
[110, 120, 310, 320]
4

# 20
[20, 30, 40]
[40, 50]
[10, 20, 30, 40, 50]
é🎉B
Aé
BΩ
5

# 21
0: Ada
1: Grace
2: Linus
Ada worked 6 hours
Grace worked 8 hours
14
3

# 22
[8, 17, 17, 31, 42, 42]
4
true
false
[42, 17, 42, 8, 17, 31]
```

For each of these six programs, direct stderr contained only the native-program
build advisory quoted above. There was no language diagnostic.

## Finding C-1: MIR panics on documented narrow-integer wrapping arithmetic

Program 23 is the minimized source-level reproducer:

```aurora
def main():
    high: int16 = 30000
    print(high.wrapping_add(10000))
```

The spelling is documented as valid for every scalar integer type in
`docs/manual/expressions.md`: `wrapping_add`, `wrapping_sub`, `wrapping_mul`,
`saturating_add`, `saturating_sub`, and `saturating_mul` are exact-width
methods. This is therefore an implementation/backend defect, not a documented
language limitation or reader error.

Forced MIR exits 1, produces no stdout, and reports:

```text
thread '<unnamed>' (...) panicked at crates/aurora-compiler/src/mir_runtime.rs:3857:46:
semantic analysis gives fixed-width integer methods matching operands
note: run with `RUST_BACKTRACE=1` environment variable to display a backtrace
error[AU4001]: Aurora MIR runtime panicked while executing the program
 --> work/fresh-eyes-corpus/23-fixed-width-arithmetic.au
```

Forced direct exits 0 and prints the correct wrapped `int16` result:

```text
-25536
```

Its stderr contained only concurrent-build/rebuild CLI advisories. The
temporary migration path is to force the direct backend for this operation;
ordinary `+` is not an equivalent workaround because its documented behavior
is checked overflow. The compiler should be repaired test-first, after which
program 23 must be rerun on both backends and required to exit 0 with identical
stdout and no language diagnostic.

## Classification

- Compiler/backend bugs: 1 (C-1).
- Documented limitations reached: 0.
- Documentation friction that prevented a program: 0.
- Compiler production files changed by this lane: 0.

## Repair verification

The follow-up compiler repair was completed test-first on 2026-07-31. The
existing `fixed_width_integer_methods_match_forced_mir_and_direct_backends`
CLI regression was expanded with all six documented `int16` wrapping and
saturating methods at their upper/lower boundaries, using contextual literal
right operands. It reproduced C-1 before the implementation changed.

The MIR runtime now applies the statically checked receiver integer kind to
both operands, matching direct emission. This is necessary because a
contextual integer literal may be materialized in MIR with its default
`int64` runtime tag even though call checking has established an `int16`
method contract. The old invariant `expect` was replaced by a controlled
`AU4001` mismatch result, so this dispatch no longer panics if the invariant is
violated.

After the repair, program 23 has full backend parity:

| Backend | Exit | Stdout | Language stderr |
| --- | --- | --- | --- |
| forced MIR | 0 | `-25536` | empty |
| forced direct | 0 | `-25536` | empty |

The direct CLI also printed its ordinary native-program build advisory.
The expanded forced-MIR/direct regression, formatting check, and Clippy with
warnings denied all passed.
