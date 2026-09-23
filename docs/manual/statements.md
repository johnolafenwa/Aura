# Statements

Statements introduce and update bindings, control execution, or evaluate an expression for its effects. This page defines when each statement is legal and what it does when it runs. The exact syntax is normative in [Grammar](/manual/grammar#suites-and-statements). Compile-time legality is normative in [Static Semantics](/manual/static-semantics). Runtime sequencing and cleanup are normative in [Execution Model](/manual/execution-model).

## Statements, Items, And Suites

Aura 0.3 has these statements:

- binding and assignment
- local `view` bindings
- expression statements
- `return`
- `assert`
- `if` / `elif` / `else`
- `while` and `for`
- statement-form `match`
- `with`
- `break`, `continue`, and `pass`

Class, enum, function, trait, and implementation declarations are items, not statements. Items appear only at module level. Declaration members such as fields, enum variants, and methods appear only in the item bodies that permit them. Aura does not support nested functions, classes, enums, traits, or implementations.

A compound statement header ends with `:` and `NEWLINE`. An indented suite follows it. A suite contains one or more statements:

```aura fragment
if ready:
    print("ready")
    record_success()
```

One-line suites such as `if ready: print("ready")` are not valid. Blank lines and comment-only lines do not make a suite nonempty. Use `pass` when a suite needs no operation.

A logical newline ends a statement. A physical newline inside an open delimiter does not end a statement. Aura has no semicolon, and a physical line cannot hold more than one statement.

## Bindings And Assignment

The first assignment to a simple name introduces a binding:

```aura
name = "aura"
count: int32 = 0
```

The binding's type is its annotation when one is present. Otherwise it is the initializer's type. After contextual literal inference, the initializer must have exactly that type.

`mut` makes a new binding assignable and usable as a mutable place:

```aura
def main():
    mut count: int32 = 0
    count = 1
    count += 2
```

Reassignment requires an existing mutable binding and keeps its type. `mut` does not make a binding dynamically typed. It also does not make a value mutable through every alias.

`from` is a contextual identifier. It is a legal binding and assignment target whenever the tokens do not form a from-import:

```aura
def main():
    mut from = "cache"
    from = "network"
```

### Assignment Targets

An assignment target starts with a name and may continue through fields or indices:

```aura fragment
point.x = 4.0
values[0] = 9
counts["ready"] = 2
user.profile.name = "Ada"
```

A place-assignment target cannot contain a call.

A type annotation is allowed only on a simple-name target. `mut` also belongs only to a new simple-name binding. These forms are invalid:

```aura
# Invalid.
# point.x: float64 = 4.0
# mut point.x = 4.0
```

Each kind of projected target has its own rules:

- **Field assignment** requires a mutable base place and a declared field.
- **List index assignment** uses the `int64` index domain.
- **Simple dict index assignment** requires exactly the dictionary's key type. It replaces the entry for an equal key or inserts a new entry, so an absent key is not an error for simple assignment. It works for every value type. The key and the value are owned storage positions, so each one is consumed when it is non-copy. This matches `set(key: own K, value: own V)`.

A tuple unpack target contains only names and parenthesized name targets, nested to any depth:

    left, right = pair
    name, (x, y) = record

The right side is evaluated once. Its tuple shape and element types must match the target exactly. A top-level comma marks the statement as an unpack rather than an expression. A tuple value expression itself requires parentheses. Tuple unpacking uses plain `=`. It does not accept a type annotation, a leading `mut`, compound assignment, a member leaf, or an index leaf.

### Compound Assignment

Aura supports the complete arithmetic compound-assignment family `+=`, `-=`, `*=`, `/=`, `%=`, and `//=`:

```aura fragment
count += 1
total *= scale
pages //= page_size
```

A compound assignment follows these rules:

- **Target.** The target must be an existing, mutable, initialized place. The statement selects that place once.
- **Operator.** It uses exactly the dispatch of the corresponding binary operator. That includes an applicable user-defined operator trait, for both a root target and a projected target.
- **Copy target.** The current value is copied before the right operand is evaluated. The result is stored into the place selected at the start. Side effects in the right operand therefore cannot change the captured left operand or redirect the store.
- **Non-copy target.** A non-copy root or projected target stays borrowed while the right operand is evaluated. An overlapping mutable borrow or consumption reports `AU3002`.
- **Result type.** The result must have the target's existing type.

Direct indexed compound assignment requires a copy `list` element or a copy `dict` value. The checker rejects a non-copy indexed element. A read-modify-write on it would need either a hidden clone or a destructive move before an operation that may fail. Instead, read the element safely or transfer its ownership explicitly, then write it with simple assignment. For a dict, use `get(key)` or `remove(key)` followed by simple assignment.

Runtime overflow and division behavior match the corresponding expression operator. Integer `/=` is rejected with the same teaching diagnostic as integer `/`. Use `//=` for an integer floor quotient. Floating-point `/=` is true division. `//=` uses the builtin numeric or Duration rule when one applies. Otherwise it may dispatch through `FloorDiv.floor_div`.

### Assignment Evaluation

- A simple-name or field assignment evaluates its right side before it creates or updates the target.
- An indexed assignment evaluates the collection place, then the index or key, then the right side. A non-copy collection base stays borrowed through those later inputs. An overlapping mutable borrow or consumption reports `AU3002`. No hidden deep clone is inserted.
- A simple dict assignment captures the key before it evaluates and consumes the value. A non-copy key is consumed when it is captured. Side effects in the value therefore cannot change the selected key.
- Reassigning exactly the moved binding or field reinitializes that place, provided the new value has the required type.
- A failed checked mutation produces its documented runtime failure or typed result. It does not create a separate language-level contract for partial assignment.

See [Ownership And Borrowing](/manual/ownership-and-borrowing) for moves, partial field moves, and mutable-place rules.

## View Bindings

`view name = place` creates an immutable shared alias. `view mut name = place` creates a mutable alias that writes through to its source and cannot be rebound. The initializer must resolve to one of these places:

- a supported addressable root
- a field path
- a fixed tuple position
- a list element or dictionary entry, optionally followed by a field or tuple projection inside the element
- an existing view

The checker rejects computed temporaries. These examples update the source through a view:

    mut pair = (1, 2)
    view mut second = pair[1]
    second = 7
    print(pair)

    mut users = [Profile(name="ada", visits=1)]
    view mut visits = users[0].visits
    visits += 1
    print(users[0].visits)

The view's pointee type is inferred. Assigning through a mutable view changes the source. It does not retarget the view.

An element or entry selector is evaluated once, when the view is created. A position outside the list or an absent key fails at that point with `AU4003`. Rebinding the index variable later does not move the view.

A view borrows its source while it is live. This borrow is called a loan. Static final-use analysis ends a loan as early as control flow safely allows. While the loan is live, the checker rejects overlapping mutation, move, rebind, cleanup, and any other mutable loan. Overlap follows these rules:

- Views of two different literal positions or keys in one collection are disjoint.
- A computed selector overlaps every element.
- Any structural mutation of the collection, such as `append`, `remove`, or `clear`, overlaps every element view.

Scope exits and control-flow exits release active loans before outer cleanup runs.

An assignment whose target projects inside an element or entry, such as `users[i].visits = 5` or `users[i].visits += 1`, writes through a mutable element loan that lasts for the statement. The element is updated in place. It is never copied out and written back.

## Expression Statements

Any expression may be used as a statement when its value is not needed:

```aura fragment
print("ready")
queue.close()
counter.increment()
```

The expression is fully evaluated, including its moves, mutations, I/O, and runtime failures. Its value is then discarded. A discarded `Result` is not propagated implicitly. Use `try` or `match` when a failure must affect control flow.

## `return`

`return` is legal only inside a function or method:

```aura
def answer() -> int32:
    return 42
```

The expression is evaluated before control returns. Its type must equal the declared return type. Bare `return` produces `None` and is valid only where `None` is a valid return:

```aura
def maybe_log(enabled: bool):
    if not enabled:
        return
    print("enabled")
```

Inside a function declared to return a view, `return view [mut] place` hands the caller a matching loan derived from the named `from` origin.

A function that does not return `None` must return on every statically reachable path. A `return` runs active `with` cleanups in reverse nesting order before control reaches the caller.

## Conditional Statements

An `if` with zero or more `elif` branches and an optional `else` runs at most one suite:

```aura fragment
if value < 0:
    print("negative")
elif value == 0:
    print("zero")
else:
    print("positive")
```

Conditions must have exactly type `bool`. Aura does not convert strings, numbers, collections, resources, or classes by truthiness.

Conditions are evaluated in source order until one is `true`. Only the selected suite runs. The checker analyzes each branch independently. It then merges ownership, partial-move, and initialization state conservatively across the paths that can continue.

A condition that tests a union place with `is None` or `is not None` [narrows](/manual/enums-and-match#conditional-narrowing) that place inside the selected suite. The narrowing also holds after the statement when the other branch cannot continue. Each `elif` condition is checked under the facts of every failed condition before it.

## `while`

A `while` statement evaluates its condition before each iteration:

```aura
def main():
    mut attempts = 0
    while attempts < 3:
        attempts += 1
```

The condition must have type `bool`. If the condition is false the first time, the body runs zero times. Aura 0.3 has no loop `else` clause.

A `while` condition that tests a union place with `is None` or `is not None` narrows the place inside the body. A `continue` in the body after such a test narrows the rest of that iteration. The condition runs before every iteration, so its narrowing holds in the body on every pass. This is true even when the body assigns the place later.

A fact established before the loop survives into the condition and the body only when no iteration can invalidate it. The checker treats the loop header as a fixed point over the loop entry and every backedge, the path from the body back to the header. A body that assigns such a place is therefore checked again without the entry fact.

The checker rejects the first move of a non-copy outer value inside a repeatable loop when that move could make a later iteration invalid. Reinitialize the place on every continuing path, or restructure ownership explicitly.

## `for` Iteration

A `for` statement binds one name, or recursively unpacks one tuple target, for each value from an iterable:

```aura fragment
for value in values:
    print(value)

for name, count in records:
    print(name)
    print(count)
```

Every target leaf is local to the body and does not escape. A leaf cannot shadow a name already visible in the same scope. A tuple target must match the yielded tuple shape exactly.

As with `while`, a narrowing fact established before the loop survives into the body only when no iteration can invalidate it. When the body assigns the place, test it again inside the body.

Use `for value in own values:` when the loop deliberately consumes a `list` or `set` and needs owned element bindings. Each owned binding, including every leaf of a tuple target, is a mutable place, so `value.append(1)` or a `mut self` method call works on it. The collection moves once, at loop entry, into a source private to the loop. Reinitializing the consumed `values` binding in the body does not switch or shorten the active iteration.

Maintained iterable forms include:

| Form | Behavior |
| --- | --- |
| `for i in range(n):` | Yields `int64` values from zero up to `n`, excluding `n`. |
| `for i in range(start, end):` | Yields `int64` values from `start` up to `end`, excluding `end`. |
| `for value in values:` | Retains the list and yields shared access for non-copy elements. |
| `for value in own values:` | Consumes the list and yields owned elements. |
| `for value in mut values:` | Binds each element as a mutable element view. Writes land in the list at once. The iterable place must be mutable. |
| `for value in set:` | Retains the set and yields shared-borrowed access. |
| `for value in own set:` | Consumes the set and yields owned elements. |
| `for value in queue:` | Receives queue items under the scheduler-aware queue iteration contract. |
| `for index, value in enumerate(seq):` | Yields `(int64, element)` pairs, counting positions from zero. |
| `for left, right in zip(first, second):` | Yields one pair per shared position and stops at the shorter sequence. |

`for value in mut set:` is not supported in Aura 0.3.

When an iterable yields tuples, the loop form decides the ownership of each non-copy tuple leaf:

- Bare, shared collection iteration gives the leaves shared provenance.
- `own` collection iteration gives owned leaves.
- Bare Queue iteration receives an owned item and gives owned leaves.
- `mut` iteration with a tuple target is rejected. Bind the tuple to one name and update its positions through it.

### Queue Iteration

Queue iteration receives values instead of traversing places. Each item arrives owned, and the queue handle is a copy value. So `own` and `mut` are rejected for Queue iteration. Use the bare form.

The bare form evaluates and copies the Queue handle once, at loop entry. It does not freeze the source binding. Rebinding the source in the body does not switch later receives to another queue. Queue iteration ends according to the close, cancellation, producer-completion, and task-failure rules in [Concurrency](/manual/concurrency).

### `enumerate` And `zip`

`enumerate` and `zip` are loop forms known to the compiler, not callable values. They are legal only as the iterable of a `for` statement. Naming either one anywhere else reports `AU2005`, and the message names the loop spelling. A user declaration of either name shadows the loop form, so an existing `def zip(...)` keeps its ordinary call meaning.

```aura
hosts = ["alpha", "beta"]
ports = [80, 443, 8080]

for index, host in enumerate(hosts):
    print(index)

for host, port in zip(hosts, ports):
    print(port)
```

Both forms read their operands by position, so each operand must be a `list[T]` or a `set[T]`. A `Range` or `Queue[T]` operand reports `AU2002`. Both forms use the bare-loop borrow default:

- An ownership modifier on the loop reports `AU3002`.
- Every operand stays shared-borrowed and frozen for the whole loop.
- A non-copy element binding is a shared borrow and cannot be moved out.

`enumerate` takes exactly one operand and `zip` takes exactly two, both positionally. Any other arity, or a named argument, reports `AU2004`.

`zip` stops as soon as any operand has no value at the current position. It performs `min(len(first), len(second))` iterations and never observes the tail of the longer sequence.

### Range Iteration

Range iteration accepts only the bare form. Every yielded `int64` is an independent copy. `mut` has no place to write back through, and `own` has nothing to transfer. Either modifier reports `AU3004`. The diagnostic explains that ownership modifiers do not apply to these copy values and suggests `for item in range(...):`.

## `break` And `continue`

`break` and `continue` are legal only inside `for` or `while`:

```aura
for value in range(10):
    if value == 5:
        break
    if value % 2 == 0:
        continue
    print(value)
```

`break` exits the nearest loop. `continue` starts that loop's next iteration. If either one exits an active `with` scope, that scope is cleaned up before control transfers.

## Match Statements

A statement-form `match` evaluates its scrutinee exactly once and tries its arms in source order. The first matching arm runs:

```aura fragment
match result:
    case Result.Ok(value):
        print(value)
    case Result.Err(message):
        print(message)
```

Every statement arm contains an indented suite. Inline statement arms such as `case Result.Ok(value): print(value)` are not valid. Inline arms exist only in match expressions whose arm body is one expression. See [Expressions](/manual/expressions#match-expressions).

A match over an enum or a boolean must be exhaustive unless `_` covers the rest. Integer, float, and string literal matches require `_`, because their value spaces are open. The checker rejects duplicate, unreachable, type-incompatible, and wrong-arity patterns.

The scrutinee's ownership depends on the form:

- `match own value` consumes a non-copy scrutinee. A non-copy tuple is consumed as one whole value and unpacked into owned pattern bindings.
- Bare `match value` keeps ownership and exposes shared access to enum payloads and tuple leaves.
- `match mut value` allows enum-payload mutation and writeback. A tuple pattern is rejected, because the minimal tuple support has no recursive mutable tuple writeback.

See [Enums And Pattern Matching](/manual/enums-and-match) for pattern forms.

## `with` And Scoped Cleanup

Aura accepts two equivalent binding forms, `with name = expression:` and `with expression as name:`:

```aura fragment
with file = try fs.open("data.txt"):
    text = try file.read_all()
    print(text)
```

```aura fragment
with TaskGroup() as group:
    group.start_soon(worker)
```

Both forms evaluate and consume the resource expression, then create a fresh mutable managed binding. Cleanup is registered after the resource is created successfully.

Supported builtin resources define their own cleanup. A user class can be a resource when it is non-generic and declares exactly `close(mut self) -> None`. The managed value cannot be moved out in a way that prevents cleanup.

The registered `close` operation runs exactly once when control leaves the body by:

- normal fallthrough
- `return`
- `break` or `continue` that exits the scope
- `try` error propagation
- a maintained Aura runtime failure

Nested cleanups run in reverse registration order. If the body is already failing and cleanup also fails, the body's diagnostic stays primary.

`aura run` and native builds share this contract. `aura run` executes the compiler's mid-level intermediate representation (MIR) through the maintained MIR runtime. Native builds use the maintained native execution paths. Backend parity tests enforce the common contract. See [Execution Model](/manual/execution-model#resource-lifetime-and-cleanup).

## `assert`

An assertion checks an invariant. It either continues or stops the program with an unrecoverable runtime diagnostic, called a trap:

    assert ready
    assert response_code == 200, "expected a successful response"

An assertion follows these rules:

- The condition must have exactly type `bool`. The optional message must have exactly type `str`.
- The condition is evaluated exactly once. A true condition falls through without evaluating the message.
- A false condition evaluates the message exactly once and traps with `AU4001`. With no message, the failure text is exactly `assertion failed`. Otherwise the supplied `str` is kept exactly, including an empty or whitespace-only value.
- The diagnostic points to the `assert` keyword.
- A trap raised while evaluating the condition or the message happens first and stays primary.
- A failed assertion runs active `with` cleanups. The assertion stays primary if cleanup also fails.

For static analysis, an assertion falls through normally. It does not refine the type or possible values of any later expression. The compiler does not strip assertions in any build mode.

Assertions are valid executable top-level statements in a script entry module. The usual rule against combining top-level execution with a local `main` still applies.

See [Assertions](/manual/assertions) for the complete contract and an executable example.

## `pass`

`pass` does nothing and introduces no binding:

```aura
def placeholder():
    pass
```

It must be on its own logical line. Use it for an intentionally empty function, method, class, trait, implementation, or control-flow suite. An enum body still needs at least one variant, and `pass` does not count as a variant.

## Module Constants, Imports, And Execution

Imports are module elements, not executable statements. Aura accepts these forms:

```aura fragment
import util.math
from util.math import double, triple
import agents.telemetry as telemetry
from agents.telemetry import record as record_event, Event
```

- Import paths are dot-separated identifiers.
- A module import may bind the whole module under a local alias.
- Each name in a from-import may have its own local name. Renamed and direct imports may appear together.
- A renamed import introduces only its local name into the importing module.

Aliases are static local names for resolved modules and declarations. They do not change visibility, nominal identity, trait implementations, initialization storage, or the package path used for resolution.

Aura does not accept wildcard imports, relative-dot imports, parenthesized import lists, or trailing import commas. [Packages](/manual/names-and-scopes#imports) defines import resolution and visibility.

An immutable binding at module level is a module constant:

```aura
message = "hello"
public retry_limit: int64 = 3

def main():
    print(message)
```

The initializer is required. The checker rejects `mut` module storage and later assignment. Constants may coexist with a local `main`. Constants in reachable dependencies initialize before the entry runs. [Names And Scopes](/manual/names-and-scopes#module-constants) defines the full scope, order, visibility, and ownership rules.

An entry module may also contain executable top-level statements:

    print(message)

These statements run in their stored source order, after the reachable module constants are ready. An entry module with executable top-level statements cannot define a local `main`. The top-level statements of an imported module do not run as a side effect of the import.

A top-level `mut name = value` statement declares `name` as a local of the entry script. A later `name = value`, or a compound assignment such as `name += value`, reassigns that same local:

```aura
mut count = 0
count = count + 1
count += 1
print(count)
```

A bare top-level binding of a new name is always a module constant, wherever it appears among the entry statements. It cannot read a top-level script local, because constants initialize before the entry runs. To keep the computation in the entry script, declare the new binding with `mut`. Otherwise, move the work into `main`.

[Functions](/manual/functions#main) and [Execution Model](/manual/execution-model#entry-module-execution) define the accepted `main` signatures and process exit behavior.

## Contextual Legality Summary

A statement that parses is not legal in every context:

- `return` requires a function or method.
- `break` and `continue` require an enclosing loop.
- Reassignment and compound assignment require an existing mutable place.
- Member and index assignment require a mutable base and cannot declare a type or use `mut`.
- Conditions require `bool`, not truthiness.
- Assertion conditions require `bool`, and assertion messages require `str`.
- Match arms must satisfy the compatibility, reachability, and exhaustiveness rules.
- `with` requires a supported resource and keeps its cleanup capability intact.
- Items cannot appear inside suites.
- Module constants are immutable and cannot use `mut` or reassignment.
- Module constants are the bindings above the first top-level statement. They cannot read top-level script locals, which initialize later.
- An entry module cannot mix executable top-level statements with a local `main`.

The complete checker rules are normative in [Static Semantics](/manual/static-semantics). Ownership effects are normative in [Ownership And Borrowing](/manual/ownership-and-borrowing).

## Grammar

[Grammar](/manual/grammar) is normative for the simple and compound statement productions, suite indentation, binding and assignment targets, loop modifiers, match arms, and `with` forms. Statements end at a `NEWLINE`. Aura has no semicolon-separated statements and no inline compound statements.

## Typing Rules

- A binding infers or checks one type. Reassignment keeps that type.
- Conditions have exactly type `bool`.
- Return values match the enclosing signature.
- The iterable determines the loop binding contract.
- Match patterns must be compatible, reachable, and exhaustive where required.
- `with` accepts only the maintained cleanup contract.
- Assertion conditions have exactly type `bool`, and assertion messages have exactly type `str`. An assertion does not refine later control flow.
- `is None` and `is not None` conditions refine a stable union place on the paths they select, as described in [Static Semantics](/manual/static-semantics#conditions).
- Contextual legality is checked after parsing.

## Runtime Semantics

- Statements run in source order within the selected suite.
- Simple-name and field assignment evaluate the right side before writing the target.
- Indexed assignment evaluates its collection and its index or key before the right side. A simple dict assignment captures its owned key before any side effects of the value.
- Compound assignment uses the corresponding binary dispatch and stores into the target it selected once. A copy target is captured before the right side. A non-copy root or projected target stays borrowed across the right side.
- Direct indexed compound assignment reads only a copy element. It traps with `AU4003` when a dictionary key is absent.
- A conditional runs at most one branch.
- A loop tests its condition, or receives a value, before each body.
- A match evaluates its scrutinee once.
- `with` registers cleanup only after resource construction succeeds.
- An assertion evaluates its condition once. It skips the message on success and evaluates the message once before failing.
- A control transfer runs every exited cleanup in reverse registration order.

## Ownership And Evaluation Order

- A binding owns, copies, or borrows its initializer, depending on type and context.
- `own` list and set iteration consumes the collection once into a loop-private source.
- Bare collection iteration retains its selected place and freezes it.
- Queue iteration copies the handle once and receives items that are already owned.
- Simple dict indexed assignment consumes non-copy keys and values into owned storage.
- Direct list and dict indexed compound assignment is limited to copy elements.
- Assignment to a place invalidates conflicting borrows and reinitializes the written place.
- Branch and loop analysis conservatively keeps any move that may reach a continuing path. No control-flow join restores ownership implicitly.

## Diagnostics

Compile-time diagnostics:

| Code | Meaning |
| --- | --- |
| `AU1101` | Malformed statement or suite syntax. |
| `AU2001` | Unresolved name or target. |
| `AU2002` | Expected-type, condition, iteration, match, return, or assignment mismatch. |
| `AU2003` | Unsupported compound-assignment operator. |
| `AU2004` | Call or target argument binding failed. |
| `AU2005` | Unsupported syntax or feature for a Python-shaped statement. |
| `AU2013` | A union match is missing a type arm, duplicates one, or cannot reach one. |
| `AU2014` | A member use through a place whose `None`-test narrowing was ended by an assignment, a mutable match, or a call with mutable access. |
| `AU2999` | An exhaustiveness, contextual-legality, or unsupported-statement rejection with no narrower code. |
| `AU3001` | Use of a moved place. |
| `AU3002` | Borrow conflict. This includes a later access that mutably borrows or consumes an overlapping, retained non-copy target of a compound or indexed assignment. |
| `AU3003` | An immutable target was used mutably. |
| `AU3004` | Invalid loop, parameter, or ownership mode. |
| `AU3005` | A non-copy direct indexed read. |
| `AU3006` | A non-copy indexed compound assignment. |

Runtime diagnostics:

| Code | Meaning |
| --- | --- |
| `AU4001` | General statement trap. |
| `AU4002` | Numeric range, overflow, or underflow failure. |
| `AU4003` | Bounds or lookup violation. |
| `AU4004` | Zero divisor. |
| `AU4005` | Trapping resource or I/O failure. This includes a cleanup failure when no earlier body failure stays primary. |

A failed assertion reports `AU4001` with `assertion failed` or the exact custom message, and points to its keyword.

## Backend Support

Every implemented statement form uses the same checker and MIR lowering for MIR execution and for direct native generation. The backend-parity suite forces cleanup, loop, match, task, and runtime-trap behavior to agree. Where direct lowering is unsupported, it is contained rather than silently given different semantics.

## Limits And Implementation-Defined Behavior

- A suite requires a real statement.
- Loops have no `else` clause.
- Statement match arms cannot be inline.
- A statement can span physical lines only inside an open `(`, `[`, or `{`. Backslash continuation is not available.
- Items cannot nest inside suites.
- Range iteration yields copy `int64` values and accepts only the bare form, as described above.

No statement evaluation order is implementation-defined.

## Status

Bindings, assignments, expression statements, `return`, `assert`, conditionals, loops, `match`, scoped cleanup, `pass`, imports, and top-level execution in entry modules are implemented as described. Tuple targets for assignment and loops are implemented.

Class and collection destructuring, loop `else`, exception statements, `yield`, `raise`, `async`, and nested declarations are not available. `try` is an expression over `Result`.

Design record: the loop ownership modes are those accepted in ADR-0006. One-time iterable selection is the accepted ADR-0017 rule. Tuple assignment and loop targets follow Accepted ADR-0026.
