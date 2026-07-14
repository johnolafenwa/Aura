# Statements

Statements introduce and update bindings, control execution, or evaluate an expression for its effects. This chapter defines their legality and observable flow. Exact syntax is normative in [Grammar](/manual/grammar#suites-and-statements), compile-time legality in [Static Semantics](/manual/static-semantics), and runtime sequencing and cleanup in [Execution Model](/manual/execution-model).

## Statements, Items, And Suites

Aurora 0.1 statements are:

- binding and assignment
- expression statements
- `return`
- `if` / `elif` / `else`
- `while` and `for`
- statement-form `match`
- `with`
- `break`, `continue`, and `pass`

Class, enum, function, trait, and implementation declarations are items, not statements. Items are module-level; declaration members such as fields, enum variants, and methods appear only in their permitted item bodies. Nested functions, classes, enums, traits, and implementations are not supported.

A compound statement header ends with `:` and `NEWLINE`, followed by an indented suite. Suites contain one or more statements:

```python
if ready:
    print("ready")
    record_success()
```

One-line suites such as `if ready: print("ready")` are not valid. Blank and comment-only lines do not make a suite nonempty; use `pass` when no operation is required.

Statements are terminated by logical newlines. Aurora has no semicolon and does not permit multiple statements on one physical line.

## Bindings And Assignment

The first assignment to a simple name introduces a binding:

```python
name = "aurora"
count: int32 = 0
```

The binding's type is its annotation when present, otherwise the initializer type. The initializer must have exactly that type after contextual literal inference.

`mut` makes a newly introduced binding assignable and usable as a mutable place:

```python
mut count: int32 = 0
count = 1
count += 2
```

Reassignment requires an existing mutable binding and preserves its type. `mut` does not mean dynamically typed, and it does not make values globally mutable through aliases.

`from` is a contextual identifier and is legal as a binding and assignment target when the token sequence is not a from-import:

```python
mut from = "cache"
from = "network"
```

### Assignment Targets

An assignment target begins with a name and may continue through fields or indices:

```python
point.x = 4.0
values[0] = 9
counts["ready"] = 2
user.profile.name = "Ada"
```

Calls cannot occur in an assignment target. Aurora has no tuple or destructuring assignment.

A type annotation is allowed only on a simple-name target. `mut` also belongs only to a new simple-name binding. These forms are invalid:

```python
# Invalid.
# point.x: float64 = 4.0
# mut point.x = 4.0
```

Field assignment requires a mutable base place and a declared field. Vector index assignment requires exactly `int32`. Map index assignment requires exactly the map's key type.

### Compound Assignment

Aurora supports the complete arithmetic compound-assignment family `+=`, `-=`, `*=`, `/=`, `%=`, and `//=`:

```python
count += 1
total *= scale
pages //= page_size
```

A compound assignment requires an existing mutable, initialized target. It reads that target once, evaluates the right operand, applies the corresponding binary operator, and stores a same-typed result. Operator traits and runtime overflow/division behavior are the same as for the corresponding expression operator. Integer `/=` is rejected with the integer `/` teaching diagnostic; use integer `//=` for a floor quotient. Floating `/=` remains true division. `//=` is builtin-only because `//` has no operator trait.

### Assignment Evaluation

A simple assignment evaluates its right side before creating or updating the target. Reassigning an exact moved binding or field reinitializes that place when the new value has the required type. Failed checked mutation produces the documented runtime failure or typed result and does not create a different language-level partial assignment contract.

See [Ownership And Borrowing](/manual/ownership-and-borrowing) for moves, partial field moves, and mutable-place rules.

## Expression Statements

Any expression may be used as a statement when its produced value is not needed:

```python
print("ready")
queue.close()
counter.increment()
```

The expression is fully evaluated, including moves, mutations, I/O, and runtime failures; its resulting value is discarded. A discarded `Result` is not implicitly propagated. Use `try` or `match` when failure must affect control flow.

## `return`

`return` is legal only inside a function or method:

```python
def answer() -> int32:
    return 42
```

The expression is evaluated before control returns. Its type must equal the declared return type. Bare `return` produces `None` and is valid only where `None` is a valid return.

```python
def maybe_log(enabled: bool):
    if not enabled:
        return
    print("enabled")
```

A non-`None` function must return on every statically reachable path. Returning runs active `with` cleanups in reverse nesting order before control reaches the caller.

## Conditional Statements

`if`, zero or more `elif` branches, and an optional `else` select at most one suite:

```python
if value < 0:
    print("negative")
elif value == 0:
    print("zero")
else:
    print("positive")
```

Conditions must have exactly type `bool`. Aurora does not convert strings, numbers, collections, resources, or classes by truthiness.

Conditions are evaluated in source order until one is `true`. Only the selected suite executes. Static checking analyzes branches independently and conservatively merges ownership, partial-move, and initialization state across paths that can continue.

## `while`

A `while` statement evaluates its condition before each iteration:

```python
mut attempts = 0
while attempts < 3:
    attempts += 1
```

The condition must have type `bool`. A false first condition executes the body zero times. Aurora 0.1 has no loop `else` clause.

Moving a non-copy outer value for the first time inside a repeatable loop is rejected when it could make a later iteration invalid. Reinitialize the place on every continuing path or restructure ownership explicitly.

## `for` Iteration

A `for` statement binds one name to each value from an iterable:

```python
for value in values:
    print(value)
```

The target is one identifier; tuple/destructuring loop targets are not implemented. The loop binding is local to the body, does not escape, and cannot shadow a name already visible in the same scope.

Maintained iterable forms include:

| Form | Behavior |
| --- | --- |
| `for i in range(n):` | Yields `int32` values from zero up to `n`, excluding `n`. |
| `for i in range(start, end):` | Yields `int32` values from `start` up to `end`, excluding `end`. |
| `for value in vec:` | Consumes a non-copy vector and yields owned elements. |
| `for value in borrow vec:` | Retains the vector and yields shared-borrowed access for non-copy elements. |
| `for value in borrow mut vec:` | Retains a mutable vector and yields mutable-borrowed access; the iterable place must be mutable. |
| `for value in set:` | Consumes a non-copy set and yields owned elements. |
| `for value in borrow set:` | Retains the set and yields shared-borrowed access. |
| `for value in queue:` | Receives queue items under the scheduler-aware queue iteration contract. |

`for value in borrow mut set:` is not supported in Aurora 0.1. Queue iteration ends according to close, cancellation, producer-completion, and task-failure rules defined in [Concurrency](/manual/concurrency).

## `break` And `continue`

`break` and `continue` are legal only inside `for` or `while`:

```python
for value in range(10):
    if value == 5:
        break
    if value % 2 == 0:
        continue
    print(value)
```

`break` exits the nearest loop. `continue` begins its next iteration. If either operation exits an active `with` scope, that scope is cleaned up before loop control transfers.

## Match Statements

Statement-form `match` evaluates its scrutinee exactly once and considers arms in source order. The first matching arm executes:

```python
match result:
    case Result.Ok(value):
        print(value)
    case Result.Err(message):
        print(message)
```

Every statement arm contains an indented suite. Inline statement arms such as `case Result.Ok(value): print(value)` are not valid. Inline arms are available only for match expressions whose arm body is one expression; see [Expressions](/manual/expressions#match-expressions).

Matches over enums and booleans must be exhaustive unless `_` covers the remainder. Integer, float, and string literal matches require `_` because their value spaces are open. Duplicate, unreachable, type-incompatible, or wrong-arity patterns are rejected.

`match value` may consume a non-copy scrutinee. `match borrow value` retains ownership and exposes shared payload access. `match borrow mut value` permits payload mutation and writes the reconstructed enum value back to the matched mutable place on normal arm exit. See [Enums And Pattern Matching](/manual/enums-and-match) for pattern forms.

## `with` And Scoped Cleanup

Aurora accepts two equivalent binding forms:

```python
with file = try fs.open("data.txt"):
    text = try file.read_all()
    print(text)
```

```python
with TaskGroup() as group:
    group.start_soon(worker)
```

The first form is `with name = expression:`. The second is `with expression as name:`. Each form evaluates and consumes the resource expression, creates a fresh mutable managed binding, and registers cleanup after resource creation succeeds.

Supported builtin resources define their cleanup behavior. A user class can be used when it is non-generic and declares exactly `close(borrow mut self) -> None`. The managed value cannot be moved out in a way that prevents cleanup.

The registered `close` operation runs exactly once when control leaves the body by:

- normal fallthrough
- `return`
- `break` or `continue` that exits the scope
- `try` error propagation
- a maintained Aurora runtime failure

Nested cleanups run in reverse registration order. If the body is already failing and cleanup also fails, the body diagnostic remains primary.

This contract is shared by `aura run` through the maintained MIR runtime and by native builds through the maintained native execution paths. Backend parity tests enforce the common contract. See [Execution Model](/manual/execution-model#resource-lifetime-and-cleanup).

## `pass`

`pass` performs no operation and produces no binding:

```python
def placeholder():
    pass
```

It must appear on its own logical line. It is used for intentionally empty function, method, class, trait, implementation, or control-flow suites. An enum body still requires at least one variant and does not use `pass` as a variant.

## Module-Level Imports And Execution

Imports are module elements rather than executable statements. Aurora accepts:

```python
import util.math
from util.math import double, triple
```

Import paths are dot-separated identifiers. There are no aliases, wildcard imports, relative-dot imports, parenthesized import lists, or trailing import commas. Import resolution and visibility are defined in [Packages](/manual/packages#imports).

An entry module may contain executable top-level statements:

```python
message = "hello"
print(message)
```

Those statements execute in their stored source order. Alternatively, the entry module may define a local `main`. It cannot combine executable top-level statements with a local `main`. Imported module top-level statements do not execute as import side effects in Aurora 0.1.

The accepted `main` signatures and process exit behavior are defined in [Functions](/manual/functions#main) and [Execution Model](/manual/execution-model#entry-module-execution).

## Contextual Legality Summary

Parsing a statement shape does not make it legal in every context:

- `return` requires a function or method.
- `break` and `continue` require an enclosing loop.
- reassignment and compound assignment require a mutable existing place.
- member and index assignment require a mutable base and cannot declare a type or use `mut`.
- conditions require `bool` rather than truthiness.
- match arms must satisfy compatibility, reachability, and exhaustiveness rules.
- `with` requires a supported resource and preserves its cleanup capability.
- items cannot appear inside suites.
- an entry module cannot mix executable top-level statements with local `main`.

The complete checker rules are normative in [Static Semantics](/manual/static-semantics), and ownership effects are normative in [Ownership And Borrowing](/manual/ownership-and-borrowing).
