# 2026-04-16 Major Language Surface Pass

## Session

- Start: 2026-04-16 00:00:18 BST
- Goal: Finish the remaining major language gaps across the trait system, richer pattern matching, advanced ownership, and call/construction ergonomics.

## Plan

1. Audit the maintained docs and proposal-aligned limits so the missing surface is concrete.
2. Add failing compiler and product regressions for each missing behavior before implementation.
3. Implement the language/runtime/tooling changes in slices, keeping examples and tutorials aligned.
4. Run the full verification matrix and update the task board when the work is complete.

## Progress

- Session opened.
- Auditing the maintained and proposal surface to identify the exact missing pieces and current parser/checker/runtime seams before adding the first failing tests.
- Restored a green compiler-fixture baseline after the in-flight trait/spawn/enum-constructor changes, including the new `try`-via-`From[...]` path, spawnable associated/module call targets, positional class construction, bare built-in enum constructors with expected type, explicit `channel[T]()` calls, float literal match patterns, and initial enum payload keyword-argument support.
- Next slice in progress: richer enums and `match`, covering multi-payload variants, nested patterns, expression-form `match`, and the remaining enum construction ergonomics in one coherent pass.
- Active focus is now the current rich enum/match regression path, with the next step being a fresh rerun of the targeted fixture after the checker/interpreter groundwork to determine the remaining MIR/runtime gaps.
- Borrowed return-value checking is now partially wired in with explicit source syntax like `-> borrow[self] T` / `-> borrow[user] T`; the immediate next step is syncing the new check-fail diagnostics and rerunning the fixture harness before expanding the remaining trait/ownership surface.
- Session elapsed time refreshed to 9h 40m. The immediate next step is rerunning the narrowed compiler failure clusters against the current in-flight tree, then clearing the remaining checker/interpreter/direct-backend regressions before the 12-hour stop rule.

## Completed

- Finished the richer pattern-matching surface:
  - multi-payload enum variants
  - named enum payload fields
  - nested patterns
  - expression-form `match`
  - floating-point literal patterns
- Finished the next trait-system slice:
  - default trait methods
  - generic trait impl headers
  - richer operator coverage with ordering traits for `<`, `<=`, `>`, and `>=`
  - maintained examples and CLI smoke coverage for `operator_traits.au` and `ordering_traits.au`
- Finished the ownership/call ergonomics slice:
  - positional class constructors
  - keyword enum payload arguments
  - bare built-in enum constructors with expected type
  - explicit `channel[T]()` calls
  - broader `spawn` / `TaskGroup.spawn(...)` targets for module-qualified functions and associated methods without `self`
  - borrowed return labels such as `borrow[shared]` flowing through calls and local bindings
- Added/updated maintained examples:
  - `examples/enums/rich_match.au`
  - `examples/traits/default_trait_methods.au`
  - `examples/traits/ordering_traits.au`
  - `examples/concurrency/spawn_associated_method.au`
  - `examples/classes/positional_constructors.au`
  - `examples/enums/constructor_ergonomics.au`
  - `examples/basics/borrowed_returns.au`
  - `examples/basics/borrowed_lifetime_labels.au`
- Updated the maintained tutorials/READMEs so the documented surface matches the implementation.

## Verification

- `cargo fmt --all`
- `cargo test -p aurora-compiler`
- `cargo test -p aura`
- `npm run test:lsp`
- `npm run check:extension`
