# Classes

A class is a nominal product type. Each value holds a fixed set of named fields and exposes the class's methods. Ordinary classes are move types. A class declared `copy class` is a copy type.

The checker statically checks class names, field types, defaults, methods, visibility, constructors, initializers, the ownership category, and recursive layout. The full syntax is in [Grammar](/manual/grammar#classes).

## Declaration

```aura
class Point:
    x: float64
    y: float64
```

A class body contains one or more fields, methods, or `pass` entries, in any order. Field names must be unique among fields. Method names must be unique among methods.

A class body may also hold a class docstring as its first entry and a field docstring directly after each field. See [Docstrings And API Docs](/manual/docstrings).

Every field has an explicit type. A field may have a default expression of exactly that type:

```aura
class Server:
    host: str = "127.0.0.1"
    port: int32 = 8080
```

Aura evaluates a default afresh for each construction that omits the field, so defaults are never shared mutable values. The checker checks a default in the declaration's context, not in the caller's local scope.

A generic class declares type parameters after its name. Each parameter may have a bound:

```aura fragment
class Box[T]:
    value: T

class NamedBox[T: Named]:
    value: T
```

Type parameter names must be unique. Every field and method type must be known and have the correct arity. Every concrete substitution must satisfy its bounds. Generic arguments are invariant. See [Generics And Traits](/manual/generics-and-traits).

## Construction

Call the class name to construct a value. A class without `__init__` takes its fields as arguments. A class with `__init__` takes the initializer's parameters instead; see [Initializers](#initializers). Field arguments may be positional in field declaration order, named by field, or positional followed by named:

```aura fragment
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

```aura fragment
box = Box[int32](value=42)
```

Without explicit arguments, the checker infers them from the supplied fields or from an expected class type. Every declared type parameter must resolve, even one that appears only in an omitted, defaulted field.

## Initializers

Declare `__init__(self, ...)` to give a class call its own parameters. The class call then takes the initializer's arguments instead of the fields, and the body sets each field:

```aura
class Server:
    host: str
    port: int64
    label: str
    retries: int64 = 3

    def __init__(self, host: own str, port: int64 = 8080, *, prefix: str = "srv"):
        self.host = host
        self.port = port
        self.label = f"{prefix}:{port}"

def main():
    server = Server("0.0.0.0", port=9090)
    other = Server(host="127.0.0.1")
    print(server.label)
    print(other.retries)
```

The initializer follows these rules:

- It is spelled `def __init__(self, ...)` with a bare `self` receiver and no result annotation, or `-> None`.
- Its parameters use the ordinary function grammar: capabilities, defaults, and the keyword-only `*` marker. They may use the class's type parameters, but `__init__` cannot declare its own.
- The class call binds arguments against the initializer's parameters. The field-based constructor is not available for that class. A class that needs both shapes declares a named factory.
- `return` without a value is allowed once every field is initialized. `return value` is rejected.
- `__init__` cannot be called as a method, used as a value, or declared in a trait.
- A `copy class` and a generic class may declare `__init__`.

Inside the body, `self` names the object under construction. A field with a declaration default starts initialized. Every other field starts uninitialized. Aura evaluates the call's arguments, then the omitted parameter defaults, then the field defaults in declaration order, and then runs the body.

### Definite Initialization

The checker tracks each field through branches, loops, and early returns. Every path that leaves the initializer must have set every field:

```aura
class Buffer:
    data: list[int64]
    capacity: int64

    def __init__(self, capacity: int64):
        if capacity < 0:
            self.capacity = 0
        else:
            self.capacity = capacity
        self.data = [self.capacity]
        self.describe()

    def describe(self):
        print(self.data.len())
```

`self.capacity` is set on both branches, so the body may read it afterwards. A field set only inside a loop body may be uninitialized after the loop, unless the loop is `while true` and the loop exits only through `break`.

| Use inside `__init__` | Requirement |
| --- | --- |
| `self.f = value` | `f` is either uninitialized or initialized on every path. A field that is set on only some paths cannot be assigned until it is known. |
| Read, lend, or pass `self.f` | `f` is initialized. |
| Assign into `self.f.g` | `f` is initialized. |
| Use `self` as a whole: a method call, an argument, a capture, or a `match` | Every field is initialized. |
| `return` or the end of the body | Every field is initialized. |
| Move `self` into an `own` parameter, a binding, or a result | Always rejected. The class call returns the object. |

After the last field is set, `self` behaves like a `mut self` receiver. The body may call methods on it and pass it as a bare or `mut` argument. A helper function may receive an initialized field such as `self.data` before the object is complete. The helper sees only that field.

An initializer cannot move a field out of `self`. Pass a copy, or compute the value before assigning it to the field.

### Failure During Initialization

An initializer cannot return a `Result`. Use a named factory for construction that can fail:

```aura
class Port:
    value: int64

    def __init__(self, value: int64):
        self.value = value

    def checked(value: int64) -> Result[Port, str]:
        if value < 1 or value > 65535:
            return Result.Err(f"port {value} is out of range")
        return Result.Ok(Port(value))

def main():
    match Port.checked(8080):
        case Result.Ok(port):
            print(port.value)
        case Result.Err(message):
            print(message)
```

A trap or a task cancellation can still stop an initializer before it finishes. Aura then drops the fields that were already set, exactly once, and the incomplete object is never visible to the caller.

## Visibility And Construction Across Modules

Classes, fields, and methods are private to their defining module unless marked `public`:

```aura
public class Counter:
    public value: int32 = 0

    public def get(self) -> int32:
        return self.value
```

Another module may import only a `public class`, and may read or call only its public members. A constructor call from another module may set only public fields. So a private field on a class that other modules construct must have a declaration default. Without one, an external caller cannot supply the required field.

For a class with `__init__`, the initializer's visibility decides who may call the class. Mark it `public def __init__` to allow class calls from other modules. A private initializer makes a public class constructible from other modules only through its public factories. The initializer body runs in the class's module, so it may set private fields for any caller.

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

```aura fragment
def main():
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

## Properties

A property is a computed value that reads like a field. Mark a shared method with `@property` on the line before it, and read it without parentheses:

```aura
class Reply:
    prompt_tokens: int64
    completion_tokens: int64

    @property
    def total_tokens(self) -> int64:
        return self.prompt_tokens + self.completion_tokens

def main():
    reply = Reply(prompt_tokens=10, completion_tokens=5)
    print(reply.total_tokens)
    print(f"{reply.total_tokens} tokens")
```

`reply.total_tokens` evaluates `reply` once and calls the getter once. The result is a fresh owned value, so a non-Copy result can be moved or bound to a `mut` local without affecting the receiver.

A getter must have this exact shape:

- It is a method in a class body. A module function, a trait method, and an `impl` method cannot be a property.
- Its receiver is bare shared `self`.
- It has no parameters and no type parameters.
- It returns an owned value other than `None`. A view result is rejected.
- Its name does not collide with a field of the class.

`@property` is the only marker a class body accepts. It stands alone on the line before `def`, and it takes no arguments. Any other `@` line in a class body is `AU2005`. Decorators on module-level functions are described in [Decorators](/manual/functions#decorators).

A property is read-only. The checker rejects each of these with `AU2045`:

- assignment and compound assignment, such as `reply.total_tokens = 3`
- assignment through the result, such as `shape.origin.x = 4`
- a `mut` argument, a `mut self` method call, or a `match mut` scrutinee
- a `view` or `view mut` of the property
- call syntax, such as `reply.total_tokens()`

```aura check-fail:AU2045
class Reply:
    prompt_tokens: int64

    @property
    def doubled(self) -> int64:
        return self.prompt_tokens * 2

def main():
    mut reply = Reply(prompt_tokens=1)
    reply.doubled = 4
```

To change the value, bind it to a local first. The local is an independent copy.

A getter's docstring documents the property. Hover on a read shows the property's type and then the docstring. See [Docstrings And API Docs](/manual/docstrings).

```aura
class Reply:
    prompt_tokens: int64
    completion_tokens: int64

    @property
    def total_tokens(self) -> int64:
        """Tokens across the prompt and the completion."""
        return self.prompt_tokens + self.completion_tokens
```

A property is not a trait member. An imported class exposes its `public` properties, and a private property is readable only in its own module.

## Mutation

A field assignment requires a mutable base place:

```aura fragment
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

## Context Manager Classes

A class is a context manager when it declares `__exit__(mut self)`. `with` calls its optional `__enter__` before the body and its `__exit__` when control leaves the body:

```aura
class Session:
    name: str

    def __enter__(mut self) -> None:
        print("open " + self.name)

    def __exit__(mut self) -> None:
        print("close " + self.name)
```

`__exit__` must take `mut self`, take no other parameters, and return `None` or `Result[None, E]`. `__enter__` must take `mut self`, take no other parameters, and return `None` or a view of `self` declared as `view [mut] T from self`. A class without `__exit__` that declares `close(mut self) -> None` is a context manager whose exit is `close`. Generic classes qualify under the same rules. Any other shape is rejected with `AU2017`.

```aura fragment
with session = Session(name="db"):
    print("using session")
```

`with` consumes the item expression into a fresh mutable managed binding. The binding cannot be moved out while its exit is registered. The exit runs exactly once per registration, on normal exits and on maintained abnormal exits, in reverse nesting order. [Statements](/manual/statements#with-and-context-managers) defines entry views, multi-item headers, and exits that return `Result`.

## Grammar

[Grammar](/manual/grammar#classes) holds the normative productions for `class`, `copy class`, visibility, type parameters, fields, field defaults, `indirect`, methods, receivers, and associated methods. A class suite contains fields, methods, `@property` methods, and `pass`. An initializer is a method named `__init__`. Aura has no separate grammar for constructors, inheritance, or destructors.

## Typing Rules

- Classes are nominal, and generic arguments are invariant.
- Every field has one declared type. Defaults and constructor arguments must have that exact type after substitution.
- Constructor binding follows field declaration order and requires every accessible field that has no default.
- A class with `__init__` binds class-call arguments against the initializer's parameters instead of its fields. Its body must initialize every field on every exit path.
- The checker rejects duplicate, unknown, inaccessible private, and excess constructor arguments.
- The receiver mode controls which field accesses are legal.
- A `copy class` requires every field to be statically copyable.
- Every direct recursive layout cycle requires `indirect`.
- The checker checks cross-module visibility and the context manager shapes of `__enter__`, `__exit__`, and `close` before lowering.

## Runtime Semantics

Construction creates one fresh nominal value. For a class with `__init__`, Aura binds the call's arguments, evaluates the field defaults in declaration order, and runs the initializer body on the new object. Otherwise Aura evaluates the supplied field expressions in call-site source order. Each result is copied or moved into its owned field slot before any later field expression runs. Aura then evaluates each omitted field's default in field declaration order, afresh each time. Binding values to field slots does not reorder evaluation, and a supplied field's default is never evaluated.

An instance call invokes the statically selected inherent or trait method. An associated method receives no implicit instance. Class equality compares the nominal class identity and the represented field values. A context manager class exits exactly once through its active `with` registration, under the cleanup rules in [Execution Model](/manual/execution-model).

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
| `AU2005` | `__init__` called as a method or used as a value, or a class-body `@` line other than a bare `@property`. |
| `AU2016` | Invalid `__init__` declaration: a receiver other than bare `self`, its own type parameters, a result other than `None`, `return value`, or a trait member named `__init__`. |
| `AU2017` | A `with` item whose class has no valid `__exit__` or `close`, or whose `__enter__` has the wrong shape. |
| `AU2043` | An invalid `Display` implementation. See [Generics And Traits](/manual/generics-and-traits#the-display-trait). |
| `AU2044` | An invalid property declaration. |
| `AU2045` | Assignment, mutable use, a view, or call syntax on a property. |
| `AU2999` | Duplicate declaration, invalid visibility, invalid recursive layout, unsupported member use, and other class rejections without a narrower code. |
| `AU3001` | Use of a moved class or field. |
| `AU3002` | Overlapping receiver and argument borrows, or moving a field through shared access. |
| `AU3003` | Mutation through an immutable class place, including a shared `self` receiver. |
| `AU3004` | Invalid ownership or receiver mode, including moving `self` out of `__init__`. |
| `AU3012` | A field of `self` used before it is initialized, an assignment to a field that may already be initialized, or `self` escaping or returning before every field is set. |

A trap inside a field default, method, or cleanup body keeps the code for the operation that trapped:

| Code | Cause |
| --- | --- |
| `AU4001` | General runtime trap. |
| `AU4002` | Arithmetic overflow or underflow. |
| `AU4003` | Bounds or lookup violation. |
| `AU4004` | Zero divisor. |
| `AU4005` | Resource or I/O failure. |

## Backend Support

MIR execution and direct native generation both implement nominal classes, generic specialization, fields and defaults, all maintained receiver modes, partial moves, structural equality, `copy class`, `indirect`, visibility, and context manager entry and exit. Both backends receive the same checked class and method metadata. Compiler analysis and the language server use that metadata for member resolution and signatures.

## Limits And Implementation-Defined Behavior

- Aura 0.8 has no class inheritance, overloads, property setters, or general destructor hook. A class has at most one initializer.
- An initializer cannot move a field out of `self` or return a `Result`.
- A class field default cannot call a user-defined function in the current compiler. Compute the value before construction and pass it as an explicit field argument.
- `indirect` is only a recursive field-layout marker. Its storage representation is not an observable language contract.
- The physical order and padding of fields are not observable language contracts.
- Construction and method evaluation order are language-defined, not implementation-defined.

## Status

Implemented:

- ordinary, copy, and generic classes
- construction and field defaults
- initializers with definite initialization, and named factories
- visibility
- inherent and associated methods, with all maintained receiver modes
- partial-field moves
- recursive `indirect` fields
- context manager classes, including generic ones
- local shared or mutable views of fixed class fields
- returned-view contracts tied to one receiver or parameter
- read-only `@property` getters
- custom rendering through the prelude `Display` trait

Views cannot be stored in class fields or other owned aggregates.

Not implemented: inheritance, properties, and destructor hooks. Do not infer them from accepted class syntax.

The fixture `crates/aura-compiler/tests/fixtures/run-pass/explicit_and_default_argument_order.au` pins the constructor evaluation order on both backends.

Design record: ADR-0015, `architecture_docs/decisions/0015-explicit-and-default-argument-order.md`, for constructor evaluation order, and ADR-0038 for field views and returned-view contracts. Both are Accepted.
