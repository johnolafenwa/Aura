# 2026-03-23: Generics, Patterns, and Duration Fixes

## Goal

Address the latest review findings in the generic type system, trait dispatch, pattern matching, literal parsing, runtime numeric safety, and maintained user-facing surface.

## Work Completed

- fixed generic method inference for method calls on generic class instances inside generic functions
- fixed user-defined generic enum unit variants so instantiated types are preserved
- fixed runtime trait dispatch for specialized generic impls such as `impl Trait for Box[String]`
- raised integer and duration literal parsing to `i128`
- added minute duration literal support with the `m` suffix
- added wildcard `case _:` support in statement-form `match`
- added trait bounds on generic class and enum type parameters
- added empty marker traits with `pass`
- rejected direct recursive class fields that would require `indirect` storage
- fixed direct-expression narrow-integer overflow checking in the interpreter and MIR runtime
- fixed whole-number float rendering so values like `5.0` and `9.0` keep their `.0`
- added maintained examples, fixture coverage, and tutorial updates for the new supported surface
- rebased the compiler coverage floor to the new measured baseline after the language-surface expansion

## Verification

- `cargo test -p aurora-compiler --test fixtures`
- `cargo test`
- `npm run test:lsp`
- `npm run check:extension`
- `npm run test:extension`
- `npm run coverage:compiler:check`
- `npm run coverage:lsp:check`
- direct example probes for `point_distance.au`, `float32_values.au`, `numeric_casts.au`, and the new maintained examples

## Follow-up

- free-function `` parameters are still not implemented
- nested patterns and expression-form `match` are still outside the compiler
- the backend is still a bootstrap MIR-artifact launcher rather than final standalone native codegen
