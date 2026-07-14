# Static Semantics

Static semantics are the rules applied after parsing and module loading and before MIR lowering or native code generation. A module is well typed only if every declaration, statement, expression, pattern, call, move, and borrow satisfies these rules.

This chapter states the cross-cutting rules. The declaration-specific chapters provide additional contracts, and [Ownership And Borrowing](/manual/ownership-and-borrowing) defines place and lifetime restrictions.

## Types And Type Equality

Aurora 0.1 uses nominal types with invariant generic arguments. Two types match when their canonical names and recursively all type arguments are equal. There is no general subtype relation and no implicit numeric widening.

Examples:

- `int` and `int64` are the same canonical type.
- `int32` and `int64` are different types.
- `Vec[int32]` and `Vec[int64]` are different types.
- two user classes with identical fields are still different types.
- an imported type retains its defining module identity even when imported under an unqualified binding.

`T?` is syntactic sugar for `Option[T]`. `int` canonicalizes to `int64`, and `str` currently canonicalizes to `String`; neither alias introduces a distinct runtime type.

Every generic type use must supply its declared number of type arguments. Non-generic types reject type arguments. `Self` is available only in supported trait and implementation type positions.

## Contextual Inference

Aurora uses local, contextual inference rather than global inference. Public function parameters, fields, method signatures, and explicit return values remain typed in source. The checker uses an expected type from an annotation, parameter, return position, collection, constructor field, or surrounding expression where the rule is unambiguous.

### Literals

- An integer literal adopts an expected integer type and must fit it; otherwise it defaults to `int64`.
- A negative integer literal is parsed as unary `-` applied to a non-negative literal and must fit the selected signed integer type.
- A floating literal adopts an expected `float32` or `float64`; otherwise it defaults to `float64`.
- `true` and `false` have type `bool`.
- A single-quoted or double-quoted ordinary string and an f-string each have type `String`; quote choice does not create a distinct type.
- A duration literal has type `Duration`.
- Bare `None` has type `None`, except in an expected `Option[T]` position where it denotes `Option.None` of that type.

### Collections

A non-empty list, set, or map infers its element/key/value type from the first value unless an expected collection type is available. All remaining values must have the same inferred type.

An empty collection literal has no self-contained element type and therefore requires an expected `Vec[T]`, `Set[T]`, or `Map[K, V]` type. `{}` is grammatically a map literal but may be interpreted as an empty `Set[T]` when its expected type is `Set[T]`.

### Generic Calls

Generic type parameters are inferred by unifying argument types with parameter type patterns and, where available, the expected result type. Explicit specialization such as `identity[int64](value)` seeds or fixes the substitutions.

Every declared type parameter must resolve. The substituted type must satisfy all declared trait bounds. Inference does not guess from unrelated declarations or from runtime values.

## Declarations

A declaration is valid only when:

- its item name does not collide with another local/imported item or a reserved builtin
- type parameter names are unique and their bounds name known traits with correct arity
- field, variant, and method names are unique within the relevant declaration
- all referenced types exist with the correct arity
- default expressions have exactly the declared parameter or field type
- a non-`None` function or method returns on every statically reachable fallthrough path
- copy classes contain only copy-compatible fields
- trait implementations satisfy the trait's type arguments, supertraits, method set, and method signatures

Class, enum, function, and trait declarations may be `public` at module scope. `impl` cannot be public because it introduces no independently imported item.

## Bindings And Assignment

The first simple-name assignment introduces a binding. Its type is the annotation when present, otherwise the initializer type. The initializer must match exactly.

`mut` makes the new binding assignable and a mutable place. Reassignment requires an existing mutable place and preserves the original type. Reassignment reinitializes a fully moved binding or field when the assigned value has the correct type.

Compound assignments `+=`, `-=`, `*=`, `/=`, `%=`, and `//=` read the current target, apply the corresponding binary operation, and write the result. The target must already exist, be mutable, not be moved, and have the operation's result type. Integer `/=` is rejected by the same rule and teaching diagnostic as integer `/`; floating `/=` remains valid.

Field assignment requires a mutable base place and a declared field. Index assignment supports `Vec[T]` with exactly an `int32` index and `Map[K, V]` with a key of exactly `K`. An annotation and `mut` are not permitted on member or index assignment.

## Expression Typing

### Unary Operators

- `not value` accepts `bool` and returns `bool`, or resolves a matching `Not.not` trait operation.
- `-value` accepts an integer or float and returns the same type, or resolves a matching `Neg.neg` operation.
- `try value` requires `value: Result[T, E1]` and an enclosing return type `Result[U, E2]`; it has type `T` when `E1 == E2` or an applicable `impl From[E1] for E2` exists.

### Binary Operators

Built-in operator typing is:

| Operators | Operand rule | Result |
| --- | --- | --- |
| `and`, `or` | both `bool` | `bool` |
| `+` | equal integer types, equal float types, or two `String` values | operand type |
| `-`, `*`, `//`, `%` | equal integer or equal float types | operand type |
| `/` | equal float types | operand type |
| `==`, `!=` | equal operand types | `bool` |
| `<`, `<=`, `>`, `>=` | equal integer or equal float types | `bool` |

When both operands have the same integer type, `/` is rejected with this exact maintained diagnostic:

```text
integer `/` is not supported; use `//` for floor division, or call `.to_float()` on both operands for true division
```

Arithmetic and ordering operators may otherwise resolve through the corresponding `Add`, `Sub`, `Mul`, `Div`, `Mod`, or `Ord` trait method. `//` is builtin-only and has no `FloorDiv` trait. Builtin equality does not dispatch through an operator trait in Aurora 0.1.

Operator operands are not implicitly widened. A literal may be contextually typed to match the other operand; non-literal values require an explicit numeric cast.

### Conditions

`if` and `while` conditions must have exactly type `bool`. `and`, `or`, and `not` also require boolean results under the rules above. Aurora does not apply general truthiness conversion to strings, collections, resources, or user types.

### Indexing And Members

Direct indexing supports `Vec[T]` with exactly an `int32` index and `Map[K, V]` with exactly `K`. For a vector, a negative index `i` is normalized once as `len + i` before the existing bounds check; this applies equally to direct reads and writes and to `get`, `set`, `remove`, both `swap` indexes, and `insert`. An index that remains invalid is not clamped. Direct access and mutating methods fail at runtime, while `get` returns `None`; `insert` accepts the post-normalization range `0..=len`. An already-bound `int64` index is not implicitly narrowed.

A direct read produces `T` or `V`, but moving a non-copy vector element by direct indexing is restricted by ownership rules; use `get()` when an explicit clone/optional result is required. Integer indexing and slicing are not defined for `String` in Aurora 0.1.

Member access must resolve to a visible field, method, enum variant, module item, or maintained builtin member. Calling a receiver method also validates whether the receiver is consumed, shared-borrowed, or mutable-borrowed.

## Call Binding

Arguments are written as positional arguments followed by named arguments. Binding proceeds against the declaration or builtin metadata:

1. positional arguments fill parameters in declaration order
2. named arguments fill the parameter with the same name
3. a parameter cannot be filled twice
4. unknown names and extra arguments are rejected
5. omitted parameters require defaults
6. each argument type must equal the substituted parameter type

Default expressions are evaluated for each call where the parameter is omitted. Defaults may refer only to names valid under the declaration's default-expression rules; they do not capture a caller's locals.

A by-value parameter consumes a non-copy argument. A `borrow` parameter requires a readable place or compatible borrowed value. A `borrow mut` parameter requires a mutable place. All arguments at one call boundary are checked together for overlapping move/shared/mutable access.

## Class Construction

Calling a class name constructs a value. Constructor fields may be supplied positionally in declaration order, then by name. Positional arguments cannot follow a named argument. Every field without a declaration default must be supplied exactly once; provided and default values must match the substituted field types.

A class receiver is declared as `self`, `borrow self`, or `borrow mut self`. A method without `self` is associated and is called through the class/type rather than an instance.

## Enum Construction And Matching

An enum variant constructor must name an existing variant and provide exactly its payload shape. A variant declares either all positional payloads or all named payloads; constructors bind accordingly.

Generic enum constructors require sufficient context to determine all type arguments. This may come from explicit specialization, an expected annotation/parameter/return type, or payload inference. Bare builtin variants such as `Some`, `Ok`, `Err`, or `None` are accepted only where the expected enum identity is unambiguous.

A match pattern must be compatible with the scrutinee type. Variant payload subpatterns must have exactly the variant's arity. Literal patterns must match the scrutinee's supported scalar type. Duplicate or unreachable arms are rejected where the checker can establish overlap.

Matches over enums and booleans must be exhaustive unless `_` covers the remainder. Literal matches over open numeric/string domains require `_`. Every arm of a match expression must produce the same result type, using the surrounding expected type where available.

## Generics, Traits, And Implementations

Traits are nominal interfaces. A bound `T: A + B` requires an applicable implementation of each trait after substitution. Supertraits are inherited requirements.

An `impl` identifies one trait specialization and one target type pattern. Its methods must correspond to trait methods; missing required methods are rejected unless the trait provides a default body. Extra methods are not part of that trait implementation.

For a concrete receiver, the checker chooses the unique applicable implementation with greatest specificity. If multiple equally specific implementations apply, the call or operator is ambiguous and rejected. Source order is not a tie breaker.

For a type parameter, available methods and operators come from its declared bounds. If multiple bounds expose an indistinguishable method, the access is ambiguous unless the language can resolve one unique contract.

Trait and implementation methods cannot declare default ordinary parameters in Aurora 0.1. Trait default method bodies are permitted; a signature-only trait method has no body after its terminating newline.

## Control Flow

`return` is valid only in a function or method. Its value must equal the declared return type; an omitted value has type `None`.

`break` and `continue` are valid only inside `for` or `while`. A loop-local binding does not escape. Moving a non-copy outer value for the first time inside a repeatable loop is rejected unless the checker can prove the path does not create an invalid next iteration.

An `if`, statement match, or match expression checks branches independently and merges move/partial-move state conservatively across reachable paths. A non-`None` function is rejected when any reachable path can fall through without returning.

## `with` Resources

`with` consumes its resource expression and creates a mutable managed binding for the body. Supported builtin resources have runtime-defined cleanup. A non-generic user class may be used only when it declares exactly a `close(borrow mut self) -> None` instance method.

The managed binding cannot be moved out in a way that would prevent required cleanup. Leaving the scope normally, by return, by loop control, or through a runtime failure runs the cleanup behavior described in [Execution Model](/manual/execution-model).

## Tasks And Static Safety

`TaskGroup.start` and `start_soon` accept named functions and associated methods without `self`. The target's ordinary parameters must be by value; borrowed task parameters are rejected because a child may outlive the starting call frame.

Arguments are consumed or copied under ordinary call rules before the child runs. Task, queue, and cancellation runtime semantics are defined by [Concurrency](/manual/concurrency).

## Entrypoint Rules

The selected entry module may use one of two shapes:

- executable top-level statements and no local `main`
- a local `main` and no executable top-level statements

The local `main` takes no parameters and returns `None` or `int32`. Imported functions named `main` are ordinary imported functions and do not become the entrypoint.
