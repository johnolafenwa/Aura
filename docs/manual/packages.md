# Packages

An Aura package is a directory that contains `Aura.toml` and a `src/` source
root. The package graph determines module paths, dependency import prefixes,
git revisions, and which directory owns `Aura.lock`.

Package resolution runs before static checking. These problems are
compile-time or tooling diagnostics:

- a malformed manifest
- an unresolved dependency
- a package cycle
- an invalid lockfile
- an import that escapes its source root

## Package Manifest

A package manifest has this shape:

```toml
[package]
name = "app"
version = "0.1.0"
edition = "2026"

[dependencies]
util = { path = "../util" }
```

All three `[package]` fields are required. An empty or unsupported value is
rejected.

| Field | Rule |
| --- | --- |
| `name` | Must match `[A-Za-z_][A-Za-z0-9_]*`. It is also the dependency import identifier. Hyphenated names are invalid, even though `version` allows `-`. |
| `version` | Must begin with an ASCII digit. The rest may contain only ASCII letters, digits, `.`, `-`, or `+`. |
| `edition` | Must be exactly `"2026"` in Aura 0.3. |

### FFI Opt-In

`allow_ffi = true` is an optional `[package]` field. Its default is `false`.
It authorizes that package to contain FFI declarations. It grants no
permission to other packages and does not validate the native code.

When any direct or transitive dependency enables FFI, the root package must
also opt in and give an exact dependency report:

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

The report lists every reachable FFI-enabled dependency by package name,
including transitive dependencies. It does not list the root package. The
resolver rejects duplicate, unknown, unreachable, and non-FFI entries. It also
rejects a report that omits an FFI-enabled dependency. Every listed dependency
must set its own `[package] allow_ffi = true`. See
[FFI v0](/manual/ffi).

### Source Roots

The package source root is always `src/`. A package entry selected by ordinary
`check`, `run`, or `build` commands must be under that root. Package-aware test
entries may instead be under the root package's `tests/` directory. Their
logical module names begin with `tests.`.

## Module Paths Inside A Package

A file below `src/` maps to its dot-separated path without `.au`:

| File | Logical local module |
| --- | --- |
| `src/main.au` | `main` |
| `src/math.au` | `math` |
| `src/helpers/text.au` | `helpers.text` |

Local imports do not start with the current package name:

```aura fragment
import helpers.text
from helpers.text import normalize
```

An import path maps to a `.au` file below the selected source root. Import
traversal cannot escape that root, including through canonicalized filesystem
paths. Cyclic source imports are rejected.

Imported modules contribute declarations, not runtime initialization. Their
top-level executable statements do not run on import in Aura 0.3.
[Names And Scopes](/manual/names-and-scopes#imports) specifies visibility and
import binding.

## Dependency Sources

Each dependency entry chooses exactly one source:

| Source | Example | Resolution contract |
| --- | --- | --- |
| Local path | `util = { path = "../util" }` | Resolves a package directory relative to the declaring manifest. |
| Git revision | `util = { git = "...", rev = "abcdef0" }` | Uses that exact 7–64 digit hexadecimal revision. |
| Git tag | `util = { git = "...", tag = "v1.0.0" }` | Resolves the tag and pins its exact revision. |
| Git branch | `util = { git = "...", branch = "main" }` | Resolves the branch and pins its exact revision. |
| Git default | `util = { git = "..." }` | Resolves branch `main` and pins its exact revision. |

- `path` and `git` cannot appear together.
- A git entry may choose at most one of `rev`, `tag`, or `branch`.
- A selector without `git` is invalid.

Registry forms are not implemented. These include string-valued version
dependencies such as `util = "1.2.0"` and detailed `version =` dependencies.
Aura 0.3 has no registry resolution, publish, or install flow.

The dependency key is not a free alias. It must exactly match the resolved
dependency's `[package].name`, so the manifest name and the import root are
identical. A dependency package named `util` is imported with that prefix:

```aura fragment
import util.math

print(util.math.double(21))
```

Inside a dependency, local imports stay relative to that dependency's `src/`.
In the loaded graph, dependency module identities keep the dependency package
prefix.

## Dependency Graph Rules

Resolution loads path and git dependencies recursively and enforces these
rules:

- no cyclic package dependency path
- one canonical directory for each package name in the graph
- the dependency key equals the resolved package name
- at most 1,024 direct dependencies per package
- at most 4,096 packages in one resolved graph
- every package has a readable `src/` directory and a valid package manifest

Two different paths cannot claim the same package name. The graph limits are
observable Aura 0.3 limits. Raising them requires matching reference and
conformance changes.

## Workspaces

A workspace-only root manifest lists exact member paths:

```toml
[workspace]
members = ["app", "util"]
```

Each member path resolves relative to the workspace root. It must identify a
package with its own `[package]` manifest and `src/`. Membership is an exact
normalized path match. Glob patterns are not implemented.

An ancestor workspace governs a package only when the package's manifest
directory is in that workspace's member list. Membership does not make one
member importable by another. The consuming member still declares the other
package under `[dependencies]`, usually as a local path dependency.

The workspace owns one `Aura.lock`. A standalone package owns `Aura.lock`
beside its own manifest. Running `aura deps update` from a workspace-only root
resolves all declared members and their dependency graphs. An empty workspace
has nothing to update and is rejected.

## Lockfile Contract

`Aura.lock` version 1 records every resolved package, ordered by package name.

- Path entries record a path relative to the lockfile root.
- Git entries record the normalized source, the exact resolved revision, and
  the original tag or branch selector where there is one.

A conceptual example:

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

For tag, branch, and default-`main` dependencies, ordinary resolution reuses a
matching locked revision. It does not follow a moved remote reference. An
explicit `rev` is already immutable.

The resolver rejects a lockfile with an unsupported version, a malformed
source entry, a missing path, git, or revision field, an invalid selector, or
an unsupported source kind.

### When The Lockfile Is Written

| Operation | Lockfile behavior |
| --- | --- |
| File-backed `check`, `run`, `build`/MIR loading, and explicit source-buffer check paths | Resolve the package graph. May create or rewrite the owning lockfile after successful loading. |
| Compiler analysis and completion of editor buffers | Use the no-lockfile path, so diagnostics and completions do not modify the workspace. |
| `aura deps update` | Always writes the package or workspace lockfile after successful resolution. |

Commit `Aura.lock` when reproducible dependency resolution matters. The
lockfile is generated state. Edit the manifest and run the resolver or the
update command. Do not hand-edit revision entries.

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

An update of all dependencies refreshes tag, branch, and default-`main`
selectors. It does not refresh exact `rev` dependencies. A named update
requires the name to be in the current graph and to refer to a git dependency.
A path package is rejected as an update target.

## Git Resolution, Cache, And Safety

A git source is either an explicit URL or SSH form, or an existing local path
relative to the declaring manifest. Aura rejects these before invoking git:

- empty sources
- option-like sources that begin with `-`
- invalid revision text
- unsafe tag or branch spellings

Aura disables interactive git credential prompts, so package commands fail
instead of waiting for terminal input. Each git command has a 60-second
default timeout. Set `AURA_GIT_TIMEOUT_MS` to a positive millisecond value to
override it.

Resolved revisions live in a content-addressed cache under
`$XDG_CACHE_HOME/aura/git`, otherwise `$HOME/.cache/aura/git`. Aura falls back
to a temporary directory when needed.

- Cached entries are validated against their recorded revision.
- Aura refuses symlinked cache paths, symlinked manifests, and symlinked
  content in a git checkout.
- Clones disable symlink materialization.
- When placements race, Aura uses an existing checkout only if it validates to
  the same revision.

These checks are part of package loading. They do not make dependency source
trustworthy. Applications must still review and pin the code they run.

## Package Root Discovery

For a file-backed command, Aura walks upward from the selected path to the
nearest `Aura.toml` that contains `[package]`. It then checks whether an
ancestor workspace lists that package as an exact member, and chooses the
lockfile root to match.

A workspace-only manifest is not a package source root. Commands that operate
on a directory, such as `deps update`, may discover either an enclosing package
or an enclosing workspace. Discovery reports a malformed manifest instead of
skipping it.

For stdin-backed compiler commands, the supplied path controls package
discovery, import resolution, diagnostics, and module identity. The source
text comes from stdin. Whether the command writes a lockfile follows the rules
in [When The Lockfile Is Written](#when-the-lockfile-is-written). See
[CLI And Tooling](/manual/cli-and-tooling#stdin-buffers).

## Visibility Across Packages

Only `public` top-level classes, enums, functions, and traits can be imported
from another module. Public classes still enforce field and method visibility
separately. Trait implementations loaded through package modules take part in
dispatch with their defining module identities preserved.

```aura fragment
from util.math import double

print(double(21))
```

Package boundaries do not create implicit public exports, wildcard imports,
relative import syntax, or prelude re-exports. [Grammar](/manual/grammar#modules-and-imports)
gives the exact import grammar.

## Current Boundaries

- registry dependencies are not implemented
- workspace membership uses exact paths, not globs
- there is no implicit dependency between workspace members
- source roots are fixed at `src/`
- ordinary package entry files must be below `src/`
- package test programs may be below the root `tests/`
- package graphs and direct dependency counts have the finite limits listed
  above
- FFI declarations require a package manifest, the declaring package's
  explicit opt-in, and an exact root dependency report when dependencies use
  FFI

See [Current Limits](/manual/current-limits#runtime) for the broader
implementation limits and [Conformance](/manual/conformance) for package test
coverage.

## Grammar

Source imports have two forms, specified in [Grammar](/manual/grammar#modules-and-imports):

- `import dotted.module [as local]`
- `from dotted.module import name [as local]`

A from-import may name several direct or aliased names. Import paths are
absolute within the resolved local or dependency namespace. The resolver uses
the path before `as`. The alias is a local binding and is never read as a
package, directory, or dependency key. Relative imports, wildcard imports, and
a package-name prefix for the current package are not part of the grammar.

`Aura.toml` and `Aura.lock` use TOML as external tooling formats. They are not
Aura source grammar. Their accepted keys, table shapes, selector combinations,
identifier rules, FFI opt-in and report fields, and lockfile version are
exactly the contracts on this page. Unrecognized source kinds and unsupported
dependency forms are rejected, not inferred.

## Typing Rules

Package and module resolution completes before static checking. Before
execution, it also authorizes every loaded FFI declaration against the
declaring package and the root dependency report.

An import binds a module namespace or a visible declaration, with its defining
module identity and declared type. A local module uses its `src/`-relative
dotted name. A dependency's package name is its import root. Only `public`
top-level declarations cross a module boundary, and class member visibility is
checked separately.

Imports do not erase types or ownership modes. Calls to imported functions and
methods are checked against their original signatures. Trait implementations
keep their defining module identities for coherence and dispatch. A package
manifest does not create an Aura value, an implicit export, a prelude, or a
relationship between workspace members.

Clone-safety obligations survive imports as part of the callable contract.
Namespace-qualified calls and directly imported calls enforce the same
inferred requirements after specialization. User-defined nominal types keep
their defining module identity during structural clone-safety analysis. An
unrelated type with the same leaf name in the importing module cannot replace
them.

## Runtime Semantics

Before loading source, resolution discovers the nearest package, any exact
containing workspace, the transitive path and git graph, and the applicable
lockfile. Imported modules contribute declarations only. Top-level executable
statements in an imported file do not run as module initialization. Only the
selected entry module supplies program execution.

Ordinary locked resolution reuses a matching exact git revision for moving
selectors. `deps update` refreshes eligible moving selectors and then rewrites
the owning version-1 lockfile deterministically. Successful file-backed
compiler paths may create or rewrite that lockfile. Analysis and completion of
editor buffers use the no-lockfile path.

## Ownership And Evaluation Order

An import binds compile-time namespaces and declarations, not runtime resource
values. Importing neither moves nor borrows a value and has no runtime
evaluation position. Ownership begins when an imported declaration is called,
constructed, or otherwise evaluated. It follows that declaration's normal
parameter, receiver, field, and return contracts.

Package traversal order cannot introduce initialization side effects. Lockfile
and git-cache writes are tooling side effects of successful graph resolution.
They happen before program execution, and a later runtime failure does not roll
them back.

## Diagnostics

| Code | Meaning |
| --- | --- |
| `AU1101` | Invalid syntax in a loaded Aura module, or malformed TOML syntax in a manifest or lockfile. |
| `AU2001` | Module, import, package, or name resolution failed. |
| `AU2002` | A cross-module type mismatch. |
| `AU2004` | Imported-call argument binding failed. |
| `AU2999` | A manifest, lockfile, package-graph, source-root, cycle, limit, FFI authorization or dependency-report, or dependency-safety rejection without a narrower code. |
| `AU3001` | Use of a moved value through an imported declaration. |
| `AU3002` | A borrow violation through an imported declaration. |
| `AU3003` | A mutability violation through an imported declaration. |
| `AU3004` | An invalid ownership mode through an imported declaration. |
| `AU3007` | An imported callable's clone-safety obligation was not satisfied, or an imported nominal value would duplicate non-cloneable `random.Rng` state. |

File-backed `check`, `run`, and `build` render package-loading diagnostics
through the normal compiler diagnostic path.

`aura deps update` renders resolver failures in human form. It uses the same
stable `error[AU####]` code and exits with status `1`. Structured `--format`
output is limited to `check`, `run`, and `build`. A malformed `deps`
invocation is a command-usage error with status `2`, not a language
diagnostic.

## Backend Support

The shared compiler front end performs package discovery, resolution, import
loading, visibility, type checking, lockfile handling, and MIR lowering. So the
MIR runtime and the direct native backend receive the same resolved
declarations and module identities. Backend parity covers imported function
behavior and cross-package trait dispatch.

Built executables do not resolve source packages at runtime. Direct builds
contain emitted program code. MIR-launcher builds contain serialized checked
MIR and the runtime launcher. Package sources and the git cache are compiler
inputs, not runtime dependencies of the built program.

## Limits And Implementation-Defined Behavior

- Source roots are fixed at `src/`. Only package tests may enter through the
  root `tests/` directory.
- Each package may declare at most 1,024 direct dependencies.
- One graph may contain at most 4,096 packages.
- Workspace membership is an exact normalized path list, not a glob.
  Membership does not imply a dependency.
- Registry and version dependencies, publish, install, wildcard imports,
  relative imports, and implicit workspace dependencies are unavailable.

Git commands default to a 60-second timeout, disable interactive credential
prompts, and use the cache and symlink checks described above. The cache
location follows `XDG_CACHE_HOME`, then `HOME`, then a temporary fallback.
Network availability, git transport, filesystem canonicalization, and
credentials depend on the host. These controls protect resolver operation.
They do not establish trust in dependency source code.

## Status

These are implemented and maintained:

- single packages and exact-path workspaces
- path dependencies, and pinned and moving git selectors
- deterministic lockfile version 1
- package visibility and import aliases
- cross-package trait dispatch
- editor no-lockfile analysis
- package-local FFI authorization and exact root FFI dependency reporting

No package semantics on this page are provisional.

Registry resolution, publishing, installation, alternative source roots,
workspace globs, wildcard or relative imports, implicit re-exports, and
import-time initialization are outside the Aura 0.3 language contract.
