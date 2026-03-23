# 2026-03-22 Traits And Bridge Resolution

## Goal

Add the first trait slice across the compiler, runtimes, examples, tutorials, and tooling, then stabilize the compiler-backed language-server path around it.

## Completed

- added parser and AST support for `trait`, `impl Trait for Type`, and inline generic bounds like `T: Greeter`
- added semantic checking for trait declarations, trait impls, bounded generic call sites, and trait method signature conformance
- added interpreter and MIR runtime dispatch for trait methods implemented through `impl` blocks
- added compiler analysis support for trait symbols and trait-aware member completions
- added maintained coverage with compiler fixtures, `examples/traits/greeter.au`, and `tutorials/15-traits.md`
- updated the current-surface/tutorial/readme docs so traits are now documented as implemented behavior
- fixed the compiler-backed LSP bridge to prefer `cargo run -p aura --` inside the Aurora source repo, which prevents stale `target/debug/aura` binaries from hiding current compiler behavior during development and test runs

## Verification

- `cargo test`
- `npm run test:lsp`
- `npm run test:extension`
- `cargo run -p aura -- run examples/traits/greeter.au`
- `cargo run -p aura -- run-mir examples/traits/greeter.au`
- `cargo run -p aura -- analyze examples/traits/greeter.au`

## Follow-up

- add module/import support so traits and generic APIs can be split across files
- extend traits toward generic trait declarations, multiple bounds, and operator traits
- keep narrowing the JS language-server fallback as more compiler-backed editor behavior lands
