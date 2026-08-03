# Expressions

An expression evaluates to a value. This chapter defines the reader-facing expression contract: available forms, grouping, precedence, evaluation order, and the main static restrictions. The exact productions and specialization/indexing disambiguation are normative in [Grammar](/manual/grammar#expressions-and-precedence). Type rules are centralized in [Static Semantics](/manual/static-semantics#expression-typing), and runtime behavior is centralized in [Execution Model](/manual/execution-model#evaluation-order).

## Primary Expressions

Primary expressions are the atoms from which postfix, prefix, and binary expressions are built:

- a name such as `count`, `from`, or `point`
- an integer, float, duration, boolean, string, or f-string literal
- `None`
- a parenthesized expression or tuple
- a list, set, or dictionary literal
- a list, set, or dictionary comprehension

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
tuple. Tuple value expressions always require parentheses; Aura does not
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
owned slices, specialization/type arguments, collection literals, and
delimited portions of headers and declarations. The lexer joins those physical
lines before the expression grammar runs.

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
- a slice evaluates its base, written start, and written end once from left to
  right; omitted endpoints evaluate nothing
- a receiver is evaluated before call arguments
- explicit call and constructor arguments are evaluated in source order, with
  copy or move results captured before later argument side effects
- collection elements are evaluated in source order
- each dictionary key is evaluated before its value, and entries are evaluated in source order
- a comprehension evaluates its clauses and filters before its textually
  leading output expression; nested clauses are outer-major, filters are
  left-to-right, and a dictionary output key precedes its value
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
Name roots and projected member places follow the same rule, and Aura never
deep-clones the selected place implicitly. Each f-string interpolation renders
to `str` at its own position before evaluation moves to the next
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
| 6 | `|` | left |
| 7 | `^` | left |
| 8 | `&` | left |
| 9 | `<<`, `>>` | left |
| 10 | `+`, `-` | left |
| 11 | `*`, `/`, `//`, `%` | left |
| 12 | prefix `match`, `try`, unary `-`, unary `~` | prefix/right |
| 13 | `**` | right |
| 14 | specialization, indexing, member access, call, numeric cast | left-to-right postfix chain |
| 15 | primary expression | — |

Arithmetic, shift, bitwise, and boolean chains are left-folded. Power is the
right-associative exception. For example:

```text
a - b - c       means (a - b) - c
not a == b      means not (a == b)
a + b * c       means a + (b * c)
2 ** 3 ** 2     means 2 ** (3 ** 2)
-2 ** 2         means -(2 ** 2)
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

`and`, `or`, and `not` operate on `bool`; Aura has no general truthiness conversion for numbers, strings, collections, resources, or classes.

`and` and `or` short-circuit:

- `left and right` evaluates `right` only when `left` is `true`
- `left or right` evaluates `right` only when `left` is `false`

`not value` evaluates its operand and negates the boolean result. A matching operator trait may provide `not` for a supported user type, as described under [Generics And Traits](/manual/generics-and-traits#operator-traits).

## Conditional Expressions

The Python-style form `value if condition else alternative` selects one value.
For example, `label = "ready" if ready else "waiting"` chooses one `str`.

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

Built-in arithmetic supports equal integer types or equal floating-point types. `str + str` concatenates strings. Aura does not implicitly widen non-literal numeric values.

| Operators | Builtin result |
| --- | --- |
| `+` | Same numeric type, `str` for string concatenation, or `Duration` for two Duration operands |
| `-` | Same numeric type, or `Duration` for two Duration operands |
| `*` | Same numeric type; `Duration` for `Duration * int64` or `int64 * Duration` |
| `**` | Same exact integer or floating type |
| `//` | Same numeric type, or `Duration` for `Duration // int64` |
| `%` | Same numeric type |
| `/` | Same floating-point type |
| unary `-` | Same numeric type |
| `&`, `|`, `^`, unary `~` | Same exact integer type |
| `<<`, `>>` | Same exact integer type for value and count |
| `==`, `!=` | `bool` for equal operand types |
| `<`, `<=`, `>`, `>=` | `bool` for equal numeric types or two Duration values |
| `in`, `not in` | `bool` for a supported container |

`Array[T]` adds exact-shape elementwise `+`, `-`, and `*` for the four
maintained numeric dtypes. A same-dtype scalar may appear on either side.
Floating Arrays also support `/`; integer Array `/` remains the same
`AU2003` static error as scalar integer `/`. Every result is a fresh Array.
There is no array-shape broadcasting or mixed promotion. See
[Numeric Arrays](/manual/numeric-arrays).

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
`Option.None` with no context for its type argument is rejected. Aura rejects
Python identity tests such as `value is None`; use `value == None`,
`value != None`, or `match`.

Arithmetic and ordering may resolve through the corresponding operator trait.
For non-numeric user types, `/` requests `Div.div`; `//` requests
`FloorDiv.floor_div` when neither a builtin numeric rule nor the builtin
`Duration // int64` rule applies. Builtin equality does not use an equality
operator trait in Aura 0.3.

Tuple `<`, `<=`, `>`, and `>=` are static errors. Aura has no lexicographic
tuple ordering, and an `Ord` implementation cannot add one to a structural
tuple type.

Builtin integer `/` is a static error, as is integer `/=`. The diagnostic directs callers to `//` for a floor quotient or to `.to_float()` on both operands for floating true division. Integer `//` rounds the mathematical quotient toward negative infinity, and integer `%` is its paired remainder. Floating `//` and `%` use the corresponding CPython-compatible divmod correction. In both numeric domains, a nonzero remainder has the divisor's sign. Integer and floating `//` or `%` by zero, and floating `/` by zero, are runtime failures. See [Execution Model](/manual/execution-model#operators) for the complete runtime contract.

Integer power is checked and preserves the exact operand type. Its exponent
must be non-negative. `x ** 0` is `1`, including `0 ** 0`. A negative exponent
visible in source is rejected with `AU2003`; a negative value discovered at
runtime fails with `AU4001`. Overflow fails with `AU4002`. Floating power also
requires equal operand types. It returns that type, reports a domain error for
zero to a negative exponent or a negative finite base with a non-integral
finite exponent, and reports a finite-input overflow with `AU4002`.

Bitwise operators use each integer's fixed declared width. `&`, `|`, and `^`
combine corresponding bits; `~` flips every bit. Binary operands must have the
same exact concrete integer type. A shift's count has the same exact type as
the shifted value and must satisfy `0 <= count < width`. Signed right shift is
arithmetic and unsigned right shift is logical. Ordinary `<<` is checked and
fails with `AU4002` when the mathematical result does not fit.

`divmod(left, right)` evaluates both arguments once and returns the same floor
quotient and remainder as `(left // right, left % right)` in one tuple. Both
arguments have one exact integer or floating type, which is also the type of
both tuple elements. A zero divisor fails with `AU4004`.

`round(value)` returns an integer unchanged with its exact type. A `float32`
or `float64` value rounds to `int64` using nearest-integer ties-to-even. Signed
zero becomes integer zero. NaN, infinity, and a rounded result outside the
`int64` range fail with `AU4002`. Aura has no digit-count overload.

An unsuffixed integer literal may take the type of a `float32` or `float64` operand when the integer value is exactly representable in that floating type. Thus `7.5 // 2` is floating floor division and `-7.5 % 2` is floating remainder. This rule never converts a bound integer variable. An inexact literal is rejected; use an explicit floating spelling when rounding at the literal is intentional, or `.to_float()` for an intentional integer-to-`float64` conversion.

Every integer type provides `.to_float() -> float64`. This conversion uses IEEE-754 round-to-nearest, ties-to-even and may lose integer precision:

```python
left: int64 = 9007199254740993
right: int64 = 2
ratio = left.to_float() / right.to_float()
rounded = left.to_float() # 9007199254740992.0
```

Use this method when rounding into the floating domain is intentional. An explicit integer `as float32` or `as float64` cast has the stricter exactness contract below.

Every scalar integer type also provides exact-width `wrapping_add`,
`wrapping_sub`, `wrapping_mul`, `saturating_add`, `saturating_sub`, and
`saturating_mul`. The scalar methods `wrapping_shl`, `wrapping_shr`,
`saturating_shl`, and `saturating_shr` take a count of the receiver's exact
type and apply the same `0 <= count < width` rule as the shift operators.
Wrapping left shift discards high bits; saturating left shift clamps to the
integer type's bounds. Both named right-shift modes produce the same value as
ordinary `>>` after validating the count. `Array[int32]` and `Array[int64]`
provide the add/subtract/multiply named operations with a same-dtype scalar or
exact-shape Array right operand. Ordinary arithmetic remains checked.

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
Every type that provides `len()` is accepted — `str`, `list[T]`, `dict[K, V]`,
`set[T]`, and `Array[T]` — and a value without that member is rejected with
`AU2002`. Their `len()` members also produce `int64`, so `len(value)` and `value.len()` have the
same static type and value. `str.byte_len()` likewise produces `int64`, but
counts UTF-8 bytes rather than the Unicode scalar values counted by
`str.len()`. Neither `len` spelling changes ownership, because `len()`
borrows its receiver.

`str(value)` produces the same `str` that `print(value)` writes and that
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
| `list[T]` | element membership | `contains` | `T` |
| `set[T]` | element membership | `contains` | `T` |
| `dict[K, V]` | key membership | dictionary key lookup | `K` |
| `str` | substring containment | `contains` | `str` |

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

A primary expression may be followed by specialization, indexing, slicing,
member access, calls, and numeric casts. Suffixes are applied from left to
right; parenthesize a larger prefix or binary expression before applying a
suffix to its result:

```python
users[0].name.clone()
Result[int32, str].Ok(7)
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

Call sites pass a value directly to bare, `own`, and `mut` parameters.
Capability-prefixed argument forms are not expressions. The callee signature
selects whether the argument receives shared access, ownership, or mutable
access. A bare parameter is logically shared for every type; an explicit
`own` parameter transfers ownership. See
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
result = Result[int32, str].Ok(7)
```

Specialization and indexing share `[...]`. The parser treats brackets as specialization only when their contents form one or more type references and either:

1. `(` follows and the base is a name or member, or
2. `.` follows and the final target name begins with uppercase ASCII.

Otherwise the brackets are indexing. Thus `Box[int32](...)` specializes, `Result[int32, str].Ok(...)` specializes, and `values[index]` indexes. A bare `Box[int32]` is not a general first-class specialized-type value.

Type arguments do not accept a trailing comma. Generic inference, arity, and trait-bound rules are defined in [Static Semantics](/manual/static-semantics#contextual-inference).

## Member Access

`object.member` selects a visible field, method, enum variant, module item, or maintained builtin member:

```python
point.x
point.distance()
Status.Ready
io.Error.NotFound
```

An instance method call evaluates the receiver before its arguments. The
method declaration determines whether the receiver is shared (`self`),
consumed (`own self`), or mutable (`mut self`). A method without a receiver is
associated and is called through its type.

Visibility and resolution are static. Missing or private members are compile-time errors.

## Indexing

`base[index]` evaluates the base, then the index. Direct indexing supports
vectors, maps, and numeric Arrays under the maintained static rules:

```python
values[0]
counts["ready"]
matrix[1, 2]
```

List indices use the `int64` index domain. Non-negative indexes are zero-based;
a negative index `i` is normalized once as `len + i`, so `values[-1]` selects
the last element. The same rule applies to indexed assignment and the public
List index methods. An index that remains outside the operation's valid range
after normalization is not clamped. A contextually typed integer literal
adopts `int64`; fixed-width `int8`, `int16`, `int32`, `uint8`, `uint16`, and
`uint32` values widen losslessly only at an index-domain position.
A dictionary index must have exactly the dictionary's key type. Direct reads are permitted
only when the dictionary value type is copyable. For a non-copy value, use `get(key)`
for an explicit cloned optional read only when the value type is clone-safe;
use `remove(key)` to transfer any stored value, including one that contains
`random.Rng`. A missing key in a direct read is a runtime `AU4003` lookup violation.

An `Array[T]` index has one `int64` coordinate per runtime axis.
Coordinates evaluate left to right and negative values normalize once against
their own axis. A direct out-of-range coordinate is `AU4003`; a direct
coordinate-count/rank mismatch is `AU4007`. `get(list[int64])` returns `None`
for an invalid coordinate or rank. Mutable `set(list[int64], value)` returns
`Some(old_value)` on success and traps on an invalid coordinate or rank.

A direct list read of a copy element returns the value. Moving a non-copy
List element by direct indexing is restricted; use `get(index)` when the
intended operation is an explicit cloned/optional read and the element type is
clone-safe. Use `pop(index)` to transfer a non-cloneable stored value. Index
assignment is a statement target and is covered by
[Statements](/manual/statements#bindings-and-assignment).

Integer indexing on `str` is unavailable. Use a slice when selecting a
substring, or the maintained string methods for whole-string operations.
Exact UTF-8 conversion is available through `text.to_bytes()` and
`str.from_bytes(bytes=...)`.

## Slicing

`base[start:end]` selects the half-open range from start inclusive to end
exclusive. Slicing is defined for `list[T]`, `str`, and `Array[T]`, and
always returns a fresh owned value of the same type:

    middle = values[1:3]
    prefix = values[:2]
    suffix = values[-2:]
    all_values = values[:]
    scalars = "A🎉Z"[1:2]
    first_rows = matrix[0:2]

An omitted start means zero and an omitted end means the source length. Equal
endpoints produce an empty result. Every written endpoint uses the `int64`
position domain. Fixed-width `int8`, `int16`, `int32`, `uint8`, `uint16`, and
`uint32` values widen losslessly at that position.

A negative endpoint `i` is normalized exactly once as `len + i`. After
normalization, start and end must each be in `0..=len`, and start must not
exceed end. Otherwise evaluation traps with `AU4003`.

Aura deliberately differs from Python here: slice endpoints are **not
clamped**. An endpoint that remains out of range after one normalization is a
broken invariant, not a request for the nearest boundary. A reversed range is
also an `AU4003` failure rather than an empty slice.

A list slice copies Copy elements and clones non-Copy elements into a fresh
owned list. The element type must therefore be clone-safe. A type containing
`random.Rng`, an opaque FFI handle, or a capturing closure environment is
rejected with `AU3007`, and a type containing a non-repeatable Task result
right is rejected with `AU3009`. Generic slicing infers the same obligation
for its element type. The source remains usable.

String endpoints count Unicode scalar values, matching `str.len()`, not
UTF-8 bytes or grapheme clusters. Locating scalar boundaries scans the source,
so str slicing is O(n); the result is a newly allocated valid UTF-8 str.
Integer `string[index]` remains unavailable.

The base, written start, and written end are evaluated once from left to right.
The selected non-Copy base remains retained through endpoint evaluation, so an
endpoint may read it but cannot mutate or consume the overlapping source.
No list, str, or Array slice is a place or a view.

An Array slice applies the range only to axis zero, copies complete rows, and
retains all later dimensions. Its first result dimension is `end - start`.
It follows the same `int64`, one-time-negative-normalization,
no-clamping, `AU4003`, owned-copy, no-step, and no-assignment rules. It is not
a multidimensional slice or view.

A second colon is reserved for future step syntax. `value[start:end:step]` and
`value[::]` report `AU2005` with `slice steps are unavailable; use an explicit
loop to select a stride`. Slice assignment and compound assignment report
`AU2005` with `slice assignment is unavailable because slices are owned
copies; mutate the source by index or build a new value`.

## Collection Literals

Aura has list, set, and dictionary literals:

```python
values = [1, 2, 3]
seen = {1, 2, 3}
counts = {"ready": 2, "done": 1}
```

The first colon in a nonempty brace literal determines dictionary syntax. Without a colon, the literal is a set. Collection literal elements, keys, and values must have consistent types after contextual inference.

Empty literals require expected types because they contain no values from which to infer element types:

```python
values: list[int32] = []
counts: dict[str, int32] = {}
seen = set[int32]()
```

`{}` is a dictionary literal. An empty set uses `set[T]()`.

Collection literals may span physical lines while their `[` or `{` remains
open, but they do not accept trailing commas. Lists and sets evaluate elements
in source order. Dictionaries evaluate each key before its value and entries in source
order. If two evaluated dictionary keys are equal, the later value replaces the
earlier value while the key retains its first insertion position.

## Comprehensions

A comprehension is an eager collection expression:

    doubled = [value * 2 for value in values]
    visible = {value for value in values if value >= 0}
    by_id = {item.id: item for item in items}

One or more `for` clauses are required. A clause may have multiple `if`
filters and may be followed by another clause:

    coordinates = [
        (row, column)
        for row in rows if row >= 0
        for column in columns if column >= 0
    ]

The syntax places the output expression first, but runtime order starts at the
first iterable. Its target is bound, its filters run left to right, and then
the next iterable is selected. At the innermost surviving combination the
output runs. Nested traversal is outer-major: every surviving inner item for
one outer target is produced before the next outer item. Dictionary output evaluates
and captures the key before evaluating the value.

Each clause uses ordinary bare-loop iteration. List and set inputs are shared
and frozen, Range yields copy values, `enumerate(...)` and `zip(...)` retain
their loop contracts, and Queue retains its special receive semantics in which
the handle is copied and each target arrives owned. A comprehension does not
accept `mut` or `own` before its source.

The result is a newly owned `list[T]`, `set[T]`, or `dict[K, V]`, never a view or
lazy iterator. Result insertion owns non-Copy values. A shared non-Copy source
element must be explicitly cloned when clone-safe; Queue-received owned values
may move directly. Targets are progressively scoped over their filters, later
clauses, and the output, then disappear when the expression ends.

Lambdas reached inside a comprehension use the ordinary ADR-0037 capture
contract. For example, a compiler-known callback can capture a Copy value
while it is called from an element expression:

    shifted_rows = [
        row.map(lambda value: value + offset)
        for row in rows
    ]

The lambda is created only for a reached element. Shared non-Copy capability
capture remains rejected, and a capturing closure cannot itself become a
stored comprehension element. See [Closures](/manual/closures).

Generator expressions remain unavailable. `(value for value in values)` and
`consume(value for value in values)` report `AU2005` and direct the author to
an eager owned list comprehension or an explicit loop.

## F-Strings

An f-string produces an owned `str` and evaluates interpolations from left
to right. Each interpolation is rendered to `str` immediately, before the
next interpolation begins:

```python
name = "aura"
count = 3
message = f"{name}: {count}"
report = f"{name:<12s} {count:>8,d}"
```

Interpolation contents are ordinary expressions. A top-level colon introduces
a statically checked format specification with fill, alignment, sign, width,
decimal grouping, precision, and a closed set of string and numeric type codes.
For numeric values, a width beginning with `0` pads after the sign, matching
Python's `09.3f` shorthand.
Formatting uses the interpolation value's exact static numeric width, so a
`float32` is formatted from its binary32 value. String spelling, escapes,
literal braces, and the complete format grammar are defined by
[Lexical Structure](/manual/lexical-structure#f-strings).

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

Use `match value` to inspect without consuming a non-copy scrutinee, or `match mut value` when an arm must mutate through payload bindings.

## `try`

`try expression` operates on `Result[T, E]`:

```python
def parse_value(text: str) -> Result[int32, str]:
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
result: Result[int32, str] = Result.Ok(7)
missing: Option[str] = Option.None
ready = Status.Ready(count=3)
```

The variant must exist and receive exactly its declared payload shape. Generic enum arguments may be inferred from an expected type or payloads; explicit specialization is required when inference cannot resolve every type parameter.

Bare builtin variants such as `Ok`, `Err`, `Some`, or `None` are accepted only where the expected enum identity is unambiguous. Qualified construction is the preferred reference and book style.

## Function Values And Indirect Calls

A named module-level function may appear as an expression. Its value is a copy
code pointer with type such as `def(T1, mut T2, own T3) -> R`, where bare
parameters are shared. Calling that expression uses the ordinary call
production and preserves the named function's parameter capabilities.
Explicit generic specialization such as `show[int32]` fixes one concrete
function value before storage or invocation.

Function-valued variables, parameters, fields, and collection elements are
ordinary primary/postfix expressions. A value with one statically known source
declaration keeps that declaration's parameter names and defaults for indirect
calls. A control-flow selection also keeps these extras when all candidates
agree on their names and default availability; each omitted argument evaluates
the selected target's own default expression. Conflicting reassignment,
structural function returns, class-field loads, and mutable-collection loads
have only the structural function type and therefore require the complete
positional argument list. Storage preserves each parameter's bare shared,
`mut`, or `own` ABI capability. Contextually typed
`lambda parameters: expression` values use the same callable contract and may
capture owned outer locals by value. See [Closures](/manual/closures).
Instance and associated method values and trait-object interactions remain
unavailable.

## Fixed-Width Numeric Example

This program packs three bytes into a `uint32`, extracts them again, and uses
the numeric helpers that return more than one value:

```aura
def pack_rgb(red: uint32, green: uint32, blue: uint32) -> uint32:
    sixteen: uint32 = 16
    eight: uint32 = 8
    return (red << sixteen) | (green << eight) | blue

def main() -> int32:
    red: uint32 = 0xFF
    green: uint32 = 0x80
    blue: uint32 = 0b0000_0000
    packed = pack_rgb(red, green, blue)
    mask: uint32 = 0xFF
    eight: uint32 = 8
    sixteen: uint32 = 16

    print(packed)
    print((packed >> sixteen) & mask)
    print((packed >> eight) & mask)
    print(packed & mask)
    print(3 ** 4)
    print(round(2.5))
    quotient, remainder = divmod(-17, 5)
    print(quotient)
    print(remainder)
    return 0
```

The program prints `16744448`, `255`, `128`, `0`, `81`, `2`, `-4`, and `3`,
one value per line.

## Forms Not Implemented

Aura 0.3 expressions do not include generator expressions, method values,
assignment expressions, call-site capability annotations, non-numeric casts, or
ordinary trailing commas. Lambdas are expression-bodied and contextually
typed; they do not add statement-bodied or implicitly reference-capturing
forms. The required singleton-tuple comma is the one tuple-specific exception.
If a form is absent from [Grammar](/manual/grammar), it is not part of the
implemented expression language.

## Grammar

Primary, postfix, power, unary, multiplicative, additive, shift, bitwise,
comparison, Boolean,
conditional, `match`, `try`, lambda, collection literal/comprehension,
constructor, and f-string expression
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

List and set comprehension output expressions determine `T`; dictionary key and value
expressions determine `K` and `V`. An expected result specialization provides
context before inference. Filters require exact `bool`, and every source uses
the static iterable rules of a bare statement loop.

## Runtime Semantics

Operands and call arguments evaluate left to right, with each copy or move
argument result captured before the next argument's side effects. Named enum
arguments evaluate in source order and then bind to declaration-order payload
slots. `and` and `or` short circuit. Conditional expressions evaluate the
condition first and exactly one selected arm. A membership test evaluates its
value before its container. A comparison chain evaluates its operands left to
right, evaluates each at most once, and stops at its first `false` link. A
binary power, shift, or bitwise expression evaluates its left operand once
before evaluating its right operand once. A compound form selects its target
place once and writes only after the operation succeeds. A
member receiver is evaluated before arguments; an index base is evaluated
before its index; a slice base is evaluated before its written start and end;
collection entries preserve source order; a match scrutinee
evaluates once; and each f-string interpolation renders immediately before the
next begins.
`try` either yields an `Ok` payload or returns the `Err` from the enclosing
function after required cleanup.

A comprehension allocates one result, evaluates every reached source once for
its current outer combination, applies filters left to right, and then
evaluates its output. Nested clauses are outer-major. Dictionary key evaluation
precedes value evaluation. A trap or `try` propagation drops the partial
result.

## Ownership And Evaluation Order

Evaluation copies copy values and moves non-copy values only when the static
context consumes them. Bare parameters grant logical shared access; `own`
parameters and consuming receivers move, while `mut` parameters grant
exclusive mutable access.
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

Comprehension targets use progressive child scopes and do not leak. Active
shared sources stay borrowed and frozen through downstream filters, clauses,
and output evaluation. Insertion into the result is owned, so copy, move,
explicit-clone, loop-carried-move, and ADR-0037 capture checks apply exactly as
they do in the equivalent nested bare loops.

## Diagnostics

`AU1101` means invalid expression syntax, including malformed comprehension
clauses and forbidden comprehension `mut`/`own` modifiers. `AU2001` means an unresolved name or
member. `AU2002` means a type, constructor-payload, match-result, or index-type
mismatch. `AU2003` means an unsupported unary, binary, compound, membership, or
cast operator. `AU2004` means call or constructor argument binding failed.
`AU2005` means an unsupported syntax or expression feature,
including the exact generator-expression guidance recorded above. `AU2999`
means an expression rejection without a narrower compile-time code. `AU3001`
means use of a moved value; `AU3002` means a borrow conflict, including a later
mutable borrow or consumption overlapping a retained non-copy binary operand,
index base, method receiver, or indexed-assignment target; `AU3003` means an
immutable place was used mutably; and `AU3004` means an invalid ownership mode.
`AU3005` means a direct indexed read would copy a non-copy stored value, and
`AU3006` means indexed compound assignment would do the same during its
read-modify-write step.
`AU3007` and `AU3009` reject a list slice whose owned result would duplicate,
respectively, non-cloneable state or a single-consumer Task observation right.
`AU4003` reports an invalid normalized slice endpoint or reversed range.
Reserved slice steps and slice assignment use `AU2005`.
At runtime, `AU4001` means a general expression trap, `AU4002` means arithmetic
overflow, underflow, range, or conversion-exactness failure, `AU4003` means a
bounds or lookup violation, `AU4004` means a zero divisor, and `AU4005` means a
trapping resource or I/O failure propagated by a call expression.
For numeric operations, `AU4001` includes a runtime negative integer exponent
and floating power domain errors. `AU4002` includes integer power overflow,
invalid shift counts, and checked-left-shift overflow.

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
Comprehensions are eager, have no `mut`/`own` source form, and do not provide
early exit, lazy resumption, or a user-defined iterable protocol; use an
explicit loop when those properties are required.
Floating values follow the specified Aura operations and shortest-round-trip
printing; no backend may substitute a different expression result as an
implementation-defined choice.

## Status

The expression forms defined positively in this chapter are implemented.
Delimiter continuation is accepted under ADR-0025 and does not add a new
expression AST form. Conditional expressions are accepted under ADR-0027, and
membership operators plus comparison chains are accepted under ADR-0028. The
minimal tuple surface and its Batch 3 B3.0-c equality amendment are Accepted
under ADR-0026. Capture-free named function values, indirect calls, and
contextually typed by-value expression closures are implemented. Method
values, generator expressions, assignment expressions, nonnumeric casts, and
call-site capability modifiers are unavailable. Eager owned list, set, and dictionary
comprehensions are implemented under Accepted ADR-0039.
Integer base spellings, fixed-width bitwise operations, and shifts are
Accepted under ADR-0047. Power, `round`, and `divmod` are Accepted under
ADR-0048.
