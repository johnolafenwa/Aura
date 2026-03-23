# 2026-03-18 Bootstrap Build Command

## Goal

Add a real `aura build` command so Aurora can produce a standalone runnable artifact instead of only executing source through `aura run`.

## Work Completed

- added `aura build -o <output> <file.au>` and `aura build -o <output> --stdin <virtual-path>` to the CLI
- kept the command test-first by adding a failing CLI product test before implementation
- implemented a bootstrap build path that:
  - type checks the source first
  - generates a temporary Rust launcher embedding the Aurora source
  - invokes `rustc`
  - links the launcher against the already-built `aurora-compiler` crate
  - produces a standalone native binary
- added runtime artifact lookup logic for `aurora-compiler` dependencies so the command works from the current Cargo workspace layout
- updated the repo, CLI, examples, tutorial, and task-board docs to describe the new command and its current scope

## Verification

- `cargo test -p aura --test cli build_produces_a_runnable_binary -- --exact`
- `cargo test -p aura --test cli`
- `cargo run -p aura -- build -o ./target/aurora-point examples/point.au`
- `./target/aurora-point`
- `cat examples/point.au | cargo run -p aura -- build -o ./target/aurora-point-stdin --stdin /virtual/point.au`
- `./target/aurora-point-stdin`

## Follow-up

- replace the bootstrap launcher build with a real MIR-native backend/codegen path
- add broader `aura build` regression coverage once the backend surface grows
- decide whether build profiles or optimization flags should become part of the CLI surface
