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
function. The statements a module writes outside every function are
verified as one more function, which reports show as `<module>`:

| Check | Example | Runtime failure it rules out |
| --- | --- | --- |
| overflow | `a + b`, `-x`, `abs(x)`, `x as int32`, `f as int64`, `n as float64` | `AU4002` integer overflow or an inexact or out-of-range conversion |
| overflow | `a << n`, `a >> n`, `a ** n`, `math.floor(x)`, `math.isqrt(n)` | `AU4002` a shift count outside the width, a left shift that loses bits, or a result outside the type; `AU4001` a negative exponent or an argument outside a `math` function's domain |
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

The same holds for each such object inside a value that crosses a boundary:

- a field of another object, a tuple item, a member of a union such as
  `Account | None`, an enum payload such as the `Ok` of a `Result`, and an
  element of a list whose elements are modeled
- a function that takes `xs: list[Account]` starts from elements that keep
  their invariants, and a caller proves it of the list it passes
- a field write that breaks an object inside a value is reported where the
  value leaves the function: at the call it is passed to, or on return
- a list built in a loop needs a loop invariant about its elements before
  it is passed on, such as `invariant forall(a in ys, a.balance >= 0)`
- a part the verifier does not model is reported as `AU5104` at the
  boundary: the values of a `dict`, the elements a comprehension builds,
  rows of a list of lists, and an object deeper than the encoding follows

A method of an `impl Trait for Class:` block cannot be `public`, so no run
checks the class invariants around it. The verifier treats it like a public
method anyway. Its proof assumes the invariants on entry, and a `mut self`
method is proved to hold them again on return. In exchange, each call that
names the method proves the invariants of its receiver, and so does each
conversion of an object to a trait object:

```aura
trait Shape:
    def area(self) -> int64:
        ensures result >= 0


class Square:
    side: int64
    invariant 0 <= self.side and self.side <= 1000


impl Shape for Square:
    def area(self) -> int64:
        return self.side * self.side  # proved: `side` is at most 1000


def main():
    print(Square(side=2).area())  # 4
```

The method assumes nothing when the program reaches it in a way that proves
nothing about the receiver. Those ways are a call on a union value, a call
in a generic function that the verifier could not copy for its type
arguments, and `Display.display`, which printing calls. When the method
belongs to an imported module, the function that reaches it this way gets an
unsupported class invariant obligation instead.

A function is proved on its own. A call relies only on what its callee
declares: the verifier proves the callee's `requires` at the call, and then
assumes its `ensures`. A call through a trait, on a value whose type is a
type parameter or a trait object, relies on the trait method's contract,
which every implementation is proved to keep. A trait object the function
itself made from a value of a known type is followed more closely: a call on
it relies on that type's own contract, and a `match` type arm on it is
decided. A trait object the function receives, reads from a collection, or
gets from two paths that hold different types may hold any type. A call of a `lemma def` relies on the
lemma's `ensures` the same way; see
[Lemmas](/manual/contracts#lemmas). One more fact about the callee's body
is used, described next. Anything else a caller needs to know must be in
the contract.

A call changes only what its callee can write. After a call, a `mut`
argument keeps the value of every field the callee never writes, so the
`ensures` only has to describe the fields that change:

```aura
class Stack:
    items: list[int64]
    cap: int64
    invariant 0 <= len(self.items) and len(self.items) <= self.cap

    public def push(mut self, x: int64):
        requires len(self.items) < self.cap
        ensures len(self.items) == len(old(self.items)) + 1
        self.items.append(x)


def fill(s: mut Stack):
    requires s.cap == 4 and len(s.items) == 0
    s.push(1)
    s.push(2)  # proved: `push` cannot write `cap`
    assert len(s.items) == 2


def main():
    mut s = Stack(items=[], cap=4)
    fill(s)
```

The verifier reads the fields a function may write from its body and from
the functions it calls. An assignment, a write through a `view mut`, a
method such as `append`, and a `mut` argument of a call each count against
the field they reach. A call whose callee is not known, such as one through
a function value or through a trait, may write any part of its `mut`
arguments.

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
the invariants and the condition that ended the loop. After a `for` loop
over a collection or an iterator, that is every invariant that does not
read the loop target: one that reads it is checked only inside the body
(see [Contracts](/manual/contracts)).

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

### Generators

The body of a generator function runs after the call returns, on the values
the call passed, so it is proved with the generator's `requires` clauses
known. Its checks are reported under the generator's name. Each call of the
generator proves the clauses, as for any function:

```aura
from iter import Generator


def countdown(n: int64) -> Generator[int64]:
    requires 0 <= n and n <= 100
    mut i = n
    while i > 0:
        invariant 0 <= i and i <= n
        yield i
        i -= 1


def main():
    for x in countdown(3):
        print(x)  # 3 2 1
```

The generator's locals keep their values across a `yield`. A value the
consumer can also reach, such as an atomic or a queue, is unknown after it,
as after any other step. A consumer knows nothing about the items it
receives beyond their type.

## Reports

`aura verify` prints a diagnostic for each check it could not prove, then a
summary line. With one file it exits with status 0 only when every check
was proved.

```text
error[AU5101]: cannot prove integer result in range
 --> scale.au:2:12
  |
2 |     return x * 2
  |            ^
  = note: in `double` (overflow check)
  = note: counterexample: x = 4611686018427387904
verified scale.au: 0 of 1 checks proved in 2 functions (/usr/bin/z3 (Z3 version 5.3.3 - 64 bit))
not proved: 1 failed, 0 not modeled, 0 undecided
```

When a check is open, a second summary line counts the open checks by
outcome: failed, not modeled, and undecided.

| Code | Meaning |
| --- | --- |
| `AU5101` | The check can fail. The counterexample gives parameter values that make it fail, as far as the solver reports them. An enum parameter shows as the name of its variant, as in `variant of light = Off`. |
| `AU5102` | The solver gave up before deciding. Raise `--timeout`, or add a `requires`, `invariant`, or `assert` that narrows the case. |
| `AU5103` | No solver could be started. |
| `AU5104` | The check is not modeled: it reads the result of a step the verifier does not model yet, such as `text.split(",")`, a stepped slice, the elements a comprehension builds, or floating-point `**`. The message names the step and its line. There is no counterexample, because the verifier does not know what the step returns. The check is not proved, and it stays a runtime check. |
| `AU5120` | The `requires` clauses and class invariants a function assumes on entry can never hold together, as in `requires x > 0 and x < 0`. No call can meet them, so every check in the function would pass for nothing, and the function does not count as proved. |
| `AU5122` | A function named with `--must-verify` has a check that is not proved. See [Functions That Must Verify](#functions-that-must-verify). |

A failure is reported only when the program can reach it. In this function
the verifier does not model `split`, so it cannot tell that the result has
at least one part, and it says so instead of inventing an input:

```text
error[AU5104]: cannot check `ensures` clause `result >= 1`: it depends on `split` at line 3, which the verifier does not model
```

A check beside such a step that does not read its result is still proved or
refuted as usual.

A check that fails to prove is not always a bug: the verifier may lack a
fact that holds. Add it as a `requires` on the function, an `invariant` on
the loop, or an `ensures` on the callee, and the verifier proves the fact
where it is stated and uses it where it is needed.

A check in a copy the compiler makes of a function is reported under the
function's own name, at the place it is written. The compiler makes such
copies for a generic function and for a function that receives a generator
as an argument.

## Verifying a Package

With one file, `aura verify` proves the functions of that file. It reads
the contracts of the modules the file imports and does not prove their
bodies. The summary says so:

```text
imported modules not checked: 1 (util); `aura verify` with no path checks every module
```

With no path, `aura verify` proves every module of the package in the
current directory. A directory, or several files, work the same way:

```bash
aura verify              # the package in the current directory
aura verify services/api # the package in that directory
aura verify src/bank.au src/ledger.au
```

A directory stands for its `src` directory when it has one. Every `.au`
file under it is verified once, as the entry of its own run. The report has
each file's diagnostics and summary line, then one line of totals:

```text
verified src/main.au: 1 of 1 checks proved in 1 functions (/usr/bin/z3 (Z3 version 5.3.3 - 64 bit))
verified src/util.au: 3 of 4 checks proved in 2 functions (/usr/bin/z3 (Z3 version 5.3.3 - 64 bit))
not proved: 1 failed, 0 not modeled, 0 undecided
verified 2 files: 4 of 5 checks proved in 3 functions; 1 failed, 0 not modeled, 0 undecided
```

A run over a package exits with status 1 when a check failed, when a file
does not compile, or when a function that must verify does not. A check
that is not modeled or undecided is reported and does not fail the run.
This differs from a run on one file, which fails unless every check is
proved. `--deny` makes those outcomes fail a package run too:

| Option | A package run also fails when |
| --- | --- |
| `--deny not-modeled` | a check is not modeled |
| `--deny undecided` | the solver could not decide a check |
| `--deny all` | any check is not proved |

`aura test --verify` verifies the package in the current directory before
it runs the tests. A failed check fails the command, and no test runs. See
[Testing](/manual/cli-and-tooling#testing).

### Functions That Must Verify

`--must-verify <function>` names a function whose checks must all be
proved. Give the name a report shows, such as `withdraw` or
`Account.deposit`. Repeat the option for more functions.

```bash
aura verify --must-verify withdraw --must-verify Account.deposit
```

A named function with a check that failed, is not modeled, or is undecided
reports `AU5122` at its `def`, and the run fails in every mode:

```text
error[AU5122]: function `power` marked verified has unproved checks
 --> src/util.au:6:1
  |
6 | public def power(x: float64) -> float64:
  | ^
  = note: checks not proved: 2; each has its own diagnostic
```

A name that matches no verified function also fails the run, so a
misspelled name cannot pass. Aura has no marker in the source for this.

### Options

| Option | Meaning |
| --- | --- |
| `--solver <path>` | The solver to run. The default is `AURA_SMT_SOLVER` when it is set, else `z3`, else `cvc5`, found on `PATH`. |
| `--timeout <ms>` | The time limit for each check. The default is 10000. |
| `--function <name>` | Verify only the functions whose name contains `name`. |
| `--deny <class>` | Make a package run fail on checks that are `not-modeled`, `undecided`, or `all` of both. See [Verifying a Package](#verifying-a-package). |
| `--must-verify <name>` | Report `AU5122` when function `name` has a check that is not proved. See [Functions That Must Verify](#functions-that-must-verify). |
| `--no-cache` | Solve every check again, without the proof cache. |
| `--format json` | Print the report as JSON: one entry for each check, with its kind, function, line, column, message, and status (`proved`, `failed`, `unknown`, or `unsupported`, which is a check that is not modeled), the counts `proved`, `failed`, `not_modeled` and `undecided`, the `must_verify` functions with their count of `open` checks, the `unchecked_modules`, then the diagnostics. A package run prints one document with a `files` list of such reports, each with its `path`, the `totals`, and `unmatched_must_verify`. A file that does not compile has an `error` in place of its report. |

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
- a list of lists: the length of each row, and the elements of each row
  when those are integers, `bool`, or `str`. A write through a borrow of a
  row, such as `grid[r][c] = v`, changes that element of that row
- a borrowed element of a list of classes or tuples, such as `xs[i]`
  passed to a function or `xs[0].add(1)`: it reads the element, and a
  write through it changes that element of the list
- list slices `xs[a:b]`, `xs[a:]`, and `xs[:b]`: the length, each element,
  and a check that the bounds are in order and inside the list. `reverse`,
  and `sort` and `sorted`: the result holds the same elements, and for
  integers sorted by their own order it is ordered. `k in xs` and
  `xs.contains(k)` for integers, `bool`, and `str`
- a comprehension over one list or `range`: its length, or with an `if`
  filter a bound on it. The elements it builds are not modeled. `min(xs)`
  and `max(xs)` of a list of integers: a bound of every element that is one
  of them
- `float32` and `float64` as IEEE-754 values, rounding each `+`, `-`, `*`,
  `/`, and `sqrt` to nearest, ties to even, with NaN, infinities, and signed
  zeros; comparisons, `abs`, `to_float`, and casts between floats and
  integers
- `str`, as a sequence of characters: literals, `+`, `==`, `<` and the other
  comparisons, `len`, `in`, `contains`, `starts_with`, and `ends_with`
- `dict` and `set` with integer or `str` keys: which keys they hold, their
  length, and a `dict`'s values when those are integers, `bool`, or `str`;
  literals, `d[k]`, `d[k] = v`, `in`, `len`, `is_empty`, `clear`, `get`,
  `remove`, `keys`, `values`, and `items` of a `dict`, and `add`, `remove`, `discard`, and
  `clear` of a `set`, with `==` of whole collections
- `Span[T]` and `view` bindings of a list: `len`, each index against the
  span's own length, the bounds of the range where a span is made, and the
  elements, in code and in contracts. A write through a `mut` span or view
  reaches the list it views
- `Array[T]`: its element count and its `shape()`. Each element read and
  write is an index check: the number of coordinates is the rank, and each
  coordinate is inside its dimension. A one-coordinate read such as `a[i]`
  therefore needs `requires len(a.shape()) == 1`, because it traps on an
  `Array` of any other rank. The elements themselves, slices, and slice
  assignment are not modeled
- tuples, classes, enums, and unions such as `int64 | None`, field by field
- `match` statements and `match` expressions: each arm is taken where its
  pattern and guard hold, and a `match` over every variant of an enum takes
  one of its arms
- a method call on a union value: the `requires` of each member's method
  is proved where the value holds that member, and its `ensures` is assumed
  there
- in contracts, a union with one member besides `None` where that member
  is expected, as in `ensures result is None or result > 0`; `==` of
  unions; and an enum variant without a payload, as in
  `ensures result == Light.Green`. A contract cannot name a payload of a
  `Result` or of another enum, because a contract may not contain `match`
- `min(xs)` and `max(xs)` also when the list is a field, as in
  `max(self.items)`
- `len`, `abs`, `min`, and `max` of integers, list `append`, `pop`, `clear`,
  element reads and writes, and `copy`
- integer bit operations and shifts. `<<` and `>>` are exact, with the
  count and lost-bits checks of the run time; `>>` keeps the sign. `&`,
  `|`, and `^` with a literal operand, and `~`, are exact. Of two values
  that are not literals, `&`, `|`, and `^` have only the bounds the
  operands' signs give, such as `0 <= a & b <= a` for `a >= 0`
- the `wrapping_` and `saturating_` methods of integers, exactly
- integer `**` with a literal exponent up to 63, or with a literal base,
  as in `2 ** k`. A power of two values that are not literals has only
  its sign and the cases of exponent 0 and 1
- `as` casts between numbers in contracts, with the same checks as in code
- of the `math` module: `floor`, `ceil`, `trunc`, `sqrt`, `is_nan`,
  `is_inf`, `is_finite`, `isqrt`, and `factorial` exactly, each with the
  check of its argument; `gcd`, `lcm`, `comb`, and `perm` by their bounds;
  `sin` and `cos` between -1 and 1, and `exp` not below 0. The overflow of
  `lcm`, `comb`, `perm`, and `exp` is not checked
- `spec def` helpers, `old`, and `result` in contracts, and `ghost` locals.
  A recursive `spec def` is unfolded 8 levels deep for arguments that are
  not literals; a check that needs more is reported as `AU5104`
- parameters, class fields, and function results of a callable alias with
  a contract ([Callable Contracts](/manual/contracts#callable-contracts)):
  calls through them prove its `requires` and assume its `ensures`, and a
  named function passed, stored, or returned as one is proved to fit it
- `extern "C"` functions with a trusted contract: an indented block of
  `requires` and `ensures` after the signature. Each call proves the
  `requires`, and the result and `mut` arguments have the `ensures`
- task starts: `group.start(work, n)` proves `work`'s `requires` on the
  arguments the child receives, and the value of `Ready` from
  `task.result()`, `poll()`, or `result_or(...)` has `work`'s `ensures`.
  Each task's function is verified like any other
- `control.recover(f, args...)` with a named function: the call proves
  `f`'s `requires`, and the result is `Ok` with a value that has `f`'s
  `ensures`, or `Err` with a trap whose contents are not modeled. The
  verifier never assumes the call
  returns `Ok`, since even a call free of checks can run out of stack. The
  caller's own values are unchanged on both paths
- state that tasks share: the value a `with m.lock():` block starts with,
  every receive from a `Queue`, `cancelled()`, and the result of every
  atomic and `sync.Map` call are each any value of their type, because
  another task may change them at any time. Inside the block the guarded
  value is a local that only this task changes, so facts about it hold
  until the block ends
- the shapes of `tensor.Tensor` values, not their elements: `shape()` and
  `len()` in code and in contracts. `zeros`, `full`, `rand`, `randn`, and
  `from_array` make a tensor of the given shape. `matmul` checks the
  `Array.matmul` shape rules, elementwise operators and comparisons check
  that the shapes broadcast, `reshape` with a literal shape checks the
  element count, `sum`, `min`, `max`, and `mean` along an axis check the
  axis, and `item()` checks that there is one element; each gives the
  result's shape. `transpose`, `flatten`, `scalar`, and the elementwise
  functions such as `relu` keep or rearrange the shape. A matrix product
  of parameters needs their ranks, as in
  `requires len(a.shape()) == 2 and len(b.shape()) == 2 and a.shape()[1] == b.shape()[0]`.
  Other tensor members, and the device and dtype checks, are not modeled
- calls through a function value that names one function, such as
  `g = inc` then `g(5)`: the call proves `inc`'s `requires` and assumes its
  `ensures`
- lambdas, local `def`s, and bound methods such as `f = account.add`,
  called in the function that makes them, directly or through a local:
  the body runs at each call, on the arguments and on what the closure
  captured. A capture by value has the value it had where the closure was
  made. A capture by reference (`[mut total]`, `[values]`) is read at the
  call and written back after it. State the closure owns and changes,
  such as a counter it captured by value, carries from one call to the
  next. Each check in the body is made at the call and reported in the
  calling function. A local `def` cannot state `requires` or `ensures`,
  and an `assert` at its top is checked against the arguments of every
  call. A call of a bound method proves the method's `requires` and
  assumes its `ensures`. A closure that is only called this way is not
  verified on its own, where its captured values would be unknown
- quantifiers: `forall(x in xs, condition)`, `exists(...)`, and
  `all([... for ...])` or `any(...)`, with any number of `for` clauses, each
  over a `range`, a modeled list, a `set`, the `keys()` of a `dict`, or
  `enumerate` or `zip` of modeled lists,
  with tuple targets and `if` filters, nested, in any contract, `assert`, or
  loop `invariant`

Anything else is a value the verifier knows nothing about: a `dict` or
`set` of other keys, the result of float `//`, `%`, and `**`, the result of most library calls, such as
`str.split`, and what a call returns through a function value the verifier cannot
follow, such as one passed as an argument, stored in a collection, or
chosen on different paths. Such a value's `requires` is reported as
`AU5104` where it leaves the function, and stays checked when it is
called. A check that reads one of them is reported as not modeled
(`AU5104`, naming the step), never as a failure with a counterexample.

Of closures, these are not modeled:

- a closure that leaves the function that makes it: passed as an
  argument, returned, stored in a field or a collection, or captured by
  another closure. Its body is verified on its own, for any captured
  value. A local it captured by reference is unknown after every call
- a recursive call of a local `def`, which has no contract to state what
  it returns
- a closure with state of its own called in a loop, and a closure that
  captured a view a function returned
- a decorated function. The decorated name is the function value the
  decorator returned, so a call of it proves no `requires` and assumes no
  `ensures`, even through a decorator that returns its argument. The
  body is still verified against its own contract

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
does about the divisor.

An exact float division or square root is slow for the solver, so a check
that reads one is first tried with what is known about the rounded result:
its sign, when it is NaN, zero, or infinite, how its size compares with its
operands and with `1.0`, and its bounds when its operands lie between
literals of the function. A check that follows from those is proved at once:

```aura
def ratio(x: float64, y: float64) -> float64:
    requires 0.0 <= x and x <= 100.0
    requires 1.0 <= y and y <= 10.0
    ensures 0.0 <= result and result <= x
    return x / y

def root(x: float64) -> float64:
    requires 1.0 <= x and x <= 100.0
    ensures 1.0 <= result and result <= 10.0
    return x.sqrt()
```

A check those facts do not prove is then tried with sample values for the
function's `float32` and `float64` parameters. The samples lie between the
float literals the function names, and only samples the `requires` clauses
allow are used. With its inputs known, the check is plain arithmetic, so a
claim that many inputs break fails at once with one of them as the
counterexample:

```aura
def round_trip(a: float64) -> float64:
    requires 1.0 <= a and a <= 1000.0
    ensures result == a
    return a / 3.0 * 3.0
```

`aura verify` reports `AU5101` here with an input such as
`a = 491.9299361266708`. Samples cannot prove a check, and they do not
cover floats inside a list or a class field.

Any other check is asked of the exact problem, which also finds the
counterexample of one that fails. One the solver cannot finish in time is
reported as `AU5102`; a contract that bounds the operands by literals often
turns it into one the first query proves.

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

A contract on an `extern "C"` function is the other boundary a proof relies on
without reading its body. The verifier cannot read C, so it proves each
`requires` at the call and assumes each `ensures` after it:

```aura fragment
extern "C" def clamp_index(i: int64, n: int64) -> int64:
    requires n > 0
    ensures 0 <= result and result < n
```

At run time, the program checks both clauses around the foreign call and keeps
those checks under `--remove-proved-checks`. A wrong `ensures` still misleads
static proofs that rely on it, so keep such a contract to what the C function
guarantees.
