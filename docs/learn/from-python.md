# Aura For Python Developers

Most of what you know transfers. Indentation, `def`, `class`, f-strings,
comprehensions, keyword arguments, `for x in items` — all of it works the way
you expect. This chapter is about the parts that do not, so the compiler stops
surprising you by the end of the page.

## There Is No `if __name__ == "__main__"`

A file with statements at the top level *is* the script. It runs top to bottom:

```aura
langs = ["python", "aura"]

for lang in langs:
    print(f"hello, {lang}")
```

When you want a real entry point — an exit code, a program you will compile —
write `main`:

```aura
def main() -> int32:
    print("hello")
    return 0
```

The one rule to remember: a file picks a side. It either has top-level
statements or an explicit `main`, never both. Declarations like `class` and
`def` are fine alongside either.

## Bindings Are Immutable Unless You Say Otherwise

This is the first error most Python developers hit:

```aura
total = 0
total = total + 1   # error: cannot assign to immutable binding `total`
```

Add `mut` and it works:

```aura
mut total = 0
total = total + 1
```

`mut` is not a type — it is permission to rebind or mutate. You will see it in
three places: local bindings, parameters that a function may change, and
methods that modify their object.

## Values Have Owners

Python passes references around and a garbage collector eventually cleans up.
Aura tracks a single owner for every value, and the *signature* tells you what
a function does to its argument.

```aura
def shout(name: str) -> str:          # shared: reads it, you keep it
    return name.to_upper()

def add_tag(tags: mut list[str], tag: own str):   # mut: changes yours
    tags.append(tag)                              # own: takes it

def consume(name: own str) -> int64:  # own: it is theirs now
    return name.len()
```

Three capabilities, and that is the whole model:

| Spelling | The callee can | You afterwards |
| --- | --- | --- |
| `name: str` | read it | still own it |
| `name: mut str` | change it in place | still own it, changed |
| `name: own str` | do anything, including keep it | no longer have it |

Calls look like Python — no sigils, no `&`:

```aura
label = "aura"
print(shout(label))
print(shout(label))   # fine, shout only reads
```

Give a value away and the compiler holds you to it:

```aura
n = consume(label)
print(label)
```

```text
error[AU3001]: use of moved value `label`
  = related owner.au:6:17: value moved here
  = help: pass shared access when ownership is not needed, or call `.clone()`
    at the move site when an independent value is required
```

Read that as the compiler asking a question: did you mean to hand it over, or
did you mean to share it? Copy small things freely — numbers, booleans,
durations are copied, not moved. Everything else (strings, collections, class
instances, files) moves.

## Classes Have No `__init__`

An Aura class is fields and methods. There is no initializer, and no `self`
assignment ceremony — you construct with keyword arguments, and fields may
declare defaults:

```aura
class Account:
    owner: str
    balance: float64
    currency: str = "USD"
```

```aura
account = Account(owner="ada", balance=0.0)
```

When you want named construction — the thing `__init__` and `@classmethod`
give you — write a function on the class that takes no `self` and returns one:

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

```aura
fresh = Account.new("ada")
mut acct = Account.opening("grace", 100.0)
```

These are "associated functions": called through the class name, free to
validate, compute, or pick defaults. You can have as many as you need, which
is more than Python gives you without `@classmethod` gymnastics.

### Methods Say What They Do To `self`

The receiver follows the same three capabilities as parameters:

```aura
    def label(self) -> str:              # reads
        return f"{self.owner}: {self.balance} {self.currency}"

    def deposit(mut self, amount: float64):   # modifies
        self.balance += amount

    def into_balance(own self) -> float64:    # consumes
        return self.balance
```

```aura
acct.deposit(25.0)
print(acct.label())
final = acct.into_balance()   # acct is gone after this
```

A bare `self` cannot mutate — the compiler will tell you to write `mut self`.
And a method named `close` is special: it makes the class a managed resource
for `with` blocks, so pick another name unless that is what you want.

### There Is No Inheritance

`class Dog(Animal):` does not parse. Aura uses traits for shared behavior and
composition for shared data — if you reach for a base class, define a trait
with the methods and implement it for each type.

## Failure Is A Return Value

There are no exceptions and no `try`/`except`. A function that can fail says so
in its type:

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

Callers must handle both sides — there is no invisible propagation:

```aura
match parse_port("8080"):
    case Result.Ok(port):
        print(f"listening on {port}")
    case Result.Err(message):
        print(f"bad config: {message}")
```

`Option[T]` plays the role of `None`-or-a-value, and `try` propagates an error
to the caller when your own function returns a `Result`. See
[Results, Options, And `try`](/learn/results-and-options).

## Types Are Static, But Locals Infer

Annotations are required where a contract crosses a boundary — parameters and
return types — and inferred everywhere else:

```aura
def total(prices: list[float64]) -> float64:
    mut sum = 0.0        # inferred float64
    for price in prices:
        sum += price
    return sum
```

Three differences worth knowing up front:

- **A missing parameter type is a parse error**, not a dynamic parameter.
  `def f(x):` does not compile.
- **Numeric types never convert implicitly.** Passing an `int32` where `int64`
  is expected is an error; cast with `as int64` or `.to_float()`. Unsuffixed
  integer literals are `int64`, floats are `float64`.
- **Generics are explicit**: `list[str]`, `dict[str, int64]`, `Option[int64]`,
  and type parameters are declared, as in `def first[T](values: list[T]) -> Option[T]`.

### If It Returns A Value, Declare The Type

A function with no `->` returns nothing. That is fine when the body really
returns nothing, and a bare `return` for an early exit is fine too:

```aura
def greet(name: str):
    print(f"hi {name}")

def early(flag: bool):
    if flag:
        return
    print("no")
```

The moment the body returns a *value*, the signature has to say so. Aura does
not infer it from the body:

```aura
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

Read `-> None` as the default that was there all along. The fix is to write
the type you meant:

```aura
def double(n: int64) -> int64:
    return n * 2
```

Coming from Python this feels like extra typing for about a day, and then it
starts reading as documentation: every signature tells you what goes in and
what comes back without opening the body.

## Things That Will Surprise You

**Integer `/` is rejected.** Python 3 made `/` true division; Aura makes you
choose, because silently truncating is the older bug:

```text
error[AU2003]: integer `/` is not supported; use `//` for floor division,
or call `.to_float()` on both operands for true division
```

**There is no truthiness.** `if values:` fails — conditions are `bool` and
nothing else. Write `if values.len() > 0:` or `if value == None:`.

**Strings are not indexable.** `s[0]` does not work; a `str` is a sequence of
Unicode scalar values, `len()` counts those, and slicing (`s[1:4]`) gives you
an owned copy. Use `s.split("")` style operations or slices instead of
character indexing.

**`is` does not exist.** Use `== None` for optionals; there is no identity
comparison.

**Reading a non-copy element out of a list by index is rejected**, because it
would move a value out of a collection you still own. Use `values.get(index)`,
which hands you an `Option` containing a clone.

**Top-level bindings live in module storage** and cannot be moved out of it.
If you want to consume a value with an `own` method, do it inside a function.

**Module state is immutable.** Constants at module level are fine; `mut` at
module level is not. Mutable state belongs to some owner — usually `main`.

## Where To Go Next

- [Values, Moves, And Borrows](/learn/ownership-and-borrowing) — the ownership
  model in depth, with the errors you will meet and how to fix each one.
- [Shaping Data](/learn/data-modeling) — classes, enums, traits, and methods.
- [Testing](/learn/testing) — `aura test`, assertions that show their values.
- [The Manual](/manual/) — the normative rules when you need the exact
  contract.
