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
- `edition` must be exactly `"2026"` in Aurora 0.2

`allow_ffi = true` is an optional `[package]` field whose default is `false`.
It authorizes that package to contain FFI declarations. It grants no ambient
permission to unrelated packages and does not validate the native code.

When any direct or transitive dependency enables FFI, the root package must
also opt in and provide an exact dependency report:

```toml
[package]
name = "app"
version = "0.1.0"
edition = "2026"
allow_ffi = true

[dependencies]
native = { path = "../native" }

[ffi]
dependencies = ["native"]
```

The report lists every reachable FFI-enabled dependency by its package name,
including transitive dependencies. It does not list the root package itself.
Duplicate, unknown, unreachable, and non-FFI entries are rejected, as is an
omitted FFI-enabled dependency. Every listed dependency must independently set
its own `[package] allow_ffi = true`. See [FFI v0](/manual/ffi).

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

Imported modules contribute declarations, not runtime initialization. Their top-level executable statements do not run as import side effects in Aurora 0.2. Visibility and import binding behavior are specified in [Names And Scopes](/manual/names-and-scopes#imports).

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

String version dependencies such as `util = "1.2.0"` and detailed `version =` dependencies are registry forms and are not implemented. Aurora 0.2 has no registry resolution, publish, or install flow.

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

Two different paths cannot both claim the same package name. The graph limits are observable Aurora 0.2 limits and may be raised only with corresponding reference and conformance changes.

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
- FFI declarations require a package manifest, the declaring package's
  explicit opt-in, and an exact root dependency report when dependencies use
  FFI

See [Current Limits](/manual/current-limits#runtime) for the broader maintained implementation limits and [Conformance](/manual/conformance) for package test coverage.

## Grammar

Source imports have the maintained forms `import dotted.module` and `from dotted.module import name`, as specified in [Grammar](/manual/grammar#imports). Import paths are absolute within the resolved local or dependency namespace. Relative imports, wildcard imports, aliases, and package-name prefixes for the current package are not grammar.

`Aurora.toml` and `Aurora.lock` use TOML as external tooling formats, not Aurora source grammar. Their accepted keys, table shapes, selector combinations, identifier rules, FFI opt-in/report fields, and lockfile version are exactly the contracts documented above; unrecognized source kinds or unsupported dependency forms are rejected rather than inferred.

## Typing Rules

Package and module resolution completes before static checking. It also
authorizes every loaded FFI declaration against the declaring package and
root dependency report before execution. An import binds a module namespace
or a visible declaration with its defining module identity and declared type.
Local modules use their `src/`-relative dotted name; a dependency's package
name is its import root. Only `public` top-level declarations cross a module
boundary, with class member visibility checked separately.

Imports do not erase types or ownership modes. Calls to imported functions and methods are checked against their original signatures, and trait implementations retain defining-module identities for coherence and dispatch. A package manifest does not create an Aurora value, implicit export, prelude, or relationship between workspace members.

Clone-safety obligations survive module imports as part of the callable contract.
Namespace-qualified and directly imported calls enforce the same inferred
requirements after specialization. User-defined nominal types retain their
defining module identity during structural clone-safety analysis, so an
unrelated same-leaf type in the importing module cannot replace them.

## Runtime Semantics

Resolution discovers the nearest package, any exact containing workspace, the transitive path/git graph, and the applicable lockfile before loading source. Imported modules contribute declarations only: top-level executable statements in an imported file are not run as module initialization. The selected entry module alone supplies program execution.

Ordinary locked resolution reuses a matching exact git revision for moving selectors. `deps update` deliberately refreshes eligible moving selectors and then deterministically rewrites the owning version-1 lockfile. Successful file-backed compiler paths may create or rewrite that lockfile; analysis and completion of editor buffers use the no-lockfile path.

## Ownership And Evaluation Order

An import binds compile-time namespaces and declarations, not runtime resource values, so importing neither moves nor borrows a value and has no runtime evaluation position. Ownership begins when an imported declaration is called, constructed, or otherwise evaluated, using the declaration's normal parameter, receiver, field, and return contracts.

Package traversal order cannot introduce initialization side effects. Lockfile and git-cache writes are tooling side effects that occur during successful graph resolution; they precede program execution and are not rolled back by a later runtime failure.

## Diagnostics

`AU1101` means invalid syntax in a loaded Aurora module or malformed TOML syntax in a manifest or lockfile. `AU2001` means module, import, package, or name resolution failed. `AU2002` means a cross-module type mismatch. `AU2004` means imported-call argument binding failed. `AU2999` means a manifest, lockfile, package-graph, source-root, cycle, limit, FFI authorization/dependency-report, or dependency-safety rejection without a narrower code. Through imported declarations, `AU3001` means use of a moved value, `AU3002` means a borrow violation, `AU3003` means a mutability violation, and `AU3004` means an invalid ownership mode. `AU3007` means an imported callable's clone-safety obligation was not satisfied or an imported nominal value would duplicate non-cloneable `random.Rng` state.

File-backed `check`, `run`, and `build` render package-loading diagnostics through the normal compiler diagnostic path. `aura deps update` renders compiler-owned resolver failures in human form with the same stable `error[AU####]` code and exit status `1`; structured `--format` output is limited to `check`, `run`, and `build`. Malformed `deps` invocation is a command-usage error with status `2`, not a language diagnostic.

## Backend Support

Package discovery, resolution, import loading, visibility, type checking, lockfile handling, and MIR lowering occur in the shared compiler front end. The MIR runtime and direct native backend therefore receive the same resolved declarations and module identities. Backend parity includes imported function behavior and cross-package trait dispatch.

Built executables do not resolve source packages at runtime. Direct builds contain emitted program code; MIR-launcher builds contain serialized checked MIR and the runtime launcher. The package sources and git cache are compiler inputs, not runtime dependencies of the built program.

## Limits And Implementation-Defined Behavior

Source roots are fixed at `src/`; package tests alone may enter through the root `tests/` directory. Each package may declare at most 1,024 direct dependencies, and one graph may contain at most 4,096 packages. Workspace membership is an exact normalized path list, not a glob, and membership does not imply a dependency. Registry/version dependencies, publish, install, wildcard imports, relative imports, and implicit workspace dependencies are unavailable.

Git commands default to a 60-second timeout, disable interactive credential prompts, and use the cache and symlink checks documented above. Cache location follows `XDG_CACHE_HOME`, then `HOME`, then a temporary fallback. Network availability, git transport, filesystem canonicalization, and credentials are host-dependent. These controls protect resolver operation; they do not establish trust in dependency source code.

## Status

Single packages, exact-path workspaces, path dependencies, pinned and moving git selectors, deterministic lockfile version 1, package visibility, cross-package trait dispatch, editor no-lockfile analysis, package-local FFI authorization, and exact root FFI dependency reporting are implemented and maintained in Aurora 0.2. No package semantics on this page are provisional.

Registry resolution, publishing, installation, alternative source roots, workspace globs, import aliases, wildcard or relative imports, implicit re-exports, and import-time initialization are unavailable. Any future mention of those facilities is non-normative until this reference and conformance suite are amended.
