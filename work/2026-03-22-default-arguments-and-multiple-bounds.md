# 2026-03-22 Default Arguments And Multiple Bounds

## Goal

Close more of the frozen v1 language surface by adding default parameter values and formally maintaining multiple trait bounds.

## Completed

- added parser and AST support for default parameter values on ordinary functions and class methods
- extended user-defined call binding so omitted optional parameters are filled from declared defaults
- added semantic checks for:
  - default parameter type compatibility
  - trailing-default ordering
  - no parameter-to-parameter references in defaults
  - no default arguments in trait or trait-impl method declarations
- added interpreter and MIR support so defaulted calls run consistently through both backends
- added compiler fixtures and a maintained example at `examples/basics/default_arguments.au`
- updated function tutorials and the current-surface reference to document the implemented default-argument rules
- verified that multiple trait bounds with `T: A + B` already work, then added fixtures, `examples/traits/multiple_bounds.au`, and tutorial/current-surface coverage so the feature is now part of the maintained language surface
- added an LSP fallback regression test for parsing/analyzing default-argument functions

## Verification

- `cargo test`
- `cargo test -p aurora-compiler --test fixtures`
- `cargo run -p aura -- run examples/basics/default_arguments.au`
- `cargo run -p aura -- run-mir examples/basics/default_arguments.au`
- `cargo run -p aura -- run examples/traits/multiple_bounds.au`
- `cargo run -p aura -- run-mir examples/traits/multiple_bounds.au`
- `npm run test:lsp`
- `npm run test:extension`

## Follow-up

- add module/import support so defaulted APIs and traits can be exercised across real multi-file programs
- continue extending traits toward generic trait declarations, generic impl headers, and operator traits
- keep shrinking the JS language-server fallback as more of the maintained surface moves under compiler-owned analysis
