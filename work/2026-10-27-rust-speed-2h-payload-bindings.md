# Rust Speed Step 2h Part 4: Payload Bindings Hold The Variant's Box

## Goal

Step 2h of Phase 2 of the Rust-speed plan moves ownership off boxes. Its
gate: leak, failure, and cancellation cleanup tests pass, and retains and
releases per program drop at least 90%. After parts 1 to 3
(`2026-10-27-rust-speed-2h-shared-arguments.md`,
`2026-10-27-rust-speed-2h-mutable-arguments.md`,
`2026-10-27-rust-speed-2h-collection-writes.md`), enum payload bindings
were most of what was left: 212,087 of the 326,693 retains across the 30
programs were in `interpreter`, and all 8,632 in `binary_trees`.

## The Old Convention

A variant keeps a payload that is not a scalar in a box of its own (step
2f). A pattern read each payload into a new temporary and, for a binding,
copied the temporary into the binding's local, so the ownership pass
retained the payload once for the copy. Native code also retained the
payload's box when it read it out of the variant, so the binding owned a
reference that it released at the end of the arm. A payload ignored with
`_` was read into a temporary and released at once.

## Changes

- Lowering (`mir.rs`): a payload whose subpattern only binds a name is
  read straight into the binding's local (`pattern_binding_target`), and
  a payload whose subpattern is `_` is not read. Writeback patterns
  (`match mut`) and typed payload views keep the old lowering.
- Native code (`native_codegen_elements.rs`, `lent_payload_reads`): a
  local assigned once from a payload read of a root local, and otherwise
  only read, holds the box the scrutinee's variant holds instead of a
  reference of its own, with the lent flag a lent list element already
  uses, so its release releases nothing. A read counts when it reads the
  binding or its parts where they lie, calls a method that reads it, or
  passes it to a parameter the callee borrows (part 1's kept parameters,
  through `callee_borrowed_operands`). The MIR keeps the scrutinee live
  while the payload is (the ownership pass reads a payload as a borrow of
  its scrutinee), and the language refuses a change to the scrutinee
  while a shared binding is still to be read. Before an instruction that
  does more than read the scrutinee, or that ends the loan of the element
  or payload the scrutinee itself holds, a lent payload that is still to
  be read takes a reference to its box. A scrutinee a closure, a task, or
  a mutable loan can reach is never lent from.
- `typed_variant_payload` (`native_codegen_enums.rs`) returns the slot's
  box with flag 1 on the fast path and the slow read's own box with flag
  0. A local of a Copy enum, which the MIR never releases, may hold a lent
  payload too: `release_boxed_copy_roots` releases it by its flag.

## Problem Found

`aura check` overflowed the stack on a match over a recursive enum where
an arm nests a variant pattern in a payload and a later arm does not:

```aura
match expr:
    case Expr.Binary(Op.Add, right):
        ...
    case Expr.Binary(_, right):
        ...
```

The unreachable-arm check (`pattern_rows_cover_pattern_union`) split a
wildcard of `Expr` into every variant even when no earlier arm told the
variants apart, and `Binary`'s `Expr` payload split again without end.
When no row has a variant pattern in the column, the check now keeps the
rows that match anything there and moves to the next column. A sema test
covers the reachable arm and an arm two nested arms cover.

A list write inside a `match items[i]:` arm, such as `items[1] = x`,
passes the checker and then fails MIR loan validation with AU4001
"invalid MIR loan flow". It is left for a later change.

## Verification

- Analysis tests: bindings read, looked up, and passed to a borrowing
  callee are lent from a parameter; a payload of a lent loop element is
  lent from it; a release of the scrutinee while the binding is live
  makes it take a reference; a binding copied whole is not lent; a
  scrutinee a task reaches is never lent from.
- New fixture `run-pass/payload_bindings_lent.au`: an expression tree
  evaluated through shared parameters, `str` payloads passed to methods
  and f-strings, nested and ignored payloads, a loop over a list of
  variants, a binding copied into a list, and a `mut` parameter changed
  after its binding's last read. Its native build
  makes 7 retains where the part 3 compiler made 2,438.
- `representation_stats`: ten evaluations of an expression tree read 50
  payloads and take no reference.
- `batch7_ownership_vocabulary`'s program no longer retained anything,
  since its only retains were payload copies; it now calls a function
  value, which still takes a reference.
- Compiler tests, backend parity, the CLI tests, `check:cross-language`,
  `check:operation-probes`, `cargo fmt --check`, hygiene, and clippy.

## Measurements

Callgrind instruction counts and `AURA_RUNTIME_STATS=1` retains, Linux
container, release builds, the 30 programs at the check size. Before is
part 3.

| Program | Retains before | Retains after | Instructions before | Instructions after |
| --- | ---: | ---: | ---: | ---: |
| `interpreter` | 212,087 | 23,005 | 97.6M | 86.8M |
| `binary_trees` | 8,632 | 0 | 4.6M | 4.2M |
| `bank_ledger` | 3,575 | 1,156 | 13.5M | 13.2M |
| `json_roundtrip` | 46,640 | 28,086 | 574.9M | 574.2M |
| `sql_engine` | 26,017 | 13,422 | 1,412.9M | 1,412.2M |
| `docdb` | 9,707 | 6,104 | 66.2M | 65.9M |
| `spreadsheet` | 2,236 | 0 | 22.3M | 22.2M |
| All 30 programs | 326,693 | 87,876 | | |

The suite showed `pubsub_broker` 12.1% lower, but direct callgrind runs
of both builds give 90.45M and 90.39M: its tasks vary between runs. No
program runs more than 0.1% more instructions.

## Against The Gate

Retains across the 30 programs are 91.7% below 2g (1,062,951 to
87,876). Per program, 16 programs are at least 90% down or retain
nothing. What is left is mostly `str`: `json_roundtrip` retains each
object key twice while it walks `fields.keys()` and looks the key up,
`docdb`, `csv_aggregate`, and `word_freq` retain `str` values they read
out of collections, and `lru_cache` retains boxed `int64` values. Unit
results and the scalar math builtins still allocate boxes, which the
census counts as last releases rather than retains.
