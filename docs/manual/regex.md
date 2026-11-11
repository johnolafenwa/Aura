# Regex Module

The `regex` module searches text with regular expressions. A pattern is
compiled once into a `regex.Regex`, which then tests, finds, captures,
replaces, and splits. Every search runs in time linear in the length of the
text, whatever the pattern.

```aura
import regex
```

## Public API

| API | Signature | Result |
| --- | --- | --- |
| `regex.compile` | `compile(pattern: str) -> Result[regex.Regex, regex.Error]` | The compiled pattern, or why it is not one. |
| `regex.escape` | `escape(text: str) -> str` | `text` with every pattern metacharacter escaped, so that as a pattern it matches exactly `text`. |
| `Regex.is_match` | `is_match(self, text: str) -> bool` | Whether the pattern matches anywhere in `text`. |
| `Regex.count` | `count(self, text: str) -> int64` | The number of non-overlapping matches. |
| `Regex.find` | `find(self, text: str) -> regex.Match \| None` | The leftmost match. |
| `Regex.find_all` | `find_all(self, text: str) -> list[str]` | The text of every match, in order. |
| `Regex.matches` | `matches(self, text: str) -> list[regex.Match]` | Every match with its position, in order. |
| `Regex.captures` | `captures(self, text: str) -> regex.Captures \| None` | The groups of the leftmost match. |
| `Regex.replace` | `replace(self, text: str, replacement: str) -> str` | `text` with the leftmost match replaced. |
| `Regex.replace_all` | `replace_all(self, text: str, replacement: str) -> str` | `text` with every match replaced. |
| `Regex.split` | `split(self, text: str) -> list[str]` | The pieces of `text` between the matches. |
| `Regex.pattern` | `pattern(self) -> str` | The pattern text the value was compiled from. |
| `Captures.len` | `len(self) -> int64` | The number of groups, the whole match included. |
| `Captures.get` | `get(self, index: int64) -> str \| None` | The text of group `index`. Group `0` is the whole match. |
| `Captures.name` | `name(self, name: str) -> str \| None` | The text of the group named `name`. |

A `regex.Match` has three public fields: `text: str`, `start: int64`, and
`end: int64`. Every method takes `self` and its arguments as shared borrows
and returns fresh owned values.

## Example

<<< ../../examples/strings/regex_log.au{aura}

It prints:

```text
true
3
[took=120ms, user=7, retries=3]
took=120ms at 9..19
4
took
120
ms
None
9: took=120ms
20: user=7
27: retries=3
café up: took:120 user=7 retries=3
café up: 120 7 3
[café up: ,  ,  , ]
invalid pattern
true
```

## Compiling A Pattern

`regex.compile` returns `Result.Err(regex.Error.Invalid(message))` for a
pattern that is not valid, so a pattern that comes from a user or a
configuration file is checked like any other input. `message` describes the
fault for a person and is not a stable format.

Write patterns as raw strings, `r"\d+"`, so that a backslash reaches the
pattern as written. In an ordinary string literal the same pattern is
`"\\d+"`.

Compile a pattern once and keep the `Regex`: in a local, a class field, or a
parameter. A `Regex` is a small copy value that names the compiled pattern,
so assigning it or passing it to a task copies it, and every copy searches
with the same compiled pattern. Printing a `Regex` shows its pattern text.
Compiling a pattern text a second time is cheap, because the runtime finds
the pattern it already knows.

The runtime remembers each distinct pattern text for the life of the
process, and each thread keeps the compiled form of up to 256 patterns it
used last. A program that compiles an unbounded number of different
patterns, such as one search pattern per request, grows by the length of
each new pattern text and by nothing else.

`Regex` has no public constructor. `regex.compile` is the only way to make
one, so every `Regex` holds a valid pattern.

## Pattern Syntax

| Form | Matches |
| --- | --- |
| `.` | Any character except a newline. |
| `\d` `\w` `\s` | A digit, a word character, whitespace. `\D` `\W` `\S` match the opposite. |
| `[abc]` `[a-z]` `[^a-z]` | One character in, or not in, a set. |
| `\p{Greek}` `\p{L}` | One character with a Unicode property. |
| `^` `$` | The start and end of the text, or of a line with `(?m)`. |
| `\b` `\B` | A word boundary, and not a word boundary. |
| `x*` `x+` `x?` | Zero or more, one or more, zero or one, taking as many as possible. |
| `x{2}` `x{2,}` `x{2,5}` | A counted repeat. |
| `x*?` `x+?` `x??` | The same repeats, taking as few as possible. |
| `a\|b` | Either side. |
| `(x)` | A numbered group. |
| `(?P<name>x)` `(?<name>x)` | A named group, which is also numbered. |
| `(?:x)` | A group that is not captured. |
| `(?i)` `(?m)` `(?s)` `(?x)` | Flags: ignore case, `^` and `$` match at lines, `.` matches a newline, ignore whitespace and allow `#` comments in the pattern. |

Classes such as `\d`, `\w`, and `\s`, word boundaries, and case-insensitive
matching follow Unicode. `\d` matches `٣` as well as `3`. Write `[0-9]` to
match ASCII digits only.

Backreferences such as `\1` and look-around such as `(?=x)` and `(?<!x)` are
not supported. They cannot be matched in linear time, and `regex.compile`
returns `regex.Error.Invalid` for a pattern that uses one.

Matches are leftmost-first: among the matches that start earliest, an
alternation prefers its left branch, as in Perl and Python.

## Positions

`Match.start` and `Match.end` count Unicode scalar values from the start of
the searched text, as `str` indexes and `str.len()` do. `end` is one past
the last character of the match, so `end - start` is the length of
`Match.text`.

## Groups

`Captures.get(0)` is the whole match and `get(1)` the first group, counted
by its opening parenthesis. A group that took no part in the match, such as
an optional group that matched nothing, is `None`. That is different from a
group that matched the empty text, which is `""`. An index or a name the
pattern does not have is also `None`.

## Replacement Text

In the replacement of `replace` and `replace_all`:

| Form | Stands for |
| --- | --- |
| `$1` `${1}` | Group 1. |
| `$name` `${name}` | The group named `name`. |
| `$0` | The whole match. |
| `$$` | A dollar sign. |

A name runs as far as letters, digits, and underscores go, so `$1a` is the
group named `1a`. Write `${1}a` for group 1 followed by `a`. A group the
pattern does not have, or one that took no part in the match, is replaced by
the empty text. To insert text exactly as it is, replace each `$` in it with
`$$`.

## Splitting

`split` returns the text between matches. A match at the start or end of the
text gives an empty first or last piece, and two matches side by side give an
empty piece between them, as `str.split` does for its separator.

## Error Model

| Variant | Meaning |
| --- | --- |
| `Invalid(message: str)` | The pattern has a syntax error, uses an unsupported feature, or compiles to a program larger than the 10 MiB limit, as a very large counted repeat can. |

Searching cannot fail. A result that would pass the 64 MiB `str` limit, which
only `replace` and `replace_all` can produce, traps with `AU4005`.

## Performance

Search time grows linearly with the text. No pattern takes exponential time,
so a pattern from an untrusted source cannot stall the program by
backtracking. The 10 MiB limit on the compiled form bounds its memory.

`is_match` and `count` are the cheapest calls, because they build no
values. `find_all` builds only the matched texts. `matches` and `captures`
build a `Match` or a group list for each result. Call the cheapest method
that answers the question.

The cross-language program
[`regex_scan`](https://github.com/johnolafenwa/Aura/tree/main/benchmarks/cross_language/regex_scan)
runs five patterns over generated log lines in Aura, Python, and Rust.

## Backend Support

The MIR runtime and the direct native backend use the same implementation,
so results match. A program that does not import `regex` does not link it.

## Limits

- Patterns match `str` only. There is no search over `list[uint8]`.
- There is no iterator of matches. `matches` and `find_all` return lists.
- A replacement is text with group references. A function cannot compute it.
- `split` has no limit on the number of pieces.
- A search always starts at the beginning of the text.

Design record: [ADR-0078: Regular expressions](https://github.com/johnolafenwa/Aura/blob/main/architecture_docs/decisions/0078-regular-expressions.md).
