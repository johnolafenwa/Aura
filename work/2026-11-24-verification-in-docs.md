# Verification In The Documentation

Date: 2026-11-24

## Goal

John asked that the verification feature be very prominent in the
documentation and the book, as a core part of the language.

## Work Completed

- README, `docs/index.md`, `docs/positioning.md`: the opening pitch names
  contracts and `aura verify`, and the first example is a verified function
  with its report and one counterexample.
- Install guides and book section 1.1: a step that installs an SMT solver.
- Book: section 1.3 proves a first contract, 3.3 introduces `requires` and
  `ensures`, chapters 3 to 20 carry contracts on their examples with real
  `aura verify` reports, and chapter 11 is "Testing and proving" with two
  new sections, 11.5 and 11.6. Chapter 21 says that tensors are not modeled.
- Manual: a Verification section on the pages for functions, classes,
  traits, collections, enums, statements, assertions, types, concurrency,
  closures, and numeric arrays, and a "What It Does Not Model" table in
  `docs/manual/verification.md`.
- A survey of verifier gaps, run with probe programs on `main` at 0.17.0.
  The gaps are fixed in separate work.

## Verification

- `python3 scripts/check_doc_examples.py`: all 1,200 examples pass.
- Every `aura verify` summary line in the book and the Manual comes from a
  run. A script re-ran the 72 summaries that follow a complete code block.
  59 matched the block as shown, and the other 13 describe an edit to the
  block that the text states.
- `python3 scripts/generate_llms.py --check`, `npm run test:docs-site`, and
  `npm run docs:build` pass.

## Follow-Up

- A docs-only pull request does not run `check_doc_examples.py` in CI, and
  nothing in CI runs `aura verify` on documentation examples. A marker on a
  code fence that the script checks with `aura verify` would keep the proofs
  in the book from going stale.
- An `examples/verification/` directory of verified programs.
- Update the limits in the Manual and the book as verifier gaps close.
