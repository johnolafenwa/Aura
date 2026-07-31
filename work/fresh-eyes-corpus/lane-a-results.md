# Fresh-eyes corpus: lane A (programs 01-08)

## Method

These eight programs were authored as day-one/day-two programs a Python
developer might try. Authoring consulted only the maintained root README,
`docs/manual/`, and `tutorials/`. Compiler tests, test fixtures, and existing
example programs were not inspected or searched before or during authoring.

Every final program was run with both forced backends:

```text
target/debug/aura run --backend mir <program>
target/debug/aura run --backend direct <program>
```

The exit status, stdout, and stderr were captured separately. All final runs
returned status 0. MIR and direct stdout were byte-identical for every
program. MIR stderr was empty. Direct stderr contained only the CLI progress
line `aura: building native program...` (33 bytes) on each run; this is
tooling progress rather than program output. Every source file also passed
`target/debug/aura fmt --check <program>`.

## Final results

| No. | Program | Surface | MIR | Direct | Stdout parity | Stdout SHA-256 |
| --- | --- | --- | --- | --- | --- | --- |
| 01 | `01_functions_control_flow.au` | functions, `for`, `if`/`elif`, `break`, `continue` | exit 0 | exit 0 | yes | `e33a675b71c9dc9a7e32ee9f78ca92dff2496b92691cf246e81996c68106a70a` |
| 02 | `02_string_munging.au` | trim, case conversion, replacement, split/join, f-strings | exit 0 | exit 0 | yes | `567c7dda6d8b389a6c60a71cf2ad2dcd58785f924c466943ccf5e1f87bbe9beb` |
| 03 | `03_class_methods.au` | class construction, shared and mutable methods | exit 0 | exit 0 | yes | `e6ec0da7138b4c24313fd80faf4a20ccce82e3cb0aefd564493f65def9885db9` |
| 04 | `04_enum_match_workflow.au` | enum construction, owned match expression | exit 0 | exit 0 | yes | `52aabf5a620c567bcf92a41237c2e6dacbab129c74e6ba2e4d5d58fb41edb4e5` |
| 05 | `05_generic_container.au` | generic class, generic function, inference and specialization | exit 0 | exit 0 | yes | `a86aa4e781cab3ad369d817e5a701ed571cd4580a796b0159338f3e4528e9e22` |
| 06 | `06_trait_impl.au` | trait, implementation, bounded generic dispatch | exit 0 | exit 0 | yes | `783d7645c1cd19214af0bae34b5293de19e622851048891441e0d11e300ddcea` |
| 07 | `07_ownership_copy_and_own.au` | copy value, shared access, explicit ownership transfer | exit 0 | exit 0 | yes | `7b5ca3894b959bf94683a3a6ead3bf5af482e800a7677320c5233d81de7d51b2` |
| 08 | `08_collection_workflow.au` | Vec mutation/sort/slice, Set and Map comprehensions, optional lookup | exit 0 | exit 0 | yes | `650cb1cd9df43890619f5087fd7d1fb4e9a7e2d913de3c9b23e0973987108614` |

## Captured stdout

### 01

```text
16
exact
```

### 02

```text
label=aurora-builds-reliable-tools
chars=28 bytes=28
```

### 03

```text
3x4 area=12
5x6 area=30
```

### 04

```text
queued at 3
running on worker-a
done
```

### 05

```text
generic hello
42
```

### 06

```text
Hello, Ada!
Ada
```

### 07

```text
copy source=7 next=8
shared access
shared access
owned transfer
```

### 08

```text
sorted=[3, 3, 5, 7, 9] middle=[3, 5, 7]
middle_total=15
unique=4 label=reading-7
```

## Fresh-eyes finding: non-copy Map index reads

The first draft of program 08 tried the Python-shaped expression
`labels[7]`, where `labels` was `Map[int32, String]`. Both backends rejected
the program identically with `AU3005`. The behavior was reduced to:

```aurora
def main():
    labels = {7: "reading-7"}
    print(labels[7])
```

Both of these commands returned exit status 1:

```text
target/debug/aura run --backend mir /private/tmp/aurora-fresh-eyes-map-noncopy-index.au
target/debug/aura run --backend direct /private/tmp/aurora-fresh-eyes-map-noncopy-index.au
```

Both emitted the same diagnostic (apart from ordinary source rendering):

```text
error[AU3005]: cannot implicitly copy `String` out of a map index; use `get(key)` for an explicit cloned optional read, or `remove(key)` to transfer ownership
 --> /private/tmp/aurora-fresh-eyes-map-noncopy-index.au:3:11
  |
3 |     print(labels[7])
  |           ^
```

Classification: documented ownership rule and minor Python-to-Aurora friction,
not a compiler bug. A direct Map index may return only a copy value, because
reading a stored `String` that way would otherwise imply either a hidden clone
or a move out of retained storage. The diagnostic is unusually effective: it
names both supported migrations and why the implicit operation is refused.
The final corpus program follows the non-throwing `get` path and exhaustively
matches `Option.Some`/`Option.None`. No compiler production code was changed.

## Verdict for lane A

The beginner-facing core represented here is coherent and has complete forced
backend parity. The only stumble was an intentional ownership boundary in a
Python-familiar indexing spelling, and its diagnostic was sufficient to repair
the program without consulting implementation material. No compiler bug or
undocumented backend difference was found in programs 01-08.
