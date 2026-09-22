# Closures

A closure is a small callable value with an expression body, written `lambda parameters: expression`. Parameter types come from context. A zero-parameter lambda may infer its result type from its body. A lambda without a capture list captures by value. An explicit capture list names outer locals and requests shared, mutable, or owned access to each one, and it must be exhaustive.

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

This prints `42`, `12`, `6`, `6`, and `owned` on separate lines:

- `factor` is copied into `scale`.
- `name` moves into a read-only closure that can be called repeatedly.
- `token` moves into a consuming closure that is called once.

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

- A lambda is the lowest-precedence expression form.
- The body is exactly one expression. The colon does not introduce an indented suite.
- Parameter lists do not accept types, defaults, or a trailing comma.
- Zero parameters use `lambda: expression`.
- A lambda may appear anywhere an expression is accepted, subject to the contextual typing rule below.
- There is no arrow spelling, statement body, `async` form, or nested `def`.

`lambda` is a contextual expression introducer. The lexer produces an identifier token for it, but at the start of an expression the spelling always begins a lambda. Member and named-argument positions may use the same identifier spelling.

## Typing Rules

### Parameter Contracts

A lambda with parameters needs a complete expected parameter contract from a structural function type such as `def(T1, mut T2, own T3) -> R`. The expected type fixes the parameter count and each parameter's capability and type. An expected result type also constrains the body. With no expected callable type, a zero-parameter `lambda: expression` may infer `def() -> R` from its body.

```aura
shared: def(str) -> int64 = lambda text: text.len()
owned: def(own str) -> str = lambda own text: text
push_one: def(mut list[int32]) -> None = lambda mut values: values.append(1)
```

- A bare lambda parameter matches a bare shared parameter.
- `own name` matches an owned parameter.
- `mut name` matches a mutable parameter.
- Modes are never changed silently.
- The body must have exactly the expected result type.
- Parameters are in scope only in the body and follow the ordinary no-shadowing rules.

The expected contract's names, keyword-only boundary, and default promises also bind the lambda. See [Function Values](/manual/functions#function-values). Each of these mismatches reports `AU2015`:

- Where the expected slot is named, the lambda parameter must use that name.
- The lambda's `*` boundary must sit where the expected contract's boundary sits.
- A lambda cannot satisfy a slot written `= ...`, because only a named declaration supplies a default.

A compiler-known callback site such as `list.map` supplies positional parameter types only, so its lambda parameters may use any names.

```aura
combine: def(left: int64, *, right: int64) -> int64 = lambda left, *, right: left - right
print(combine(10, right=3))
```

The compiler does not guess parameter types from operations in the body. Generic lambdas and lambda parameter type annotations are not available.

### Capture-Free And Capturing Closures

A capture-free lambda uses the ordinary function-value representation and is Copy and Transfer. It may appear anywhere an ordinary function value can appear, including arguments, fields, collections, and returns.

A capturing closure carries environment and call-kind metadata that a written `def(...) -> R` storage type does not describe. A capturing closure may be:

- held in an immutable local that is inferred or contextually typed
- called directly
- passed directly to a compiler-known repeatable callback site, such as the list algorithms and `control.retry`. These sites also borrow a packed Shared value with the same contract.
- moved into a qualifying task start
- packed into an owned callable storage type

It cannot pass through a thin `def` parameter, field, element, or annotated result. Those boundaries report `AU2002`.

### Owned Callable Storage

`Callable[def(...) -> R]`, `Callable[mut def(...) -> R]`, and `Callable[own def(...) -> R]` are owned storage types. They hold a Shared, Mutable, or Consuming callable with a complete contract and an erased capture set. See [Function Values](/manual/functions#function-values) for contracts. `TaskCallable[...]` is the same storage with every capture proven Transfer.

Packing is explicit. Call a non-generic alias of such a type with one value to pack it. The packed value keeps the closure's own environment.

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

Packing rules:

- The storage contract must admit the packed value's contract.
- The call kind may only weaken. A Shared value packs into any kind. A Mutable value packs into Mutable or Consuming storage. A Consuming value packs only into Consuming storage. The reverse reports `AU2015`.
- A closure or function value that reaches a `Callable` destination without the constructor reports `AU2015` as implicit erased storage.
- A loan capture cannot be packed and reports `AU3010`. Loan captures include `[values]`, `[mut values]`, and a live view. Move or clone an owned value into the lambda instead.
- Packing into `TaskCallable[...]` also requires every capture type to be Transfer. Otherwise it reports `AU3008`, naming the capture. An ordinary erased `Callable` is never Transfer.

Packed values are non-Copy and non-cloneable:

- `dict.get`, snapshots, and generic cloning reject them with `AU3007`.
- They have no equality, which reports `AU2008`.
- They move like any owned value, and use after a move or a consuming call reports `AU3001`.

Packed values may be stored in parameters such as `mut Bumper` and `own Finish`, in results, class fields, and list and dictionary elements. They cross modules through public aliases.

How a packed value is called depends on its kind:

- A Shared value is called through any access.
- A Mutable value is called only through a mutable place. Otherwise the call reports `AU3003`.
- A Consuming value is called once, by an owner.
- A callable element of a list or dictionary is called through a shared read scoped to the call. A Consuming element must be removed before it is consumed.

### Merging Closures In Branches

A conditional or `match` expression merges capturing closures only when the destination has an explicit common `Callable` contract, from an annotation, parameter, or return type. Each reached branch packs its own value, and untaken lambda expressions acquire no captures. Without such a contract, different capturing closures cannot merge. Call the closure inside each branch instead, or return capture-free lambdas or named functions that share one structural `def(...) -> R` type.

### What A Lambda Captures

In a lambda without a list, a resolved name is a capture only when it denotes an outer owned local or an `own` parameter. Lambda parameters, module functions, types, builtins, and imported items resolve normally and are not stored in the environment.

An explicit list is exhaustive. Every resolved outer local that the body uses must appear exactly once, and every entry must be used. Entries are acquired left to right:

| Entry | Contract |
| --- | --- |
| `value` | Shared live loan of `value`. |
| `mut value` | Exclusive mutable live loan. `value` must be a mutable place or mutable view. |
| `own value` | By-value Copy snapshot or move. |

- A projected place must first be named by a `view` binding.
- A shared or mutable view can be reborrowed but cannot be captured with `own`.
- A bare capture of a Copy local is a live loan by design. Write `own value` for a snapshot.

## Runtime Semantics

Evaluating a lambda constructs its callable value immediately. An ordinary lambda snapshots each captured Copy value and moves each captured non-Copy owned value. An explicit list instead acquires its declared loans or owned captures left to right. Loan releases belong to the closure environment. They run exactly once, when the closure's final use or scope ends.

A call evaluates the arguments under the closure's contextual structural function signature, then evaluates the body. A closure whose body only reads its captures borrows its environment for the call and can be called repeatedly. A body that consumes a non-Copy capture consumes the closure on its first call, and the move checker rejects any later call or use.

Capture-free lambdas dispatch as ordinary function values. Capturing closures carry an owned environment and are non-Copy, even when every capture is Copy. They are also not clone-safe. A clone-producing generic specialization that would duplicate the environment reports `AU3007`. Use a named function or a capture-free lambda when a callable must be copied or cloned.

## Ownership And Evaluation Order

Implicit capture is by value and happens when the closure is created, not on the first call. Copy captures leave their sources usable. Non-Copy captures move, so using the outer source afterward reports `AU3001`. Clone before creation when both owners are needed:

```aura
def main():
    name = "Aura"
    kept = name.clone()
    length: def() -> int64 = lambda: kept.len()
    print(name)
    print(length())
```

### Capturing Parameters

An explicit capture list may name a bare or mutable enclosing parameter. This creates a loan bounded by the closure's live region. Without a list, those parameters are not captured. Take the input as `own`, or clone it into an owned local, for by-value capture.

An inner lambda without a list cannot capture a bare parameter of its enclosing lambda. An explicit bare entry creates a shared contained reborrow. A `mut` entry requires a mutable outer capability. When you need an independent snapshot, make the outer parameter `own` and use an `own` capture, or pass the value to a named helper that creates the owned closure.

### Call Kinds

Every closure has one of three call kinds:

- **Shared**, or shared-repeatable. The body only reads its captures. A closure over shared loans is Shared.
- **Mutable**, or mutable-repeatable. The body mutates a capture. It calls a `mut` operation on an owned capture, whether that capture moved or copied in implicitly or through `own name`. Or it writes through a `mut` entry from an explicit capture list.
- **Consuming**. The body consumes a non-Copy `own` capture. The closure is single-use, even when its environment also holds loans.

A Mutable closure must be held in a `mut` local or called through a mutable place, and its calls run one after another through that place. The updated capture value persists in the environment across calls, so a packed counter keeps counting across calls. The original outer binding does not change. A shared-view capture stays read-only and mutation reports `AU3003`. Capture it with `mut` to get a mutable view of the original place instead.

### Transfer And Tasks

A by-value closure is Transfer exactly when all of its captures are Transfer. Moving a qualifying closure into `TaskGroup.start`, `start_soon`, or an explicit-stack variant transfers the whole environment to child-owned storage. A non-Transfer leaf gets the ordinary `AU3008` boundary explanation. A closure that holds any shared or mutable loan is always non-Transfer. It cannot cross a task, Queue, supervisor, detached-work, or FFI boundary.

### Comprehension Interaction

Comprehensions do not change closure capture. A lambda that encloses a comprehension captures the outer names used by the iterable, filter, and output expressions. Comprehension targets are local bindings in the lambda body, not captures.

A lambda expression inside a comprehension is created at that runtime position:

- It may snapshot a Copy target.
- It may move a Queue-received owned target when the surrounding use permits one consuming closure.
- It may capture a shared non-Copy list or set target only through an explicit loan list whose lifetime stays inside the synchronous containing task. Otherwise, pass an explicit clone to a named helper, or arrange another owned value outside the comprehension.

The ordinary storage boundary still applies. A capturing closure cannot be the result of a list, set, or dictionary comprehension, because collection storage erases its environment and call-kind metadata. It may be used immediately at a compiler-known callback or direct-call site inside an iterable, filter, key, value, or element expression. It is created only when the preceding clauses and filters reach it. Repeatable callback sites still reject a consuming closure.

## Diagnostics

| Code | Cause |
| --- | --- |
| `AU1101` | Malformed lambda parameter or body syntax. |
| `AU2002` | Missing or mismatched parameter context, parameter capability, or result type. A capturing closure at a thin `def` storage boundary. A consuming closure where a repeatable callback is required. |
| `AU2015` | Implicit erased storage, where a closure or function reaches `Callable[...]` without its constructor. A packed value whose contract is not admitted. A call kind that would strengthen. |
| `AU3001` | Use after a non-Copy value moved into a closure. Use after a consuming closure or consuming callable call. |
| `AU3002` | Overlapping capture loans and source accesses. |
| `AU3003` | Capability escalation, mutation of a shared-view capture, or a Mutable call through an immutable place. |
| `AU3007` | Duplicating a packed callable. |
| `AU3008` | A captured value is not Transfer, so the environment cannot cross a task boundary or become `TaskCallable`. |
| `AU3010` | A loan closure escaping into storage, a loan capture packed into owned callable storage, or a callable boundary that erases metadata. |

The shared-capability diagnostic recommends cloning to an owned local or taking owned input. Move diagnostics name the closure creation or the consuming call as the ownership origin.

## Backend Support

Contextual checking, capture and loan analysis, move checking, MIR lowering, and direct native lowering implement the same closure contract. Both maintained backends:

- copy, move, or loan captures at creation
- preserve shared-repeatable and mutable-repeatable calls
- enforce single-use consumption
- write through mutable loans immediately
- clean up an environment exactly once

Compiler analysis and the language server provide lambda parameter scope, definitions for captured names, callable hover, completions, and the compiler-owned diagnostics.

## Bound Methods

`receiver.method` outside a call position is a bound method. It is a compiler-synthesized closure whose single capture is the receiver. Its contract is the method's own parameter names, keyword-only boundary, default availability, passing modes, and result.

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

The call kind follows the receiver:

| Method receiver | Bound closure |
| --- | --- |
| `self` | Repeatable. |
| `mut self` | Mutable. The receiver is environment-owned state that every call updates. |
| `own self` | Consuming. |

The receiver is acquired exactly once, when the bound method is created:

- A `copy class` receiver is snapshotted wherever it is reached: an owned local, a shared or mutable parameter, or a `view`. The closure is an independent owned value, and later calls never change the original.
- Any other receiver must be an owned local, which moves into the closure, or a fresh temporary.
- A non-Copy shared or mutable parameter, or a view of a non-Copy value, cannot be bound this way. Clone the value into an owned local first.

Trait methods selected for the concrete receiver type bind the same way and dispatch statically. Their contract is the trait's public one: the trait's parameter names, keyword-only boundary, and default availability, never the implementation's local parameter names. Each slot, including a returned-view origin, forwards to the implementation by position. So an implementation may name its parameters, including keyword-only ones, as it likes.

When a call omits an argument that has a declaration default, the method's own default expression supplies it, evaluated at the call. Bound methods store, pack, and start like any closure with the same call kind.

### Generic And View-Returning Methods

A generic method's type arguments must be concrete. Write them explicitly as `receiver.method[T]`, or `Class.method[T]` for an associated method. Or let an expected callable contract supply them, such as the contract of a `Callable[...]` constructor call, or an annotated function-typed local for an associated method.

A `view ... from self` method cannot be bound. A method that returns a view of one of its other parameters keeps that contract in the bound closure. See [Stored View Contracts](#stored-view-contracts).

| Code | Cause |
| --- | --- |
| `AU2005` | A bare generic method reference. An associated method of a generic class named without a call. |
| `AU2002` | A wrong type-argument count or an unsatisfiable bound. |
| `AU3010` | Binding a `view ... from self` method. |
| `AU3002` | Binding a non-Copy shared or mutable parameter. |
| `AU3004` | Binding a view of a non-Copy value. |
| `AU3001` | A later use of the moved receiver. The diagnostic points at the binding site. |
| `AU3003` | Calling a Mutable bound method through an immutable local. |

## Stored View Contracts

A function whose result is `view [mut] T from name`, for one of its ordinary parameters, keeps that contract in its value type. The type is written `def(pair: Pair) -> view str from pair`.

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

The contract:

- The origin is one named parameter of the contract. A positional-only slot or an unknown name reports `AU3010`.
- A `view mut` result requires a `mut` origin parameter.
- Such a value stores, packs into `Callable[...]`, and passes like any other callable with an identical contract.
- An owned-result contract does not admit a view-returning function, and a view contract does not admit an owned-result function. Both report `AU2002`.

A call through the value is a returned-view call:

- Its result must initialize a `view` binding, or the call reports `AU3010`.
- A `view mut` result needs a mutable origin argument.
- The callee behind a value is opaque. So the view's footprint is every fixed field path or tuple position of the origin argument whose type is the result type. The whole origin stays borrowed for the binding's lifetime.

A `from self` result has no value type. A bound method that returns a view of its receiver cannot be stored and reports `AU3010`. A bound method whose view originates in another parameter keeps that contract. A `TaskCallable[...]` contract cannot return a view and reports `AU3008`.

## Limits And Implementation-Defined Behavior

### Language Limits

- Closures are expression-only and contextually typed.
- They do not support statement bodies, inline parameter types, defaults, generics, implicit reference capture, trait objects, FFI callbacks, asynchronous syntax, returned loan closures, or lifetime-bearing structural callable types.
- Owned callable storage holds owned captures only. Loan captures stay in local loan closures.
- A stored callable may return a view of one explicit parameter, but never of a captured receiver.
- Explicit capture lists accept local identifiers. Project a field into a named view before capturing it.

### Thin `def` Types And Callback Sites

Arbitrary structural `def` parameters, stored `def` fields, collection elements, and annotated returns carry only capture-free code pointers in the current compiler. Compiler-known callback sites preserve repeatable closure metadata. They also borrow packed Shared values whose contract is ABI-equal and positionally callable. `control.retry` and the list callbacks reject consuming and Mutable callbacks. Task start accepts a qualifying closure by move for one call.

Conditional and `match` expressions cannot merge capturing closures from several branches unless the destination has an explicit common `Callable` contract. In that case each reached branch packs its own value, as described in [Merging Closures In Branches](#merging-closures-in-branches). This is an explicit closure-union boundary, not an implementation-defined coercion.

### Representation

- A packed value uses the closure's existing environment as its storage, so packing allocates no second environment on either backend.
- On the direct backend, every concrete union is one inline tagged value. A runtime-object member rides along as an owned handle word.
- On the direct backend, a callable value is four words: a descriptor that names its shape, meaning the lowered function and its captures, plus three environment words.
- Captures that fit in three words live in the value itself. A wider environment lives in one checked block, and a failure to allocate it raises `AU4005`.
- A call goes through the descriptor without boxing.
- The value is boxed into a runtime function value only where it leaves generated code: when it is stored into a container, started as a task, passed to a runtime helper, or handed to a generic frame whose contract differs in layout.
- This layout makes no FFI or ABI stability claim.

A `TaskCallable[...]` value is a stored task target. Every `TaskGroup` start method takes it by move for one child call, whether it is Shared, Mutable, or Consuming. A Mutable target's captures are child-owned state, with no writeback to the parent. See [Concurrency](/manual/concurrency).

The capture, callability, ownership, and Transfer rules are language-defined. The implementation does not choose between reference and value capture.

## Status

Implemented:

- expression closures and by-value capture
- explicit shared, mutable, and owned capture lists
- mutable-repeatable closure calls
- loan cleanup
- closures inside comprehensions, under the same contract

[Functions](/manual/functions) governs capture-free function values. The task-boundary Transfer rules govern closures that cross into tasks.

Design record: ADR-0037, `architecture_docs/decisions/0037-expression-closures-and-value-capture.md`, for expression closures and by-value capture. ADR-0038 for explicit capture lists, mutable-repeatable calls, and loan cleanup. ADR-0039 for comprehensions. ADR-0033 for task-boundary Transfer. All are Accepted.
