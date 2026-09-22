# Grammar

This chapter defines the complete source grammar of Aura 0.3. The grammar is
normative once the lexer has formed tokens.
[Static Semantics](/manual/static-semantics) defines the static restrictions
the grammar does not express, such as types, visibility, ownership,
exhaustiveness, valid receivers, and API-specific rules.

## Notation

The grammar uses an Extended Backus-Naur Form (EBNF) style notation:

| Notation | Meaning |
| --- | --- |
| quoted text | A literal token |
| `name` | A nonterminal |
| `[ item ]` | Optional |
| `{ item }` | Repeats zero or more times |
| `( a \| b )` | Selects one alternative |
| a comma | Separates sequence elements. `","` is the source comma token. |
| comments inside grammar blocks | Informative |

The lexer produces two kinds of tokens used here:

- Layout tokens: `NEWLINE`, `INDENT`, `DEDENT`, and `EOF`.
- Lexical tokens: `IDENT`, `INTEGER`, `FLOAT`, `DURATION`, `STRING`,
  `FSTRING`, and `BOOLEAN`, described below.

A comma-separated source list accepts a trailing comma only when its
production explicitly adds one. The singleton tuple forms `(value,)`, `(T,)`,
and `(pattern,)` require their one comma. Multi-element tuples do not accept a
trailing comma.

`NEWLINE` in the productions means a logical newline. A physical newline
suppressed inside an open `(`, `[`, or `{` never reaches this grammar.
Delimiter continuation changes token formation, not the expression
productions. It does not add a trailing comma to any list form.

## Lexical Grammar

```ebnf
ascii-letter = "A" … "Z" | "a" … "z" ;
digit        = "0" … "9" ;
binary-digit = "0" | "1" ;
octal-digit  = "0" … "7" ;
hex-digit    = digit | "a" … "f" | "A" … "F" ;

IDENT = (ascii-letter | "_"), { ascii-letter | digit | "_" } ;

decimal-digits  = digit, { digit } ;
decimal-integer = digit, { digit | ("_", digit) } ;
hex-integer     = ("0x" | "0X"), hex-digit,
                  { hex-digit | ("_", hex-digit) } ;
binary-integer  = ("0b" | "0B"), binary-digit,
                  { binary-digit | ("_", binary-digit) } ;
octal-integer   = ("0o" | "0O"), octal-digit,
                  { octal-digit | ("_", octal-digit) } ;
INTEGER  = decimal-integer | hex-integer | binary-integer | octal-integer ;
EXPONENT = ("e" | "E"), [ "+" | "-" ], digit, { digit } ;
FLOAT    = decimal-digits, ".", decimal-digits, [ EXPONENT ]
         | decimal-digits, EXPONENT ;
DURATION = decimal-digits, ("ms" | "s" | "m") ;
BOOLEAN  = "true" | "false" ;
```

The productions carry these additional lexical rules:

- Identifiers are ASCII and case-sensitive. Unicode is allowed in string
  contents.
- Integers may be decimal, hexadecimal, binary, or octal. Before contextual
  typing, they must fit the lexer's unsigned 128-bit literal representation.
- An underscore is accepted only between digits valid for the selected base.
- Floats must be finite `f64` values at lexing time. Leading-dot and
  trailing-dot float forms are not accepted.
- Duration literals represent non-negative integral decimal milliseconds,
  seconds, or minutes. After scaling, they must fit signed 128-bit
  nanoseconds.
- A negative number is unary `-` applied to a positive literal, not one
  lexical token.

## Keywords And Contextual Words

The reserved token words are:

```text
class enum def trait impl import from mut own indirect public extern opaque
return assert if elif else and or not match case for in while break
continue pass try with as true false
```

Several words are contextual:

| Word | Contextual role |
| --- | --- |
| `from` | Introduces a from-import at module level and completes a returned-view annotation. It may also be an identifier wherever the grammar expects one. |
| `view` | Special in complete local-view, returned-view, and `return view` forms. |
| `type` | At module level, `type` followed by an identifier, optional type parameters, and `=` declares a type alias. Elsewhere it is an ordinary identifier. |
| `is` | After a comparison operand, forms the `is [not] None` test. Elsewhere it is an ordinary identifier. |
| `lambda` | Lexed as an identifier, but introduces a lambda at the start of an expression. Member and named-argument positions may still use the spelling. |

`copy`, `self`, `None`, `set`, `Self`, and `_` are lexed as identifiers. They
acquire special meaning only in the positions defined below.

## Strings And F-Strings

`STRING` is an ordinary, triple-quoted, or raw string:

- An ordinary string uses a matching pair of single or double quotes.
- A triple-quoted string uses three matching single or double quotes and may
  span physical lines.
- A raw string uses lowercase `r` immediately followed by one single or double
  quote.

Ordinary and triple-quoted strings accept the same escapes:

| Escape | Meaning |
| --- | --- |
| `\n` | line feed |
| `\t` | tab character in the decoded value |
| `\"` | double quote |
| `\'` | single quote |
| `\\` | backslash |
| `\0` | NUL |
| `\xHH` | byte-valued Unicode scalar from exactly two hexadecimal digits |
| `\u{H...}` | Unicode scalar from one or more hexadecimal digits |

An invalid scalar, unknown escape, missing digit, or missing or mismatched
closing quote is a lexical error.

A triple-quoted value preserves every scalar between its delimiters. Aura does
not trim the first or last newline, remove indentation, or normalize
whitespace.

In a raw string, backslashes are content. A backslash may keep the active
quote inside the value, and both characters are preserved. A raw string
cannot span a physical line or end in an odd run of backslashes. Raw triple
strings, raw f-strings, and byte strings are not tokens. There is no separate
character-literal token.

`FSTRING` begins with `f"` and ends at the matching double quote.
`{ expression }` interpolates an ordinary Aura expression. Literal braces work
as follows:

- Two opening braces insert one literal opening brace.
- Two closing braces insert one literal closing brace.
- In Aura 0.3, a lone closing brace outside an interpolation is also literal.

Interpolations may contain nested braces and ordinary single- or
double-quoted strings. Braces inside those strings do not change the
interpolation depth. Empty or invalid interpolations are rejected.

An interpolation may end with one top-level `:` followed by this static format
grammar:

```text
[[fill]align] [sign] [width] [","] ["." precision] [type]
```

| Part | Values |
| --- | --- |
| `align` | `<`, `^`, or `>` |
| `sign` | `+`, `-`, or a space |
| `type` | `d`, `f`, `e`, `x`, `X`, `b`, `o`, `%`, or `s` |
| `width`, `precision` | Decimal values through `1_000_000` |

The parser accepts a complete expression before it looks for the separator.
Colons inside slices, calls, dictionaries, and other nested delimiters
therefore remain expression syntax. Nested fields and dynamic specifications
are rejected. Single-quoted f-strings and conversion flags are not supported.

`\t` creates a tab in a decoded ordinary string, but a physical tab is
rejected outside a triple-quoted string. A physical tab inside a
triple-quoted string is exact string content.

## Comments, Physical Lines, And Indentation

`#` starts a comment outside a string and consumes the rest of the physical
line. There are no block comments.

The source is UTF-8. One optional UTF-8 byte-order mark (BOM) is ignored, and
only at the beginning of the file.

Layout tokens form as follows:

1. A blank or comment-only physical line produces no token and does not
   affect indentation.
2. Every other physical line is measured by its number of leading ASCII
   spaces.
3. In ordinary block-layout mode, an increase from the current indentation
   count emits one `INDENT` and pushes that exact count.
4. In ordinary block-layout mode, a decrease emits `DEDENT` tokens until an
   earlier count is reached. A count not present on the stack is
   inconsistent indentation and is rejected.
5. The line content is tokenized. An ordinary-layout line emits one
   `NEWLINE`. A continuation line suppresses it. A delimited
   expression-`match` layout island emits only the layout tokens its header
   and arms require.
6. At end of source, the remaining indentation levels emit `DEDENT`, followed
   by `EOF`.

Outside an open delimiter, Aura does not prescribe four-space indentation. It
requires a consistent return to previous block levels. The maintained
formatter and examples use four spaces.

While a `(`, `[`, or `{` is open, ordinary physical newlines and their
leading spaces produce no layout tokens. Delimiters must nest and match by
kind. A delimited expression-form `match` is a layout island: its header and
arms keep the layout tokens the match productions require, even though an
outer delimiter is still open.

Backslash continuation is unavailable. Ordinary, raw, and f-strings are
single-line. Triple-quoted ordinary strings may span physical lines without
creating layout tokens. Existing comma-separated forms do not gain a trailing
comma.

## Punctuation And Operators

```text
( ) [ ] { } : , . ?
= == != < <= > >=
+ += - -= * *= ** **= / /= // //= % %=
& &= | |= ^ ^= ~ << <<= >> >>=
->
```

There is no semicolon, assignment expression, unary plus, or lambda arrow.

## Modules And Imports

```ebnf
module = { module-element }, EOF ;

module-element = import-declaration | module-constant | item | statement ;

module-constant
    = [ "public" ], IDENT, [ ":", type ], "=", expression, NEWLINE ;

import-declaration
    = "import", identifier-path, [ "as", import-alias ], NEWLINE
    | "from", identifier-path, "import",
      import-name, { ",", import-name }, NEWLINE ;

import-name  = identifier, [ "as", import-alias ] ;
import-alias = IDENT ;

identifier-path = identifier, { ".", identifier } ;
identifier      = IDENT | "from" ;
```

Imports, module constants, items, and executable top-level statements may be
interleaved syntactically. They still run in a fixed category order:

1. Imports resolve before initializer checking.
2. Module constants initialize after their dependencies, in declaration source
   order.
3. Executable entry statements run only after constant initialization
   completes.

The compiled module represents these as separate categories. Programs MUST use
the defined category ordering and MUST NOT infer another execution order from
cross-category interleaving.

An `as` clause binds the complete imported module or declaration under the
written local alias. One from-import may mix direct and aliased names.
Aliasing does not change the target module's identity, visibility, type
identity, or package resolution path.

Wildcard imports, relative-dot imports, parenthesized import lists, and
trailing import commas are not part of the grammar.

## Items

```ebnf
item
    = [ "public" ], class-declaration
    | [ "public" ], enum-declaration
    | [ "public" ], type-alias-declaration
    | [ "public" ], function-declaration
    | [ "public" ], extern-function-declaration
    | [ "public" ], extern-opaque-declaration
    | [ "public" ], trait-declaration
    | impl-declaration ;

type-alias-declaration
    = "type", identifier, [ bounded-type-parameters ], "=", type, NEWLINE ;

extern-function-declaration
    = "extern", STRING, "def", identifier,
      "(", [ parameter-list ], ")", "->", type, NEWLINE ;

extern-opaque-declaration
    = "extern", STRING, "opaque", "class", identifier, NEWLINE ;
```

Item declarations are module-level. They are not statements and cannot appear
inside function or control-flow suites. `public` is not allowed on an
implementation block.

A type alias is recognized contextually. A module-level line that reads
`type`, an identifier, optional bounded type parameters, and `=` declares a
transparent alias for the written target type. The target may use the alias's
own type parameters. Anywhere else, `type` is an ordinary identifier. Static
checking rejects alias expansion cycles with `AU2012`.

Parsing requires the extern ABI string to be exactly `"C"`. Extern
declarations are bodyless and non-generic.
[FFI v0](/manual/ffi) restricts their parameter modes and types.

## Type References And Type Parameters

```ebnf
type
    = type-atom, { "|", type-atom } ;

type-atom
    = [ "indirect" ], type-primary,
      [ "?" ] ;

type-primary
    = identifier-path, [ "[", type-list, "]" ]
    | grouped-type
    | tuple-type
    | function-type
    | owned-callable-type ;

grouped-type = "(", type, ")" ;

owned-callable-type
    = ( "Callable" | "TaskCallable" ), "[",
      [ "mut" | "own" ], function-type, "]" ;

type-list = type, { ",", type } ;

tuple-type
    = "(", type, ",", ")"
    | "(", type, ",", type, { ",", type }, ")" ;

function-type
    = "def", "(", [ function-type-parameters ], ")", "->", function-type-result ;

function-type-result
    = type-atom
    | "view", [ "mut" ], type-atom, "from", identifier ;

function-type-parameters
    = function-type-parameter, { ",", function-type-parameter },
      [ ",", "*", ",", named-function-type-parameter,
        { ",", named-function-type-parameter } ]
    | "*", ",", named-function-type-parameter,
      { ",", named-function-type-parameter } ;

function-type-parameter
    = named-function-type-parameter
    | [ "mut" | "own" ], type, [ "=", "..." ] ;

named-function-type-parameter
    = identifier, ":", [ "mut" | "own" ], type, [ "=", "..." ] ;

plain-type-parameters
    = "[", identifier, { ",", identifier }, "]" ;

bounded-type-parameters
    = "[", bounded-type-parameter,
      { ",", bounded-type-parameter }, "]" ;

bounded-type-parameter
    = identifier, [ ":", type, { "+", type } ] ;
```

### Function Types

A function type spells a complete callable contract, for example
`def(int32, label: str, *, retries: int32 = ...) -> bool`.

| Slot form | Meaning |
| --- | --- |
| bare type | Shared access |
| `mut` type | Requires caller-visible mutable access |
| `own` type | Transfers the argument |
| unnamed slot | Positional-only |
| `name: type` | Exposes a name |
| slot after `*` | Must be named, and is keyword-only |
| `= ...` | Promises that the target supplies a default |

A literal or expression after `=` in a type is rejected with `AU1101`.

The same `*` boundary rule applies to declaration and lambda parameter lists.
A list has at most one `*`, it must be followed by at least one named
parameter, and it adds no variadics.

`indirect` is invalid on a function type, because the value is already a code
pointer.

`Callable[...]` and `TaskCallable[...]` wrap one function type as owned
callable storage. Inside the brackets:

- a bare `def` is Shared
- `mut def` is Mutable
- `own def` is Consuming

`mut def` and `own def` are not valid outside those brackets. `indirect` is
invalid on owned callable types.

### Optional, Grouped, And Tuple Types

There is no `?` type suffix. An optional type is written as the union
`T | None`, including when `T` is a tuple type. A stray `?` after a type is an
ordinary AU1101 parse error.

Type lists and type-parameter lists are nonempty when brackets are present.
They do not accept trailing commas.

- `(T,)` is a singleton tuple type.
- `(T)` is a grouped type that denotes `T` itself. It exists for precedence.
- `()` and a trailing comma on a multi-element tuple type are rejected.

The grammar places `indirect` before any type primary. Statically, it is valid
only on a complete named type reference where the recursive-field rules
permit it. An `indirect` tuple type is rejected.

### Union Types

`A | B` is an anonymous closed union of its written members. `|` binds more
loosely than `indirect`, so `indirect Node | None` has the members
`indirect Node` and `None`. A union with a trailing `None` member is the
optional form of its other members.

A union may appear wherever `type` appears, including type-argument lists,
tuple elements, and function-type parameters. A function-type result is one
type atom, so grouping is needed in two cases:

- a union result, as in `def() -> (int64 | str)`
- a function type that is itself a union member, as in
  `(def() -> int64) | None`

A missing member before or after `|` is a parse error.

Written members normalize statically:

- Nested unions flatten.
- Duplicate and reordered spellings name the same type.
- A union with one distinct member is that member.
- `None` is the unit member.

[Types](/manual/types) gives the member rules.

## Classes

```ebnf
class-declaration
    = [ "copy" ], "class", identifier,
      [ bounded-type-parameters ],
      ":", NEWLINE, INDENT,
      class-member, { class-member },
      DEDENT ;

class-member
    = "pass", NEWLINE
    | [ "public" ], field-declaration
    | [ "public" ], method-declaration ;

field-declaration
    = identifier, ":", type,
      [ "=", expression ], NEWLINE ;
```

`copy` is contextual and is recognized only immediately before `class`.
Fields and methods may be interleaved. `pass` permits an otherwise empty class
body. A comment-only body is not a suite.

## Enums

```ebnf
enum-declaration
    = "enum", identifier, [ bounded-type-parameters ],
      ":", NEWLINE, INDENT,
      enum-variant, { enum-variant },
      DEDENT ;

enum-variant
    = identifier, [ "(", enum-payload-list, ")" ], NEWLINE ;

enum-payload-list
    = type, { ",", type }
    | identifier, ":", type,
      { ",", identifier, ":", type } ;
```

A variant payload list is either entirely positional or entirely named. Empty
payload parentheses and mixed positional and named declarations are rejected.
A variant with no payload omits the parentheses.

## Functions, Methods, And Parameters

```ebnf
function-declaration
    = "def", identifier, [ bounded-type-parameters ],
      "(", [ parameter-list ], ")",
      [ return-annotation ],
      ":", NEWLINE, suite ;

method-declaration
    = "def", identifier, [ bounded-type-parameters ],
      "(", [ method-parameter-list ], ")",
      [ return-annotation ],
      ":", NEWLINE, suite ;

parameter-list
    = parameter, { ",", parameter },
      [ ",", keyword-only-parameters ]
    | keyword-only-parameters ;

keyword-only-parameters
    = "*", ",", parameter, { ",", parameter } ;

method-parameter-list
    = receiver, [ ",", parameter-list ]
    | parameter-list ;

receiver
    = "self"
    | "mut", "self"
    | "own", "self" ;

parameter
    = identifier, ":",
      [ "mut" | "own" ],
      type,
      [ "=", expression ] ;

return-annotation
    = "->", type
    | "->", "view", [ "mut" ], type,
      "from", identifier ;
```

A receiver, when present, is the first method parameter. Each capability has
exactly one spelling:

| Capability | Receiver | Ordinary parameter |
| --- | --- | --- |
| Shared | `self` | `name: T` |
| Mutable | `mut self` | `name: mut T` |
| Consuming | `own self` | `name: own T` |

A first method parameter written as `self: Type` is rejected. It is not
interpreted as an ordinary parameter. Use one of the receiver forms above.

Call sites pass the value directly. They never prefix an argument with a
capability.

Bare means shared access for every type, including declaration-known copy
types. An ordinary return type annotation is an owned return. A view
annotation names one receiver or ordinary parameter as its origin. Static
semantics validates the view's kind, provenance, and caller-place
requirements.

Parameter lists, calls, and return annotations do not accept trailing commas.
Static checking also restricts duplicate names, default placement and
availability, and mutable task targets.

## Traits And Implementations

```ebnf
trait-declaration
    = "trait", identifier, [ plain-type-parameters ], ":",
      [ type, { ",", type }, ":" ],
      NEWLINE, INDENT,
      trait-member, { trait-member },
      DEDENT ;

trait-member
    = "pass", NEWLINE
    | trait-method ;

trait-method
    = "def", identifier, [ bounded-type-parameters ],
      "(", [ method-parameter-list ], ")",
      [ return-annotation ],
      ( NEWLINE | ":", NEWLINE, suite ) ;

impl-declaration
    = "impl", [ bounded-type-parameters ],
      identifier, [ "[", type-list, "]" ],
      "for", type,
      ":", NEWLINE, INDENT,
      impl-member, { impl-member },
      DEDENT ;

impl-member
    = "pass", NEWLINE
    | method-declaration ;
```

Trait-declaration type parameters use the plain form. Bounds on them are
expressed through supertraits or method constraints, not inline in the trait
parameter list.

A trait method is either signature-only, with a newline immediately after the
return annotation, or provides one default body after `:`.

The second colon in a trait header separates an optional comma-separated
supertrait list from the body, for example `trait Child: Parent, Named:`.

## Suites And Statements

```ebnf
suite = INDENT, statement, { statement }, DEDENT ;

statement
    = assignment-statement
    | view-statement
    | return-statement
    | assert-statement
    | pass-statement
    | if-statement
    | match-statement
    | for-statement
    | with-statement
    | while-statement
    | break-statement
    | continue-statement
    | expression-statement ;

statement-end = NEWLINE | DEDENT | EOF ;

assignment-statement
    = [ "mut" ], assignment-target,
      [ ":", type ],
      assignment-operator,
      expression, statement-end
    | unpack-target, "=", expression, statement-end ;

view-statement
    = "view", [ "mut" ], identifier,
      "=", expression, statement-end ;

assignment-target
    = identifier,
      { ".", identifier | "[", expression, "]" } ;

unpack-target
    = binding-target, ",", binding-target,
      { ",", binding-target }
    | "(", binding-target-list, ")" ;

binding-target-list
    = binding-target, ","
    | binding-target, ",", binding-target,
      { ",", binding-target } ;

binding-target
    = identifier
    | "(", binding-target-list, ")" ;

assignment-operator
    = "=" | "+=" | "-=" | "*=" | "**=" | "/=" | "//=" | "%="
    | "&=" | "|=" | "^=" | "<<=" | ">>=" ;

return-statement
    = "return", [ expression ], statement-end
    | "return", "view", [ "mut" ],
      expression, statement-end ;
assert-statement     = "assert", non-tuple-expression,
                       [ ",", non-tuple-expression ], statement-end ;
pass-statement       = "pass", NEWLINE ;
break-statement      = "break", NEWLINE ;
continue-statement   = "continue", NEWLINE ;
expression-statement = expression, statement-end ;
```

Assignment targets follow these rules:

- An annotation is valid only on a simple-name assignment target.
- A place assignment target cannot contain calls.
- An unpack target contains only names and recursively parenthesized
  binding-target lists. It uses plain `=`, has no annotation or leading
  `mut`, and must match one exact tuple shape.
- The top-level comma distinguishes `left, right = pair` from an expression.
  Parentheses group or nest an unpack target.

One-line suites are not supported.

The optional top-level comma in an assertion belongs to `assert-statement`.
Tuple operands must be parenthesized.

## Conditional And Loop Statements

```ebnf
if-statement
    = "if", expression, ":", NEWLINE, suite,
      { "elif", expression, ":", NEWLINE, suite },
      [ "else", ":", NEWLINE, suite ] ;

while-statement
    = "while", expression, ":", NEWLINE, suite ;

for-statement
    = "for", loop-target, "in",
      [ "mut" | "own" ],
      expression, ":", NEWLINE, suite ;

loop-target = identifier | unpack-target ;
```

The loop target is one identifier or a recursively nested tuple unpack target.
Tuple leaves inherit the ownership provenance of the yielded element. A tuple
target is rejected with `mut` iteration, because the minimal tuple surface has
no recursive writeback. Loop `else` clauses are not supported.

The iteration modifier depends on the iterable:

| Iterable | Without a modifier | Explicit `mut` or `own` |
| --- | --- | --- |
| Collection place | Shared iteration | Allowed |
| Queue | Receives owned items | Rejected, because iteration is a receive operation |
| Range | Yields independent copy `int64` values | Rejected, because there is no place or ownership transfer to modify |

The iterable position also recognizes two compiler-known call shapes,
`enumerate(expression)` and `zip(expression, expression)`. They are not
values and have no production outside this position. Static semantics rejects
either name elsewhere. A user declaration of the name shadows the loop form.
Explicit ownership modifiers are rejected for both, because they iterate with
the bare-loop shared default.

## `with` Statements

```ebnf
with-statement
    = "with", identifier, "=", expression,
      ":", NEWLINE, suite
    | "with", expression, "as", identifier,
      ":", NEWLINE, suite ;
```

The two forms are equivalent. Static semantics requires a supported resource
and a fresh binding.

## Patterns And Statement Matches

```ebnf
match-statement
    = "match", [ "mut" | "own" ],
      expression, ":", NEWLINE,
      INDENT, match-statement-arm,
      { match-statement-arm }, DEDENT ;

match-statement-arm
    = "case", pattern, [ "if", expression ],
      ":", NEWLINE, suite ;

pattern
    = closed-pattern, { "|", closed-pattern } ;

closed-pattern
    = "_"
    | BOOLEAN
    | STRING
    | FLOAT
    | INTEGER
    | "-", (INTEGER | FLOAT)
    | tuple-pattern
    | type-pattern
    | binding-pattern
    | variant-pattern ;

binding-pattern = IDENT ;

type-pattern = type, "as", IDENT ;

variant-pattern
    = identifier-path,
      [ "(", [ pattern, { ",", pattern } ], ")" ] ;

tuple-pattern
    = "(", pattern, ")"
    | "(", pattern, ",", ")"
    | "(", pattern, ",", pattern,
      { ",", pattern }, ")" ;
```

Pattern parsing uses these contextual rules:

- `Type as name` selects a direct union member or the same singleton type. A
  type alias must expand to one member. `None` selects the unit member.
- Exact `_` is the wildcard.
- One unparenthesized, unqualified name that begins with lowercase ASCII or
  `_` is a binding.
- A dotted name, a capitalized name, or any name followed by parentheses is a
  variant pattern.
- Payload patterns are positional, even when the variant declaration used
  named payload fields.
- A parenthesized comma form is a fixed-arity recursive tuple pattern.
- `|` has the lowest pattern precedence and joins alternatives.
- Parentheses group one pattern when no comma is present.

Every alternative of an or-pattern must bind the same names with identical
exact types and capabilities.

A guard is an ordinary expression, checked as exactly `bool`. Its pattern's
bindings are in scope.

An unguarded top-level binding is an irrefutable catch-all and must be the
final arm. A guarded top-level binding does not contribute to exhaustiveness.

There are no range, collection destructuring, rest, named-payload, duration,
or f-string patterns.

`match mut` rejects a tuple pattern, because mutable tuple reconstruction and
writeback are not part of the minimal surface. Statement match arms always
contain suites. `case pattern: statement` is not valid.

## Expressions And Precedence

From lowest to highest precedence:

| Level | Form | Associativity |
| --- | --- | --- |
| 1 | conditional expression | right |
| 2 | `or` | left |
| 3 | `and` | left |
| 4 | prefix `not` | right |
| 5 | `==`, `!=`, `<`, `<=`, `>`, `>=`, `in`, `not in` | chained left to right |
| 6 | `|` | left |
| 7 | `^` | left |
| 8 | `&` | left |
| 9 | `<<`, `>>` | left |
| 10 | `+`, `-` | left |
| 11 | `*`, `/`, `//`, `%` | left |
| 12 | prefix `match`, `try`, unary `-`, unary `~` | right/prefix |
| 13 | `**` | right |
| 14 | specialization, indexing, slicing, member access, call, numeric cast | left-to-right postfix chain |
| 15 | primary | — |

```ebnf
expression           = lambda-expression | non-tuple-expression ;
non-tuple-expression = conditional-expression ;

lambda-expression
    = "lambda", [ lambda-capture-list ],
      [ lambda-parameter-list ], ":", expression ;

lambda-parameter-list
    = lambda-parameter, { ",", lambda-parameter },
      [ ",", "*", ",", lambda-parameter, { ",", lambda-parameter } ]
    | "*", ",", lambda-parameter, { ",", lambda-parameter } ;

lambda-capture-list
    = "[", lambda-capture,
      { ",", lambda-capture }, "]" ;

lambda-capture
    = [ "mut" | "own" ], identifier ;

lambda-parameter
    = [ "mut" | "own" ], identifier ;

conditional-expression
    = or-expression,
      [ "if", or-expression, "else", conditional-expression ] ;

or-expression
    = and-expression, { "or", and-expression } ;

and-expression
    = not-expression, { "and", not-expression } ;

not-expression
    = { "not" }, comparison-expression ;

comparison-expression
    = bitwise-or-expression,
      ( { comparison-operator, bitwise-or-expression } | none-test ) ;

none-test
    = "is", [ "not" ], "None" ;

comparison-operator
    = "==" | "!=" | "<" | "<=" | ">" | ">=" | "in" | "not", "in" ;

bitwise-or-expression
    = bitwise-xor-expression, { "|", bitwise-xor-expression } ;

bitwise-xor-expression
    = bitwise-and-expression, { "^", bitwise-and-expression } ;

bitwise-and-expression
    = shift-expression, { "&", shift-expression } ;

shift-expression
    = additive-expression, { ("<<" | ">>"), additive-expression } ;

additive-expression
    = multiplicative-expression,
      { ("+" | "-"), multiplicative-expression } ;

multiplicative-expression
    = prefix-expression,
      { ("*" | "/" | "//" | "%"), prefix-expression } ;

prefix-expression
    = match-expression
    | "try", prefix-expression
    | "-", prefix-expression
    | "~", prefix-expression
    | power-expression ;

power-expression
    = postfix-expression, [ "**", prefix-expression ] ;

postfix-expression
    = primary-expression,
      { specialization-suffix
      | index-suffix
      | member-suffix
      | call-suffix
      | numeric-cast-suffix } ;

index-suffix
    = "[", expression, { ",", expression }, "]"
    | "[", [ expression ], ":", [ expression ], "]" ;
member-suffix = ".", identifier ;
call-suffix   = "(", [ argument, { ",", argument } ], ")" ;
argument      = [ identifier, "=" ], expression ;

numeric-cast-suffix = "as", numeric-type ;

numeric-type
    = "int" | "int8" | "int16" | "int32" | "int64" | "int128" | "intsize"
    | "uint8" | "uint16" | "uint32" | "uint64" | "uint128" | "uintsize"
    | "float32" | "float64" ;
```

### Associativity

Conditional expressions associate to the right. The condition is an
`or-expression`. The two value arms may contain nested conditional
expressions through grouping or through the recursive alternative arm.

Arithmetic, shift, bitwise, and Boolean chains fold to the left. Power is the
exception and associates to the right. Power binds more tightly than a unary
operator on its left, while its right operand may begin with unary `-` or
`~`. Casts bind more tightly than power and arithmetic.

### Comparison Chains

Equality, ordering, and membership share one comparison level. They chain the
Python way instead of folding to the left:

- `a < b <= c` is one chain of two links over three operands.
- A chain of `n` operators means the conjunction of its `n` adjacent
  comparisons. Each operand is evaluated at most once.
- `not a == b` means `not (a == b)`, because prefix `not` binds looser than
  the comparison level.
- `a not in b` is one comparison operator.

`is` is a contextual word. `value is None` and `value is not None` are
complete comparison-level forms that take no further comparison operator on
either side. `a is None == b` and `a < b is None` are therefore syntax errors,
not chains.

### Indexing And Slicing

Comma-separated index expressions are accepted only for `Array[T]`, which
requires one `int64` coordinate per runtime axis. Other indexable types take
one index expression.

The one-colon bracket forms are owned slices. Each endpoint is optional, so
`value[start:end]`, `value[:end]`, `value[start:]`, and `value[:]` all use the
second `index-suffix` alternative. On `Array[T]`, the range copies the first
axis.

A second colon is reserved step syntax. It is rejected with `AU2005` and is
not part of the accepted grammar. A slice suffix is an expression only and
cannot be an assignment target.

## Primary Expressions And Literals

```ebnf
primary-expression
    = identifier
    | INTEGER
    | DURATION
    | FLOAT
    | BOOLEAN
    | STRING
    | FSTRING
    | parenthesized-expression
    | list-literal
    | brace-literal
    | list-comprehension
    | set-comprehension
    | dictionary-comprehension ;

list-literal
    = "[", [ expression, { ",", expression } ], "]" ;

brace-literal
    = "{", "}"
    | "{", expression, { ",", expression }, "}"
    | "{", expression, ":", expression,
      { ",", expression, ":", expression }, "}" ;

list-comprehension
    = "[", expression, comprehension-clauses, "]" ;

set-comprehension
    = "{", expression, comprehension-clauses, "}" ;

dictionary-comprehension
    = "{", expression, ":", expression,
      comprehension-clauses, "}" ;

comprehension-clauses
    = comprehension-for,
      { comprehension-if | comprehension-for } ;

comprehension-for
    = "for", loop-target, "in", comprehension-component ;

comprehension-if
    = "if", comprehension-component ;

comprehension-component
    = lambda-expression | or-expression ;

parenthesized-expression
    = "(", expression, ")"
    | tuple-expression ;

tuple-expression
    = "(", expression, ",", ")"
    | "(", expression, ",", expression,
      { ",", expression }, ")" ;
```

### Lambdas

Lambda parameters take their types from an expected structural function
type. That type's result also constrains the body. A zero-parameter lambda may
infer its result from the body. The colon introduces one expression, not a
suite.

Lambda parameters do not accept annotations, defaults, generics, or a trailing
comma. A capture list is exhaustive and nonempty. Each entry names an outer
local place and requests shared, mutable, or by-value owned capture.

### Tuples, Sets, And Dictionaries

`(value)` is grouping and `(value,)` is a singleton tuple. Tuple value
expressions require parentheses. An unparenthesized comma is accepted only in
an unpack target. `()` and a trailing comma on a multi-element tuple are
rejected.

A nonempty brace literal is a dictionary when its first element is followed by
`:`, and a set otherwise. `{}` is an empty dictionary. An empty set uses the
typed `set[T]()` constructor.

### Comprehensions

A comprehension has one or more `for` clauses. A clause may be followed by
zero or more `if` filters before another `for` clause.

Clause targets use `loop-target`, including recursive tuple targets. The
iterable position has no `mut` or `own` modifier, because comprehension
clauses always use the bare-loop contract.

Components use the non-conditional `or-expression`, which keeps a following
comprehension `if` distinct from a conditional expression. Use parentheses
when an iterable or filter itself needs a conditional expression. A lambda is
still syntactically admissible as a component. It is then subject to the
ordinary iterable rule or the exact-Boolean static rule.

The result expression, or the dictionary key and value expressions, may be any
expression.

These are invalid:

- a comma after the comprehension clauses
- a mixture of comma-separated literal entries and clauses

Generator expressions are not part of this grammar.

## Explicit Specialization

```ebnf
specialization-suffix = "[", type-list, "]" ;
```

Specialization and indexing use the same brackets, so the parser and static
context disambiguate them. Brackets form specialization when their contents
scan as one or more type references and either:

1. `(` follows and the base is a name or member, or
2. `.` follows and the final target name begins with uppercase ASCII.

Otherwise, a bare bracket suffix starts as an index expression. Static
resolution reinterprets `function[Types]` as explicit specialization when
`function` resolves to a generic named function and the complete expression is
used as a function value. In every other case the brackets stay indexing.

| Expression | Parse |
| --- | --- |
| `Box[int32](value)` | Specialization |
| `Result[int32, str].Ok(1)` | Specialization |
| `show[int32]` | May produce one concrete function value |
| `value[index]` | Indexing |

A top-level colon inside the brackets selects slicing instead of
specialization or indexing. Slice endpoints are expressions, checked under the
exact rules in
[Static Semantics](/manual/static-semantics#indexing-slicing-and-members).

## Match Expressions

```ebnf
match-expression
    = "match", [ "mut" | "own" ],
      expression, ":", NEWLINE,
      INDENT, match-expression-arm,
      { match-expression-arm }, DEDENT ;

match-expression-arm
    = "case", pattern, [ "if", expression ], ":",
      ( expression, match-expression-arm-end
      | NEWLINE, INDENT, expression, statement-end, DEDENT ) ;

match-expression-arm-end
    = NEWLINE | DEDENT | ")" | "]" | "}" | EOF ;
```

A match-expression arm contains exactly one expression. The expression is
either inline after the colon or on one indented following line. The arm is
not a general statement suite.

A complete match expression may appear in any expression position, such as a
return, initializer, call argument, collection element, or grouping
expression.

Inside a continued delimiter, the match header and arms form a layout island
and keep their required layout tokens. The containing delimiter may close
after the final inline arm or on its own following line.

## Syntactic Complexity Limits

The implementation rejects source that exceeds the parser's complexity budget
instead of risking host stack exhaustion:

| Construct | Limit |
| --- | --- |
| Nested expressions, prefix forms, parentheses, types, patterns, and statements | 128 parser levels |
| Binary-operator and postfix chains | The 128th chained operation is rejected |
| One comprehension | A 128th combined `for` clause or `if` filter is rejected |
| F-string interpolation brace nesting | 128 |

These are observable implementation limits of Aura 0.3. Inputs that exceed
them must be rejected cleanly.

## Syntax Not In Aura 0.3

The grammar intentionally excludes:

- semicolons and multiple statements on one physical line
- backslash line continuation
- multiline f-strings
- local item declarations, decorators, and attributes
- wildcard, relative-dot, and parenthesized import syntax
- ordinary trailing commas other than the required singleton-tuple comma
- collection, range, rest, and class patterns
- call-site capability annotations
- exception statements, `raise`, and `yield`
- generator expressions and generator functions

If a form is absent from this grammar, examples and books must not present it
as implemented Aura.

Design record: [ADR-0052, anonymous closed union types](https://github.com/johnolafenwa/Aura/blob/main/architecture_docs/decisions/0052-anonymous-closed-union-types.md).
