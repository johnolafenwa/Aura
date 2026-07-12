# Packages

An Aurora package is a directory containing `Aurora.toml` and a `src/` source root. The package graph determines module paths, dependency import prefixes, git revisions, and the owner of `Aurora.lock`.

Package resolution is performed before static checking. A malformed manifest, unresolved dependency, package cycle, invalid lockfile, or import that escapes its source root is a compile-time/tooling diagnostic.

## Package Manifest

The supported package manifest shape is:

```toml
[package]
name = "app"
version = "0.1.0"
edition = "2026"

[dependencies]
util = { path = "../util" }
```

All three package fields are required:

- `name` must match `[A-Za-z_][A-Za-z0-9_]*`; it is also the dependency import identifier
- `version` must begin with an ASCII digit and otherwise contain only ASCII letters, digits, `.`, `-`, or `+`
- `edition` must be exactly `"2026"` in Aurora 0.1

An empty or unsupported value is rejected. Hyphenated package names are invalid even though `-` is allowed later in the version string.

The conventional and required package source root is `src/`. A package entry selected by ordinary check/run/build commands must be under that root. Package-aware test entries may instead be under the root package's `tests/` directory; they receive logical module names beginning with `tests.`.

## Module Paths Inside A Package

A file path below `src/` maps to its dot-separated path without `.au`:

| File | Logical local module |
| --- | --- |
| `src/main.au` | `main` |
| `src/math.au` | `math` |
| `src/helpers/text.au` | `helpers.text` |

Local imports are not prefixed with the current package name:

```python
import helpers.text
from helpers.text import normalize
```

An import path maps directly to a `.au` file below the selected source root. Import traversal cannot escape that root, including through canonicalized filesystem paths. Cyclic source imports are rejected.

Imported modules contribute declarations, not runtime initialization. Their top-level executable statements do not run as import side effects in Aurora 0.1. Visibility and import binding behavior are specified in [Names And Scopes](/manual/names-and-scopes#imports).

## Dependency Sources

Each dependency table entry must choose exactly one source:

| Source | Example | Resolution contract |
| --- | --- | --- |
| Local path | `util = { path = "../util" }` | Resolves a package directory relative to the declaring manifest. |
| Git revision | `util = { git = "...", rev = "abcdef0" }` | Uses that exact 7–64 digit hexadecimal revision. |
| Git tag | `util = { git = "...", tag = "v1.0.0" }` | Resolves the tag and pins its exact revision. |
| Git branch | `util = { git = "...", branch = "main" }` | Resolves the branch and pins its exact revision. |
| Git default | `util = { git = "..." }` | Resolves branch `main` and pins its exact revision. |

`path` and `git` cannot appear together. A git entry may choose at most one of `rev`, `tag`, or `branch`; selectors without `git` are invalid.

String version dependencies such as `util = "1.2.0"` and detailed `version =` dependencies are registry forms and are not implemented. Aurora 0.1 has no registry resolution, publish, or install flow.

The dependency table key is not a free alias: it must exactly match the resolved dependency's `[package].name`. This keeps the manifest name and import root identical. A dependency package named `util` is imported with that prefix:

```python
import util.math

print(util.math.double(21))
```

Inside a dependency, its own local imports remain relative to its `src/`; when exposed in the loaded graph, dependency module identities retain the dependency package prefix.

## Dependency Graph Rules

Resolution recursively loads path and git dependencies and enforces:

- no cyclic package dependency path
- one canonical directory for each package name in the graph
- the dependency key equals the resolved package name
- at most 1,024 direct dependencies per package
- at most 4,096 packages in one resolved graph
- every package has a readable `src/` directory and a valid package manifest

Two different paths cannot both claim the same package name. The graph limits are observable Aurora 0.1 limits and may be raised only with corresponding reference and conformance changes.

## Workspaces

A workspace-only root manifest lists exact member paths:

```toml
[workspace]
members = ["app", "util"]
```

Each member path is resolved relative to the workspace root and must identify a package with its own `[package]` manifest and `src/`. Membership is an exact normalized path match; glob patterns are not implemented.

A package is governed by an ancestor workspace only when its manifest directory appears in that workspace's member list. Workspace membership does not automatically make one member importable by another. The consuming member still declares the other package under `[dependencies]`, commonly as a local path dependency.

The workspace owns one `Aurora.lock`. A standalone package owns `Aurora.lock` beside its own manifest. Running `aura deps update` from a workspace-only root resolves all declared members and their dependency graphs; an empty workspace has nothing to update and is rejected.

## Lockfile Contract

`Aurora.lock` version 1 records every resolved package in deterministic package-name order. Path entries record a path relative to the lockfile root. Git entries record the normalized source, exact resolved revision, and the original tag or branch selector where applicable.

Conceptual examples:

```toml
version = 1

[[package]]
name = "util"
version = "0.1.0"
source = "git"
git = "https://example.com/util.git"
rev = "0123456789abcdef0123456789abcdef01234567"
branch = "main"
```

For tag, branch, and default-`main` dependencies, ordinary resolution reuses a matching locked revision instead of silently following a moved remote reference. An explicit `rev` is already immutable. A lockfile with an unsupported version, malformed source entry, missing path/git/revision, invalid selector, or unsupported source kind is rejected.

File-backed `check`, `run`, `build`/MIR loading, and explicit source-buffer check paths resolve the package graph and may create or rewrite the owning lockfile after successful loading. Compiler analysis and completion of editor buffers deliberately use the no-lockfile path so diagnostics and completions do not dirty the workspace. `aura deps update` always writes the applicable package/workspace lockfile after successful resolution.

Applications should commit `Aurora.lock` when reproducible dependency resolution matters. The lockfile is generated state; edit the manifest and use the resolver/update command rather than hand-maintaining revision entries.

## Updating Git Dependencies

From a package or workspace directory, refresh every eligible git dependency:

```bash
aura deps update
```

During repository development the equivalent is:

```bash
cargo run -p aura -- deps update
```

Refresh one named git dependency:

```bash
aura deps update util
```

An all-dependency update refreshes tag, branch, and default-`main` selectors. Exact `rev` dependencies are not refreshed. A named update requires that the name be present in the current graph and refer to a git dependency; path packages are rejected as update targets.

## Git Resolution, Cache, And Safety

A git source is either an explicit URL/SSH form or an existing local path relative to the declaring manifest. Empty sources, option-like sources beginning with `-`, invalid revision text, and unsafe tag/branch spellings are rejected before invoking git.

Aurora disables interactive git credential prompts so package commands fail rather than hang waiting for terminal input. Each git command has a 60-second default timeout. Set `AURORA_GIT_TIMEOUT_MS` to a positive millisecond value to override that timeout.

Resolved revisions are materialized in a content-addressed cache under `$XDG_CACHE_HOME/aurora/git`, otherwise `$HOME/.cache/aurora/git`, with a temporary-directory fallback when needed. Cached entries are validated against their recorded revision. Aurora refuses symlinked cache paths, symlinked manifests, and symlinked content in a git checkout; clones also disable symlink materialization. Concurrent cache placement uses a compatible existing checkout only when it validates to the same revision.

These checks are part of package loading behavior, not a guarantee that dependency source is trustworthy. Applications must still review and pin the code they execute.

## Package Root Discovery

For a file-backed command, Aurora walks upward from the selected path to find the nearest `Aurora.toml` containing `[package]`. It then checks whether an ancestor workspace exactly lists that package as a member and chooses the corresponding lockfile root.

A workspace-only manifest is not itself a package source root. Commands operating from a directory, such as `deps update`, may discover either an enclosing package or an enclosing workspace. Malformed manifests encountered during discovery are reported rather than silently skipped.

For stdin-backed compiler commands, the supplied path still controls package discovery, import resolution, diagnostics, and module identity while source text comes from stdin. Whether that command writes a lockfile follows the command-specific rule above. See [CLI And Tooling](/manual/cli-and-tooling#stdin-buffers).

## Visibility Across Packages

Only `public` top-level classes, enums, functions, and traits can be imported from another module. Public classes still enforce field and method visibility separately. Trait implementations loaded through package modules participate in dispatch with their defining module identities preserved.

```python
from util.math import double

print(double(21))
```

Package boundaries do not create implicit public exports, wildcard imports, relative import syntax, or prelude re-exports. The exact import grammar is in [Grammar](/manual/grammar#imports).

## Current Boundaries

- registry dependencies are not implemented
- workspace membership uses exact paths, not globs
- there is no implicit dependency between workspace members
- source roots are fixed at `src/`
- ordinary package entry files must be below `src/`; package test programs may be below the root `tests/`
- package graphs and direct dependency counts have the documented finite limits

See [Current Limits](/manual/current-limits#runtime) for the broader maintained implementation limits and [Conformance](/manual/conformance) for package test coverage.
