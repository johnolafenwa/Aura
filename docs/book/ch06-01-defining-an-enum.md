# Defining an enum

Let's give each book in the tracker a status. The quick way is a string
field, `status: str`, holding `"to-read"`, `"reading"`, or `"finished"`. That
works until someone writes `"Reading"` or `"finshed"`. The compiler sees a
`str` either way, and the mistake waits for whatever code compares
`status == "finished"` and quietly gets `false`.

An enum lists the possibilities once, as part of the type:

*Filename: src/main.au*

```aura
enum Status:
    ToRead
    Reading
    Finished

def main():
    status = Status.Reading
    print(status)
    other = status
    print(status == other)
```

<span class="caption">Listing 6-1: An enum with three variants</span>

```console
$ aura run src/main.au
Status.Reading
true
```

The declaration starts with `enum` and the type's name, and the indented
body lists the *variants*, one per line, in `CamelCase`. A `Status` value is
always exactly one of them. You name a variant through its enum, as in
`Status.Reading`, so variants from different enums never collide. Printing a
variant shows that same qualified name.

Now a typo is a compile error. `Status.Finshed` isn't a variant of `Status`,
and the compiler says so on the line where it appears, instead of the
program carrying a bad string around until something trips over it. And as
we'll see in the next section, a `match` over a `Status` has to say what to
do with each of the three variants, so adding a fourth later shows you every
place that needs updating.

Notice also that `other = status` didn't move `status`: we compared the two
afterwards. An enum whose variants carry no data, or only copy values, is a
copy value itself. It is as cheap to duplicate as an integer.

## Variants that carry data

A status is only a label. A command for the tracker needs more. "Read" needs
a book title and a page count, "finish" needs a title, and "list" and "quit"
need nothing at all. Each variant of an enum can carry its own *payload*:

*Filename: src/main.au*

```aura
enum Command:
    Read(title: str, pages: int64)
    Finish(str)
    List
    Quit

def main():
    first = Command.Read(title="Dune", pages=50)
    second = Command.Finish("Dune")
    third = Command.List
    print(first)
    print(second)
    print(third)
```

<span class="caption">Listing 6-2: A `Command` enum whose variants carry different data</span>

```console
$ aura run src/main.au
Command.Read(Dune, 50)
Command.Finish(Dune)
Command.List
```

There are three variant shapes here. `Read` has named payloads, written like
fields, `title: str` and `pages: int64`. `Finish` has one positional payload,
written as only its type. `List` and `Quit` have no payload, and you write
them without parentheses, both in the declaration and when you use them.
A payload variant is built by calling it like a function, with named or
positional arguments to match its declaration. Use names when a variant
has more than one payload: `Command.Read(title="Dune", pages=50)` can't be
misread the way `Command.Read("Dune", 50)` might be once the variant grows a
third field.

The payload slots are owned, the same as constructor fields on a class.
`Command.Finish(title)` moves the string in `title` into the command. And
because `Command` has variants that hold a `str`, the whole enum is an owned
value, so assignment moves it:

```aura check-fail:AU3001
enum Command:
    Read(title: str, pages: int64)
    Finish(str)
    List
    Quit

def main():
    first = Command.Read(title="Dune", pages=50)
    copied = first
    print(first)
```

```console
$ aura check src/main.au
error[AU3001]: use of moved value `first`
 --> src/main.au:10:11
   |
10 |     print(first)
   |           ^
  = related src/main.au:9:14: value moved here
  = help: pass shared access when ownership is not needed, or transfer this non-cloneable value only once
```

The compiler decides this from the declaration, not from the variant you
happen to hold. Even `Command.List`, which carries nothing, is a `Command`,
and `Command` is an owned type because some of its variants own strings.

## Why not a class?

You could try to model commands with a class instead:

```aura
class CommandRecord:
    kind: str
    title: str = ""
    pages: int64 = 0
```

Every `CommandRecord` has all three fields, whatever its `kind`, so the type
happily represents a "quit" command with a title, a "read" command with no
title, and a `kind` of `"raed"`. Every function that receives a
`CommandRecord` has to check for those cases or hope they never happen. This
is the shape of a lot of Python code, where a class has a handful of
attributes that are `None` except in some situations, and the knowledge of
which situations lives in comments.

The enum version can't express any of those. A `Command.Quit` has no title
because the variant has no payload. A `Command.Read` always has both a title
and a page count, because you can't construct it without them. The rule of
thumb is short: use a class when every field is present at once, and an enum
when exactly one of several shapes is present. Often you'll use both, a class
that holds an enum field, such as a `Book` with a `status: Status`.

## Recursive enums

A variant's payload can be the enum itself. That lets an enum describe
nested structure, like an arithmetic expression where each operand is
another expression:

*Filename: src/main.au*

```aura
enum Expr:
    Number(int64)
    Add(Expr, Expr)
    Multiply(Expr, Expr)

def main():
    expr = Expr.Add(Expr.Number(2), Expr.Multiply(Expr.Number(3), Expr.Number(4)))
    print(expr)
```

<span class="caption">Listing 6-3: An enum for arithmetic expressions, with variants that contain other expressions</span>

```console
$ aura run src/main.au
Expr.Add(Expr.Number(2), Expr.Multiply(Expr.Number(3), Expr.Number(4)))
```

This value is the expression `2 + 3 * 4`, already grouped: an `Add` whose
right-hand side is a `Multiply`. Each `Expr` owns its sub-expressions, so the
whole tree has one owner, `expr`, and is released as one when `expr` goes out
of scope. In the next section we'll write a function that walks this tree and
computes 14.

Classes are stricter about recursion. A class field whose type contains the
class itself has to be marked `indirect`, which tells the compiler to store
that field out of line so the class has a fixed size:

```aura check-fail:AU2999
class Chapter:
    title: str
    next: Chapter | None = None
```

```console
$ aura check src/main.au
error[AU2999]: recursive field `next` on class `Chapter` requires `indirect`
 --> src/main.au:3:5
  |
3 |     next: Chapter | None = None
  |     ^
```

Writing `next: indirect Chapter | None = None` fixes it. Enum payloads, as
Listing 6-3 shows, can refer to their own enum without the marker. The
[Manual](/manual/enums-and-match) has the complete rules for declaring
enums, including generic enums such as `Result[T, E]`.

An enum on its own only holds data. To do anything with a `Command` or an
`Expr`, we need to find out which variant we have and get at its payload.
That is the job of `match`.
