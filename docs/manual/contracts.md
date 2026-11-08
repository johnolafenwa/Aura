# Contracts

A contract states a rule of a function, loop, or class in the code itself.
A `requires` clause says what a caller must provide, an `ensures` clause
what the function promises back, and an `invariant` what stays true while
a loop runs or while an object exists. Aura checks every clause each time
it applies, in `aura run` and in built programs alike, and stops the program
with `AU4009` when one is false.

`aura verify` proves clauses before the program runs, together with the
arithmetic and list indexes around them; see
[Verification](/manual/verification). The checks still run in every build.

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

- A `spec def` is a module-level function or a method. Its body is one
  `return` of an expression that follows the condition rules, after an
  optional docstring.
- It takes no `mut` parameters. It may state `requires` clauses, which say
  where it is defined: they are checked when it runs, and `aura verify`
  proves them wherever a contract uses it. It takes no other clauses.
- It may call other `spec def` functions, itself included.
- Only contract clauses and other `spec def` bodies may call or name a
  module-level one. Using one anywhere else is `AU2048`.
- `public spec def` exports it for other modules' contracts.

A `spec def` method reads `self` and is an ordinary method too, so code may
call it as well as conditions:

```aura
class Account:
    balance: int64
    limit: int64 = 1000

    spec def headroom(self) -> int64:
        return self.limit - self.balance

    public def deposit(mut self, amount: int64):
        requires amount > 0 and amount <= self.headroom()
        ensures self.headroom() == old(self.headroom()) - amount
        self.balance += amount


def main():
    mut account = Account(balance=10)
    account.deposit(5)
    print(account.headroom())  # 985
```

A condition may call a method only when it is a read-only builtin method,
such as `contains`, or a `spec def` method of the receiver's class. It
takes `self`, never `mut self`.

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
  loop target is bound, so it can read the target. A loop over a
  `range(...)` call checks it once more when the range runs out, with the
  target at the range's end (or at its start when the range is empty), so
  after `for i in range(n)` the invariant holds with `i == n`. A `break`
  leaves without a check.
- `decreases measure` takes an `int64` measure. It is checked each time
  the body starts: the measure must be at least 0, and smaller than it was
  the previous time. A loop that keeps both rules must end. A `for` loop
  always ends, so it takes no `decreases`, and a loop takes at most one.

### Recursive Functions

A function that calls itself can state a `decreases` measure too. It is
checked on entry, where it must be at least 0, and before every call the
function makes to itself, where the measure of the call's arguments must
be smaller than the measure on entry. A recursion that keeps both rules
must end.

```aura
def gcd(a: int64, b: int64) -> int64:
    requires a >= 0 and b >= 0
    decreases b
    if b == 0:
        return a
    return gcd(b, a % b)

def main():
    print(gcd(84, 36))  # 12
```

A method can state one too; its measure may read `self`, and a call on
another object, such as `rest.total()` on the next node of a list, computes
the callee's measure with `self` naming that object.

Functions that call each other in a cycle each state a measure, and every
call between two of them checks that the callee's measure of the call's
arguments is smaller than the caller's measure on entry:

```aura
def is_even(n: int64) -> bool:
    requires n >= 0
    decreases n
    if n == 0:
        return true
    return is_odd(n - 1)

def is_odd(n: int64) -> bool:
    requires n >= 0
    decreases n
    if n == 0:
        return false
    return is_even(n - 1)

def main():
    print(is_even(10))  # true
```

A call to a function that never calls back is not compared, so a measured
function may call a measured helper with any argument.

The measure is computed again from the call's arguments before the call,
so each argument the measure reads must be a pure expression, and a
receiver the measure reads must name a variable or a field of one. A
function takes at most one `decreases`. A cycle through a function that
states no measure, a call through a function value, or a call through a
trait is not checked.

## Trait Methods

A trait method can state `requires` and `ensures` clauses, and every
implementation of it inherits them. A call through the trait, such as a
call on a value whose type is a type parameter bounded by the trait, relies
on the same contract whichever implementation runs.

```aura
trait Shape:
    def area(self) -> int64:
        ensures result >= 0

class Square:
    side: int64

impl Shape for Square:
    def area(self) -> int64:
        return self.side * self.side

def total[T: Shape](items: list[T]) -> int64:
    mut sum = 0
    for item in items:
        sum += item.area()
    return sum

def main():
    print(total([Square(side=1), Square(side=2)]))  # 5
```

The clauses are checked in each implementation as if it had written them,
and its parameters keep the trait's names, which the clauses read. A trait
method takes no `decreases`.

An implementation may refine the contract:

- It keeps every `ensures` of the trait, and may add more. A caller that
  knows the implementation relies on all of them.
- Stating its own `requires` replaces the trait's, so it may ask for less.
  A call through the trait checks the implementation's `requires` when it
  runs, and `aura verify` proves that the trait's `requires` imply them,
  so a caller that proved the trait's never fails them.

```aura
trait Scaler:
    def scale(self, x: int64) -> int64:
        requires 0 <= x and x <= 100
        ensures result >= 0

class Double:
    pad: int64

impl Scaler for Double:
    def scale(self, x: int64) -> int64:
        requires 0 <= x and x <= 1000
        ensures result == 2 * x
        return 2 * x

def main():
    d = Double(pad=0)
    print(d.scale(500))  # 1000
```

## Lemmas

A `lemma def` is a proof: a function with no result whose `ensures`
clauses state a fact, and whose body shows why it holds. `aura verify`
proves a lemma once, like any function, and then relies on its `ensures`
wherever it is called. A call written as a statement is erased when the
program runs, so a lemma costs nothing at run time, however much work its
body does.

```aura
spec def tri(n: int64) -> int64:
    return n * (n + 1) // 2

lemma def tri_bounded(n: int64):
    requires 0 <= n and n <= 1000
    decreases n
    ensures 0 <= tri(n) and tri(n) <= 500500
    if n > 0:
        tri_bounded(n - 1)

def sum_to(n: int64) -> int64:
    requires 0 <= n and n <= 1000
    ensures result == tri(n)
    mut total = 0
    mut i = 0
    while i < n:
        invariant 0 <= i and i <= n
        invariant total == tri(i)
        tri_bounded(i)
        i += 1
        total += i
    return total

def main():
    print(sum_to(10))  # 55
```

A recursive lemma with a `decreases` measure is a proof by induction: the
call `tri_bounded(n - 1)` assumes the fact for the smaller case. A lemma
body holds only `assert`, `if`, loops, assignments of local names, and
calls of other lemmas, each pure, and it may read `spec def` helpers.
Its parameters cannot be `mut`, and it returns nothing. The arguments of a
lemma call written as a statement must be pure, since the call does not
run. Because nothing checks an erased call when the program runs, a lemma
or a lemma call that the verifier cannot prove keeps
`aura build --remove-proved-checks` from removing any check.

## Ghost Locals

A `ghost` local holds a value only the verifier reads: a count of steps, a
copy of an earlier value, or the result of a `spec def`. Declare it with
`ghost name = value`, or `ghost mut name = value` to change it later with
`ghost name = value` or `ghost name += value`. The program never computes a
ghost local, and an `assert` or loop clause that reads one is checked by
`aura verify` but not when the program runs.

```aura
def total(xs: list[int64]) -> int64:
    requires len(xs) <= 100
    requires all([0 <= x and x <= 10 for x in xs])
    ensures result <= 1000
    mut s = 0
    ghost mut steps = 0
    for i in range(len(xs)):
        invariant steps == i and 0 <= s and s <= 10 * i
        s += xs[i]
        ghost steps += 1
    assert steps == len(xs)
    return s

def main():
    print(total([1, 2, 3]))  # 6
```

A ghost value, and an `assert` that reads a ghost local, must be pure, and
either may call `spec def` functions. Only ghost values, `assert`s, and
loop clauses may read a ghost local; ordinary code that reads one, or
assigns its name without `ghost`, reports `AU2048`. As with an erased
lemma call, a ghost `assert` that the verifier cannot prove keeps
`aura build --remove-proved-checks` from removing any check.

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

`aura verify` goes further and proves that an object holds its invariants
everywhere it crosses a function boundary, and assumes it there; see
[What It Proves](/manual/verification#what-it-proves).

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
the start of a clause line, `spec` and `lemma` only before `def`, and
`ghost` only before a local's assignment. Elsewhere they
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
- `lemma def` methods

## Diagnostics

| Code | When |
| --- | --- |
| `AU2048` | A clause out of place, an impure condition, a misused `old`, `spec def`, or quantifier, or a form listed under Not Available Yet. |
| `AU2002` | A condition that is not `bool`, or a `decreases` measure that is not `int64`. |
| `AU4009` | A clause was false at run time. |
