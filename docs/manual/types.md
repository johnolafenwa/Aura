# Types

This page covers Aura's type system: scalars, unions, copy and move categories, builtin and user types, annotations, and casts. Aura is statically typed, so every expression has a type. Type annotations are part of the public shape of functions, fields, methods, and many empty literals.

The type system keeps three facts visible:

- what kind of value a program has
- whether the value is copied or moved
- whether failure is represented in the return type

## Scalar Types

This section covers transparent aliases and unions first, then the builtin scalar types.

### Aliases And Unions

A transparent alias uses `type Name = Type`. It may take generic parameters and `public` visibility. For example, `type ToolValue = int64 | str | None` names a union of three existing types. An alias adds no runtime wrapper and no new nominal identity.

Union identity is normalized:

- member order does not matter
- nested unions flatten
- duplicate members are removed
- a single member collapses to that member's type

`None` is the unit member. A declared type parameter may be a member, and substitution renormalizes the result. So `V | None` with `V = int64 | None` is `int64 | None`. See [type parameters as union members](/manual/generics-and-traits#type-parameters-as-union-members).

**Entering a union.** A value enters a union at an explicit expected-type boundary, such as an annotated binding or a declared parameter or result. The value's type must be a direct member. A contextual numeric literal must have exactly one eligible member. For an ambiguous literal, use a typed spelling or an intermediate binding.

There is no implicit conversion between unrelated unions, or between containers with different element types.

**Leaving a union.** Select a member with the [type patterns](/manual/enums-and-match#union-type-patterns) described in the match chapter. Or test the `None` member with `is None` and `is not None`. Those tests [narrow](/manual/enums-and-match#conditional-narrowing) a stable place to its remaining members for the selected branch. See [Optional And Result Types](#optional-and-result-types) for the optional form `T | None`.

### Builtin Scalars

| Type | Description |
| --- | --- |
| `bool` | Boolean value: `true` or `false`. |
| `int` | Alias for `int64`. It is not a distinct type. |
| `int8`, `int16`, `int32`, `int64`, `int128`, `intsize` | Signed integers. |
| `uint8`, `uint16`, `uint32`, `uint64`, `uint128`, `uintsize` | Unsigned integers. |
| `float32`, `float64` | Floating-point values. |
| `str` | Owned UTF-8 string. `len()` counts Unicode scalar values and `byte_len()` counts encoded bytes. |
| `None` | Unit type and unit value. |
| `Duration` | Signed 128-bit nanosecond duration used by arithmetic, sleeps, timeouts, and scheduling APIs. |
| `Range` | Integer range returned by `range(...)`. |

### Integers

Integer bounds are exact:

| Type | Inclusive range |
| --- | --- |
| `int8` | -128 through 127 |
| `int16` | -32,768 through 32,767 |
| `int32` | -2,147,483,648 through 2,147,483,647 |
| `int64` | -9,223,372,036,854,775,808 through 9,223,372,036,854,775,807 |
| `int128` | -2^127 through 2^127 - 1 |
| `uint8` | 0 through 255 |
| `uint16` | 0 through 65,535 |
| `uint32` | 0 through 4,294,967,295 |
| `uint64` | 0 through 18,446,744,073,709,551,615 |
| `uint128` | 0 through 2^128 - 1 |
| `intsize` | host-pointer-width signed range |
| `uintsize` | host-pointer-width unsigned range |

`int` and `int64` have identical bounds, type identity, layout, and runtime behavior.

**Literals.** An integer literal may be decimal, hexadecimal with `0x`, binary with `0b`, or octal with `0o`. Underscores may appear between digits. Every spelling follows the same contextual typing and bounds rules. An unsuffixed integer literal takes its type from the first rule that applies:

1. an expected integer type, when one is available
2. an expected `float32` or `float64`, when the value is exactly representable in that target
3. otherwise, `int64`

The second rule is literal typing. It is not a conversion that integer variables can use.

The checker checks every numeric literal against its target type. An integer literal must fit an annotated integer target. A float-context integer literal must be exactly representable in its `float32` or `float64` target. For an inexact value, make the rounding explicit with a floating spelling or `.to_float()`. Every integer type provides `.to_float() -> float64`. It permits IEEE-754 round-to-nearest, ties-to-even conversion, for when an application wants to enter the floating domain.

**Fixed-width APIs.** The `int64` default does not widen explicitly typed APIs. Fixed `int32` contracts stay `int32`. These include `main()` exit statuses, queue capacities, and bounded process and network I/O byte-count parameters.

**Positions.** Position APIs are the one exception, and they use `int64`:

- range bounds and yields
- collection indices
- slice endpoints
- enumeration positions
- Array coordinates

At those positions, values of type `int8`, `int16`, `int32`, `uint8`, `uint16`, or `uint32` widen losslessly. This conversion is unavailable in ordinary assignments, arguments, operators, and returns.

**Lengths.** Length results are also `int64`, so they compose directly with ranges and indices. The builtin `len`, `str.len`, `str.byte_len`, `list.len`, `dict.len`, and `set.len` all return `int64`.

`random.secure_bytes(n)` is a separate byte-count API. `n` is `int64`, with a fixed per-request resource and safety ceiling of `2147483647`.

### Floating Point

`float32` and `float64` use the IEEE-754 binary32 and binary64 representations.

Literal lexing first requires a finite binary64 value. Contextual `float32` conversion may then round or overflow, as recorded in [Current Limits](/manual/current-limits).

Runtime operations may produce NaN. In Aura 0.3, `/`, `//`, or `%` by a floating zero is an explicit runtime failure. Those operators never produce infinity or NaN.

### Duration And Range

`Duration` stores a signed 128-bit count of nanoseconds. Literal units normalize exactly to nanoseconds. Literals are non-negative, but associated constructors and arithmetic can produce negative values. Whether a value is representable in the language is separate from whether it is valid as a host wait or deadline.

The associated constructors `Duration.ms(int64)`, `Duration.seconds(int64)`, and `Duration.minutes(int64)` accept signed counts. Duration values support:

- checked addition and subtraction with another Duration
- multiplication by `int64`, in either operand order
- floor division by `int64`
- full value-based comparison

`to_ms()` and `to_seconds()` convert the exact rational unit value to the nearest representable IEEE-754 binary64 value, ties-to-even. They may round.

`Range` contains `int64` start and end values. It iterates from the start, inclusive, to the end, exclusive.

### Strings

`str` owns its UTF-8 storage. Aura has no separate slice layout and no lifetime-bearing text-view type.

A bare `value: str` parameter grants shared access. Bare parameters grant shared access for copy and move types alike. An implementation may pass copy bits directly without changing that source contract.

| Operation | Behavior |
| --- | --- |
| `str.len() -> int64` | Scans the text and counts Unicode scalar values in O(n). |
| `str.byte_len() -> int64` | Reads the UTF-8 byte count in O(1). |
| `str.to_bytes() -> list[uint8]` | Strict UTF-8 boundary out of text. |
| `str.from_bytes(list[uint8]) -> Result[str, bytes.Error]` | Strict UTF-8 boundary into text. |

`list[uint8]` is Aura's bytes representation.

Aura has no distinct character type, no integer str indexing, and no `chars()`, `ord()`, or `chr()`. String slicing accepts `int64` scalar endpoints and runs in O(n) over the source. It returns a fresh owned str. It is not a view or a byte-indexing operation.

## Copy And Move Categories

A Copy value stays usable after you assign it or pass it through a value or `own` position. These are Copy:

- numbers
- `bool`
- `Duration`
- `Queue[T]`
- `Task[T]`, only when `T` is repeatable as defined in [Provisional Transfer Classification](#provisional-transfer-classification)
- tuple values when every element type is copyable
- `copy class` values whose fields are all copyable
- user enum values when every declared payload type is statically copyable
- `Lookup[T]`, `Poll[T]`, `Result[T, E]`, `SendError[T]`, and `QueueReceive[T]` when all payload types are copyable
- union values when every member type is copyable

A move value transfers ownership. These are move values:

- tuple values with at least one move element
- union values with at least one move member
- `str`
- `list[T]`
- `dict[K, V]`
- `set[T]`
- `random.Rng`
- ordinary user classes
- user enum values with any move payload
- `json.Value` and `json.Error`
- `Lookup`, `Poll`, `Result`, and related outcome values with move payloads
- `TaskGroup`
- file, process, supervisor, and network resources
- opaque FFI handles declared by `extern "C" opaque class`. FFI is the foreign function interface.

You can still share a move value through a bare parameter or access it mutably through a `mut` parameter. When the type supports cloning, you can duplicate it explicitly with a method such as `.clone()`.

**Always move.** `TaskResult[T]`, `SelectOutcome[Q, T]`, `WaitAny[T]`, and `WaitAll[T]` are move outcome values, even when every payload type is copyable. `Range` is also not a general copy type in Aura 0.3. Use a range directly in iteration instead of relying on duplication.

A generic user-enum payload whose declared type is an unconstrained type parameter is not assumed copyable. This holds even when one later instantiation supplies a copy type.

**Handles.** `Queue[T]` is a copy handle to shared runtime state. A `Task[T]` handle is copyable only under a condition, so aliases cannot duplicate a single-consumer result right. Copying an allowed handle never copies queued values or task results. It gives another reference to the same queue or task.

**Slicing.** Slicing `str` produces a fresh owned str. Slicing `list[T]` produces a fresh owned list and is clone-producing for `T`:

- Copy elements are copied.
- Non-Copy elements must be clone-safe.
- `random.Rng` state is rejected with `AU3007`.
- Non-repeatable Task observation rights are rejected with `AU3009`.

The result is another move value, independent of the source.

**Clone safety.** Copy and move classification is distinct from clone safety. `random.Rng` is more than a move type: it exposes no public duplication route. A clone-producing operation is valid only when its produced type cannot contain an `Rng` through an ordinary value-storing class, enum, or collection path. `Task[T]` and `Queue[T]` stop that traversal, because copying either handle does not observe or copy its stored `T`. Moving, removing, or receiving a value also transfers one owner instead of cloning it.

## Tuple Types

`(T1, T2)` is a fixed two-element structural tuple type. `(T,)` is a fixed singleton tuple type. Tuple arity and the corresponding element types are part of type identity. A tuple type may appear anywhere a complete type reference is accepted, including parameter, field, payload, local annotation, and return positions.

A tuple is copyable if and only if every element is copyable. The classification is recursive through nested tuples. Any other tuple is a move value. Unpacking a move tuple consumes the source as one whole value. It does not expose positional partial moves that stay independently reusable.

You can compare two tuple values with `==` or `!=` only when they have the same static tuple type. The comparison is recursive over corresponding element values. It reads both operands and consumes neither, whatever their copy classification. Runtime metadata carried with a tuple value is not part of value equality. Tuple ordering is not defined.

Aura has no empty tuple type and does not convert tuples to or from collections. See [Tuples](/manual/tuples) for construction, unpacking, patterns, indexing, and the exact current boundary.

## Provisional Transfer Classification

`Transfer` is the static property checked at a task boundary. It means ownership of a value may cross from one Aura task worker to another. It is separate from both Copy and clone safety.

The compiler derives `Transfer`. It is not a builtin trait, and source code cannot implement or assert it. An ordinary user trait also named `Transfer` does not affect this structural classification.

**Transfer types.**

- All copy types and `str` are `Transfer`.
- `list[T]`, `set[T]`, `dict[K, V]`, tuples, classes, and enums are `Transfer` exactly when all their stored component types are.
- The same recursive rule covers unions and data wrappers such as `Lookup`, `Poll`, `Result`, task and queue outcomes, errors, and `json.Value`.
- A union is Transfer when every member is Transfer. For a member that is not, `AU3008` names the failing member.
- `Queue[T]` and `Task[T]` handles are `Transfer` whatever `T` is. Moving the handle does not inspect or move the stored payload.

Queue construction, `put`, and `try_put` separately require the payload `T` to be `Transfer`. Handle copies, receives, fallback receives, and `close` do not recheck `T`.

**Not Transfer.**

- shared and mutable capability views
- `random.Rng`
- `TaskGroup`
- live filesystem, process, pipe, supervisor, listener, socket, stream, HTTP-exchange, WebSocket, or TLS resources

An individual host type can join the Transfer set only through a later design decision that proves its thread safety.

Owned data returned from a host operation is classified by the data it stores, not by where it came from. Examples are completed output and structural error values. `process.Completed`, `net.HttpResponse`, and `net.UdpDatagram` are explicitly Transfer owned snapshots. Their live sources, `process.Child`, `net.HttpExchange`, and `net.UdpSocket`, are not.

**Copy snapshots.** Reading a Copy value through shared or mutable access produces an independent owned snapshot. It does not transport the capability. That snapshot may cross when its type is `Transfer`. Non-copy access cannot use this exception, because value capture would require ownership.

**Generics.** An unconstrained generic parameter does not prove `Transfer`. The compiler does not infer a deferred Transfer contract. It rejects a task or Queue boundary with an unresolved parameter with `AU3008`.

A generic task target is usable when call inference has already produced complete concrete capture and result types. A task target may spell explicit specialization narrowly as `function[Types]` or `Type.associated_method[Types]`. Outside a TaskGroup start target, brackets keep their ordinary indexing meaning. A bare target is valid when its declared or default context already makes every relevant type concrete.

**Task copies.** `Task[T]` is always `Transfer`, but its Copy classification is conditional. It is copyable only when one of these holds:

- `T` is copyable
- `T` is `Queue[...]`
- `T` is `Task[U]` and `U` is recursively repeatable

This rule stops a nested handle such as `Task[Task[str]]` from being copied to duplicate a single-consumer result right.

**Runtime.** The pinned-worker runtime uses this classification at its task boundary. Queue and Task handle state is synchronized for cross-worker use. All other task captures and results stay owned, structural `Transfer` values. So the boundary stays share-nothing even when sibling task bodies run on different pinned workers.

## Builtin Generic Types

Ordinary absence is the union `T | None`, not a generic wrapper type. The builtin generic enums below cover outcomes that must stay distinct from a present `None` payload.

| Type | Meaning |
| --- | --- |
| `Lookup[T]` | `Found(T)` or `Missing`. The outcome of `list.get`, `dict.get`, and `dict.remove`. |
| `Poll[T]` | `Ready(T)` or `Unavailable`. The outcome of `Queue.poll` and `Task.poll`. |
| `Result[T, E]` | `Ok(T)` or `Err(E)`. Use it for recoverable failure. |
| `list[T]` | Owned ordered collection. |
| `dict[K, V]` | Owned key/value dictionary. |
| `set[T]` | Owned set of unique values. |
| `Array[T]` | Owned contiguous row-major numeric array. `T` is exactly `int32`, `int64`, `float32`, or `float64`. |
| `Queue[T]` | Scheduler-aware typed queue handle. |
| `Task[T]` | Transferable task-result handle. It is Copy only under the conditions in [Provisional Transfer Classification](#provisional-transfer-classification). |
| `SendError[T]` | Queue send failure that carries the unsent value. |
| `QueueReceive[T]` | Queue receive outcome. |
| `TaskResult[T]` | Task result outcome. |
| `SelectOutcome[Q, T]` | Typed `select(...)` outcome for Queue payload `Q` and Task result `T`. An absent source category uses `None`. |
| `WaitAny[T]` | `wait_any(...)` outcome. |
| `WaitAll[T]` | `wait_all(...)` outcome. |

`Array[T]` carries runtime rank and `list[int64]` shape metadata, not shape-level static type arguments. Every Array has rank at least one, may contain zero-length dimensions, and owns its contiguous CPU buffer. It is non-Copy, explicitly cloneable, and structurally `Transfer`. A Task result that contains an Array keeps the ordinary single-consumer observation right. See [Numeric Arrays](/manual/numeric-arrays).

## Resource And Module Types

Builtin modules provide these types. Their names are reserved.

| Module | Types |
| --- | --- |
| `io` | `io.Error` |
| `fs` | `fs.File` |
| `json` | `json.Value`, `json.Error` |
| `random` | `random.Rng` |
| `net` | `net.TcpListener`, `net.TcpStream`, `net.UdpSocket`, `net.UdpDatagram`, `net.HttpListener`, `net.HttpExchange`, `net.HttpResponse`, `net.WebSocketListener`, `net.WebSocket`, `net.UnixListener`, `net.UnixStream`, `net.TlsListener`, `net.TlsStream` |
| `process` | `process.Child`, `process.Pipe`, `process.Completed`, `process.Supervisor`, `process.ExitStatus`, `process.Wait`, `process.Stdio`, `process.Error`, `process.RestartPolicy`, `process.SupervisorEvent`, `process.SupervisorWait` |

Scope a resource with `with` or close it explicitly.

`random.Rng` is an opaque move type, not a resource. It has mutable state but no `close()` operation and no `with` contract. [Randomness Module](/manual/randomness) gives its complete type and sequence rules.

`json.Value` is a move type. Its recursive variants represent Null, Boolean, `int64`, finite `float64`, str, list, and dict object data. `json.Error` is a move type because its Syntax variant owns a str. [JSON Module](/manual/json) gives their exact variants and number rules.

## Type Annotations

Simple annotations:

```aura
count: int32 = 0
name: str = "aura"
```

Collection annotations:

```aura
names: list[str] = []
lookup: dict[str, int32] = {}
seen = set[int32]()
```

An empty collection literal needs an expected type. Constructors work too:

```aura
names = list[str]()
lookup = dict[str, int32]()
seen = set[int32]()
```

An optional annotation injects a bare value or `None` at the annotated destination:

```aura
name: str | None = None
label: str | None = "name"
```

Type arguments are invariant. When brackets are present they must be nonempty and exactly match the declared arity. Aura does not implicitly convert `list[int32]` to `list[int64]`. It does not treat structurally identical user classes as the same type.

## Optional And Result Types

An optional value is a union whose last member is `None`. `T | None` is the only optional spelling. There is no builtin `Option` type, no `Some` constructor, and no `T?` suffix.

A value enters the union at a typed destination, and `None` selects its absence member there. Construct a `Result` with its enum name:

```aura
maybe: str | None = "name"
missing: str | None = None

result: Result[int32, str] = Result.Ok(42)
failure: Result[int32, str] = Result.Err("bad number")
```

Bare `None` on its own is the unit value and renders as `None`. It becomes an optional only where a `T | None` annotation, parameter, result, or other expected type applies. An unannotated `count = 5` is a plain `int64`. To keep the optional type, write `count: int64 | None = 5`. A function declared `-> T | None` returns absence with an explicit `return None`.

Test presence with `is None` and `is not None`. These [narrow](/manual/enums-and-match#conditional-narrowing) a stable place to its remaining members for the selected branch. Or select a member with a [type pattern](/manual/enums-and-match#union-type-patterns):

```aura
def describe(value: str | None):
    if value is None:
        print("missing")
        return
    print(value.len())

def label(value: int64 | None) -> str:
    match value:
        case int64 as number:
            return f"{number}"
        case None:
            return "missing"
```

`value == None` and `value != None` are ordinary comparisons for a `T | None` operand. They do not narrow. Unit `None == None` is `true` and unit `None != None` is `false`.

When an operation must keep a missing entry distinct from a present `None` payload, it returns `Lookup[T]` or `Poll[T]` from [Builtin Generic Types](#builtin-generic-types). See [Collections](/manual/collections) and [Concurrency](/manual/concurrency).

A match may use qualified or short-form variants when the type is known:

```aura
result: Result[int32, str] = Result.Ok(42)
match result:
    case Result.Ok(value):
        print(value)
    case Result.Err(message):
        print(message)
```

## User Types

Classes create product types:

```aura
class Point:
    x: float64
    y: float64
```

Enums create sum types:

```aura
enum Load[T]:
    Ready(value: T)
    Empty
    Failed(message: str)
```

Traits define shared behavior:

```aura
trait Named:
    def name(self) -> str
```

## Recursive Fields

Direct recursive fields are not implemented. Mark a recursive class field `indirect`:

```aura
class Node:
    value: int32
    next: indirect Node | None = None
```

`indirect` gives the field a level of indirection, so the value has a finite size.

## Casts

A numeric cast uses `value as NumericType`. Non-numeric casts are not implemented.

- An integer-to-integer cast requires the value to fit the target bounds.
- An integer-to-float cast requires exact representability and rejects silent precision loss. When a possibly rounded `float64` result is intended, use integer `.to_float()`.
- A float-to-integer cast requires a finite in-range value and truncates toward zero.
- `float64` to `float32` rounds through the host `float32` representation.
- `float32` to `float64` preserves the represented value.

A cast is checked at runtime when the source value is not a compile-time literal. A failed cast is a runtime diagnostic, not `Result.Err`.

To convert text to a number, use a parsing function:

```aura
def parse_answer() -> Result[int32, str]:
    value = try parse_int32("42")
    return Result.Ok(value)
```

## Grammar

[Grammar](/manual/grammar) collects the type syntax. A type is an identifier or module-qualified type path with optional bracketed type arguments. In class-field position it may carry `indirect`.

The `mut` and `own` parameter modifiers are not type constructors. Bare, `mut`, and `own` parameter capabilities govern access at a call boundary. Every `-> T` annotation describes an owned result.

## Typing Rules

- Every expression has one static type.
- An annotation, parameter, return, field, collection element, or expected enum context may type a compatible literal. Otherwise integer literals default to `int64` and floating literals to `float64`.
- Unqualified `int` is exactly `int64`.
- Non-literal values never widen implicitly.
- Generic arity, substitutions, bounds, field recursion, optional desugaring, cast legality, and exact assignment equality are checked before execution.

## Runtime Semantics

Copy scalars and declared copy aggregates are represented by value. Other values use the maintained owned runtime representations documented on their feature pages.

Arithmetic and casts are checked and may trap with a runtime diagnostic. Typed library absence or failure stays a `T | None`, `Lookup`, `Poll`, or `Result` value.

`indirect` inserts the maintained runtime indirection needed to construct a recursive field.

## Ownership And Evaluation Order

The static type decides whether reading an owned place copies it or moves it. A copy declaration is valid only when every stored field or payload is copy.

Borrowing and parameter passing do not change the underlying type. Aura inserts no hidden cloning and no runtime coercion. Type annotations are erased after checking and add no evaluation step.

Generic clone-producing uses infer clone-safety obligations, which are checked after specialization. This does not change the underlying copy or move category.

## Diagnostics

Compile-time codes:

| Code | Cause |
| --- | --- |
| `AU1101` | Malformed type, type-argument, or annotation syntax. |
| `AU2001` | An unknown or unavailable type name. |
| `AU2002` | A type mismatch, unresolved contextual literal typing, or a generic arity, payload, field, or annotation mismatch. |
| `AU2003` | An unsupported numeric operator or cast. |
| `AU2004` | Invalid constructor argument binding. |
| `AU2010` | A value that is not a direct member of its expected union. |
| `AU2011` | An ambiguous contextual literal. |
| `AU2012` | A cyclic transparent alias. |
| `AU2014` | A member use whose `None`-test narrowing was invalidated. |
| `AU2999` | An invalid recursive layout, or another type rejection with no narrower category. |
| `AU3001` | Use of a moved non-copy value, including an already-consumed task binding. |
| `AU3002` | A borrow conflict. |
| `AU3003` | Mutation through an immutable place. |
| `AU3004` | An invalid ownership or receiver type mode. |
| `AU3005` | A non-copy indexed read. |
| `AU3006` | A non-copy indexed compound assignment. |
| `AU3007` | An operation or specialization that would duplicate non-cloneable state, such as `random.Rng`, an opaque FFI handle, or a capturing closure environment. |
| `AU3008` | A non-Transfer task or Queue boundary. |
| `AU3009` | Cloning, a collection read, or an aggregate copy that would duplicate a single-consumer task-result right. |

Runtime codes:

| Code | Cause |
| --- | --- |
| `AU4001` | A general checked trap. |
| `AU4002` | Numeric overflow, underflow, range, or exactness failure. |
| `AU4003` | A bounds or lookup violation. |
| `AU4004` | A zero divisor. |
| `AU4005` | A trapping resource or I/O failure. |

## Backend Support

The checker produces one canonical type model. MIR lowering, compiler-backed analysis, and direct native code generation all use it. MIR is the compiler's mid-level intermediate representation.

Every type this page documents as implemented is supported by both maintained execution paths. The backend parity gate keeps out any backend surface that cannot preserve the same behavior.

## Limits And Implementation-Defined Behavior

**Casts and recursion.** User-defined numeric casts and non-numeric casts are unavailable. Recursive value fields require `indirect`.

**Method values.** Method values have no separate type.

- `receiver.method` is a bound method: a closure over the receiver with the method's complete contract.
- `Class.method` on a non-generic class is a capture-free function value.
- A generic method's type arguments must be explicit or supplied by an expected contract.
- A `view ... from self` result cannot be bound.

See [Closures](/manual/closures#bound-methods).

**Function types.** Capture-free named function values use `def(T1, mut T2, own T3) -> R`. Bare parameters are shared, and the written `mut` and `own` modes are part of the type. Contextually typed lambdas use the same source-level callable signature. A capturing closure also owns its hidden environment.

Arbitrary stored and parameter `def` types describe capture-free code pointers. Compiler-known callback and task-start sites preserve the additional closure metadata.

**Owned callables.** `Callable[def(...) -> R]`, `Callable[mut def(...) -> R]`, `Callable[own def(...) -> R]`, and `TaskCallable[...]` are owned storage types. They hold a Shared, Mutable, or Consuming callable with an erased capture set. They are non-Copy and non-cloneable. Only `TaskCallable` is Transfer.

**Numeric widths.** `intsize` and `uintsize` follow the target pointer width. Host process exit transport may narrow an `int32` after Aura returns it. Other numeric widths and overflow behavior are language-defined, not implementation-defined.

**FFI.** FFI v0 opaque handles are nominal wrappers for one non-null foreign pointer. They are non-Copy, non-cloneable, and non-Transfer. Extern functions are direct-call-only declarations, not `def(...) -> ...` values.

**Union layout.** Every normalized union has one explicit-tag layout plan, shared by both execution paths:

- Logical tags are the dense canonical member ordinals.
- The tag is the smallest unsigned width that holds every member. One byte covers up to 256 members.
- The payload is aligned to the widest member alignment and sized to the widest member.
- The total size rounds up to the aggregate alignment.
- Scalars use their width, and unit `None` is empty.
- Every boxed aggregate, handle, callable, or type parameter occupies one pointer.

The plan is an internal, target-dependent Aura ABI. ABI means application binary interface. It is not a C ABI or a portable serialization. The lowered program encodes the plan together with each member's drop obligation. So importers and native caches rebuild the plan instead of guessing a tag from source order. Only the active payload owns cleanup, and it is destroyed once.

## Status

These types, as this Manual describes them, are implemented: scalar, collection, enum, class, trait-bound, resource, optional, result, and indirect types. Structural tuple types and tuple equality are implemented.

Ordinary `-> T` return values are owned. An explicit `-> view [mut] T from origin` declaration returns non-owning access instead. A view descriptor is not an owned or structural `def(...) -> R` type, and it cannot be stored in fields or collections.

Capture-free function types and by-value expression closures are implemented. FFI v0 fixed-width declarations, byte and string views, and opaque handle types are implemented. Extern functions do not become first-class function values.

`str` is the owned UTF-8 text type. A distinct borrowed text-view type is unavailable. Current syntax does not imply any of the unavailable types.

Design records:

- `architecture_docs/decisions/0007-duration-representation.md`: the signed nanosecond Duration representation and operators.
- `architecture_docs/decisions/0019-duration-conversion-and-timer-policy.md`: Duration rounding, rendering, and invalid host-timer policy.
- `architecture_docs/decisions/0026-minimal-tuples.md`: structural tuple types and tuple equality.
- `architecture_docs/decisions/0033-structural-transfer-and-task-results.md`: Transfer classification and conditional `Task[T]` copies.
