# Packages And Workspaces

This chapter covers Aura's package system. A package is a directory with an
`Aura.toml` manifest and a `src/` directory. Packages can depend on local
paths and git repositories, and a workspace groups several packages under one
root.

## Single Package

A package keeps its manifest at the root and its source files under `src/`:

```text
my-app/
  Aura.toml
  src/main.au
  src/helpers/math.au
```

The manifest declares the package identity:

```toml
[package]
name = "app"
version = "0.1.0"
edition = "2026"
```

Run the package by pointing `aura` at a file under `src/`:

```bash
cargo run -p aura -- run my-app/src/main.au
```

The compiler treats the directory that holds `Aura.toml` as the package root
and `src/` as the source root. Local imports resolve relative to `src/`:

```aura fragment
import helpers.math    # resolves to src/helpers/math.au
```

An alias changes the local spelling. It does not change how the path
resolves:

```aura fragment
import helpers.math as integer_math
from helpers.math import double as twice
```

## Local Path Dependencies

Declare a dependency with a path relative to the manifest directory:

```toml
[dependencies]
util = { path = "../util" }
```

Then import it through the package name:

```aura fragment
import util.math
```

The dependency must have its own `Aura.toml` with a matching `name`.
Transitive dependencies resolve through the package graph.

See [examples/packages/local_path_dependencies/app/src/main.au](../examples/packages/local_path_dependencies/app/src/main.au).

## Git Dependencies

A dependency can also come from a git repository:

```toml
[dependencies]
util = { git = "https://github.com/example/util.git" }
jsonx = { git = "https://github.com/example/jsonx.git", tag = "v0.3.1" }
release_math = { git = "https://github.com/example/math.git", branch = "release" }
frozen_math = { git = "https://github.com/example/math.git", rev = "4f2c9d8b7e..." }
```

Git dependencies accept three selectors:

| Selector | Meaning |
| --- | --- |
| `branch = "name"` | Track a branch. With no selector, the default is `"main"`. |
| `tag = "v1.0.0"` | Pin to a specific tag. |
| `rev = "abc123..."` | Pin to an exact commit. |

You import a git dependency by package name, as with a path dependency:

```aura fragment
import util.math
import jsonx.parser
```

You can alias the full dependency path after it resolves:

```aura fragment
import util.math as util_math
from jsonx.parser import parse as parse_json
```

## Workspaces

A workspace root groups related packages under one top-level manifest:

```toml
[workspace]
members = ["app", "util"]
```

```text
my-workspace/
  Aura.toml           # workspace root
  app/
    Aura.toml          # [package] name = "app"
    src/main.au
  util/
    Aura.toml          # [package] name = "util"
    src/math.au
```

Each member keeps its own `[package]` section and dependency list. The
workspace root only declares membership.

See [examples/packages/workspace/Aura.toml](../examples/packages/workspace/Aura.toml) and [examples/packages/workspace/app/src/main.au](../examples/packages/workspace/app/src/main.au).

## Lockfiles

Aura writes an `Aura.lock` file that records the resolved dependency graph.
Its location depends on the project shape:

- For a standalone package, it sits beside `Aura.toml`.
- For workspace members, it sits at the workspace root.

The lockfile records:

- each local path dependency with its relative path
- each git dependency with its source URL and the exact pinned revision

This makes builds reproducible. Later runs use the pinned revisions until you
update the lockfile yourself.

To refresh moving git references, run the update command from inside the
package or workspace:

```bash
aura deps update
aura deps update util
```

`aura deps update` refreshes every branch, tag, and default-`main` git
dependency in the current package graph. `aura deps update util` refreshes
only the named git dependency.

## Current Limits

The package system is local-first:

- The supported dependency forms are `{ path = "..." }` and
  `{ git = "...", branch/tag/rev = "..." }`.
- A version-only registry dependency such as `util = "0.1.0"` is rejected
  with a clear diagnostic.
- There is no registry, publish, or install flow.
- There is no version solving.
