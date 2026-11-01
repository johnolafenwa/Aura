# Generic functions and classes

Let's start with the duplication problem in its plainest form. Here are two
functions that find the largest value in a list, one for integers and one for
strings:

*Filename: src/main.au*

```aura
def largest_int(values: own list[int64]) -> int64:
    mut rest = values
    mut best = rest.pop()
    while rest.len() > 0:
        candidate = rest.pop()
        if candidate > best:
            best = candidate
    return best

def largest_str(values: own list[str]) -> str:
    mut rest = values
    mut best = rest.pop()
    while rest.len() > 0:
        candidate = rest.pop()
        if candidate > best:
            best = candidate
    return best

def main():
    print(largest_int([3, 9, 4]))
    print(largest_str(["pear", "fig", "plum"]))
```

<span class="caption">Listing 10-1: Two functions that differ only in their types</span>

```console
$ aura run src/main.au
9
plum
```

The functions take the list with `own` because they pull values out of it with
`pop`, and the result has to be an owned value we can return. An `own`
parameter can't be mutated in place, so the first line moves it into a `mut`
binding called `rest`. Apart from the types in the signatures, the two bodies
are character for character the same. We'd like to write the body once. We'll
come back to `largest` at the end of this section, because making it generic
turns out to need one more idea. First let's look at a case that works straight
away.

## A generic class

An editor keeps an undo history: a stack of actions, newest on top. A tool that
walks a directory tree keeps a stack of depths. Both are stacks, and the code
that pushes and pops doesn't care what's on the stack. Here is one `Stack`
class for both:

*Filename: src/main.au*

```aura
class Stack[T]:
    items: list[T]

    def push(mut self, item: own T):
        self.items.append(item)

    def pop(mut self) -> T | None:
        if self.items.len() == 0:
            return None
        return self.items.pop()

    def size(self) -> int64:
        return self.items.len()

def main():
    mut undo: Stack[str] = Stack(items=[])
    undo.push("type hello")
    undo.push("delete word")
    print(undo.size())
    print(undo.pop())
    print(undo.pop())
    print(undo.pop())

    mut depths = Stack[int64](items=[])
    depths.push(3)
    print(depths.size())
    print(depths)
```

<span class="caption">Listing 10-2: A `Stack` class with a type parameter `T`</span>

```console
$ aura run src/main.au
2
delete word
type hello
None
1
Stack(items=[3])
```

The `[T]` after the class name declares a *type parameter*. Inside the class,
`T` stands for whatever type a particular stack holds. The field `items` is a
`list[T]`, `push` takes a `T`, and `pop` returns `T | None` so that popping an
empty stack is an ordinary outcome instead of a trap. You could name the
parameter anything, but a single capital letter is the convention, and `T` is
the usual first choice.

`main` creates two stacks in the two ways Aura offers. The first annotates the
binding as `Stack[str]`, and the constructor call takes its type from that
annotation. The second writes the type argument on the constructor itself,
`Stack[int64](items=[])`. Both mean the same thing. What you can't do is leave
the type out entirely when nothing determines it: `Stack(items=[])` with no
annotation gives the compiler an empty list and no way to know what `T` is.
When the fields do determine it, as in `Stack(items=["a", "b"])`, you can leave
the type out and Aura will infer `Stack[str]`.

Once a stack has a type, it holds only that type. If we annotate a
`Stack[int64]` and give it strings, the checker catches the mismatch:

```aura check-fail:AU2002
class Stack[T]:
    items: list[T]

def main():
    s: Stack[int64] = Stack(items=["a"])
    print(s)
```

```console
$ aura check src/main.au
error[AU2002]: list literal elements must all have type `int64`, found `str`
 --> src/main.au:5:36
  |
5 |     s: Stack[int64] = Stack(items=["a"])
  |                                    ^
```

A `Stack[int64]` and a `Stack[str]` are different types, and there's no general
"stack of anything" type that both fit into. Aura's generic arguments are
invariant: a `Stack[int32]` isn't a `Stack[int64]` either, even though every
`int32` value fits in an `int64`.

### Ownership in a generic signature

Look again at `push(mut self, item: own T)`. The `own` is there for a reason,
and the quickest way to see it is to take it away:

```aura check-fail:AU3002
class Stack[T]:
    items: list[T]

    def push(mut self, item: T):
        self.items.append(item)
```

```console
$ aura check src/main.au
error[AU3002]: parameter `item` is borrowed; declare it as `own T` to take ownership
 --> src/main.au:5:27
  |
5 |         self.items.append(item)
  |                           ^
  = related src/main.au:4:24: parameter `item` is borrowed here
  = help: declare the parameter as `own T` when the function should consume this non-cloneable value
```

A bare parameter gives the function shared access, as you learned in [Chapter
4](/book/ch04-00-understanding-ownership). Shared access lets `push` look at
`item` but not keep it, and storing it in the list is keeping it. For a
concrete type like `int64` that wouldn't matter, because integers are copy
values. But Aura checks a generic body once, for every possible `T`, before any
particular `T` is chosen. Some `T`s will be owned values like `str` or a list,
so the body has to be correct for them. The ownership mode you write in a
generic signature is fixed when the function is declared, and it stays the same
for every type the function is used with.

That gives you a simple rule. If a generic function stores, returns, or
otherwise consumes a `T` it receives, the parameter needs `own T`.

## Generic functions

Functions take type parameters the same way, in brackets after the name. Here
are two helpers that work on any stack:

```aura
class Stack[T]:
    items: list[T]

    def push(mut self, item: own T):
        self.items.append(item)

    def pop(mut self) -> T | None:
        if self.items.len() == 0:
            return None
        return self.items.pop()

    def size(self) -> int64:
        return self.items.len()

def pop_or[T](stack: mut Stack[T], fallback: own T) -> T:
    item = stack.pop()
    if item is None:
        return fallback
    return item

def drain[T](stack: mut Stack[T]) -> list[T]:
    mut out: list[T] = []
    while stack.size() > 0:
        item = stack.pop()
        if item is not None:
            out.append(item)
    return out

def main():
    mut undo: Stack[str] = Stack(items=[])
    undo.push("type hello")
    print(pop_or(undo, "nothing to undo"))
    print(pop_or(undo, "nothing to undo"))
    mut depths: Stack[int64] = Stack(items=[])
    depths.push(1)
    depths.push(2)
    print(drain(depths))
    print(depths.size())
```

<span class="caption">Listing 10-3: Generic functions that work on any `Stack[T]`</span>

```console
$ aura run src/main.au
type hello
nothing to undo
[2, 1]
0
```

When we call `pop_or(undo, "nothing to undo")`, the compiler works out `T` from
the arguments: `undo` is a `Stack[str]`, so `T` is `str`, and the fallback must
be a `str` too. We never write `pop_or[str](...)`, although we could.

Inside `pop_or`, the test `item is None` narrows `item`. After the `if`
returns, the compiler knows `item` is a `T` and not `None`, so `return item`
matches the declared return type. `drain` uses the opposite test, `is not
None`, to append only real items. This is the same narrowing you saw for
optional values in [Chapter 6](/book/ch06-00-enums-and-match), and it works on
a type parameter as well as on a concrete type.

Sometimes there's nothing to infer from. Here's a small function that picks a value or a fallback:

```aura
def unwrap_or[T](value: own T | None, fallback: own T) -> T:
    if value is None:
        return fallback
    return value

def main():
    missing: int64 | None = None
    print(unwrap_or(missing, 5))
    print(unwrap_or("x".strip_prefix("y"), "default"))
    print(unwrap_or[str](None, "z"))
```

```console
$ aura run src/main.au
5
default
z
```

The first two calls give the compiler a typed first argument. In the third, a
bare `None` says nothing about `T`, so we write the type argument explicitly
with `unwrap_or[str]`. That bracket syntax is called explicit specialization,
and it works on constructors (`Stack[int64](...)`) and on enum variants
(`Result[int64, str].Ok(7)`) too.

## Generic enums

You've been using generic enums for a while, and Chapter 9 leaned on them
heavily. `Result[T, E]` is an enum with two type parameters, and `Lookup[T]`
has one. Your own enums can take parameters the same way:

```aura
enum Reply[T]:
    Value(T)
    Pending
    Failed(str)

def main():
    first: Reply[int64] = Reply.Value(42)
    second: Reply[int64] = Reply.Failed("timed out")
    for reply in [first, second]:
        match reply:
            case Reply.Value(number):
                print(number + 1)
            case Reply.Pending:
                print("pending")
            case Reply.Failed(reason):
                print(reason)
```

In the `Reply.Value(number)` arm, `number` has type `int64`, because the match
is over a `Reply[int64]`. The compiler substitutes the type argument into each
variant's payload.

## What generic code costs

When you build a native binary, Aura compiles a generic function separately for
each set of type arguments the program uses. `pop_or` called with a
`Stack[str]` and with a `Stack[int64]` becomes two pieces of machine code, each
working directly on its own type. You don't pay at run time for having written
the function once. The cost is in compile time and binary size, which grow a
little with every distinct instantiation. A diagnostic always names the
function as you wrote it, `pop_or`, never a generated copy.

Generic code is also checked against everything it does. If a generic body
copies a value of type `T`, the compiler remembers that `T` must be safe to
copy and checks it at every call site. Most types are, but a random-number
generator isn't, because duplicating it would make two generators produce the
same "random" sequence:

```aura check-fail:AU3007
import random

def snapshot[T](values: list[T]) -> list[T]:
    return values.copy()

def main():
    scores = [3, 9, 4]
    print(snapshot(scores))
    generators = [random.Rng(seed=1)]
    print(snapshot(generators).len())
```

```console
$ aura check src/main.au
error[AU3007]: cannot use function `snapshot` because `random.Rng` contains non-cloneable `random.Rng` state
 --> src/main.au:10:11
   |
10 |     print(snapshot(generators).len())
   |           ^
  = help: move or remove the value so it has one owner, or construct an independent generator with an explicit seed
```

The call with `scores` is fine. The call with a list of generators is rejected
at the call, with a message that says why. You didn't have to write the
requirement down; the compiler inferred it from the `.copy()` in the body. The
[Manual](/manual/generics-and-traits) has the full rules for these inferred
requirements.

## Back to `largest`

Now let's try to merge the two functions from Listing 10-1 into one:

```aura check-fail:AU2999
def largest[T](values: own list[T]) -> T:
    mut rest = values
    mut best = rest.pop()
    while rest.len() > 0:
        candidate = rest.pop()
        if candidate > best:
            best = candidate
    return best

def main():
    print(largest([3, 9, 4]))
```

<span class="caption">Listing 10-4: A generic `largest` that doesn't compile yet</span>

```console
$ aura check src/main.au
error[AU2999]: unsupported operands for binary expression: `T`
 --> src/main.au:6:12
  |
6 |         if candidate > best:
  |            ^
```

The compiler points at the `>`. Everything else in the body works for any `T`:
popping, comparing lengths, assigning. But `>` isn't defined for every type:
there's no obvious meaning for comparing two stacks, or two settings files.
Because the body is checked once for all possible `T`s, and some `T`s have no
`>`, the body is rejected, even though our one call uses integers.

Python would run this and fail at run time on the first type without `>`. Aura
wants the requirement in the signature, so that a caller can see from the
declaration alone what `largest` needs from `T`, and so that the error lands at
the call that passes an unsuitable type instead of deep inside the function.
The way to state "T must support `>`" is a trait bound, and traits are the
subject of the next section.
