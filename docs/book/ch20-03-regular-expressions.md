# Regular expressions

Suppose `courier` has been running for a while and has written a line to a
log file for each envelope it handled:

```text
10:02:34 delivered id=7f3a to=ops@example.com bytes=512 took=18ms
10:02:35 rejected id=91bc reason=bad-signature
10:02:39 delivered id=c04d to=ada@example.org bytes=20480 took=131ms
```

Now somebody asks how many messages were delivered, to whom, and how large
they were. The string methods from [Chapter 8](/book/ch08-02-strings) can
answer that. `split(" ")` gives the words and `split_once("=")` takes a word
apart. But the code would be a pile of index arithmetic that breaks the
first time a line gains a field. A *regular expression* describes the shape
of the text we are looking for, and the `regex` module finds it.

## Compiling and matching

A pattern is compiled once into a `regex.Regex`, and the compiled value
does the searching:

*Filename: src/main.au*

```aura
import regex

def run() -> Result[None, regex.Error]:
    log = "10:02:34 delivered id=7f3a to=ops@example.com bytes=512 took=18ms\n10:02:35 rejected id=91bc reason=bad-signature\n10:02:39 delivered id=c04d to=ada@example.org bytes=20480 took=131ms"
    lines = log.split("\n")

    delivered = try regex.compile(r"delivered id=\w+")
    print(delivered.is_match(lines[0]))
    print(delivered.is_match(lines[1]))
    print(delivered.count(log))
    print(delivered.find_all(log))

    match delivered.find(log):
        case None:
            print("no match")
        case regex.Match as first:
            print(f"{first.text} at {first.start}..{first.end}")
    return Result.Ok(None)

def main():
    match run():
        case Result.Ok(_):
            pass
        case Result.Err(regex.Error.Invalid(message)):
            print(message)
```

<span class="caption">Listing 20-8: Testing, counting, and finding matches</span>

```console
$ aura run src/main.au
true
false
2
[delivered id=7f3a, delivered id=c04d]
delivered id=7f3a at 9..26
```

The pattern `delivered id=\w+` matches the literal text `delivered id=`
followed by one or more word characters. It is written as a raw string,
`r"..."`, so that the backslash reaches the regex engine as written. In an
ordinary string literal you would have to double it, `"\\w+"`, and patterns
are hard enough to read already. Make raw strings a habit for patterns.

`regex.compile` returns a `Result`, because a pattern is a small program and
can have syntax errors. Once we have a `Regex`, searching cannot fail, and
the methods return plain values. They are listed here from cheapest to most
detailed. `is_match` answers yes or no. `count` counts matches without
building anything. `find_all` returns the matched texts. `find` returns the
first match as a `regex.Match` with its `text` and its `start` and `end`
positions, or `None`. Positions count characters, the way `str` indexes do,
so `log[first.start:first.end]` is the matched text. Call the cheapest
method that answers your question.

A `Regex` is a small copy value. You can keep it in a binding, pass it to
functions, or store it in a class field, and every copy searches with the
same compiled pattern. Compile a pattern once, outside any loop.

## Capturing fields

Finding a line is half the job. We want the id, the recipient, and the size
out of it. Parentheses in a pattern mark *groups*, and each group remembers
the part of the text it matched. A group can be given a name with
`(?P<name>...)`:

*Filename: src/main.au*

```aura
import regex

class Delivery:
    id: str
    recipient: str
    size: int64

def field(groups: regex.Captures, name: str) -> str:
    match own groups.name(name):
        case str as text:
            return text
        case None:
            return ""

def parse_line(pattern: regex.Regex, line: str) -> Delivery | None:
    match pattern.captures(line):
        case None:
            return None
        case regex.Captures as groups:
            size = parse_int64(field(groups, "bytes")).unwrap_or(0)
            return Delivery(id=field(groups, "id"), recipient=field(groups, "to"), size=size)

def run() -> Result[None, regex.Error]:
    lines = [
        "10:02:34 delivered id=7f3a to=ops@example.com bytes=512 took=18ms",
        "10:02:35 rejected id=91bc reason=bad-signature",
        "10:02:39 delivered id=c04d to=ada@example.org bytes=20480 took=131ms",
    ]
    fields = try regex.compile(r"id=(?P<id>\w+) to=(?P<to>\S+) bytes=(?P<bytes>\d+)")
    mut total = 0
    for line in lines:
        match parse_line(fields, line):
            case None:
                print("skipped a line")
            case Delivery as delivery:
                total += delivery.size
                print(f"{delivery.id}: {delivery.size} bytes to {delivery.recipient}")
    print(f"{total} bytes delivered")
    return Result.Ok(None)

def main():
    match run():
        case Result.Ok(_):
            pass
        case Result.Err(regex.Error.Invalid(message)):
            print(message)
```

<span class="caption">Listing 20-9: Pulling named fields out of log lines</span>

```console
$ aura run src/main.au
7f3a: 512 bytes to ops@example.com
skipped a line
c04d: 20480 bytes to ada@example.org
20992 bytes delivered
```

`captures` returns the groups of the first match, or `None` when the line
does not match at all, which is how the `rejected` line is skipped. On a
`regex.Captures`, `name("id")` gives the text of the named group and
`get(1)` gives a group by number, where group 0 is the whole match. Both
return `str | None`, because a group can exist in the pattern and still
take no part in a match (an optional group, for example). Our `field`
helper turns that into an empty string.

The pattern guarantees that the `bytes` group is all digits, so the
`parse_int64` can only fail on a number too large for `int64`, and
`unwrap_or(0)` covers that. This is the usual division of labor: the
pattern checks the shape of the text, and typed parsing turns the pieces
into values.

One detail of `\d` and `\w` is worth knowing: they follow Unicode. `\d`
matches `٣` (an Arabic-Indic three) as well as `3`, and `parse_int64` would
then reject it. When you mean ASCII digits only, write `[0-9]`.

## Replacing and splitting

A `Regex` can also rewrite text. Before we send that log to anyone, we
should hide the recipients:

```aura
import regex

def run() -> Result[None, regex.Error]:
    line = "10:02:34 delivered id=7f3a to=ops@example.com bytes=512 took=18ms"

    address = try regex.compile(r"(\w+)@([\w.]+)")
    print(address.replace_all(line, "***@$2"))
    print(address.replace_all(line, "$1 at $2"))

    gaps = try regex.compile(r"\s+")
    print(gaps.split("10:02:34   delivered\tid=7f3a"))

    print(regex.escape("took=1.5ms (approx)"))
    return Result.Ok(None)

def main():
    match run():
        case Result.Ok(_):
            pass
        case Result.Err(regex.Error.Invalid(message)):
            print(message)
```

```console
$ aura run src/main.au
10:02:34 delivered id=7f3a to=***@example.com bytes=512 took=18ms
10:02:34 delivered id=7f3a to=ops at example.com bytes=512 took=18ms
[10:02:34, delivered, id=7f3a]
took=1\.5ms \(approx\)
```

In the replacement text, `$1` and `$2` stand for the first and second
groups, `$name` for a named group, and `$0` for the whole match. To put a
real dollar sign in the output, write `$$`. `replace` changes only the
first match and `replace_all` changes every one.

`split` cuts the text at each match, so splitting on `\s+` handles runs of
spaces and tabs that `str.split(" ")` would turn into empty strings.

`regex.escape` goes the other way: it takes text and returns a pattern that
matches exactly that text, with every special character escaped. Use it
whenever part of a pattern comes from a user. Without it, someone searching
for `1.5ms` would also find `105ms`, because `.` matches any character.

## What the engine will not do

Patterns from users can be wrong, so let's see what a bad one looks like,
along with one that is valid in Python and not in Aura:

```aura
import regex

def main():
    print(regex.compile(r"id=(\w+"))
    print(regex.compile(r"(\w+) \1"))
```

```console
$ aura run src/main.au
Result.Err(regex.Error.Invalid(regex parse error:
    id=(\w+
       ^
error: unclosed group))
Result.Err(regex.Error.Invalid(regex parse error:
    (\w+) \1
          ^^
error: backreferences are not supported))
```

The first is a typo, and the message points at the parenthesis that was
never closed. The second is a design decision. `\1` is a *backreference*:
"the same text group 1 matched". Python and Perl support it, along with
look-around assertions such as `(?=...)`. Aura's engine supports neither.

The reason is a guarantee. Engines that support those features search by
trying one possibility after another and backing up when one fails, and for
some patterns the number of possibilities doubles with each character of
input. A pattern like `(a+)+$` can take seconds on thirty characters and
effectively forever on sixty. If any pattern or any searched text comes
from outside, that is a way to freeze your server with one request. Aura's
engine never backs up. Every search takes time proportional to the length
of the text, whatever the pattern, and the price is that the features which
need backing up are missing. In practice a backreference can almost always
be replaced by capturing both pieces and comparing them in code.

A few other limits are worth knowing. Patterns search `str` only, not byte
lists. The replacement is always text, so a function cannot compute it.
`matches` and `find_all` return complete lists, with no lazy iterator. The
[Regex Module](/manual/regex) page has the whole pattern syntax, including
the flags `(?i)` for ignoring case and `(?m)` for matching `^` and `$` at
each line.

We can now read `courier`'s log. The ids in it have been made up so far.
Next we generate real ones.
