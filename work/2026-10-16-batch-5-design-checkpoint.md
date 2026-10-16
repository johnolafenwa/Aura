# 2026-10-16 Batch 5 Design Checkpoint

## Goal

John asked to start Batch 5, lean native binaries. The roadmap starts each
batch with a design checkpoint, so this work measures today's executables,
finds why unused runtime code stays linked, and proposes the design in
`architecture_docs/19-batch-5-design-checkpoint.md` for John's ruling. No
implementation code changes.

## Work Completed

- Built `b516bd5` with `cargo build --release -p aura -p aura-compiler` on
  Linux x86-64, Rust 1.95.0, GCC 13.3, GNU ld 2.42.
- Measured `aura` (16,630,416 bytes), `target/release/libaura_compiler.a`
  (132,235,406 bytes), hello world (5,057,904 bytes), and the reference
  agent (5,384,768 bytes), each built with `aura build --backend direct`.
  A Rust hello world with fat LTO, one codegen unit, and stripped symbols is
  305,880 bytes.
- Rebuilt hello world with `AURA_NATIVE_KEEP_SYMBOLS=1` and grouped
  `nm -S --size-sort -C` output by crate and module. The checkpoint's
  component table comes from that grouping.
- Built a call graph from `objdump -d` of the symbol-kept hello world and
  searched it from `main`. It shows the direct edge `main` →
  `aura_direct_run_root` → `drop_in_place<Box<dyn rustls::State<..>>>`,
  which is the `Value` drop glue reaching TLS state.
- Confirmed in source: the string-keyed `evaluate_host_builtin_with_args`
  dispatcher, emitted wrappers for all 110 builtin-module functions in hello
  world, the runtime's use of `builtin_modules::host_builtin_metadata`, JSON
  type text decoded with `serde_json`, and `rustls` default features
  building `aws-lc-rs`.
- Found a tracked 1,001,545-byte `crates/aura-compiler/src/mir.rs.orig`
  left from commit `a01a2a3`. The checkpoint schedules its removal in
  step 0.
- Found that secure WebSocket clients build their root store from the
  platform's certificates, while Aura's TLS client uses `webpki-roots`. The
  checkpoint keeps this behavior and records it.

## Verification

Documentation only. No compiler, CLI, or runtime code changed.

## Follow-Up

- John's rulings on the four owner questions in the checkpoint.
- After ratification, the six-step delivery plan in the checkpoint.
