# ADR-0050: Module-level constants and deterministic initialization

- Status: Accepted
- Date: 2026-08-02
- Roadmap decision: Batch S1, S4.5
- Builds on: ADR-0016, ADR-0022, ADR-0033, and ADR-0042

## Context

Reusable modules need named configuration values, protocol identifiers,
lookup data, and constructed immutable values alongside functions. Requiring a
function merely to expose a stable value obscures APIs and prevents a natural
module shape containing constants and `def main()`.

Module storage also creates initialization-order and ownership questions.
Aura therefore needs one eager, deterministic, once-only rule and must not
turn module bindings into shared mutable globals.

## Decision

### Declaration syntax

A simple bare binding at module level declares a module constant:

```aura
max_attempts: int64 = 5
service_name = "planner"
public default_timeout = Duration.seconds(30)

def main():
    print(service_name)
```

The accepted forms are:

```text
["public"] identifier [":" type] "=" expression NEWLINE
```

The initializer is required. Its type is the annotation when present and is
otherwise inferred from the initializer under ordinary contextual literal
rules. `public` exposes the constant to qualified and from-import lookup.
Without `public`, access is limited to the defining module.

A module constant may coexist with functions, classes, enums, traits,
implementations, imports, and a local `main`. It occupies the ordinary
module-level name space and cannot collide with another item or import
binding. Uppercase spelling is a style convention only.

`mut` is not permitted on a module-level binding. Assignment, compound
assignment, mutable argument passing, mutable matching, and any operation that
requires the constant's storage to be a mutable place are rejected. Other
top-level executable statements remain script-entry behavior and cannot be
mixed with a local `main`.

### Static scope and use-before-initialization

Imports and item declarations are resolved before initializer checking.
Functions and types may therefore be named regardless of their textual item
position. Module constants enter value scope in declaration order.

An initializer may read an earlier constant in the same module and any public
constant in an imported dependency module. A direct reference to the constant
being declared or to a later constant in the same module is rejected with
`AU2001` as use before initialization. The checker constructs the direct
constant-initializer dependency graph and rejects every cycle with `AU2999`,
reporting the complete cycle as related locations.

A function body may reference any constant in its module because the body can
run after module initialization. If a constant initializer calls user code
that reaches a constant whose initialization has not completed, the runtime
initialization guard rejects that access with `AU4001`. This guard covers
indirect calls, function values, recursion, and cross-function paths that are
not direct initializer references. No uninitialized or partially initialized
value is observable.

### Dependency and source order

Before entry-module top-level execution or `main`, Aura eagerly initializes
every module reachable from the entry module:

1. initialize imported dependency modules before the importing module
2. visit sibling dependencies in the order their import declarations first
   appear in the importing source
3. initialize each module's constants in declaration source order
4. initialize a module identity at most once, even when several modules reach
   it through a diamond
5. begin entry execution only after all reachable module constants are ready

The import graph must be acyclic and is rejected statically when it is not.
The once-only state machine is `uninitialized`, `initializing`, `ready`, or
`failed`. Re-entry into an `initializing` module is an initialization-cycle
error. A failed module retains its failure and cannot be retried during the
same program run.

Only module constants participate in imported-module initialization.
Script-entry statements in another module are not import side effects.

Initializers run on the entry thread before Aura starts the structured runtime
worker pool. They cannot race with tasks. The order above is a language rule,
not filesystem traversal order or linker order.

### Evaluation, failure, and cleanup

Each initializer evaluates exactly once using ADR-0016 sequencing. Its value
is fully constructed before the constant becomes readable. A trap or
propagated failure aborts module initialization, prevents entry execution, and
retains the initializer's diagnostic as primary. Fully initialized constants
and partial values are cleaned up in reverse construction order where their
types require cleanup.

After entry execution and its structured task scope finish, module constants
are cleaned up once in reverse global initialization order. An uncaught entry
failure remains primary if shutdown cleanup also fails.

Constant initialization is runtime evaluation, not macro expansion. An
initializer may call functions and allocate values. It remains subject to the
ordinary operation, resource, recursion, and allocation limits. Compile-time
folding is allowed only when it preserves evaluation, failure, and diagnostic
semantics.

### Ownership of constant reads

Reading a Copy-typed module constant produces the ordinary Copy value. Reading
a non-Copy module constant provides shared access to its stored value. Such a
read cannot move the value, pass it to an `own` parameter, return its storage
as an owned value, capture it by ownership, or place it into owned storage.
Code that needs an independent owned value calls an available explicit
`clone()` or constructor.

Shared method calls and externally observable operations retain their normal
type contracts. Binding immutability does not claim that every reachable host
resource is pure; it guarantees that Aura code cannot replace, consume, or
obtain mutable access to the module storage through the constant name.

Public imported constants retain the defining module as their storage owner.
An import does not copy initialization or create a second stored value.

## Diagnostics

- `AU1101` reports malformed module-constant declarations.
- `AU2001` reports an unknown constant or direct use before its declaration is
  initialized, with the declaration as a related location.
- `AU2002` reports annotation/initializer type mismatch.
- `AU2999` reports a direct constant dependency cycle or cyclic module import,
  listing the cycle path.
- `AU3001` reports an attempted move from non-Copy module storage.
- `AU3003` reports `mut`, reassignment, compound assignment, mutable access,
  or another request for a mutable module place. Its help explains that module
  bindings are immutable and directs stateful data into a local value owned by
  `main` or another explicit owner.
- `AU4001` reports indirect use before initialization or runtime
  initialization re-entry. Initializer operation failures retain their own
  runtime codes.

## Backend requirements

The frontend emits one canonical module-initialization plan shared by MIR and
direct execution. Both backends use the same module identity, once-state,
dependency order, source order, access guard, failure retention, and cleanup
order. Native link order cannot affect initialization.

Both backends must expose a constant to compiler analysis and the language
server with its declared/inferred type, visibility, defining module, and
shared-read capability.

## Limits

Module constants are eagerly initialized; there is no lazy constant, mutable
global binding, thread-local module binding, reinitialization API, explicit
initialization block, compile-time-function evaluator, or configurable
initialization priority. This decision does not make arbitrary initializer
expressions usable in type positions, pattern literals, or array extents.

## Consequences

Aura modules can expose stable values naturally and can define `main`
alongside them. Dependency-first, source-ordered, guarded initialization makes
side effects and failures reproducible. Shared reads preserve one owner for
non-Copy data, and the absence of mutable module places keeps application
state under explicit local or structured owners.

## Completion test matrix

- parser tests for inferred and annotated constants, `public`, interleaving
  with every item kind and imports, coexistence with `main`, malformed forms,
  and module-level `mut`
- static tests for type inference/context, visibility, item/import collisions,
  same-module earlier reads, direct later/self reads, direct dependency cycles,
  module import cycles, and functions that name constants
- ownership tests for all Copy reads, shared non-Copy calls, moves, `own`
  arguments, owned returns/storage/captures, mutable calls/matches, assignment,
  compound assignment, and explicit cloning
- runtime order tests for dependency-before-importer, sibling first-import
  order, source order, diamond initialization exactly once, entry after ready,
  allocation and user-function initializers, and no worker-pool race
- failure tests for direct and indirect use before initialization, recursive
  initializer calls, retained failed state, operation diagnostics, reverse
  cleanup, and no entry execution after failure
- optimization and native-link-order tests proving the canonical plan is not
  reordered
- byte-identical MIR/direct output and diagnostics, package/multi-module
  fixtures, compiler analysis, completion, hover, go-to-definition,
  language-server, bundled-editor, maintained example, and executable Manual
  coverage

## Ratification

Batch S1 accepts this as Aura 0.3's module-level constant and initialization
contract. Parser, resolver, dependency planner, runtime guards, both backends,
diagnostics, package behavior, reference, examples, and tooling land together.
