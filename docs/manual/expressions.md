# Expressions

An expression evaluates to a value. This chapter defines the reader-facing expression contract: available forms, grouping, precedence, evaluation order, and the main static restrictions. The exact productions and specialization/indexing disambiguation are normative in [Grammar](/manual/grammar#expressions-and-precedence). Type rules are centralized in [Static Semantics](/manual/static-semantics#expression-typing), and runtime behavior is centralized in [Execution Model](/manual/execution-model#evaluation-order).

## Primary Expressions

Primary expressions are the atoms from which postfix, prefix, and binary expressions are built:

- a name such as `count`, `from`, or `point`
- an integer, float, duration, boolean, string, or f-string literal
- `None`
- a parenthesized expression or tuple
- a list, set, or map literal

```python
count
42
3.14
10ms
true
"text"
'text'
f"count={count}"
None
(left + right)
(left, right)
(left,)
[1, 2, 3]
{"ready": 2}
{1, 2, 3}
```

The lexical spelling and default literal types are defined by [Lexical Structure](/manual/lexical-structure). A name must resolve under [Names And Scopes](/manual/names-and-scopes).

Parentheses without a comma group exactly one expression. `(value)` is a
group, `(value,)` is a singleton tuple, and `(left, right)` is a two-element
tuple. Tuple value expressions always require parentheses; Aurora does not
accept a naked comma expression.

## Tuple Expressions

A tuple expression evaluates and captures its elements left to right:

    pair = ("north", 7)
    nested = (pair, (true,))

Its type is the fixed structural tuple of its element types. A tuple copies if
every element type copies; otherwise it moves as one complete value. See
[Tuples](/manual/tuples) for unpacking and matching.

A postfix tuple index is deliberately narrow:

    coordinates = (3, 4)
    vertical = coordinates[1]

The index must be a non-negative integer literal known at compile time, must be
in bounds, and must select a copy element. The result is a copy. Dynamic,
negative, out-of-bounds, and non-copy-element tuple indexing are static errors;
unpack a tuple when ownership of a non-copy element is required.

Tuple `==` and `!=` require the same static tuple type and compare
corresponding element values recursively. They read both operands without
consuming either one. Tuple ordering operators remain unavailable.

## Delimiter Continuation

An expression may span physical lines while a `(`, `[`, or `{` remains open.
This applies uniformly to grouping, function and constructor calls, indexes,
specialization/type arguments, collection literals, and delimited portions of
headers and declarations. The lexer joins those physical lines before the
expression grammar runs.

Continuation indentation is visual only. It does not create a suite or alter
evaluation order. The maintained style indents continued content by one level.
A trailing comma is still invalid, and a newline outside an open delimiter
still ends the logical line. Backslashes do not continue a line. Ordinary
strings and f-strings remain single-line.

## Evaluation Order

Except for short-circuit boolean operators and control-flow expressions, evaluation is left-to-right:

- a binary expression evaluates its left operand before its right operand
- a postfix expression evaluates its base before its suffix inputs
- an index evaluates its base before its index
- a receiver is evaluated before call arguments
- explicit call and constructor arguments are evaluated in source order, with
  copy or move results captured before later argument side effects
- collection elements are evaluated in source order
- each map key is evaluated before its value, and entries are evaluated in source order
- f-string interpolations are evaluated from left to right
- a conditional expression evaluates its condition first and then exactly one arm
- a match scrutinee is evaluated once, before arm selection
- a comparison chain evaluates operands left to right at most once and does
  not evaluate any operand after its first false link

Evaluation order matters when an expression moves a value, mutates through a
call, performs I/O, or can produce a runtime failure. A copy place contributes
the copied value captured at its evaluation point. A non-copy place selected as
a binary left operand, index base, method receiver, or indexed-assignment target
remains borrowed through the operation's later inputs. Another shared borrow is
permitted, but an overlapping mutable borrow or consumption is rejected with
`AU3002`, which identifies both the conflict and the retained-borrow origin.
Name roots and projected member places follow the same rule, and Aurora never
deep-clones the selected place implicitly. Each f-string interpolation renders
to `String` at its own position before evaluation moves to the next
interpolation. Static borrow analysis checks all accesses at one call boundary
together even though runtime evaluation remains ordered.

## Precedence And Associativity

The following table runs from lowest to highest precedence:

| Level | Form | Associativity |
| --- | --- | --- |
| 1 | `value if condition else alternative` | right |
| 2 | `or` | left |
| 3 | `and` | left |
| 4 | prefix `not` | right |
| 5 | `==`, `!=`, `<`, `<=`, `>`, `>=`, `in`, `not in` | chained left to right |
| 6 | `+`, `-` | left |
| 7 | `*`, `/`, `//`, `%` | left |
| 8 | prefix `match`, `try`, unary `-` | prefix/right |
| 9 | specialization, indexing, member access, call, numeric cast | left-to-right postfix chain |
| 10 | primary expression | — |

Arithmetic and boolean chains are left-folded. For example:

```text
a - b - c       means (a - b) - c
not a == b      means not (a == b)
a + b * c       means a + (b * c)
```

Equality, ordering, and membership share one precedence level and chain the
Python way. `a < b < c` is one chain, not a comparison of a comparison, and
`a == b == c` and `a < b == c` chain likewise. A chain of `n` operators is
equivalent to the conjunction of its `n` adjacent comparisons, except that each
operand expression is evaluated at most once. Parentheses still make a nested
Boolean comparison explicit, so `(a == b) == c` is a distinct form that
compares a `bool` against `c`.

Parentheses override precedence:

```python
scaled = (left + right) * factor
inside = lower < value < upper
```

## Boolean Operators

`and`, `or`, and `not` operate on `bool`; Aurora has no general truthiness conversion for numbers, strings, collections, resources, or classes.

`and` and `or` short-circuit:

- `left and right` evaluates `right` only when `left` is `true`
- `left or right` evaluates `right` only when `left` is `false`

`not value` evaluates its operand and negates the boolean result. A matching operator trait may provide `not` for a supported user type, as described under [Generics And Traits](/manual/generics-and-traits#operator-traits).

## Conditional Expressions

The Python-style form `value if condition else alternative` selects one value.
For example, `label = "ready" if ready else "waiting"` chooses one `String`.

The condition is evaluated first, exactly once, and must have type `bool`.
When it is `true`, only `value` is evaluated; when it is `false`, only
`alternative` is evaluated. Both arms must have one static result type.
Surrounding expected context flows into both arms, so contextual literals such
as integer literals, `None`, and empty collections can adopt that type. This
context is structural: an empty collection nested inside a tuple arm adopts
the corresponding concrete nested type from the other arm or the surrounding
expected type. Contextual typing never implicitly converts an already-bound
value.

The form has lower precedence than `or` and associates to the right.
`a or b if ready else c` means `(a or b) if ready else c`, while
`a if first else b if second else c` means
`a if first else (b if second else c)`.

Both arms are checked even when the condition is a literal. Ownership state is
checked independently for each arm and merged conservatively afterward. A
non-copy value moved by either arm is therefore unavailable after the
conditional expression. The surrounding use determines whether an arm is
moved: passing the result to an ordinary shared-borrow parameter borrows the
selected arm and preserves both source owners, while assignment, return, or an
`own` parameter consumes the selected value.

## Arithmetic And Comparison

Built-in arithmetic supports equal integer types or equal floating-point types. `String + String` concatenates strings. Aurora does not implicitly widen non-literal numeric values.

| Operators | Builtin result |
| --- | --- |
| `+` | Same numeric type, `String` for string concatenation, or `Duration` for two Duration operands |
| `-` | Same numeric type, or `Duration` for two Duration operands |
| `*` | Same numeric type; `Duration` for `Duration * int64` or `int64 * Duration` |
| `//` | Same numeric type, or `Duration` for `Duration // int64` |
| `%` | Same numeric type |
| `/` | Same floating-point type |
| unary `-` | Same numeric type |
| `==`, `!=` | `bool` for equal operand types |
| `<`, `<=`, `>`, `>=` | `bool` for equal numeric types or two Duration values |
| `in`, `not in` | `bool` for a supported container |

For tuple operands, `==` and `!=` require exactly the same static tuple type.
They compare corresponding element values from left to right using ordinary
equality, recursively for nested tuples. The comparison reads both complete
operands and does not move either one, including a tuple that contains
non-copy elements. Runtime tuple element-type, transport, or backend metadata
does not participate in the value result.

A tuple literal on either side may be contextually typed from the other
operand's known tuple type, recursively through nested literals. After that
symmetric contextual typing, the two static tuple types must still match
exactly. Evaluating either operand keeps its ordinary ownership effects; the
equality operation adds no move of the resulting tuple.

Equality and inequality have one contextual `Option` rule: when either operand
has static type `Option[T]`, a bare `None` on the other side denotes
`Option.None` of that same specialization. The rule is symmetric. Unit
`None == None` is `true` and unit `None != None` is `false`; a qualified
`Option.None` with no context for its type argument is rejected. Aurora rejects
Python identity tests such as `value is None`; use `value == None`,
`value != None`, or `match`.

Arithmetic and ordering may resolve through the corresponding operator trait.
For non-numeric user types, `/` requests `Div.div`; `//` requests
`FloorDiv.floor_div` when neither a builtin numeric rule nor the builtin
`Duration // int64` rule applies. Builtin equality does not use an equality
operator trait in Aurora 0.1.

Tuple `<`, `<=`, `>`, and `>=` are static errors. Aurora has no lexicographic
tuple ordering, and an `Ord` implementation cannot add one to a structural
tuple type.

Builtin integer `/` is a static error, as is integer `/=`. The diagnostic directs callers to `//` for a floor quotient or to `.to_float()` on both operands for floating true division. Integer `//` rounds the mathematical quotient toward negative infinity, and integer `%` is its paired remainder. Floating `//` and `%` use the corresponding CPython-compatible divmod correction. In both numeric domains, a nonzero remainder has the divisor's sign. Integer and floating `//` or `%` by zero, and floating `/` by zero, are runtime failures. See [Execution Model](/manual/execution-model#operators) for the complete runtime contract.

An unsuffixed integer literal may take the type of a `float32` or `float64` operand when the integer value is exactly representable in that floating type. Thus `7.5 // 2` is floating floor division and `-7.5 % 2` is floating remainder. This rule never converts a bound integer variable. An inexact literal is rejected; use an explicit floating spelling when rounding at the literal is intentional, or `.to_float()` for an intentional integer-to-`float64` conversion.

Every integer type provides `.to_float() -> float64`. This conversion uses IEEE-754 round-to-nearest, ties-to-even and may lose integer precision:

```python
left: int64 = 9007199254740993
right: int64 = 2
ratio = left.to_float() / right.to_float()
rounded = left.to_float() # 9007199254740992.0
```

Use this method when rounding into the floating domain is intentional. An explicit integer `as float32` or `as float64` cast has the stricter exactness contract below.

Duration arithmetic operates on the exact signed nanosecond representation.
Addition, subtraction, and multiplication are checked. `Duration // int64`
rounds the signed nanosecond quotient toward negative infinity; a zero divisor
fails with `AU4004`, and an unrepresentable result fails with `AU4002`.
Duration equality and ordering compare that signed count. The language has no
`Duration / int64`, `Duration % int64`, `Duration * float`, or unary
`-Duration` rule. Use `Duration.ms(-1)` when a negative value is needed, and
remember that negative values are not valid host waits.

## `len` And `str`

`len(value)` and `str(value)` are maintained builtin functions, not syntax.

`len(value)` delegates to the value's own `len()` member and produces `int64`.
Every type that provides `len()` is accepted — `String`, `Vec[T]`, `Map[K, V]`,
and `Set[T]` in Aurora 0.1 — and a value without that member is rejected with
`AU2002`. `String.len()`, `Vec[T].len()`, `Map[K, V].len()`, and
`Set[T].len()` also produce `int64`, so `len(value)` and `value.len()` have the
same static type and value. `String.byte_len()` likewise produces `int64`, but
counts UTF-8 bytes rather than the Unicode scalar values counted by
`String.len()`. Neither `len` spelling changes ownership, because `len()`
borrows its receiver.

`str(value)` produces the same `String` that `print(value)` writes and that
`f"{value}"` interpolates. It accepts any value the renderer accepts, so it is
total over the maintained surface rather than restricted to scalars.

```python
hosts = ["alpha", "beta"]
print(len(hosts))
print(str(len(hosts)))
```

Both names are builtin function names and, like `print` and `abs`, cannot be
redefined by a program.

## Membership And Comparison Chains

`value in container` and `value not in container` test membership and produce
`bool`. The container decides both the member the test delegates to and the
type the value must have:

| Container | Tests | Delegates to | Value type |
| --- | --- | --- | --- |
| `Vec[T]` | element membership | `contains` | `T` |
| `Set[T]` | element membership | `contains` | `T` |
| `Map[K, V]` | key membership | `contains_key` | `K` |
| `String` | substring containment | `contains` | `String` |

Any other container type is rejected with `AU2003`; a value whose type is not
the container's element, key, or substring type is rejected with `AU2002`. An
unsuffixed numeric literal on the value side may adopt the container's element
or key type. `not in` is exactly the negation of `in`, not a separate member.

Both operands are read. `in` never moves either operand, because the member it
delegates to takes a shared borrow of the container and a shared borrow of the
value. The value is evaluated before the container, matching source order.

```python
ports = [80, 443]
print(443 in ports)
print(8080 not in ports)
print("/health" in "GET /health HTTP/1.1")
```

A comparison chain such as `low <= value < high` evaluates its operands left to
right, evaluates each operand at most once, and stops at the first link that is
`false`. The operands after that link are not evaluated. Every link must be a
valid comparison of its two adjacent operands under the rules above, and the
chain's result is `bool`.

The same rule applies to tuple equality links. In
`first == middle != last`, `middle` is evaluated once and reused by both
adjacent links, while `last` is skipped when the first link is false. Tuple
equality does not consume any evaluated chain operand.

```python
def in_range(value: int32, low: int32, high: int32) -> bool:
    return low <= value < high
```

Each operand of a chain is checked as if it were evaluated, even where
short-circuiting would skip it at runtime. A chain therefore reports an
ownership conflict that only one runtime path would reach, which is the same
conservative rule the other branching forms use.

## Numeric Casts

`expression as NumericType` performs an explicit numeric conversion. Supported target spellings are:

```text
int int8 int16 int32 int64 int128 intsize
uint8 uint16 uint32 uint64 uint128 uintsize
float32 float64
```

The target spelling `int` is exactly the same target type as `int64`.

Casts are postfix and bind more tightly than arithmetic:

```python
whole = 7.9 as int32
widened = 3 as float64
total = left + right as int64
```

The last example means `left + (right as int64)`. Use parentheses when the cast should apply to a larger expression. Non-numeric casts are not implemented. Conversion must satisfy the checked range and precision rules in [Types](/manual/types#casts).

## Postfix Expressions

A primary expression may be followed by specialization, indexing, member access, calls, and numeric casts. Suffixes are applied from left to right; parenthesize a larger prefix or binary expression before applying a suffix to its result:

```python
users[0].name.clone()
Result[int32, String].Ok(7)
value as int64
```

Postfix chains are limited by the maintained syntax-complexity budget described in [Grammar](/manual/grammar#syntactic-complexity-limits).

## Calls And Argument Binding

A call has zero or more comma-separated arguments:

```python
print("hello")
range(1, 4)
process.run(["echo", "hi"], stdout=process.pipe(), group=true)
replace(from="old", to="new")
```

Positional arguments come before named arguments. Static binding proceeds as follows:

1. Positional arguments fill parameters in declaration order.
2. A named argument fills the parameter with the same name.
3. A parameter cannot be filled more than once.
4. Unknown names and extra arguments are rejected.
5. Every omitted parameter must have a default.
6. Each argument must have the substituted parameter type.

Arguments do not accept a trailing comma. A call may span physical lines while
its `(` remains open. Every supplied argument is evaluated first in call-site
source order before the next expression begins. A copy or move result is captured in
its parameter slot; a borrow-mode selection is established without cloning and
remains subject to the retained-borrow overlap rule. Later side effects cannot
change an earlier captured argument. Defaults for omitted parameters are then
evaluated afresh in declaration order. Binding a named value to its parameter
slot never reorders evaluation, and no default runs for a supplied parameter.
Mutable defaults are not shared process-global singletons.

Call sites pass a value directly to default-mode, `own`, `borrow`, and `borrow
mut` parameters. Prefix argument forms such as `own value` or `borrow value`
are not expressions. The callee signature selects whether the argument is
moved, shared-borrowed, or mutable-borrowed. A bare non-copy parameter is a
shared borrow; an explicit `own` parameter moves it. See
[Functions](/manual/functions#parameter-passing-modes) and [Ownership And
Borrowing](/manual/ownership-and-borrowing).

Calling a class name constructs the class. Calling an enum variant constructs
that variant. Every class field and enum payload is an owned position.
Constructor arguments follow the same positional-then-named rule and must
supply every required field or payload exactly once. Named enum-variant
arguments evaluate in their written source order; their captured results then
bind by payload name to declaration-order slots. Slot binding never reorders
the argument expressions.

## Explicit Generic Specialization

Explicit type arguments use brackets:

```python
box = Box[int32](value=42)
value = identity[int64](7)
result = Result[int32, String].Ok(7)
```

Specialization and indexing share `[...]`. The parser treats brackets as specialization only when their contents form one or more type references and either:

1. `(` follows and the base is a name or member, or
2. `.` follows and the final target name begins with uppercase ASCII.

Otherwise the brackets are indexing. Thus `Box[int32](...)` specializes, `Result[int32, String].Ok(...)` specializes, and `values[index]` indexes. A bare `Box[int32]` is not a general first-class specialized-type value.

Type arguments do not accept a trailing comma. Generic inference, arity, and trait-bound rules are defined in [Static Semantics](/manual/static-semantics#contextual-inference).

## Member Access

`object.member` selects a visible field, method, enum variant, module item, or maintained builtin member:

```python
point.x
point.distance()
Status.Ready
io.Error.NotFound
```

An instance method call evaluates the receiver before its arguments. The method declaration determines whether the receiver is shared-borrowed (`self` or `borrow self`), consumed (`own self`), or mutable-borrowed (`borrow mut self`). A method without a receiver is associated and is called through its type.

Visibility and resolution are static. Missing or private members are compile-time errors.

## Indexing

`base[index]` evaluates the base, then the index. Direct indexing supports vectors and maps under the maintained static rules:

```python
values[0]
counts["ready"]
```

Vector indices have exactly type `int32`. Non-negative indexes are zero-based;
a negative index `i` is normalized once as `len + i`, so `values[-1]` selects
the last element. The same rule applies to indexed assignment and the public
Vec index methods. An index that remains outside the operation's valid range
after normalization is not clamped. A contextually typed integer literal may
adopt `int32`, but an already-bound `int64` value is not implicitly narrowed.
A map index must have exactly the map's key type. Direct reads are permitted
only when the map value type is copyable. For a non-copy value, use `get(key)`
for an explicit cloned optional read only when the value type is clone-safe;
use `remove(key)` to transfer any stored value, including one that contains
`random.Rng`. A missing key in a direct read is a runtime `AU4003` lookup violation.

A direct vector read of a copy element returns the value. Moving a non-copy
vector element by direct indexing is restricted; use `get(index)` when the
intended operation is an explicit cloned/optional read and the element type is
clone-safe. Use `remove(index)` to transfer a non-cloneable stored value. Index assignment is a
statement target and is covered by
[Statements](/manual/statements#bindings-and-assignment).

Aurora 0.1 does not define integer indexing or slicing for `String`. Use the
maintained string methods for whole-string operations. Phase 3 provides exact
UTF-8 conversion through `text.to_bytes()` and
`String.from_bytes(bytes=...)`; scalar iteration and slicing remain future
work.

## Collection Literals

Aurora has list, set, and map literals:

```python
values = [1, 2, 3]
seen = {1, 2, 3}
counts = {"ready": 2, "done": 1}
```

The first colon in a nonempty brace literal determines map syntax. Without a colon, the literal is a set. Collection literal elements, keys, and values must have consistent types after contextual inference.

Empty literals require expected types because they contain no values from which to infer element types:

```python
values: Vec[int32] = []
counts: Map[String, int32] = {}
seen: Set[int32] = {}
explicit_seen: Set[int32] = Set{}
```

`{}` is grammatically an empty map but is accepted as an empty set when its expected type is `Set[T]`. `Set{}` is the unambiguous empty-set form.

Collection literals may span physical lines while their `[` or `{` remains
open, but they do not accept trailing commas. Lists and sets evaluate elements
in source order. Maps evaluate each key before its value and entries in source
order. If two evaluated map keys are equal, the later value replaces the
earlier value while the key retains its first insertion position.

## F-Strings

An f-string produces an owned `String` and evaluates interpolations from left
to right. Each interpolation is rendered to `String` immediately, before the
next interpolation begins:

```python
name = "aurora"
count = 3
message = f"{name}: {count}"
```

Interpolation contents are ordinary expressions. String spelling, escapes, literal braces, and unsupported formatting syntax are defined by [Lexical Structure](/manual/lexical-structure#f-strings).

## Match Expressions

`match` may produce a value. Its scrutinee is evaluated exactly once. Arms are considered in source order, and only the first matching arm expression is evaluated.

An arm contains exactly one expression. It may be inline:

```python
label = match code:
    case 0: "ok"
    case _: "other"
```

Or the expression may be placed on one indented following line:

```python
label = match code:
    case 0:
        "ok"
    case _:
        "other"
```

The indented form is still one expression, not a suite of statements. Every arm must produce one compatible result type, and the match must be exhaustive under [Enums And Pattern Matching](/manual/enums-and-match#exhaustiveness-and-wildcards).

A complete match expression may appear anywhere an expression is expected,
including an initializer, return value, call argument, collection element, or
grouping. Inside an enclosing delimiter, its required arm layout forms a
layout island rather than being suppressed by ordinary continuation. The exact
forms are defined in [Grammar](/manual/grammar#match-expressions).

Use `match borrow value` to inspect without consuming a non-copy scrutinee, or `match borrow mut value` when an arm must mutate through payload bindings.

## `try`

`try expression` operates on `Result[T, E]`:

```python
def parse_value(text: String) -> Result[int32, String]:
    value = try parse_int32(text)
    return Result.Ok(value)
```

The operand is evaluated once:

- `Result.Ok(value)` makes the `try` expression produce `value`
- `Result.Err(error)` returns immediately from the enclosing function

The enclosing function must return a compatible `Result`. When the error types differ, one applicable `From[SourceError] for TargetError` implementation may convert the error. Early return runs active `with` cleanups. See [Execution Model](/manual/execution-model#try).

## Enum Construction

Enum constructors use the enum or specialized enum name followed by the variant:

```python
result: Result[int32, String] = Result.Ok(7)
missing: Option[String] = Option.None
ready = Status.Ready(count=3)
```

The variant must exist and receive exactly its declared payload shape. Generic enum arguments may be inferred from an expected type or payloads; explicit specialization is required when inference cannot resolve every type parameter.

Bare builtin variants such as `Ok`, `Err`, `Some`, or `None` are accepted only where the expected enum identity is unambiguous. Qualified construction is the preferred reference and book style.

## Forms Not Implemented

Aurora 0.1 expressions do not include comprehensions, lambdas, assignment
expressions, call-site borrow annotations, non-numeric casts, or ordinary
trailing commas. The required singleton-tuple comma is the one tuple-specific
exception. If a form is absent from [Grammar](/manual/grammar), it is not part
of the implemented expression language.

## Grammar

Primary, postfix, unary, multiplicative, additive, comparison, Boolean,
conditional, `match`, `try`, collection, constructor, and f-string expression
productions are normative in [Grammar](/manual/grammar). The comparison
production covers equality, ordering, and membership at one level and admits a
chain of two or more operators. The precedence and
associativity table above resolves every accepted operator sequence. A
spelling absent from those productions is not accepted as an implicit
extension.

## Typing Rules

Each expression receives exactly one static type. Calls, constructors,
operators, indexing, member access, collections, matches, casts, and `try` must
satisfy the specific rules above after generic substitution. Context may type a
literal, including an exactly representable integer literal in a floating
context, but never converts a bound variable. Branching expressions require a
single result type on every arm.

## Runtime Semantics

Operands and call arguments evaluate left to right, with each copy or move
argument result captured before the next argument's side effects. Named enum
arguments evaluate in source order and then bind to declaration-order payload
slots. `and` and `or` short circuit. Conditional expressions evaluate the
condition first and exactly one selected arm. A membership test evaluates its
value before its container. A comparison chain evaluates its operands left to
right, evaluates each at most once, and stops at its first `false` link. A
member receiver is evaluated before arguments; an index base is evaluated
before its index; collection entries preserve source order; a match scrutinee
evaluates once; and each f-string interpolation renders immediately before the
next begins.
`try` either yields an `Ok` payload or returns the `Err` from the enclosing
function after required cleanup.

## Ownership And Evaluation Order

Evaluation copies copy values and moves non-copy values only when the static
context consumes them. Default-mode non-copy parameters borrow; `own`
parameters and consuming receivers move; explicit borrows retain the owner.
Non-copy indexed reads report `AU3005` and require the safe method surface
instead of an implicit copy. `in` and `not in` read both operands and move
neither. Equality and inequality themselves also read both resulting operands
and move neither; this includes structural tuple equality. Evaluation inside
an operand retains its ordinary ownership effects. A comparison chain checks
every operand as if it were evaluated, even where short-circuiting would skip
it. Binary left operands, index bases, method receivers, and
indexed-assignment targets retain their non-copy borrow through later inputs.
An overlapping mutable borrow or consumption is rejected with `AU3002`, and
no hidden clone repairs the invalid expression.

## Diagnostics

`AU1101` means invalid expression syntax. `AU2001` means an unresolved name or
member. `AU2002` means a type, constructor-payload, match-result, or index-type
mismatch. `AU2003` means an unsupported unary, binary, compound, membership, or
cast operator. `AU2004` means call or constructor argument binding failed. `AU2005`
means focused migration guidance for a Python-shaped expression. `AU2999`
means an expression rejection without a narrower compile-time code. `AU3001`
means use of a moved value; `AU3002` means a borrow conflict, including a later
mutable borrow or consumption overlapping a retained non-copy binary operand,
index base, method receiver, or indexed-assignment target; `AU3003` means an
immutable place was used mutably; and `AU3004` means an invalid ownership mode.
`AU3005` means a direct indexed read would copy a non-copy stored value, and
`AU3006` means indexed compound assignment would do the same during its
read-modify-write step.
At runtime, `AU4001` means a general expression trap, `AU4002` means arithmetic
overflow, underflow, range, or conversion-exactness failure, `AU4003` means a
bounds or lookup violation, `AU4004` means a zero divisor, and `AU4005` means a
trapping resource or I/O failure propagated by a call expression.

## Backend Support

All expression forms marked implemented lower to MIR and are supported by the
direct native backend. The forced backend-parity matrix verifies their
observable results and primary traps. Compiler analysis and LSP diagnostics
are produced before backend selection.

## Limits And Implementation-Defined Behavior

The parser caps expression nesting and operator chains at 128. Physical lines
continue only while a source delimiter remains open; backslashes and
multiline string/f-string literals do not continue them. Ordinary trailing
commas are unavailable; `(value,)` is the required singleton tuple spelling.
Collection and string resource caps are documented by their feature pages.
Floating values follow the specified Aurora operations and shortest-round-trip
printing; no backend may substitute a different expression result as an
implementation-defined choice.

## Status

The expression forms defined positively in this chapter are implemented.
Delimiter continuation is accepted under ADR-0025 and does not add a new
expression AST form. Conditional expressions are accepted under ADR-0027, and
membership operators plus comparison chains are accepted under ADR-0028. The
minimal tuple surface and its Batch 3 B3.0-c equality amendment are Accepted
under ADR-0026. Lambdas,
comprehensions, assignment expressions, general callables, nonnumeric casts,
and call-site ownership modifiers are unavailable. Parser migration hints for
unavailable spellings do not make them language features.
