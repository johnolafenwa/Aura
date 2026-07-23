# Organizing Code

A single-file program is a fine way to start. At some point, though, helper types want a home, public APIs want to be marked as such, and dependencies want to be named somewhere the compiler can read them. That is what Aurora's module and package system is for.

This chapter walks from a single file to a package with dependencies.

## Local Modules

Say the program has some math helpers. Move them into their own file:

```
helpers/math.au
```

```python
public def double(value: int32) -> int32:
    return value * 2

def internal(value: int32) -> int32:
    return value + 1
```

From another file, import the module and call its public names:

```python
import helpers.math

print(helpers.math.double(21))
```

Only declarations marked `public` are visible outside the file. `internal(...)` may be called from within `helpers/math.au`, but importers cannot reach it. This is not a convention; the compiler enforces it.

## Two Styles Of Import

`import helpers.math` brings the whole module namespace in, so calls read `helpers.math.double(21)`. When a single name is the local concept, use `from ... import ...` to pull the name directly:

```python
from helpers.math import double

print(double(21))
```

Both styles are useful. A quick rule: when a file imports many names from a module, keep the module prefix; when the imported name is the central concept of the file, drop it.

Both styles also preserve the full callable contract. If a public generic
helper performs a clone-producing operation, its inferred clone-safety
obligation follows the import and is checked where the helper is specialized.

## Packages

A **package** is a directory with an `Aurora.toml` manifest and usually a `src/` directory:

```
app/
├── Aurora.toml
└── src/
    └── main.au
```

```toml
[package]
name = "app"
version = "0.1.0"
edition = "2026"
```

Manifest package names must be valid Aurora identifiers — letters, digits, and underscores. Hyphenated names are rejected because `import my-util.math` would not parse as an Aurora expression.

Commands that take a source file inside a package infer the nearest package root automatically. `aura run src/main.au` from inside `app/` works the same as running it from the repo root.

## Dependencies

Dependencies live under `[dependencies]` in the manifest:

```toml
[dependencies]
util = { path = "../util" }
jsonx = { git = "https://github.com/example/jsonx.git", branch = "main" }
```

- **Path dependencies** point at another local package. Good for related crates in the same repository or workspace.
- **Git dependencies** point at a git repository. Optional `rev`, `tag`, or `branch` selectors pin the version; without one, the dependency defaults to `branch = "main"`.

Both shapes are pinned by exact revision (or canonical path) in `Aurora.lock`. Repeat runs resolve the same code until you ask for an update:

```bash
aura deps update
aura deps update util
```

## Package Names Are Import Roots

A dependency is imported by its package name:

```python
import util.math

print(util.math.double(10))
```

Because the import syntax uses the package name directly, a manifest that declares `name = "my-app"` is rejected: `import my-app.foo` would try to subtract `app.foo` from `my`.

## Workspaces

When several packages live together, a **workspace** manifest coordinates them:

```toml
[workspace]
members = ["app", "util"]
```

Each member is still an ordinary package with its own `Aurora.toml`. The workspace root owns the shared `Aurora.lock`.

## A Good Module Boundary

A module boundary should usually hide representation and expose behaviour:

```python
public class Counter:
    value: int32 = 0

    public def inc(borrow mut self):
        self.value += 1

    public def get(borrow self) -> int32:
        return self.value
```

Callers see `Counter.inc()` and `Counter.get()`; they never reach `.value` directly. The internal representation is free to change in ways the external contract does not.

Keep helper functions private unless another module genuinely needs them. A smaller public surface is easier to keep stable.

## Notes On Editor Tooling

`aura analyze` and `aura complete` can read an editor buffer through `--stdin` while resolving imports relative to the file being edited. These stdin-mode commands deliberately do not write `Aurora.lock`. Lockfile changes happen only when you `check`, `run`, `build`, or explicitly `deps update`.

Reference: [Packages](/manual/packages), [CLI And Tooling](/manual/cli-and-tooling).
