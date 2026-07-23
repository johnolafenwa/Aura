# 2026-04-15 Package Manifests and Dependencies

## Goal

Implement the first Aurora package-system milestone:

- `Aurora.toml` manifest parsing
- manifest-rooted `src/` packages
- local path dependencies
- multi-package workspaces
- manifest-aware `check` / `run` / `run-mir` / `build` / `analyze` / `complete`
- a first local lockfile
- explicit unsupported diagnostics for version-only registry dependencies

## Work Completed

- Added `Aurora.toml` manifest parsing to the compiler with support for `[package]`, `[dependencies]`, and `[workspace]`.
- Added manifest-rooted `src/` package loading plus local path dependency resolution by package name across `check`, `run`, `run-mir`, `build`, `analyze`, and `complete`.
- Added recursive package-graph resolution so transitive local path dependencies are resolved and available in the module registry.
- Added relative `Aurora.lock` generation at the package root or workspace root.
- Added clear diagnostics for version-only dependencies such as `util = "0.1.0"`, which remain intentionally unsupported for now.
- Added compiler regression coverage for standalone packages, workspace members, transitive path dependencies, lockfile generation, and unsupported version-only dependencies.
- Added CLI product coverage for manifest-aware `check`, `run`, `run-mir`, `build`, `analyze`, `complete`, stdin-backed tooling, and maintained package examples.
- Added a compiler-bridge regression for package-aware analysis and completions in the language-server package.
- Added maintained package examples under `examples/packages/local_path_dependencies/` and `examples/packages/workspace/` with committed relative lockfiles.
- Updated the README/tutorial/examples surface to teach the new package behavior and current limits.
- Normalized path handling for source-override entry files so package-aware analysis, completion, MIR lowering, and build paths stay stable even when the current buffer path does not yet exist on disk.

## Verification

- `cargo fmt --all`
- `cargo test -p aurora-compiler`
- `cargo test -p aura`
- `npm run test:lsp`
- `npm run check:extension`

## Follow-Up

- Add package-manager commands only after the manifest/path-dependency surface is stable enough to justify them.
- Extend the package system later with registry or git dependencies, version resolution, and publish/install flows instead of expanding that surface ad hoc.
