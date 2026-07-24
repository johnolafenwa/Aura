# Grammar

This chapter defines the complete source grammar of Aurora 0.1. The grammar is normative after lexical token formation. Static restrictions—types, visibility, ownership, exhaustiveness, valid receivers, and API-specific rules—are defined by [Static Semantics](/manual/static-semantics).

## Notation

The grammar uses an EBNF-style notation:

- quoted text is a literal token
- `name` is a nonterminal
- `[ item ]` is optional
- `{ item }` repeats zero or more times
- `( a | b )` selects one alternative
- a comma in the grammar separates sequence elements; `","` is the source comma token
- comments inside grammar blocks are informative

`NEWLINE`, `INDENT`, `DEDENT`, and `EOF` are layout tokens produced by the lexer. `IDENT`, `INTEGER`, `FLOAT`, `DURATION`, `STRING`, `FSTRING`, and `BOOLEAN` are lexical tokens described below.

Comma-separated source lists do not accept a trailing comma unless their
production explicitly adds one. The singleton tuple forms `(value,)`, `(T,)`,
and `(pattern,)` require their one comma; multi-element tuples do not accept a
trailing comma.

`NEWLINE` in the productions means a logical newline. A physical newline
suppressed inside an open `(`, `[`, or `{` never reaches this grammar.
Delimiter continuation changes token formation, not the expression
productions; it does not add a trailing comma to any list form.

## Lexical Grammar

```ebnf
ascii-letter = "A" … "Z" | "a" … "z" ;
digit        = "0" … "9" ;
hex-digit    = digit | "a" … "f" | "A" … "F" ;

IDENT = (ascii-letter | "_"), { ascii-letter | digit | "_" } ;

INTEGER  = digit, { digit } ;
EXPONENT = ("e" | "E"), [ "+" | "-" ], digit, { digit } ;
FLOAT    = INTEGER, ".", digit, { digit }, [ EXPONENT ]
         | INTEGER, EXPONENT ;
DURATION = INTEGER, ("ms" | "s" | "m") ;
BOOLEAN  = "true" | "false" ;
```

Identifiers are ASCII and case-sensitive. Unicode is allowed in string contents. Integers are decimal and must fit the lexer’s unsigned 128-bit literal representation before contextual typing. Floats must be finite `f64` values at lexing time. Duration literals represent non-negative integral milliseconds, seconds, or minutes and must fit signed 128-bit nanoseconds after scaling.

There are no hexadecimal, octal, binary, underscored, leading-dot, or trailing-dot numeric forms. A negative number is unary `-` applied to a positive literal, not one lexical token.

## Keywords And Contextual Words

The reserved token words are:

```text
class enum def trait impl import from mut borrow own indirect public
return assert if elif else and or not match case for in while break
continue pass try with as true false
```

`from` is contextual: it introduces a from-import at module level and may also be used as an identifier where the grammar expects one. `copy`, `self`, `None`, `Set`, `Self`, and `_` are lexed as identifiers and acquire special meaning only in the positions defined below.

## Strings And F-Strings

`STRING` is a single-line ordinary string delimited by a matching pair of
single quotes or double quotes. Both forms produce the same token value and
accept the same escapes:

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
closing quote is a lexical error. Triple-quoted, raw, and byte-string literals
are not part of Aurora 0.1. There is no separate character-literal token.

`FSTRING` begins with `f"` and ends at the matching double quote.
`{ expression }` interpolates an ordinary Aurora expression. Two opening braces insert one
literal opening brace, and two closing braces insert one literal closing brace.
A lone closing brace outside an interpolation is also literal in Aurora 0.1.
Interpolations may contain nested braces and ordinary single- or double-quoted
strings; braces inside those strings do not change interpolation depth. Empty
or invalid interpolations are rejected. Single-quoted f-strings, conversion
flags, and format-specifier mini-languages are not supported.

Although `\t` creates a tab in a decoded string, a literal physical tab character anywhere in a source line is rejected before tokenization, including inside a comment or string.

## Comments, Physical Lines, And Indentation

`#` starts a comment outside a string and consumes the rest of the physical line. There are no block comments.

The source is UTF-8. One optional UTF-8 BOM is ignored only at the beginning of the file.

Layout token formation is:

1. A blank or comment-only physical line produces no token and does not affect indentation.
2. Every other physical line is measured by its number of leading ASCII spaces.
3. In ordinary block-layout mode, an increase from the current indentation
   count emits one `INDENT` and pushes that exact count.
4. In ordinary block-layout mode, a decrease emits `DEDENT` tokens until an
   earlier count is reached. A count not present on the stack is inconsistent
   indentation and is rejected.
5. The line content is tokenized. An ordinary-layout line emits one
   `NEWLINE`; a continuation line suppresses it; and a delimited
   expression-`match` layout island emits only the layout tokens required by
   its header and arms.
6. At end of source, remaining indentation levels emit `DEDENT`, followed by `EOF`.

Outside an open delimiter, Aurora does not prescribe four-space indentation;
it requires consistent return to previous block levels. The maintained
formatter and examples use four spaces.

While a `(`, `[`, or `{` remains open, ordinary physical newlines and their
leading spaces do not produce layout tokens. Delimiters must nest and match by
kind. A delimited expression-form `match` is a layout island: its header and
arms retain the layout tokens required by the match productions even though an
outer delimiter remains open.

Backslash continuation is not part of Aurora 0.1. Ordinary strings and
f-strings remain single-line, and existing comma-separated forms do not gain a
trailing comma.

## Punctuation And Operators

```text
( ) [ ] { } : , . ?
= == != < <= > >=
+ += - -= * *= / /= // //= % %=
->
```

There is no semicolon, assignment expression, exponentiation, unary plus,
bitwise operator, lambda arrow, or conditional-expression operator.

## Modules And Imports

```ebnf
module = { module-element }, EOF ;

module-element = import-declaration | item | statement ;

import-declaration
    = "import", identifier-path, NEWLINE
    | "from", identifier-path, "import",
      identifier, { ",", identifier }, NEWLINE ;

identifier-path = identifier, { ".", identifier } ;
identifier      = IDENT | "from" ;
```

Imports, items, and executable top-level statements may be interleaved syntactically. The compiled module represents imports, items, and top-level statements as separate categories; programs MUST NOT depend on their original cross-category interleaving as an execution order.

There are no import aliases, wildcard imports, relative-dot imports, parenthesized import lists, or trailing import commas.

## Items

```ebnf
item
    = [ "public" ], class-declaration
    | [ "public" ], enum-declaration
    | [ "public" ], function-declaration
    | [ "public" ], trait-declaration
    | impl-declaration ;
```

`public` is not allowed on an implementation block. Item declarations are module-level; they are not statements and cannot appear inside function/control-flow suites.

## Type References And Type Parameters

```ebnf
type
    = [ "indirect" ], type-primary,
      [ "?" ] ;

type-primary
    = identifier-path, [ "[", type-list, "]" ]
    | tuple-type ;

type-list = type, { ",", type } ;

tuple-type
    = "(", type, ",", ")"
    | "(", type, ",", type, { ",", type }, ")" ;

plain-type-parameters
    = "[", identifier, { ",", identifier }, "]" ;

bounded-type-parameters
    = "[", bounded-type-parameter,
      { ",", bounded-type-parameter }, "]" ;

bounded-type-parameter
    = identifier, [ ":", type, { "+", type } ] ;
```

`T?` denotes `Option[T]`, including when `T` is a tuple type. Type and
type-parameter lists are nonempty when brackets are present and do not accept
trailing commas. `(T,)` is a singleton tuple type; `(T)` is not a type. `()`
and a trailing comma on a multi-element tuple type are rejected. Although the
grammar places `indirect` before any type primary, it is statically valid only
on the complete named type reference where recursive-field rules permit it;
an `indirect` tuple type is rejected.

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

`copy` is contextual and is recognized only immediately before `class`. Fields and methods may be interleaved. `pass` permits an otherwise empty class body; a comment-only body is not a suite.

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

A variant payload list is either entirely positional or entirely named. Empty payload parentheses and mixed positional/named declarations are rejected. A no-payload variant omits parentheses.

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
    = parameter, { ",", parameter } ;

method-parameter-list
    = receiver, [ ",", parameter, { ",", parameter } ]
    | parameter-list ;

receiver
    = "self"
    | "borrow", "self"
    | "own", "self"
    | "borrow", "mut", "self" ;

parameter
    = identifier, ":",
      [ "own" | "borrow", [ "mut" ], [ borrow-label ] ],
      type,
      [ "=", expression ] ;

borrow-label = "[", identifier, "]" ;

return-annotation
    = "->",
      [ "borrow", [ "mut" ], [ borrow-label ] ],
      type ;
```

A receiver, when present, is the first method parameter. Bare `self` and `borrow self` are the two spellings of a shared receiver, `own self` is consuming, and `borrow mut self` is mutable. A first method parameter written as `self: Type` is rejected rather than interpreted as an ordinary parameter; use one of the receiver forms above. Ordinary parameter modifiers appear after the colon: `own T`, `borrow T`, or `borrow mut T`. Call sites pass the value directly and never prefix an argument with an ownership modifier.

Parameter lists, calls, and return annotations do not accept trailing commas. Static checking further restricts duplicate names, default placement/availability, mutable-borrow task targets, and borrowed return sources.

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

Trait-declaration type parameters use the plain form; bounds on those parameters are expressed through supertraits or method constraints rather than inline bounds in the trait parameter list. Trait methods may be signature-only (newline immediately after the return annotation) or provide one default body after `:`.

The second colon in a trait header separates an optional comma-separated supertrait list from the body, for example `trait Child: Parent, Named:`.

## Suites And Statements

```ebnf
suite = INDENT, statement, { statement }, DEDENT ;

statement
    = assignment-statement
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

assignment-operator = "=" | "+=" | "-=" | "*=" | "/=" | "//=" | "%=" ;

return-statement     = "return", [ expression ], statement-end ;
assert-statement     = "assert", non-tuple-expression,
                       [ ",", non-tuple-expression ], statement-end ;
pass-statement       = "pass", NEWLINE ;
break-statement      = "break", NEWLINE ;
continue-statement   = "continue", NEWLINE ;
expression-statement = expression, statement-end ;
```

An annotation is valid only on a simple-name assignment target. Place
assignment targets cannot contain calls. An unpack target contains only names
and recursively parenthesized binding-target lists; it uses plain `=`, has no
annotation or leading `mut`, and must match one exact tuple shape. The
top-level comma distinguishes `left, right = pair` from an expression.
Parentheses group or nest an unpack target. One-line suites are not supported.
The optional top-level comma in an assertion belongs to `assert-statement`;
tuple operands must be parenthesized.

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
      [ "own" | "borrow", [ "mut" ] ],
      expression, ":", NEWLINE, suite ;

loop-target = identifier | unpack-target ;
```

The loop target is one identifier or a recursively nested tuple unpack target.
Tuple leaves inherit the yielded element's ownership provenance. A tuple
target is rejected with `borrow mut` iteration because the minimal tuple
surface has no recursive writeback. Loop `else` clauses are not supported.
Static semantics resolve the absent modifier by iterable kind. Explicit
modifiers are rejected for Queue iteration because it is a receive operation
rather than collection-place traversal.

## `with` Statements

```ebnf
with-statement
    = "with", identifier, "=", expression,
      ":", NEWLINE, suite
    | "with", expression, "as", identifier,
      ":", NEWLINE, suite ;
```

The two forms are equivalent. Static semantics require a supported resource and a fresh binding.

## Patterns And Statement Matches

```ebnf
match-statement
    = "match", [ "borrow", [ "mut" ] ],
      expression, ":", NEWLINE,
      INDENT, match-statement-arm,
      { match-statement-arm }, DEDENT ;

match-statement-arm
    = "case", pattern, ":", NEWLINE, suite ;

pattern
    = "_"
    | BOOLEAN
    | STRING
    | FLOAT
    | INTEGER
    | "-", (INTEGER | FLOAT)
    | tuple-pattern
    | binding-pattern
    | variant-pattern ;

binding-pattern = IDENT ;

variant-pattern
    = identifier-path,
      [ "(", [ pattern, { ",", pattern } ], ")" ] ;

tuple-pattern
    = "(", pattern, ",", ")"
    | "(", pattern, ",", pattern,
      { ",", pattern }, ")" ;
```

Pattern parsing uses these contextual rules:

- exact `_` is the wildcard
- one unparenthesized, unqualified name beginning with lowercase ASCII or `_` is a binding
- a dotted name, a capitalized name, or any name followed by parentheses is a variant pattern
- payload patterns are positional even when the variant declaration used named payload fields
- a parenthesized comma form is a fixed-arity recursive tuple pattern

There are no guards, alternatives, ranges, collection destructuring, rest
patterns, named-payload patterns, duration patterns, or f-string patterns.
`match borrow mut` rejects a tuple pattern because mutable tuple
reconstruction/writeback is not part of the minimal surface. Statement match
arms always contain suites; `case pattern: statement` is not valid.

## Expressions And Precedence

From lowest to highest precedence:

| Level | Form | Associativity |
| --- | --- | --- |
| 1 | `or` | left |
| 2 | `and` | left |
| 3 | prefix `not` | right |
| 4 | `==`, `!=`, `<`, `<=`, `>`, `>=` | non-associative in 0.1 |
| 5 | `+`, `-` | left |
| 6 | `*`, `/`, `//`, `%` | left |
| 7 | prefix `match`, `try`, unary `-` | right/prefix |
| 8 | specialization, indexing, member access, call, numeric cast | left-to-right postfix chain |
| 9 | primary | — |

```ebnf
expression           = non-tuple-expression ;
non-tuple-expression = or-expression ;

or-expression
    = and-expression, { "or", and-expression } ;

and-expression
    = not-expression, { "and", not-expression } ;

not-expression
    = { "not" }, comparison-expression ;

comparison-expression
    = additive-expression,
      [ comparison-operator, additive-expression ] ;

comparison-operator
    = "==" | "!=" | "<" | "<=" | ">" | ">=" ;

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
    | postfix-expression ;

postfix-expression
    = primary-expression,
      { specialization-suffix
      | index-suffix
      | member-suffix
      | call-suffix
      | numeric-cast-suffix } ;

index-suffix  = "[", expression, "]" ;
member-suffix = ".", identifier ;
call-suffix   = "(", [ argument, { ",", argument } ], ")" ;
argument      = [ identifier, "=" ], expression ;

numeric-cast-suffix = "as", numeric-type ;

numeric-type
    = "int" | "int8" | "int16" | "int32" | "int64" | "int128" | "intsize"
    | "uint8" | "uint16" | "uint32" | "uint64" | "uint128" | "uintsize"
    | "float32" | "float64" ;
```

Arithmetic and Boolean chains are left-folded. The optional comparison suffix
permits exactly one unparenthesized equality or ordering operator. Ordering,
equality, and mixed chains are rejected rather than left-folded or given
Python-style chained-comparison semantics; write the repeated operations with
`and`. `not a == b` means `not (a == b)`. Casts bind more tightly than
arithmetic.

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
    | explicit-set-literal ;

list-literal
    = "[", [ expression, { ",", expression } ], "]" ;

brace-literal
    = "{", "}"
    | "{", expression, { ",", expression }, "}"
    | "{", expression, ":", expression,
      { ",", expression, ":", expression }, "}" ;

explicit-set-literal
    = "Set", "{", [ expression, { ",", expression } ], "}" ;

parenthesized-expression
    = "(", expression, ")"
    | tuple-expression ;

tuple-expression
    = "(", expression, ",", ")"
    | "(", expression, ",", expression,
      { ",", expression }, ")" ;
```

`(value)` is grouping and `(value,)` is a singleton tuple. Tuple value
expressions require parentheses; an unparenthesized comma is accepted only in
an unpack target. `()` and a trailing comma on a multi-element tuple are
rejected. A nonempty brace literal is a set when its first element is not
followed by `:`, otherwise it is a map. `{}` parses as an empty map but may be
contextually typed as an empty `Set[T]`; `Set{}` is the unambiguous empty-set
form.

## Explicit Specialization

```ebnf
specialization-suffix = "[", type-list, "]" ;
```

Specialization and indexing use the same brackets, so parser context disambiguates them. Brackets form specialization only when their contents scan as one or more type references and either:

1. `(` follows and the base is a name or member, or
2. `.` follows and the final target name begins with uppercase ASCII.

Otherwise the brackets form an index expression. Consequently, `Box[int32](value)` and `Result[int32, String].Ok(1)` specialize, while a bare `value[index]` indexes. A specialized callable/type is not a general first-class expression when no qualifying call/member follows.

## Match Expressions

```ebnf
match-expression
    = "match", [ "borrow", [ "mut" ] ],
      expression, ":", NEWLINE,
      INDENT, match-expression-arm,
      { match-expression-arm }, DEDENT ;

match-expression-arm
    = "case", pattern, ":",
      ( expression, match-expression-arm-end
      | NEWLINE, INDENT, expression, statement-end, DEDENT ) ;

match-expression-arm-end
    = NEWLINE | DEDENT | ")" | "]" | "}" | EOF ;
```

A match-expression arm contains exactly one expression, either inline after the colon or on one indented following line. It is not a general statement suite.

A complete match expression may appear in a return, initializer, call
argument, collection element, grouping expression, or other expression
position. When it appears inside a continued delimiter, its header and arms
form a layout island and retain their required layout tokens. The containing
delimiter may close after the final inline arm or on its own following line.

## Syntactic Complexity Limits

The implementation rejects source that exceeds the maintained parser complexity budget rather than risking host stack exhaustion:

- nested expressions, prefix forms, parentheses, types, patterns, and statements are limited to 128 parser levels
- binary-operator and postfix chains reject the 128th chained operation
- f-string interpolation brace nesting is limited to 128

These are implementation limits of Aurora 0.1 and therefore observable parts of the current reference. A future implementation may raise them but must continue to reject excessive input cleanly.

## Syntax Not In Aurora 0.1

The grammar intentionally excludes:

- semicolons and multiple statements on one physical line
- backslash line continuation
- multiline ordinary strings and f-strings
- lambdas, local item declarations, comprehensions, decorators, and attributes
- wildcard/aliased/relative import syntax
- ordinary trailing commas other than the required singleton-tuple comma
- match guards, alternative patterns, and collection patterns
- call-site `borrow` annotations
- exception statements, `raise`, and `yield`
- detached `spawn`, `select`, and proposal-only concurrency syntax

If a form is absent from this grammar, examples and books must not present it as implemented Aurora.
