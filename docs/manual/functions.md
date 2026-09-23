# Functions

Functions are module-level declarations introduced by `def`. A function's callable contract applies at every call site. It fixes:

- the parameter names, passing modes, and types
- the generic parameters and their bounds
- the return behavior
- any inferred clone-safety obligations

```aura
def add(left: int32, right: int32) -> int32:
    return left + right
```

[Grammar](/manual/grammar#functions-methods-and-parameters) has the complete declaration grammar. This page defines the matching static and runtime rules.

## Signatures And Return Types

Every ordinary parameter has an explicit type. The return annotation is optional, and omitting it is exactly the same as writing `-> None`. An ordinary `-> T` return is owned. A returned view uses the separate `-> view [mut] T from origin` form.

```aura
def square(value: int32) -> int32:
    return value * value

def log(message: str):
    print(message)
```

`return expression` must have exactly the declared return type. `return` without an expression has type `None` and is valid only in a `None`-returning function. Reaching the end of a `None` function returns `None` implicitly.

A function with any other return type must return on every statically reachable path:

```aura
def classify(value: int32) -> str:
    if value < 0:
        return "negative"
    return "non-negative"
```

A function may return one fixed structural tuple. The return annotation and the returned value both use parentheses. A comma distinguishes a singleton tuple from grouping. For example, `def locate() -> (str, int64):` may `return ("north", 7)`, and the caller may bind the result with `name, number = locate()`. Whether a returned tuple copies or moves follows the recursive classification of the whole tuple. See [Tuples](/manual/tuples).

Aura has no implicit numeric widening and no general return coercion. A bare `None` or a member value in an argument or return position adopts an expected `T | None` union. Grouping does not discard that context. A function declared `-> T | None` returns absence with an explicit `return None`. [Static Semantics](/manual/static-semantics#contextual-inference) covers other contextual literal typing and the union equality rule.

Function names share the module item namespace with classes, enums, traits, and imports. The checker rejects duplicate items and any attempt to redefine a maintained builtin function name. Ordinary parameter names must be unique. When a method has a receiver, no other parameter can be named `self`. In a method declaration, `self: Type` is rejected, not treated as an ordinary first parameter. Receivers use `self`, `own self`, or `mut self`. See [Names And Scopes](/manual/names-and-scopes) for the complete namespace rules.

A function is private to its defining module by default. Prefix the declaration with `public` to make it importable from another module:

```aura
public def double(value: int32) -> int32:
    return value * 2
```

Visibility controls name access. It does not change the ownership or type rules of the signature.

## Parameter Passing Modes

The passing mode is part of the function signature:

| Declaration | Contract at the call boundary |
| --- | --- |
| `value: T` | Shared access. An implementation may pass copy bits directly without changing the source contract. |
| `value: own T` | Owned argument. A move value is consumed. A copy value is duplicated. |
| `value: mut T` | Exclusive mutable borrow. The argument must be a mutable place. |

```aura
def consume(name: own str):
    print(name)

def length(text: str) -> int64:
    return text.len()

def push_name(names: mut list[str], name: own str):
    names.append(name)
```

Write the modifier in the declaration, after the colon. A call passes the expression directly, because Aura has no call-site capability prefix:

```aura fragment
mut names = list[str]()
push_name(names, "Ada")
```

Each argument must have exactly the substituted parameter type.

A call keeps its access to each non-copy method receiver and non-copy argument while it evaluates every later sibling expression. A later sibling may take a shared borrow of the same place, including through an access nested inside another call or expression. It may not mutably borrow or consume an overlapping place. A violation reports `AU3002`. [Ownership And Borrowing](/manual/ownership-and-borrowing) specifies the ownership and place rules.

The meaning of a bare parameter is fixed where the function is declared, not separately at each call. An unconstrained generic `value: T` is therefore a shared borrow, because `T` is not known to be copyable at the declaration. This choice is declaration-stable: specializing the function later with `T = int32` does not turn the parameter into an owned value. Write `value: own T` when a generic function must consume or return its argument.

## Call Binding

A call takes positional arguments followed by named arguments:

```aura
def render(name: str, count: int32 = 1):
    print(name)

render("Aura")
render("Aura", 2)
render(name="Aura", count=2)
```

Binding is deterministic:

1. positional arguments fill parameters in declaration order
2. named arguments fill the parameter with the same name
3. one parameter cannot be filled twice
4. unknown names and excess positional arguments are rejected
5. every omitted parameter must have a default
6. each bound argument must have the parameter's exact substituted type

A positional argument cannot follow a named argument. Parameter and argument lists may span physical lines while their parentheses are open. They do not accept trailing commas in Aura 0.3.

### Keyword-Only Parameters

A `*` boundary in a parameter list makes every parameter after it keyword-only. A keyword-only parameter binds by name only:

```aura
def configure(path: str, *, retries: int32 = 2, verbose: bool = false) -> int32:
    if verbose:
        print(path)
    return retries

configure("cfg")
configure("cfg", retries=5)
configure("cfg", verbose=true, retries=1)
```

- A positional argument that would reach a keyword-only parameter reports `AU2004` with ``parameter `x` of function `f` is keyword-only``.
- Omitting a required keyword-only parameter reports the missing name.
- A parameter list may have at most one `*`. At least one named parameter must follow it. It adds no variadic parameters.
- The boundary is part of the function's complete callable contract. See [Function Values](#function-values). A trait implementation must place it exactly where the trait declaration does.

## Default Arguments

A default is permitted on a bare shared or `own` parameter of a top-level function or class method:

```aura
def greet(name: str = "world"):
    print("hello " + name)
```

The complete rules are:

- A `mut` parameter cannot have a default, whether or not its type is copyable. The default would be a temporary the caller cannot see, so every mutation would be a silently lost write. Require the caller to pass a value, or take the parameter as `own T` and return the result.
- A shared-borrow default is permitted. Its temporary lives until the call completes.
- An `own` default is permitted. The call consumes its fresh temporary.
- After the first defaulted positional parameter, every remaining positional parameter must also have a default. Keyword-only parameters bind by name, so a required keyword-only parameter may follow a defaulted one.
- The default expression must have exactly the declared parameter type.
- A default expression cannot reference any parameter of the same declaration, including an earlier parameter.
- Trait method declarations and trait implementation methods cannot declare defaults.

A default is evaluated afresh each time its argument is omitted. It is not a process-global singleton value. A call evaluates its arguments in this order:

1. Supplied arguments evaluate in call-site source order. Each one finishes before the next supplied expression begins.
2. Defaults for omitted parameters then evaluate in declaration order.

A copy or move result is captured in its parameter slot. A borrow-mode selection is established without cloning and stays subject to the retained non-copy overlap rules. Later side effects cannot change an argument that is already captured. Binding named values to parameter slots never reorders their evaluation. A supplied argument suppresses its default. See [Execution Model](/manual/execution-model#evaluation-order).

## Named Arguments For Builtins

Maintained builtin functions and methods use the same binding rules. Their parameter names come from their API metadata:

```aura
import process

process.run(["/bin/echo", "hi"], stdout=process.pipe(), group=true)
```

```aura
import net

net.http_request_text_timeout(method="POST", url="http://127.0.0.1:8080/jobs", body="{}", headers={}, timeout=2s)
```

The module pages and the [API Index](/manual/api-index) are authoritative for builtin parameter names, defaults, and return types.

## `try` And Result Returns

`try` is valid only when its operand has type `Result[T, E1]` and the enclosing function returns `Result[U, E2]`:

```aura
def parse_total(left: str, right: str) -> Result[int32, str]:
    a = try parse_int32(left)
    b = try parse_int32(right)
    return Result.Ok(a + b)
```

`Result.Ok(value)` makes `try` evaluate to `value`. `Result.Err(error)` returns from the enclosing function immediately. `E1` must equal `E2`, or an applicable `impl From[E1] for E2` with a `from` method must be visible. Active `with` cleanups run during this early return. See [Execution Model](/manual/execution-model#try).

## Owned Returns

Every ordinary return type annotation describes an owned result:

```aura
class User:
    name: str
    score: int32

def score(user: User) -> int32:
    return user.score
```

Here the caller receives an ordinary `int32` copy. A copy result needs no provenance annotation, because it is an independent owned value.

For a non-copy result, the function must produce ownership. It can do that in four ways:

- construct a fresh value
- clone a clone-safe value
- move from an `own` parameter
- call an operation that consumes an owner

```aura fragment
def copy_name(user: User) -> str:
    return user.name.clone()

def into_name(user: own User) -> str:
    return user.name
```

A bare or `mut` parameter grants access. It does not give the function ownership of a non-copy value stored behind that access. The checker rejects moving such a value into the result. Use one of the owned-result patterns above instead. See [Ownership And Borrowing](/manual/ownership-and-borrowing#owned-returns).

## Returned Views

A function or method can return non-owning access to one declared receiver or parameter, called its origin:

    class User:
        name: str

    class Counter:
        value: int64

    def name(user: User) -> view str from user:
        return view user.name

    def value_mut(counter: mut Counter) -> view mut int64 from counter:
        return view mut counter.value

    def bump(value: mut int64):
        value += 1

The signature rules are:

- The `from` origin is mandatory and belongs to the signature.
- The origin is recorded by receiver or parameter slot, across modules and in trait conformance. An implementation may rename its parameter but may not select another slot.
- A shared result may use a bare or `mut` origin. A mutable result requires a `mut` origin.
- Owned and defaulted parameters cannot be origins.

Every reachable `return view` must trace to the origin or one of its supported fixed field or tuple projections. Locals, temporaries, enum arm payloads, and newly allocated values cannot escape. Return paths may select different fixed projections of the one declared origin.

At a call site, these rules apply:

- The origin argument must be an addressable place.
- A returned view may initialize a matching local `view` or `view mut` binding.
- A shared result may instead be read directly within one containing expression.
- A mutable result may instead be reborrowed immediately into a `mut` call.
- A returned view cannot initialize an ordinary owned binding or be stored inside an aggregate.

These calls use the declarations above:

    user = User(name="Ada")
    view current = name(user)
    print(current)
    print(name(user))

    mut counter = Counter(value=0)
    view mut editable = value_mut(counter)
    editable += 1
    print(counter.value)
    bump(value_mut(counter))
    print(counter.value)

The caller locks the origin conservatively, and execution keeps the exact projection that was selected. Selecting a different root reports `AU3010`. Structural `def(...) -> R` types cannot represent returned-view origin metadata, so they do not accept these functions.

## Generic Functions

Type parameters follow the function name:

```aura
def identity[T](value: own T) -> T:
    return value
```

Bounds restrict substitutions:

```aura fragment
def describe[T: Greeter](value: T) -> str:
    return value.greet()

def use_both[T: First + Second](value: T) -> int32:
    return value.score()
```

The checker infers type arguments from the call arguments and from an expected result type when one is available. Explicit specialization fixes them:

```aura fragment
answer = identity[int64](42)
```

Every type parameter must resolve, every bound must hold, and explicit type arguments must have the declared arity. See [Generics And Traits](/manual/generics-and-traits#inference-and-specialization).

A clone-producing operation over an unresolved type parameter does not make the generic declaration invalid. Instead, the checker infers a clone-safety obligation for that parameter. A call discharges the obligation after substitution. A generic caller propagates an unresolved obligation as part of its own callable contract. The requirement also applies when the callable is imported or used as a maintained task target. See [Generics And Traits](/manual/generics-and-traits#inferred-clone-safety-obligations).

## Function Values

A module-level named function is a value. Its type uses declaration-shaped syntax, `def(T1, mut T2, own T3) -> R`. The zero-parameter form is `def() -> R`. Bare parameters are shared. A function type may appear anywhere another complete type may appear. That includes variable and parameter annotations, class fields, return types, alias targets, and collection element types.

Public user-module functions are values, and so are maintained builtin-module functions such as `process.pipe`. Calling an imported builtin through a value uses the same builtin dispatch and result type as calling its qualified name.

Function values are code pointers. They carry no captured environment, which this page calls thin. They are copy values, so cloning is unnecessary. Copying one, or passing one as an `own def(...) -> R` parameter, does not invalidate the source binding. Function values also satisfy `Transfer`.

Inside a function type, `|` binds more loosely than `->`. So `def() -> int32 | None` is the union of a function type and `None`, that is `(def() -> int32) | None`. A function value that returns an optional groups its result type, as in `def() -> (int32 | None)`. The parenthesized single type is grouping, not a tuple.

### Callable Contracts

A callable type spells a complete contract. Each slot is one of two kinds:

- An unnamed slot, such as `int64`, `mut Counter`, or `own str`, is positional-only.
- A named slot, such as `value: int64`, may also be called by name.

A `*` boundary makes the named slots after it keyword-only. `= ...` after a slot promises that the target supplies a default for it. The default expression itself belongs only to the target declaration and is never written in a type.

The contract includes the parameter names, modes, and types, the keyword-only boundary, default availability, and the result type. Two structurally identical signatures with different exposed names or default promises are different contracts.

```aura
type Unary = def(int64) -> int64
type Renderer = def(value: int64, *, prefix: str = ...) -> str

def render(value: int64, *, prefix: str = "value") -> str:
    return f"{prefix}: {value}"

def increment(value: int64) -> int64:
    return value + 1

renderer: Renderer = render
print(renderer(4))
print(renderer(4, prefix="n"))
step: Unary = Unary(increment)
print(step(4))
```

An inferred local binding takes its target's complete contract: the declaration's names, `*` boundary, capabilities, and default availability. A call through it accepts the same named arguments and may omit the same parameters as a direct call. An omitted argument evaluates the runtime-selected target's own default expression afresh.

A written destination contract governs calls through that destination. A written destination is an annotation, parameter, field, element type, alias, or return type. Through it, an unnamed slot is called positionally, a named slot may be called by name, and an omitted slot must carry `= ...`.

Ordinary indirect calls evaluate and bind arguments under the selected function's unchanged capability contract. A stored value never takes an argument its contract does not admit.

If an indirect-call default traps, the diagnostic uses the public target name and the precise span of the default expression. Compiler-generated default helpers never appear in the call chain.

### Matching Contracts

A bare annotation, parameter pass, field or element store, alias binding, return, or branch join must preserve the value's complete callable contract. A difference reports `AU2015` and describes the differing slot, kind, or obligation. A written destination does not restrict the value implicitly.

Inference never invents a common contract. Each of these rules reports `AU2015` with the differing slot when it fails:

- Rebinding a local must satisfy the contract the local already has.
- Without an expected type, the arms of a conditional or `match` expression must share one complete contract.
- A container literal's element contract is its annotation, or else its first element's contract. Every later element and insertion must satisfy it.
- Repeated generic evidence must agree with its first observation.

### Adapters

A safe restriction requires an explicit adapter. For example, declare `type Unary = def(int64) -> int64` and write `Unary(double)` to hide `double`'s parameter name. An adapter may:

- hide exposed names
- drop default availability
- restrict a positional-or-keyword slot to keyword-only

An adapter cannot rename a slot, invent defaults, or make keyword-only slots positional. It also cannot change parameter types, modes, capabilities, or result guarantees.

Use an adapter to bring each value to a common contract. Calling a non-generic alias of a thin `def` type with one function value or capture-free lambda, such as `Unary(increment)`, yields that alias's contract when the alias admits the value. Otherwise it reports `AU2015`. Thin adapters add no environment. A capturing closure packs into an owned `Callable[...]` storage type instead. See [Closures](/manual/closures#typing-rules). Owned `Callable[...]` and `TaskCallable[...]` constructors keep their own capture, call-kind, and Transfer admission rules.

### Generic Function Values

A generic named function must receive explicit type arguments, as in `show_int = show[int32]`, or a concrete expected function type. An expected type can specialize a variable annotation, argument, field, collection element, or parameter default. For example, a generic `empty` can be used where `def() -> (str | None)` is required. A generic name with neither source of type arguments does not have one concrete function-value type.

### Methods And Returned Views As Values

An associated method without `self`, named as `Class.method`, is a thin function value carrying the method's complete contract when its class is not generic. A generic method takes explicit type arguments as `Class.method[T]`, or infers them from an expected function type.

Outside call position, `receiver.method` is a bound method. A bound method is a compiler-synthesized closure over the receiver, specified in [Closures](/manual/closures#bound-methods). Lambdas and closure capture are specified separately.

A function that returns `view [mut] T from name` for one of its parameters keeps that contract in its value type, as in `def(pair: Pair) -> view str from pair`. A method result declared `from self` has no value type. [Closures](/manual/closures#stored-view-contracts) specifies storage, packing, and calls through such values.

## Function Values And Task Starts

The ordinary and explicit-stack `TaskGroup` start methods accept a named function value as their target. They also accept a direct named function or an associated method without `self` as the target.

```aura
def work(value: int32) -> int32:
    return value * 2

worker = work

with group = TaskGroup():
    task = group.start(worker, 21)
```

Task capture ownership does not depend on the target function's call application binary interface (ABI). Each argument is first copied or moved into capture storage owned by the task. Then the target's parameter mode applies:

- An `own` target parameter consumes its capture.
- A bare shared parameter accesses that storage for the duration of the child call.
- A `mut` target parameter is rejected, because mutable access to detached capture storage has no writeback contract the caller can see.

A stored `TaskCallable[...]` value is also a valid target. It moves into the start for one child call. An ordinary erased `Callable` is not a valid target, because its environment was never proven `Transfer`. That case reports `AU3008`. See [Concurrency](/manual/concurrency).

## `main`

In the selected entry module, a local function named `main` is the entrypoint when the module has no executable top-level statements. Its only valid signatures are:

```aura
def main() -> int32:
    return 0
```

```aura
def main():
    print("done")
```

`main` takes no parameters and returns exactly `int32` or `None`. A returned `int32` becomes the requested host exit status. `None` means success. An imported function named `main` stays an ordinary imported function. A file cannot combine a local `main` with executable top-level statements.

[Execution Model](/manual/execution-model#entry-module-execution) specifies the alternate top-level execution form, evaluation order, cleanup on return, and the 256-call runtime depth limit.

## Grammar

[Grammar](/manual/grammar) is normative for function and method declarations, generic parameters and bounds, receiver and ordinary parameter forms, defaults, owned return annotations, and call arguments. Ordinary functions are module items. Nested function declarations are not accepted. [Closures](/manual/closures) specifies expression lambdas.

## Typing Rules

- Every ordinary parameter has one declared type and a declaration-stable passing mode.
- A call binds positional arguments, then named arguments. It substitutes inferred or explicit generic arguments, enforces bounds and exact types, and fills only legal defaults.
- A call enforces inferred clone-safety obligations after substitution.
- Every reachable path of a non-`None` function returns the declared type.
- Shared or mutable access never authorizes moving a non-copy value into the result. A non-copy return requires an owned source.

## Runtime Semantics

- The callee target is resolved statically.
- Supplied arguments evaluate left to right. Each result is captured before the side effects of later arguments.
- Omitted defaults then evaluate afresh in declaration order.
- A call creates one frame.
- `return` transfers its value after the exited cleanups run. `try` may perform that return early.
- Entry `main` maps `None` to success, and maps an `int32` result to the requested host process status.

## Ownership And Evaluation Order

- Bare parameters grant shared access. An implementation may pass copy bits directly.
- `own` parameters consume their arguments.
- `mut` requires one exclusive mutable place and writes through it.
- Borrowed default temporaries live through the call. Owned defaults are consumed. Mutable-borrow defaults are rejected as guaranteed lost writes.
- A task start first stores owned captures, then invokes the target under its declared ABI.

## Diagnostics

Compile-time diagnostics:

| Code | Meaning |
| --- | --- |
| `AU1101` | Malformed function, method, parameter, return, or call syntax. |
| `AU2001` | The call target or referenced declaration could not be resolved. |
| `AU2002` | Signature, function-value capability, parameter, default, return, bound, or entrypoint type mismatch. |
| `AU2004` | Positional or named argument binding failed. |
| `AU2005` | Focused guidance for an unavailable callable spelling. This includes a generic method value whose type arguments are neither written nor implied by an expected contract. |
| `AU2015` | A function value's complete callable contract differs from the one required. The message names the differing slot, kind, or obligation. |
| `AU2999` | Another callable rejection with no narrower compile-time code. |
| `AU3001` | A moved argument was used. |
| `AU3002` | Borrow or alias conflict. |
| `AU3003` | Mutability violation. |
| `AU3004` | Invalid parameter, receiver, return, or task-capture ownership mode. |
| `AU3007` | A call specialization would duplicate non-cloneable `random.Rng` state, or could not satisfy a callable clone-safety obligation. |
| `AU3008` | A task start target is an ordinary erased `Callable`, whose environment was never proven `Transfer`. |
| `AU3010` | A returned view has an invalid escape, origin, caller place, kind, or provenance path. |

Runtime diagnostics:

| Code | Meaning |
| --- | --- |
| `AU4001` | Call-depth or general call trap. |
| `AU4002` | Arithmetic overflow or underflow in a callee. |
| `AU4003` | Bounds or lookup violation in a callee. |
| `AU4004` | Zero divisor in a callee. |
| `AU4005` | Trapping resource or I/O failure in a callee. |

A callee's `AU4002`, `AU4003`, `AU4004`, or `AU4005` keeps the same typed Aura call frames and task ancestry on mid-level intermediate representation (MIR) execution and on direct-native execution.

## Backend Support

Ordinary, returned-view, indirect function-value, generic, imported, associated, trait-dispatched, and maintained task-target calls are implemented for MIR execution and for direct native builds. Shared semantic checking and the forced parity matrix require identical call results and primary failures on both. Compiler analysis and the language server use the same resolved signature metadata, including inferred clone-safety obligations.

## Limits And Implementation-Defined Behavior

Aura has none of these:

- trait-object function interactions
- Aura variadic functions
- overloads
- nested functions
- mutable-parameter task targets

[Closures](/manual/closures) specifies expression lambdas. They do not add nested item declarations. Written function types express bare shared, `mut`, and `own` parameter contracts.

Runtime calls are limited to 256 nested Aura frames. The host process's exit representation may narrow the requested `int32` after it leaves Aura. Function binding and evaluation order are otherwise not implementation-defined.

## Status

The contracts described above are implemented for functions, methods, generics, capture-free function values, default arguments, named arguments, ordinary owned returns, returned views, inferred clone-safety, task targets, and entrypoints. By-value expression closures are implemented.

The fixture `crates/aura-compiler/tests/fixtures/run-pass/explicit_and_default_argument_order.au` pins the supplied and default argument evaluation and capture rules on both backends.

Foreign function interface (FFI) v0 provides bodyless, direct-call-only `extern "C" def` declarations. They are not function values. [FFI v0](/manual/ffi) specifies their restricted signatures.

Design record: supplied and default argument evaluation and capture follow `architecture_docs/decisions/0015-explicit-and-default-argument-order.md`, which is Accepted. By-value expression closures follow Accepted ADR-0037.
