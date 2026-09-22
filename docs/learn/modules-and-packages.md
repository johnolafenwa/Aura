# Organizing Code

This page takes a program from a single file to a package with dependencies.
A single file is a fine place to start. As a program grows, helper types need
a home, public APIs need marking, and dependencies need a place where the
compiler can read them. Aura's modules and packages cover all three.

## Local Modules

Say the program has some math helpers. Move them into their own file:

```
helpers/math.au
```

```aura
public def double(value: int32) -> int32:
    return value * 2

def internal(value: int32) -> int32:
    return value + 1
```

From another file, import the module and call its public names:

```aura
import helpers.math

print(helpers.math.double(21))
```

Only declarations marked `public` are visible outside the file. Code inside
`helpers/math.au` may call `internal(...)`, but importers cannot reach it.
The compiler enforces this. It is not a convention.

## Two Styles Of Import

`import helpers.math` brings in the whole module namespace, so calls read
`helpers.math.double(21)`. Use `from ... import ...` to bring in a single
name directly:

```aura
from helpers.math import double

print(double(21))
```

A quick rule for choosing:

- When a file imports many names from a module, keep the module prefix.
- When the imported name is the central concept of the file, drop the prefix.

## Choosing Local Import Names

Use `as` when the full module path is too long to repeat, or when two modules
export the same short name:

```aura
import helpers.math as integer_math
from helpers.counter import Counter as ReadableCounter

print(integer_math.double(21))
counter = ReadableCounter(value=2)
```

The alias is the only local name that the import entry introduces. It
changes how the importer spells the name. The declaration keeps its original
module identity, type, visibility, and behavior.

A from-import may mix direct and aliased entries:

```aura
from helpers.math import double as twice, empty
```

Both import styles keep the full callable contract. Suppose a public generic
helper performs an operation that produces a clone. Its inferred clone-safety
requirement follows the import, and the compiler checks it where the helper
is specialized.

## Packages

A package is a directory with an `Aura.toml` manifest, and usually a `src/`
directory:

```
app/
├── Aura.toml
└── src/
    └── main.au
```

```toml
[package]
name = "app"
version = "0.1.0"
edition = "2026"
```

A package name must be a valid Aura identifier: letters, digits, and
underscores. See [Package Names Are Import Roots](#package-names-are-import-roots)
for why hyphens are rejected.

A command that takes a source file inside a package finds the nearest package
root automatically. Running `aura run src/main.au` from inside `app/` works
the same as running it from the repo root.

## Dependencies

Dependencies go under `[dependencies]` in the manifest:

```toml
[dependencies]
util = { path = "../util" }
jsonx = { git = "https://github.com/example/jsonx.git", branch = "main" }
```

- **Path dependencies** point at another local package. They suit related
  packages in the same repository or workspace.
- **Git dependencies** point at a git repository. An optional `rev`, `tag`,
  or `branch` selector pins the version. Without one, the dependency defaults
  to `branch = "main"`.

`Aura.lock` pins both kinds by exact revision or canonical path. Repeat runs
resolve the same code until you ask for an update:

```bash
aura deps update
aura deps update util
```

## Package Names Are Import Roots

You import a dependency by its package name:

```aura
import util.math

print(util.math.double(10))
```

Because the import uses the package name directly, a manifest that declares
`name = "my-app"` is rejected. `import my-app.foo` would try to subtract
`app.foo` from `my`.

## Workspaces

When several packages live together, a workspace manifest coordinates them:

```toml
[workspace]
members = ["app", "util"]
```

Each member is still an ordinary package with its own `Aura.toml`. The
workspace root owns the shared `Aura.lock`.

## A Good Module Boundary

A module boundary should usually hide the representation and expose behavior:

```aura
public class Counter:
    value: int32 = 0

    public def inc(mut self):
        self.value += 1

    public def get(self) -> int32:
        return self.value
```

Callers see `Counter.inc()` and `Counter.get()`. They never reach `.value`
directly, so the internal representation can change without breaking the
external contract.

Keep helper functions private unless another module needs them. A smaller
public surface is easier to keep stable.

## Notes On Editor Tooling

`aura analyze` and `aura complete` can read an editor buffer through
`--stdin` and resolve imports relative to the file being edited. In stdin
mode these commands do not write `Aura.lock`. The lockfile changes only when
you run `check`, `run`, `build`, or `deps update`.

Reference: [Packages](/manual/packages), [CLI And Tooling](/manual/cli-and-tooling).
