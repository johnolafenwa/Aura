# Language Specification

This Manual is the normative specification of the implemented Aura 0.3
development language. It defines what a conforming implementation must
provide:

- the source language and its static rules
- the ownership model and execution behavior
- the maintained runtime APIs
- the package model and tool contracts

The specification describes exactly the language implemented in this
repository.

## Scope

The specification covers:

- UTF-8 `.au` source text, tokens, indentation, and the complete accepted
  grammar
- declarations, statements, expressions, patterns, names, scopes, and
  visibility
- types, inference, generics, traits, calls, and operator resolution
- moves, copies, borrows, mutable places, resources, and owned returns
- module loading, packages, entry modules, top-level execution, and `main`
- evaluation order, control flow, runtime failures, cleanup, tasks,
  cancellation, and backend equivalence
- maintained builtin functions, enums, modules, resources, and CLI and editor
  contracts
- implementation limits that valid or invalid Aura programs can observe

The specification does not define the compiler's private Rust data
structures, its mid-level intermediate representation (MIR) encoding, the
native ABI, object-file layout, or internal optimization choices. The
exception is where one of these affects an observable language or tool
contract.

## Normative Language

These key words are normative: **MUST**, **MUST NOT**, **REQUIRED**,
**SHOULD**, **SHOULD NOT**, and **MAY**.

| Key word | Meaning |
| --- | --- |
| **MUST**, **MUST NOT** | A requirement for conforming implementations or programs. |
| **SHOULD**, **SHOULD NOT** | A strong recommendation. A deviation needs a documented reason and must not contradict a MUST-level rule. |
| **MAY** | Permitted behavior or an optional implementation technique. |

A plain present-tense statement is also normative when it describes accepted
syntax, static behavior, evaluation, runtime results, or public APIs.

Examples are illustrative. An example's exact output or diagnostic is part of
the contract only when the surrounding paragraph says so.

## Specification Version

This reference describes Aura 0.3 as implemented by the repository that
contains it. Aura is an advanced technical preview, so source and API
contracts may change before a tagged stable release.

Any behavior change MUST update these in the same pass:

- the relevant reference page
- conformance tests
- examples
- tutorials
- the work record

The repository commit identifies the precise revision of the specification.
The rendered Manual is stamped with source version 0.3.4 (technical preview)
and its implementation baseline commit. Release builds supply that commit
without writing a self-referential hash into this source page. The
[Manual overview](/manual/) gives the exact precedence and the local fallback.

## Authority And Conformance

The normative Manual and its executable conformance suite together define the
maintained language:

1. This specification states the intended rule.
2. Compiler fixtures and regression tests make the rule executable.
3. The compiler, runtime, CLI, and language server implement the rule.
4. Categorized examples and Learn chapters teach the rule without extending
   it.

A disagreement between the Manual, the tests, and the implementation is a
project defect. It must be resolved deliberately. Undocumented behavior does
not silently become a language feature. Proposal-only behavior does not
override the maintained reference.

[Conformance](/manual/conformance) maps rules to tests.
[Status And Compatibility](/manual/status-and-compatibility) gives the
preview stability policy.

## Processing Model

A source file passes through these observable phases:

1. **Decoding and lexing.** The implementation accepts UTF-8, forms tokens,
   and emits indentation tokens.
2. **Parsing.** Tokens form a module abstract syntax tree (AST) according to
   the [complete grammar](/manual/grammar).
3. **Module and package loading.** Imports resolve relative to the package
   source root and the dependency graph.
4. **Static checking.** The checker validates names, types, trait
   implementations, calls, ownership, borrows, patterns, control flow, and
   entrypoint rules.
5. **Lowering and execution.** `aura run` executes checked MIR. A direct build
   emits native code. When direct emission is unavailable, the default auto
   build may package checked MIR in a native launcher. All maintained
   representations MUST agree on program behavior.

A failure in phases 1 to 4 is a compile-time diagnostic.

A checked program may still produce an explicit runtime error. Examples are
checked integer overflow, division by zero, out-of-bounds mutation, I/O
failure, recursion-depth exhaustion, and invalid resource state. Where an API
specifies it, a recoverable library failure uses a typed `Result` or outcome
enum instead.

## Terms

| Term | Definition |
| --- | --- |
| **Module** | The declarations, imports, and optional top-level statements in one `.au` source file, together with its logical package-qualified name. |
| **Entry module** | The source file selected by a run, build, check, test, analysis, or completion command. Entrypoint-only rules, such as the `main` signature, apply to this module. |
| **Item** | A top-level declaration of a class, enum, Aura function, extern function, extern opaque handle, trait, or trait implementation. |
| **Binding** | A name associated with a value, parameter, pattern payload, module, type parameter, or declaration. |
| **Place** | A storage location that may be read, moved, assigned, or borrowed: a local binding, a field path, or a supported indexed location. |
| **Copy type** | A type whose values are duplicated, not consumed, by assignment and by-value use. |
| **Move type** | A type whose by-value use transfers ownership. The source place is then unavailable until it is reinitialized. |
| **Clone-producing operation** | An operation that creates a second owned structural value and keeps the original. Examples are explicit collection copies, cloned collection reads, and task-result observations. |
| **Clone-safety obligation** | An inferred callable requirement: a substituted type must not duplicate non-cloneable state through a clone-producing operation. Aura 0.3 protects `random.Rng` state under this contract. |
| **Borrow** | Temporary access to an existing place without transferring ownership. A shared borrow permits reading. A mutable borrow permits exclusive mutation. |
| **Owned position** | A source position that consumes a non-copy value. These include an explicit `own` parameter or collection loop, a class field or enum payload constructor, assignment, return, and maintained storing APIs. |
| **Default parameter mode** | The unmodified `value: T` spelling. Its source contract is shared access for every type. An implementation may pass copy bits directly without changing that contract. The shared mode stays the same after generic specialization. |
| **Resource** | A runtime-backed value with an explicit `close()` contract. Where documented, it also has lexical cleanup through `with`. |
| **Diverging path** | A control-flow path that does not reach the next statement normally. It returns, breaks, continues, propagates an error, or terminates through a runtime failure. |

## Defined, Implementation-Defined, And Unspecified Behavior

Aura aims to have no undefined behavior at the language level:

- Programs that violate a static rule MUST be rejected.
- A checked operation that fails MUST produce the documented typed outcome or
  runtime diagnostic, never memory-unsafe behavior.

Some behavior is intentionally platform-dependent:

- `intsize` and `uintsize` follow the host pointer width.
- Filesystem paths, process behavior, Unix sockets, available address
  families, and host error messages depend on the platform.
- The order of external events and of concurrently ready tasks is not a
  deterministic language guarantee unless an API states otherwise.
- Dictionary and set iteration follow the maintained runtime's
  insertion-oriented representation today. Programs should rely only on
  ordering that the relevant API contract explicitly promises.

Implementation-defined or platform-dependent behavior MUST stay within the
constraints documented by the relevant Manual page.

Behavior the specification does not grant is unspecified. Portable Aura
programs must not require it. This includes dependence on object layout, task
scheduling order, hash identity, native symbol names, and diagnostic byte
offsets.

## Reference Organization

Read the normative core in this order:

1. [Lexical Structure](/manual/lexical-structure)
2. [Grammar](/manual/grammar)
3. [Names And Scopes](/manual/names-and-scopes)
4. [Types](/manual/types) and [Static Semantics](/manual/static-semantics)
5. [Ownership And Borrowing](/manual/ownership-and-borrowing)
6. [Expressions](/manual/expressions), [Statements](/manual/statements),
   [Closures](/manual/closures), [FFI v0](/manual/ffi), and the declaration
   chapters
7. [Execution Model](/manual/execution-model)
8. the runtime and library chapters, and the [API Index](/manual/api-index)
9. [Diagnostics](/manual/diagnostics),
   [Current Limits](/manual/current-limits), and
   [Conformance](/manual/conformance)

The Learn track and a future book may reorder concepts for teaching, but they
MUST not contradict these rules.
