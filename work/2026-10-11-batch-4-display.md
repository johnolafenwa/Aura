# Batch 4 Section E: Display And Read-Only Properties

## Goal

Implement section E of the
[Batch 4 design checkpoint](../architecture_docs/18-batch-4-design-checkpoint.md):
the prelude `Display` trait with unified rendering dispatch, the custom-Display
f-string format subset, atomic `print`, and read-only `@property` getters, with
`AU2043`, `AU2044`, and `AU2045` and language-server support.

## Work Completed

- The prelude `trait Display` with `def display(self) -> str`. A module that
  declares or imports its own `Display` shadows it.
- `AU2043` rules: local class or enum target, every specialization covered,
  no bounds, no overlap, exact method conformance.
- One rendering path for `print`, `str(value)`, f-string fields, and nested
  structural rendering on both backends. Each implementation gets a synthesized
  `render <Type>` wrapper. The MIR interpreter calls it directly. The direct
  backend registers each wrapper's native thunk at startup.
- The f-string string subset for custom-Display values: fill, alignment, width,
  precision, and `s`. Other flags are `AU2002`.
- `print` renders its whole line before one write. A trap in `display` writes
  nothing.
- `@property` lexing and parsing. Any other decorator is `AU2005`. Misplaced
  or repeated markers are `AU2044`.
- `AU2044` getter rules and `AU2045` use rules. The checker rewrites each
  property read into a getter call, so MIR and the direct backend need no
  property-specific lowering.
- Analysis hover, completion, document symbols, and definition for properties.
  The language server maps the `property` kind.
- Fixtures, the `examples/traits/display.au` and
  `examples/classes/properties.au` examples, manual, learn, and tutorial pages,
  and the ADR-0055 status.

- Coverage follow-up after hosted CI failed the compiler coverage gate.
  Fixtures now read properties in every statement and expression position
  the checker rewrites, and cover the remaining `AU2043` target cases and the
  thousands-separator `AU2002` case. A native runtime unit test drives the
  direct `Display` registry, and a rewriter unit test covers modules with
  extern items. The display hook receives the nominal type name, which
  removes an unreachable fallback arm from both backends.
- Fixed indexing a property result, such as `value.tags[0]`. The checker
  matched the getter as a receiver method and asked for type arguments.

## Verification

- `cargo test -p aura-compiler --test fixtures`
- `cargo test -p aura-compiler --lib`
- filtered `cargo test -p aura --test cli` runs for the new examples and the
  broken-module diagnostic
- direct `aura build --backend direct` runs of every new run-pass fixture,
  compared byte for byte with the MIR output
- Full `cargo llvm-cov --workspace` on the merged tree, measured locally:
  95.35% regions, 97.36% functions, 96.50% lines. Running as root fails
  `package_io_helpers_cover_local_error_paths` locally. That costs 27 lines
  and 6 functions of `package.rs` that hosted CI covers.
- Forced MIR and direct runs of `property_read_positions`,
  `display_format_structural_values`, `property_reads`, and
  `display_format_subset` match the expected stdout.
- `cargo test -p aura-compiler --lib format` and the fixtures test on the
  merged tree.
- `cargo fmt --all --check`, `npm run check:clippy`,
  `npm run check:doc-examples`, `npm run check:hygiene`, and
  `npm run coverage:lsp:check`

## Follow-Up

- Property hover shows the getter docstring once section D attaches
  docstrings to methods.
- The backend-parity matrix runs in hosted CI.
