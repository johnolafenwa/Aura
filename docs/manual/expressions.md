# Expressions

An expression evaluates to a value. This chapter defines the reader-facing expression contract: available forms, grouping, precedence, evaluation order, and the main static restrictions. The exact productions and specialization/indexing disambiguation are normative in [Grammar](/manual/grammar#expressions-and-precedence). Type rules are centralized in [Static Semantics](/manual/static-semantics#expression-typing), and runtime behavior is centralized in [Execution Model](/manual/execution-model#evaluation-order).

## Primary Expressions

Primary expressions are the atoms from which postfix, prefix, and binary expressions are built:

- a name such as `count`, `from`, or `point`
- an integer, float, duration, boolean, string, or f-string literal
- `None`
- a parenthesized expression
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
[1, 2, 3]
{"ready": 2}
{1, 2, 3}
```

The lexical spelling and default literal types are defined by [Lexical Structure](/manual/lexical-structure). A name must resolve under [Names And Scopes](/manual/names-and-scopes).

Parentheses group exactly one expression. `(value)` is a group, not a tuple, and `(left, right)` is not Aurora 0.1 syntax.

## Evaluation Order

Except for short-circuit boolean operators and control-flow expressions, evaluation is left-to-right:

- a binary expression evaluates its left operand before its right operand
- a postfix expression evaluates its base before its suffix inputs
- an index evaluates its base before its index
- a receiver is evaluated before call arguments
- explicit call and constructor arguments are evaluated in source order
- collection elements are evaluated in source order
- each map key is evaluated before its value, and entries are evaluated in source order
- f-string interpolations are evaluated from left to right
- a match scrutinee is evaluated once, before arm selection

Evaluation order matters when an expression moves a value, mutates through a call, performs I/O, or can produce a runtime failure. Static borrow analysis checks all accesses at one call boundary together even though runtime evaluation remains ordered.

## Precedence And Associativity

The following table runs from lowest to highest precedence:

| Level | Form | Associativity |
| --- | --- | --- |
| 1 | `or` | left |
| 2 | `and` | left |
| 3 | prefix `not` | right |
| 4 | `==`, `!=` | left |
| 5 | `<`, `<=`, `>`, `>=` | left |
| 6 | `+`, `-` | left |
| 7 | `*`, `/`, `//`, `%` | left |
| 8 | prefix `match`, `try`, unary `-` | prefix/right |
| 9 | specialization, indexing, member access, call, numeric cast | left-to-right postfix chain |
| 10 | primary expression | — |

All binary chains are left-folded. For example:

```text
a - b - c       means (a - b) - c
a == b == c     means (a == b) == c
a < b < c       means (a < b) < c
not a == b      means not (a == b)
a + b * c       means a + (b * c)
```

Aurora does not implement Python-style chained comparisons. Because comparison results are `bool`, a chained form such as `a < b < c` will normally fail static type checking rather than perform a mathematical range test. Write `a < b and b < c`.

Parentheses override precedence:

```python
scaled = (left + right) * factor
inside = lower < value and value < upper
```

## Boolean Operators

`and`, `or`, and `not` operate on `bool`; Aurora has no general truthiness conversion for numbers, strings, collections, resources, or classes.

`and` and `or` short-circuit:

- `left and right` evaluates `right` only when `left` is `true`
- `left or right` evaluates `right` only when `left` is `false`

`not value` evaluates its operand and negates the boolean result. A matching operator trait may provide `not` for a supported user type, as described under [Generics And Traits](/manual/generics-and-traits#operator-traits).

## Arithmetic And Comparison

Built-in arithmetic supports equal integer types or equal floating-point types. `String + String` concatenates strings. Aurora does not implicitly widen non-literal numeric values.

| Operators | Builtin result |
| --- | --- |
| `+` | Same numeric type, or `String` for string concatenation |
| `-`, `*`, `//`, `%` | Same numeric type |
| `/` | Same floating-point type |
| unary `-` | Same numeric type |
| `==`, `!=` | `bool` for equal operand types |
| `<`, `<=`, `>`, `>=` | `bool` for equal numeric types |

Arithmetic and ordering may resolve through the corresponding operator trait. For non-numeric user types, `/` requests `Div.div`; `//` has no operator trait and is builtin-only. Builtin equality does not use an equality operator trait in Aurora 0.1.

Builtin integer `/` is a static error, as is integer `/=`. The diagnostic directs callers to `//` for a floor quotient or to `.to_float()` on both operands for floating true division. Integer `//` rounds the mathematical quotient toward negative infinity, and integer `%` is its paired remainder. Floating `//` and `%` use the corresponding CPython-compatible divmod correction. In both numeric domains, a nonzero remainder has the divisor's sign. Integer and floating `//` or `%` by zero, and floating `/` by zero, are runtime failures. See [Execution Model](/manual/execution-model#operators) for the complete runtime contract.

Every integer type provides `.to_float() -> float64`. This conversion uses IEEE-754 round-to-nearest, ties-to-even and may lose integer precision:

```python
left: int64 = 9007199254740993
right: int64 = 2
ratio = left.to_float() / right.to_float()
rounded = left.to_float() # 9007199254740992.0
```

Use this method when rounding into the floating domain is intentional. An explicit integer `as float32` or `as float64` cast has the stricter exactness contract below.

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

Arguments do not accept a trailing comma and ordinary calls remain on one physical line. Explicit arguments are evaluated in source order. A default expression is evaluated afresh each time its parameter is omitted; mutable defaults are not shared process-global singletons.

Call sites pass a value directly to owned, `borrow`, and `borrow mut` parameters. Prefix argument forms such as `borrow value` are not expressions. The callee signature selects whether the argument is moved, shared-borrowed, or mutable-borrowed. See [Functions](/manual/functions#parameter-passing-modes) and [Ownership And Borrowing](/manual/ownership-and-borrowing).

Calling a class name constructs the class. Calling an enum variant constructs that variant. Constructor arguments follow the same positional-then-named rule and must supply every required field or payload exactly once.

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

An instance method call evaluates the receiver before its arguments. The method declaration determines whether the receiver is consumed, shared-borrowed, or mutable-borrowed. A method without `self` is associated and is called through its type.

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
A map index must have exactly the map's key type.

A direct vector read of a copy element returns the value. Moving a non-copy vector element by direct indexing is restricted; use `get(index)` when the intended operation is an explicit cloned/optional read. Index assignment is a statement target and is covered by [Statements](/manual/statements#bindings-and-assignment).

Aurora 0.1 does not define integer indexing or slicing for `String`. Use the
maintained string methods for whole-string operations; scalar iteration and
explicitly encoded String/bytes conversion arrive in Phase 3, while slicing
waits for Phase 7.

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

Collection literals do not accept trailing commas and remain on one physical line. Lists and sets evaluate elements in source order. Maps evaluate each key before its value and entries in source order.

## F-Strings

An f-string produces an owned `String` and evaluates interpolations from left to right:

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

A complete match expression may appear anywhere an expression is expected, including an initializer, return value, call argument, collection element, or grouping. It is the only maintained multiline accommodation inside a surrounding delimiter. The exact closing-delimiter rule is defined in [Grammar](/manual/grammar#match-expressions); it does not provide general line continuation.

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

Aurora 0.1 expressions do not include tuples, comprehensions, lambdas, conditional expressions, assignment expressions, call-site borrow annotations, non-numeric casts, general multiline delimiters, or trailing commas. If a form is absent from [Grammar](/manual/grammar), it is not part of the implemented expression language.
