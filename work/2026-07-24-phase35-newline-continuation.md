# Phase 3.5 newline continuation

## Goal

Implement the first Phase 3.5 language change as a lexical/layout extension:
allow expressions and delimited declaration parts to span physical lines
while `(`, `[`, or `{` remains open, without changing type, ownership, or
runtime semantics.

## Work Completed

- Added a typed source-delimiter stack with last-opened, first-closed pairing
  and source-related `AU1001` diagnostics for unexpected, mismatched, and
  unclosed delimiters.
- Suppressed ordinary `NEWLINE`, `INDENT`, and `DEDENT` formation while a
  source delimiter remains open. Continuation indentation is visual and does
  not mutate the surrounding block-indentation stack; physical tabs remain
  invalid.
- Preserved blank/comment-only handling, trailing comments, physical token
  lines and columns, and the newline after the outermost closer.
- Preserved expression-form `match` inside delimiters through explicit layout
  islands for its header, `case` arms, and indented arm expressions.
- Kept trailing commas, backslash continuation, multiline ordinary strings,
  and multiline f-strings unavailable. Delimiters inside strings, f-strings,
  and comments do not affect source continuation.
- Added Provisional
  `architecture_docs/decisions/0025-newline-continuation-and-delimited-layout.md`
  and aligned Lexical Structure, Grammar, Expressions, Functions,
  Collections, Current Limits, the conformance map, and the Manual index.
- Added `examples/basics/multiline_expressions.au`, registered its exact
  `80\n20\n` compiler smoke output, indexed it in the example catalog, and
  added it to the root quick-start surface. Added
  `tutorials/24-multiline-expressions.md` plus tutorial-surface updates.
- Preserved compiler-backed analysis of incomplete editor buffers whose
  dangling member calls now also leave source delimiters open. The language
  server retains symbols, occurrences, and member completions while reporting
  source-delimiter diagnostics with opener related information.
- Updated the VS Code newline command to recognize unmatched source
  delimiters without treating delimiters in ordinary strings, f-strings, or
  comments as source layout.
- Strengthened executable-reference integrity for the new positive surface,
  its retained negative boundaries, ADR/example/tutorial/editor evidence, and
  stale pre-continuation wording.

## Verification

- Focused lexer tests pass for all delimiter kinds, nested continuation,
  continuation indentation, blank/comment lines, physical spans, delimited
  match layout islands, physical tabs, and source-related pairing failures.
- Focused parser tests pass for the maintained multiline grammar positions and
  unchanged layout-island behavior.
- All 807 compiler unit tests and all nine fixture families pass. The full
  64-test language-server suite and 13-test VS Code extension suite pass,
  including the pre-existing multi-dangling-dot recovery contract.
- `cargo run -p aura -- run examples/basics/multiline_expressions.au` prints
  the exact maintained `80\n20\n` result.
- `npm run check:reference`, its 237-fence inventory, `npm run docs:build`,
  `cargo fmt --all -- --check`, and `git diff --check` pass.
- The forced MIR/direct fixture matrix and the exact full `npm run ci`
  decision gate pass.
- The frozen compiler-coverage gate passes at 61,133/63,639 lines
  (96.06216313895567%), 3,992/4,116 functions (96.98736637512148%), and
  89,215/94,642 regions (94.26575938800956%). The initial four-line floor gap
  was closed by pinning typed member-completion recovery when an escaped quote
  precedes the receiver inside a continued call. Every new test pins
  observable parsing, diagnostics, execution, recovery, or editor behavior.
  No synthetic coverage test, exclusion, or coverage-only production
  restructuring was added.

## Follow-Up

1. Commit newline continuation as its own Phase 3.5 decision.
2. Start the tuple ticket only after that commit, preserving the strict
   Phase 3.5 decision order.
