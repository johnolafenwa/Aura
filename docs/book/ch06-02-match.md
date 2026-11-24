# The `match` control flow construct

`match` compares a value against a series of *patterns* and runs the code
for the first one that fits. If you've used Python 3.10's `match` statement,
the shape will look familiar. The difference that matters is that Aura checks
your patterns at compile time: a `match` over an enum must handle every
variant, or the program doesn't compile.

Let's start with the `Status` enum from the previous section and turn each
variant into a label:

*Filename: src/main.au*

```aura
enum Status:
    ToRead
    Reading
    Finished

def label(status: Status) -> str:
    match status:
        case Status.ToRead:
            return "on the pile"
        case Status.Reading:
            return "in progress"
        case Status.Finished:
            return "done"

def short(status: Status) -> str:
    return match status:
        case ToRead: "-"
        case Reading: "~"
        case Finished: "x"

def main():
    for status in [Status.ToRead, Status.Reading, Status.Finished]:
        print(f"{short(status)} {label(status)}")
```

<span class="caption">Listing 6-4: Two ways to match on a `Status`</span>

```console
$ aura run src/main.au
- on the pile
~ in progress
x done
```

The value after `match` is the *scrutinee*, here `status`. Each `case` line
holds a pattern followed by a colon. Aura evaluates the scrutinee once, tries
the patterns from top to bottom, and runs the body of the first one that
matches. No other arm runs.

`label` uses the *statement* form. Each arm has an indented block of
statements, the same as an `if` branch, and here each block returns.
`short` uses the *expression* form: `return match status:` makes the whole
`match` produce a value, and each arm is a single expression. When the arms
are short, you can put the expression on the same line as the pattern. Every
arm of a `match` expression has to produce the same type.

`short` also leaves the enum name off its patterns, writing `ToRead` instead
of `Status.ToRead`. That works whenever the compiler already knows the
scrutinee's type, as it does here from the parameter. The qualified form
always works and is clearer when there's any doubt.

## Matches are exhaustive

Let's forget an arm:

```aura check-fail:AU2999
enum Status:
    ToRead
    Reading
    Finished

def label(status: Status) -> str:
    match status:
        case Status.ToRead:
            return "on the pile"
        case Status.Reading:
            return "in progress"
```

<span class="caption">Listing 6-5: A `match` that doesn't cover `Status.Finished`</span>

```console
$ aura check src/main.au
error[AU2999]: non-exhaustive match over `Status`: missing `Finished`
 --> src/main.au:7:5
  |
7 |     match status:
  |     ^
```

The compiler names the variant we left out. This check is what makes enums
worth the trouble. In Python, an `if`/`elif` chain over status strings that
forgets `"finished"` falls off the end and returns `None`, and a Python
`match` with a missing case does nothing at all. Both fail silently, at run
time, on whichever input you didn't test. In Aura the gap is a compile error.
Better still, when you add a variant to an enum later, every `match` that
doesn't handle it stops compiling, and the error list is a to-do list of the
places that need thought.

When you really do want to treat the remaining variants the same way, end the
`match` with `case _:`. The `_` pattern matches anything and binds nothing:

```aura fragment
match status:
    case Status.Finished:
        print("done")
    case _:
        print("not done yet")
```

Use `_` deliberately. It also matches variants that don't exist yet, so a
`match` that ends in `_` won't warn you when the enum grows.

## Binding payloads

Patterns can do more than name a variant. When a variant carries data, the
pattern can bind that data to names for the arm to use. Here is a function
that describes each `Command`:

*Filename: src/main.au*

```aura
enum Command:
    Read(title: str, pages: int64)
    Finish(str)
    List
    Quit

def describe(command: Command) -> str:
    return match command:
        case Command.Read(title, pages) if pages >= 100:
            f"a long session: {pages} pages of {title}"
        case Command.Read(title, pages):
            f"read {pages} pages of {title}"
        case Command.Finish(title):
            f"finish {title}"
        case Command.List | Command.Quit:
            "no book needed"

def main():
    commands = [
        Command.Read(title="Dune", pages=40),
        Command.Read(title="Emma", pages=120),
        Command.Finish("Beloved"),
        Command.List,
        Command.Quit,
    ]
    for command in commands:
        print(describe(command))
```

<span class="caption">Listing 6-6: Binding payloads, a guard, and an or-pattern</span>

```console
$ aura run src/main.au
read 40 pages of Dune
a long session: 120 pages of Emma
finish Beloved
no book needed
no book needed
```

In `case Command.Read(title, pages):`, the names `title` and `pages` aren't
values to compare against. They're new bindings that receive the payload,
in the order the variant declares it, and they exist only inside that arm.
`Command.Finish(title)` binds the one string a `Finish` carries. The payload
names you use in a pattern don't have to match the names in the declaration,
but matching them keeps the code easy to follow.

Listing 6-6 also shows two more pattern features.

The first arm has a *guard*: `if pages >= 100` after the pattern. The arm
runs only if the pattern matches and the guard is true. If the guard is
false, matching moves on to the next arm, which is why the 40-page command
falls through to the plain `Command.Read` arm. A guard can be any `bool`
expression, and it can use the names the pattern bound.

The last arm joins two patterns with `|`, so it matches either a `List` or a
`Quit`. Each alternative in an or-pattern has to bind the same names, which
here is none.

Guards don't count towards exhaustiveness, because the compiler can't know
in general whether a condition will be true. If we deleted the unguarded
`Command.Read` arm, the compiler would reject the `match`: a `Read` with
fewer than 100 pages would have nowhere to go.

## Matching literal values

A pattern can also be a literal: an integer, a `bool`, a string, or a float.
That makes `match` a natural way to dispatch on words typed by a user:

*Filename: src/main.au*

```aura
def parse_command(line: str) -> str:
    return match line:
        case "list": "List"
        case "quit" | "exit": "Quit"
        case other: f"unknown: {other}"

def main():
    print(parse_command("list"))
    print(parse_command("exit"))
    print(parse_command("dance"))
```

<span class="caption">Listing 6-7: Matching string literals, with a catch-all binding</span>

```console
$ aura run src/main.au
List
Quit
unknown: dance
```

The last arm's pattern is a plain lowercase name, `other`. A name pattern
matches anything and binds the whole scrutinee, so it works like `_` except
that the arm can use the value. Like `_`, it has to be the last arm.

There are infinitely many strings and integers, so a literal `match` can
never list them all. The compiler insists on a catch-all arm, and names the
exact fix:

```aura check-fail:AU2999
def describe(count: int64) -> str:
    match count:
        case 0:
            return "none"
        case 1:
            return "one"
```

```console
$ aura check src/main.au
error[AU2999]: `match` over `int64` with literal patterns requires a final `case _:` arm
 --> src/main.au:2:5
  |
2 |     match count:
  |     ^
```

A `bool` is the exception: `case true:` and `case false:` together cover
every value, so no catch-all is needed.

## Matching and ownership

A `match` reads its scrutinee the same way a function with a bare parameter
does: with shared access. The payload bindings are borrowed from the value
being matched, so you can read them but not take them. Let's see what
happens when an arm tries to keep a title:

```aura check-fail:AU3002
enum Command:
    Read(title: str, pages: int64)
    Finish(str)
    List
    Quit

def finished_title(command: Command, done: mut list[str]):
    match command:
        case Command.Finish(title):
            done.append(title)
        case _:
            pass

def main():
    mut done = list[str]()
    finished_title(Command.Finish("Beloved"), done)
    print(done)
```

```console
$ aura check src/main.au
error[AU3002]: cannot move `title` out of a shared match on `command`
 --> src/main.au:10:25
   |
10 |             done.append(title)
   |                         ^
  = related src/main.au:9:29: value is borrowed here
  = help: write `match own command` to consume the scrutinee, or call `.clone()` to consume an independent copy
  = fix: replace src/main.au:10:30-10:30 with `.clone()`
```

`append` takes ownership of what it stores. The `title` binding only borrows
the string inside `command`, which still belongs to whoever owns `command`.
This is the same rule as moving a field out of a borrowed class in
[Chapter 4](/book/ch04-02-shared-and-mutable-access), and the help line gives
the same two fixes. Cloning keeps the command intact. Writing `match own`
consumes the command and hands each arm owned payloads, which needs the
function to own `command` in the first place:

*Filename: src/main.au*

```aura
enum Command:
    Read(title: str, pages: int64)
    Finish(str)
    List
    Quit

def record_finish(command: own Command, done: mut list[str]):
    match own command:
        case Command.Finish(title):
            done.append(title)
        case _:
            pass

def main():
    mut done = list[str]()
    record_finish(Command.Finish("Beloved"), done)
    record_finish(Command.List, done)
    print(done)
```

<span class="caption">Listing 6-8: `match own` gives the arm ownership of the payload</span>

```console
$ aura run src/main.au
[Beloved]
```

`match own` uses up the command, and the string moves from the command into
the list without being copied. There is also `match mut`, which lends the
arms mutable access to the payloads so they can change them in place. The
pattern is the one you've seen for parameters and loops: bare for reading,
`mut` for changing, `own` for taking. Copy payloads, like the `pages` count,
are copied into their bindings in every form, so they never cause these
errors.

## Proving every arm

The compiler checks that a `match` has an arm for every variant. It doesn't
check what the arms compute. That is the job of `aura verify`, the command
that proves a function's contract and its arithmetic for every input (see
[Contracts and proofs](/book/ch11-04-contracts-and-proofs)), and it follows
a `match` the way the compiler does, one arm at a time.

Here is a function that works out how far into a book a command leaves us.
Its contract says the position never moves backwards and never passes the
last page:

*Filename: src/main.au*

```aura
enum Command:
    Read(title: str, pages: int64)
    Finish(str)
    List
    Quit

def pages_after(command: Command, pages_read: int64, total: int64) -> int64:
    requires 0 <= pages_read and pages_read <= total
    ensures pages_read <= result and result <= total
    match command:
        case Command.Read(_, pages):
            return min(pages_read + pages, total)
        case Command.Finish(_):
            return total
        case Command.List | Command.Quit:
            return pages_read

def main():
    print(pages_after(Command.Read(title="Dune", pages=40), 103, 412))
    print(pages_after(Command.Finish("Dune"), 103, 412))
    print(pages_after(Command.List, 103, 412))
```

```console
$ aura run src/main.au
143
412
103
$ aura verify src/main.au
error[AU5101]: cannot prove `ensures` clause `pages_read <= result and result <= total`
 --> src/main.au:9:13
  |
9 |     ensures pages_read <= result and result <= total
  |             ^
  = note: in `pages_after` (ensures check)
  = note: counterexample: variant of command = 3, command.Finish[0] = "", command.Read[0] = "", command.Read[1] = -1, pages_read = 9223372036854775807, total = 9223372036854775807
error[AU5101]: cannot prove integer result in range
 --> src/main.au:12:24
   |
12 |             return min(pages_read + pages, total)
   |                        ^
  = note: in `pages_after` (overflow check)
  = note: counterexample: variant of command = 3, command.Finish[0] = "", command.Read[0] = "", command.Read[1] = 9223372036854775807, pages_read = 1, total = 1
verified src/main.au: 3 of 5 checks proved in 2 functions (/usr/bin/z3 (Z3 version 5.3.3 - 64 bit))
```

The program prints what we expected, and the verifier still found two
inputs we didn't think of. A counterexample for an enum takes a moment to
read. It numbers the variants in alphabetical order from 0, so variant 3 of
`Finish`, `List`, `Quit`, and `Read` is `Read`, and `command.Read[1]` is that
variant's second payload, the page count. The payloads listed for other
variants are filler. So the first counterexample is a `Read` command with
`pages` of -1, which moves the position backwards and breaks the `ensures`
clause. The second is a `Read` with an enormous page count, which overflows
the addition before `min` can cap it. The `Finish`, `List`, and `Quit` arms
are fine: neither counterexample lands in them.

A payload is data from outside, here from whatever the user typed, so the
`Read` arm has to deal with any `int64`. A guard handles the bad counts, and
taking the smaller of `pages` and the pages left removes the addition that
could overflow:

```aura
enum Command:
    Read(title: str, pages: int64)
    Finish(str)
    List
    Quit

def pages_after(command: Command, pages_read: int64, total: int64) -> int64:
    requires 0 <= pages_read and pages_read <= total
    ensures pages_read <= result and result <= total
    match command:
        case Command.Read(_, pages) if pages > 0:
            return pages_read + min(pages, total - pages_read)
        case Command.Finish(_):
            return total
        case Command.Read(_, _) | Command.List | Command.Quit:
            return pages_read

def main():
    print(pages_after(Command.Read(title="Dune", pages=40), 103, 412))
    print(pages_after(Command.Read(title="Dune", pages=-5), 103, 412))
    print(pages_after(Command.Finish("Dune"), 103, 412))
    print(pages_after(Command.List, 103, 412))
```

<span class="caption">Listing 6-9: A `match` whose arms are all proved to keep the contract</span>

```console
$ aura run src/main.au
143
103
412
103
$ aura verify src/main.au
verified src/main.au: 8 of 8 checks proved in 2 functions (/usr/bin/z3 (Z3 version 5.3.3 - 64 bit))
```

The verifier understands the guard too. In the first arm it knows `pages` is
positive, and in the last arm it knows that a `Read` that got there has a
count of zero or less. Exhaustiveness and verification answer different
questions about the same `match`: the compiler says every variant has an
arm, and the verifier says every arm keeps the promise.

One limit applies today. The verifier follows a `match` statement, like the
one above, whose arms `return`. It does not yet model the value of a `match`
used as an expression, as in `return match command:`, so an `ensures` clause
that depends on one is reported as unproved. Write the statement form in a
function whose result you want proved.

## A recursive match

Now we can evaluate the `Expr` tree from the previous section. Each variant
gets an arm, and the arms for `Add` and `Multiply` call the function again on
their sub-expressions:

*Filename: src/main.au*

```aura
enum Expr:
    Number(int64)
    Add(Expr, Expr)
    Multiply(Expr, Expr)

def evaluate(expr: Expr) -> int64:
    return match expr:
        case Expr.Number(value): value
        case Expr.Add(left, right): evaluate(left) + evaluate(right)
        case Expr.Multiply(left, right): evaluate(left) * evaluate(right)

def main():
    expr = Expr.Add(Expr.Number(2), Expr.Multiply(Expr.Number(3), Expr.Number(4)))
    print(evaluate(expr))
    print(expr)
```

<span class="caption">Listing 6-10: Evaluating an expression tree with a recursive `match`</span>

```console
$ aura run src/main.au
14
Expr.Add(Expr.Number(2), Expr.Multiply(Expr.Number(3), Expr.Number(4)))
```

The structure of `evaluate` follows the structure of `Expr` exactly: one arm
per variant, and a recursive call wherever the variant contains another
`Expr`. Because `evaluate` takes shared access and `match` borrows, `left`
and `right` are borrowed sub-trees, and nothing is copied or consumed as the
function walks down. `main` can still print `expr` afterwards. If someone adds
a `Subtract` variant to `Expr`, `evaluate` stops compiling until it learns
what to do with one.

Patterns can nest, too: `case Expr.Add(Expr.Number(0), right):` would match
only additions whose left operand is the number zero. Aura also has patterns
for tuples, lists, and class fields. The
[Manual](/manual/enums-and-match) describes them all, along with the precise
coverage rules.

We have one more kind of "one of several things" to look at, and it's the
most common of all: a value that might not be there.
