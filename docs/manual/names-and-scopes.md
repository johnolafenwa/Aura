# Names And Scopes

This page covers how Aura resolves names. Resolution is static and never falls back to dynamic lookup. A name denotes one of these:

- a local binding, parameter, or pattern payload
- a type parameter
- a module
- a function, class, enum, or trait
- an enum variant, through a qualified path
- a maintained builtin

## Identifiers And Reserved Names

An identifier starts with an ASCII letter or `_`. The rest may be ASCII letters, digits, or `_`. The lexer reserves the words listed in [Lexical Structure](/manual/lexical-structure).

`copy`, `self`, `None`, and `_` are contextual identifiers. Their special meaning depends on where they appear in the grammar.

User items cannot redefine builtin type names or builtin top-level function names. `Self` is reserved in trait and implementation type contexts. It cannot be declared as a type parameter.

## Module Scope

One `.au` file defines one module scope. These kinds of name share its top-level namespace:

- classes
- enums
- functions
- extern functions and extern opaque handle types
- traits
- module constants
- imported names
- imported module aliases

Because they share one namespace, a local item cannot reuse a name that is already imported or declared as another kind of item. A trait implementation block introduces no top-level name. It attaches behavior to an existing trait and type combination.

Imports belong to the module wherever they appear in the file. The compiler resolves them before it checks function bodies and top-level statements.

## Module Constants

A bare binding at module level declares an immutable module constant. The initializer is required and the annotation is optional. `public` exposes the constant to qualified imports and from-imports.

    max_attempts: int64 = 5
    service_name = "planner"
    public default_region = "eu-west"

    def main():
        print(service_name)

Constants become available in declaration order. An initializer may read an earlier constant in the same module. Reading itself or a later constant is `AU2001`, use before initialization. Functions and types are available to every initializer, wherever they appear in the file.

**Initialization order.** Aura initializes every reachable module before the entry runs:

- dependencies initialize before the modules that import them
- sibling dependencies follow first-import order
- constants within one module follow declaration order
- a module reached through several imports initializes once
- an initializer failure prevents the entry from running

**Reads.** Reading a Copy-typed constant produces an ordinary copy. Reading a non-Copy constant gives shared access to the one value stored by its defining module. That read cannot move the value into owned storage, pass it to an `own` parameter, or request mutable access. When you need independent owned data, use an explicit supported `.clone()` or a constructor.

**Immutability.** Module storage is immutable. Module-level `mut`, reassignment, compound assignment, and mutable access are `AU3003` errors. Keep stateful application data in a local value owned by `main` or another explicit owner.

**Script locals.** An entry script's top-level locals live apart from module storage. [Top-Level Statement Scope](#top-level-statement-scope) covers them.

## Imports

An unaliased module import binds the first path component as a namespace:

```aura
import tools.text

value = tools.text.parse("input")
```

An aliased module import binds the complete module under the alias. It does not bind the path's first component:

```aura
import tools.text as text_tools

value = text_tools.parse("input")
```

A from-import binds the requested public items directly:

```aura
from tools.text import parse, ResultRow
```

Each from-import entry may take a local alias. Direct and aliased entries can appear together:

```aura
from tools.text import parse as parse_text, ResultRow
```

**Aliases.** An alias occupies the same module-level namespace as items, module constants, and other imports. The compiler rejects:

- duplicate aliases
- aliases that collide with another name
- reserved names and `_` as aliases
- duplicate imports of one target in a declaration

An alias changes only the local spelling. The target keeps its defining module, nominal identity, visibility, trait implementations, initialization storage, and documentation target.

**Paths.** An import path is a list of dot-separated identifiers. It maps to a module path inside the current package and dependency graph. Import syntax has no filesystem path traversal. [Packages](/manual/packages) describes package roots and dependency aliases.

**Visibility.** Another module can import only `public` top-level items of these kinds: classes, enums, Aura functions, extern functions, extern opaque handle types, traits, and module constants. Class fields and methods have their own visibility. A non-public member is accessible inside its defining module and rejected across a module boundary. An alias does not bypass that boundary.

**Identity.** An import is not a file include. An imported declaration keeps its defining module identity. Aura uses that identity for private access, qualified type names, diagnostics, trait implementations, and go-to-definition.

## Type Names

The compiler resolves a type name from these sources, in order:

1. type parameters in the innermost declaration
2. `Self` in a trait or trait-implementation method where it is permitted
3. local and directly imported class, enum, extern opaque handle, and trait names
4. module-qualified public types
5. builtin and builtin-module type names

A type's arguments must match the arity it declares.

A generic type parameter is in scope throughout the class, enum, function, trait, implementation, or method that owns it. A method may add type parameters to the ones it inherits from its enclosing declaration. A parameter name cannot duplicate another parameter in the same declaration, and a method cannot reuse `Self`.

The implementation rejects duplicate type parameters. Do not rely on an inner method's type parameter shadowing one from its enclosing generic declaration. That is not a portable technique, so give each parameter a distinct name.

## Function And Method Scope

A function body starts with bindings for its ordinary parameters. A method body also binds the contextual receiver `self` when the method declares a receiver.

Parameter names, `self`, local bindings, loop bindings, `with` bindings, and pattern bindings all live in the function's value namespace. A use is valid only after the binding is introduced on the current control-flow path.

Aura 0.3 does not support local function, class, enum, or trait declarations. An item lives at module level or as a member of an enclosing declaration that permits it.

## Local Bindings

Assigning to a simple name that has not been seen yet introduces a binding:

```aura
def main():
    name = "Aura"
    mut count: int32 = 0
```

The compiler checks the initializer before the new name becomes available. A binding without `mut` is immutable.

A later assignment to an existing name is a reassignment, not a new shadowing declaration. It is valid only for a mutable place of the same type.

`mut` is allowed only when introducing a simple local binding. It cannot redeclare an existing name, and it cannot prefix a field or index assignment.

A binding introduced inside an `if` branch, loop body, match arm, or `with` body does not escape that body. At control-flow joins, the checker merges ownership effects on outer bindings conservatively.

## Lambda Scope

Each lambda creates one parameter scope for its expression body. Its parameters:

- become visible only after the colon
- cannot duplicate one another
- follow the ordinary no-shadowing rules

The body may resolve outer locals and owned parameters. Those resolved owned values become by-value captures. Module items, builtins, imports, and the lambda's own parameters are not captured.

A bare or `mut` parameter of an enclosing function cannot be captured. It is a capability into the caller's storage, not an owned value.

Compiler analysis keeps hover and definition identity for lambda parameters and captured outer names. See [Closures](/manual/closures).

## Comprehension Scope

A comprehension creates one nested expression scope. Clause targets enter that scope one at a time, in runtime order, even though the output expression is written first:

    pairs = [
        (left, right)
        for left in values
        for right in values if right > left
    ]

A clause's iterable expression cannot see that clause's own target. Once bound, the target is visible in:

- the clause's filters
- every later iterable and filter
- the output key/value or element expression

So an earlier target is visible while an inner iterable is selected.

Targets use the ordinary `loop-target` grammar and the no-shadowing rule. A target cannot reuse a local that is visible outside the comprehension, or an earlier clause target. The leaves of a tuple target enter together after the source item is selected. No comprehension target is visible after the closing `]` or `}`.

**Lambdas and comprehensions.** A lambda that encloses a comprehension captures the names used by source, filter, and output expressions, under the capture rules in [Closures](/manual/closures). It does not capture comprehension targets, because those are local to the lambda body. A lambda created inside a comprehension may capture a currently bound target only when those capture rules permit that by-value capture.

## No-Shadowing Rules

Aura rejects these forms of shadowing because they are ambiguous:

- a `for` binding cannot reuse a visible name
- a comprehension target cannot reuse a visible name or an earlier clause target
- a `with` binding cannot reuse a visible name
- a match payload binding cannot reuse a visible name
- a second `mut name = ...` cannot redeclare `name`
- assignment to an existing immutable name is not interpreted as a new inner binding

As a result, a reader can normally tie one local spelling to one logical binding for the whole function. Use a distinct name when you transform a value.

## Block Scope And Control Flow

The checker checks each branch, loop body, match arm, and `with` body against a child view of the current local environment. Reads and writes must be valid on every reachable path.

At a control-flow join, a move or partial move that may have happened on any reachable path makes the affected outer place unavailable. The exception is a place that is definitely reinitialized on all relevant paths. A binding created only inside a child block never enters the parent scope.

For limited reachability and loop-flow reasoning, the compiler recognizes constant `true` and constant `false`, including grouped and `not` forms. Write clear control flow instead of depending on aggressive compile-time evaluation.

## Pattern Scope

Each match arm has its own scope for payload bindings. A binding is available only in that arm's body or value expression.

```aura
match result:
    case Result.Ok(value):
        print(value)
    case Result.Err(message):
        print(message)
```

- `_` binds nothing.
- A lowercase unqualified pattern name is a binding pattern.
- A variant pattern may be unqualified when the scrutinee type supplies the enum identity. Otherwise it uses `Enum.Variant` or a module-qualified path.

Borrowed and mutably borrowed matches attach borrow provenance to their payload bindings. Those bindings cannot be used after a mutation invalidates the matched place.

## Class And Trait Member Lookup

For a class value, member lookup considers the fields and methods the class declares. Public access rules apply across modules. A method call also considers trait methods from implementations visible in the current module and package context.

Trait method selection uses:

- the receiver type
- explicit or inferred trait arguments
- type-parameter bounds
- implementation specificity

When several implementations are equally applicable, the call is ambiguous. The checker rejects it and never picks one by source order.

An associated method has no receiver. You reference it through the type, for example `Worker.create(...)`. An instance method needs a receiver compatible with its declared contract: shared `self`, consuming `own self`, or mutable `mut self`.

## Builtin Names And Modules

Top-level builtin functions such as `print`, `range`, `sleep`, and `select` are available without an import. Builtin enum names such as `SelectOutcome` are also reserved and available without an import.

Builtin modules must be imported before you use their module-qualified members. They include `fs`, `io`, `net`, `process`, `random`, `sys`, `path`, `bytes`, `json`, `toml`, `log`, `trace`, and `metrics`.

`random.Rng` is the builtin type and constructor spelling for a deterministic generator. Its methods stay module-qualified through the receiver type. There is no implicit global random-stream name. The secure operations are `random.secure_int` and `random.secure_bytes`.

Builtin behavior follows where a declaration comes from, not a matching module or type spelling. A user source file whose logical module name is `random` may declare its own `Rng` class. That class stays an ordinary user class in checking, analysis, MIR lowering, clone-safety classification, and both backends. MIR is the compiler's mid-level intermediate representation.

Builtin enum types such as `Result`, `Lookup`, `Poll`, `QueueReceive`, and `process.Error` use the same qualified-member model as user enums. Short-form variant patterns and constructors work only where the checker can determine a unique expected enum type.

## Top-Level Statement Scope

An entry module may contain executable top-level statements instead of a local `main`. Those statements share one top-level local environment, separate from module storage. They run in source order after the reachable module constants finish initializing.

A `mut` simple-name assignment declares a mutable top-level local. For example, `mut count = 0` creates one. Later plain and compound assignments to that name, such as `count = count + 1` and `count += 1`, stay in the statement stream and update the same local.

A bare assignment to a new name depends on where it appears:

- Above the first top-level statement, it declares a module constant. Other modules can import it, and it initializes before any statement runs.
- From the first top-level statement on, it declares an immutable top-level local. It reads earlier locals, and it cannot be reassigned.

Imports and `def`, `class`, `enum`, `trait`, `impl`, and `type` declarations are not statements. So a file whose first executable line comes after its constants keeps all of them as constants.

```aura
limit = 100                       # module constant

mut scores = [30, 60, 90]         # first statement
scores.append(120)

total = scores.len()              # immutable top-level local
print(f"{total} of {limit}")
```

A `public` binding must be a module constant, so `public name = value` after the first statement is a syntax error (`AU1101`). Functions see module constants but not top-level locals.

An imported module contributes items and eagerly initialized constants. Its top-level executable statements are checked as source, but they do not run as import side effects. Put reusable executable work inside public functions.

## Grammar

[Lexical Structure](/manual/lexical-structure) defines identifier spelling. The [Grammar](/manual/grammar) defines these binding positions:

- module declarations and imports
- function and lambda parameters
- receivers
- simple-name assignments
- statement-`for`, comprehension-clause, and `with` targets
- match payloads
- generic parameter lists

Member access uses a dot-separated syntactic path. It adds no dynamic lookup syntax.

## Typing Rules

- Every value and type name resolves statically, by the priority and namespace rules above.
- A resolved value binding has one fixed type.
- Reassignment requires an existing mutable binding of the same type. It never creates a shadow.
- Generic and `Self` resolution happens before substitution and bound checking.
- The checker rejects ambiguous trait implementations and unavailable or private names. It never selects one by source order.

## Runtime Semantics

A local or parameter reference reads its statically selected storage place. Module, type, function, and associated-member names select compiler metadata. None of them performs a runtime dictionary lookup.

Module constants initialize once, in the dependency and source order defined above. The entry module's executable statements then run in source order. Imports never run imported top-level statements as initialization side effects.

## Ownership And Evaluation Order

Resolving a name has no side effect. Evaluating the resolved place may copy, borrow, mutate, or move it, depending on its type and the surrounding expression.

An initializer is evaluated before its new local enters scope. Block and pattern scopes are entered only for the runtime path that is selected. The checker merges ownership state from continuing paths conservatively.

Comprehension clause scopes enter one at a time. A target is established only after its iterable value is selected, and only for the current item. Filters and inner clauses that are not reached establish no bindings. The whole scope is discarded with the expression.

## Diagnostics

| Code | Cause |
| --- | --- |
| `AU2001` | An unknown, unavailable, or unresolved name. This includes a module constant read before initialization and a comprehension target used outside its expression. |
| `AU2002` | Type-name arity and related expected-type failures. |
| `AU2999` | A duplicate, reserved, private, ambiguous, or otherwise invalid name or scope declaration that has no narrower code. |
| `AU3001` | A read of a moved place, or an attempted move out of non-Copy module storage. |
| `AU3002` | A borrow conflict. |
| `AU3003` | An immutable place, or a request for mutable module storage. |
| `AU3004` | An invalid ownership mode. |
| `AU4001` | Runtime re-entry into module initialization. |

`AU3001` through `AU3004` report places that became invalid after resolution. They include related source spans and repair guidance where applicable.

## Backend Support

Name, visibility, trait, module, and scope resolution happen in the compiler front end. MIR execution and direct native builds share that result, so both backends receive the same resolved targets and substituted types. The compiler-backed language server uses the same resolution for hover, definitions, and diagnostics.

## Limits And Implementation-Defined Behavior

- Local declarations and comprehension targets cannot shadow visible locals in the positions listed above.
- Items cannot be nested in function suites.
- Wildcard imports, relative-dot imports, and imported top-level execution are unavailable.
- Import aliases follow the ordinary no-collision and visibility rules.

[Packages](/manual/packages) specifies how packages map to the filesystem. That mapping is not left to implementation-defined name lookup.

## Status

Implemented:

- static lexical scope
- module imports and aliases
- module constants
- visibility
- generic and type namespaces
- member lookup
- comprehension scopes
- the entry-module top-level scope described above

Unavailable: dynamic names, reflection-based lookup, nested items, import side effects, wildcard imports, and user-selectable shadowing. An identifier that lexes today does not imply any future name-resolution form.

Design record: `architecture_docs/decisions/0037-expression-closures-and-value-capture.md`.
