# Compile-Time Evaluation And Conditional Compilation

Use `const` on a module binding when its value must be ready at build time. Use an `if` over that value or the builtin `build` module to select code for the target.

```aura
import build

def square(value: int64) -> int64:
    return value * value

const block_size = square(8)

if build.freestanding:
    export "C" def block_bytes() -> int64:
        return block_size
else:
    def main():
        print(block_size)
```

## Module Bindings

The compiler evaluates pure module initializers, including ordinary bindings without the marker. An initializer can read earlier bindings and call pure Aura functions. It can construct a scalar, string, byte string, tuple, list, dictionary, set, class or enum. Iteration order and type information survive evaluation. `public const` makes a required binding available to importers.

A pure computation can update its own local variables and collections. Reading the environment, performing I/O, consulting the clock, starting a task or calling foreign code requires runtime execution. An ordinary binding with such an initializer runs during module initialization. Writing `const` there produces `AU3025`. Move the effect into a runtime function, or use an ordinary binding if the effect belongs at startup.

A local value can run a pure `__drop__` hook within the initializer’s budget. A result that still owns a drop hook needs runtime initialization. This includes a hook owned by an item in a returned collection.

Integer operations obey Aura's target widths. Floating-point values round to their declared precision. A trap in a pure initializer is a compile error, even on an ordinary binding. The diagnostic names the binding and includes its Aura call frames.

| Limit per initializer | Diagnostic | What to change |
| --- | --- | --- |
| 100,000 MIR steps including nested calls | `AU3027` | Reduce iteration or recursion. |
| 8 MiB of cumulative logical storage | `AU3028` | Reduce temporary data and allocation. |

Frozen bindings have no startup call. Native code uses scalar immediates or read-only data. A hosted program attaches a shared runtime box to an aggregate on first use. Freestanding objects can read a computed table without linking the Aura runtime.

## Target Properties

`import build` gives the program the following constants. Cross-compilation uses the destination machine's properties.

| Member | Type | Values |
| --- | --- | --- |
| `os` | `str` | `"linux"`, `"macos"`, `"windows"`, or `"none"` on a freestanding target |
| `arch` | `str` | Architecture name, such as `"x86_64"`, `"aarch64"` or `"riscv64"` |
| `pointer_width` | `int64` | Pointer size in bits |
| `endianness` | `str` | `"little"` or `"big"` |
| `freestanding`, `hosted` | `bool` | The selected execution environment |
| `debug`, `release` | `bool` | The selected build profile |
| `cfg` | `dict[str, str]` | Features from the entry manifest and command line |

Use membership when a feature might be absent. A bare `--cfg tracing` sets its value to the string `"true"`. An empty value is legal. The first equals sign separates the name from its value.

```aura
import build

def main():
    if "tracing" in build.cfg:
        print(build.cfg["tracing"])
    else:
        print("tracing off")
```

```sh
aura run --cfg tracing=verbose main.au
```

## Conditional Suites

An ordinary `if` can select code when its condition uses only build properties, marked constants, literals and operators on them. The compiler parses all arms. It resolves imports, checks types and lowers code only in the selected arm. The other arm may reference a module, foreign symbol or function unavailable on this target. A spelling or indentation error that prevents parsing still needs a fix.

At module scope, conditional suites can enclose imports, functions, classes, implementations and foreign declarations. A class can select fields or methods. In a function, a condition that reads a parameter or local value remains a runtime branch with the usual checks on both suites.

`aura check --all-targets` repeats checking for the supported targets with the selected cfg values and profile. It names the target in a diagnostic. Formatting preserves every arm. Documentation and verification use the current configuration. The language server dims inactive source and can navigate to identifiable declarations inside it.

[Build flags and package defaults](/manual/packages#build-configuration) · [Freestanding builds](/manual/freestanding)
