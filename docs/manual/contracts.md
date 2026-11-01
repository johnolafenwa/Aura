# Contracts

A contract states a rule of a function, loop, or class in the code itself.
A `requires` clause says what a caller must provide, an `ensures` clause
what the function promises back, and an `invariant` what stays true while
a loop runs or while an object exists. Aura checks every clause each time
it applies, in `aura run` and in built programs alike, and stops the program
with `AU4009` when one is false.

Contracts are the first stage of Aura's verification plan. A later
`aura verify` command will prove clauses before the program runs; until
then they are runtime checks. See the
[verification design](https://github.com/johnolafenwa/Aura/blob/main/architecture_docs/23-verification-design-checkpoint.md).

## Function Contracts

`requires` and `ensures` lines come at the top of a function or method
body, after the docstring and before the first other statement.

```aura
def withdraw(balance: int64, amount: int64) -> int64:
    """Returns the balance left after taking out `amount`."""
    requires amount > 0
    requires amount <= balance
    ensures result >= 0
    return balance - amount


def main():
    print(withdraw(100, 30))  # 70
```

- `requires condition` is checked on entry, after the arguments are bound.
  It is the caller's promise.
- `ensures condition` is checked on every path that returns, after any
  `with` exits have run. It is the function's promise. Inside `ensures`,
  `result` names the returned value. A function that returns `None` checks
  its `ensures` when it falls off the end or reaches `return`.
- Clauses may come in any mix and run in source order. A clause after the
  first other statement is `AU2048`.

Clauses work the same way on methods, generic functions, and recursive
functions. A recursive call checks the clauses again.

## Conditions

A condition has type `bool` and must not change program state. It may:

- read parameters, `self`, fields, elements, slices, and module constants
- use operators, comparisons, conditional expressions, f-strings, and
  comprehensions
- call pure builtins such as `len`, `abs`, `min`, `max`, `sum`, `all`,
  `any`, `ord`, `chr`, `sorted`, and `range`
- call read-only methods such as `contains`, `find`, `count`, `keys`,
  `starts_with`, and `copy` on builtin collections, strings, and tuples
- call `spec def` functions

A condition may not call a user function, call any method of a user class,
enum, or type parameter, contain a lambda, `match`, or `try`, or name an
explicit specialization. Each of these is `AU2048`. A condition that is not
`bool` is `AU2002`.

Arithmetic in a condition is checked like any other arithmetic, so an
overflow in a condition stops the program with its usual diagnostic.

## `old`

Inside `ensures`, `old(expr)` is the value `expr` had on entry, after the
`requires` checks:

```aura
def push(xs: mut list[int64], x: int64):
    ensures len(xs) == len(old(xs)) + 1
    ensures xs[len(xs) - 1] == x
    xs.append(x)


def main():
    mut xs = [1, 2]
    push(xs, 3)
    print(xs)  # [1, 2, 3]
```

`expr` reads a parameter or `self`, or a field, element, or slice of one.
The function keeps a copy of the value, so `expr` must have a Copy type or
be a `str`, list, dict, set, or `Array`. `old` anywhere else is `AU2048`.

## Quantifiers

`forall(x in collection, condition)` is true when `condition` holds for
every element, and `exists(x in collection, condition)` when it holds for
at least one. The binding may be a tuple, as in
`forall((i, x) in enumerate(xs), x >= i)`, and the collection may be a
`range`.

A quantifier evaluates `condition` for every element, then combines the
results like `all` and `any`.

## `spec def`

A `spec def` is a named helper for conditions:

```aura
spec def is_sorted(xs: list[int64]) -> bool:
    return forall(i in range(len(xs) - 1), xs[i] <= xs[i + 1])


def insert_sorted(xs: mut list[int64], x: int64):
    requires is_sorted(xs)
    ensures is_sorted(xs)
    ensures len(xs) == len(old(xs)) + 1
    ensures exists(y in xs, y == x)
    mut i = len(xs)
    xs.append(x)
    while i > 0 and xs[i - 1] > x:
        invariant 0 <= i and i < len(xs)
        decreases i
        previous = xs[i - 1]
        xs[i] = previous
        i -= 1
    xs[i] = x


def main():
    mut xs = [1, 3, 5]
    insert_sorted(xs, 4)
    print(xs)  # [1, 3, 4, 5]
```

- A `spec def` is a module-level function. Its body is one `return` of an
  expression that follows the condition rules, after an optional docstring.
- It takes no `mut` parameters and declares no clauses of its own.
- It may call other `spec def` functions, itself included.
- Only contract clauses and other `spec def` bodies may call or name it.
  Using one anywhere else is `AU2048`.
- `public spec def` exports it for other modules' contracts.

## Loop Invariants And `decreases`

A loop can state what stays true while it runs, with `invariant` clauses at
the top of its body. A `while` loop can also state why it ends, with one
`decreases` clause.

```aura
def find(xs: list[int64], key: int64) -> int64:
    """Index of `key` in sorted `xs`, or -1."""
    mut lo = 0
    mut hi = len(xs)
    while lo < hi:
        invariant 0 <= lo and lo <= hi and hi <= len(xs)
        decreases hi - lo
        mid = lo + (hi - lo) // 2
        if xs[mid] < key:
            lo = mid + 1
        elif xs[mid] > key:
            hi = mid
        else:
            return mid
    return -1


def total(xs: list[int64]) -> int64:
    mut acc = 0
    for i, x in enumerate(xs):
        invariant acc == sum(xs[0:i])
        acc += x
    return acc


def main():
    print(find([1, 3, 5, 7], 5))  # 2
    print(total([1, 2, 3]))  # 6
```

- A `while` invariant is checked before every test of the condition: on
  entry, after every iteration (including one that ends in `continue`),
  and when the condition ends the loop. A `break` leaves without a check.
- A `for` invariant is checked at the start of every iteration, after the
  loop target is bound, so it can read the target.
- `decreases measure` takes an `int64` measure. It is checked each time
  the body starts: the measure must be at least 0, and smaller than it was
  the previous time. A loop that keeps both rules must end. A `for` loop
  always ends, so it takes no `decreases`, and a loop takes at most one.

## Class Invariants

A class can state what is true of every one of its objects, with
`invariant` lines in its body. Inside them, `self` names the object.

```aura
class Account:
    balance: int64
    limit: int64 = 1000
    invariant self.balance >= 0
    invariant self.balance <= self.limit

    public def deposit(mut self, amount: int64):
        requires amount > 0
        self.balance += amount


def main():
    mut account = Account(balance=10)
    account.deposit(5)
    print(account.balance)  # 15
```

The invariants are checked:

- after every construction, by a field call such as `Account(balance=10)`
  or by `__init__`, in whichever module builds the object, and
- on entry to and on every return from each `public` method that takes
  `mut self`. On entry they run after the method's `requires`; on return,
  after its `ensures`.

Methods that are not `public`, and methods that only read `self`, run
without checks, so a private helper may pass through a state that breaks
an invariant as long as the public method that calls it restores it. Code
in the class's own module that assigns a field directly is not checked
either. Objects that `json.decode` builds are not checked yet.

The method name `__invariant__` is reserved for the check.

## Failures

A failed clause stops the program with `AU4009`. The message names the
clause and its condition as written, and a comparison reports its two
operands as a failed `assert` does:

```text
error[AU4009]: `requires` clause failed: `amount <= balance`
 --> bank.au:4:14
  = note: left = 500
  = note: right = 100
  = note: Aura call chain (innermost first): withdraw at 1:1 -> main at 9:1
```

A failed `decreases` says which rule broke: the measure `went below 0`, or
`did not get smaller in the last iteration`. A class invariant failure
shows `Class.__invariant__` as the innermost frame of the call chain,
followed by the method or construction that ran the check.

## Contextual Keywords

`requires`, `ensures`, `invariant`, and `decreases` are keywords only at
the start of a clause line, and `spec` only before `def`. Elsewhere they
are ordinary names, so `requires = 1`, `invariant: int64`, and the call
`requires(x)` keep their meaning. Write a space before a parenthesis that
starts a condition: `requires (a or b) and c`. `result`, `old`, `forall`,
and `exists` are special only inside clauses and `spec def` bodies.

## Not Available Yet

These forms report `AU2048`:

- `ensures` on `__init__` or on a function with a view result
- an `ensures` that reads an `own` parameter of a non-Copy type, which the
  body may have moved; `old(param)` reads it on entry instead
- a parameter named `result` in a function with `ensures`
- clauses on the methods of a trait implementation
- `spec def` methods

## Diagnostics

| Code | When |
| --- | --- |
| `AU2048` | A clause out of place, an impure condition, a misused `old`, `spec def`, or quantifier, or a form listed under Not Available Yet. |
| `AU2002` | A condition that is not `bool`, or a `decreases` measure that is not `int64`. |
| `AU4009` | A clause was false at run time. |
