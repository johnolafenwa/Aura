# Aurora Architecture Docs

This folder is a guided architecture map for Aurora as it exists in this repository today. It is written for two audiences at once:

- readers who want an accurate explanation of how the current implementation works
- readers who are new to compiler internals and need each stage explained from first principles

The docs are intentionally grounded in the actual source tree. When a chapter says "this is how Aurora works", it points at the real implementation files, not an idealized design that only exists in a proposal.

![Aurora compiler pipeline](assets/compiler-pipeline.svg)

```mermaid
flowchart LR
    A["Aurora source (.au)"] --> B["lexer.rs"]
    B --> C["parser.rs"]
    C --> D["ast.rs"]
    D --> E["sema.rs"]
    E --> F["mir.rs"]
    F --> G["mir_runtime.rs (aura run)"]
    F --> H["native_codegen.rs (aura build)"]
    H --> I["native_runtime.rs"]
    E --> J["analysis.rs (aura analyze / complete)"]
    J --> K["aurora-language-server"]
    K --> L["vscode-aurora"]
```

## Reading order

1. [00-glossary.md](00-glossary.md)
2. [01-system-overview.md](01-system-overview.md)
3. [02-ast-and-source-model.md](02-ast-and-source-model.md)
4. [03-lexer.md](03-lexer.md)
5. [04-parser.md](04-parser.md)
6. [05-semantic-analysis.md](05-semantic-analysis.md)
7. [06-mir.md](06-mir.md)
8. [07-mir-runtime.md](07-mir-runtime.md)
9. [08-native-codegen-and-runtime.md](08-native-codegen-and-runtime.md)
10. [09-packages-and-module-loading.md](09-packages-and-module-loading.md)
11. [10-cli-and-build-tools.md](10-cli-and-build-tools.md)
12. [11-editor-tooling.md](11-editor-tooling.md)
13. [12-testing-and-quality.md](12-testing-and-quality.md)
14. [13-end-to-end-walkthrough.md](13-end-to-end-walkthrough.md)

## Source anchors

The main implementation files these docs refer to are:

- [`crates/aurora-compiler/src/lib.rs`](../crates/aurora-compiler/src/lib.rs)
- [`crates/aurora-compiler/src/lexer.rs`](../crates/aurora-compiler/src/lexer.rs)
- [`crates/aurora-compiler/src/parser.rs`](../crates/aurora-compiler/src/parser.rs)
- [`crates/aurora-compiler/src/ast.rs`](../crates/aurora-compiler/src/ast.rs)
- [`crates/aurora-compiler/src/sema.rs`](../crates/aurora-compiler/src/sema.rs)
- [`crates/aurora-compiler/src/mir.rs`](../crates/aurora-compiler/src/mir.rs)
- [`crates/aurora-compiler/src/mir_runtime.rs`](../crates/aurora-compiler/src/mir_runtime.rs)
- [`crates/aurora-compiler/src/runtime_value.rs`](../crates/aurora-compiler/src/runtime_value.rs)
- [`crates/aurora-compiler/src/native_codegen.rs`](../crates/aurora-compiler/src/native_codegen.rs)
- [`crates/aurora-compiler/src/native_runtime.rs`](../crates/aurora-compiler/src/native_runtime.rs)
- [`crates/aurora-compiler/src/package.rs`](../crates/aurora-compiler/src/package.rs)
- [`crates/aurora-compiler/src/analysis.rs`](../crates/aurora-compiler/src/analysis.rs)
- [`crates/aura/src/main.rs`](../crates/aura/src/main.rs)
- [`tools/aurora-language-server/src/server.js`](../tools/aurora-language-server/src/server.js)
- [`tools/aurora-language-server/src/compiler_bridge.js`](../tools/aurora-language-server/src/compiler_bridge.js)
- [`tools/vscode-aurora/src/extension.js`](../tools/vscode-aurora/src/extension.js)

## What Aurora is today

Aurora is a language implementation monorepo. The current maintained execution architecture is:

- `aura run` parses, checks, lowers to MIR, and executes with the MIR runtime
- `aura build` parses, checks, lowers to MIR, lowers again into native code with Cranelift, and links against the direct runtime
- editor tooling prefers compiler-produced analysis and only falls back to local JavaScript analysis when it has to

These docs explain that architecture, not an older interpreter-only design and not a future proposal-only design.
