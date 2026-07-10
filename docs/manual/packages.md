# Packages

Aurora packages use `Aurora.toml` manifests. Packages define source roots, dependency names, and lockfile behavior.

## Package Manifest

```toml
[package]
name = "app"
version = "0.1.0"
edition = "2026"

[dependencies]
util = { path = "../util" }
```

The conventional source root is `src/`. Commands infer the nearest package root for files inside a package.

Package names must be valid Aurora import identifiers. Hyphenated names are rejected so the manifest name and import root cannot disagree.

## Workspace Manifest

```toml
[workspace]
members = ["app", "util"]
```

A workspace root coordinates member packages and owns the workspace `Aurora.lock`.

## Dependency Kinds

| Kind | Example | Contract |
| --- | --- | --- |
| Local path | `util = { path = "../util" }` | Uses a package on disk. Imports use `import util.module`. |
| Git branch | `util = { git = "...", branch = "main" }` | Resolves the branch and pins the exact revision in `Aurora.lock`. |
| Git tag | `util = { git = "...", tag = "v1.0.0" }` | Resolves the tag and pins the exact revision. |
| Git rev | `util = { git = "...", rev = "abc123" }` | Uses the exact revision. |

Registry dependencies are not implemented.

## Lockfiles

`Aurora.lock` records resolved dependency revisions. For git dependencies, the lockfile keeps builds repeatable.

Commands that work on persisted package files may update the lockfile when resolution changes. Stdin analysis and completion do not mutate lockfiles, so editor integrations can analyze unsaved buffers without dirtying the project.

## Dependency Updates

Refresh all eligible git dependencies:

```bash
cargo run -p aura -- deps update
```

Refresh one dependency:

```bash
cargo run -p aura -- deps update util
```

## Imports

Import a module:

```python
import util.math

print(util.math.double(21))
```

Import public names:

```python
from util.math import double

print(double(21))
```

Only `public` declarations are visible across module boundaries.

## Package Root Inference

When a command is given a source file under a package, Aurora walks upward to find the nearest `Aurora.toml`. That package root controls dependency resolution and local module imports.

For stdin commands, the supplied path is used for package-root and import resolution while the source text comes from stdin.
