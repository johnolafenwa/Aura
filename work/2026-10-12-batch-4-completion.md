# Batch 4 Completion

## Goal

Finish every family in the
[Batch 4 design checkpoint](../architecture_docs/18-batch-4-design-checkpoint.md),
merge each one after a green hosted run, and leave `main` ready for a
release. Version bumps and tags are out of scope.

## Work Completed

| Family | Pull request | Work note |
| --- | --- | --- |
| Trailing commas and parenthesized imports | #33 | `2026-10-11-batch-4-trailing-commas.md` |
| Syntax-reflowing formatter | #34 | `2026-10-11-batch-4-formatter.md` |
| Text and byte conveniences | #36 | `2026-10-11-batch-4-text.md` |
| Display and read-only properties | #38 | `2026-10-11-batch-4-display.md` |
| Unpacking, sequence patterns, and class patterns | #39 | `2026-10-11-batch-4-patterns.md` |
| Docstrings and generated API docs | #40 | `2026-10-11-batch-4-docs.md` |
| Ownership fixes found during the work | #41 | `2026-10-11-batch-4-move-fixes.md` |

Each pull request merged main before its final hosted run. Conflicts
between families were resolved by keeping both sides. The docstring and
Display branches also gained getter docstrings in property hover and
completion once both were present.

This note closes the batch: the task board entry, the ADR-0056 status, and
the roadmap row now record the implemented state.

## Verification

- Every pull request above passed hosted CI on Ubuntu and macOS, which
  runs formatting, clippy, tests, the backend parity matrix, the compiler
  coverage floors, LSP coverage, documentation examples, and the LLM
  documentation check.
- After the last merge, the structural patterns branch was verified
  locally against the Display and docstring work: compiler library tests,
  fixtures, the CLI example, formatter, and documentation tests, all 773
  documentation examples, and clippy.

## Follow-Up

Defects found during the work and not fixed in this batch:

- A view local forwarded from a method's returned view fails at run time
  with `AU4001` "uses ended loan".
- A union over a module-alias type, such as `geometry.Point | None`, fails
  at run time with `AU4001` "union tag test type identity mismatch".
- Duplicate or-alternatives of refutable class patterns over a union are
  not reported.
- `x.m() as float64` reports "result type mismatch".
- A comprehension over `obj.items()` fails with "cannot move borrowed
  value" or "unknown name".
- `dict.set` type-checks, but MIR lowering reports "unsupported method
  call".
- About 100 lines of unreachable checker code remain. Removing them needs
  an owner decision.
- The MIR validator may not check enum payload arity.
- `native_runtime_direct_cancellation_releases_frame_owned_argument_references`
  may be flaky.
