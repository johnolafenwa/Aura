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
| overflow | `a + b`, `-x`, `abs(x)`, `x as int32`, `f as int64`, `n as float64` | `AU4002` integer overflow or an inexact or out-of-range conversion |
| division by zero | `a // b`, `a % b`, `x / y` of floats | `AU4004` zero divisor |
| index in range | `xs[i]`, `xs[i] = v`, `xs.pop()` | `AU4003` index out of range |
| key present | `d[k]`, `s.remove(x)` | `AU4003` missing key, `AU4008` missing element |
| assert | `assert total >= 0` | `AU4001` assertion failed |
| requires | each call of a function with `requires` | `AU4009` at the call |
| ensures | each `return` of a function with `ensures` | `AU4009` on return |
| loop invariant | each loop `invariant` | `AU4009` in the loop |
| decreases | each `decreases` measure of a loop or a recursive function | `AU4009` in the loop or at the recursive call |
| class invariant | constructions and calls of public `mut self` methods | `AU4009` at the check |

`aura verify` also proves that an object of a class with invariants holds
them wherever it crosses a function boundary: at each call of a public
method that only reads `self`, for each argument passed to a function, and
for a function's result and `mut` parameters on return. Each function's
proof assumes the same on its side, so a public getter, or a function that
takes an `Account`, starts from an object that keeps its invariants. No run
checks these places, so while one of them is unproved
`aura build --remove-proved-checks` removes nothing.

A function is proved on its own. A call relies only on what its callee
declares: the verifier proves the callee's `requires` at the call, and then
assumes its `ensures`. A call through a trait, on a value whose type is a
type parameter, relies on the trait method's contract, which every
implementation is proved to keep. A call of a `lemma def` relies on the
lemma's `ensures` the same way; see
[Lemmas](/manual/contracts#lemmas). Nothing else about the callee's body is used, so
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
every iteration, which proves that the loop ends. On a function, it is
proved to be at least 0 on entry and smaller at every call the function
makes to itself, which proves that the recursion ends:

```aura
def gcd(a: int64, b: int64) -> int64:
    requires a >= 0 and b >= 0
    decreases b
    ensures result >= 0
    if b == 0:
        return a
    return gcd(b, a % b)
```

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
| `AU5104` | The check depends on something the verifier does not model yet, such as floating-point `**`. It is not proved, and it stays a runtime check. |

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
| `--no-cache` | Solve every check again, without the proof cache. |
| `--format json` | Print the report as JSON: one entry for each check, with its kind, function, line, column, message, and status (`proved`, `failed`, `unknown`, or `unsupported`), then the diagnostics. |

### Proof Cache

`aura verify` keeps each function's results in a proof cache, under
`verify` in `AURA_CACHE_DIR` when it is set, or else in
`~/.cache/aura/verify`. A function whose checks, the facts they rely on,
the solver version, and the time limit are all unchanged is answered from
the cache without starting the solver, so verifying again after a small
edit solves only what the edit touched. The summary line says how many
functions the cache answered. A check the solver gave up on is never
cached.


In VS Code, the Aura extension runs `aura verify` each time an Aura file
opens or is saved. Each check it cannot prove shows as a diagnostic at the
check, with the counterexample, and each line whose checks are all proved
ends with a `✓ proved` hint. The results show until the file is edited
again. Set `aura.verifyOnSave` to `false` to turn this off.

## What It Models

The verifier reasons about values exactly, as mathematical integers and
booleans, with each integer type's range as a check:

- integers of every width, `bool`, and `None`
- lists, with their lengths, and their elements when those are integers,
  `bool`, `str`, or tuples and classes of modeled values; the length of any
  other list
- `float32` and `float64` as IEEE-754 values, rounding each `+`, `-`, `*`,
  `/`, and `sqrt` to nearest, ties to even, with NaN, infinities, and signed
  zeros; comparisons, `abs`, `to_float`, and casts between floats and
  integers
- `str`, as a sequence of characters: literals, `+`, `==`, `<` and the other
  comparisons, `len`, `in`, `contains`, `starts_with`, and `ends_with`
- `dict` and `set` with integer or `str` keys: which keys they hold, their
  length, and a `dict`'s values when those are integers, `bool`, or `str`;
  literals, `d[k]`, `d[k] = v`, `in`, `len`, `is_empty`, `clear`, `keys`,
  `values`, and `items` of a `dict`, and `add`, `remove`, `discard`, and
  `clear` of a `set`, with `==` of whole collections
- tuples, classes, enums, and unions such as `int64 | None`, field by field
- `len`, `abs`, `min`, and `max` of integers, list `append`, `pop`, `clear`,
  element reads and writes, and `copy`
- `spec def` helpers, `old`, and `result` in contracts, and `ghost` locals
- task starts: `group.start(work, n)` proves `work`'s `requires` on the
  arguments the child receives, and the value of `Ready` from
  `task.result()`, `poll()`, or `result_or(...)` has `work`'s `ensures`.
  Each task's function is verified like any other
- calls through a function value that names one function, such as
  `g = inc` then `g(5)`: the call proves `inc`'s `requires` and assumes its
  `ensures`
- quantifiers: `forall(x in xs, condition)`, `exists(...)`, and
  `all([... for ...])` or `any(...)`, with any number of `for` clauses, each
  over a `range`, a modeled list, a `set`, the `keys()` of a `dict`, or
  `enumerate` or `zip` of modeled lists,
  with tuple targets and `if` filters, nested, in any contract, `assert`, or
  loop `invariant`

Anything else is a value the verifier knows nothing about: a `dict` or
`set` of other keys, the result of float `//`, `%`, and `**`, the result of most library calls, such as
`str.split` or `dict.get`, and what a call returns through a function value the verifier cannot
follow, such as one passed as an argument, stored in a collection, or
chosen on different paths. Such a value's `requires` is reported as
`AU5104` where it leaves the function, and stays checked when it is
called. A check that depends on one of them can
still be proved when the contract around it says enough.

A loop that builds a list proves what it builds with a quantified
invariant:

```aura
def squares(n: int64) -> list[int64]:
    requires 0 <= n and n <= 1000
    ensures len(result) == n
    ensures forall(i in range(n), result[i] == i * i)
    mut out: list[int64] = []
    mut i = 0
    while i < n:
        invariant 0 <= i and i <= n
        invariant len(out) == i
        invariant forall(j in range(i), out[j] == j * j)
        out.append(i * i)
        i += 1
    return out
```

A `for` loop over a `range(...)` call can say the same with less: its
invariants also hold when the range runs out, with the target at the end,
so `len(out) == i` gives `len(out) == n` after `for i in range(n)`.

A dictionary read proves its key is there from what the code and contracts
say about it:

```aura
def price(prices: dict[str, int64], item: str) -> int64:
    requires item in prices
    requires all([prices[k] >= 0 for k in prices.keys()])
    ensures result >= 0
    return prices[item]
```

A float contract is about IEEE-754 values, so `x == x` fails for NaN, and a
contract that needs `x / y` to be a number says so, as `requires y > 0.0`
does about the divisor. Proofs about float division and square roots can
take the solver longer than proofs about integers; one it cannot finish in
time is reported as `AU5102`.

A quantifier over a `str`, or over a list of other elements, is reported as
`AU5104`. See the
[verification design](https://github.com/johnolafenwa/Aura/blob/main/architecture_docs/23-verification-design-checkpoint.md).

## Trust

A check counts as proved only when the solver answers that no input breaks
it. `aura verify` itself removes nothing: every check stays in the program.

`aura build --remove-proved-checks` builds a native binary without the
checks that the verifier proves can never fail, and prints how many of each
kind it removed:

- the list bounds checks of element reads and element borrows
  (`view x = xs[i]`), when the index is proved to be in range and not
  negative, since an unchecked read does not count from the end of the list
- the overflow and zero-divisor checks of integer `+`, `-`, `*`, `//`, `%`,
  `/`, and negation; the native backend also tells LLVM that such a result
  does not wrap, which lets it optimize the arithmetic around it
- `assert`s, and the `ensures`, loop `invariant`, and `decreases` checks

A check goes only when it is proved on every path that reaches it. A
function's `requires` stays checked on entry, since a caller the verifier
does not see may break it. The flag runs the verifier first, so it needs a
solver, and it uses the proof cache. A `--debug` build keeps every check.

Removing a check moves the verifier and the solver into what the program
trusts: a wrong proof would turn a trap into an out-of-bounds read, a
wrapped integer, or a broken contract. So the
flag is off by default for now. It becomes the default for release builds
once differential testing of such builds against the interpreter covers it.
