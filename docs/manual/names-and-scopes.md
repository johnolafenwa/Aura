# Names And Scopes

Aurora resolves names statically. A name denotes a local binding, parameter, pattern payload, type parameter, module, function, class, enum, trait, enum variant through a qualified path, or maintained builtin. Name resolution never falls back to dynamic lookup.

## Identifiers And Reserved Names

Identifiers are ASCII letters or `_`, followed by ASCII letters, digits, or `_`. The lexer reserves the words listed by [Lexical Structure](/manual/lexical-structure). `copy`, `self`, `None`, and `_` are contextual identifiers whose special meaning depends on their grammatical position.

Builtin type names and builtin top-level function names cannot be redefined by user items. `Self` is reserved within trait and implementation type contexts and cannot be declared as a type parameter.

## Module Scope

One `.au` file defines one module scope. Its top-level item namespace contains:

- classes
- enums
- functions
- traits
- imported names
- imported module aliases

These categories share the same top-level item name space. A local item cannot reuse a name already imported or declared as another item kind. Trait implementation blocks do not introduce a top-level name; they attach behavior to an existing trait/type combination.

Imports are module-level regardless of their textual position in the file. They are resolved before static checking of function bodies and top-level statements.

## Imports

Module imports bind the first path component as a namespace:

```python
import tools.text

value = tools.text.parse("input")
```

From-imports bind the requested public items directly:

```python
from tools.text import parse, ResultRow
```

An import path consists of dot-separated identifiers and maps to a module path inside the current package/dependency graph. Filesystem path traversal is not part of import syntax. Package roots and dependency aliases are described by [Packages](/manual/packages).

Only `public` top-level classes, enums, functions, and traits may be imported from another module. Class fields and methods also have individual visibility. A non-public member remains accessible inside its defining module but is rejected across a module boundary.

Imports do not mean "include this file". Imported declarations retain their defining module identity, which is used for private access, qualified type names, diagnostics, trait implementations, and go-to-definition.

## Type Names

Types are resolved from:

1. type parameters in the innermost declaration
2. `Self` in a trait or trait-implementation method where it is permitted
3. local and directly imported class, enum, and trait names
4. module-qualified public types
5. builtin and builtin-module type names

Type arguments must have exactly the arity declared by the target type. Generic type parameters are in scope throughout their owning class, enum, function, trait, implementation, or method as appropriate. A method may add type parameters to those inherited from its enclosing declaration, but a parameter name cannot duplicate another parameter in the same declaration and `Self` cannot be reused.

The implementation rejects duplicate type parameters. Type parameter shadowing between an enclosing generic declaration and an inner method is not a portable language technique; declarations should use distinct names.

## Function And Method Scope

A function body begins with bindings for its ordinary parameters. A method body additionally binds the contextual receiver `self` when a receiver was declared.

Parameter names, `self`, local bindings, loop bindings, `with` bindings, and pattern bindings occupy the function's value namespace. A use is valid only after the binding has been introduced on the current control-flow path.

Aurora 0.1 does not support local function, class, enum, or trait declarations. Items are module-level or members of their permitted enclosing declaration.

## Local Bindings

An assignment to a previously unseen simple name introduces a binding:

```python
name = "Aurora"
mut count: int32 = 0
```

The initializer is checked before the new name becomes available. A binding without `mut` is immutable. A later assignment to an existing name is reassignment, not a new shadowing declaration, and is valid only for a mutable place with the same type.

`mut` is permitted only when introducing a simple local binding. It cannot redeclare an existing name and cannot prefix a field or index assignment.

Bindings introduced inside an `if` branch, loop body, match arm, or `with` body do not escape that body. Effects on ownership state of an outer binding are merged conservatively at control-flow joins.

## Lambda Scope

Each lambda creates one parameter scope for its expression body. Parameters
become visible only after the colon, cannot duplicate one another, and follow
the ordinary no-shadowing rules. The body may resolve outer locals and owned
parameters. Those resolved owned values become by-value captures; module
items, builtins, imports, and the lambda's own parameters do not.

A bare or `mut` parameter of an enclosing function is a capability into the
caller's storage rather than an owned value and cannot be captured. Lambda
parameters and captured outer names retain hover and definition identity in
compiler analysis. See [Closures](/manual/closures).

## No-Shadowing Rules

Aurora deliberately rejects several ambiguous forms of shadowing:

- a `for` binding cannot reuse a visible name
- a `with` binding cannot reuse a visible name
- a match payload binding cannot reuse a visible name
- a second `mut name = ...` cannot redeclare `name`
- assignment to an existing immutable name is not interpreted as a new inner binding

This means a reader can normally associate one local spelling with one logical binding for the duration of a function. Use a distinct name when transforming a value.

## Block Scope And Control Flow

Each branch, loop body, match arm, and `with` body is checked with a child view of the current local environment. Reads and writes must be valid on every reachable path.

When control flow joins, a move or partial move that may have happened on any reachable path makes the affected outer place unavailable unless it was definitely reinitialized on all relevant paths. A binding created only inside a child block is never introduced into the parent scope.

The compiler recognizes constant `true`, constant `false`, and their grouped/`not` forms for limited reachability and loop-flow reasoning. Programs should still express clear control flow rather than depend on aggressive compile-time evaluation.

## Pattern Scope

Each match arm has its own payload-binding scope. Bindings become available only in that arm's body or value expression.

```python
match result:
    case Result.Ok(value):
        print(value)
    case Result.Err(message):
        print(message)
```

`_` binds nothing. A lowercase unqualified pattern name is a binding pattern. Variant patterns may be unqualified when the scrutinee type supplies the enum identity; otherwise they use `Enum.Variant` or a module-qualified path.

Borrowed and mutable-borrowed matches attach borrow provenance to payload bindings. Those bindings cannot be used after a mutation invalidates the matched place.

## Class And Trait Member Lookup

For a class value, member lookup considers fields and methods declared by the class. Public access rules apply across modules. A method call also considers trait methods from implementations visible through the current module/package context.

Trait method selection uses the receiver type, explicit or inferred trait arguments, type-parameter bounds, and implementation specificity. Multiple equally applicable implementations are ambiguous and MUST be rejected instead of selected by source order.

Associated methods are methods without a receiver and are referenced through
the type, for example `Worker.create(...)`. Instance methods require a receiver
compatible with their declared shared (`self`), consuming (`own self`), or
mutable (`mut self`) contract.

## Builtin Names And Modules

Top-level builtin functions such as `print`, `range`, `sleep`, and `select`
are available without import. Builtin enum names such as `SelectOutcome` are
also reserved and available without import. Builtin modules such as `fs`,
`io`, `net`, `process`, `random`, `sys`, `path`, `bytes`, `json`, `toml`,
`log`, `trace`, and `metrics` must be imported before their module-qualified
members are used.

`random.Rng` is the builtin type and constructor spelling for a deterministic
generator. Its methods remain module-qualified through the receiver type;
there is no implicit global random-stream name. The secure operations are
`random.secure_int` and `random.secure_bytes`.

Builtin behavior follows declaration origin, not a coincidental module/type
spelling. A user source file whose logical module name is `random` may declare
its own `Rng` class; that class remains an ordinary user class in checking,
analysis, MIR lowering, clone-safety classification, and both backends.

Builtin enum types such as `Option`, `Result`, `QueueReceive`, and `process.Error` use the same qualified-member model as user enums. Short-form variant patterns and constructors are available only where the checker can determine a unique expected enum type.

## Top-Level Statement Scope

An entry module may contain executable top-level statements instead of a local `main`. Those statements share one top-level local environment and execute in source order after checking.

Imported modules contribute declarations, not executable initialization: their top-level statements are checked as source but are not run as import side effects in Aurora 0.1. Reusable modules should therefore keep executable work inside public functions. This boundary may be tightened in a later release, but programs MUST NOT depend on imported top-level side effects today.

## Grammar

Identifier spelling is defined by [Lexical Structure](/manual/lexical-structure).
The binding positions are module declarations and imports, function and lambda
parameters, receivers, simple-name assignments, `for` and `with` targets,
match payloads, and generic parameter lists in the
[Grammar](/manual/grammar). Member access uses a dot-separated syntactic path;
it does not add dynamic lookup syntax.

## Typing Rules

Every value and type name is resolved statically in the priority and namespace
rules above. A resolved value binding carries one fixed type. Reassignment
requires the existing mutable binding and the same type; it never creates a
shadow. Generic and `Self` resolution occurs before substitution and bound
checking. Ambiguous trait implementations and unavailable or private names are
rejected rather than selected by source order.

## Runtime Semantics

Local and parameter references read their statically selected storage place;
module, type, function, and associated-member names select compiler metadata
and do not perform a runtime dictionary lookup. Entry-module top-level bindings
are created in source order. Imports load declarations during compilation and
do not execute imported top-level statements as initialization side effects.

## Ownership And Evaluation Order

Resolving a name has no side effect, but evaluating the resolved place may copy,
borrow, mutate, or move it according to its type and the surrounding
expression. Initializers are evaluated before a new local enters scope. Block
and pattern scopes are entered only for the selected runtime path; ownership
state from continuing paths is merged conservatively by the checker.

## Diagnostics

`AU2001` reports unknown, unavailable, or unresolved names. `AU2002` covers
type-name arity and related expected-type failures. `AU2999` covers duplicate,
reserved, private, ambiguous, or otherwise invalid name/scope declarations not
assigned a narrower code. Reads of places invalidated after resolution use
`AU3001` for a moved place, `AU3002` for a borrow conflict, `AU3003` for an
immutable place, and `AU3004` for an invalid ownership mode, with related
source spans and repair guidance where applicable.

## Backend Support

Name, visibility, trait, module, and scope resolution are compiler-front-end
operations shared by MIR execution and direct native builds. Both backends
receive the same resolved targets and substituted types. Compiler-backed LSP
hover, definitions, and diagnostics use that same resolution result.

## Limits And Implementation-Defined Behavior

Local declarations cannot shadow visible locals in the positions listed above;
items cannot be nested in function suites; wildcard or relative-dot imports and
import aliases are unavailable; and imported top-level execution is absent.
Package filesystem mapping is specified by [Packages](/manual/packages), not
left to implementation-defined name lookup.

## Status

Static lexical scope, module imports, visibility, generic/type namespaces,
member lookup, and the documented entry-module top-level scope are implemented.
Dynamic names, reflection-based lookup, nested items, import side effects,
wildcard imports, and user-selectable shadowing are unavailable. No future
name-resolution form is implied by an identifier that happens to lex today.
