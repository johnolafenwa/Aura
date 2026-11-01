# Verification design checkpoint: contracts and static proof

Status: **Ratified 2026-10-23 by the owner.**

Date: 2026-10-23. Source baseline: Batch 7 step 8h part 1 (main @ 2914ecf).

This checkpoint plans software verification for Aura: contracts that state
what a function requires and guarantees, runtime checks of those contracts,
and static proof that a program meets them and never traps. It is a new
roadmap item, [Batch V](14-priority-roadmap.md#priority-batches). No code
changes ship with this checkpoint.

Each section states what the compiler does today, the options, and the
chosen default. The [Ratification Record](#ratification-record) states what
the owner decided.

## Starting Point

Several rules Aura already enforces remove the problems that make
verification hard in other languages.

- **No aliased mutation.** [Call-boundary
  exclusivity](../docs/manual/ownership-and-borrowing.md#call-boundary-exclusivity)
  and loan checking mean a `mut` borrow never overlaps another live access to
  the same place. A verifier can model a `mut` parameter as two values, the
  one on entry and the one on exit, with no heap model and no separation
  logic. Prusti, Creusot and Verus rely on the same property of Rust.
- **Value semantics for collections.** Lists, dictionaries and sets are
  copy-on-write values since Batch 7a. Their logical model is a mathematical
  sequence, map or set, not a graph of objects.
- **Every trap is named in MIR.** Since Batch 7 step 8f
  (`crates/aura-compiler/src/mir_checks.rs`), every numeric `Binary` and
  `Unary` names its `ArithmeticTrap`, and every list read is either checked
  or covered by an `IndexInRange` fact. `validate_checks` refuses an
  unchecked access that no fact covers. Each named trap is a ready-made proof
  obligation: "this trap cannot fire".
- **Fully typed MIR.** Step 8c made every assigned local carry a concrete
  type, and the MIR validator enforces it. A verifier reads MIR without
  re-running inference.
- **No `unsafe`, raw pointers, or null.** Absence is `Optional`, failure is
  `Result`, and both are typed.
- **A reference interpreter.** `aura run` executes MIR directly and stays the
  parity reference for native code.
- **`assert` exists** with exact evaluate-once semantics
  ([Assertions](../docs/manual/assertions.md)). It does not narrow or refine
  types, which is the gap contracts fill.

The hard parts are the same as in other verifiers: tasks and concurrency,
closures, FFI and I/O, floating point, and logical models for `str`, `dict`
and `set`.

One Aura-specific detail matters from the start: list indexing accepts
negative indices, read as `len() + index`. The in-range obligation for
`xs[i]` is therefore `-len(xs) <= i and i < len(xs)`, not the Rust
`0 <= i < len`. The existing `IndexInRange` fact proves the narrower
`0 <= index < len`, and stays valid.

## A. Levels

| Level | What it gives | Reference systems | Solver |
| --- | --- | --- | --- |
| V1 Runtime contracts | `requires`, `ensures` and invariants checked when the program runs. Contracts are documentation that cannot go stale, and oracles for tests and fuzzing. | Eiffel, Ada 2012, D | No |
| V2 No runtime errors | Static proof that no overflow, out-of-range index, division by zero, `Optional` unwrap failure or contract violation can happen. Proved checks are removed. | SPARK (silver) | Yes |
| V3 Functional correctness | Proof that every `ensures` holds: loop invariants, termination, pure specification functions, ghost code, quantifiers. | SPARK (gold), Dafny, Verus, Creusot | Yes |
| V4 Wider coverage | Models of `str`, `dict` and `set`; contracts on `Callable` types; task bodies; trusted specifications for FFI and I/O. | Verus, Prusti | Yes |

Refinement types (`int64 where self >= 0`, as in Liquid Haskell or F*) are
not a separate type system. If they arrive, they are sugar that desugars to
the same contracts, so there is one checker and one set of diagnostics.

Bounded checking complements every level: `aura test` can run generated
inputs with contracts as the oracle, in the style of QuickCheck or Kani. It
finds counterexamples without proving anything, and needs no solver.

## B. Surface Syntax

### Options

- **B1. Clause statements (chosen).** `requires`, `ensures`, `invariant` and
  `decreases` lines at the top of a function or loop body, in the position a
  docstring takes.
- **B2. Decorators.** `@requires(...)` above `def`. Rejected: a decorator is
  an expression evaluated at module initialization that replaces the
  function value ([Decorators](../docs/manual/functions.md#decorators)). A
  contract must be static, part of the signature, and visible to callers
  that never run the decorator.
- **B3. Signature clauses.** Clauses between the parameter list and the
  colon, as in Dafny. Rejected: long signatures break Aura's one-line `def`
  header, and Python-shaped formatting has no place for them.

### Shape

```aura fragment
def binary_search(xs: list[int64], key: int64) -> int64:
    """Index of `key` in sorted `xs`, or -1."""
    requires is_sorted(xs)
    ensures result == -1 or (0 <= result and result < len(xs) and xs[result] == key)
    mut lo = 0
    mut hi = len(xs)
    while lo < hi:
        invariant 0 <= lo and lo <= hi and hi <= len(xs)
        decreases hi - lo
        mid = lo + (hi - lo) / 2
        if xs[mid] < key:
            lo = mid + 1
        elif xs[mid] > key:
            hi = mid
        else:
            return mid
    return -1

spec def is_sorted(xs: list[int64]) -> bool:
    return forall(i in 0..len(xs) - 1, xs[i] <= xs[i + 1])
```

Rules:

- Clauses come after the docstring and before the first other statement. A
  clause anywhere else is a syntax error.
- `requires`, `ensures`, `invariant`, `decreases`, `spec` and `ghost` are
  contextual keywords. They are keywords only in clause position or before
  `def`, so existing programs that use these names as identifiers keep
  compiling.
- Inside `ensures`, `result` names the return value and `old(expr)` names the
  value of `expr` on entry. `old` may only read a parameter or a path from
  one. Outside `ensures` both are ordinary names.
- A class may declare `invariant` clauses in its body. The invariant holds
  after construction, and is checked on entry and exit of every public method
  that takes `mut self`.
- `forall(binding in range, condition)` and `exists(...)` range over integer
  ranges and over collection elements. They are legal only in spec
  expressions.
- `ghost` locals and `spec def` functions exist only for specifications. They
  are erased before ownership insertion and code generation.

### Specification expressions

A specification expression is pure: it may read values, call `spec def`
functions and pure built-ins (`len`, comparisons, arithmetic, element reads),
and must have exact type `bool` (or an integer for `decreases`). It may not
move, mutate, allocate a resource, start a task, or call a non-`spec`
function. Sema checks this in a pure mode, so a spec never changes ownership
state. Arithmetic in specs is checked like ordinary arithmetic at runtime;
the verifier reasons about it as mathematical integers with the same trap
obligations.

## C. Compiler Architecture

1. **Parser and sema.** Clause statements become fields of the function,
   loop and class declarations, not body statements. Contracts are part of a
   function's signature, so a caller can rely on a callee's `ensures` without
   reading its body (modular verification). Overriding methods and trait
   implementations may weaken `requires` and strengthen `ensures`, never the
   reverse; V1 starts by requiring them to repeat or omit the trait's
   contract.
2. **MIR.** Two new instructions:
   - `Assume { cond }`: a fact the verifier may use; a no-op at runtime.
   - `Obligation { cond, kind, span }`: a condition that must hold, with
     `kind` one of `Requires`, `Ensures`, `LoopInvariant`, `ClassInvariant`,
     `Decreases`.

   `requires` lowers to an `Obligation` at every call site and an `Assume` at
   entry; `ensures` lowers to an `Obligation` before every return and an
   `Assume` after every call. `old(e)` becomes a ghost snapshot local taken
   at entry. Ghost locals and `spec` calls are marked so the erasure step
   can remove them.
3. **Runtime checks (V1).** At the native boundary, each `Obligation` lowers
   to a check that traps with a new runtime code, and `Assume` disappears.
   The interpreter evaluates each `Obligation` the same way. Checks run in
   every build mode by default.
4. **Verifier (V2 onward).** `aura verify` runs on MIR after `resolve_checks`
   and before `insert_ownership`, because retain and release carry no logical
   meaning. It computes weakest preconditions over the control-flow graph,
   cutting loops at their `invariant` clauses, and gathers obligations from
   three sources: `Obligation` instructions, the `ArithmeticTrap` on every
   `Binary` and `Unary`, and every checked list read. It writes SMT-LIB text
   to an external solver and reports each obligation as proved, unproved with
   a counterexample, or timed out. The CLI prints the report and the language
   server shows it inline.
5. **Proofs feed `mir_checks`.** A proved obligation becomes a recorded fact,
   generalising `IndexInRange` to a fact that names the obligation it proves.
   `validate_checks` keeps its rule that nothing is unchecked unless a fact
   covers it. Under LLVM, proved ranges also become `llvm.assume`, range
   metadata and `nsw`/`nuw` flags, which help loop vectorization.
6. **Erasure.** After verification, ghost locals, `spec` calls, and
   `Assume` instructions are removed. Proved `Obligation` instructions become
   facts. Unproved ones stay as runtime checks.

### Logical model

| Aura | SMT model |
| --- | --- |
| `int32`, `int64`, `uint64` | Mathematical integers with range obligations from each trap. Wrapping and saturating methods use bit-vectors. |
| `bool` | `Bool` |
| `float32`, `float64` | Uninterpreted in V2; IEEE floating point (`FloatingPoint`) is a V4 option |
| `list[T]` | Sequence of `T` with length |
| Classes, tuples, records | Datatypes, one constructor |
| Enums, unions, `Optional`, `Result` | Algebraic datatypes |
| `mut` parameter | A pair of values: on entry and on exit |
| `str`, `dict`, `set` | Opaque in V2; sequence, map and set models in V4 |
| Resources, tasks, FFI, `Callable` | Opaque; callers see only declared contracts |

## D. Solver

### Options

- **D1. External SMT solver through SMT-LIB text (chosen).** `aura verify`
  starts Z3 or cvc5 as a subprocess and talks SMT-LIB 2. The solver is found
  on `PATH` or through `AURA_SMT_SOLVER`. Nothing is linked into the
  compiler or into compiled programs, so binary size and `aura build` are
  unchanged.
- **D2. Link Z3 into the compiler.** Rejected: a large C++ build dependency
  and a larger toolchain for a feature many builds never use.
- **D3. Translate to Boogie, Why3 or Viper.** Rejected: a second toolchain
  and a translation layer to keep in step with MIR. Aura's value semantics
  and exclusivity mean the safe subset needs no heap model, so a direct
  encoding is small. Verus takes a similar direct route.

CI installs one pinned solver version and runs the verifier tests against it.
Solver output is cached per obligation, keyed on the obligation text and the
solver version, so repeated runs are fast and reproducible.

## E. Trust and Check Removal

Once proofs remove checks, the verifier and the solver join the trusted base.
The policy:

- Debug builds keep every check.
- Release builds remove a check only when the obligation was proved in the
  same build. This is on by default once V2 has fuzz parity coverage.
- `aura build --keep-proved-checks` turns proof-based removal off.
- Parity fuzzing compares the interpreter, which checks everything, against
  native release builds.

This extends ratified Batch 7 rule C1 ("keep every bounds check observable;
remove only proven ones"). It widens where proofs come from without changing
what a program observes: a removed check is one that cannot fire.

## F. Delivery Plan

| Stage | Scope | Size | Depends on |
| --- | --- | --- | --- |
| V0 | This checkpoint | 1 docs PR | Nothing |
| V1 | Clause syntax, spec sublanguage in sema, `Obligation`/`Assume` in MIR, runtime checks in interpreter and native, `spec def`, `old`, `result`, loop and class invariants, manual page and diagnostics | 3 to 4 PRs | After the Batch 7 Phase 2 release, so it does not collide with LLVM work in `mir.rs` |
| V2 | `aura verify`, SMT-LIB encoding for integers, `bool`, records, enums, `Optional`, `Result` and lists of scalars; trap, index and contract obligations; proved facts feed `mir_checks` and LLVM hints; LSP report | 4 to 6 PRs | V1 and the LLVM backend |
| V3 | Required loop invariants for static proof, `decreases`, ghost code, lemmas, quantifiers over lists, `mut` frame conditions, trait-level contracts for generics | 6 to 10 PRs | V2 |
| V4 | Models of `str`, `dict`, `set`; contracts on `Callable` types; task bodies verified sequentially under structured concurrency; trusted specifications for FFI and I/O | Open-ended | V3 |

V1 progress: V1a (`requires` and `ensures` on functions and methods,
`result`, the pure condition check, runtime checks on both backends,
`AU2048` and `AU4009`) is implemented. V1b adds loop `invariant` clauses and
a runtime `decreases` check on `while` (measure at least 0 and strictly
smaller at each body entry), and class invariants: the parser turns a
class's `invariant` lines into a hidden `__invariant__(self)` method that
construction sites, `__init__` callers, and public `mut self` methods (on
entry and exit) call, so the check always runs in the class's module.
Condition purity now also requires a method call's receiver to have a
builtin type. V1c adds `old(expr)` (the parser replaces each call with a
hidden name; the function copies the value after its entry checks),
`forall`/`exists` (rewritten to `all`/`any` over a list comprehension), and
module-level `spec def` helpers (one pure `return`, no `mut` parameters,
usable only in clauses and other spec bodies). V1d moves the material into
its own manual page.

V1 needs each stage's full local gates, coverage floors, and interpreter and
native parity, like every other batch. V1 also adds a fixture family of
contract programs that V2 then proves.

## Diagnostics

| Code | When |
| --- | --- |
| New `AU20xx` codes | A clause out of position, an impure or non-`bool` spec expression, `old` or `result` outside `ensures`, a ghost value reaching runtime code |
| New `AU40xx` runtime code | A contract check failed at runtime. The message names the clause kind and its source span, and reports operand values like a failed `assert`. |
| New `AU5xxx` family | Verifier results: unproved obligation (with counterexample), solver timeout, solver not found |

Exact numbers are assigned in V1 and V2.

## Owner Questions

1. **Contract syntax.** Clause statements at the top of a body (B1), not
   decorators (B2) or signature clauses (B3).
2. **Solver.** An external solver through SMT-LIB, used only by
   `aura verify` (D1).
3. **Check removal.** Release builds remove proved checks by default once
   parity fuzzing covers V2, with a flag to keep them.
4. **Timing.** This checkpoint now; V1 after the Batch 7 Phase 2 release.

## Ratification Record

The owner decided on 2026-10-23 to take every recommendation:

1. **Contract syntax.** Clause statements (B1).
2. **Solver.** External Z3 or cvc5 through SMT-LIB, only in `aura verify`
   (D1).
3. **Check removal.** On by default in release builds once V2 has fuzz
   parity coverage; `--keep-proved-checks` turns it off.
4. **Timing.** V0 now; V1 after the Batch 7 Phase 2 release; V2 and V3
   follow as Batch V.
