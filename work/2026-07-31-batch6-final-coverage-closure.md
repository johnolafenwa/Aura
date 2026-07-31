# Batch 6 final coverage closure

## Goal

Pass the frozen `96.18/96.97/94.62` compiler coverage floors after the
0.2.0 release-preparation commit, using only tests that pin observable
language behavior, then obtain the exact totals for the single final
downward-truncated re-ratchet.

## Work completed

The clean full-access replay at `b6230af` passed 334 CLI tests, 1,498
compiler-library tests, and every compiler integration target. It covered
86,650 of 90,002 lines (96.275638%), 5,706 of 5,870 functions
(97.206133%), and 126,854 of 134,075 regions (94.614208%). The region total
was eight regions below the frozen floor; behavior, parity, and all tests
were otherwise green.

The existing lambda diagnostic regression now pins four previously
unrecorded outcomes:

- grouped and tuple type-shaped bodies followed by a second colon receive the
  dedicated contextual-type teaching diagnostic;
- a slice body followed by a second colon remains an ordinary statement
  syntax error rather than being mislabeled as a type annotation; and
- a literal body followed by a second colon receives that same ordinary
  syntax error.

These cases cover eight additional parser regions. The definitive clean
full-access replay at `b2fdfdc` confirms 126,862 of 134,075 regions
(94.62017527503264%), above the frozen floor.

## Verification

- focused instrumented parser regression: passed;
- definitive replay: 334/334 CLI tests, 1,498/1,498 compiler-library tests,
  and every compiler integration target passed;
- exact coverage: 86,655/90,002 lines (96.28119375124997%),
  5,706/5,870 functions (97.206132879046%), and 126,862/134,075 regions
  (94.62017527503264%);
- retained log SHA-256:
  `61bb557eba8aeadba5350594d7e63253af293d422411c915a7419942a99b5657`;
- retained JSON SHA-256:
  `9c199436f07f5c33f98f66fa0e3453bcd099b0ed840f691d7691308bac9e122d`;
- no test was added solely for line execution;
- no coverage exclusion was added; and
- an experimental slice-span assertion and a redundant mixed-tuple case were
  discarded because they did not add a distinct coverage or behavior claim.

## Follow-up

The single end-of-Batch-6 re-ratchet now truncates those exact percentages
downward to `96.28/97.20/94.62`. Commit that one floor update and run the
final clean full CI before tagging.
