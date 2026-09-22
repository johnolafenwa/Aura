# Classes

A class is a nominal product type. Each value holds a fixed set of named fields and exposes the class's methods. Ordinary classes are move types. A class declared `copy class` is a copy type.

The checker statically checks class names, field types, defaults, methods, visibility, constructors, the ownership category, and recursive layout. The full syntax is in [Grammar](/manual/grammar#classes).

## Declaration

```aura
class Point:
    x: float64
    y: float64
```

A class body contains one or more fields, methods, or `pass` entries, in any order. Field names must be unique among fields. Method names must be unique among methods.

Every field has an explicit type. A field may have a default expression of exactly that type:

```aura
class Server:
    host: str = "127.0.0.1"
    port: int32 = 8080
```

Aura evaluates a default afresh for each construction that omits the field, so defaults are never shared mutable values. The checker checks a default in the declaration's context, not in the caller's local scope.

A generic class declares type parameters after its name. Each parameter may have a bound:

```aura
class Box[T]:
    value: T

class NamedBox[T: Named]:
    value: T
```

Type parameter names must be unique. Every field and method type must be known and have the correct arity. Every concrete substitution must satisfy its bounds. Generic arguments are invariant. See [Generics And Traits](/manual/generics-and-traits).

## Construction

Call the class name to construct a value. Arguments may be positional in field declaration order, named by field, or positional followed by named:

```aura
point = Point(3.0, 4.0)
server = Server()
custom = Server("0.0.0.0", port=9090)
named = Point(x=3.0, y=4.0)
```

Every constructor field is an owned position. `Point` behaves like `Point(x: own float64, y: own float64)`, and `Box[T]` behaves like `Box(value: own T)`. A non-copy argument moves into the new object. Defaults also create fresh owned field values.

Construction follows these rules:

1. Positional arguments fill fields in declaration order.
2. A positional argument cannot follow a named argument.
3. A field cannot be supplied more than once.
4. An unknown field name or an excess positional argument is rejected.
5. Every field without a default must be supplied.
6. Each supplied or default value must have the field's exact substituted type.
7. Every field argument is `own`. A move value is consumed, and a copy value is duplicated.

Evaluation order is fixed:

1. Aura evaluates every supplied field expression in call-site source order.
2. Each result is copied or moved into its owned field slot before the next supplied expression begins. A later side effect cannot change an earlier captured field value.
3. Aura then evaluates the defaults of omitted fields in field declaration order.

Binding positional or named arguments to field slots never reorders evaluation. Supplying a field skips that field's default entirely.

Generic arguments may be explicit:

```aura
box = Box[int32](value=42)
```

Without explicit arguments, the checker infers them from the supplied fields or from an expected class type. Every declared type parameter must resolve, even one that appears only in an omitted, defaulted field.

## Visibility And Construction Across Modules

Classes, fields, and methods are private to their defining module unless marked `public`:

```aura
public class Counter:
    public value: int32 = 0

    public def get(self) -> int32:
        return self.value
```

Another module may import only a `public class`, and may read or call only its public members. A constructor call from another module may set only public fields. So a private field on a class that other modules construct must have a declaration default. Without one, an external caller cannot supply the required field.

An imported declaration keeps its defining module's identity for private-access checks. See [Names And Scopes](/manual/names-and-scopes#imports).

## Methods And Receivers

```aura
class Counter:
    value: int32 = 0

    def get(self) -> int32:
        return self.value

    def increment(mut self):
        self.value += 1

    def into_value(own self) -> int32:
        return self.value

    def zero() -> Counter:
        return Counter(value=0)
```

The receiver, when present, is the first method parameter:

| Receiver | Call contract |
| --- | --- |
| `self` | Shared receiver and the default spelling. It can read, but cannot mutate or move non-copy fields out. |
| `own self` | Consuming receiver. A non-copy instance is moved into the call. |
| `mut self` | Exclusive mutable receiver. The call requires a mutable place and may mutate it. |
| none | Associated method. It is called through the type, not an instance. |

```aura
mut counter = Counter.zero()
counter.increment()
print(counter.get())
value = counter.into_value()
```

In every other respect, methods follow the function rules for generic parameters, ordinary parameters, defaults, and owned returns. Ordinary parameter names must be unique and cannot collide with a declared `self` receiver. A typed first parameter such as `self: Counter` is not a receiver. The checker rejects it with a diagnostic that names the valid forms. `Self` may appear in a method's parameter and return types, and it denotes the enclosing class specialization.

An associated method has no implicit `self` and is called as `Counter.zero()`. Instance call syntax works only for methods with a compatible receiver and for trait methods selected for the instance type.

A method named without a call is a value:

- `Counter.zero` is a function value with the method's contract. This works for non-generic classes only.
- `counter.read` is a bound method closure over the receiver.
- A generic method spells its type arguments as `method[T]`.

See [Closures](/manual/closures#bound-methods).

## Mutation

A field assignment requires a mutable base place:

```aura
mut counter = Counter.zero()
counter.value = 10
counter.increment()
```

An owned local is mutable only when it is introduced with `mut`. Inside a `mut self` method, `self` is a mutable place, although the parameter binding itself cannot be reassigned. Inside a shared `self` method, the checker rejects mutation through `self`.

Moving one non-copy field out of an owned class partially moves the value. The other fields stay usable. Use of the whole class is rejected until the moved field is reinitialized. See [Ownership And Borrowing](/manual/ownership-and-borrowing#partial-moves-and-reinitialization).

## Returning Fields

A consuming receiver owns the class value, so it may return an owned field:

```aura
class User:
    name: str

    def into_name(own self) -> str:
        return self.name
```

A shared receiver cannot move an owned field out. When the field type supports cloning, clone it to produce an owned result:

```aura
class User:
    name: str

    def name_copy(self) -> str:
        return self.name.clone()
```

Returning a copy-valued field produces an ordinary independent copy:

```aura
class Counter:
    value: int32

    def value_copy(self) -> int32:
        return self.value
```

To return a non-copy field through an ordinary `-> T` result, the method needs ownership. Clone the field when it is clone-safe, or consume the owner with `own self`. A method may instead declare `-> view [mut] T from self` and return non-owning access to the receiver or to one of its supported fixed projections. See [Functions](/manual/functions#owned-returns).

## `copy class`

```aura
copy class Pair:
    left: int32
    right: int32
```

Assignment and by-value use duplicate a `copy class` value. The declaration is valid only when every field is statically copyable. A field of type `str`, a collection, a resource, an ordinary class, or an enum with move payloads prevents a `copy class` declaration.

Copyability is structural through copy classes and eligible enum payloads. A generic type parameter is not assumed copyable because some instantiation uses a copy type. [Types](/manual/types#copy-and-move-categories) lists all copy and move categories.

## Recursive Fields And `indirect`

A class cannot contain itself through a path of direct class fields. This covers direct self-recursion, recursion nested inside another type, and mutual recursion through other classes.

Mark a field `indirect` to break the direct layout cycle:

```aura
class Node:
    value: int32
    next: indirect Node | None = None
```

The `indirect` marker stores the whole field indirectly. `|` binds more loosely than `indirect`, so the field's type is the optional union of an indirect `Node` with `None`.

`indirect` applies to the complete type reference that follows it. It is a field-layout marker. It is not a pointer expression and not a runtime operation. At least one field on every recursive layout cycle must be `indirect`.

## User Resource Classes

`with` can manage a non-generic user class that declares this exact instance method:

```aura
class Resource:
    name: str

    def close(mut self) -> None:
        print("closing " + self.name)
```

The method must be named `close`, use `mut self`, take no ordinary parameters, and return `None`. In Aura 0.3, `with` does not support generic user resource classes.

```aura
with resource = Resource(name="db"):
    print("using resource")
```

`with` consumes the resource expression into a fresh mutable managed binding. The binding cannot be moved out while cleanup is active. Cleanup runs exactly once per registration, on normal exits and on maintained abnormal exits, in reverse nesting order. See [Execution Model](/manual/execution-model#resource-lifetime-and-cleanup).

## Grammar

[Grammar](/manual/grammar#classes) holds the normative productions for `class`, `copy class`, visibility, type parameters, fields, field defaults, `indirect`, methods, receivers, and associated methods. A class suite contains fields, methods, and `pass`. Aura has no separate grammar for constructors, properties, inheritance, or destructors.

## Typing Rules

- Classes are nominal, and generic arguments are invariant.
- Every field has one declared type. Defaults and constructor arguments must have that exact type after substitution.
- Constructor binding follows field declaration order and requires every accessible field that has no default.
- The checker rejects duplicate, unknown, inaccessible private, and excess constructor arguments.
- The receiver mode controls which field accesses are legal.
- A `copy class` requires every field to be statically copyable.
- Every direct recursive layout cycle requires `indirect`.
- The checker checks cross-module visibility and the exact user-resource shape `close(mut self) -> None` before lowering.

## Runtime Semantics

Construction creates one fresh nominal value. Aura evaluates the supplied field expressions in call-site source order. Each result is copied or moved into its owned field slot before any later field expression runs. Aura then evaluates each omitted field's default in field declaration order, afresh each time. Binding values to field slots does not reorder evaluation, and a supplied field's default is never evaluated.

An instance call invokes the statically selected inherent or trait method. An associated method receives no implicit instance. Class equality compares the nominal class identity and the represented field values. A managed user-resource class is closed exactly once by its active `with` registration, under the cleanup rules in [Execution Model](/manual/execution-model).

## Ownership And Evaluation Order

- Every constructor field is an owned destination. Copy arguments are copied, and non-copy arguments move into the new value.
- Ordinary classes move. Valid `copy class` values copy.
- A shared receiver reads, `own self` consumes, and `mut self` requires an exclusive mutable place.
- Moving an owned non-copy field partially moves its class until that field is reinitialized. Moving a field through a borrowed receiver is rejected.
- Aura inserts no hidden clone at a constructor, field, receiver, or return boundary.
- Constructor side effects follow the supplied-then-default order above, even when named arguments bind fields out of declaration order.

## Diagnostics

| Code | Cause |
| --- | --- |
| `AU1101` | Malformed class, field, method, or receiver syntax. |
| `AU2001` | Unresolved class, field type, method, or member. |
| `AU2002` | Field, default, or constructor type mismatch. Generic arity or bound failure. A non-copy field in a `copy class`. |
| `AU2004` | Constructor or method argument-binding failure. |
| `AU2999` | Duplicate declaration, invalid visibility, invalid recursive layout, unsupported member use, and other class rejections without a narrower code. |
| `AU3001` | Use of a moved class or field. |
| `AU3002` | Overlapping receiver and argument borrows, moving a field through shared access, or an invalid user-resource `close` contract. |
| `AU3003` | Mutation through an immutable class place, including a shared `self` receiver. |
| `AU3004` | Invalid ownership or receiver mode. |

A trap inside a field default, method, or cleanup body keeps the code for the operation that trapped:

| Code | Cause |
| --- | --- |
| `AU4001` | General runtime trap. |
| `AU4002` | Arithmetic overflow or underflow. |
| `AU4003` | Bounds or lookup violation. |
| `AU4004` | Zero divisor. |
| `AU4005` | Resource or I/O failure. |

## Backend Support

MIR execution and direct native generation both implement nominal classes, generic specialization, fields and defaults, all maintained receiver modes, partial moves, structural equality, `copy class`, `indirect`, visibility, and user-resource cleanup. Both backends receive the same checked class and method metadata. Compiler analysis and the language server use that metadata for member resolution and signatures.

## Limits And Implementation-Defined Behavior

- Aura 0.3 has no class inheritance, overloads, property syntax, custom constructor hook, or general destructor hook.
- `with` cannot manage a generic user class directly.
- A class field default cannot call a user-defined function in the current compiler. Compute the value before construction and pass it as an explicit field argument.
- `indirect` is only a recursive field-layout marker. Its storage representation is not an observable language contract.
- The physical order and padding of fields are not observable language contracts.
- Construction and method evaluation order are language-defined, not implementation-defined.

## Status

Implemented:

- ordinary, copy, and generic classes
- construction and field defaults
- visibility
- inherent and associated methods, with all maintained receiver modes
- partial-field moves
- recursive `indirect` fields
- non-generic user-resource classes
- local shared or mutable views of fixed class fields
- returned-view contracts tied to one receiver or parameter

Views cannot be stored in class fields or other owned aggregates.

Not implemented: inheritance, properties, custom constructor or destructor hooks, and generic `with` resources. Do not infer them from accepted class syntax.

The fixture `crates/aura-compiler/tests/fixtures/run-pass/explicit_and_default_argument_order.au` pins the constructor evaluation order on both backends.

Design record: ADR-0015, `architecture_docs/decisions/0015-explicit-and-default-argument-order.md`, for constructor evaluation order, and ADR-0038 for field views and returned-view contracts. Both are Accepted.
