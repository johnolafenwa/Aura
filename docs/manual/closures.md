# Closures

Aurora closures use `lambda parameters: expression`. They are small
expression-bodied callable values. Parameter types come from context; a
zero-parameter lambda may infer its result type from its body. Captures are
always by value: Copy values are copied and owned non-Copy values are moved
when the closure is created.

```aurora
def main():
    factor: int32 = 2
    scale: def(int32) -> int32 = lambda value: value * factor

    name = "Aurora"
    length: def() -> int64 = lambda: name.len()

    token = "owned"
    take: def() -> String = lambda: token

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
    = "lambda", [ lambda-parameter,
      { ",", lambda-parameter } ], ":", expression ;

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
the contextual typing rule below. There is no arrow spelling, capture list,
statement body, `async` form, or nested `def`.

## Typing Rules

A lambda with parameters requires a complete expected parameter contract from
a structural function type such as `def(T1, mut T2, own T3) -> R`. The
expected type fixes the parameter count and each parameter's capability and
type. An expected result type also constrains the body. A zero-parameter
`lambda: expression` may instead infer `def() -> R` from the body when no
expected callable type is present.

```aurora
shared: def(String) -> int64 = lambda text: text.len()
owned: def(own String) -> String = lambda own text: text
push_one: def(mut Vec[int32]) -> None = lambda mut values: values.push(1)
```

A bare lambda parameter matches a bare shared parameter, `own name` matches an
owned parameter, and `mut name` matches a mutable parameter. Modes cannot be
silently changed. The body must have exactly the expected result type.
Parameters are in scope only in the body and follow the ordinary no-shadowing
rules.

The compiler does not guess parameter types from body operations. Generic
lambdas and lambda parameter type annotations are unavailable. A capture-free
lambda uses the ordinary function-value representation and is Copy and
Transfer. It may appear anywhere an ordinary function value can appear,
including arguments, fields, collections, and returns.

A capturing closure retains semantic environment and call-kind metadata that
an arbitrary written `def(...) -> R` storage type does not describe. It may be
held in an immutable inferred or contextually typed local, called directly,
passed directly to compiler-known repeatable callback sites such as the Vec
algorithms and `control.retry`, or moved into a qualifying task start. It
cannot be coerced through an arbitrary written `def` parameter, stored in a
`def` field or collection element, or returned through an annotated `def`
result. Those metadata-erasing boundaries report `AU2002`.

A conditional or `match` expression also cannot merge capturing closure
values from different branches. The branches may have different capture
sets, ownership states, and call kinds, and Phase 6.3 has no closure-union
type that preserves those differences. Call the closure inside each branch,
or return capture-free lambdas or named functions with one structural
`def(...) -> R` type. Creating and calling a closure wholly inside a branch
remains supported.

A resolved name in the body is a capture only when it denotes an outer owned
local or an `own` parameter. Lambda parameters, module functions, types,
builtins, and imported items are resolved normally and are not stored in the
environment.

## Runtime Semantics

Evaluating a lambda constructs its callable value immediately. Each captured
Copy value is snapshotted into the environment; each captured non-Copy owned
value moves into it. Later changes to an outer mutable Copy binding do not
retarget the snapshot.

Calling the closure evaluates arguments under its contextual structural
function signature and then evaluates the body. A closure whose body only
reads captures borrows its environment for the call and can be invoked
repeatedly. A body that consumes a non-Copy capture consumes the closure on
its first call. The existing move checker rejects another call or use.

Capture-free lambdas dispatch as ordinary function values. Capturing closures
carry an owned environment and are non-Copy, including when their captures
are individually Copy.

## Ownership And Evaluation Order

Capture is by value and happens at closure creation, not on the first call.
Copy captures leave their sources usable. Non-Copy captures move, so using the
outer source afterward reports `AU3001`. Clone before creation when both
owners are required:

```aurora
name = "Aurora"
kept = name.clone()
length: def() -> int64 = lambda: kept.len()
print(name)
print(length())
```

A bare parameter of an enclosing function is shared capability, not owned
data, and cannot be captured. Take it as `own`, or clone the data into an
owned local before building the closure. A `mut` enclosing parameter is also
caller-owned capability and cannot be captured.

Phase 6.3 closure environments are read-only. A body cannot pass a capture to
a `mut` parameter, call a `mut self` method on a capture, or otherwise request
mutable access to it. This does not restrict the lambda's own `mut` parameter,
which writes through the mutable argument supplied for that call.

A closure is Transfer exactly when all of its captures are Transfer. Moving a
qualifying closure into `TaskGroup.start`, `start_soon`, or an explicit-stack
variant transfers the complete environment to child-owned storage. A
non-Transfer leaf retains the ordinary `AU3008` boundary explanation.

## Diagnostics

`AU1101` reports malformed lambda parameter or body syntax. `AU2002` reports a
missing or mismatched parameter context, parameter capability, result type,
metadata-erasing storage boundary, or a consuming closure supplied where a
repeatable callback is required. `AU3001` reports use after a non-Copy value
moved into a closure and use after a consuming closure call. `AU3002` rejects
capture of shared or mutable caller capability. `AU3003` rejects mutable
access through a captured environment. `AU3008` reports a closure whose
captured environment cannot cross a task boundary because some captured value
is not Transfer.

The shared-capability diagnostic recommends cloning to an owned local or
taking owned input. Move diagnostics identify closure creation or the
consuming call as the ownership origin.

## Backend Support

Contextual checking, capture analysis, move checking, MIR lowering, and direct
native lowering implement the same closure contract. Both maintained backends
copy or move captures at creation, preserve repeated read-only calls, enforce
single-use consumption statically, and clean up an owned environment exactly
once. Compiler analysis and the language server expose lambda parameter scope,
captured-name definitions, callable hover, completions, and the compiler-owned
diagnostics.

## Limits And Implementation-Defined Behavior

Closures are expression-only and contextually typed. They do not support
statement bodies, inline parameter types, defaults, generics, capture lists,
implicit reference capture, mutable captured state, method values, trait
objects, FFI callbacks, asynchronous syntax, or comprehensions. In-loan
capture waits for the separate loan/view design.

Arbitrary structural `def` parameters and stored `def` fields, collection
elements, and annotated returns currently carry only capture-free code
pointers. Compiler-known callback sites preserve repeatable closure metadata;
`control.retry` and the Vec callbacks reject consuming closures. Task start
accepts a qualifying closure by move for one invocation.

Conditional and `match` expressions cannot merge capturing closures from
multiple branches. This is an explicit closure-union boundary, not an
implementation-defined coercion.

The capture, callability, ownership, and Transfer rules are language-defined;
the implementation does not choose a reference-versus-value capture mode.

## Status

Expression closures and by-value capture are implemented under Provisional
ADR-0037
(`architecture_docs/decisions/0037-expression-closures-and-value-capture.md`)
through the Batch 5 checkpoint. Capture-free function values remain governed
by [Functions](/manual/functions), and task-boundary Transfer remains governed
by Accepted ADR-0033.
