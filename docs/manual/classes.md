# Classes

Classes define nominal product types: one value contains a fixed set of named fields and exposes class methods. Ordinary classes are move types unless declared `copy class`.

The complete syntax is in [Grammar](/manual/grammar#classes). Class names, field types, defaults, methods, visibility, constructors, ownership category, and recursive layout are all statically checked.

## Declaration

```python
class Point:
    x: float64
    y: float64
```

A class body contains one or more fields, methods, or `pass` entries. Fields and methods may be interleaved. Field names must be unique among fields, and method names must be unique among methods.

Every field has an explicit type. A field may have a default expression of exactly that type:

```python
class Server:
    host: String = "127.0.0.1"
    port: int32 = 8080
```

Field defaults are evaluated afresh for each construction where the field is omitted. They are not shared mutable singletons. A default is checked in declaration context, not in the caller's local scope.

Generic classes declare bounded or unbounded type parameters after the name:

```python
class Box[T]:
    value: T

class NamedBox[T: Named]:
    value: T
```

Type parameter names must be unique, all field and method types must be known with the correct arity, and every concrete substitution must satisfy its bounds. Generic arguments are invariant. See [Generics And Traits](/manual/generics-and-traits).

## Construction

Calling the class name constructs a value. Arguments may be positional in field declaration order, named by field, or positional followed by named:

```python
point = Point(3.0, 4.0)
server = Server()
custom = Server("0.0.0.0", port=9090)
named = Point(x=3.0, y=4.0)
```

Construction follows these rules:

1. positional arguments fill fields in declaration order
2. positional arguments cannot follow a named argument
3. a field cannot be supplied more than once
4. an unknown field name or excess positional argument is rejected
5. every field without a default must be supplied
6. each provided or default value must have the field's exact substituted type
7. constructing with a move value consumes that value; copy values are duplicated

Generic arguments may be explicit:

```python
box = Box[int32](value=42)
```

Without explicit arguments, the checker infers them from provided fields or an expected class type. Every declared type parameter must resolve, even when it appears only in an omitted/defaulted field.

## Visibility And Construction Across Modules

Classes, fields, and methods are private to their defining module unless marked `public`:

```python
public class Counter:
    public value: int32 = 0

    public def get(borrow self) -> int32:
        return self.value
```

Another module may import only a `public class`. It may read or call only public members. A cross-module constructor may explicitly initialize only public fields. Consequently, a private field on a publicly constructed class must have a declaration default; otherwise an external caller cannot satisfy the required field.

Imported declarations retain their defining module identity for private-access checks. See [Names And Scopes](/manual/names-and-scopes#imports).

## Methods And Receivers

```python
class Counter:
    value: int32 = 0

    def get(borrow self) -> int32:
        return self.value

    def increment(borrow mut self):
        self.value += 1

    def into_value(self) -> int32:
        return self.value

    def zero() -> Counter:
        return Counter(value=0)
```

The receiver, when present, is the first method parameter:

| Receiver | Call contract |
| --- | --- |
| `borrow self` | Shared receiver. It can read, but cannot mutate or move non-copy fields out. |
| `borrow mut self` | Exclusive mutable receiver. The call requires a mutable place and may mutate it. |
| `self` | Consuming receiver. A non-copy instance is moved into the call. |
| none | Associated method. It is called through the type, not an instance. |

```python
mut counter = Counter.zero()
counter.increment()
print(counter.get())
value = counter.into_value()
```

Methods otherwise follow the function rules for generic parameters, ordinary parameters, defaults, returns, and borrowed returns. Ordinary parameter names are unique and cannot collide with a declared `self` receiver. `Self` may be used in class method parameter and return type positions and denotes the enclosing class specialization.

An associated method has no implicit `self` and is called as `Counter.zero()`. Instance syntax is reserved for methods with a compatible receiver and for trait methods selected for the instance type.

## Mutation

A field assignment requires a mutable base place:

```python
mut counter = Counter.zero()
counter.value = 10
counter.increment()
```

An owned local is mutable only when introduced with `mut`. Inside a `borrow mut self` method, `self` is a mutable place even though parameter bindings themselves are not reassigned. Inside `borrow self`, mutation through `self` is rejected.

Moving one non-copy field from an owned class partially moves that value. Disjoint fields remain usable, but use of the complete class is rejected until the moved field is reinitialized. See [Ownership And Borrowing](/manual/ownership-and-borrowing#partial-moves-and-reinitialization).

## Returning Fields

A consuming receiver may return an owned field because it owns the class value:

```python
class User:
    name: String

    def into_name(self) -> String:
        return self.name
```

A shared-borrowed receiver cannot move an owned field. Clone to produce an owned result:

```python
class User:
    name: String

    def name_copy(borrow self) -> String:
        return self.name.clone()
```

Alternatively, an advanced API may declare a borrowed return tied to `self`:

```python
class User:
    name: String

    def name_ref(borrow self) -> borrow[self] String:
        return self.name
```

Borrowed return provenance is specified in [Functions](/manual/functions#borrowed-returns).

## `copy class`

```python
copy class Pair:
    left: int32
    right: int32
```

A `copy class` value is duplicated by assignment and by-value use. The declaration is valid only when every field is statically copyable. A `String`, collection, resource, ordinary class, or enum with move payloads therefore prevents copy-class declaration.

Copyability is structural through copy classes and eligible enum payloads, but generic type parameters are not assumed copyable merely because one later instantiation happens to use a copy type. The complete current categories are listed in [Types](/manual/types#copy-and-move-categories).

## Recursive Fields And `indirect`

A field layout cannot contain its class again through an all-direct class-field path. This includes direct self-recursion, recursion nested inside another type, and mutual recursion through other classes.

Mark a field `indirect` to break the direct layout cycle:

```python
class Node:
    value: int32
    next: indirect Option[Node] = Option.None
```

`indirect` applies to the complete following type reference. It is a field-layout marker, not a general pointer expression and not valid as an arbitrary runtime operation. At least one field on every recursive layout cycle must provide the indirection.

## User Resource Classes

A non-generic user class may be managed by `with` when it declares this exact instance method shape:

```python
class Resource:
    name: String

    def close(borrow mut self) -> None:
        print("closing " + self.name)
```

The method must be named `close`, use `borrow mut self`, take no ordinary parameters, and return `None`. Generic user resource classes are not supported by `with` in Aurora 0.1.

```python
with resource = Resource(name="db"):
    print("using resource")
```

`with` consumes the resource expression into a fresh mutable managed binding. That binding cannot be moved out while cleanup is active. Cleanup runs exactly once for the registration on normal and maintained abnormal exits, in reverse nesting order. See [Execution Model](/manual/execution-model#resource-lifetime-and-cleanup).
