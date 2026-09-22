# Expressions

This page defines Aura's expressions: the available forms, how they group,
their precedence, their evaluation order, and their main static restrictions.
An expression evaluates to a value.

Three pages hold the normative detail:

- [Grammar](/manual/grammar#expressions-and-precedence) has the exact
  productions and the rule that tells specialization apart from indexing.
- [Static Semantics](/manual/static-semantics#expression-typing) has the type
  rules.
- [Execution Model](/manual/execution-model#evaluation-order) has the runtime
  behavior.

## Find A Form

| Form | Section |
| --- | --- |
| names, literals, `None`, `( )` groups | [Primary Expressions](#primary-expressions) |
| `(a, b)`, `pair[0]` | [Tuple Expressions](#tuple-expressions) |
| `and`, `or`, `not` | [Boolean Operators](#boolean-operators) |
| `a if cond else b` | [Conditional Expressions](#conditional-expressions) |
| `+`, `-`, `*`, `/`, `//`, `%`, `**` | [Arithmetic And Comparison](#arithmetic-and-comparison) |
| `&`, `\|`, `^`, `~`, `<<`, `>>` | [Bitwise And Shift Operators](#bitwise-and-shift-operators) |
| `==`, `!=`, `<`, `<=`, `>`, `>=` | [Equality](#equality) and [Operator Result Types](#operator-result-types) |
| `is None`, `is not None` | [None Tests](#none-tests) |
| `in`, `not in`, `a < b < c` | [Membership And Comparison Chains](#membership-and-comparison-chains) |
| `value as int64` | [Numeric Casts](#numeric-casts) |
| `f(x)`, `f(name=x)` | [Calls And Argument Binding](#calls-and-argument-binding) |
| `Box[int32](...)` | [Explicit Generic Specialization](#explicit-generic-specialization) |
| `object.member` | [Member Access](#member-access) |
| `values[i]`, `counts[key]` | [Indexing](#indexing) |
| `values[start:end]` | [Slicing](#slicing) |
| `[...]`, `{...}` | [Collection Literals](#collection-literals) and [Comprehensions](#comprehensions) |
| `f"..."` | [F-Strings](#f-strings) |
| `len`, `str` | [`len` And `str`](#len-and-str) |
| `divmod`, `round`, `.to_float()` | [Division And Remainder](#division-and-remainder) and [Rounding And Conversion](#rounding-and-conversion) |
| `match` | [Match Expressions](#match-expressions) |
| `try` | [`try`](#try) |
| `Result.Ok(7)` | [Enum Construction](#enum-construction) |
| function values, `lambda` | [Function Values And Indirect Calls](#function-values-and-indirect-calls) |

## Primary Expressions

Primary expressions are the atoms that postfix, prefix, and binary
expressions build on:

- a name, such as `count`, `from`, or `point`
- an integer, float, duration, boolean, string, or f-string literal
- `None`
- a parenthesized expression or tuple
- a list, set, or dictionary literal
- a list, set, or dictionary comprehension

```aura
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

[Lexical Structure](/manual/lexical-structure) defines literal spellings and
default literal types. A name must resolve under the rules in
[Names And Scopes](/manual/names-and-scopes).

Parentheses without a comma group exactly one expression:

- `(value)` is a group.
- `(value,)` is a singleton tuple.
- `(left, right)` is a two-element tuple.

A tuple value always needs parentheses. Aura does not accept a bare comma
expression.

## Tuple Expressions

A tuple expression evaluates and captures its elements from left to right:

    pair = ("north", 7)
    nested = (pair, (true,))

Its type is the fixed structural tuple of its element types. A tuple copies
when every element type copies. Otherwise it moves as one complete value.
[Tuples](/manual/tuples) covers unpacking and matching.

A postfix tuple index is narrow on purpose:

    coordinates = (3, 4)
    vertical = coordinates[1]

The index must be a non-negative integer literal known at compile time. It
must be in bounds and must select a copy element. The result is a copy.
Dynamic, negative, out-of-bounds, and non-copy-element tuple indexes are
static errors. To take ownership of a non-copy element, unpack the tuple.

Tuples support `==` and `!=`, as described in
[Tuple Equality](#tuple-equality). Tuple ordering operators are unavailable.

## Delimiter Continuation

An expression may span physical lines while a `(`, `[`, or `{` is open. The
lexer joins those lines before the expression grammar runs. The rule is the
same for every delimited form:

- grouping
- function and constructor calls
- indexes and owned slices
- specialization and type arguments
- collection literals
- delimited parts of headers and declarations

Continuation indentation is visual only. It does not create a suite or change
evaluation order. The standard style indents continued content by one level.

The other line rules still hold:

- A trailing comma is invalid.
- A newline outside an open delimiter ends the logical line.
- A backslash does not continue a line.
- Ordinary strings and f-strings stay on one line.

## Evaluation Order

Evaluation runs left to right. The exceptions are the short-circuit boolean
operators and the control-flow expressions.

- A binary expression evaluates its left operand before its right operand.
- A postfix expression evaluates its base before its suffix inputs.
- An index evaluates its base before its index.
- A slice evaluates its base, written start, and written end once each, from
  left to right. An omitted endpoint evaluates nothing.
- A receiver is evaluated before the call arguments.
- Explicit call and constructor arguments are evaluated in source order. Each
  copy or move result is captured before later arguments run their side
  effects.
- Collection elements are evaluated in source order.
- Dictionary entries are evaluated in source order, each key before its value.
- A comprehension evaluates its clauses and filters before its output
  expression, even though the output is written first. Nested clauses run
  outer-major, filters run left to right, and a dictionary output's key comes
  before its value.
- F-string interpolations are evaluated from left to right.
- A conditional expression evaluates its condition first and then exactly one
  arm.
- A match scrutinee is evaluated once, before an arm is selected.
- A comparison chain evaluates its operands left to right, each at most once.
  It evaluates no operand after its first false link.

Order matters when an expression moves a value, mutates through a call,
performs I/O, or can fail at runtime. These rules apply:

- A copy place contributes the value copied at its evaluation point.
- A non-copy place stays borrowed through the operation's later inputs when it
  is a binary left operand, an index base, a method receiver, or an
  indexed-assignment target. Another shared borrow is allowed. An overlapping
  mutable borrow or consumption is rejected with `AU3002`. The diagnostic
  names both the conflict and the place where the retained borrow began.
- Name roots and projected member places follow the same rule. Aura never
  deep-clones the selected place implicitly.
- Static borrow analysis checks all accesses at one call boundary together,
  even though runtime evaluation stays ordered.

## Precedence And Associativity

The table runs from lowest to highest precedence:

| Level | Form | Associativity |
| --- | --- | --- |
| 1 | `value if condition else alternative` | right |
| 2 | `or` | left |
| 3 | `and` | left |
| 4 | prefix `not` | right |
| 5 | `==`, `!=`, `<`, `<=`, `>`, `>=`, `in`, `not in` | chained left to right |
| 6 | `\|` | left |
| 7 | `^` | left |
| 8 | `&` | left |
| 9 | `<<`, `>>` | left |
| 10 | `+`, `-` | left |
| 11 | `*`, `/`, `//`, `%` | left |
| 12 | prefix `match`, `try`, unary `-`, unary `~` | prefix/right |
| 13 | `**` | right |
| 14 | specialization, indexing, member access, call, numeric cast | left-to-right postfix chain |
| 15 | primary expression | none |

Arithmetic, shift, bitwise, and boolean chains fold to the left. Power is the
right-associative exception:

```text
a - b - c       means (a - b) - c
not a == b      means not (a == b)
a + b * c       means a + (b * c)
2 ** 3 ** 2     means 2 ** (3 ** 2)
-2 ** 2         means -(2 ** 2)
```

Equality, ordering, and membership share one precedence level and chain as
they do in Python. `a < b < c` is one chain, not a comparison of a
comparison. `a == b == c` and `a < b == c` chain the same way. A chain of `n`
operators equals the conjunction of its `n` adjacent comparisons, except that
each operand expression is evaluated at most once. Parentheses make a nested
comparison explicit: `(a == b) == c` is a different form that compares a
`bool` against `c`.

Parentheses override precedence:

```aura
scaled = (left + right) * factor
inside = lower < value < upper
```

## Boolean Operators

`and`, `or`, and `not` operate on `bool`. Aura has no general truthiness
conversion for numbers, strings, collections, resources, or classes.

`and` and `or` short-circuit:

- `left and right` evaluates `right` only when `left` is `true`.
- `left or right` evaluates `right` only when `left` is `false`.

`not value` evaluates its operand and negates the boolean result. A supported
user type can provide `not` through the matching operator trait. See
[Generics And Traits](/manual/generics-and-traits#operator-traits).

## Conditional Expressions

`value if condition else alternative` selects one of two values, as in
Python. For example, `label = "ready" if ready else "waiting"` chooses one
`str`.

- The condition is evaluated first, exactly once, and must have type `bool`.
- When the condition is `true`, only `value` is evaluated. When it is
  `false`, only `alternative` is evaluated.
- Both arms must have one static result type.
- Both arms are checked, even when the condition is a literal.

The expected type from the surrounding context flows into both arms. So
contextual literals such as integer literals, `None`, and empty collections
can adopt that type. This context is structural. An empty collection nested
inside a tuple arm adopts the matching concrete nested type from the other
arm or from the expected type. Contextual typing never converts a value that
is already bound.

The form binds more loosely than `or` and associates to the right:

- `a or b if ready else c` means `(a or b) if ready else c`.
- `a if first else b if second else c` means
  `a if first else (b if second else c)`.

A condition that tests a union place with `is None` or `is not None`
[narrows](/manual/enums-and-match#conditional-narrowing) that place in the
arm the test selects. So `value + 1 if value is not None else 0` reads
`value` as its non-`None` member.

The checker tracks ownership separately for each arm and merges the results
conservatively afterward. A non-copy value that either arm moves is
unavailable after the conditional expression. The surrounding use decides
whether an arm moves:

- Passing the result to an ordinary shared-borrow parameter borrows the
  selected arm and keeps both source owners.
- Assignment, return, or an `own` parameter consumes the selected value.

## Arithmetic And Comparison

Built-in arithmetic works on two operands of the same integer type or the
same floating-point type. `str + str` concatenates strings. Aura does not
implicitly widen non-literal numeric values.

### Operator Result Types

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
| `&`, `\|`, `^`, unary `~` | Same exact integer type |
| `<<`, `>>` | Same exact integer type for value and count |
| `==`, `!=` | `bool` for equal operand types |
| `<`, `<=`, `>`, `>=` | `bool` for equal numeric types or two Duration values |
| `in`, `not in` | `bool` for a supported container |

### Division And Remainder

Builtin integer `/` is a static error, and so is integer `/=`. The
diagnostic points to `//` for a floor quotient, or to `.to_float()` on both
operands for floating true division.

- Integer `//` rounds the mathematical quotient toward negative infinity.
  Integer `%` is its paired remainder.
- Floating `//` and `%` use the matching CPython-compatible divmod
  correction.
- In both numeric domains, a nonzero remainder has the divisor's sign.
- Integer and floating `//` or `%` by zero, and floating `/` by zero, are
  runtime failures.

[Execution Model](/manual/execution-model#operators) has the complete runtime
contract.

`divmod(left, right)` evaluates both arguments once. It returns the same floor
quotient and remainder as `(left // right, left % right)`, in one tuple. Both
arguments have one exact integer or floating type, and both tuple elements
have that type too. A zero divisor fails with `AU4004`.

### Power

Integer power is checked and keeps the exact operand type:

- The exponent must be non-negative.
- `x ** 0` is `1`, including `0 ** 0`.
- A negative exponent visible in source is rejected with `AU2003`.
- A negative exponent found at runtime fails with `AU4001`.
- Overflow fails with `AU4002`.

Floating power also requires equal operand types and returns that type. It
reports a domain error with `AU4001` for zero raised to a negative exponent,
and for a negative finite base with a non-integral finite exponent. Overflow
from finite inputs fails with `AU4002`.

### Bitwise And Shift Operators

Bitwise operators use each integer's fixed declared width. `&`, `|`, and `^`
combine corresponding bits. `~` flips every bit. Binary operands must have
the same exact concrete integer type.

A shift's count has the same exact type as the shifted value, and it must
satisfy `0 <= count < width`. Signed right shift is arithmetic. Unsigned right
shift is logical. Ordinary `<<` is checked and fails with `AU4002` when the
mathematical result does not fit.

### Wrapping And Saturating Operations

Ordinary arithmetic is checked. For other overflow behavior, every scalar
integer type provides exact-width `wrapping_add`, `wrapping_sub`,
`wrapping_mul`, `saturating_add`, `saturating_sub`, and `saturating_mul`.

The scalar methods `wrapping_shl`, `wrapping_shr`, `saturating_shl`, and
`saturating_shr` take a count of the receiver's exact type. They apply the
same `0 <= count < width` rule as the shift operators.

- Wrapping left shift discards high bits.
- Saturating left shift clamps to the integer type's bounds.
- Both named right-shift modes produce the same value as ordinary `>>` after
  they validate the count.

`Array[int32]` and `Array[int64]` provide the add, subtract, and multiply
named operations. The right operand is a same-dtype scalar or an exact-shape
Array.

### Rounding And Conversion

`round(value)` returns an integer unchanged, with its exact type. A `float32`
or `float64` value rounds to `int64` using nearest-integer ties-to-even.
Signed zero becomes integer zero. NaN, infinity, and a rounded result outside
the `int64` range fail with `AU4002`. Aura has no digit-count overload.

An unsuffixed integer literal may take the type of a `float32` or `float64`
operand when the literal's value is exactly representable in that floating
type. So `7.5 // 2` is floating floor division and `-7.5 % 2` is floating
remainder. This rule never converts a bound integer variable. An inexact
literal is rejected. Write an explicit floating spelling when rounding at the
literal is intended, or use `.to_float()` for an intended integer-to-`float64`
conversion.

Every integer type provides `.to_float() -> float64`. The conversion uses
IEEE-754 round-to-nearest, ties-to-even, and may lose integer precision:

```aura
left: int64 = 9007199254740993
right: int64 = 2
ratio = left.to_float() / right.to_float()
rounded = left.to_float() # 9007199254740992.0
```

Use this method when rounding into the floating domain is intended. An
explicit integer `as float32` or `as float64` cast has a stricter exactness
contract. See [Numeric Casts](#numeric-casts).

### Duration Arithmetic

Duration arithmetic works on the exact signed nanosecond count.

- Addition, subtraction, and multiplication are checked.
- `Duration // int64` rounds the signed nanosecond quotient toward negative
  infinity. A zero divisor fails with `AU4004`. An unrepresentable result
  fails with `AU4002`.
- Duration equality and ordering compare the signed count.

The language has no `Duration / int64`, `Duration % int64`,
`Duration * float`, or unary `-Duration` rule. Use `Duration.ms(-1)` when you
need a negative value. Negative values are not valid host waits.

### Array Operators

`Array[T]` adds exact-shape elementwise `+`, `-`, and `*` for its four
supported numeric dtypes. A same-dtype scalar may appear on either side.
Floating Arrays also support `/`. Integer Array `/` is the same `AU2003`
static error as scalar integer `/`. Every result is a fresh Array. There is
no array-shape broadcasting and no mixed promotion. See
[Numeric Arrays](/manual/numeric-arrays).

### Equality

`==` and `!=` produce `bool` for two operands of equal type. They read both
operands and move neither. Evaluating an operand keeps its own ordinary
ownership effects.

Optional values have no separate equality rule. A `T | None` operand compares
with a bare `None`, or with a value of one of its member types, under the
[union rule](#union-equality). The comparison does not narrow the operand.
Unit `None == None` is `true`, and unit `None != None` is `false`.

Builtin equality does not use an equality operator trait in Aura 0.3.

### Tuple Equality

Tuple `==` and `!=` require exactly the same static tuple type. They compare
corresponding element values from left to right with ordinary equality,
recursing into nested tuples. The comparison reads both complete operands and
moves neither, even when a tuple contains non-copy elements. Only element
values decide the result. Runtime element-type, transport, and backend
metadata does not.

A tuple literal on either side may take its context type from the other
operand's known tuple type, recursively through nested literals. After this
symmetric contextual typing, the two static tuple types must still match
exactly. The equality operation adds no move of the resulting tuple.

### Union Equality

Two values of the same normalized union are equal when their active members
agree and their payloads are equal. Equality is available only when every
member defines it. So a union with a callable, `random.Rng`, opaque handle,
or `Array` member reports `AU2008`, whatever its current member is.

When exactly one operand has a union type, the other operand is injected for
the comparison under the [union injection](/manual/types#scalar-types) rules:

- A typed member value selects its member.
- A literal that fits several members is `AU2011`.
- A nonmember, or a different normalized union, is `AU2003`.

The rule is symmetric, so `value == 1` and `1 == value` behave the same. Each
operand is evaluated once, and no payload is moved or cloned. Comparing with
`None` works when the union has a `None` member, and it establishes no
narrowing fact.

Where a hashing operation is available, a union hashes as its active member.
So a member value and the union that holds it hash alike and may share a
dictionary or set slot. Different members compare unequal but may collide.

Ordering and arithmetic are never available on a union, even when every
member supports them. They report `AU2003`.

### None Tests

`value is None` and `value is not None` are the two `None` tests. They sit at
the comparison level, produce `bool`, and evaluate their operand exactly once
as its declared type.

`is` exists only inside these two complete forms. Aura rejects `None is
value`, other identity comparisons, `isinstance`, and truthiness unwrapping.
A `None` test cannot be chained with a comparison operator.

A test on a place whose type has no `None` member is a constant test, and it
still evaluates the operand. When the operand is a stable place of a union
type, the test also establishes a narrowing fact for the branches it selects.
See [conditional narrowing](/manual/enums-and-match#conditional-narrowing).
Equality with `None` compares values and never narrows.

### Operator Traits

Arithmetic and ordering may resolve through the corresponding operator trait.
For non-numeric user types, `/` requests `Div.div`. `//` requests
`FloorDiv.floor_div` when neither a builtin numeric rule nor the builtin
`Duration // int64` rule applies.

Tuple `<`, `<=`, `>`, and `>=` are static errors. Aura has no lexicographic
tuple ordering, and an `Ord` implementation cannot add one to a structural
tuple type.

## Membership And Comparison Chains

`value in container` and `value not in container` test membership and produce
`bool`. The container decides which member the test delegates to and which
type the value must have:

| Container | Tests | Delegates to | Value type |
| --- | --- | --- | --- |
| `list[T]` | element membership | `contains` | `T` |
| `set[T]` | element membership | `contains` | `T` |
| `dict[K, V]` | key membership | dictionary key lookup | `K` |
| `str` | substring containment | `contains` | `str` |

- Any other container type is rejected with `AU2003`.
- A value whose type is not the container's element, key, or substring type
  is rejected with `AU2002`.
- An unsuffixed numeric literal on the value side may adopt the container's
  element or key type.
- `not in` is exactly the negation of `in`, not a separate member.

`in` reads both operands and never moves either one. The member it delegates
to takes a shared borrow of the container and a shared borrow of the value.
The value is evaluated before the container, matching source order.

```aura
ports = [80, 443]
print(443 in ports)
print(8080 not in ports)
print("/health" in "GET /health HTTP/1.1")
```

A comparison chain such as `low <= value < high` evaluates its operands left
to right, each at most once, and stops at the first link that is `false`. It
does not evaluate the operands after that link. Every link must be a valid
comparison of its two adjacent operands under the rules above. The chain's
result is `bool`.

```aura
def in_range(value: int32, low: int32, high: int32) -> bool:
    return low <= value < high
```

Tuple equality links follow the same rule. In `first == middle != last`,
`middle` is evaluated once and reused by both adjacent links, and `last` is
skipped when the first link is false. Tuple equality does not consume any
evaluated chain operand.

The checker treats every chain operand as evaluated, even one that
short-circuiting would skip at runtime. So a chain reports an ownership
conflict that only one runtime path would reach. The other branching forms
use the same conservative rule.

## Numeric Casts

`expression as NumericType` performs an explicit numeric conversion. The
supported target spellings are:

```text
int int8 int16 int32 int64 int128 intsize
uint8 uint16 uint32 uint64 uint128 uintsize
float32 float64
```

The target spelling `int` is exactly the same target type as `int64`.

Casts are postfix and bind more tightly than arithmetic:

```aura
whole = 7.9 as int32
widened = 3 as float64
total = left + right as int64
```

The last line means `left + (right as int64)`. Use parentheses to cast a
larger expression. Each conversion must satisfy the checked range and
precision rules in [Types](/manual/types#casts). Non-numeric casts are not
implemented.

## Postfix Expressions

A primary expression may be followed by specialization, indexing, slicing,
member access, calls, and numeric casts. Suffixes apply from left to right. To
apply a suffix to the result of a prefix or binary expression, parenthesize
that expression first:

```aura
users[0].name.clone()
Result[int32, str].Ok(7)
value as int64
```

The syntax-complexity budget in
[Grammar](/manual/grammar#syntactic-complexity-limits) limits postfix chains.

## Calls And Argument Binding

A call has zero or more comma-separated arguments:

```aura
print("hello")
range(1, 4)
process.run(["echo", "hi"], stdout=process.pipe(), group=true)
replace(from="old", to="new")
```

Positional arguments come before named arguments. Static binding works as
follows:

1. Positional arguments fill parameters in declaration order.
2. A named argument fills the parameter with the same name.
3. A parameter cannot be filled more than once.
4. Unknown names and extra arguments are rejected.
5. Every omitted parameter must have a default.
6. Each argument must have the substituted parameter type.

Arguments do not accept a trailing comma. A call may span physical lines while
its `(` is open.

Evaluation follows these rules:

- Every supplied argument is evaluated in call-site source order before the
  next expression begins.
- A copy or move result is captured in its parameter slot. A borrow-mode
  selection is set up without cloning and is subject to the retained-borrow
  overlap rule.
- Later side effects cannot change an argument that is already captured.
- Defaults for omitted parameters are then evaluated afresh, in declaration
  order. No default runs for a supplied parameter.
- Binding a named value to its parameter slot never reorders evaluation.
- Mutable defaults are not shared process-global singletons.

Call sites pass a value directly to bare, `own`, and `mut` parameters.
Capability-prefixed argument forms are not expressions. The callee's
signature decides whether the argument receives shared access, ownership, or
mutable access. A bare parameter is logically shared for every type. An
explicit `own` parameter transfers ownership. See
[Functions](/manual/functions#parameter-passing-modes) and [Ownership And
Borrowing](/manual/ownership-and-borrowing).

Calling a class name constructs the class. Calling an enum variant constructs
that variant. Every class field and enum payload is an owned position.
Constructor arguments follow the same positional-then-named rule and must
supply every required field or payload exactly once. Named enum-variant
arguments evaluate in their written source order. Their captured results then
bind by payload name to declaration-order slots. Slot binding never reorders
the argument expressions.

## Explicit Generic Specialization

Explicit type arguments use brackets:

```aura
box = Box[int32](value=42)
value = identity[int64](7)
result = Result[int32, str].Ok(7)
```

Specialization and indexing share `[...]`. The parser treats brackets as
specialization only when their contents form one or more type references and
one of these holds:

1. `(` follows and the base is a name or member.
2. `.` follows and the final target name begins with uppercase ASCII.

Otherwise the brackets are indexing. So `Box[int32](...)` specializes,
`Result[int32, str].Ok(...)` specializes, and `values[index]` indexes. A bare
`Box[int32]` is not a general first-class specialized-type value.

Type arguments do not accept a trailing comma.
[Static Semantics](/manual/static-semantics#contextual-inference) defines
generic inference, arity, and trait-bound rules.

## Member Access

`object.member` selects a visible field, method, enum variant, module item, or
builtin member:

```aura
point.x
point.distance()
Status.Ready
io.Error.NotFound
```

An instance method call evaluates the receiver before its arguments. The
method declaration decides how it takes the receiver:

- `self` shares it.
- `own self` consumes it.
- `mut self` takes it mutably.

A method without a receiver is associated and is called through its type.

Visibility and resolution are static. A missing or private member is a
compile-time error.

## Indexing

`base[index]` evaluates the base, then the index. Direct indexing works on
lists, dictionaries, and numeric Arrays:

```aura
values[0]
counts["ready"]
matrix[1, 2]
```

### List Indexes

List indexes use the `int64` index domain.

- Non-negative indexes are zero-based.
- A negative index `i` is normalized once as `len + i`, so `values[-1]`
  selects the last element.
- An index still outside the operation's valid range after normalization is
  not clamped.
- A contextually typed integer literal adopts `int64`. Fixed-width `int8`,
  `int16`, `int32`, `uint8`, `uint16`, and `uint32` values widen losslessly,
  but only at an index-domain position.

The same rules apply to indexed assignment and the public List index methods.

A direct list read of a copy element returns the value. Moving a non-copy List
element by direct indexing is restricted and reports `AU3005`. For an explicit
cloned optional read of a clone-safe element type, use `get(index)`. To
transfer a stored value that cannot be cloned, use `pop(index)`. Index
assignment is a statement target. See
[Statements](/manual/statements#bindings-and-assignment).

### Dictionary Indexes

A dictionary index must have exactly the dictionary's key type. Direct reads
are allowed only when the value type is copyable. For a non-copy value:

- Use `get(key)` for an explicit cloned optional read, when the value type is
  clone-safe.
- Use `remove(key)` to transfer any stored value, including one that contains
  `random.Rng`.

A missing key in a direct read is a runtime `AU4003` lookup violation.

### Array Coordinates

An `Array[T]` index has one `int64` coordinate per runtime axis. Coordinates
evaluate left to right, and each negative value is normalized once against
its own axis.

- A direct out-of-range coordinate is `AU4003`.
- A direct coordinate-count or rank mismatch is `AU4007`.
- `get(list[int64])` returns a `T | None`. It is `None` for an invalid
  coordinate or rank.
- Mutable `set(list[int64], value)` returns the old scalar on success. It
  traps on an invalid coordinate or rank instead of returning `None`.

### String Indexes

Integer indexing on `str` is unavailable. Use a slice to select a substring,
or the string methods for whole-string operations. Exact UTF-8 conversion is
available through `text.to_bytes()` and `str.from_bytes(bytes=...)`.

## Slicing

`base[start:end]` selects the half-open range from `start`, inclusive, to
`end`, exclusive. Slicing works on `list[T]`, `str`, and `Array[T]`. It always
returns a fresh owned value of the same type:

    middle = values[1:3]
    prefix = values[:2]
    suffix = values[-2:]
    all_values = values[:]
    scalars = "A🎉Z"[1:2]
    first_rows = matrix[0:2]

### Endpoints

- An omitted start means zero. An omitted end means the source length.
- Equal endpoints produce an empty result.
- Every written endpoint uses the `int64` position domain. Fixed-width
  `int8`, `int16`, `int32`, `uint8`, `uint16`, and `uint32` values widen
  losslessly at that position.
- A negative endpoint `i` is normalized exactly once as `len + i`.
- After normalization, start and end must each be in `0..=len`, and start must
  not exceed end. Otherwise evaluation traps with `AU4003`.

Aura differs from Python here on purpose: slice endpoints are **not
clamped**. An endpoint still out of range after one normalization is a broken
invariant, not a request for the nearest boundary. A reversed range is also an
`AU4003` failure, not an empty slice.

The base, written start, and written end are evaluated once each, from left
to right. A non-Copy base stays retained while the endpoints are evaluated.
So an endpoint may read the source but cannot mutate or consume it. No list,
str, or Array slice is a place or a view.

### List Slices

A list slice copies Copy elements and clones non-Copy elements into a fresh
owned list, so the element type must be clone-safe.

- A type that contains `random.Rng`, an opaque FFI handle, or a capturing
  closure environment is rejected with `AU3007`.
- A type that contains a non-repeatable Task result right is rejected with
  `AU3009`.

Generic slicing infers the same obligation for its element type. The source
stays usable.

### String Slices

String endpoints count Unicode scalar values, matching `str.len()`. They do
not count UTF-8 bytes or grapheme clusters. Finding scalar boundaries scans
the source, so str slicing is O(n). The result is a newly allocated, valid
UTF-8 str. Integer `string[index]` is unavailable.

### Array Slices

An Array slice applies the range only to axis zero. It copies complete rows
and keeps all later dimensions. Its first result dimension is `end - start`.
It follows the same rules as other slices: the `int64` domain, one-time
negative normalization, no clamping, `AU4003`, an owned copy, no step, and no
assignment. It is not a multidimensional slice or a view.

### Steps And Assignment

A second colon is reserved for future step syntax. `value[start:end:step]` and
`value[::]` report `AU2005` with `slice steps are unavailable; use an explicit
loop to select a stride`.

Slice assignment and compound assignment report `AU2005` with `slice
assignment is unavailable because slices are owned copies; mutate the source
by index or build a new value`.

## Collection Literals

Aura has list, set, and dictionary literals:

```aura
values = [1, 2, 3]
seen = {1, 2, 3}
counts = {"ready": 2, "done": 1}
```

In a nonempty brace literal, the first colon makes it a dictionary. Without a
colon, it is a set. Elements, keys, and values must have consistent types
after contextual inference.

An empty literal contains no values to infer an element type from, so it
needs an expected type:

```aura
values: list[int32] = []
counts: dict[str, int32] = {}
seen = set[int32]()
```

`{}` is a dictionary literal. An empty set uses `set[T]()`.

Collection literals may span physical lines while their `[` or `{` is open.
They do not accept trailing commas.

- Lists and sets evaluate elements in source order.
- Dictionaries evaluate entries in source order, each key before its value.
- When two evaluated dictionary keys are equal, the later value replaces the
  earlier one. The key keeps its first insertion position.

## Comprehensions

A comprehension is an eager collection expression:

    doubled = [value * 2 for value in values]
    visible = {value for value in values if value >= 0}
    by_id = {item.id: item for item in items}

A comprehension needs one or more `for` clauses. A clause may have several
`if` filters and may be followed by another clause:

    coordinates = [
        (row, column)
        for row in rows if row >= 0
        for column in columns if column >= 0
    ]

### Order

The output expression is written first, but evaluation starts at the first
iterable:

1. The clause binds its target.
2. Its filters run from left to right.
3. The next clause selects its iterable, and the steps repeat.
4. At the innermost surviving combination, the output runs.

Nested traversal is outer-major. Every surviving inner item for one outer
target is produced before the next outer item. A dictionary output evaluates
and captures the key before it evaluates the value.

### Sources And Results

Each clause iterates the way a bare `for` loop does:

- List and set inputs are shared and frozen.
- Range yields copy values.
- `enumerate(...)` and `zip(...)` keep their loop contracts.
- Queue keeps its special receive semantics: the handle is copied and each
  target arrives owned.

A comprehension does not accept `mut` or `own` before its source.

The result is a newly owned `list[T]`, `set[T]`, or `dict[K, V]`. It is never
a view or a lazy iterator. Inserting into the result takes ownership of
non-Copy values. A shared non-Copy source element must be cloned explicitly,
when it is clone-safe. Values received from a Queue are owned and may move
directly.

Targets are scoped progressively over their filters, later clauses, and the
output. They disappear when the expression ends.

### Lambdas In Comprehensions

A lambda inside a comprehension uses the ordinary closure capture rules. For
example, a compiler-known callback can capture a Copy value while an element
expression calls it:

    shifted_rows = [
        row.map(lambda value: value + offset)
        for row in rows
    ]

The lambda is created only for an element that evaluation reaches. Shared
non-Copy capability capture is still rejected. A capturing closure cannot
itself become a stored comprehension element. See
[Closures](/manual/closures).

### Generator Expressions

Generator expressions are unavailable. `(value for value in values)` and
`consume(value for value in values)` report `AU2005`. The diagnostic suggests
an eager owned list comprehension or an explicit loop.

## F-Strings

An f-string produces an owned `str`. It evaluates interpolations from left to
right, and renders each one to `str` before the next begins:

```aura
name = "aura"
count = 3
message = f"{name}: {count}"
report = f"{name:<12s} {count:>8,d}"
```

Interpolation contents are ordinary expressions. A top-level colon starts a
statically checked format specification. It supports fill, alignment, sign,
width, decimal grouping, precision, and a closed set of string and numeric
type codes.

- For numeric values, a width that begins with `0` pads after the sign, like
  Python's `09.3f` shorthand.
- Formatting uses the value's exact static numeric width, so a `float32` is
  formatted from its binary32 value.

[Lexical Structure](/manual/lexical-structure#f-strings) defines string
spelling, escapes, literal braces, and the complete format grammar.

## `len` And `str`

`len(value)` and `str(value)` are builtin functions, not syntax.

`len(value)` delegates to the value's own `len()` member and produces `int64`.
It accepts every type that provides `len()`: `str`, `list[T]`, `dict[K, V]`,
`set[T]`, and `Array[T]`. A value without that member is rejected with
`AU2002`.

- The `len()` members also produce `int64`, so `len(value)` and `value.len()`
  have the same static type and value.
- `str.len()` counts Unicode scalar values. `str.byte_len()` counts UTF-8
  bytes and also produces `int64`.
- Neither `len` spelling changes ownership, because `len()` borrows its
  receiver.

`str(value)` produces the same `str` that `print(value)` writes and that
`f"{value}"` interpolates. It accepts any value the renderer accepts, not only
scalars.

The unit value `None` renders as `None` in `print`, `str`, and interpolation.
So an absent `T | None` value, or a unit enum payload such as
`Result.Ok(None)`, prints by name instead of as empty text.

```aura
hosts = ["alpha", "beta"]
print(len(hosts))
print(str(len(hosts)))
```

Both names are builtin function names. Like `print` and `abs`, a program
cannot redefine them.

## Match Expressions

`match` can produce a value. Its scrutinee is evaluated exactly once. Arms are
tried in source order, and only the first matching arm's expression is
evaluated.

An arm contains exactly one expression. It may be inline:

```aura
label = match code:
    case 0: "ok"
    case _: "other"
```

Or the expression may sit on one indented line after the `case`:

```aura
label = match code:
    case 0:
        "ok"
    case _:
        "other"
```

The indented form is still one expression, not a suite of statements. Every
arm must produce one compatible result type. The match must be exhaustive
under [Enums And Pattern Matching](/manual/enums-and-match#typing-rules).

A complete match expression may appear anywhere an expression is expected:
an initializer, a return value, a call argument, a collection element, or a
group. Inside an enclosing delimiter, the required arm layout forms a layout
island. Ordinary continuation does not suppress it. [Grammar](/manual/grammar#match-expressions)
defines the exact forms.

Use `match value` to inspect a non-copy scrutinee without consuming it. Use
`match mut value` when an arm must mutate through payload bindings.

## `try`

`try expression` operates on `Result[T, E]`:

```aura
def parse_value(text: str) -> Result[int32, str]:
    value = try parse_int32(text)
    return Result.Ok(value)
```

The operand is evaluated once:

- `Result.Ok(value)` makes the `try` expression produce `value`.
- `Result.Err(error)` returns immediately from the enclosing function.

The enclosing function must return a compatible `Result`. When the error types
differ, one applicable `From[SourceError] for TargetError` implementation may
convert the error. An early return runs active `with` cleanups. See
[Execution Model](/manual/execution-model#try).

## Enum Construction

An enum constructor names the enum, or a specialized enum, followed by the
variant:

```aura
result: Result[int32, str] = Result.Ok(7)
missing: Lookup[str] = Lookup.Missing
ready = Status.Ready(count=3)
```

The variant must exist and receive exactly its declared payload shape. Generic
enum arguments may be inferred from an expected type or from the payloads.
When inference cannot resolve every type parameter, explicit specialization is
required.

Bare builtin variants such as `Ok`, `Err`, `Some`, or `None` are accepted only
where the expected enum identity is unambiguous. Qualified construction is the
preferred style for reference and book code.

## Function Values And Indirect Calls

A named module-level function may appear as an expression. Its value is a copy
code pointer with a type such as `def(T1, mut T2, own T3) -> R`, where bare
parameters are shared. Calling that value uses the ordinary call production
and keeps the named function's parameter capabilities. Explicit generic
specialization such as `show[int32]` fixes one concrete function value before
it is stored or called.

Function-valued variables, parameters, fields, and collection elements are
ordinary primary and postfix expressions. Whether an indirect call keeps
parameter names and defaults depends on where the value came from:

- A value with one statically known source declaration keeps that
  declaration's parameter names and defaults.
- A control-flow selection keeps them too, when all candidates agree on their
  names and default availability. Each omitted argument evaluates the
  selected target's own default expression.
- Conflicting reassignment, structural function returns, class-field loads,
  and mutable-collection loads have only the structural function type. Calls
  through them need the complete positional argument list.

Storage keeps each parameter's bare shared, `mut`, or `own` ABI capability.

Contextually typed `lambda parameters: expression` values use the same
callable contract and may capture owned outer locals by value. See
[Closures](/manual/closures). `receiver.method` outside call position is a
bound method closure, and `Class.method` is an associated function value. See
[Closures](/manual/closures#bound-methods). Interactions with trait objects
are unavailable.

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

Aura 0.3 expressions do not include:

- generator expressions
- assignment expressions
- call-site capability annotations
- non-numeric casts
- ordinary trailing commas. The required singleton-tuple comma is the one
  exception.

Lambdas are expression-bodied and contextually typed. There are no
statement-bodied lambdas and no implicitly reference-capturing lambdas.

A form that is absent from [Grammar](/manual/grammar) is not part of the
implemented expression language.

## Grammar

[Grammar](/manual/grammar) holds the normative productions for these
expressions: primary, postfix, power, unary, multiplicative, additive, shift,
bitwise, comparison, Boolean, conditional, `match`, `try`, lambda, collection
literal and comprehension, constructor, and f-string.

The comparison production covers equality, ordering, and membership at one
level, and it admits a chain of two or more operators. The precedence and
associativity table above resolves every accepted operator sequence. A
spelling absent from those productions is not accepted as an implicit
extension.

## Typing Rules

Each expression receives exactly one static type. Calls, constructors,
operators, indexing, member access, collections, matches, casts, and `try`
must satisfy their rules above after generic substitution.

- Context may type a literal, including an exactly representable integer
  literal in a floating context. Context never converts a bound variable.
- Branching expressions need a single result type on every arm.
- A list or set comprehension's output expression determines `T`. A
  dictionary comprehension's key and value expressions determine `K` and `V`.
  An expected result specialization provides context before inference.
- Comprehension filters need exact `bool`, and every source uses the static
  iterable rules of a bare statement loop.

## Runtime Semantics

Evaluation follows the order in [Evaluation Order](#evaluation-order). In
addition:

- A binary power, shift, or bitwise expression evaluates its left operand once
  and then its right operand once.
- A compound form selects its target place once and writes only after the
  operation succeeds.
- Named enum arguments evaluate in source order and then bind to
  declaration-order payload slots.
- A membership test evaluates its value before its container.
- `try` either yields an `Ok` payload or returns the `Err` from the enclosing
  function after the required cleanup.
- A comprehension allocates one result. For each current outer combination, it
  evaluates every reached source once, applies filters left to right, and
  then evaluates its output. A trap or `try` propagation drops the partial
  result.

## Ownership And Evaluation Order

Evaluation copies copy values. It moves non-copy values only when the static
context consumes them. Bare parameters grant logical shared access. `own`
parameters and consuming receivers move. `mut` parameters grant exclusive
mutable access.

- A non-copy indexed read reports `AU3005`. Use the safe method surface
  instead of an implicit copy.
- `in`, `not in`, `==`, and `!=` read both resulting operands and move
  neither, including structural tuple equality. Evaluation inside an operand
  keeps its ordinary ownership effects.
- A comparison chain checks every operand as if it were evaluated, even where
  short-circuiting would skip it.
- Binary left operands, index bases, method receivers, and
  indexed-assignment targets keep their non-copy borrow through later inputs.
  An overlapping mutable borrow or consumption is rejected with `AU3002`. No
  hidden clone repairs the expression.

Comprehension targets use progressive child scopes and do not leak. Active
shared sources stay borrowed and frozen through downstream filters, clauses,
and output evaluation. Inserting into the result takes ownership. So copy,
move, explicit-clone, loop-carried-move, and closure capture checks apply
exactly as they do in the equivalent nested bare loops.

## Diagnostics

Compile-time codes:

| Code | Cause |
| --- | --- |
| `AU1101` | Invalid expression syntax, including malformed comprehension clauses and forbidden comprehension `mut` or `own` modifiers. |
| `AU2001` | An unresolved name or member. |
| `AU2002` | A type, constructor-payload, match-result, or index-type mismatch. |
| `AU2003` | An unsupported unary, binary, compound, membership, or cast operator. |
| `AU2004` | Call or constructor argument binding failed. |
| `AU2005` | An unsupported syntax or expression feature, including generator expressions, reserved slice steps, and slice assignment. |
| `AU2008` | Equality on a union that has a member without equality. |
| `AU2010` | A value is not a direct member of its expected union. |
| `AU2011` | A literal that more than one union member could accept. |
| `AU2014` | A member use through a place whose `None`-test narrowing was invalidated by an assignment, a mutable match, or a call with mutable access. |
| `AU2999` | An expression rejection without a narrower compile-time code. |
| `AU3001` | Use of a moved value. |
| `AU3002` | A borrow conflict, including a later mutable borrow or consumption that overlaps a retained non-copy binary operand, index base, method receiver, or indexed-assignment target. |
| `AU3003` | An immutable place used mutably. |
| `AU3004` | An invalid ownership mode. |
| `AU3005` | A direct indexed read would copy a non-copy stored value. |
| `AU3006` | Indexed compound assignment would copy a non-copy stored value during its read-modify-write step. |
| `AU3007` | A list slice whose owned result would duplicate non-cloneable state. |
| `AU3009` | A list slice whose owned result would duplicate a single-consumer Task observation right. |

Runtime codes:

| Code | Cause |
| --- | --- |
| `AU4001` | A general expression trap, including a runtime negative integer exponent and floating power domain errors. |
| `AU4002` | Arithmetic overflow, underflow, range, or conversion-exactness failure, including integer power overflow, invalid shift counts, and checked-left-shift overflow. |
| `AU4003` | A bounds or lookup violation, including an invalid normalized slice endpoint or a reversed range. |
| `AU4004` | A zero divisor. |
| `AU4005` | A trapping resource or I/O failure propagated by a call expression. |
| `AU4007` | A direct Array coordinate-count or rank mismatch. |

## Backend Support

Every expression form this page marks as implemented lowers to MIR, Aura's
mid-level intermediate representation. The direct native backend supports
every one of them. The backend-parity test matrix forces each backend and
checks that the observable results and primary traps match. Compiler analysis
and language-server diagnostics are produced before a backend is selected.

## Limits And Implementation-Defined Behavior

- The parser caps expression nesting and operator chains at 128.
- Physical lines continue only while a source delimiter is open. Backslashes
  and multiline string or f-string literals do not continue them.
- Ordinary trailing commas are unavailable. `(value,)` is the required
  singleton tuple spelling.
- Collection and string resource caps are documented on their feature pages.
- Comprehensions are eager and have no `mut` or `own` source form. They do not
  provide early exit, lazy resumption, or a user-defined iterable protocol.
  Use an explicit loop when you need those properties.
- Floating values follow the specified Aura operations and shortest
  round-trip printing. No backend may substitute a different expression
  result as an implementation-defined choice.

## Status

The expression forms this page defines positively are implemented. They
include:

- delimiter continuation, which adds no new expression form to the syntax
  tree
- conditional expressions
- membership operators and comparison chains
- tuples and tuple equality
- capture-free named function values, indirect calls, and contextually typed
  by-value expression closures
- eager owned list, set, and dictionary comprehensions
- integer base spellings, fixed-width bitwise operations, and shifts
- power, `round`, and `divmod`
- contextual `is None` and `is not None` tests with short-circuit narrowing

Generator expressions, assignment expressions, nonnumeric casts, and call-site
capability modifiers are unavailable.

Design record:
[ADR-0025](https://github.com/johnolafenwa/Aura/blob/main/architecture_docs/decisions/0025-newline-continuation-and-delimited-layout.md),
[ADR-0026](https://github.com/johnolafenwa/Aura/blob/main/architecture_docs/decisions/0026-minimal-tuples.md),
[ADR-0027](https://github.com/johnolafenwa/Aura/blob/main/architecture_docs/decisions/0027-conditional-expressions.md),
[ADR-0028](https://github.com/johnolafenwa/Aura/blob/main/architecture_docs/decisions/0028-membership-and-comparison-chains.md),
[ADR-0037](https://github.com/johnolafenwa/Aura/blob/main/architecture_docs/decisions/0037-expression-closures-and-value-capture.md),
[ADR-0039](https://github.com/johnolafenwa/Aura/blob/main/architecture_docs/decisions/0039-comprehensions.md),
[ADR-0047](https://github.com/johnolafenwa/Aura/blob/main/architecture_docs/decisions/0047-integer-literals-bitwise-and-shifts.md),
[ADR-0048](https://github.com/johnolafenwa/Aura/blob/main/architecture_docs/decisions/0048-power-round-divmod-and-math.md).
