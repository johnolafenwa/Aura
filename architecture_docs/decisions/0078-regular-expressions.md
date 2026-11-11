# ADR-0078: Regular expressions

- Status: Accepted
- Date: 2026-11-10
- Builds on: ADR-0057, ADR-0073

## Context

Aura had `str` methods for fixed separators and prefixes and nothing for
patterns. Log parsing, input validation, and text rewriting needed either a
hand-written scanner or a foreign library. This is the second step of the
streaming I/O and standard-library work.

## Decision

### The engine

The `regex` module runs on the Rust `regex` crate. Its searches take time
linear in the text for every pattern, which matters for a language whose
servers take patterns and text from outside: no pattern can stall a task by
backtracking. The price is that backreferences and look-around are not
available. `regex.compile` reports a pattern that uses one as
`regex.Error.Invalid`, like any other pattern it cannot compile.

### The API

```aura fragment
regex.compile(pattern: str) -> Result[regex.Regex, regex.Error]
regex.escape(text: str) -> str
Regex.is_match(text) -> bool
Regex.count(text) -> int64
Regex.find(text) -> regex.Match | None
Regex.find_all(text) -> list[str]
Regex.matches(text) -> list[regex.Match]
Regex.captures(text) -> regex.Captures | None
Regex.replace(text, replacement) -> str
Regex.replace_all(text, replacement) -> str
Regex.split(text) -> list[str]
Regex.pattern() -> str
```

- A pattern is compiled explicitly and the result is a value. There are no
  module-level `regex.search(pattern, text)` shortcuts, so a loop cannot
  compile a pattern on every pass by accident.
- An invalid pattern is a `Result.Err`, not a trap, because patterns come
  from configuration and from users.
- `Match.start` and `Match.end` count Unicode scalar values, the unit of
  every `str` index in Aura. The engine reports byte offsets, and the
  runtime converts them in one pass over the text (no pass at all for ASCII
  text).
- `Captures.get` returns `None` for a group that took no part in the match
  and `""` for one that matched the empty text. The two are different
  facts about the match.
- Replacement text uses `$1`, `${name}`, and `$$`.

### How a `Regex` is represented

`Regex`, `Match`, `Captures`, `Error`, and `compile` are Aura source
(`crates/aura-compiler/std/regex.au`) over host functions named
`regex.__aura_*`, the pattern the `json` and `iter` modules use. No compiler
special case exists for the module.

A `Regex` holds one `int64`: the number the runtime gave its pattern in a
process-wide table of pattern texts. Each thread keeps the compiled programs
of the patterns it used last (the eight most recent found by comparing
numbers, and up to 256 in a map) and compiles a pattern again from the
table's text when it has dropped it.

Two other representations were built and measured first:

- A `Regex` that held the pattern text, looked up in a per-thread cache on
  every call. Reading a `str` field out of a class instance costs about
  2,000 instructions and a release, more than a short search itself, and
  every method paid it. On `regex_scan` this was 1.83 s where Python takes
  1.63 s. The number is an inline load.
- Cloning the compiled program out of the cache on each call. A clone
  starts with empty scratch space and rebuilds it on the first search,
  about 49 microseconds per call.

`Regex` is a `copy class`, so a program copies it freely and every copy
names the same compiled pattern. The number is private state that only
`regex.compile` produces, so a `Regex` always names a pattern the table
holds.

The table never forgets a pattern text. A program that compiles an
unbounded number of distinct patterns grows by the length of each text. The
compiled programs, which are the large part, stay bounded per thread. A
handle type with a destructor would free the text too, at the cost of a new
runtime value kind and a slower call path. The text was judged cheap enough
to keep.

### Linking

Each host function has its own exported entry, so a program that does not
import `regex` links none of the engine. The lean-binary gate lists the
`regex`, `regex_automata`, and `regex_syntax` symbols as absent from every
subject.

## Consequences

- `regex_scan`, the 32nd cross-language program, runs five patterns over
  200,000 generated log lines: Aura 0.83 s, Python 1.55 s, Rust 0.27 s on
  the 4-core cloud host. Aura and Rust run the same engine. The remaining
  difference is the cost of building `Match` values and lists.
- A compiled program calls each search through a helper that takes the
  pattern number as an integer and the texts as handles, and returns a
  `bool` or an `int64` as one word. The general host call, which boxes each
  integer and passes an argument buffer, cost about 1,900 instructions
  around a search of about 650 and made the program take 1.19 s.
  `matches` gets the start and end of each match from the host and slices
  the text itself.
- The engine adds `regex`, `regex-automata`, and `regex-syntax` to the
  runtime's dependencies. `aho-corasick` and `memchr` were already there.

## Not decided here

- A cheaper class instance and list, which is where the rest of the
  distance to Rust is. It belongs with the general work on runtime-boundary
  costs noted in `work/2026-11-10-streaming-io-and-stdlib.md`.
- An iterator of matches, search over `list[uint8]`, a replacement computed
  by a function, and a split limit.
