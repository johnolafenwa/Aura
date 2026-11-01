# Strings

Our gradebook's data will arrive as text, one student per line:

```text
Ada Lovelace: 72, 95, 88
```

Turning a line like that into a name and a list of numbers means trimming,
splitting, and parsing, and printing a report afterwards means padding and
joining. All of that happens on `str`, Aura's string type. You've used `str`
since the first chapter, and most of its methods will be familiar from
Python. The parts that differ are the ones that come from two decisions: a
`str` is UTF-8 text, and a `str` is an owned value that never changes.

## What a `str` is

A `str` holds a sequence of Unicode characters, stored as UTF-8 bytes. The
string owns those bytes, the way a list owns its elements, and nothing can
change them after the string is made. Every method that seems to modify a
string, such as `replace`, `to_upper`, or `trim`, returns a new string and
leaves the original as it was.

Because a `str` is an owned value, not a copy value, assignment moves it:

```aura check-fail:AU3001
def main():
    s = "abc"
    t = s
    print(s)
```

```console
$ aura check src/main.au
error[AU3001]: use of moved value `s`
 --> src/main.au:4:11
  |
4 |     print(s)
  |           ^
  = related src/main.au:3:9: value moved here
  = help: pass shared access when ownership is not needed, or call `.clone()` at the move site when an independent value is required
  = fix: replace src/main.au:3:10-3:10 with `.clone()`
```

That's the same rule you met in [Chapter 4](/book/ch04-00-understanding-ownership),
and the fix the compiler offers is the same: `t = s.clone()` makes an
independent copy. Note the spelling. Strings copy with `.clone()`, while
lists, dictionaries, and sets copy with `.copy()`. A function that only reads
a string should take it as a plain `str` parameter, which lends it for the
call and doesn't move anything.

## Length, and what counts as a character

Let's look at a name with an accent in it:

*Filename: src/main.au*

```aura
def main():
    name = "Zoë"
    print(name.len())
    print(name.byte_len())
    print(name[2:])
    print(name[:-1])
    print(name[::-1])
    cheer = "A🎉"
    print(f"{cheer.len()} scalars, {cheer.byte_len()} bytes")
```

<span class="caption">Listing 8-8: Counting and slicing Unicode text</span>

```console
$ aura run src/main.au
3
4
ë
Zo
ëoZ
2 scalars, 5 bytes
```

`len()` counts *Unicode scalar values*, which for most text means the
characters you see: `Zoë` has three. `byte_len()` counts the UTF-8 bytes
behind them, and `ë` takes two bytes, so there are four. The emoji takes four
bytes on its own. The two numbers have different costs as well as different
meanings: `byte_len()` is stored with the string and is instant, while
`len()` has to walk the bytes to count scalars, so it takes time proportional
to the length of the string. In a loop over a long string, that difference is
worth remembering.

Slices count scalar values too, so `name[2:]` is the `ë` and never half of
it. Every slice of a `str` is a new, valid string: you can't cut a character
in two. Slices follow the same rules as list slices: the end is exclusive,
negative positions count from the end, and a step of `-1` walks backwards,
which reverses the string.

One caution about "characters". A scalar value is not always what a reader
would call one character. Some accented letters can also be written as a
plain letter followed by a combining accent, and some emoji are built from
several scalars joined together. Aura counts scalars, not those visual
clusters, which is also what Python's `len` does.

A string slice that goes past the end doesn't quietly stop at the end the way
Python's does. Like a list slice, it traps:

```aura
def main():
    word = "café"
    print(word[1:9])
```

```console
$ aura run src/main.au
error[AU4003]: slice end `9` is outside `0..=4`
 --> src/main.au:3:17
  |
3 |     print(word[1:9])
  |                 ^
  = note: Aura call chain (innermost first): main at 1:1
```

The message tells you the valid range. Aura chooses a trap over clamping
because a slice that asks for nine characters of a four-character word
usually means the program's idea of the text is wrong, and silently
returning less hides that.

## No indexing by position

If you try to take one character with an index, as you might in Python, the
compiler stops you:

```aura check-fail:AU2999
def main():
    word = "café"
    print(word[0])
```

```console
$ aura check src/main.au
error[AU2999]: cannot index non-Array, list, or dict value `str`
 --> src/main.au:3:11
  |
3 |     print(word[0])
  |           ^
```

and a `for` loop over a string isn't available either:

```aura check-fail:AU2999
def main():
    word = "café"
    for ch in word:
        print(ch)
```

```console
$ aura check src/main.au
error[AU2999]: `for` currently requires a `Range`, `Queue[T]`, `list[T]`, or `set[T]` iterable, found `str`
 --> src/main.au:3:5
  |
3 |     for ch in word:
  |     ^
```

This trips up everyone coming from Python, so let's be clear about why it
happens. Aura has no character type: a single letter is a `str` of length
one. Finding the scalar at position `n` in UTF-8 means walking from the
start, so if a string could be indexed or iterated one position at a time, a
loop like this would quietly turn a linear algorithm into a quadratic one.
Aura would rather make you ask for the characters explicitly, so the cost is
visible. You ask with `chars()`, which walks the string once and gives back a
`list[str]` of one-character strings. `ord` turns one of them into its
Unicode scalar value, and `chr` goes the other way:

*Filename: src/main.au*

```aura
def main():
    word = "naïve"
    for ch in word.chars():
        print(f"{ch} {ord(ch)}")
    print(chr(97))
```

```console
$ aura run src/main.au
n 110
a 97
ï 239
v 118
e 101
a
```

Notice that `ï` is one character here, even though it takes two bytes in
UTF-8. `chars()` splits on Unicode scalars, not bytes. Passing `ord` a string
that isn't exactly one character, or `chr` a number that isn't a valid
scalar, stops the program with a trap.

In practice, most text processing doesn't need single characters at all. You
want words, fields, lines, and prefixes, and the methods in the rest of this
section work on those directly. When you need one character from a known
position, a slice such as `word[0:1]` gives it to you as a one-character
`str` without building the whole list.

## Searching

*Filename: src/main.au*

```aura
def main():
    comment = "Great work on the essay; revise the essay intro."
    print(comment.contains("essay"))
    print(comment.starts_with("Great"))
    print(comment.count("essay"))
    print(comment.find("revise"))
    print(comment.find("excellent"))
    match comment.find(";"):
        case int64 as at:
            print(comment[:at])
        case None:
            print(comment)
    print(comment.replace("essay", "report"))
    print(comment.to_upper())
```

<span class="caption">Listing 8-9: Searching and transforming a string</span>

```console
$ aura run src/main.au
true
true
2
25
None
Great work on the essay
Great work on the report; revise the report intro.
GREAT WORK ON THE ESSAY; REVISE THE ESSAY INTRO.
```

`contains`, `starts_with`, and `ends_with` answer yes-or-no questions.
`count` counts non-overlapping matches. `find` returns the position of the
first match, counted in scalars like `len()` and slices, so you can feed it
straight into a slice. When there's no match it returns `None`, not Python's
`-1`, so its type is `int64 | None` and you have to deal with both cases before
you can use the number. The `match` with a type pattern does that, as you saw
in [Chapter 6](/book/ch06-00-enums-and-match). A `-1` that slips through
unchecked would slice from the end of the string, which is exactly the kind
of bug the type rules out.

`strip_prefix` and `strip_suffix` follow the same pattern. They return the
rest of the string if the prefix or suffix was there, and `None` if it
wasn't:

```aura
def main():
    file_name = "term1-grades.csv"
    match file_name.strip_suffix(".csv"):
        case str as stem:
            print(stem)
        case None:
            print("not a CSV file")
```

## Splitting and parsing

Now we have what we need to read a gradebook line. Let's write a function
that turns the text after the colon into a list of scores, and use it from
`main`:

*Filename: src/main.au*

```aura
def parse_scores(text: str) -> list[int64]:
    mut scores: list[int64] = []
    for part in text.split(","):
        match parse_int64(part.trim()):
            case Result.Ok(score):
                scores.append(score)
            case Result.Err(message):
                print(f"skipping {part.trim()}: {message}")
    return scores

def main():
    line = "  Ada Lovelace: 72, 95, x, 88  "
    trimmed = line.trim()
    pair = trimmed.split_once(":")
    if pair is not None:
        name, rest = pair
        scores = parse_scores(rest)
        print(f"{name} has {scores.len()} scores: {scores}")
    else:
        print(f"no colon in {trimmed}")
```

<span class="caption">Listing 8-10: Parsing a line of text into a name and scores</span>

```console
$ aura run src/main.au
skipping x: invalid digit found in string
Ada Lovelace has 3 scores: [72, 95, 88]
```

Let's take it from the top of `main`. `trim()` returns a copy of the line
without the whitespace at either end; `trim_start()` and `trim_end()` trim one
side. `split_once(":")` splits at the first colon and returns both halves as
a `(str, str)` tuple, or `None` if there's no colon, so its type is
`(str, str) | None`. The `if pair is not None:` test narrows it to the tuple,
and `name, rest = pair` unpacks it. (`rsplit_once` splits at the last
occurrence instead.)

In `parse_scores`, `split(",")` breaks the rest of the line into a
`list[str]` at every comma. The pieces still have their spaces, so each one
gets trimmed before `parse_int64` tries to read it as a number.
`parse_int64` returns a `Result`, because text from outside the program might
not be a number at all, and the `x` in our line shows the `Err` case. Here we
print a note and keep going; [Chapter 9](/book/ch09-00-error-handling) shows
how to pass the error up to the caller instead. `parse_int32` and
`parse_float64` work the same way.

For whole blocks of text, two more splitting methods help. `lines()` breaks
text at newlines, and handles Windows-style line endings as well.
`split_whitespace()` splits on any run of spaces, tabs, or newlines, and drops
the empty pieces that `split(" ")` would leave between two spaces:

*Filename: src/main.au*

```aura
def main():
    report = """Ada Lovelace: 72, 95, 88
Grace Hopper: 88, 91
Alan Turing: 64, 70, 91
"""
    for line in report.lines():
        print(line.split_whitespace())
```

```console
$ aura run src/main.au
[Ada, Lovelace:, 72,, 95,, 88]
[Grace, Hopper:, 88,, 91]
[Alan, Turing:, 64,, 70,, 91]
```

The triple-quoted string keeps its text exactly as written, newlines
included, and the final newline doesn't produce an empty last line. Notice,
too, that a printed list of strings shows them without quotation marks.

While we're on literals: the usual escapes `\n`, `\t`, `\\`, `\"`, and `\'`
work in ordinary strings, along with `\0`, `\xHH`, and `\u{...}`. A Python
habit that doesn't carry over is `\r`, which isn't a supported escape; the
compiler reports `AU1001` for it, and you can write `\x0d` if you need a
carriage return. A string with an `r` prefix, such as `r"C:\grades"`, treats
backslashes as ordinary characters.

## Building strings

There are three common ways to put a string together. The first is `+`,
which joins two strings. *Two strings*, that is:

```aura check-fail:AU2999
def main():
    total = 255
    print("total: " + total)
```

```console
$ aura check src/main.au
error[AU2999]: binary operator operands must match, found `str` and `int64`
 --> src/main.au:3:11
  |
3 |     print("total: " + total)
  |           ^
```

Aura doesn't convert numbers to text behind your back, any more than it
converts an `int32` to an `int64`. Write the conversion: `total.to_string()`
or `str(total)` both give `"255"`.

The second way, and usually the best one, is an f-string. You've been using
them all along, and they also take a format specification after a colon that
controls width, alignment, and precision. The third is `join`, which glues a
list of strings together with the receiver as the separator. Here's a small
report that uses all of them:

*Filename: src/main.au*

```aura
def score_line(name: str, scores: list[int64]) -> str:
    mut parts: list[str] = []
    for score in scores:
        parts.append(f"{score:>3}")
    return f"{name:<14}|" + " ".join(parts)

def main():
    rule = "-".repeat(26)
    print(f"{'Student':<14}| Scores")
    print(rule)
    print(score_line("Ada Lovelace", [72, 95, 88]))
    print(score_line("Grace Hopper", [88, 91]))
    print(rule)
    average = 84.6
    print(f"class average: {average:.1f} ({average / 100.0:.0%})")
```

<span class="caption">Listing 8-11: Formatting a report with f-strings, `join`, and `repeat`</span>

```console
$ aura run src/main.au
Student       | Scores
--------------------------
Ada Lovelace  | 72  95  88
Grace Hopper  | 88  91
--------------------------
class average: 84.6 (85%)
```

`{score:>3}` right-aligns a number in three columns, `{name:<14}`
left-aligns a name in fourteen, `.1f` shows one decimal place, and `.0%`
multiplies by a hundred and adds a percent sign. The compiler checks each
specification against the type of the value before the program runs, so a
`.1f` on a string is a compile-time error rather than a surprise in the
output. `"-".repeat(26)` makes a string of 26 dashes; it's the same method the
progress bar used in [Chapter 7](/book/ch07-00-modules-and-packages).

The pattern in `score_line`, collecting pieces in a `list[str]` and joining
them once at the end, is the clearest way to build a string out of many
parts. For a few pieces in a loop, `+=` on a mutable string works too:

```aura
def main():
    mut summary = "scores:"
    for score in [72, 95, 88]:
        summary += f" {score}"
    print(summary)
```

```console
$ aura run src/main.au
scores: 72 95 88
```

Strings compare with `==` and `!=`, and order with `<` and friends by their
UTF-8 bytes, which is the same order `sort` uses. The full list of string
methods is in the Manual's [API index](/manual/api-index), and
[Types](/manual/types) covers `to_bytes` and `str.from_bytes` for converting
to and from raw bytes.

We can now read a student's name and scores out of a line of text. A
gradebook has many such lines, and the natural way to hold them is to look up
scores by name. That's what dictionaries are for.
