# Packages And Workspaces

Aurora now supports a first local package-system milestone built around `Aurora.toml`.

## Single Package

Package source files live under `src/`.

```toml
[package]
name = "app"
version = "0.1.0"
edition = "2026"
```

```text
app/
  Aurora.toml
  src/main.au
  src/helpers/math.au
```

Run the package entrypoint by pointing `aura` at the real file under `src/`:

```bash
cargo run -p aura -- run examples/packages/local_path_dependencies/app/src/main.au
```

The current compiler treats the directory containing `Aurora.toml` as the package root and `src/` as the package source root.

## Local Path Dependencies

Dependencies are declared relative to the manifest directory:

```toml
[dependencies]
util = { path = "../util" }
```

Import them through the package name:

```python
import util.math
```

Inside the current package, local imports still stay local:

```python
import helpers.math
```

See [examples/packages/local_path_dependencies/app/src/main.au](../examples/packages/local_path_dependencies/app/src/main.au).

## Workspaces

Workspace roots group member packages under a top-level manifest:

```toml
[workspace]
members = ["app", "util"]
```

Member packages still carry their own `[package]` section and dependency lists.

See [examples/packages/workspace/Aurora.toml](../examples/packages/workspace/Aurora.toml) and [examples/packages/workspace/app/src/main.au](../examples/packages/workspace/app/src/main.au).

## Lockfiles

Aurora now writes a local `Aurora.lock`:

- beside the package manifest for a standalone package
- at the workspace root for workspace members

The current lockfile records the resolved local package graph with relative package paths.

## Current Limits

The package system is intentionally local-first right now:

- supported dependency form: `{ path = "../other-package" }`
- unsupported for now: registry versions like `util = "0.1.0"`
- unsupported for now: git dependencies, publishing, install flows, and version solving

That is deliberate. Aurora can now support real multi-package local development before taking on registry infrastructure.
