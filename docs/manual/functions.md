# Functions

Functions are module-level declarations introduced by `def`. Their signatures fix the parameter names, parameter passing modes, parameter types, generic parameters and bounds, and return contract used at every call site.

```python
def add(left: int32, right: int32) -> int32:
    return left + right
```

The complete declaration grammar is in [Grammar](/manual/grammar#functions-methods-and-parameters). This chapter defines the corresponding static and execution rules.

## Signatures And Return Types

Every ordinary parameter has an explicit type. A return annotation is optional; omitting it is exactly equivalent to `-> None`.

```python
def square(value: int32) -> int32:
    return value * value

def log(message: String):
    print(message)
```

`return expression` must have exactly the declared return type. `return` without an expression has type `None` and is valid only in a `None`-returning function. Reaching the end of a `None` function returns `None` implicitly.

A function with any other return type must return on every statically reachable path:

```python
def classify(value: int32) -> String:
    if value < 0:
        return "negative"
    return "non-negative"
```

There is no implicit numeric widening or general return coercion. Contextual literal typing and `None`-to-`Option[T]` handling follow [Static Semantics](/manual/static-semantics#contextual-inference).

Function names share the module item namespace with classes, enums, traits, and imports. Duplicate items and attempts to redefine maintained builtin function names are rejected. Ordinary parameter names must be unique. A method parameter also cannot be named `self` when the method has a receiver. In a method declaration, `self: Type` is rejected rather than treated as an ordinary first parameter; receivers use `self`, `borrow self`, `own self`, or `borrow mut self`. See [Names And Scopes](/manual/names-and-scopes) for the complete namespace rules.

A function is private to its defining module by default. Prefix the declaration with `public` to make it importable from another module:

```python
public def double(value: int32) -> int32:
    return value * 2
```

Visibility controls name access, not the ownership or type rules of the signature.

## Parameter Passing Modes

The passing mode is part of the function signature:

| Declaration | Contract at the call boundary |
| --- | --- |
| `value: T` | Shared borrow when `T` is non-copy; by value when `T` is copy. |
| `value: own T` | Owned argument. A move value is consumed; a copy value is duplicated. |
| `value: borrow T` | Explicit shared borrow. The caller retains ownership; the callee cannot move through the borrow. |
| `value: borrow mut T` | Exclusive mutable borrow. The argument must be a mutable place. |

```python
def consume(name: own String):
    print(name)

def length(text: borrow String) -> int32:
    return text.len()

def push_name(names: borrow mut Vec[String], name: own String):
    names.push(name)
```

The modifier is written in the declaration after the colon. Calls pass the
expression directly; Aurora has no call-site `own` or `borrow` syntax:

```python
mut names = Vec[String]()
push_name(names, "Ada")
```

Arguments must have exactly the substituted parameter type. Calls also reject overlapping move, shared-borrow, and mutable-borrow access at the same call boundary. The ownership and place rules are specified in [Ownership And Borrowing](/manual/ownership-and-borrowing).

The bare rule is resolved where the function is declared, not independently
at each call. An unconstrained generic `value: T` therefore resolves to a
shared borrow because `T` is not known copyable there. That choice is
**declaration-stable**: specializing the function later with `T = int32` does
not turn the parameter into an owned value. Write `value: own T` when a generic
function must consume or return its argument.

## Call Binding

Calls accept positional arguments followed by named arguments:

```python
def render(name: String, count: int32 = 1):
    print(name)

render("Aurora")
render("Aurora", 2)
render(name="Aurora", count=2)
```

Binding is deterministic:

1. positional arguments fill parameters in declaration order
2. named arguments fill the parameter with the same name
3. one parameter cannot be filled twice
4. unknown names and excess positional arguments are rejected
5. every omitted parameter must have a default
6. each bound argument must have the parameter's exact substituted type

Positional arguments cannot follow a named argument. Parameter and argument lists do not accept trailing commas in Aurora 0.1.

## Default Arguments

A default is permitted on an ordinary default-mode, `own`, or shared-`borrow`
parameter of a top-level function or class method:

```python
def greet(name: String = "world"):
    print("hello " + name)
```

The complete rules are:

- `borrow mut` parameters cannot have defaults, regardless of whether their
  types are copyable; the default would be a caller-invisible temporary, so
  every mutation would be a silent lost write. Require the caller to pass a
  value, or take the parameter as `own T` and return the result
- a shared-borrow default is permitted; its default temporary lives until the
  call completes
- an `own` default is permitted and its fresh temporary is consumed by the call
- after the first defaulted parameter, every remaining parameter must also have a default
- the default expression must have exactly the declared parameter type
- a default expression cannot reference any parameter of the same declaration, including an earlier parameter
- trait method declarations and trait implementation methods cannot declare defaults

Defaults are evaluated afresh when the corresponding argument is omitted. They are not process-global singleton values. Explicit arguments are evaluated in source order, and omitted defaults are associated with their declaration-order slots; see [Execution Model](/manual/execution-model#evaluation-order).

## Named Arguments For Builtins

Maintained builtin functions and methods use the same binding rules, with parameter names defined by their API metadata:

```python
import process

process.run(["/bin/echo", "hi"], stdout=process.pipe(), group=true)
```

```python
import net

net.http_request_text_timeout(method="POST", url="http://127.0.0.1:8080/jobs", body="{}", headers={}, timeout=2s)
```

The module chapters and [API Index](/manual/api-index) are authoritative for builtin parameter names, defaults, and return types.

## `try` And Result Returns

`try` is valid only when its operand has type `Result[T, E1]` and the enclosing function returns `Result[U, E2]`:

```python
def parse_total(left: String, right: String) -> Result[int32, String]:
    a = try parse_int32(left)
    b = try parse_int32(right)
    return Result.Ok(a + b)
```

`Result.Ok(value)` makes `try` evaluate to `value`. `Result.Err(error)` returns from the enclosing function immediately. `E1` must equal `E2`, or an applicable `impl From[E1] for E2` with a `from` method must be visible. Active `with` cleanups run during this early return. See [Execution Model](/manual/execution-model#try).

## Borrowed Returns

A return annotation can identify a borrow source in an API contract:

```python
def identity(value: borrow[source] int32) -> borrow[source] int32:
    return value
```

The source in brackets is either the borrowed parameter name or its borrow label. In Aurora 0.1, calls returning a copy type materialize an ordinary copied value. Calls returning a non-copy borrowed result are rejected until the Phase 6 live-alias representation is implemented.

Eligible source rules are:

- `-> borrow T` may derive from a shared- or mutable-borrowed parameter or receiver
- `-> borrow mut T` may derive only from a mutable-borrowed parameter or receiver
- when exactly one eligible source exists, the source may be omitted and is inferred
- when multiple eligible sources exist, the return annotation must select one by parameter name, `self`, or label
- for non-copy declarations, the returned expression must actually derive from the selected source
- an owned non-copy expression cannot satisfy a borrowed return contract

Labels are signature-level provenance names, not general lexical lifetime variables:

```python
def choose(left: borrow[left_source] int32, right: borrow[right_source] int32) -> borrow[left_source] int32:
    return left
```

A borrowed return of a copy type is materialized as an ordinary copy value. A call producing a borrowed `String`, collection, resource, ordinary class, or other non-copy value fails during checking with guidance to return an owned clone or expose an owner method. Non-copy borrowed-return declarations remain checked so source and trait contracts are reserved consistently for Phase 6. Trait implementations must preserve the trait method's parameter passing, return passing, and semantic return-source slot; parameter names and labels themselves may be renamed. See [Ownership And Borrowing](/manual/ownership-and-borrowing#borrowed-returns-and-provenance).

## Generic Functions

Type parameters follow the function name:

```python
def identity[T](value: own T) -> T:
    return value
```

Bounds restrict substitutions:

```python
def describe[T: Greeter](value: borrow T) -> String:
    return value.greet()

def use_both[T: First + Second](value: T) -> int32:
    return value.score()
```

The checker infers type arguments from call arguments and an available expected result type. Explicit specialization fixes them:

```python
answer = identity[int64](42)
```

Every type parameter must resolve, all bounds must hold, and explicit type arguments must have the declared arity. See [Generics And Traits](/manual/generics-and-traits#inference-and-specialization).

## Callable Targets And Task Starts

Aurora 0.1 does not provide general first-class function variables, lambdas, or closures. The task API has a specific callable-target form: `TaskGroup.start` and `start_soon` accept a named function or an associated method without `self`.

```python
def work(value: int32) -> int32:
    return value * 2

with group = TaskGroup():
    task = group.start(work, 21)
```

Task capture ownership is independent of the target function's call ABI. Each
argument is first copied or moved into task-owned capture storage: `own` target
parameters consume their capture, while default-mode and explicit shared
parameters borrow from that storage for the duration of the child call.
`borrow mut` targets are rejected because mutable access to detached capture
storage has no caller-visible writeback contract. See [Concurrency](/manual/concurrency).

## `main`

In the selected entry module, a local function named `main` is the entrypoint when there are no executable top-level statements. Its only valid signatures are:

```python
def main() -> int32:
    return 0
```

```python
def main():
    print("done")
```

`main` takes no parameters and returns exactly `int32` or `None`. A returned `int32` becomes the requested host exit status; `None` means success. An imported function named `main` remains an ordinary imported function. A file cannot combine a local `main` with executable top-level statements.

The alternate top-level execution form, evaluation order, cleanup on return, and the 256-call runtime depth limit are specified in [Execution Model](/manual/execution-model#entry-module-execution).
