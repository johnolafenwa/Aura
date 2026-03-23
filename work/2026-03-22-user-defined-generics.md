# 2026-03-22 User-Defined Generics

## Goal

Add the first user-defined generic language slice across the compiler, examples, tutorials, and tooling.

## Completed

- added parser and AST support for generic `class`, `enum`, and `def` declarations
- added checker support for generic type parameters, generic function inference, generic class constructors, generic enum payload constructors, and generic method calls
- kept runtime values type-erased while preserving generic type information in checking and MIR
- added compiler fixtures for parsing, checking, and running user-defined generic programs
- added a maintained generic example at `examples/generics/box_and_wrapper.au`
- added tutorial coverage in `tutorials/14-generics.md` and updated the current-surface chapters
- updated the LSP fallback parser/tests so generic declarations do not regress editor behavior

## Verification

- `cargo test -p aurora-compiler --test fixtures`

## Follow-up

- extend generic support to traits and the iteration model
- add module/import support so generic APIs can be split across files
- audit runtime type inference so generic type arguments survive more unannotated execution paths
