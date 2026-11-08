# Verification

`aura verify` proves, before a program runs, that its checks cannot fail.
It reads the same [contracts](/manual/contracts) that `aura run` checks at
run time, and it also proves that no integer overflows, no divisor is zero,
and no list index is out of range. Each check it cannot prove is reported
with the inputs that break it.

```bash
aura verify bank.au
```

`aura verify` needs an SMT solver: [Z3](https://github.com/Z3Prover/z3) or
[cvc5](https://cvc5.github.io/). It runs the solver as a separate program, so
nothing is added to the compiler or to built programs. Install one with your
package manager (`brew install z3`, `apt install z3`, or
`pip install z3-solver`), or with `scripts/install-z3.sh` from the Aura
repository, which installs the version Aura's own tests use.

## What It Proves

`aura verify` proves each of these checks on every path through each
function:

| Check | Example | Runtime failure it rules out |
| --- | --- | --- |
| overflow | `a + b`, `-x`, `abs(x)`, `x as int32` | `AU4002` integer overflow |
| division by zero | `a // b`, `a % b` | `AU4004` zero divisor |
| index in range | `xs[i]`, `xs[i] = v`, `xs.pop()` | `AU4003` index out of range |
| assert | `assert total >= 0` | `AU4001` assertion failed |
| requires | each call of a function with `requires` | `AU4009` at the call |
| ensures | each `return` of a function with `ensures` | `AU4009` on return |
| loop invariant | each loop `invariant` | `AU4009` in the loop |
| decreases | each `decreases` measure | `AU4009` in the loop |
| class invariant | constructions and calls of public `mut self` methods | `AU4009` at the check |

A function is proved on its own. A call relies only on what its callee
declares: the verifier proves the callee's `requires` at the call, and then
assumes its `ensures`. Nothing else about the callee's body is used, so
what a caller needs to know must be in the contract.

```aura
def clamp(x: int64, lo: int64, hi: int64) -> int64:
    requires lo <= hi
    ensures lo <= result and result <= hi
    if x < lo:
        return lo
    if x > hi:
        return hi
    return x


def percent(x: int64) -> int64:
    p = clamp(x, 0, 100)
    return p * 1000  # proved: p is at most 100


def main():
    print(percent(250))  # 100000
```

Inside a function, its own `requires` clauses are assumed, since every call
proves them.

## Loops

A loop is proved one iteration at a time. On entering the loop, the verifier
forgets the value of every local the loop body assigns, and keeps only what
the loop's `invariant` clauses say about them. It proves each invariant on
entry and after every iteration, and the code after the loop learns only
the invariants and the condition that ended the loop.

```aura
def sum_to(n: int64) -> int64:
    requires 0 <= n and n <= 1000
    mut i = 0
    mut total = 0
    while i < n:
        invariant 0 <= i and i <= n
        invariant 0 <= total and total <= i * 1000
        total += i
        i += 1
    return total


def main():
    print(sum_to(10))  # 45
```

Without the second invariant, `total += i` cannot be proved: nothing would
bound `total` in a later iteration. Two facts need no invariant: a local the
loop never assigns keeps its value, and a counter the loop only steps up
(`i += 1`) or only steps down stays on its side of its entry value. That
covers indexes such as `for i in range(len(xs))` and `while i < len(xs)`.

A `decreases` measure is proved to stay at least 0 and to get smaller in
every iteration, which proves that the loop ends.

## Reports

`aura verify` prints a diagnostic for each check it could not prove, then a
summary line. It exits with status 0 only when every check was proved.

```text
error[AU5101]: cannot prove integer result in range
 --> scale.au:2:12
  |
2 |     return x * 2
  |            ^
  = note: in `double` (overflow check)
  = note: counterexample: x = 4611686018427387904
verified scale.au: 0 of 1 checks proved in 2 functions (/usr/bin/z3 (Z3 version 5.3.3 - 64 bit))
```

| Code | Meaning |
| --- | --- |
| `AU5101` | The check can fail. The counterexample gives parameter values that make it fail, as far as the solver reports them. |
| `AU5102` | The solver gave up before deciding. Raise `--timeout`, or add a `requires`, `invariant`, or `assert` that narrows the case. |
| `AU5103` | No solver could be started. |
| `AU5104` | The check depends on something the verifier does not model yet, such as floating-point arithmetic. It is not proved, and it stays a runtime check. |

A check that fails to prove is not always a bug: the verifier may lack a
fact that holds. Add it as a `requires` on the function, an `invariant` on
the loop, or an `ensures` on the callee, and the verifier proves the fact
where it is stated and uses it where it is needed.

### Options

| Option | Meaning |
| --- | --- |
| `--solver <path>` | The solver to run. The default is `AURA_SMT_SOLVER` when it is set, else `z3`, else `cvc5`, found on `PATH`. |
| `--timeout <ms>` | The time limit for each check. The default is 10000. |
| `--function <name>` | Verify only the functions whose name contains `name`. |
| `--format json` | Print the report as JSON: one entry for each check, with its kind, function, line, column, message, and status (`proved`, `failed`, `unknown`, or `unsupported`), then the diagnostics. |

## What It Models

The verifier reasons about values exactly, as mathematical integers and
booleans, with each integer type's range as a check:

- integers of every width, `bool`, and `None`
- lists of integers and of `bool`, with their lengths and elements, and the
  length of any other list
- tuples, classes, enums, and unions such as `int64 | None`, field by field
- `len`, `abs`, `min`, and `max` of integers, list `append`, `pop`, `clear`,
  element reads and writes, and `copy`
- `spec def` helpers, `old`, and `result` in contracts

Anything else is a value the verifier knows nothing about: a `str`, a
`dict`, a `set`, a float, the result of most library calls, and what a call
through a function value returns. A check that depends on one of them can
still be proved when the contract around it says enough.

Quantifiers (`forall`, `exists`) in contracts, and `str`, `dict`, and `set`
models, are planned. See the
[verification design](https://github.com/johnolafenwa/Aura/blob/main/architecture_docs/23-verification-design-checkpoint.md).

## Trust

A check counts as proved only when the solver answers that no input breaks
it. Every check stays in the program at run time: `aura verify` adds proof,
and does not yet remove checks.
