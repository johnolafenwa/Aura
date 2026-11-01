# Bindings and mutability

A program needs somewhere to keep the values it is working with. In Aura you
give a value a name by writing `name = value`, exactly as you would in Python.
We call that a *binding*: the name is bound to the value. There is no `let`,
`var`, or other keyword in front of it.

To follow along with this chapter, make a scratch package with
`aura new concepts`, and put each example in its `src/main.au`. Then run it
with `aura run src/main.au`.

## Bindings are immutable by default

Here is a small program that binds a name, prints it, and then tries to give
it a new value:

*Filename: src/main.au*

```aura check-fail:AU3003
def main():
    x = 5
    print(f"x is {x}")
    x = 6
    print(f"x is {x}")
```

<span class="caption">Listing 3-1: Trying to assign a second value to an immutable binding</span>

In Python this prints two lines. In Aura it doesn't run at all:

```console
$ aura run src/main.au
error[AU3003]: cannot assign to immutable binding `x`; declare it with `mut` to rebind it
 --> src/main.au:4:5
  |
4 |     x = 6
  |     ^
```

You saw this error in the word counter, and you will see it again, so it's
worth reading closely once. The compiler points at line 4, where we assign
`6` to `x`. It says `x` is an *immutable binding*: when a binding is created
with plain `name = value`, it keeps that value for as long as it exists. The
message also tells us the fix: if we want to change `x` later, we have to say
so when we create it, with `mut`.

*Filename: src/main.au*

```aura
def main():
    mut x = 5
    print(f"x is {x}")
    x = 6
    print(f"x is {x}")
```

<span class="caption">Listing 3-2: A mutable binding can be assigned again</span>

```console
$ aura run src/main.au
x is 5
x is 6
```

`mut` goes in front of the binding the first time it appears. After that,
`x = 6` and the compound forms such as `x += 1` work as you would expect.
Aura has the usual compound assignments: `+=`, `-=`, `*=`, `//=`, `%=`, and
`**=`, plus `/=` for floating-point numbers.

Why make immutability the default? When you read a function, most of the
bindings in it are set once and never touched again. In a language where any
name can be reassigned at any point, you can't know that without reading
every line between where a value is set and where it is used. In Aura, a
binding without `mut` is a promise that its value never changes, and the
compiler holds you to it. That makes code easier to read, and it makes it
safer to change: if you rearrange a function and accidentally reassign
something that other code relies on, the compiler tells you. The bindings that
do change carry a `mut` that says so, which is exactly where you want your
attention to go when something behaves unexpectedly.

The cost is a few extra `mut` keywords, and the error above the first few
times you forget one. Most people find that it fades quickly, and that the
`mut` marks end up being useful information.

## `mut` covers changes inside a value too

`mut` isn't only about pointing a name at a new value. It also controls
whether you may change the value the binding holds. A list is a good example:

```aura check-fail:AU3003
def main():
    guests = ["Ada", "Grace"]
    guests.append("Linus")
    print(guests)
```

```console
$ aura run src/main.au
error[AU3003]: method `append` requires a mutable receiver
 --> src/main.au:3:12
  |
3 |     guests.append("Linus")
  |            ^
  = help: declare the receiver place with `mut` before calling a mutating method
```

`append` changes the list it is called on, which the message calls the
*receiver*. Because `guests` is immutable, its list can't be changed either.
The `help` line says what to do, and with `mut guests = ["Ada", "Grace"]` the
program prints `[Ada, Grace, Linus]`.

This is a real difference from Python, where nothing is fixed by default:
any name can be rebound, and any list can be changed through any name that
refers to it. In Python you can pass a list to a function and
have no idea, from the call, whether it comes back changed. In Aura an
immutable binding means the whole value is frozen, contents and all. When you
need a value that changes, you say so once, and anyone reading the code can
see it. Functions follow the same idea through their parameters, as the
`counts: mut dict[str, int64]` parameter in the word counter showed. We'll
look at that properly in
[Chapter 4](/book/ch04-00-understanding-ownership).

## A binding keeps its type

Aura works out the type of a binding from the value you give it. `x = 5`
makes `x` an `int64`, Aura's default integer type, and `name = "Ada"` makes
`name` a `str`. That type is fixed for the life of the binding, even when the
binding is `mut`:

```aura check-fail:AU2002
def main():
    mut attempts = 0
    attempts = "three"
```

```console
$ aura run src/main.au
error[AU2002]: cannot assign value of type `str` to `attempts` of type `int64`
 --> src/main.au:3:5
  |
3 |     attempts = "three"
  |     ^
```

`mut` lets you replace the value, but the new value has to be the same type
as the old one. This is where Aura differs from Python most visibly. A Python
name can hold an integer on one line and a string on the next, and code
further down has to cope with either. Aura decides each binding's type once,
which is what lets it check every use of that binding before the program
runs.

Some languages let you reuse a name for a new binding in the same scope, which
is called *shadowing*. Aura doesn't. Writing `x = ...` a second time in the
same block always means assignment to the existing `x`, which is why
Listing 3-1 was an error rather than a new binding. If you need a value of a
different type, give it a different name, such as `attempts_text`.

## Writing the type yourself

You can write a binding's type after its name, separated by a colon:

```aura
def main():
    count: int64 = 3
    ratio: float64 = 0.25
    mut names: list[str] = []
    names.append("Ada")
    print(names)
```

Most of the time you don't need to. The type of `count` would have been
`int64` anyway. There are two situations where you do. The first is when the
value doesn't say enough. An empty list literal `[]` could be a list of
anything, so the compiler needs to be told what will go in it:

```aura check-fail:AU2002
def main():
    empty = []
```

```console
$ aura run src/main.au
error[AU2002]: empty list literals require an expected `list[T]` type annotation in the bootstrap compiler
 --> src/main.au:2:13
  |
2 |     empty = []
  |             ^
```

`names: list[str] = []` above is the fix, and the same goes for an empty
dictionary, as in `counts: dict[str, int64] = {}` from Chapter 2. (The
"bootstrap compiler" in the message is how the current preview compiler
refers to itself.)

The second is when you want a type other than the default. `count: int32 = 3`
makes `count` a 32-bit integer instead of a 64-bit one. We'll see in the
[next section](/book/ch03-02-data-types) why that choice sticks, and why
Aura won't mix the two.

Every binding needs a value when it is created. There is no way to declare a
name now and give it a value later, which means there is no such thing in Aura
as reading a binding that was never set.

## Blocks and scope

A binding exists from the line that creates it to the end of the block it was
created in. A block is an indented body: a function body, or the body of an
`if`, `else`, `while`, or `for`. Here is a pattern that is common in Python:

```aura check-fail:AU2001
def main():
    raining = true
    if raining:
        advice = "take an umbrella"
    else:
        advice = "leave it at home"
    print(advice)
```

```console
$ aura run src/main.au
error[AU2001]: unknown name `advice`
 --> src/main.au:7:11
  |
7 |     print(advice)
  |           ^
```

In Python, a name assigned inside an `if` is visible after it, as long as the
branch that set it actually ran. In Aura each branch is its own block, and the
two `advice` bindings end when their blocks end. By line 7 neither exists, so
the compiler reports an unknown name.

Aura scopes bindings to blocks because the alternative needs a run-time check
that can fail. If one branch of an `if` forgot to set `advice`, Python would
only find out when that branch ran and the `print` raised a `NameError`. Aura
wants every name to be known to exist wherever it is used, at compile time,
and tying bindings to blocks is the simplest rule that makes that possible.
Block scope also matters for cleanup, which we'll see in
[Chapter 4](/book/ch04-00-understanding-ownership): when a block ends, the
values it owned are cleaned up right there.

There are two good ways to write this. When each branch only chooses a
value, use a *conditional expression*, which has the same syntax as
Python's:

```aura
def main():
    raining = true
    advice = "take an umbrella" if raining else "leave it at home"
    print(advice)
```

When the branches need to do more work, create a `mut` binding with a
default before the `if`, and assign to it inside:

```aura
def main():
    raining = false
    mut advice = "leave it at home"
    if raining:
        advice = "take an umbrella"
    print(advice)
```

Because `advice` belongs to the function's block, it is still alive at the
`print`.

## Module constants

Bindings don't have to be inside a function. A binding at the top level of a
file, next to your `def`s, is a *module constant*:

```aura
FREEZING_F = 32.0

def main():
    print(FREEZING_F)
```

Every function in the module can read it. As the name says, it can't be
changed, not even from inside a function:

```aura check-fail:AU3003
FREEZING_F = 32.0

def main():
    FREEZING_F = 0.0
```

```console
$ aura run src/main.au
error[AU3003]: module constant `FREEZING_F` is immutable
 --> src/main.au:4:5
  |
4 |     FREEZING_F = 0.0
  |     ^
  = help: put mutable state in a local value owned by `main` or another explicit owner
```

Notice that Aura did not treat `FREEZING_F = 0.0` inside `main` as a new local
binding, which is what Python would do. Inside a function, a name that already
refers to something visible is assigned, not redeclared. And you can't make a
module-level binding `mut` either; `mut visits = 0` at the top level of a file
with a `main` is rejected with "module bindings are immutable".

The `help` line explains the design. Global mutable state is one of the most
common sources of hard-to-find bugs: any function, anywhere, might change it,
and when concurrent tasks share it, two of them can change it at once. Aura
doesn't allow it. If several functions need to update a value, one of them
owns it (usually `main`) and passes it to the others, which is exactly what
the word counter did with its dictionary. Module constants are for values
that really are constant, like `TOP` in Chapter 2 or a conversion factor.

The rule is slightly different in a script, a file with no `main`. There the
top-level statements are the program, so a top-level `mut count = 0` is an
ordinary local binding of that program, and later lines can change it. A
plain top-level binding in a script is still a module constant.

Constant names in capitals, as in `FREEZING_F`, are a convention, not a rule.
Other bindings, like function names, use `snake_case`: lowercase words joined
by underscores.

## Copying numbers

One last thing to notice before we move on. Binding one name to another works
the way you would hope for numbers:

```aura
def main():
    a = 5
    mut b = a
    b += 1
    print(f"a = {a}, b = {b}")
```

```console
$ aura run src/main.au
a = 5, b = 6
```

`b = a` copied the number, so changing `b` left `a` alone. Numbers, booleans,
and a few other small types are *copy values*, which behave like this
everywhere. Strings, lists, dictionaries, and most other types are *owned
values*, and binding one to a new name moves it rather than copying it.
That's the subject of
[Chapter 4](/book/ch04-00-understanding-ownership), and it's why the word
counter's dictionary needed a string of its own. For now, let's look at the
types themselves.
