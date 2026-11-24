# ADR-0150: A check that reads an unmodeled result is not a failure

Status: Accepted.

## Context

`aura verify` executes MIR symbolically. Where the encoding does not model
a step, such as `text.split(",")`, a slice, or `x & 255`, it gives the
result a fresh constant of the right type. The solver may give that
constant any value. A check that read it was reported as failed (`AU5101`)
with a counterexample, although the program can never produce the value
the solver chose. `len(text.split(",")) >= 1` "failed" with `text = ""`.

## Decision

The executor records every constant it makes up for an unmodeled step,
with the step's name and source line (`Exec::unmodeled`). When the solver
refutes a check, the verifier walks what the check reads: the constants in
its goal, the facts that mention them, and the definition of each defined
constant. A definition is followed from the constant to its operands and
not back. If the walk reaches a recorded constant, the check is reported
as not modeled (`Status::Unsupported`, `AU5104`) with the step named, and
no counterexample.

The path condition is left out of the walk. Paths that part on an
unmodeled result join again, and the joined path condition still names
it, so counting it would mark every later check in the function. The one
exception is a check that fails wherever its block is reached, such as
`assert n > 0` lowered to a branch: its branch condition is what it reads.

A loop the compiler writes, for a comprehension or a built-in such as
`sum`, has no place for an invariant. A temporary it carries across
iterations and that is read after the loop is recorded the same way.

A value the program really does not know is not recorded: a parameter, a
callee's result beyond its `ensures`, a variable a user loop writes, a
task's outcome, the type inside a received trait object.

## Consequences

- A not-modeled check is never proved and never removed by
  `--remove-proved-checks`.
- The walk is syntactic. A check that reads an unmodeled result and would
  fail for every value of it is reported as not modeled, not as failed.
  Both outcomes keep `aura verify` from exiting 0.
- A counterexample for a check on a path that parts on an unmodeled result
  may give inputs that do not take that path.
- Modeling a step later means no longer wrapping it in `Exec::unmodeled`.
- The proof cache key includes the recorded steps.
