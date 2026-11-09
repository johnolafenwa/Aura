# Evaluation of the work since 0.13.0-preview

Date: 2026-11-09. Asked for by John: check that what landed on `main`
after `v0.13.0-preview` (`14f77d4`) works and holds up, fix what does
not, and say what should come next.

## What was checked

| Change | Result |
| --- | --- |
| #245, #248 compact JSON steps 1 and 2 | Observed only; the JSON thread owns that code. #245 is in the last complete CI run on `main` (`e29035f`), which was green; the run for #248 (`0bc4fe1`) was still going when this was written. |
| #246 `dict.get` / `dict.remove` models | Hold. True claims prove; a false one gets the counterexample `k = "a"`. |
| #246 callable alias contracts on fields and results | Hold. A function that does not keep the alias's `ensures` is refused where it is stored or returned, a call through the field relies on the contract, and the runtime check agrees (`AU4009`). |
| #246 `self.step(n)` from a method | Works. |
| #246, #250 float rounding facts | Found a soundness bug for `float32`, fixed here (below). The float64 facts are sound. |
| #249 `IntoIterator` bounds on classes and impls | Hold in every shape tried. Sibling `self.x()` calls inside a generic bounded impl work. |
| #247 solver restart and CI flake fixes | Was never merged, and its own new test failed on Linux CI. Finished here (below). |
| Float test `contracts_read_float_and_str_values` | 0.1 s locally and 0.06 s in CI, against a 10 s limit. No longer near it. |

## Fixed here

### False `float32` proofs

The quick first pass for float `+ - * /` and `sqrt` (#246, #250) gives the
solver bounds computed from the float literals the function names. It
read the literals as float64 values, filtered them as float64 values
(finite, positive), and then computed and stated the bounds in the
operation's own type. A float64 literal such as `1e-300` is zero as a
float32 and `1e300` is infinite, so a `float32` operation got bounds of
`0 / 0`, `inf - inf` or `0 * inf`. Those are NaN, no value compares with
NaN, and the fact ruled out every value: anything proved.

```aura
def div_tiny(x: float32, y: float32, w: float64) -> float32:
    requires w >= 1e-300 and x >= 0.0 and y > 0.0
    ensures x > 0.0
    return x / y
```

`aura verify` reported this proved, `aura run` failed the `ensures` for
`x = 0`, and `aura build --remove-proved-checks` removed the check.

The fix (`verify_float.rs`, `values_in`): the literals are first turned
into values of the operation's type, the ones that are not finite there
are left out, and duplicates are merged. Tests:
`float32_bounds_leave_out_literals_that_round_to_zero_or_infinity` and
`float32_bounds_are_not_taken_from_float64_literals_that_round_away`.

### Solver restart (#247)

#247 dropped a solver after an `unknown` answer and retried once when a
solver died mid-query. Two things were wrong with it:

- The retry happened only when the dead process was seen to have exited
  within half a second of its pipe closing. On the Linux runner it was
  not, and the run failed with `the SMT solver stopped: Broken pipe`.
  Now a failed write or an end of output marks the solver stopped, and
  that alone decides the retry. A solver found dead between queries
  (closing a scope, sending the next fact) is replaced as well.
- Its test passed without testing anything: the version probe
  (`z3 --version`) ran the fake solver first and used up its one death.
  The fake solver now answers the probe without dying.

Stress testing also showed a flake that is on `main` today: a fake solver
script written by the test process can be held open for writing by a
process another test forks at that moment, and running it fails with
`Text file busy`. The scripts are now written by a child process.
With all three changes, 400 runs of the solver tests under load pass.

## Gaps left for later

- `list[T]` does not implement `IntoIterator[T]`, so a function bounded
  by `S: IntoIterator[T]` does not take a list.
- Enum type parameters and supertraits cannot use a bound that names a
  trait with associated types (`AU2002`); a trait's own type parameters
  cannot carry bounds at all (parse error).
- A false float64 claim that needs exact rounding, such as
  `a / 3.0 * 3.0 == a`, ends as `unknown` after the full time limit
  (about 24 s for two checks). Correct, but slow.
- Other tests that write a script and run it (`crates/aura/tests/cli.rs`,
  `runtime_value_tests.rs`) can hit the same `Text file busy` race.
