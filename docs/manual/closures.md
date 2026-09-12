# Closures

Aura closures use `lambda parameters: expression`. They are small
expression-bodied callable values. Parameter types come from context; a
zero-parameter lambda may infer its result type from its body. A lambda without
a capture list captures by value. An explicit exhaustive capture list can
instead request shared, mutable, or owned access to named outer locals.

```aura
def main():
    factor: int32 = 2
    scale: def(int32) -> int32 = lambda value: value * factor

    name = "Aura"
    length: def() -> int64 = lambda: name.len()

    token = "owned"
    take: def() -> str = lambda: token

    print(scale(21))
    print(scale(6))
    print(length())
    print(length())
    print(take())
```

This prints `42`, `12`, `6`, `6`, and `owned` on separate lines. `factor` is
copied into `scale`; `name` moves into a read-only, repeatable closure; and
`token` moves into a consuming closure that is called once.

## Grammar

The closure productions are:

```ebnf
lambda-expression
    = "lambda", [ lambda-capture-list ],
      [ lambda-parameter,
      { ",", lambda-parameter } ], ":", expression ;

lambda-capture-list
    = "[", lambda-capture,
      { ",", lambda-capture }, "]" ;

lambda-capture
    = [ "mut" | "own" ], identifier ;

lambda-parameter
    = [ "mut" | "own" ], identifier ;
```

A lambda is the lowest-precedence expression form. The body is exactly one
expression; the colon does not introduce an indented suite. Parameter lists do not accept
types, defaults, or a trailing comma. Zero parameters use `lambda:
expression`.

`lambda` is a contextual expression introducer: the lexer still produces an
identifier token, but the spelling always begins a lambda at the start of an
expression. Member and named-argument positions may use the same identifier
spelling. A lambda may appear anywhere an expression is accepted, subject to
the contextual typing rule below. There is no arrow spelling, statement body,
`async` form, or nested `def`.

## Typing Rules

A lambda with parameters requires a complete expected parameter contract from
a structural function type such as `def(T1, mut T2, own T3) -> R`. The
expected type fixes the parameter count and each parameter's capability and
type. An expected result type also constrains the body. A zero-parameter
`lambda: expression` may instead infer `def() -> R` from the body when no
expected callable type is present.

```aura
shared: def(str) -> int64 = lambda text: text.len()
owned: def(own str) -> str = lambda own text: text
push_one: def(mut list[int32]) -> None = lambda mut values: values.append(1)
```

A bare lambda parameter matches a bare shared parameter, `own name` matches an
owned parameter, and `mut name` matches a mutable parameter. Modes cannot be
silently changed. The body must have exactly the expected result type.
Parameters are in scope only in the body and follow the ordinary no-shadowing
rules.

The expected contract's names, keyword-only boundary, and default promises
bind the lambda too (see [Function Values](/manual/functions#function-values)).
Where the expected slot is named, the lambda parameter must use that name;
the lambda's `*` boundary must sit where the expected contract's does; and a
lambda cannot satisfy a slot written `= ...`, because only a named
declaration supplies a default. Each mismatch reports `AU2015`. A
compiler-known callback site such as `list.map` supplies positional parameter
types only, so its lambda parameters may use any names.

```aura
combine: def(left: int64, *, right: int64) -> int64 = lambda left, *, right: left - right
print(combine(10, right=3))
```

The compiler does not guess parameter types from body operations. Generic
lambdas and lambda parameter type annotations are unavailable. A capture-free
lambda uses the ordinary function-value representation and is Copy and
Transfer. It may appear anywhere an ordinary function value can appear,
including arguments, fields, collections, and returns.

A capturing closure retains semantic environment and call-kind metadata that
an arbitrary written `def(...) -> R` storage type does not describe. It may be
held in an immutable inferred or contextually typed local, called directly,
passed directly to compiler-known repeatable callback sites such as the list
algorithms and `control.retry` (which also borrow a packed Shared value with
the same contract), moved into a qualifying task start, or packed
into an owned callable storage type. It cannot be coerced through a thin
`def` parameter, field, element, or annotated result; those boundaries
report `AU2002`.

`Callable[def(...) -> R]`, `Callable[mut def(...) -> R]`, and
`Callable[own def(...) -> R]` are owned storage types for a Shared, Mutable,
or Consuming callable with a complete contract (see [Function
Values](/manual/functions#function-values)) and an erased capture set;
`TaskCallable[...]` is the same storage whose every capture is proven
Transfer. Packing is explicit: calling a non-generic alias of such a type
with one value packs it, and the packed value keeps the closure's own
environment.

```aura
class Counter:
    total: int64

    def bump(mut self) -> int64:
        self.total = self.total + 1
        return self.total

type Reader = Callable[def() -> int64]
type Bumper = Callable[mut def() -> int64]
type Finish = Callable[own def() -> str]

def make_reader(base: int64) -> Reader:
    offset = base + 1
    return Reader(lambda: offset)

def make_bumper() -> Bumper:
    counter = Counter(total=0)
    return Bumper(lambda [own counter]: counter.bump())

reader = make_reader(1)
print(reader())
print(reader())
def make_finish(text: own str) -> Finish:
    return Finish(lambda [own text]: text)

def finish_once(finish: own Finish) -> str:
    return finish()

mut bumper = make_bumper()
print(bumper())
print(bumper())
print(finish_once(make_finish("done")))
```

The packed value's contract must be admitted by the storage contract, and
its call kind may only weaken: a Shared value packs into any kind, a
Mutable value into Mutable or Consuming storage, and a Consuming value only
into Consuming storage; the reverse reports `AU2015`. A closure or function
value that reaches a `Callable` destination without the constructor reports
`AU2015` as implicit erased storage. A loan capture (`[values]`,
`[mut values]`, or a live view) cannot be packed (`AU3010`); move or clone
an owned value into the lambda instead. Packing into `TaskCallable[...]`
additionally requires every capture type to be Transfer (`AU3008`, naming
the capture); an ordinary erased `Callable` is never Transfer.

Packed values are non-Copy and non-cloneable, so `dict.get`, snapshots, and
generic cloning reject them (`AU3007`), they have no equality (`AU2008`), and
they move like any owned value (`AU3001` after a move or a consuming call).
They may be stored in parameters (`mut Bumper`, `own Finish`), results,
class fields, list and dictionary elements, and cross modules through public
aliases. A Shared value is called through any access, a Mutable value only
through a mutable place (`AU3003`), and a Consuming value once, by an owner;
a callable element of a list or dictionary is called through a call-scoped
shared read, and a Consuming element must be removed before it is consumed.

A conditional or `match` expression merges capturing closures only through
an explicit common `Callable` contract on the destination (an annotation,
parameter, or return type): each reached branch packs its own value, and
untaken lambda expressions acquire no captures. Without such a contract,
different capturing closures still cannot merge; call the closure inside
each branch, or return capture-free lambdas or named functions with one
structural `def(...) -> R` type.

A resolved name in a lambda without a list is a capture only when it denotes
an outer owned local or an `own` parameter. Lambda parameters, module
functions, types, builtins, and imported items are resolved normally and are
not stored in the environment.

An explicit list is exhaustive. Every resolved outer local used by the body
must appear exactly once, and every entry must be used. Entries are acquired
left to right:

| Entry | Contract |
| --- | --- |
| `value` | Shared live loan of `value`. |
| `mut value` | Exclusive mutable live loan; `value` must be a mutable place or mutable view. |
| `own value` | By-value Copy snapshot or move under ADR-0037. |

A projected place must first be named by a `view` binding. A shared or mutable
view can be reborrowed but cannot be captured with `own`. A bare capture of a
Copy local is intentionally live; write `own value` for a snapshot.

## Runtime Semantics

Evaluating a lambda constructs its callable value immediately. For an ordinary
lambda, each captured Copy value is snapshotted and each captured non-Copy
owned value moves. An explicit list instead acquires its declared loans or
owned captures left to right. Loan releases belong to the closure environment
and run exactly once when its final use or scope ends.

Calling the closure evaluates arguments under its contextual structural
function signature and then evaluates the body. A closure whose body only
reads captures borrows its environment for the call and can be invoked
repeatedly. A body that consumes a non-Copy capture consumes the closure on
its first call. The existing move checker rejects another call or use.

Capture-free lambdas dispatch as ordinary function values. Capturing closures
carry an owned environment and are non-Copy, including when their captures
are individually Copy. They are also not clone-safe: a clone-producing
generic specialization that would duplicate the environment reports
`AU3007`. Use a named function or capture-free lambda when a callable must be
copied or cloned.

## Ownership And Evaluation Order

Implicit capture is by value and happens at closure creation, not on the first call.
Copy captures leave their sources usable. Non-Copy captures move, so using the
outer source afterward reports `AU3001`. Clone before creation when both
owners are required:

```aura
def main():
    name = "Aura"
    kept = name.clone()
    length: def() -> int64 = lambda: kept.len()
    print(name)
    print(length())
```

A bare or mutable enclosing parameter may be named in an explicit capture
list, which creates a loan bounded by the closure's live region. Without a
list, those capabilities are not captured; take the input as `own` or clone it
into an owned local for by-value capture.

An inner lambda without a list cannot capture a bare parameter of its enclosing
lambda. An explicit bare entry creates a shared contained reborrow; a `mut`
entry requires a mutable outer capability. When an independent snapshot is
needed, make the outer parameter `own` and use an `own` capture, or pass the
value to a named helper that creates the owned closure.

A by-value closure environment is read-only unless the body mutates one of
its captures: a `mut` operation on an owned capture, whether it moved or
copied into the environment implicitly or through `own name`, or a write
through a `mut` entry from an explicit capture list. Such a closure is
Mutable (mutable-repeatable): it must be held in a `mut` local or called
through a mutable place, and calls run sequentially through that place. A
closure whose body only reads its captures, including a shared-loan closure,
is Shared (shared-repeatable). A body that consumes a non-Copy `own` capture
remains a consuming, single-use closure, even when the environment also
contains loans.

A by-value closure is Transfer exactly when all of its captures are Transfer. Moving a
qualifying closure into `TaskGroup.start`, `start_soon`, or an explicit-stack
variant transfers the complete environment to child-owned storage. A
non-Transfer leaf retains the ordinary `AU3008` boundary explanation. A
closure containing any shared or mutable loan is always non-Transfer and
cannot cross a task, Queue, supervisor, detached-work, or FFI boundary.

A body may call a `mut` operation on an owned capture, whether it moved or
copied into the environment implicitly or through `own name`. The closure is
then Mutable: it must be held in a `mut` local or called through a mutable
place, and the updated capture value persists in the environment across
calls, so a packed counter counts across invocations. The original outer
binding is not changed. A shared-view capture stays read-only (`AU3003`);
capture it `mut` for a mutable view of the original place instead.

### Comprehension Interaction

Comprehensions do not change closure capture. A lambda enclosing a
comprehension captures outer names used by iterable, filter, and output
expressions, while comprehension targets are local bindings in the lambda body
and are not captures.

A lambda expression reached inside a comprehension is created at that runtime
position. It may snapshot a Copy target, and it may move a Queue-received owned
target when the surrounding use permits one consuming closure. A shared
non-Copy list/set target may be captured only through an explicit loan list
whose lifetime remains inside the synchronous containing task; otherwise pass
an explicit clone to a named helper or arrange another owned value outside the
comprehension.

The ordinary storage boundary also remains. A capturing closure cannot itself
be inserted as a list, set, or dictionary comprehension result because collection
storage erases its environment/call-kind metadata. It may be used immediately
at a compiler-known callback or direct-call site inside an iterable, filter,
key, value, or element expression. Such creation happens only when preceding
clauses and filters reach it, and repeatable callback sites still reject a
consuming closure.

## Diagnostics

`AU1101` reports malformed lambda parameter or body syntax. `AU2002` reports a
missing or mismatched parameter context, parameter capability, result type,
thin-`def` storage boundary, or a consuming closure supplied where a
repeatable callback is required. `AU2015` reports implicit erased storage
(a closure or function reaching `Callable[...]` without its constructor), a
packed value whose contract is not admitted, or a call kind that would
strengthen. `AU3001` reports use after a non-Copy value moved into a closure
and use after a consuming closure or consuming callable call. `AU3002`
rejects overlapping capture loans and source accesses. `AU3003` rejects
capability escalation, mutation of a shared-view capture, or a Mutable call
through an immutable place. `AU3007` rejects duplicating a packed callable.
`AU3008` reports a closure whose captured environment cannot cross a task
boundary or become `TaskCallable` because some captured value is not
Transfer. `AU3010` reports a loan closure escaping into storage, a loan
capture packed into owned callable storage, or a metadata-erasing callable
boundary.

The shared-capability diagnostic recommends cloning to an owned local or
taking owned input. Move diagnostics identify closure creation or the
consuming call as the ownership origin.

## Backend Support

Contextual checking, capture and loan analysis, move checking, MIR lowering,
and direct native lowering implement the same closure contract. Both maintained
backends copy, move, or loan captures at creation; preserve shared- and
mutable-repeatable calls; enforce single-use consumption; write mutable loans
through immediately; and clean up an environment exactly once. Compiler
analysis and the language server expose lambda parameter scope,
captured-name definitions, callable hover, completions, and the compiler-owned
diagnostics.

## Bound Methods

`receiver.method` outside call position is a bound method: a
compiler-synthesized closure whose single capture is the receiver and whose
contract is the method's own parameter names, keyword-only boundary, default
availability, passing modes, and result. The call kind follows the receiver
capability: a `self` method binds a Repeatable closure, a `mut self` method
binds a Mutable closure whose receiver is environment-owned state updated on
every call, and an `own self` method binds a Consuming closure. The receiver
is acquired exactly once when the bound method is created. A `copy class`
receiver is snapshotted wherever it is reached — an owned local, a shared or
mutable parameter, or a `view` — so the closure is an independent owned
value and later calls never change the original. Any other receiver must be
an owned local, which moves into the closure, or a fresh temporary. A
non-Copy shared or mutable parameter and a view of a non-Copy value cannot be
bound by this spelling; clone the value into an owned local first. Trait
methods selected for the concrete receiver type bind the same way and
dispatch statically, but their contract is the trait's public one: the
trait's parameter names, keyword-only boundary, and default availability,
never the implementation's local parameter names. Each slot, including a
returned-view origin, forwards to the implementation by ordinal. In the
current compiler an implementation whose keyword-only parameter is named
differently from the trait's cannot be bound (`AU2005`); name it as the trait
does, or call the method directly. An omitted argument with a declaration
default is supplied by the method's own default expression, evaluated at the
call. Bound methods store, pack, and start like any closure with the same
call kind.

```aura
class Counter:
    total: int64

    def read(self) -> int64:
        return self.total

    def bump(mut self, by: int64 = 1) -> int64:
        self.total = self.total + by
        return self.total

type Bumper = Callable[mut def(by: int64 = ...) -> int64]

def main():
    counter = Counter(total=1)
    read = counter.read
    print(read())
    mut bump = Bumper(Counter(total=10).bump)
    print(bump())
    print(bump(by=5))
```

A generic method's type arguments must be concrete: write them explicitly
as `receiver.method[T]` (or `Class.method[T]` for an associated method), or
let an expected callable contract supply them, such as the contract of a
`Callable[...]` constructor call or an annotated function-typed local for an
associated method. A bare generic method reference reports `AU2005`, a
wrong type-argument count or an unsatisfiable bound reports `AU2002`, and an
associated method of a generic class still needs a call (`AU2005`). A
`view ... from self` method cannot be bound (`AU3010`), while a method
returning a view of one of its other parameters keeps that contract in the
bound closure (see Stored View Contracts). Binding a non-Copy shared or
mutable parameter reports `AU3002`, binding a view of a non-Copy value
reports `AU3004`, and a later use of the moved receiver reports `AU3001`
pointing at the binding site. Calling a Mutable bound method through an
immutable local reports `AU3003`.

## Stored View Contracts

A function whose result is `view [mut] T from name` for one of its ordinary
parameters keeps that contract in its value type, written
`def(pair: Pair) -> view str from pair`. The origin is one named parameter
of the contract; a positional-only slot or an unknown name is rejected
(`AU3010`), and a `view mut` result requires a `mut` origin parameter. Such
a value stores, packs into `Callable[...]`, and passes like any other
callable with an identical contract: an owned-result contract does not admit
a view-returning function and a view contract does not admit an
owned-result function (`AU2002`). A call through the value is a returned-view
call: its result must initialize a `view` binding (`AU3010`), a `view mut`
result needs a mutable origin argument, and because the callee behind a value
is opaque the view's footprint is every fixed field path or tuple position of
the origin argument whose type is the result type, so the whole origin stays
borrowed for the binding's lifetime. A `from self` result has no value type,
so a bound method returning a view of its receiver cannot be stored
(`AU3010`); a bound method whose view originates in another parameter keeps
that contract, and a `TaskCallable[...]` contract cannot return a view
(`AU3008`).

```aura
class Pair:
    left: str
    right: str

def pick_left(pair: Pair) -> view str from pair:
    return view pair.left

def pick_right(pair: Pair) -> view str from pair:
    return view pair.right

type Picker = Callable[def(pair: Pair) -> view str from pair]

def main():
    pair = Pair(left="ada", right="linus")
    packed = Picker(pick_right)
    view tail = packed(pair)
    print(tail)
    chooser: def(pair: Pair) -> view str from pair = pick_left
    view head = chooser(pair)
    print(head)
```

## Limits And Implementation-Defined Behavior

Closures are expression-only and contextually typed. They do not support
statement bodies, inline parameter types, defaults, generics, implicit
reference capture, trait objects, FFI callbacks, asynchronous
syntax, returned loan closures, or lifetime-bearing structural callable types.
Owned callable storage holds owned captures only; loan captures stay in
local loan closures. A stored callable may return a view of one explicit
parameter but never of a captured receiver. A packed value keeps the closure's existing environment
as its storage, so packing allocates no second environment on either
backend; the checkpoint's inline-buffer plan is the native ABI target this
representation stands in for. A `TaskCallable[...]` value is a stored task
target: every `TaskGroup` start method takes it by move for one child call,
whether Shared, Mutable, or Consuming, and a Mutable target's captures are
child-owned state with no parent writeback (see
[Concurrency](/manual/concurrency)).
Explicit lists accept local identifiers; project a field into a named view
before capturing it.

Arbitrary structural `def` parameters and stored `def` fields, collection
elements, and annotated returns currently carry only capture-free code
pointers. Compiler-known callback sites preserve repeatable closure metadata
and borrow packed Shared values with an ABI-equal, positionally callable
contract; `control.retry` and the list callbacks reject consuming and
Mutable callbacks. Task start accepts a qualifying closure by move for one
invocation.

Conditional and `match` expressions cannot merge capturing closures from
multiple branches unless the destination carries an explicit common
`Callable` contract, in which case each reached branch packs its own value
(see the merge rule above). This is an explicit closure-union boundary, not
an implementation-defined coercion.

The capture, callability, ownership, and Transfer rules are language-defined;
the implementation does not choose a reference-versus-value capture mode.

## Status

Expression closures and by-value capture are implemented under Accepted ADR-0037
(`architecture_docs/decisions/0037-expression-closures-and-value-capture.md`)
after ratification at the Batch 6 opening checkpoint. Capture-free function
values remain governed by [Functions](/manual/functions), and task-boundary
Transfer remains governed by Accepted ADR-0033. Comprehensions preserve this
contract under Accepted ADR-0039. Explicit shared/mutable/owned capture lists,
mutable-repeatable closure calls, and loan cleanup are implemented under
ADR-0038.
