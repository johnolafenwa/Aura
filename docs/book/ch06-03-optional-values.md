# `T | None` and concise matching

Some functions don't always have an answer. Searching a shelf for a title
might find it or might not. Stripping a prefix from a string only works if
the prefix is there. In Python, such a function returns `None` when it has
nothing to give, and the caller is supposed to check. When the caller
forgets, the program carries on with a `None` until something tries to use
it, and then fails with a `TypeError` or `AttributeError` somewhere else.
Tony Hoare, who introduced null references in 1965, later called them his
"billion-dollar mistake", and this is the reason.

Aura keeps `None`, but it puts the possibility in the type. A function that
might not return a title has the return type `str | None`, read "a `str` or
`None`". The `|` makes a *union* type, and a union with `None` is how Aura
spells an optional value. You can't use a `str | None` as a `str` until you've
checked which one it is, and the compiler makes sure you do.

## A function that might find nothing

Here is a search over a list of titles that returns the position of a match,
or `None` if there isn't one:

*Filename: src/main.au*

```aura
def find(titles: list[str], wanted: str) -> int64 | None:
    mut index: int64 = 0
    for title in titles:
        if title == wanted:
            return index
        index += 1
    return None

def main():
    titles = ["Dune", "Emma", "Beloved"]
    print(find(titles, "Emma"))
    print(find(titles, "Ulysses"))
```

<span class="caption">Listing 6-10: A search that returns `int64 | None`</span>

```console
$ aura run src/main.au
1
None
```

Both `return index` and `return None` are allowed, because each one is a
member of the declared return type. You don't wrap the integer in anything:
an `int64` is already an `int64 | None`.

If you know Python's type hints, this is the same idea as
`Optional[int]`, which Python also lets you write as `int | None`. The
difference is that Python's hints are advice for tools like mypy, and the
interpreter ignores them. In Aura the type is checked every time you build.

## Using an optional value

Let's try to use the result as if it were a number:

```aura check-fail:AU2003
def find(titles: list[str], wanted: str) -> int64 | None:
    mut index: int64 = 0
    for title in titles:
        if title == wanted:
            return index
        index += 1
    return None

def main():
    titles = ["Dune", "Emma", "Beloved"]
    position = find(titles, "Emma")
    print(position + 1)
```

```console
$ aura check src/main.au
error[AU2003]: operator `+` is not supported for union `int64 | None`: unions have no ordering or arithmetic
 --> src/main.au:12:11
   |
12 |     print(position + 1)
   |           ^
  = help: select one member with a type pattern before comparing or computing
```

`position` has the type `int64 | None`, and there is no sensible meaning for
`None + 1`. So the compiler refuses to add anything to the union as a whole.
The same thing happens when you pass an optional value to a function that
wants the plain type. `strip_prefix` on a string returns `str | None`, since
the prefix may not be there:

```aura check-fail:AU2002
def shout(text: str) -> str:
    return text.to_upper()

def main():
    rest = "finish Dune".strip_prefix("finish ")
    print(shout(rest))
```

```console
$ aura check src/main.au
error[AU2002]: argument type mismatch for function `shout`: expected `str`, found `str | None`
 --> src/main.au:6:17
  |
6 |     print(shout(rest))
  |                 ^
```

We know the prefix is there in this case, but the compiler checks types, not
the contents of string literals. In real code the string would come from a
user, and the check would be the only thing standing between a typo and a
crash.

The help line in the first error says to "select one member". There are two
ways to do that.

## Checking with `is None`

The most direct is an `if` that tests for `None`. Inside each branch, the
compiler knows which member it has:

*Filename: src/main.au*

```aura
def find(titles: list[str], wanted: str) -> int64 | None:
    mut index: int64 = 0
    for title in titles:
        if title == wanted:
            return index
        index += 1
    return None

def report(titles: list[str], wanted: str):
    position = find(titles, wanted)
    if position is None:
        print(f"{wanted} is not on the shelf")
        return
    print(f"{wanted} is book number {position + 1}")

def main():
    titles = ["Dune", "Emma", "Beloved"]
    report(titles, "Emma")
    report(titles, "Ulysses")
    match find(titles, "Beloved"):
        case int64 as index:
            print(f"found at index {index}")
        case None:
            print("missing")
```

<span class="caption">Listing 6-11: Narrowing with `is None`, and matching on a union member</span>

```console
$ aura run src/main.au
Emma is book number 2
Ulysses is not on the shelf
found at index 2
```

After `if position is None: ... return`, the rest of `report` can only run
when `position` holds an integer, and the compiler knows it. From that line
on, `position` has the type `int64`, so `position + 1` is fine. This is
called *narrowing*. It works with `is None` and `is not None`, inside the
`if` and `else` branches, after an early `return` (or `continue` or `break`
in a loop), and through `and`, `or`, and `not`.

Narrowing applies to a binding you can name: a local, a parameter, or a field
of one of those. A function call like `find(titles, wanted)` gives a new
value every time it runs, so to narrow its result, bind it to a name first,
as `report` does.

Narrowing lasts only as long as the compiler can be sure of it. Assign a new
value to the binding and the fact goes away, even inside the branch that
tested it:

```aura check-fail:AU2003
def main():
    mut position: int64 | None = 3
    if position is not None:
        position = None
        print(position + 1)
```

```console
$ aura check src/main.au
error[AU2003]: operator `+` is not supported for union `int64 | None`: unions have no ordering or arithmetic
 --> src/main.au:5:15
  |
5 |         print(position + 1)
  |               ^
  = help: select one member with a type pattern before comparing or computing
```

Only `is None` and `is not None` narrow. `position == None` is an ordinary
comparison that returns a `bool` and tells the compiler nothing.

## Matching on a union member

The `match` at the end of Listing 6-11 is the other way. A union is matched
by the type of its members: `case int64 as index:` matches when the value is
an integer and binds it as `index`, and `case None:` matches the absence. As
with enums, the arms must cover every member, so a `match` over
`int64 | None` without a `None` arm doesn't compile.

Which should you use? `is None` with an early return reads best when the
missing case is a short detour, like printing a message and stopping, and the
rest of the function works on the value. `match` reads best when both cases
do real work, or when the union has more than two members, as in
`int64 | str | None`.

## No `Option`, no `T?`

If you've used Rust, Swift, or Kotlin, you might reach for `Option[str]` or
`str?`. Neither exists in Aura:

```aura check-fail:AU2001
def main():
    nickname: Option[str] = None
```

```console
$ aura check src/main.au
error[AU2001]: unknown type `Option`
 --> src/main.au:2:15
  |
2 |     nickname: Option[str] = None
  |               ^
```

```aura check-fail:AU1101
def main():
    nickname: str? = None
```

```console
$ aura check src/main.au
error[AU1101]: expected Newline
 --> src/main.au:2:13
  |
2 |     nickname: str? = None
  |             ^
```

Aura uses the union because it says exactly what the value can be, in the
words a Python programmer already uses, and because it needs no wrapping: a
`str` is a valid `str | None` as it stands, with no `Some(...)` around it.

There is one place where Aura does use an enum for "maybe missing". Looking
up a position in a list with `titles.get(5)` returns `Lookup[str]`, with the
variants `Lookup.Found(value)` and `Lookup.Missing`. That keeps "there is no
entry" separate from "the entry is `None`" in a collection that stores
optional values. We'll meet `Lookup` properly in
[Chapter 8](/book/ch08-00-collections).

## Putting it together

Let's finish the chapter by parsing commands for the reading tracker. The
parser takes a line of text and returns a `Command`, or `None` if it can't
make sense of the line. The main loop then uses everything from this
chapter: narrowing to skip lines that didn't parse, a `match` over the
command's variants, and a `match` over the result of `find`:

*Filename: src/main.au*

```aura
enum Command:
    Finish(str)
    List
    Quit

def find(titles: list[str], wanted: str) -> int64 | None:
    mut index: int64 = 0
    for title in titles:
        if title == wanted:
            return index
        index += 1
    return None

def parse_command(line: str) -> Command | None:
    words = line.trim()
    match words:
        case "list":
            return Command.List
        case "quit":
            return Command.Quit
        case _:
            pass
    title = words.strip_prefix("finish ")
    if title is not None:
        return Command.Finish(title)
    return None

def main():
    titles = ["Dune", "Emma", "Beloved"]
    for line in ["list", "finish Emma", "finish Ulysses", "dance", " quit "]:
        command = parse_command(line)
        if command is None:
            print(f"I don't understand '{line}'")
            continue
        match command:
            case Command.Finish(title):
                match find(titles, title):
                    case int64 as index:
                        print(f"finished book {index + 1}: {title}")
                    case None:
                        print(f"{title} is not on the shelf")
            case Command.List:
                print(titles)
            case Command.Quit:
                print("bye")
```

<span class="caption">Listing 6-12: Parsing and running tracker commands</span>

```console
$ aura run src/main.au
[Dune, Emma, Beloved]
finished book 2: Emma
Ulysses is not on the shelf
I don't understand 'dance'
bye
```

`parse_command` handles the one-word commands with a literal `match`, and
the `case _: pass` arm lets every other line fall through to the code after
it. `strip_prefix("finish ")` returns the rest of the line when it starts
with `finish `, and `None` otherwise. Inside the `if title is not None`
branch, `title` is a `str`, and because it is an owned local, it can move
into `Command.Finish`.

In `main`, `if command is None: ... continue` narrows `command` to a plain
`Command` for the rest of the loop body, so the `match` that follows only
needs arms for the three variants. Each place where a value might be
missing is visible in a type, and each one is handled before the value is
used. The [Manual](/manual/enums-and-match) has the full narrowing and union
matching rules.

## Summary

Enums let you say that a value is exactly one of several shapes, each with
its own data, so that combinations that make no sense can't be built. `match`
takes them apart: it binds payloads to names, tests literals, refines arms
with guards, and combines patterns with `|`, and the compiler checks that
every variant is handled. Like function parameters, a `match` borrows by
default, with `match own` and `match mut` when an arm needs to take or change
the payload. For values that might be missing, `T | None` puts the
possibility in the type, and `is None` or a type pattern selects the member
before you can use it.

Our programs are getting long enough to want more than one file. In the next
chapter we'll split code into modules, decide what each module exposes, and
organize a growing program into a package.
