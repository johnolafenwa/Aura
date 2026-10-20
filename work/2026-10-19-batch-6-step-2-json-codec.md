# Batch 6 Step 2: The `json.Codec` Trait

## Goal

Add the `json.Codec` trait, `json.DecodeError`, and the generic entry points
`json.encode`, `json.decode`, `json.decode_text`, and `json.schema`, as the
ratified Batch 6 checkpoint describes. Implementations are explicit in this
step. Step 3 derives them.

## Work Completed

- Module-qualified trait names. `impl shapes.Make for X`, `T: shapes.Make`,
  and `trait Described: shapes.Named` parse and check, through a module alias
  too. Each imported module's traits register as `prefix.Trait`. Trait
  identity resolves a qualified name to its declaring module, so
  `from shapes import Make` and `shapes.Make` are one trait. MIR records an
  implementation under the trait's declared name, which both backends
  dispatch by.
- Diagnostics name a bound by its qualified name unless the bare name refers
  to the same trait in the current module, as in
  `type int64 does not implement trait json.Codec`.
- Aura-source builtin parts. `builtin_sources.rs` checks
  `crates/aura-compiler/std/json.au` as module `json`, with the host `json`
  namespace imported under its own name. Its exports merge into the
  namespace `import json` and `from json import ...` bind. The checked part
  is cached per thread.
- The registry carries the part only when a loaded module imports `json`, so
  other programs compile exactly as before. Enums of a source part keep
  qualified runtime names. Native reachability keeps `Display` wrappers as
  roots, so `json.DecodeError` renders the same on both backends.
- `json.decode[T]` is an ordinary generic function that calls `T.decode`, so
  step 1 specializes it per type.
- `json.DecodeError` prints as `path: message`.

## Verification

- Run-pass `json_codec_explicit`, also on the direct backend through a CLI
  parity test.
- Check-fail `json_codec_missing_schema`, `json_decode_requires_codec`, and
  `qualified_trait_unknown`.
- CLI tests `module_qualified_traits_work_in_bounds_impls_and_supertraits_on_both_backends`
  and `json_codec_implementations_dispatch_across_modules_on_both_backends`.
- `builtin_sources_tests.rs` checks the merge and the registry flag.
- `examples/json/typed_codec.au` in the example smoke tests and the direct
  backend's example surface.

## Follow-Up

- Step 3 derives `encode` and `decode` for classes from an empty
  `impl json.Codec for T`.
- A hand-written decoder that decodes a nested value must prefix the nested
  error's path itself.
