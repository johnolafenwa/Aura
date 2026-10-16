# Private Classes Across Modules on MIR

## Goal

Fix the `AU4001` "unknown MIR class" failure that John hit in an
application built with Aura 0.4.1. A module declared a private class and
used it inside its public API. Calling that API from another module failed
on the MIR backend, so the application had to make the class public.

## Cause

`push_imported_module_classes_from_namespace` in
`crates/aura-compiler/src/mir.rs` registered only `namespace.classes`, which
holds a module's public classes. The module's own function bodies are
lowered from `all_functions`, so they constructed and called private
classes that the MIR runtime never registered. The native backend lowers
classes on its own path and was not affected.

## Work Completed

- The MIR lowering registers each imported module's public classes and then
  the private classes it declares. It filters `all_classes` by owning module,
  because that map also lists classes the module imports. Visibility checks
  run in the checker and are unchanged: an importer still cannot name the
  private class.
- Regression tests in `crates/aura-compiler/tests/modules.rs`:
  `imported_module_can_use_its_private_class_at_runtime` and
  `transitively_imported_module_can_use_private_resource_and_generic_classes`.
  Both failed with `AU4001` before the fix.

## Verification

- `cargo test -p aura-compiler --test modules`: 23 passed.
- The reported program shape runs on `--backend mir`, `--backend direct`,
  and as a built binary.
- `cargo test -p aura-compiler -p aura`: all pass locally except the
  root-only `package_io_helpers_cover_local_error_paths`. One timing test,
  `loop_backedge_safepoints_prevent_socket_readiness_starvation`, failed
  once under the parallel run and passed three reruns alone.
- `npm run check:clippy` and `cargo fmt --all --check` pass.

## Follow-Up

- Applications that made a class public to work around this can make it
  private again once they move to a release with this fix.
