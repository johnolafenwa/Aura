# Batch 4 design checkpoint: everyday syntax and API usability

Status: **Ratified 2026-10-11 by delegated decision**

Date: 2026-10-11. Source baseline: `5652e46` (main after
[PR #31](https://github.com/johnolafenwa/Aura/pull/31)).

This checkpoint settles the detailed design that
[roadmap Batch 4](14-priority-roadmap.md#priority-batches) left open. The
owner asked for Batch 4 to be completed and made ready for release, and
delegated the remaining choices. Each section below states the chosen rule,
the alternatives considered, and the evidence the implementation must pin.

The binding inputs are
[ADR-0063](decisions/0063-everyday-syntax-and-pattern-ergonomics.md)
(everyday syntax and patterns),
[ADR-0055](decisions/0055-display-trait-and-properties.md) (Display and
properties), and
[ADR-0056](decisions/0056-docstrings-and-documentation-metadata.md)
(docstrings). Where this checkpoint narrows an ADR baseline, the narrower
rule is the implemented contract, and the ADR records the narrowing.

Batch 4 is delivered as independent feature families. Each family lands in
its own pull request with parser, checker, both backends where relevant,
language-server, formatter, manual, example, and tutorial updates.

| Family | Section | Backends touched |
| --- | --- | --- |
| Trailing commas and parenthesized imports | [A](#a-trailing-commas-and-parenthesized-imports) | parser only |
| Unpacking, sequence patterns, and class patterns | [B](#b-unpacking-sequence-patterns-and-class-patterns) | checker, MIR, direct |
| Text and byte conveniences | [C](#c-text-and-byte-conveniences) | checker, MIR, direct |
| Docstrings and generated API docs | [D](#d-docstrings-and-generated-api-docs) | parser, analysis, CLI, LSP |
| Display and read-only properties | [E](#e-display-and-read-only-properties) | checker, MIR, direct |
| Syntax-reflowing formatter | [F](#f-syntax-reflowing-formatter) | CLI, LSP, extension |

New diagnostic codes for Batch 4 use the `AU2040`–`AU2049` block, so they
cannot collide with codes allocated by the concurrent Batch 2 and Batch 3
work.

| Code | Meaning |
| --- | --- |
| `AU2040` | invalid class pattern: unknown, private, or repeated field, or a property in a pattern |
| `AU2041` | invalid rest pattern or rest target |
| `AU2042` | invalid or misplaced docstring, or a documentation size limit |
| `AU2043` | invalid `Display` implementation |
| `AU2044` | invalid property declaration |
| `AU2045` | invalid property use: assignment, call syntax, mutable or owned place use |

## A. Trailing commas and parenthesized imports

**Rule.** Every comma-separated list that is closed by `)`, `]`, or `}`
accepts one optional trailing comma before the closing delimiter. This
covers:

- call arguments, including keyword arguments
- parameter lists of functions, methods, trait methods, extern functions, and
  lambdas written in parentheses
- list, set, and dictionary displays, and comprehension-free collection
  literals
- tuple values, tuple types, tuple targets, and tuple patterns
- type-argument and type-parameter lists in brackets
- function-type parameter lists
- enum variant payload declarations and variant payload patterns
- trait and implementation type-argument lists
- subscripts with several indices
- lambda capture lists
- parenthesized import lists

A trailing comma never changes meaning, with one exception that already
exists: inside parentheses, `(x,)` is a one-element tuple and `(x)` is
grouping. `(x, y,)` is the same two-element tuple as `(x, y)`.

A list that contains only a comma, such as `f(,)` or `[,]`, is rejected.
Two commas in a row are rejected. A trailing comma is not accepted in a list
that has no closing delimiter, such as an unparenthesized tuple
`a, b, = pair`, an unparenthesized `return a, b,`, or an unparenthesized
import list. The existing required comma of the unparenthesized one-element
forms is unchanged.

**Parenthesized imports.** A from-import may wrap its names in parentheses:

```aura fragment
from agents.tools import (
    ToolCall,
    ToolResult as Result,
    run_tool,
)
```

The parentheses hold one or more names. Line breaks inside them follow the
ordinary delimiter-continuation rule. `import (a, b)` is not a form; a
module import names one module path.

**Alternatives.** Python-style trailing commas everywhere, including
unparenthesized tuples, was rejected because a stray trailing comma after
`return value` would silently create a tuple. Aura already requires the
explicit `(value,)` form for one-element tuples in that position.

**Evidence.** Parser fixtures pin each position above with and without the
trailing comma, the empty-comma and double-comma rejections, the
unparenthesized rejections, and AST equality between the two spellings.

## B. Unpacking, sequence patterns, and class patterns

### B1. Tuple rest

A tuple target or tuple pattern may contain one starred element:

```aura
def main():
    first, *rest = (1, "two", 3.0)
    print(first)
    print(rest)
```

- `*name` binds a tuple of the positions it covers. Its type is the tuple of
  those element types. It must cover at least one position, because Aura has
  no empty tuple; covering zero positions is `AU2041`.
- `*_` discards zero or more positions.
- At most one starred element appears in one tuple level. A second one is
  `AU2041`. A starred element outside a tuple target or tuple pattern is
  `AU2041`.
- Arity is checked statically. The tuple must have at least as many
  positions as the non-starred elements.
- Ownership follows the existing tuple unpacking rules position by position.
  A rest tuple of non-Copy positions is built from moved positions in an
  owned unpack, and is a shared provenance tuple in a bare `match`.

Rest binding performs no allocation beyond building the rest tuple value. It
has no runtime check.

### B2. List sequence patterns

A `match` on a `list[T]` scrutinee accepts sequence patterns:

```aura
def describe(values: list[int64]) -> str:
    match values:
        case []:
            return "empty"
        case [only]:
            return f"one: {only}"
        case [first, *_, last]:
            return f"from {first} to {last}"
```

- `[p1, ..., pn]` matches a list of exactly `n` elements.
- `[p1, *rest, pn]` matches a list of at least the fixed count. At most one
  starred element appears.
- Element subpatterns may be any pattern that fits `T`.
- Coverage: a final `_` or name arm covers the remainder. Sequence arms
  contribute no exhaustiveness coverage on their own, except that the set
  `[]` plus `[_, *_]` covers every length.
- The length is read once when the arm is tested. Elements are tested in
  source order, left to right.
- Only a bare `match` accepts sequence patterns. `match mut` and
  `match own` with a sequence pattern are rejected with `AU2041`; a later
  batch can add them with element write-back and moves.
- A bare `match` binds Copy elements as values and non-Copy elements as
  shared element views under ADR-0061. The arm locks the list as other bare
  matches do.
- `*rest` binds a fresh owned `list[T]` copy of the covered elements. It
  requires clone-safe elements, as a list slice does. `*_` copies nothing.

Statement unpacking of a list, such as `first, *rest = items`, is not
added. A list's length is not static, so it belongs in a `match`.

### B3. Class patterns

A class pattern names a class and keyword field subpatterns:

```aura
class Point:
    x: int64
    y: int64

def main():
    p = Point(x=0, y=7)
    match p:
        case Point(x=0, y=y):
            print(f"on the y axis at {y}")
        case Point(x=x, y=_):
            print(f"x is {x}")
```

- Field subpatterns are keyword only: `Point(x=pattern)`. Positional class
  subpatterns are not a form, so the order of declared fields is never part
  of a pattern contract.
- `Point()` matches any `Point` and binds nothing.
- Fields may be omitted. A field may appear at most once.
- Only stored fields may appear. A property, a method, or an unknown name is
  `AU2040`. Properties never run during matching.
- A field must be visible at the pattern: a private field matches only inside
  the declaring module, the same rule as a field read. A violation is
  `AU2040`.
- The scrutinee may be the class itself or a union that has the class as a
  member. For a union, the class pattern is a member test like
  `case Point as p`, and it counts toward union coverage only when every
  field subpattern is irrefutable.
- A class pattern may appear nested inside tuple, enum payload, and sequence
  patterns.
- Capabilities follow tuple patterns: a bare `match` gives shared field
  provenance, `match own` consumes the object and moves the selected fields,
  and `match mut` is rejected with `AU2040` for class patterns.
- Generic classes match through their specialized field types. A type
  argument in the pattern, such as `Box[int64](value=v)`, is not a form; the
  scrutinee type supplies the specialization.
- `case Point(...) as whole` is not added; bind the scrutinee before the
  match if both are needed.

**Evidence.** Fixtures pin each rest form, arity and repeated-rest
rejections, list lengths zero through three, nested patterns, guard
interaction, evaluation order, Copy and non-Copy element bindings, the
shared lock during the arm, `*rest` clone-safety, class field visibility
across modules, property rejection, union member tests, coverage, and
MIR/direct parity for every runnable fixture.

### B4. Implementation refinements

The implementation keeps the rules above with these refinements:

- **Empty parentheses.** `Name()` always parses as a class pattern. A
  payload-free enum variant is written without parentheses, so `Red()` over
  an enum is `AU2040`. This keeps `Point()` unambiguous without consulting
  types in the parser.
- **Sequence coverage.** Irrefutable arms for every length `0` through
  `k - 1`, plus an irrefutable arm with `k` fixed elements and a rest, cover
  every list. `[]` plus `[_, *_]` is the `k = 1` case. Other sequence arms
  contribute no coverage.
- **Class coverage.** A class value is also covered by class patterns whose
  field patterns together cover every value, column by column, such as one
  arm per variant of an enum field. An irrefutable class pattern remains the
  simple case.
- **Diagnostic codes.** Enum variant or type patterns over a class scrutinee
  report `AU2040`. Rest misplacement detected while parsing reports `AU2041`.
  A sequence pattern over a non-list scrutinee and a non-exhaustive list or
  class match keep the general `AU2999` match codes.
- **Properties.** Aura has no properties yet, so the field rule rejects
  methods and unknown names with `AU2040`. Properties will fall under the
  same rejection.
- **Rest slices.** A named sequence rest builds its list with the same
  duplication check as a list slice, so a list of `Rng` values rejects it.

## C. Text and byte conveniences

The additions come from the parsing work in `examples/` and agent-tool
programs that today use `split`, `strip_prefix`, and manual loops. Each
method reads a shared receiver and returns fresh owned values.

| Method | Signature | Contract |
| --- | --- | --- |
| `str.trim_start` | `trim_start() -> str` | Removes leading Unicode whitespace. |
| `str.trim_end` | `trim_end() -> str` | Removes trailing Unicode whitespace. |
| `str.split_once` | `split_once(separator: str) -> (str, str) \| None` | Splits at the first occurrence. `None` when absent. |
| `str.rsplit_once` | `rsplit_once(separator: str) -> (str, str) \| None` | Splits at the last occurrence. `None` when absent. |
| `str.lines` | `lines() -> list[str]` | Splits at `\n`, removing one `\r` before each `\n`. A final line terminator adds no empty line. |
| `str.split_whitespace` | `split_whitespace() -> list[str]` | Splits on runs of Unicode whitespace and drops empty parts. |
| `str.find` | `find(text: str) -> int64 \| None` | Index of the first occurrence, counted in Unicode scalar values. `None` when absent. |
| `str.count` | `count(text: str) -> int64` | Number of non-overlapping occurrences. |
| `str.repeat` | `repeat(count: int64) -> str` | Concatenates `count` copies. |
| `bytes.starts_with` | `starts_with(value: list[uint8], prefix: list[uint8]) -> bool` | Byte prefix test. |
| `bytes.ends_with` | `ends_with(value: list[uint8], suffix: list[uint8]) -> bool` | Byte suffix test. |
| `bytes.find` | `find(value: list[uint8], needle: list[uint8]) -> int64 \| None` | Byte index of the first occurrence. |

- An empty `separator` in `split_once` and `rsplit_once` traps with
  `AU4003`, the same as `split("")`. An empty `text` in `count` traps with
  `AU4003`. An empty `text` in `find` returns `0`.
- `repeat` with a negative count traps with `AU4003`. A result above the
  64 MiB string limit traps with `AU4005` before allocating.
- Indexes are `int64`. `str.find` is O(n) in scalar values, matching
  `str.len`.

**Alternatives.** Python's `partition`, `lstrip`, `rstrip`, and
`splitlines` spellings were rejected in favor of names consistent with the
existing `trim`, `strip_prefix`, and `starts_with` surface. `str.index`
returning a trap was rejected; `find` returns an explicit `None`.

**Evidence.** Fixtures cover ASCII and multibyte text, empty receivers,
absent separators, `\r\n` input, the documented traps, and MIR/direct
parity.

## D. Docstrings and generated API docs

ADR-0056 is implemented with these settled details.

**First slot.** A docstring is a plain triple-quoted string literal that is
the complete first statement of a module, class, enum, trait, function, or
method body. Comments and blank lines before it are trivia. A module
docstring must precede every import. A signature-only trait method may have
a docstring-only body; it stays signature-only.

**Fields.** A plain triple-quoted string literal on the line directly after a
field declaration in a class body documents that field. A class body has no
other use for a string statement, so this form is not ambiguous. Enum
variants have no documentation in this batch.

**Parameters.** A parameter is documented in its function docstring under an
`Args:` section. Each entry is `name: description`, indented under the
heading; continuation lines are indented further. The section is
presentation data: hover, signature help, and `aura doc` read it. The
compiler does not validate it.

**Limits.** One docstring is at most 65,536 UTF-8 bytes after escape
processing. The documentation of one module's public declarations is at most
4,194,304 bytes in total. Exceeding either limit is `AU2042` at the literal,
with the measured and permitted counts.

**Misplaced docstrings.** A triple-quoted string that is not in a first slot
remains an ordinary expression statement, as today. An f-string, raw string,
or non-triple string in a first slot is not documentation and gets no
diagnostic.

**Normalization.** Presentation uses one shared algorithm:

1. Split the text into lines.
2. Remove leading and trailing blank lines.
3. Keep the first line's own text after trimming its leading spaces.
4. Find the smallest indentation of the remaining non-blank lines, counting
   spaces only, and remove that many leading spaces from each of them.
5. Remove trailing spaces on every line.

Tabs are content. Stored text is never rewritten.

**Execution.** A docstring produces no MIR, no constant, and no allocation.
The parser records it on the declaration and drops the statement.

**Presentation.** Hover shows the fenced signature, then the normalized
docstring, then existing notes. Control characters other than tab and
newline are escaped. Raw HTML is escaped. Completion detail shows the first
non-empty line, truncated at 160 Unicode scalar values with `…`.

**Generated API docs.** `aura doc [path ...] [--out DIR] [--json]` checks
each source and writes one Markdown page per module, listing the module
docstring and every public declaration with its canonical signature,
documentation, fields, parameters, and methods. `--json` writes the same
data as one JSON document per module, for other tools. Private declarations
are omitted. The command does not run code inside documentation.

**Interface retention.** Imported public declarations keep their docstrings
for hover. The documentation is kept in the analysis data that the language
server reads; it is not part of executable MIR or native cache keys.

## E. Display and read-only properties

ADR-0055 is implemented with these settled details.

**Display.** The prelude declares `trait Display` with
`def display(self) -> str`. A class or enum declared in the current module
may implement it with `impl Display for Type:`. A generic local type may use
`impl[T] Display for Box[T]:`. `AU2043` rejects an implementation for a
type from another module, a builtin, a tuple, or a union, a second
overlapping implementation, and a method whose receiver, parameters, type
parameters, or result differ from the trait.

**Dispatch.** `print(value)`, `str(value)`, f-string fields with no format
specification, and the structural rendering of a nested element, field,
key, payload, or union member all call the type's `display` when it has an
implementation. Otherwise the existing structural rendering applies
unchanged. `value.display()` is available on every type that has an
implementation.

**`str(value)`.** `str(value)` becomes a builtin conversion that renders any
displayable value. It replaces the current rejection.

**Format specifications.** A custom-Display value in an f-string accepts the
string subset only: fill, alignment, width, precision, and `s`. Precision
truncates first, then padding applies. Other flags are `AU2002`.

**Atomic output.** `print` renders the whole value and newline into one
string before it writes. If rendering traps, nothing is printed by that
`print`. One `print` call issues one write of its complete text, so the
text of two concurrent `print` calls never interleaves within a line on
either backend.

**Cycles.** Aura values are trees or explicit `indirect` chains. A Display
implementation that renders itself recursively follows ordinary recursion
and the recursion-depth limit. Structural rendering does not add a separate
cycle marker in this batch.

**Properties.** `@property` before a method in a class body declares a
read-only computed property, as ADR-0055 specifies. The parser accepts only
the exact `@property` line; any other decorator is `AU2005`, because general
decorators are Batch 6 work. `AU2044` rejects a getter that is not on a
class, has a receiver other than bare `self`, has parameters or type
parameters, has no result or a `None` result, or collides with a field or
another member. `value.name` calls the getter once after evaluating `value`
once. `AU2045` rejects assignment, compound assignment, `mut` or `own`
binding, view creation, and call syntax on a property. Properties are not
trait members.

**Documentation.** A property getter's docstring is the property's
documentation, and hover on `value.name` shows it.

**Evidence.** Fixtures cover every dispatch entry point, nested rendering
inside each aggregate, generic local implementations, every `AU2043` and
`AU2044` and `AU2045` case, the string format subset, a trapping Display
implementation with no partial output, property reads in chains and
expressions, Copy and non-Copy property results, imported Display dispatch
and properties, and MIR/direct parity.

## F. Syntax-reflowing formatter

`aura fmt` becomes a token-based formatter with these rules:

- **Indentation.** Four spaces per level.
- **Spacing.** One space around binary operators, after commas, and after a
  colon in annotations and dictionary entries. No
  space inside brackets, before a comma, before a call parenthesis, or around
  `=` in keyword arguments and parameter defaults.
- **Blank lines.** At most two consecutive blank lines at module level and
  one inside a body. Two blank lines separate top-level declarations. No
  blank lines at the start of a body or the end of the file.
- **Reflow.** A logical line that fits in 88 columns is joined onto one
  line. A line that does not fit breaks its outermost bracket first: each
  element goes on its own line, indented one level, with a trailing comma.
  The rule repeats inward until each line fits or cannot break further.
- **Magic trailing comma.** A bracketed list written with a trailing comma
  stays exploded one element per line, even when it would fit.
- **Comments.** Comments are preserved, attached to the line they started on.
  A comment inside a bracketed list forces that list to stay exploded.
- **Strings.** String, f-string, and triple-quoted literals are copied byte
  for byte, including docstrings.
- **Safety.** The formatter parses its output and compares the token stream,
  without layout tokens, to the input. A mismatch aborts without writing.

`aura fmt --check` reports files that would change. `aura fmt --stdin`
formats standard input to standard output. The language server provides
whole-document formatting by calling `aura fmt --stdin`, and the VS Code
extension enables format-on-save through the standard editor setting.

**Alternatives.** An AST pretty-printer was rejected because the AST drops
comments and the exact spelling of literals. A width of 100 was considered;
88 matches the widest lines in the maintained examples.

**Evidence.** Golden tests cover each rule, idempotence on every file under
`examples/` and `tutorials/`, comment preservation, magic trailing commas,
deep nesting, the token-equality guard, and the LSP formatting request.

## Ratification record

The owner delegated the Batch 4 design on 2026-10-11 with the instruction
to complete Batch 4 and make it ready for release. Every rule above is the
ratified default. The ADR status lines for ADR-0055, ADR-0056, and ADR-0063
point here for the detailed contract.
