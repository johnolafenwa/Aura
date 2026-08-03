# S1 module constants

## Goal

Implement accepted ADR-0050 as Aura 0.3 behavior: immutable module constants,
deterministic eager initialization, one defining storage identity, compiler and
editor analysis, and maintained reference/tutorial/example coverage.

## Work completed

- Added inferred, annotated, and `public` module-constant declarations beside
  imports, items, executable entry statements, and `main`.
- Added declaration-order checking, collision and visibility handling,
  dependency imports, shared non-Copy ownership, Copy reads, precise move and
  mutation diagnostics, and indirect initialization re-entry diagnostics.
- Added a canonical dependency-first initialization plan. It follows first
  import source order, follows constant declaration order, and visits a diamond
  dependency once.
- Added eager once-only storage and reverse release to MIR and direct execution.
  MIR ready values use one `Arc<Value>` allocation, and direct values retain one
  opaque allocation. Imported aliases preserve the defining storage identity.
- Added compiler analysis and LSP constant symbols, hover, definitions, global
  completion, and module-member completion.
- Added multi-module, package, ownership, failure, backend-parity, parser,
  analysis, and shared-allocation regression coverage.
- Updated the Manual grammar and module semantics, the module tutorial, the
  maintained example catalog, and executable reference examples affected by
  the rule that mutable state is local to an explicit owner.

## Verification

- `cargo test -p aura-compiler module_constant -- --nocapture`: 5 passed.
- `cargo test -p aura-compiler --test fixtures`: 9 fixture families passed.
- Package dependency constant focused test: passed.
- MIR/direct runs for local, imported, stateful shared, and indirect-re-entry
  fixtures: byte-equivalent observable output and diagnostics.
- LSP helper conversion and module-constant integration tests: 2 passed.
- `npm run check --workspace tools/aura-language-server`: passed.
- Executable Manual reference integrity: 127 verified blocks passed; all 27
  feature pages retain executable coverage.

The repository-wide full gate was intentionally not run for this isolated
ticket. The parent Batch S1 integration owns the combined gate.

## Follow-up

Integrate this commit with the other S4 ticket commits, resolve any overlapping
generated test-literal additions mechanically, and run the combined Batch S1
gate once at the integration checkpoint.
