# Glossary

This glossary gives short, concrete definitions for the terms used throughout the architecture docs.

## Core language terms

- Source text
  Aurora code exactly as written in a `.au` file or editor buffer.
- Token
  A small classified unit produced by the lexer, such as `Identifier("main")`, `KwIf`, `IntLiteral(42)`, `Indent`, or `Dedent`.
- AST
  The abstract syntax tree. Aurora stores source structure in Rust data types such as `Module`, `Stmt`, `Expr`, and `TypeRef`.
- TypeRef
  A syntactic type annotation from the parser, before semantic analysis decides what it means.
- Program
  Aurora's typed semantic model. It contains the original module plus symbol tables for classes, enums, functions, traits, imports, and module namespaces.
- MIR
  Middle intermediate representation. Aurora lowers checked programs into a control-flow-graph style IR with blocks, instructions, operands, and terminators.
- Runtime value
  The concrete value manipulated at execution time. In Aurora this lives in `runtime_value.rs` and includes scalars, collections, class instances, enum variants, tasks, queues, and I/O resources.

## Compiler pipeline terms

- Lexing
  Turning text into tokens.
- Parsing
  Turning tokens into the AST.
- Semantic analysis
  Turning the raw AST into a checked, typed model while validating names, types, ownership rules, traits, and control flow.
- Lowering
  Translating a higher-level representation into a lower-level one. Aurora lowers `Program` into MIR, and MIR into native code for `build`.
- Direct backend
  Aurora's native code generation path in `native_codegen.rs`. It compiles MIR to machine code using Cranelift and relies on `native_runtime.rs`.
- MIR runtime
  The execution engine in `mir_runtime.rs` that interprets MIR directly.

## Repository terms

- Module
  A single Aurora source file plus its imports and top-level items.
- Package
  A manifest-rooted unit using `Aurora.toml` and `src/`.
- Workspace
  A set of Aurora packages managed from a workspace-level `Aurora.toml`.
- Lockfile
  `Aurora.lock`, which records resolved package dependency state, especially git revisions.
- Builtin module
  A compiler-injected namespace such as `io`, `fs`, or `net`.
- Compiler analysis
  Machine-readable diagnostics, symbol, hover, definition, and completion data emitted by `analysis.rs` and surfaced through `aura analyze` and `aura complete`.
- LSP
  Language Server Protocol. Aurora uses it to power editor features such as diagnostics, hover, go-to-definition, and completion.

## Rust implementation terms

- Recursive descent parser
  A parser built as a set of hand-written Rust functions where each function parses one grammar region.
- Basic block
  A straight-line sequence of MIR instructions with exactly one terminator at the end.
- Terminator
  A MIR operation that changes control flow, such as `Goto`, `Branch`, `Match`, `Select`, or `Return`.
- Opaque value
  A boxed runtime value used by the direct native backend when a value is too rich to pass directly in registers.
- Plain class
  A native-codegen optimization category for classes whose fields can be passed directly through the ABI instead of boxing.
