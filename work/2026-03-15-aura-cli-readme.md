# 2026-03-15 aura CLI README Session

## Goal

Add a focused README that explains how to build the Aurora CLI once and use the resulting compiler binary directly without `cargo run`.

## Changes

- added `crates/aura/README.md`
- documented `cargo build -p aura --release`
- documented direct binary usage through `./target/release/aura`
- documented the current `check`, `run`, and `ast` commands
- documented copying the compiled binary onto `PATH`

## Verification

- instructions were matched against the current CLI implementation in `crates/aura/src/main.rs`
- `cargo build -p aura --release` passed
- `./target/release/aura check examples/point.au` printed `ok`
- `./target/release/aura run examples/basic_addition.au` printed `16`
- `./target/release/aura run examples/top_level_addition.au` printed `16`
