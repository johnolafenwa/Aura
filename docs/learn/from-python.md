# Aura For Python Developers

Most of what you know from Python carries over. Indentation, `def`, `class`,
f-strings, comprehensions, keyword arguments, and `for x in items` all work
the way you expect. This page covers the parts that differ, so the compiler
stops surprising you.

## There Is No `if __name__ == "__main__"`

A file with statements at the top level is a script. It runs from top to
bottom:

```aura
langs = ["python", "aura"]

for lang in langs:
    print(f"hello, {lang}")
```

When you want a real entry point, such as a program that returns an exit
code or one you will compile, write `main`:

```aura
def main() -> int32:
    print("hello")
    return 0
```

A file picks one side. It has either top-level statements or an explicit
`main`, never both. Declarations such as `class` and `def` work alongside
either one.

## Bindings Are Immutable Unless You Say Otherwise

This is the first error most Python developers hit:

```aura check-fail:AU3003
def main():
    total = 0
    total = total + 1   # error: cannot assign to immutable binding `total`
```

Add `mut` and it works:

```aura
def main():
    mut total = 0
    total = total + 1
```

`mut` is not a type. It is permission to rebind or mutate. You will see it in
three places:

- local bindings
- parameters that a function may change
- methods that modify their object

Top-level entry scripts follow the same rule. A `mut` binding and its later
plain or compound assignments belong to the script's shared local
environment:

```aura
mut count = 0
count = count + 1
count += 1
print(count)  # 2
```

A new bare top-level binding such as `limit = 3` declares an immutable module
constant. Module constants initialize before the top-level entry statements
run, even when the two are interleaved in the file. Use `mut limit = ...`
when the value must be computed from an earlier top-level script local.

## Values Have Owners

Python passes references around, and a garbage collector cleans up later.
Aura tracks a single owner for every value. The function signature tells you
what a function does to its argument:

```aura
def shout(name: str) -> str:          # shared: reads it, you keep it
    return name.to_upper()

def add_tag(tags: mut list[str], tag: own str):   # mut: changes yours
    tags.append(tag)                              # own: takes it

def consume(name: own str) -> int64:  # own: it is theirs now
    return name.len()
```

Three capabilities make up the whole model:

| Spelling | The callee can | You afterwards |
| --- | --- | --- |
| `name: str` | read it | still own it |
| `name: mut str` | change it in place | still own it, changed |
| `name: own str` | do anything, including keep it | no longer have it |

Calls look like Python, with no sigils and no `&`:

```aura fragment
label = "aura"
print(shout(label))
print(shout(label))   # fine, shout only reads
```

Once you give a value away, the compiler holds you to it:

```aura fragment
n = consume(label)
print(label)
```

```text
error[AU3001]: use of moved value `label`
  = related owner.au:6:17: value moved here
  = help: pass shared access when ownership is not needed, or call `.clone()`
    at the move site when an independent value is required
```

Read that error as a question: did you mean to hand the value over, or to
share it?

Small values copy instead of moving. Numbers, booleans, and durations are
copied, so you can pass them freely. Everything else moves, including
strings, collections, class instances, and files.

## Classes Have No `__init__`

An Aura class is fields and methods. There is no initializer and no `self`
assignment ceremony. You construct an instance with keyword arguments, and
fields may declare defaults:

```aura
class Account:
    owner: str
    balance: float64
    currency: str = "USD"
```

```aura fragment
account = Account(owner="ada", balance=0.0)
```

For named construction, the job `__init__` and `@classmethod` do in Python,
write a function on the class that takes no `self` and returns an instance:

```aura
class Account:
    owner: str
    balance: float64
    currency: str = "USD"

    def new(owner: own str) -> Account:
        return Account(owner=owner, balance=0.0)

    def opening(owner: own str, deposit: float64) -> Account:
        return Account(owner=owner, balance=deposit)
```

```aura fragment
fresh = Account.new("ada")
mut acct = Account.opening("grace", 100.0)
```

These are associated functions. You call them through the class name, and
they are free to validate, compute, or pick defaults. A class can have as
many as it needs, without `@classmethod` decorators.

### Methods Say What They Do To `self`

The receiver uses the same three capabilities as parameters:

```aura fragment
    def label(self) -> str:              # reads
        return f"{self.owner}: {self.balance} {self.currency}"

    def deposit(mut self, amount: float64):   # modifies
        self.balance += amount

    def into_balance(own self) -> float64:    # consumes
        return self.balance
```

```aura fragment
acct.deposit(25.0)
print(acct.label())
final = acct.into_balance()   # acct is gone after this
```

A bare `self` cannot mutate. The compiler tells you to write `mut self`.

A method named `close` is special. It makes the class a managed resource for
`with` blocks, so pick another name unless that is what you want.

### There Is No Inheritance

`class Dog(Animal):` does not parse. Aura uses traits for shared behavior and
composition for shared data. Where you would reach for a base class, define a
trait with the methods and implement it for each type.

## Failure Is A Return Value

Aura has no exceptions and no `try`/`except`. A function that can fail says
so in its return type:

```aura
def parse_port(text: str) -> Result[int64, str]:
    match parse_int64(text):
        case Result.Ok(port):
            if port > 65535:
                return Result.Err("port out of range")
            return Result.Ok(port)
        case Result.Err(_):
            return Result.Err(f"not a number: {text}")
```

The caller must handle both outcomes. Errors never propagate invisibly:

```aura fragment
match parse_port("8080"):
    case Result.Ok(port):
        print(f"listening on {port}")
    case Result.Err(message):
        print(f"bad config: {message}")
```

`T | None` covers "a value or `None`". A `str | None` holds either a string
or `None`, and `is None` and `is not None` test which. When your own function
returns a `Result`, `try` passes an error up to the caller. See
[Results And Optional Values](/learn/results-and-options).

## Types Are Static, But Locals Infer

Annotations are required where a contract crosses a boundary, which means
parameters and return types. Aura infers every other type:

```aura
def total(prices: list[float64]) -> float64:
    mut sum = 0.0        # inferred float64
    for price in prices:
        sum += price
    return sum
```

Three differences to know up front:

- **A missing parameter type** is a parse error, not a dynamic parameter.
  `def f(x):` does not compile.
- **Numeric types** never convert implicitly. Passing an `int32` where an
  `int64` is expected is an error. Cast with `as int64` or `.to_float()`.
  Unsuffixed integer literals are `int64`, and float literals are `float64`.
- **Generics are explicit**: `list[str]`, `dict[str, int64]`, `Lookup[int64]`.
  Type parameters are declared, as in
  `def first[T](values: list[T]) -> T | None`.

### If It Returns A Value, Declare The Type

A function with no `->` returns nothing. That is fine when the body returns
nothing. A bare `return` for an early exit is fine too:

```aura
def greet(name: str):
    print(f"hi {name}")

def early(flag: bool):
    if flag:
        return
    print("no")
```

When the body returns a value, the signature must say so. Aura does not infer
the return type from the body:

```aura check-fail:AU2002
def double(n: int64):
    return n * 2
```

```text
error[AU2002]: return type mismatch: expected `None`, found `int64`
 --> ret_bad.au:2:5
  |
2 |     return n * 2
  |     ^
```

A missing `->` means `-> None`. To fix the error, write the type you meant:

```aura
def double(n: int64) -> int64:
    return n * 2
```

For a day or so this feels like extra typing. Then it starts to read as
documentation. Every signature tells you what goes in and what comes back,
without opening the body.

## Things That Will Surprise You

**Integer `/` is rejected.** Python 3 made `/` true division. Aura makes you
choose, because silent truncation is the older bug:

```text
error[AU2003]: integer `/` is not supported; use `//` for floor division,
or call `.to_float()` on both operands for true division
```

**There is no truthiness.** `if values:` fails, because a condition must be a
`bool`. Write `if values.len() > 0:` or `if value is None:`.

**Strings are not indexable.** `s[0]` does not work. A `str` is a sequence of
Unicode scalar values, and `len()` counts those. A slice such as `s[1:4]`
gives you an owned copy. Use slices, or operations in the style of
`s.split("")`, instead of indexing single characters.

**`is` only tests for `None`.** `value is None` and `value is not None` check
an optional `T | None` and narrow it. There is no identity comparison between
arbitrary values.

**You cannot index a non-copy list element into an owned value.** That would
move a value out of a collection you still own. Instead:

- Use `view value = values[index]` to access the element in place.
- Use `view mut` through a mutable list to update the selected element.
- Use `values.get(index)` for an independent cloned value. It requires
  clone-safe elements and returns `Lookup.Found(value)` holding a clone, or
  `Lookup.Missing` for an invalid position.

**Top-level bindings live in module storage.** You cannot move a value out of
module storage. To consume a value with an `own` method, do it inside a
function.

**Module state is immutable.** In a file with `main`, constants at module
level are fine, but `mut` at module level is not. Mutable state belongs to an
owner, usually `main`.

## Where To Go Next

- [Values, Moves, And Borrows](/learn/ownership-and-borrowing): the ownership
  model in depth, with the errors you will meet and how to fix each one.
- [Shaping Data](/learn/data-modeling): classes, enums, methods, copy classes,
  and generics.
- [Testing](/learn/testing): `aura test`, and assertions that show their
  values.
- [The Manual](/manual/): the exact rules when you need the full contract.
