# Generics And Traits

This page covers generic declarations, inference and explicit specialization, traits, implementations, and how Aura dispatches trait methods and operators.

A generic declaration is parameterized over types. A trait is a nominal interface: it names a set of methods, and a type satisfies it only through an `impl`. Traits provide generic bounds, method dispatch, operator dispatch, supertrait requirements, and `try` error conversion.

Aura has no structural typing. A type whose methods happen to have matching names does not satisfy a trait. It needs a visible, applicable `impl`.

## Generic Declarations

Classes, enums, functions, methods, and implementation blocks can declare type parameters:

```aura
class Box[T]:
    value: T

enum MaybePair[T]:
    One(T)
    Two(T, T)

def identity[T](value: own T) -> T:
    return value
```

- Type parameter names must be unique within their declaration.
- `Self` is reserved and cannot be declared as a type parameter.
- A generic use must supply exactly the declared number of type arguments.
- Generic arguments are invariant. Aura never widens them implicitly or converts them structurally.

A bound follows a type parameter after `:`. Join bounds with `+` to require every one of them:

```aura fragment
def use_value[T: Display + Score](value: T) -> int32:
    print(value.display())
    return value.score()
```

Classes and enums can carry bounds too. The checker enforces them when it resolves a construction, and when bounded generic operations use the specialized value:

```aura fragment
class NamedBox[T: Named]:
    value: T

enum MaybeNamed[T: Named]:
    Some(T)
    Empty
```

[Grammar](/manual/grammar#type-references-and-type-parameters) gives the exact parameter-list forms.

## Inference And Specialization

A generic call infers its type arguments by unifying the argument types with the parameter type patterns. An expected result type, when one is available, adds constraints. Generic class and enum construction infers the same way from the supplied fields or payloads and an expected constructed type.

```aura fragment
boxed = Box(value=7)          # Box[int64]
value = identity("Aura")   # str
```

Every declared type parameter must resolve. The checker does not invent a type for a parameter that appears in no supplied value and no expected context.

Parameter ownership is fixed at the generic declaration. An unconstrained `T` is not assumed copyable, so a bare `value: T` is a shared borrow. It stays a shared borrow even when a call later specializes `T` to a copy type. Write `value: own T` when the body consumes, stores, or returns the argument.

Explicit specialization fixes the type arguments:

```aura fragment
boxed = Box[int64](value=42)
value = identity[int64](42)
ok = Result[int32, str].Ok(7)
```

Explicit arguments must have the exact arity and satisfy every substituted bound. Specialization and indexing share bracket syntax. [Grammar](/manual/grammar#explicit-specialization) gives the parser rules that tell them apart.

### Type Parameters As Union Members

A declared type parameter can be a union member, and a generic alias can name such a union. Substitution renormalizes the union. With `V = int64 | None`, the union `V | None` is `int64 | None`. With `V = None`, it is unit `None`. A generic union never keeps a phantom outer tag to tell those results apart.

```aura
type MaybeValue[V] = V | None

def absent[V]() -> V | None:
    return None

def present[V](value: own V) -> V | None:
    return value

def main():
    first: MaybeValue[int64] = present[int64](4)
    if first is not None:
        print(first + 1)
    print(absent[int64 | None]() is None)
    print(present[int64 | None](None) is None)
```

This prints `5`, `true`, and `true`.

Inference binds a union-member parameter to the rest of the argument. Every concrete member of the parameter's union must be a member of the argument, and the remaining members bind the parameter:

- `V | None` infers `V = Dog` from `Dog` or from `Dog | None`.
- `V | None` infers `V = int64 | str` from `int64 | str | None`.

The checker rejects two cases with `AU2010`. One is an argument that leaves nothing for the parameter, such as `None`. The other is a union whose members could belong to more than one parameter. Specialize explicitly in both cases. The checker does not invert normalization.

Inside a generic body, the checker cannot assume that `V`, `W`, and `None` are distinct:

- On `V | None`, `value is None` narrows to `None` and `value is not None` narrows to `V`. That `V` is only the non-`None` refinement. The test does not prove that `V` itself excludes `None`.
- A `None` test on a bare `V` value is decided at run time. So is a `None` test on a union whose only `None` could come from a type parameter.
- A type arm `case V as inner` is rejected with `AU2013`, because `V` is not proved to be one member disjoint from every other arm. Write `case None` and a catch-all instead.

Concrete union properties come from the concrete members after specialization. An unresolved member grants nothing without a bound.

## Inferred Clone-Safety Obligations

Aura infers clone-safety obligations from the clone-producing operations in a callable's body. When such an operation works on an unresolved type parameter, the checker accepts it and records which declared parameters must be safe to clone. A concrete call discharges those obligations after substitution.

A type is safe to clone when duplicating it cannot duplicate `random.Rng` state through an ordinary class, enum, or collection path. `Task[T]` and `Queue[T]` handles stop this traversal, because an allowed handle copy does not observe or copy `T`. A clone barrier is not an escape from the task-boundary rule. Queue payloads and task results must separately be `Transfer`, and `Task[T]` is non-copy when `T` is not repeatable.

Obligations travel with the callable:

- A generic-to-generic call propagates the obligation to the caller.
- Inference runs to a fixed point and does not depend on declaration order.
- Imported functions and methods keep the resulting contract.
- The same rules apply to ordinary, inherent, associated, task-target, trait, operator, and `From` calls.

There is no source annotation for this obligation in Aura 0.3.

When a type is concrete, a substitution containing `random.Rng` is rejected with `AU3007`. A concrete type whose clone safety cannot be proved is also rejected, conservatively. Moving, removing, receiving, or rearranging one owned value does not produce a clone and adds no obligation.

### Verified Clone-Safety Contracts

These examples pin the observable boundary. The trait examples depend on the conformance rule in [Implementation Method Conformance](#implementation-method-conformance).

A generic clone helper is valid for a safe specialization:

```aura
def duplicate[T](values: list[T]) -> list[T]:
    return values.copy()

def main() -> int32:
    values = [1, 2]
    print(duplicate(values))
    return 0
```

The same callable rejects an unsafe concrete specialization:

```aura check-fail:AU3007
import random

def duplicate[T](values: list[T]) -> list[T]:
    return values.copy()

def reject(values: list[random.Rng]) -> list[random.Rng]:
    return duplicate(values)
```

The requirement survives a generic-to-generic call:

```aura check-fail:AU3007
import random

def duplicate[T](values: list[T]) -> list[T]:
    return values.copy()

def forward[T](values: list[T]) -> list[T]:
    return duplicate(values)

def reject(values: list[random.Rng]) -> list[random.Rng]:
    return forward(values)
```

A signature-only trait method does not let an implementation add a hidden requirement:

```aura check-fail:AU3007
trait Copier[T]:
    def copy_values(self) -> list[T]

class Wrapper[T]:
    values: list[T]

impl[T] Copier[T] for Wrapper[T]:
    def copy_values(self) -> list[T]:
        return self.values.copy()
```

A trait default body can establish the requirement for safe specializations:

```aura
trait Duplicator[T]:
    def duplicate(self, values: list[T]) -> list[T]:
        return values.copy()

class Marker[T]:
    value: T

impl[T] Duplicator[T] for Marker[T]:
    pass

def main() -> int32:
    marker = Marker(0)
    values = [4, 5]
    print(marker.duplicate(values))
    return 0
```

The same contract rejects its unsafe specialization:

```aura check-fail:AU3007
import random

trait Duplicator[T]:
    def duplicate(self, values: list[T]) -> list[T]:
        return values.copy()

class Marker[T]:
    value: T

impl[T] Duplicator[T] for Marker[T]:
    pass

def reject(marker: Marker[random.Rng], values: list[random.Rng]) -> list[random.Rng]:
    return marker.duplicate(values)
```

## Trait Declarations

A trait declares a nominal method contract:

```aura
trait Greeter:
    def greet(self) -> str
```

A trait method is either signature-only, ending at the newline, or has a default body after `:`:

```aura
trait Named:
    def name(self) -> str

    def label(self) -> str:
        return "name=" + self.name()
```

A marker trait contains `pass` and no required methods:

```aura
trait Marker:
    pass
```

Trait names and method names must be unique in their scopes. Trait type parameter lists use the plain parameter form:

```aura
trait Mapper[T]:
    def map(self, value: own T) -> T
```

A trait method's own generic parameters can have bounds. Ordinary trait method parameters cannot have defaults.

An obligation inferred from a trait default body is part of the trait method's contract. It is substituted through `Self`, the trait arguments, and the method type arguments, for every implementation and every form of dispatch. A signature-only trait method has no inferred clone-safety obligation.

A trait is private to its defining module unless it is declared `public trait`. Implementation blocks have no exported name of their own and cannot be prefixed with `public`. Their methods become available through the implemented public trait or type when the implementation is loaded.

## `Self`

`Self` denotes the implementing or enclosing concrete class specialization. It is available in the supported type positions of class, trait, and implementation methods:

```aura
trait Combine:
    def combine(self, other: Self) -> Self
```

- `Self` takes no type arguments.
- `Self` is not a global type. An unrelated top-level function cannot use it.
- Inside a trait declaration, `Self` starts as a placeholder. Inside an implementation, it is replaced by the implementation target.

## Implementations

An implementation attaches one trait specialization to one target type pattern:

```aura fragment
class Person:
    name: str

impl Greeter for Person:
    def greet(self) -> str:
        return "hello " + self.name
```

Implementations can be specialized or generic:

```aura fragment
impl Mapper[int32] for Doubler:
    def map(self, value: own int32) -> int32:
        return value * self.factor
```

```aura fragment
impl[T] Mapper[T] for Box[T]:
    def map(self, value: own T) -> T:
        return value
```

```aura fragment
impl Displayable for Box[str]:
    def display(self) -> str:
        return self.value.clone()
```

Target rules:

- The target must have a concrete or generic named outer type, such as `Box[T]`. A bare type parameter target, as in `impl[T] Trait for T`, is rejected.
- Implementation type parameters can have bounds.
- Every parameter used by the target or trait pattern must resolve during applicability checking.

Overlap rules:

- Two implementations with exactly the same trait specialization and target are duplicates. The checker rejects them.
- More general and more specialized overlapping patterns can coexist.
- Dispatch selects the unique applicable implementation with the greatest structural specificity. Equal-best matches are ambiguous and rejected.
- Source order never breaks a tie.

Aura 0.3 imposes no separate orphan rule. An implementation must still refer to known, visible types and traits. It takes part only where it is present in the loaded module or package context.

### Trait Methods On Unions

A union value dispatches a trait method when every member implements the same trait specialization with one contract. One contract means the same receiver mode, parameter modes and types, and owned result type after each member is substituted for `Self`. The active member's implementation runs:

- A `mut self` method mutates the payload in place, and the tag stays fixed.
- An `own self` method consumes the union.

A result written as `Self` that would differ per member is not one contract. Match the member and declare the result union explicitly. There is no duck typing over common method names, no `impl` for an anonymous union, and no inferred trait intersection.

The same all-member rule decides whether a union satisfies a trait bound. A union type argument satisfies `T: Named` when every member implements that specialization, and every method the bound exposes, supertraits included, has one coherent contract across the members.

- A member without the implementation reports `AU2002`.
- Members whose contracts differ report `AU2999`, the same code as a direct call on the union.

A bounded callee then dispatches each trait call on the active member, exactly as the direct call would.

## Implementation Method Conformance

An implementation can define only methods that belong to the trait. It must provide every signature-only method. It inherits a default method that it omits, and it can override one.

For each explicitly implemented method, conformance compares:

- receiver presence and passing mode: shared `self`, consuming `own self`,
  `mut self`, or none
- ordinary parameter count and substituted types
- each ordinary parameter's resolved owned, shared, or mutable access mode
- owned return type
- the trait method's substituted clone-safety obligations

Ordinary parameter names can differ between the trait and the implementation when their positions and types still match.

The checker rejects the following before any body runs:

- extra methods
- missing required methods
- receiver mismatches
- signature mismatches
- default ordinary arguments added by an implementation method

An explicit implementation MUST NOT strengthen its trait method's clone-safety contract. Its body can rely on obligations that the trait method already inferred. It cannot add a requirement that bound-based callers cannot see. Aura 0.3 has no explicit clone-safety annotation, so generic clone-producing behavior belongs in a trait default body. An implementation that adds such a requirement is rejected with `AU3007`.

An `impl` targeting any builtin type MUST NOT explicitly define or inherit a trait method whose name is a builtin member of that target. This covers:

- the runtime handles `Queue[T]`, `Task[T]`, `TaskGroup`, `random.Rng`, `fs.File`, and the `net` and `process` handles
- the builtin value types, such as `str`, `list[T]`, `dict[K, V]`, `set[T]`, `Duration`, and the scalar types

Builtin member names are reserved for their runtime operation. A collision reports `AU2006`. Rename the trait method to fix it. The check runs after default trait methods are inherited. A trait method whose name does not collide implements and dispatches normally on a builtin target.

## Trait Method Dispatch

For a concrete value, member lookup considers inherent class methods and applicable visible trait implementations. The selected method keeps its declared receiver and argument ownership behavior.

A trait method selected for a concrete value also binds as a method value. Outside a call, `value.method` is a closure over the receiver with the method's complete contract. The receiver rules are in [Closures](/manual/closures#bound-methods).

For a type parameter, only the methods justified by its declared bounds are available:

```aura fragment
def say_hello[T: Greeter](value: T):
    print(value.greet())
```

A specialized trait bound supplies its type arguments:

```aura fragment
def apply[M: Mapper[int32]](mapper: M, value: int32) -> int32:
    return mapper.map(value)
```

A call is ambiguous and rejected when several bounds, or several equally specific implementations, expose an indistinguishable applicable method.

Traits can also declare associated methods without `self`:

```aura fragment
trait Factory:
    def make() -> int32

impl Factory for Widget:
    def make() -> int32:
        return 7

value = Widget.make()
```

Associated trait methods follow the same rules as receiver methods. Concrete and bound-based dispatch enforce the same substituted clone-safety contract.

## Supertraits

A trait can require one or more supertraits:

```aura fragment
trait Labelled: Named:
    def label(self) -> str:
        return "name=" + self.name()
```

The second colon ends the header. Separate multiple supertraits with commas.

`impl Labelled for User` is valid only when `User` also satisfies `Named` through an applicable implementation. Implementing the child trait does not create the parent implementation. A child bound makes the supertrait methods available, and default child methods can call them.

Supertrait types must name known traits with exact arity. Bound and dispatch checking closes the requirements transitively.

## Operator Traits

When no builtin numeric or string operator rule applies, an operator requests a trait method:

| Source operator | Trait method |
| --- | --- |
| `left + right` | `Add.add` |
| `left - right` | `Sub.sub` |
| `left * right` | `Mul.mul` |
| `left / right` | `Div.div` |
| `left // right` | `FloorDiv.floor_div` |
| `left % right` | `Mod.mod` |
| `-value` | `Neg.neg` |
| `not value` | `Not.not` |
| `<`, `<=`, `>`, `>=` | `Ord.lt`, `Ord.le`, `Ord.gt`, `Ord.ge` |

Builtin rules come first:

- Builtin numeric `//` and the heterogeneous builtin `Duration // int64` rule take precedence over trait dispatch. Otherwise `//` and `//=` request an applicable `FloorDiv.floor_div` implementation.
- `/` on equal integer operands is rejected with the integer-division teaching diagnostic. It is not dispatched to `Div.div`. `/` still requests `Div.div` for an applicable non-numeric user type.
- The divisor-sign rule for `%` describes builtin numeric remainder. `Mod.mod` on a user type has the semantics of that implementation.

These declarations show the maintained generic shapes:

```aura
trait Add[Rhs, Out]:
    def add(self, rhs: Rhs) -> Out

trait FloorDiv[Rhs, Out]:
    def floor_div(self, rhs: Rhs) -> Out

trait Neg[Out]:
    def neg(self) -> Out

trait Ord[Rhs]:
    def lt(self, rhs: Rhs) -> bool
    def le(self, rhs: Rhs) -> bool
    def gt(self, rhs: Rhs) -> bool
    def ge(self, rhs: Rhs) -> bool
```

`Sub`, `Mul`, `Div`, and `Mod` use the same binary `Rhs, Out` shape as `Add` and `FloorDiv`. `Not` uses the unary `Out` shape. Ordering methods must return `bool`.

Some operators never use traits:

- `and` and `or` do not dispatch through traits.
- Builtin `==` and `!=` do not use an equality trait in Aura 0.3. This includes recursive structural tuple equality, which a trait implementation cannot override.
- Builtin operations take precedence wherever their concrete value rule applies.

An operator that selects a trait method also enforces that method's substituted clone-safety obligations.

## `From` And `try`

When `try` propagates `Result[T, SourceError]` from a function that returns `Result[U, TargetError]`:

- If the error types are equal, no trait is needed.
- Otherwise the checker looks for an applicable `impl From[SourceError] for TargetError` that contains `from`.

The conventional contract is:

```aura
trait From[Source]:
    def from(value: own Source) -> Self
```

The checker enforces the selected `From.from` method's clone-safety obligations before it accepts the conversion. At run time the conversion runs before `Result.Err` is returned from the enclosing function. If no applicable conversion exists, `try` is rejected. See [Functions](/manual/functions#try-and-result-returns).

## Current Generic And Trait Boundaries

- Generic arguments are invariant, and there is no general subtyping.
- Type inference is local and contextual, not whole-program.
- Trait and implementation methods cannot give ordinary parameters defaults.
- Generic user classes cannot serve as `with` resources.
- Generic task targets are permitted when their callable type arguments can be resolved. Bare shared and `own` targets use task-owned captures. `mut` targets are rejected.
- Equal-specificity overlapping implementations are an error at the use site.
- Clone-safety obligations are inferred, not written. An explicit implementation cannot strengthen the contract inferred by its trait method.

[Current Limits](/manual/current-limits) collects the observable syntax and implementation limits. [Static Semantics](/manual/static-semantics#generics-traits-and-implementations) has the cross-cutting type rules.

## Grammar

[Grammar](/manual/grammar) has the normative productions for type parameters, bounds, explicit specialization, trait declarations, supertraits, `Self`, and implementation blocks.

Classes, enums, functions, methods, traits, and implementations use the declaration-specific parameter forms shown above. Trait methods are signature-only or have a default suite. Implementation methods always use ordinary method-definition syntax.

## Typing Rules

- Generic arguments are invariant and have exact arity.
- Inference is local and contextual. It must resolve every declared parameter and satisfy every substituted bound.
- A parameter that is a union member binds to the argument's remaining members. Unions are renormalized after every substitution.
- A generic body is checked once, universally, before any specialization.
- Trait satisfaction is nominal, through a visible applicable `impl`. It is never structural.
- An implementation must conform after substitution in receiver mode, parameter modes and types, owned return type, clone-safety obligations, and supertrait requirements.
- Dispatch selects one unique applicable implementation of greatest specificity. Equal-best matches are rejected.
- `Self` denotes the enclosing or implementing concrete specialization, and only in its supported declaration contexts.

## Runtime Semantics

Generic construction and calls use the statically resolved specialization. There is no runtime generic inference.

A union value built inside a generic body keeps the body's symbolic member layout until a concrete operation reads it. Every union operation aligns the value to the union it names, by the active member's identity. A `V`-typed union payload flattens into its destination without a nested wrapper. So both execution paths agree on the one normalized value.

Trait member calls and operator calls invoke the statically selected implementation. When the implementation omits a method, the trait default body runs. Source order never resolves overlapping implementations.

`try` invokes the selected `From[Source]` conversion before it constructs the enclosing `Result.Err`.

Traits do not create runtime reflection, dynamic method dictionaries, or implicit conversions.

## Ownership And Evaluation Order

Parameter ownership is resolved at the generic declaration and stays stable after specialization. An unresolved bare `T` is shared, even when a later substitution is a copy type. `own T` is the explicit consuming form. Trait and implementation signatures must agree on that resolved mode.

The receiver is evaluated before the ordinary arguments. A selected method keeps its declared receiver and parameter behavior. `From.from` owns its source error.

No generic or trait boundary inserts a hidden clone, coercion, or ownership-mode change. Clone-producing bodies infer obligations, generic calls propagate them, and concrete dispatch discharges them after substitution.

## Diagnostics

| Code | Cause |
| --- | --- |
| `AU1101` | Malformed generic, trait, supertrait, specialization, or implementation syntax. |
| `AU2001` | Unknown type, trait, method, or member. |
| `AU2002` | Inference failure, wrong generic arity, unsatisfied bound, missing trait satisfaction, ambiguous equal-specificity dispatch, invalid specialization, or substituted type mismatch. |
| `AU2003` | An operator that no builtin rule or applicable operator trait supplies. |
| `AU2004` | Call argument binding, and ordinary default arguments in trait methods. |
| `AU2006` | A trait method name that collides with a builtin member. Rename the trait method. |
| `AU2010` | A type parameter that a union-member argument cannot determine. Specialize explicitly. |
| `AU2013` | A type arm over a type parameter. Use `case None` and a catch-all. |
| `AU2999` | Duplicate or invalid implementations, method-conformance or supertrait failure, unsupported implementation targets, and other generic or trait rejections. |
| `AU3001` | Use after an owned generic value or receiver has moved. |
| `AU3002` | A borrow conflict, or storing through a bare shared generic parameter. |
| `AU3003` | A mutable receiver call through an immutable place. |
| `AU3004` | An invalid ownership mode. |
| `AU3007` | An unsafe concrete clone specialization, an unprovable concrete requirement, or an implementation that would strengthen its trait method's clone-safety contract. |

A selected body keeps its own runtime diagnostics:

| Code | Cause |
| --- | --- |
| `AU4001` | General trap. |
| `AU4002` | Arithmetic overflow or underflow. |
| `AU4003` | Bounds or lookup violation. |
| `AU4004` | Zero divisor. |
| `AU4005` | Resource or I/O failure. |

## Backend Support

MIR execution and direct native generation both implement generic functions, classes, enums, and methods, traits, supertraits, default trait bodies, generic and specialized implementations, operator dispatch, `Self`, and `From` conversion. MIR is the compiler's mid-level intermediate representation.

Both backends maintain user-trait dispatch on builtin values for noncolliding method names. That includes `Queue[T]`, `Task[T]`, `TaskGroup`, `random.Rng`, `str`, and the builtin collections. Builtin target members always keep builtin dispatch.

The checker gives lowering, analysis, and the language server one resolved specialization and implementation target, including inferred clone-safety obligations. The parity gate rejects dispatch behavior that differs between backends.

## Limits And Implementation-Defined Behavior

Aura 0.3 has none of the following:

- trait objects or dynamic dispatch
- associated types or constants
- higher-kinded parameters
- default type arguments
- `where` clauses
- specialization annotations
- general subtyping
- a separate orphan-rule restriction

A bare target parameter in `impl[T] Trait for T` is unsupported. Equal-specificity overlaps are errors. Ordinary trait and implementation parameters cannot add defaults. Generic user classes cannot be `with` resources.

The rules above define inference and dispatch. Source order and backend implementation choices do not.

## Status

Implemented:

- invariant generics, local and contextual inference, and explicit specialization
- nominal traits and bounds, supertraits, and default methods
- generic and specialized implementations with unique-most-specific dispatch
- operator traits, `Self`, and `From`-based `try` conversion
- inferred clone-safety contracts
- type parameters as union members, renormalizing specialization, remainder inference, and universal generic body checks

Ordinary `-> T` return values are owned. Generic functions and methods can instead declare `-> view [mut] T from origin`. Trait implementations preserve the trait declaration's origin slot as well as its specialized pointee type.

Trait objects, dynamic dispatch, associated types, higher-kinded types, general subtyping, and arbitrary blanket implementation targets are unavailable.

## Design Record

- [ADR-0033: Structural Transfer and task-result consumption](https://github.com/johnolafenwa/Aura/blob/main/architecture_docs/decisions/0033-structural-transfer-and-task-results.md)
