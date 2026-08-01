# Packages And Workspaces

Aura supports a package system built around `Aura.toml` manifest files. Packages let you organize larger projects with multiple source directories, share code through local path and git dependencies, and group related packages into workspaces.

## Single Package

A package has an `Aura.toml` manifest and source files under `src/`:

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

The compiler treats the directory containing `Aura.toml` as the package root and `src/` as the source root. Local imports resolve relative to `src/`:

```python
import helpers.math    # resolves to src/helpers/math.au
```

## Local Path Dependencies

Declare dependencies relative to the manifest directory:

```toml
[dependencies]
util = { path = "../util" }
```

Then import through the package name:

```python
import util.math
```

The dependency package must have its own `Aura.toml` with a matching `name`. Transitive dependencies are resolved through the package graph.

See [examples/packages/local_path_dependencies/app/src/main.au](../examples/packages/local_path_dependencies/app/src/main.au).

## Git Dependencies

Dependencies can also come from git repositories:

```toml
[dependencies]
util = { git = "https://github.com/example/util.git" }
jsonx = { git = "https://github.com/example/jsonx.git", tag = "v0.3.1" }
release_math = { git = "https://github.com/example/math.git", branch = "release" }
frozen_math = { git = "https://github.com/example/math.git", rev = "4f2c9d8b7e..." }
```

Git dependencies support three selectors:

- `branch = "name"` -- track a branch (default: `"main"` when no selector is provided)
- `tag = "v1.0.0"` -- pin to a specific tag
- `rev = "abc123..."` -- pin to an exact commit

Imports work the same way as path dependencies -- use the package name:

```python
import util.math
import jsonx.parser
```

## Workspaces

Workspace roots group related packages under a single top-level manifest:

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

Member packages keep their own `[package]` section and dependency lists. The workspace root only declares membership.

See [examples/packages/workspace/Aura.toml](../examples/packages/workspace/Aura.toml) and [examples/packages/workspace/app/src/main.au](../examples/packages/workspace/app/src/main.au).

## Lockfiles

Aura writes an `Aura.lock` file to record the resolved dependency graph:

- for standalone packages: beside `Aura.toml`
- for workspace members: at the workspace root

The lockfile records:

- local path dependencies with their relative paths
- git dependencies with their source URL and the exact pinned revision

This ensures reproducible builds. Later runs use the pinned revisions from the lockfile until you explicitly update it.

When you want to refresh moving git references, use the CLI update command from inside the package or workspace:

```bash
aura deps update
aura deps update util
```

`aura deps update` refreshes all branch/tag/default-main git dependencies in the current package graph. `aura deps update util` refreshes only the named git dependency.

## Current Limits

The package system is intentionally local-first:

- supported dependency forms: `{ path = "..." }` and `{ git = "...", branch/tag/rev = "..." }`
- version-only registry dependencies like `util = "0.1.0"` are rejected with a clear diagnostic
- no registry, publish, or install flows yet
- no version solving

This is deliberate -- Aura supports real multi-package development before taking on registry infrastructure.
