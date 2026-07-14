# Generics And Traits

Generics parameterize declarations over types. Traits are nominal interfaces used for generic bounds, method dispatch, operator dispatch, supertrait requirements, and `try` error conversion.

Aurora does not use structural typing: having methods with matching spellings does not satisfy a trait. A visible applicable `impl` is required.

## Generic Declarations

Classes, enums, functions, methods, and implementation blocks may declare type parameters:

```python
class Box[T]:
    value: T

enum MaybePair[T]:
    One(T)
    Two(T, T)

def identity[T](value: own T) -> T:
    return value
```

Type parameter names must be unique within their declaration. `Self` is reserved and cannot be declared as a type parameter. A generic use must provide exactly the declared arity; generic arguments are invariant and are never implicitly widened or structurally converted.

Bounds follow a type parameter after `:`. `+` means every listed bound is required:

```python
def use_value[T: Display + Score](value: borrow T) -> int32:
    print(value.display())
    return value.score()
```

Classes and enums may also carry bounds. The checker enforces them when resolving construction and when the specialized value is used through bounded generic operations:

```python
class NamedBox[T: Named]:
    value: T

enum MaybeNamed[T: Named]:
    Some(T)
    Empty
```

The exact parameter-list forms are in [Grammar](/manual/grammar#type-references-and-type-parameters).

## Inference And Specialization

Generic calls infer substitutions by unifying argument types with parameter type patterns. An available expected result type may add constraints. Generic class and enum construction similarly use provided fields/payloads and an expected constructed type.

Parameter ownership is resolved at the generic declaration. Because an
unconstrained `T` is not assumed copyable, a bare `value: T` is a shared borrow
and remains declaration-stable even when a call later specializes `T` to a
copy type. Use `value: own T` when the generic body consumes, stores, or returns
the argument.

```python
boxed = Box(value=7)          # Box[int64]
value = identity("Aurora")   # String
```

Every declared type parameter must resolve. The checker does not invent a type for a parameter that appears nowhere in supplied values or expected context.

Explicit specialization fixes the arguments:

```python
boxed = Box[int64](value=42)
value = identity[int64](42)
ok = Result[int32, String].Ok(7)
```

Explicit arguments must have exact arity and satisfy all substituted bounds. Specialization and indexing share bracket syntax; the parser rules that distinguish them are specified in [Grammar](/manual/grammar#explicit-specialization).

## Trait Declarations

A trait declares a nominal method contract:

```python
trait Greeter:
    def greet(borrow self) -> String
```

Trait methods may be signature-only, ending at the newline, or may provide a default body after `:`:

```python
trait Named:
    def name(borrow self) -> String

    def label(borrow self) -> String:
        return "name=" + self.name()
```

A marker trait contains `pass` and no required methods:

```python
trait Marker:
    pass
```

Trait names and method names must be unique in their scopes. Trait type parameter lists use the plain parameter form:

```python
trait Mapper[T]:
    def map(borrow self, value: own T) -> T
```

Bounds may appear on a trait method's own generic parameters. Ordinary trait method parameters cannot have defaults.

A trait is private to its defining module unless declared `public trait`. Implementation blocks have no independent exported name and cannot be prefixed with `public`; their methods become available through the implemented public trait/type context when the implementation is loaded.

## `Self`

`Self` denotes the implementing or enclosing concrete class specialization in supported class, trait, and implementation method type positions:

```python
trait Combine:
    def combine(borrow self, other: borrow Self) -> Self
```

`Self` takes no type arguments. It is not a global type and is unavailable in an unrelated top-level function. Inside a trait declaration it is initially a placeholder; inside an implementation it is substituted with the implementation target.

## Implementations

An implementation attaches one trait specialization to one target type pattern:

```python
class Person:
    name: String

impl Greeter for Person:
    def greet(borrow self) -> String:
        return "hello " + self.name
```

Generic and specialized implementations are supported:

```python
impl Mapper[int32] for Doubler:
    def map(borrow self, value: own int32) -> int32:
        return value * self.factor
```

```python
impl[T] Mapper[T] for Box[T]:
    def map(borrow self, value: own T) -> T:
        return value
```

```python
impl Displayable for Box[String]:
    def display(borrow self) -> String:
        return self.value.clone()
```

An implementation target must have a concrete or generic named outer type such as `Box[T]`; a bare target type parameter in `impl[T] Trait for T` is rejected. Implementation type parameters may have bounds, and every parameter used by the target/trait pattern must resolve during applicability checking.

Two implementations with exactly the same trait specialization and target are duplicates and are rejected. More general and more specialized overlapping patterns may coexist. Dispatch selects the unique applicable implementation with greatest structural specificity; equal-best matches are ambiguous and rejected. Source order is never a tie breaker.

Aurora 0.1 does not impose a separate orphan-rule restriction, but an implementation must refer to known visible types and traits and participates only where that implementation is present in the loaded module/package context.

## Implementation Method Conformance

An implementation may define only methods belonging to the trait. It must provide every signature-only required method; a trait method with a default body is inherited when omitted. An implementation may override a default method.

For an explicitly implemented method, conformance compares:

- receiver presence and passing mode (shared `self`/`borrow self`, consuming `own self`, `borrow mut self`, or none)
- ordinary parameter count and substituted types
- each ordinary parameter's resolved owned/shared-borrow/mutable-borrow mode
- return type and owned/shared-borrow/mutable-borrow mode
- the semantic source slot of a borrowed return

Ordinary parameter names and borrow-label spellings may differ between the trait and implementation when they identify the same parameter position. Changing which parameter supplies a borrowed result is a signature mismatch.

Aurora 0.1 retains these borrowed-return conformance rules even though only copy-valued borrowed-return calls are executable. Calls producing non-copy borrowed results are rejected before backend lowering.

Implementation methods cannot add default ordinary arguments. Extra methods, missing required methods, receiver mismatches, and signature mismatches are rejected before body execution.

## Trait Method Dispatch

For a concrete value, member lookup considers inherent class methods and applicable visible trait implementations. The selected method keeps its declared receiver and argument ownership behavior.

For a type parameter, only methods justified by declared bounds are available:

```python
def say_hello[T: Greeter](value: borrow T):
    print(value.greet())
```

Specialized trait bounds provide their type arguments:

```python
def apply[M: Mapper[int32]](mapper: borrow M, value: int32) -> int32:
    return mapper.map(value)
```

If multiple bounds or equally specific implementations expose an indistinguishable applicable method, the call is ambiguous and rejected.

Traits may also declare associated methods without `self`:

```python
trait Factory:
    def make() -> int32

impl Factory for Widget:
    def make() -> int32:
        return 7

value = Widget.make()
```

## Supertraits

A trait may require one or more supertraits:

```python
trait Labelled: Named:
    def label(borrow self) -> String:
        return "name=" + self.name()
```

The second colon terminates the header. Multiple supertraits are comma-separated.

An `impl Labelled for User` is valid only when the same target also satisfies `Named` through an applicable implementation. Implementing the child does not synthesize the parent implementation. Supertrait methods are available through a child bound, and default child methods may call them.

Supertrait types must name known traits with exact arity. Requirements are transitively closed during bound and dispatch checking.

## Operator Traits

When builtin numeric/string operator rules do not apply, these operator spellings request traits and method names:

| Source operator | Trait method |
| --- | --- |
| `left + right` | `Add.add` |
| `left - right` | `Sub.sub` |
| `left * right` | `Mul.mul` |
| `left / right` | `Div.div` |
| `left % right` | `Mod.mod` |
| `-value` | `Neg.neg` |
| `not value` | `Not.not` |
| `<`, `<=`, `>`, `>=` | `Ord.lt`, `Ord.le`, `Ord.gt`, `Ord.ge` |

There is no `FloorDiv` operator trait. `//` and `//=` are builtin numeric
floor-division spellings only. Equal integer operands with `/` are rejected
with the integer-division teaching diagnostic rather than dispatched to
`Div.div`; `/` can still request `Div.div` for an applicable non-numeric user
type. The divisor-sign rule for `%` describes builtin numeric remainder;
`Mod.mod` on a user type has the semantics of that implementation.

The maintained generic shapes are illustrated by:

```python
trait Add[Rhs, Out]:
    def add(borrow self, rhs: Rhs) -> Out

trait Neg[Out]:
    def neg(borrow self) -> Out

trait Ord[Rhs]:
    def lt(borrow self, rhs: Rhs) -> bool
    def le(borrow self, rhs: Rhs) -> bool
    def gt(borrow self, rhs: Rhs) -> bool
    def ge(borrow self, rhs: Rhs) -> bool
```

`Sub`, `Mul`, `Div`, and `Mod` follow the binary `Rhs, Out` shape; `Not` follows the unary `Out` shape. Ordering methods must return `bool`.

`and` and `or` do not dispatch through traits. Builtin `==` and `!=` also do not use an equality trait in Aurora 0.1. Builtin operations take precedence where their concrete scalar/string rule applies.

## `From` And `try`

When `try` propagates `Result[T, SourceError]` from a function returning `Result[U, TargetError]`, exact error-type equality needs no trait. Otherwise the checker looks for an applicable `impl From[SourceError] for TargetError` containing `from`.

The conventional contract is:

```python
trait From[Source]:
    def from(value: own Source) -> Self
```

The selected conversion runs before `Result.Err` is returned from the enclosing function. If no applicable conversion exists, `try` is rejected. See [Functions](/manual/functions#try-and-result-returns).

## Current Generic And Trait Boundaries

- generic arguments are invariant and there is no general subtyping
- type inference is local/contextual rather than whole-program inference
- trait and implementation method defaults for ordinary parameters are not supported
- generic user classes cannot currently serve as `with` resources
- generic task targets are permitted when their callable type arguments can be
  resolved; default/shared and `own` targets use task-owned captures, while
  `borrow mut` targets are rejected
- equal-specificity overlapping implementations remain an error at the use site

Observable syntax and implementation limits are collected in [Current Limits](/manual/current-limits), while cross-cutting type rules are in [Static Semantics](/manual/static-semantics#generics-traits-and-implementations).

## Grammar

The normative productions for type parameters, bounds, explicit
specialization, trait declarations, supertraits, `Self`, and implementation
blocks are in [Grammar](/manual/grammar). Classes, enums, functions, methods,
traits, and implementations use the declaration-specific parameter forms
shown above. Trait methods may be signature-only or have a default suite;
implementation methods always use ordinary method-definition syntax.

## Typing Rules

Generic arguments are invariant and have exact arity. Inference is local and
contextual, must resolve every declared parameter, and must satisfy every
substituted bound. Trait satisfaction is nominal through a visible applicable
`impl`, never structural. Implementations must conform after substituting
receiver mode, parameter modes and types, return mode and type, borrowed-return
source, and supertrait requirements. Dispatch selects one unique
greatest-specificity applicable implementation; equal-best matches are
rejected. `Self` denotes the enclosing/implementing concrete specialization
only in its supported declaration contexts.

## Runtime Semantics

Generic construction and calls use the statically resolved specialization;
there is no runtime generic inference. Trait member and operator calls invoke
the statically selected implementation, inheriting a trait default body when
the implementation omits that method. Source order never resolves overlapping
implementations. `try` invokes the selected `From[Source]` conversion before
constructing the enclosing `Result.Err`. Traits do not create runtime
reflection, dynamic method dictionaries, or implicit conversions.

## Ownership And Evaluation Order

Parameter ownership is resolved at the generic declaration and remains stable
after specialization: an unresolved bare `T` is shared, even when one later
substitution is copy, while `own T` is the explicit consuming form. Trait and
implementation signatures must agree on that resolved mode. Receiver
evaluation precedes ordinary arguments, selected methods keep their declared
receiver/parameter behavior, and `From.from` owns its source error. No generic
or trait boundary inserts a hidden clone, coercion, or ownership-mode change.

## Diagnostics

`AU1101` reports malformed generic, trait, supertrait, specialization, or
implementation syntax. `AU2001` reports unknown types, traits, methods, and
members. `AU2002` covers inference failure, generic arity, unsatisfied bounds,
missing trait satisfaction, ambiguous equal-specificity dispatch, invalid
specialization, and substituted type mismatch. `AU2003` reports an unsupported
operator when no builtin rule or applicable operator trait supplies it.
`AU2004` reports call argument binding and the prohibition on ordinary default
arguments in trait methods. `AU2999` covers duplicate/invalid implementations,
method-conformance or supertrait failure, unsupported implementation targets,
and remaining generic/trait rejections. `AU3001` reports use after an owned
generic or receiver move. `AU3002` reports borrow conflicts, storing through a
default-borrowed generic parameter, or contained non-copy borrowed returns.
`AU3003` reports a mutable receiver call through an immutable place, and
`AU3004` reports an invalid ownership mode. A selected body retains its runtime
diagnostic: `AU4001` for a general trap, `AU4002` for arithmetic overflow or
underflow, `AU4003` for a bounds or lookup violation, `AU4004` for a zero
divisor, and `AU4005` for a resource or I/O failure.

## Backend Support

Generic functions, classes, enums, methods, traits, supertraits, default trait
bodies, generic and specialized implementations, operator dispatch, `Self`,
and `From` conversion are implemented for MIR execution and direct native
generation. User-trait dispatch on builtin `Queue[T]` and `Task[T]` handles is
also maintained on both backends. The checker supplies one resolved
specialization and implementation target to lowering, analysis, and the LSP;
the parity gate rejects backend-specific dispatch behavior.

## Limits And Implementation-Defined Behavior

Aurora 0.1 has no trait objects, dynamic dispatch, associated types or
constants, higher-kinded parameters, default type arguments, `where` clauses,
specialization annotations, general subtyping, or separate orphan-rule
restriction. A bare target parameter in `impl[T] Trait for T` is unsupported.
Equal-specificity overlaps remain errors, ordinary trait/impl parameters cannot
add defaults, generic user classes cannot be `with` resources, and calls
producing non-copy borrowed results are contained. Inference and dispatch are
defined by the rules above rather than source order or backend implementation
choice.

## Status

Invariant generics, local/contextual inference, explicit specialization,
nominal traits and bounds, supertraits, default methods, generic and specialized
implementations, unique-most-specific dispatch, operator traits, `Self`, and
`From`-based `try` conversion are implemented for the post-Phase 1.5 surface.
Live non-copy borrowed results are reserved for the Phase 6 alias work. Trait
objects, dynamic dispatch, associated types, higher-kinded types, general
subtyping, and arbitrary blanket implementation targets are unavailable.
