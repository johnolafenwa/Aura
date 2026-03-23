# 2026-03-18 MIR-Native `with`

## Goal

Remove `with` from the backend fallback surface by making MIR preserve and execute deterministic cleanup semantics directly.

## Work Completed

- added a failing compiler test that lowers `examples/resources/with_resource.au` to MIR and runs it directly through `run_mir(...)`
- added explicit MIR cleanup instructions so the lowerer now preserves `with` resource lifetime instead of erasing it into a plain binding
- taught the MIR lowerer to:
  - push cleanup scope on `with` entry
  - pop cleanup on normal `with` exit
  - emit cleanup before explicit `return`, `break`, and `continue` when those exits leave a `with` scope
- added a MIR-runtime cleanup stack so early returns, including `try`-driven early return, unwind active resources correctly
- implemented MIR cleanup execution for resource instances with `close(...)`
- removed the old MIR support gate that rejected `with`
- updated the CLI/backend regression coverage so fallback build coverage now uses a real remaining fallback example from the concurrency surface
- updated docs and task tracking so `with` is no longer listed as a backend fallback feature

## Verification

- `cargo test -p aurora-compiler tests::mir_runtime_runs_with_example_natively -- --exact`
- `cargo test -p aura --test cli run_mir_executes_with_example -- --exact`
- `cargo test -p aura --test cli build_handles_backend_fallback_examples -- --exact`

## Follow-up

- implement native MIR/runtime support for `spawn`
- implement native MIR/runtime support for `select` and the concurrency/runtime surface
- keep shrinking backend fallbacks until `run-mir` and `build` no longer depend on interpreter fallback
