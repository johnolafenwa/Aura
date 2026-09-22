# Static Semantics

Static semantics are the rules the compiler checks after parsing and module loading, and before MIR lowering or native code generation. MIR is the compiler's mid-level intermediate representation. A module is well typed only if every declaration, statement, expression, pattern, call, move, and borrow satisfies these rules.

This page states the rules that cut across the language. The chapters on individual declarations add their own contracts. [Ownership And Borrowing](/manual/ownership-and-borrowing) defines place and lifetime restrictions.

## Types And Type Equality

Aura 0.3 has no general subtype relation and no implicit numeric widening. Types match by these rules:

- **Nominal types** match when their canonical names are equal and all type arguments are equal, recursively. Aura 0.3 primarily uses nominal types, and generic arguments are invariant.
- **Tuple types** are structural. Two tuple types match exactly when they have the same arity and every corresponding element type matches, recursively.

Examples:

- `int` and `int64` are the same canonical type.
- `int32` and `int64` are different types.
- `list[int32]` and `list[int64]` are different types.
- two user classes with identical fields are still different types.
- an imported type retains its defining module identity even when imported under an unqualified binding.

An optional type is the union `T | None`. Aura has no builtin `Option` type and no `T?` suffix.

`int` canonicalizes to `int64`, and `str` canonicalizes to `str`. Neither name introduces a distinct runtime type.

Every use of a generic type must supply its declared number of type arguments. A non-generic type rejects type arguments. `Self` is available only in supported trait and implementation type positions.

## Contextual Inference

Aura infers types locally from context. It does not use global inference. Public function parameters, fields, method signatures, and explicit return values stay typed in source.

The checker takes an expected type from context when the rule is unambiguous. The context can be an annotation, a parameter, a return position, a collection, a constructor field, or a surrounding expression.

### Literals

- **Integer literals** adopt an expected integer type and must fit it. In an expected `float32` or `float64` context, an integer literal adopts that floating type only when its mathematical integer value is exactly representable there. An inexact case is a static error that directs the author to an explicit floating spelling or `.to_float()`. With no expected type, an integer literal defaults to `int64`.
- **Negative integer literals** parse as unary `-` applied to a non-negative literal. They follow the same exact float-context rule, or must fit the selected signed integer type.
- **Floating literals** adopt an expected `float32` or `float64`. Otherwise they default to `float64`.
- **`true` and `false`** have type `bool`.
- **String literals** have type `str`, whether single-quoted, double-quoted, triple-quoted, raw, or formatted. The delimiter and literal form do not create distinct types. The checker checks each f-string format specification against the interpolation's static type. Breaking a string-only, integer-only, numeric-only, sign, precision, or grouping restriction is a compile-time error under `AU2002`.
- **Duration literals** have type `Duration`.
- **Bare `None`** has type `None`. In an expected `T | None` position, it denotes that union's absence member. Expected-union context flows through grouping, annotated bindings, return positions, and argument positions. An unannotated `x = None` is a unit binding, not an inferred optional.

For `==` and `!=`, a `T | None` operand compares with a bare `None`, or with a value of a member type, under the union equality rule. The comparison does not narrow. Unit `None == None` is `true` and unit `None != None` is `false`.

### Collections

A non-empty list, set, or dictionary infers its element, key, and value types from the first value, unless an expected collection type is available. All remaining values must have the same inferred type.

A dictionary literal may repeat an equal key. At runtime the later value replaces the earlier value, and the key keeps its first insertion position.

An empty list or dictionary literal requires an expected `list[T]` or `dict[K, V]` type. `{}` is a dictionary literal. An empty set uses `set[T]()`.

### Comprehensions

A list comprehension has type `list[T]`, a set comprehension has type `set[T]`, and a dictionary comprehension has type `dict[K, V]`. An expected collection specialization flows into the element, key, and value expressions before inference. Otherwise those output expressions determine `T`, `K`, and `V` under the ordinary exact-type and contextual-literal rules. A filter must have exactly type `bool`.

The checker checks clauses in runtime order:

1. A clause's iterable is checked before its target enters scope.
2. The target receives the same type and ownership provenance as an ordinary bare `for` target.
3. The target then becomes visible to that clause's filters, to later clauses, and to the output.

Targets cannot shadow visible names or earlier targets, and they do not escape the expression.

Every clause classifies its iterable the same way a bare `for` statement does:

- Lists and sets provide shared elements.
- Range provides copy `int64`.
- The compiler-known `enumerate` and `zip` forms keep their contracts.
- Queue provides owned items, the same exception it has in a bare `for` loop.

The `mut` and `own` clause modifiers are not part of the syntax.

The output storage owns each value inserted into it:

- A copy value is copied.
- An owned non-Copy value moves.
- A shared non-Copy target cannot be inserted without an explicit clone-safe `.clone()`.
- Queue targets already own their received items.

Move checking treats each clause as potentially repeated. It keeps every active source borrow alive through downstream clauses and output evaluation. It rejects loop-carried full or partial moves from outer places.

### Lambdas

A lambda with parameters requires an expected structural function type. That type fixes the parameter count, each parameter type, and each bare, `mut`, or `own` capability. An expected result type constrains the body. The body is checked once under those parameter bindings. Aura does not infer parameter types from operations in the body.

A zero-parameter lambda may infer `def() -> R` from its body when no expected callable type is present.

Without a capture list, a lambda captures by value the outer owned locals and `own` parameters that its body references. When the lambda expression is evaluated, Copy values are snapshotted and non-Copy values move. An explicit, exhaustive capture list may instead request a shared loan, a mutable loan, or an owned capture for each outer local the body uses.

Calling a mutable-loan closure requires a mutable closure place. Loan closures are non-Transfer and stay inside synchronous local use. [Tasks And Static Safety](#tasks-and-static-safety) defines Transfer. See [Closures](/manual/closures) for the full closure contract.

Capture-free lambdas may cross every ordinary structural function-value boundary. Capturing closures carry environment and call-kind metadata. So they cannot coerce through arbitrary written-`def` parameters, stored fields, collections, or annotated returns. These positions preserve the metadata:

- immutable local bindings
- compiler-known repeatable callbacks
- direct calls
- qualifying task starts

### Generic Calls

The checker infers generic type parameters by unifying argument types with parameter type patterns. Where available, it also uses the expected result type. Explicit specialization such as `identity[int64](value)` seeds or fixes the substitutions.

Every declared type parameter must resolve. The substituted type must satisfy all declared trait bounds. Inference does not guess from unrelated declarations or from runtime values.

## Declarations

A declaration is valid only when:

- its item name does not collide with another local/imported item or a reserved builtin
- type parameter names are unique and their bounds name known traits with correct arity
- field, variant, and method names are unique within the relevant declaration
- all referenced types exist with the correct arity
- default expressions have exactly the declared parameter or field type
- a non-`None` function or method returns on every statically reachable fallthrough path
- every view-returning declaration names one bare or mutable receiver/parameter
  origin, and every `return view` derives from that origin with matching kind
- copy classes contain only copy-compatible fields
- trait implementations satisfy the trait's type arguments, supertraits, method set, and method signatures

Class, enum, function, and trait declarations may be `public` at module scope. `impl` cannot be public, because it introduces no item that can be imported on its own.

An extern declaration joins the module namespace but has no Aura body. Its rules:

- The ABI must be `"C"`.
- Its package must be authorized.
- Its complete signature must belong to the fixed FFI v0 scalar/view/opaque-handle table. FFI is the foreign function interface.
- Extern functions are direct-call-only. Referencing one without immediately calling it is rejected. It does not produce a function value.

An opaque declaration contributes a nominal type but no constructor, fields, methods, or Aura-visible layout. See [FFI v0](/manual/ffi).

## Bindings And Assignment

The first simple-name assignment introduces a binding. Its type is the annotation when present, otherwise the initializer type. The initializer must match exactly.

`mut` makes the new binding assignable and a mutable place. Reassignment requires an existing mutable place and keeps the original type. Reassigning a value of the correct type reinitializes a fully moved binding or field.

The compound assignments are `+=`, `-=`, `*=`, `**=`, `/=`, `%=`, `//=`, `&=`, `|=`, `^=`, `<<=`, and `>>=`. Each one reads the current target, applies the corresponding binary operation, and writes the result only after the operation succeeds. The target must already exist, be mutable, not be moved, and have the operation's result type. Integer `/=` is rejected by the same rule and teaching diagnostic as integer `/`. Floating `/=` is valid.

Field assignment requires a mutable base place and a declared field.

Index assignment supports `list[T]` with the `int64` index domain, and `dict[K, V]` with a key of exactly `K`. For a dictionary:

- Simple index assignment accepts any `V`. It replaces the entry for an equal existing key, or inserts a new entry.
- The key and value are owned storage positions. Each is consumed when non-copy.
- The key is fully evaluated and captured before the assigned value is evaluated. Side effects in the value cannot retarget the write.
- Compound indexed assignment is permitted only for copy `V`. For non-copy `V` it is rejected, rather than implicitly cloning the value or removing it before the operator completes.

An annotation and `mut` are not permitted on member or index assignment.

## Expression Typing

### Unary Operators

- `not value` accepts `bool` and returns `bool`, or resolves a matching `Not.not` trait operation.
- `-value` accepts an integer or float and returns the same type, or resolves a matching `Neg.neg` operation.
- `~value` accepts an integer and returns the same exact integer type.
- `try value` requires `value: Result[T, E1]` and an enclosing return type `Result[U, E2]`. It has type `T` when `E1 == E2` or an applicable `impl From[E1] for E2` exists.

### Binary Operators

Built-in operator typing is:

| Operators | Operand rule | Result |
| --- | --- | --- |
| `and`, `or` | both `bool` | `bool` |
| `+` | equal integer types, equal float types, two `str` values, or two Duration values | operand type |
| `-` | equal integer types, equal float types, or two Duration values | operand type |
| `*` | equal integer types, equal float types, `Duration` and `int64` in either order | numeric operand type, or `Duration` |
| `**` | equal integer types or equal float types | operand type |
| `//` | equal integer types, equal float types, or `Duration // int64` | numeric operand type, or `Duration` |
| `%` | equal integer or equal float types | operand type |
| `/` | equal float types | operand type |
| `&`, `|`, `^` | equal concrete integer types | operand type |
| `<<`, `>>` | equal concrete integer types | left operand type |
| `==`, `!=` | equal operand types | `bool` |
| `<`, `<=`, `>`, `>=` | equal integer types, equal float types, or two Duration values | `bool` |

When both operands have the same integer type, `/` is rejected with this exact diagnostic:

```text
integer `/` is not supported; use `//` for floor division, or call `.to_float()` on both operands for true division
```

Operands are not implicitly widened. An integer literal may be contextually typed to match an integer operand. It may also match a `float32` or `float64` operand when the literal is exactly representable in that floating type. A floating literal may adopt the other operand's floating type. Non-literal values need an explicit numeric cast or an integer `.to_float()` conversion.

**Operator traits.** Arithmetic and ordering operators may otherwise resolve through the corresponding `Add`, `Sub`, `Mul`, `Div`, `FloorDiv`, `Mod`, or `Ord` trait method. The builtin numeric and Duration rules take precedence over operator-trait dispatch. In Aura 0.3, builtin equality does not dispatch through an operator trait. Bitwise operations, shifts, and power are builtin numeric operations and do not dispatch through operator traits either.

**Integer power** requires a non-negative exponent. A negative exponent visible in source is `AU2003`. A negative value discovered only during execution is a runtime failure.

**Tuple equality.** Tuple `==` and `!=` require operands with the same static tuple type. They apply builtin equality recursively to corresponding element types and produce `bool`. Nested tuple elements follow the same rule. Both operands are read, not consumed. Runtime metadata attached to a tuple value adds no further type-compatibility or equality input.

When one equality operand is a tuple literal and the other has a known tuple type, the known type contextually types the literal, recursively. The rule is symmetric. Each equality link in a comparison chain applies this contextual typing before it enforces exact operand-type equality.

Tuple `<`, `<=`, `>`, and `>=` are rejected. Structural tuple types have no lexicographic ordering and cannot acquire one through `Ord`. See [Tuples](/manual/tuples).

**Union equality.** Union `==` and `!=` require the same normalized union on both sides, or one union operand and one operand that injects as a direct member. Every member must define equality. The result is `bool`, both operands are read, and no narrowing fact results. Ordering and arithmetic operators reject union operands with `AU2003`.

### Conditions

`if` and `while` conditions must have exactly type `bool`. So must the condition in `value if condition else alternative`. `and`, `or`, and `not` also require boolean results under the rules above. Aura does not convert strings, collections, resources, or user types to truthiness.

**Narrowing.** `place is None` and `place is not None` on a stable place of a union type establish complementary narrowing facts:

| Condition | Where it holds | Where it fails |
| --- | --- | --- |
| `place is None` | the place has effective type `None` | the place has the union of its remaining members |
| `place is not None` | the place has the union of its remaining members | the place has effective type `None` |

Facts follow control flow by these rules:

- Facts compose through `not`, parentheses, and short-circuit `and`/`or`. The right operand of `and`/`or` is checked under the left operand's fact.
- An `if` branch, `while` body, or conditional-expression arm is checked under the facts its condition selects.
- A branch that diverges with `return`, `break`, or `continue` contributes nothing to the join. Its complement's facts continue after the statement.
- At a join, a place keeps only the facts present on every reachable path.
- A `while` body is checked twice. A fact established before the loop survives only when no iteration can invalidate it.
- A fact ends when code assigns, moves, or mutably matches the place or an ancestor, or passes either to a call with mutable access. A later member use through the stale place is `AU2014`.

A single remaining member becomes the effective type of the place. A larger remaining set only sharpens `match` coverage. Equality with `None` does not narrow.

### Assertions

An `assert` condition must have exactly type `bool`. Its optional message must have exactly type `str`. Either mismatch is `AU2002`, reported at the `assert` keyword span.

The checker applies the condition's ownership effects first. It then checks the optional message from the resulting state. The message is evaluated lazily at runtime, so moves and mutations that happen only in the message do not carry into the fallthrough state. The statement itself has ordinary fallthrough and makes no lasting type or value refinement.

### Indexing, Slicing, And Members

**Direct indexing** supports three forms:

| Base | Index |
| --- | --- |
| `list[T]` | the `int64` index domain |
| `dict[K, V]` | exactly `K` |
| `Array[T]` | one `int64` coordinate per runtime axis |

List positions follow these rules:

- A negative index `i` is normalized once as `len + i`, before the bounds check. This applies to direct reads and writes, and to `get`, `set`, `pop`, and both `swap` indexes.
- Fixed-width `int8`, `int16`, `int32`, `uint8`, `uint16`, and `uint32` values widen losslessly only at these positions.
- Direct access, `set`, `pop`, and `swap` fail at runtime when the normalized position is invalid. `get` returns `None`.
- `insert` clamps its position to `0..=len`.

A direct read produces `T` or `V` only when that element or value type is copyable. For non-copy contents, use one of these forms instead:

- **List element.** Use `get(index)` for an explicit cloned optional read, only when the element type is clone-safe. Or bind the element in place with `view name = values[index]`.
- **Dictionary value.** Use `get(key)` only when the value type is clone-safe. Use `remove(key)` to transfer ownership, or take a `view` of the entry.
- **Field through an element.** `values[index].field` reads or assigns the field in place without copying the element.

These non-copy direct-read rejections use `AU3005`. A non-copy indexed compound assignment uses `AU3006`, because its initial read has the same ownership problem. A missing dictionary key in a direct read is runtime diagnostic `AU4003`. Integer indexing is not defined for `str`.

**Slicing.** A slice suffix is defined on `list[T]`, `str`, and `Array[T]`. It returns a fresh owned value of the source type.

- Each written endpoint uses the `int64` position domain, including lossless widening from the supported narrow fixed-width types.
- An omitted endpoint contributes no expression.
- Each negative written endpoint is normalized once as `len + i`. Then both effective endpoints must lie in `0..=len`, and start must not exceed end. Invalid or reversed bounds are runtime `AU4003`.
- String slicing counts Unicode scalar values and returns `str`.
- A slice is not a place. It cannot be the target of assignment or mutable access.
- Step syntax and slice assignment are reported as unsupported forms with `AU2005`.

List slicing creates a clone-producing obligation for `T`. Copy elements are copied, and non-Copy elements must be clone-safe. A concrete or transitive `random.Rng` element is rejected with `AU3007`. A non-repeatable Task observation right is rejected with `AU3009`. An unresolved generic `T` carries the inferred obligation to specialization.

**Arrays.** `Array[T]` is specialized only by `int32`, `int64`, `float32`, or `float64`.

- Its constructors require exact `list[int64]` shape metadata.
- Array slicing uses the same one-colon grammar and endpoint rules. It copies only a first-axis range and keeps the remaining runtime dimensions.
- Array/Array arithmetic requires identical `T`. Runtime shape equality is checked with `AU4007`.
- Scalar arithmetic requires exactly `T`, with no mixed promotion or broadcasting. Integer Array `/` is `AU2003`.
- Array `map[U]` requires exact repeatable `def(T) -> U` and restricts `U` to the four Array dtypes.
- `sum`, `min`, and `max` return `T`. `mean` returns `float64` for all dtypes.
- Mutable `set`, `fill`, and indexed assignment require a mutable Array place.
- Array `get` converts an invalid coordinate or a runtime-rank mismatch to `None`. `set` traps for either failure, and returns the replaced scalar in `Some` only after a valid update.

**Members.** Member access must resolve to a visible field, method, enum variant, module item, or maintained builtin member. Calling a receiver method also checks whether the receiver is consumed, shared-borrowed, or mutable-borrowed.

**Retained borrows.** A non-copy place selected as a binary left operand, index base, method receiver, or indexed-assignment target stays borrowed through the operation's later inputs.

- Another shared borrow is valid.
- An overlapping mutable borrow or consumption is rejected with `AU3002`. The diagnostic reports the retained selection as the borrow origin.
- The rule applies to name roots and to projected member places.
- The checker never makes the operation legal by assuming a deep clone.
- Equality and inequality keep this borrow through the right operand and consume neither operand. Tuple equality does not introduce a recursive move.

## Call Binding

Arguments are written as positional arguments followed by named arguments. Binding proceeds against the declaration or builtin metadata:

1. positional arguments fill parameters in declaration order
2. named arguments fill the parameter with the same name
3. a parameter cannot be filled twice
4. unknown names and extra arguments are rejected
5. omitted parameters require defaults
6. each argument type must equal the substituted parameter type

**Evaluation order.** A call evaluates its arguments in this order:

1. Each supplied argument expression is evaluated in call-site source order, and finishes before the next one begins.
2. A copy or move result is captured in its argument slot. A borrow-mode selection is established without cloning and is checked under the retained-borrow rule above. Later side effects cannot re-read or change an earlier captured argument.
3. Defaults for omitted parameters are then evaluated in declaration order, on every call where the parameter is omitted.

Binding a named argument to its declaration slot does not reorder evaluation. No default is evaluated for a supplied parameter.

**Defaults.** Defaults may refer only to names valid under the declaration's default-expression rules. They do not capture a caller's locals.

- A bare shared default's temporary lives through the call.
- An `own` default is consumed.
- A `mut` default is rejected, because mutations to its caller-invisible temporary would be silently lost.

**Parameter capabilities.** A bare parameter grants logical shared access for every type. The ABI may pass copy bits directly, but specialization never changes the declared capability. An `own` parameter consumes a non-copy argument. A `mut` parameter requires a mutable place. All arguments at one call boundary are checked together for overlapping move, shared, and mutable access.

**Indirect calls.** A call through a function value follows the same rules.

- When the value has one statically known declaration, binding uses that declaration's parameter names and defaults.
- A control-flow join keeps those names and defaults only when all candidate contracts agree on names and default availability. An omitted argument evaluates the runtime-selected target's own default expression.
- Conflicting reassignment, return through a structural function annotation, class-field storage, and mutable-collection storage erase the names and defaults. The call must then supply every parameter positionally.

The structural function type does keep each parameter's capability. Bare is shared. `mut` requires a mutable place and caller-visible writeback. `own` transfers a non-copy argument. Function-type assignment and substitution require those modes, the parameter types, and the return type to match.

**Extern calls** use ordinary positional and named argument binding and left-to-right evaluation. They then apply the declared FFI capabilities:

- Scalars require bare parameters.
- `str` is a bare const UTF-8 view.
- `list[uint8]` is a bare const byte view, or a `mut` fixed-length byte view. A `mut` view requires a mutable place.
- Opaque handles permit bare sharing or `own` consumption.

Extern defaults, generics, callbacks, variadics, returned views, and raw pointers are rejected.

**List methods with callbacks** use exact structural callback types:

| Method | Callback type |
| --- | --- |
| `map` | `f: def(T) -> U` |
| `filter` | `f: def(T) -> bool` |
| keyed `sort` | `key: def(T) -> K` |

Bare callback parameters are shared capabilities. A callback with `mut T` or `own T` is not substitutable. `filter` is clone-producing and adds the ordinary clone-safety obligation for `T`. `sort` requires `T` to support the existing natural `<` ordering, and keyed `sort` requires that ordering for `K`. Both forms of `sort` require a mutable list place. `map` and `filter` keep a shared receiver.

**`control.retry`** requires `worker: def() -> Result[T, E]`, `max_attempts: int32`, and `initial_backoff: Duration`. The worker function type must match exactly, including its empty parameter list and `Result` return identity. `T` and `E` are inferred from that return specialization, or supplied through ordinary explicit generic specialization. The callback is not widened from a function with parameters or with a different return type.

## Class Construction

Calling a class name constructs a value. Constructor fields may be supplied positionally in declaration order, then by name. Positional arguments cannot follow a named argument.

Every field without a declaration default must be supplied exactly once. Provided and default values must match the substituted field types. Supplied field expressions follow the same source-order capture rule as call arguments. A later field expression cannot change an earlier captured field value.

A class receiver is declared as shared `self`, consuming `own self`, or mutable `mut self`. A first method parameter written `self: Type` is rejected with guidance naming those forms. A method without a receiver is associated. It is called through the class or type rather than an instance.

## Enum Construction And Matching

An enum variant constructor must name an existing variant and provide exactly its payload shape. A variant declares either all positional payloads or all named payloads, and constructors bind accordingly.

Named payload expressions evaluate in their written source order, and each result is captured. The results then bind by payload name to the variant's declaration-order slots. Declaration-slot binding does not reorder evaluation.

A generic enum constructor needs enough context to determine all type arguments. The context may come from explicit specialization, an expected annotation, parameter, or return type, or payload inference. Bare builtin variants such as `Some`, `Ok`, `Err`, or `None` are accepted only where the expected enum identity is unambiguous.

**Match patterns.** A pattern must be compatible with the scrutinee type.

- Variant payload subpatterns must have exactly the variant's arity.
- Literal patterns must match the scrutinee's supported scalar type.
- Duplicate or unreachable arms are rejected where the checker can establish overlap.
- Matches over enums and booleans must be exhaustive unless `_` covers the remainder.
- Literal matches over open numeric or string domains require `_`.
- Every arm of a match expression must produce the same result type, using the surrounding expected type where available.

**Conditional expressions.** Both value arms are checked against one result type. Surrounding expected context applies to both arms. Without such context, a context-dependent literal may adopt the type established by the other arm. No rule widens or converts an already-bound value. The condition is checked first, then each arm starts from the resulting ownership state.

## Generics, Traits, And Implementations

Traits are nominal interfaces. A bound `T: A + B` requires an applicable implementation of each trait after substitution. Supertraits are inherited requirements.

An `impl` identifies one trait specialization and one target type pattern. Its methods must correspond to trait methods. A missing required method is rejected unless the trait provides a default body. Extra methods are not part of that trait implementation.

**Implementation selection.** For a concrete receiver, the checker chooses the unique applicable implementation with the greatest specificity. If several equally specific implementations apply, the call or operator is ambiguous and rejected. Source order is not a tie breaker.

For a type parameter, available methods and operators come from its declared bounds. If several bounds expose an indistinguishable method, the access is ambiguous unless the language can resolve one unique contract.

**Union receivers.** A union receiver has a trait method when every member resolves that method through the same trait specialization, with one contract after substituting the member for `Self`. One contract means the same receiver mode, parameter modes and types, and result type.

- The call dispatches on the active member.
- A member without an implementation, or with a differing contract, is rejected with `AU2999`.
- A mutable call locks the union's tag for the call.
- A consuming call consumes the whole union.
- `.clone()` is available when every member is Copy or clones itself.

**Type parameters as union members.** A type parameter may be a union member. Unifying such a union with an argument requires every concrete member of the pattern to be a member of the argument. The parameter binds to the normalized remainder. An empty remainder, or a remainder that could belong to more than one parameter, requires explicit specialization (`AU2010`).

Substituted unions are renormalized. A generic body is checked once under its symbolic members:

- `is None` on `V | None` refines to `None` and to `V`.
- `is None` on a bare `V` is a runtime test.
- `case V` is rejected with `AU2013`.

**Method defaults.** Trait and implementation methods cannot declare default ordinary parameters in Aura 0.3. Trait default method bodies are permitted. A signature-only trait method has no body after its terminating newline.

**Clone-safety obligations.** A clone-producing operation over unresolved generic types infers clone-safety obligations on the declared parameters that contribute to it. Calls propagate those obligations to a fixed point and discharge them after substitution. The contract applies equally to ordinary, imported, inherent, associated, bounded trait, operator, task-target, and `From` calls. A concrete type that contains non-cloneable `random.Rng` state, or whose safety cannot be proved, is rejected with `AU3007`.

An obligation inferred from a trait default method is part of that method's contract. It is structurally substituted through `Self`, trait arguments, and method arguments. An explicit implementation may satisfy that contract but MUST NOT add a clone-safety requirement absent from it. Recursive nominal type inspection terminates conservatively rather than assuming an expanding cycle is safe.

## Control Flow

`return` is valid only in a function or method. Its ordinary value must equal the declared return type. An omitted value has type `None`. `return view` is valid only under a matching declared view return, and must preserve the exact receiver or parameter provenance root.

`break` and `continue` are valid only inside `for` or `while`. A loop-local binding does not escape. Moving a non-copy outer value for the first time inside a repeatable loop is rejected, unless the checker can prove the path does not create an invalid next iteration.

A comprehension is expression control flow, not a statement loop:

- `break`, `continue`, and `return` cannot appear in its clauses or output.
- Each filter checks one conditional path. Later clauses and output effects apply only on the path where every preceding filter is true.
- The resulting ownership state is merged conservatively.
- `try` is still an expression. It may propagate from a reached source, filter, key, value, or element, after cleaning up the partial result.

An `if`, statement match, match expression, or conditional expression checks its branches independently. It merges move and partial-move state conservatively across reachable paths. A non-`None` function is rejected when any reachable path can fall through without returning.

## `with` Resources

`with` consumes its resource expression and creates a mutable managed binding for the body. Supported builtin resources have runtime-defined cleanup. A non-generic user class may be used only when it declares exactly a `close(mut self) -> None` instance method.

The managed binding cannot be moved out in a way that would prevent required cleanup. Cleanup runs when the scope ends normally, by return, by loop control, or through a runtime failure. [Execution Model](/manual/execution-model) describes the cleanup behavior.

## Tasks And Static Safety

**Task targets.** `TaskGroup.start`, `start_soon`, `start_with_stack`, and `start_soon_with_stack` accept these targets:

- a direct named function
- an associated method without `self`
- a capture-free function value
- a closure value

The explicit-stack methods first require an exact `int64` capacity.

Target arguments are copied or moved into task-owned capture storage, independent of the target ABI. Bare shared target parameters borrow that storage for the child call. `own` parameters consume it. `mut` target parameters are rejected. Generic targets also enforce their inferred clone-safety obligations after specialization.

Task-target resolution accepts a concrete function value. Explicit `function[Types]` specialization may produce such a value before the call. The direct associated-method spelling `Type.associated_method[Types]` is limited to the callable-target slot. A bare target is also concrete when its declared or default context already resolves its complete types.

**Transfer.** Each value captured by these four start methods, and the specialized target return type, must be `Transfer`. Transfer is a structural property the compiler derives. It is not a builtin user trait, and a same-named ordinary user trait cannot confer it. The compiler follows collection, tuple, class, and enum storage to the first non-transferable leaf.

| Type | Transfer |
| --- | --- |
| all copy types and `str` | yes |
| aggregates | when every stored component is |
| `Queue[T]` and `Task[T]` handles | yes, without traversing `T` |
| capability views, `random.Rng`, `TaskGroup`, live host resources | no |

A task-start expression that reads a Copy value through shared or mutable access captures an owned Copy snapshot, not the access capability. It is allowed when that value type is Transfer. Non-copy access cannot be captured by value without ownership, and the capability itself never crosses.

A by-value closure target is Transfer exactly when every stored capture is Transfer. The complete closure value is moved or copied into task-owned storage before the child calls it. Capture-free lambdas are Copy and Transfer. A closure with any shared or mutable loan capture is non-Transfer and is rejected with `AU3008`.

Queue construction, `put`, and `try_put` require the payload `T` to be `Transfer`. Operations that use only the handle, such as receive, fallback, and close, do not recheck it. A fully concrete generic specialization is checked structurally. An unresolved type parameter at a task or Queue boundary is rejected conservatively. The compiler does not infer a deferred Transfer contract.

A Transfer rejection uses `AU3008`. It identifies the task or Queue boundary and gives the nested component path that caused it, such as a field that contains `fs.File`. It does not suggest implementing a `Transfer` trait.

**Task results.** Task results are either repeatable or single-consumer. `Task[T]` is copyable only when `T` is copyable, `T` is `Queue[...]`, or `T` is a recursively repeatable `Task[...]`. For any other transferable `T`, each observation consumes the task's unique observation right:

- `result`, `poll`, and `result_or` consume the right on every outcome.
- `wait_any` and `wait_all` consume the complete task list. `wait_any` abandons the unchosen rights.
- `select(...)` consumes every non-repeatable Task source at call entry and abandons each losing right.

This prevents handle aliases, including nested `Task[Task[str]]`, from producing a second value. The related diagnostics are:

| Code | Cause |
| --- | --- |
| `AU3009` | cloning, reading through a clone-producing collection method, or copying an aggregate that contains such a right |
| `AU3001` | reusing the task binding after a consuming observation, the ordinary moved-value error |
| `AU3002` | consuming through shared access |

[Concurrency](/manual/concurrency) defines task, queue, and cancellation runtime semantics.

Design records: [ADR-0008](https://github.com/johnolafenwa/Aura/blob/main/architecture_docs/decisions/0008-task-result-ownership.md) and [ADR-0033](https://github.com/johnolafenwa/Aura/blob/main/architecture_docs/decisions/0033-structural-transfer-and-task-results.md).

## Entrypoint Rules

The selected entry module may use one of two shapes:

- executable top-level statements and no local `main`
- a local `main` and no executable top-level statements

The local `main` takes no parameters and returns `None` or `int32`. An imported function named `main` is an ordinary imported function and does not become the entrypoint.
