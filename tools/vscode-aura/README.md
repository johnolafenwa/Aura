# Aura Programming Language

Aura is a Python-familiar systems programming language with explicit ownership,
native compilation, and structured concurrency. The language, compiler, and
editor tooling are developed in the [Aura repository](https://github.com/johnolafenwa/Aura).

This preview extension provides the maintained VS Code experience for `.au`
source files:

- Aura syntax highlighting and language-aware indentation
- snippets for functions, classes, matching, concurrency, lambdas, and FFI
- compiler-backed diagnostics, completions, hover, go-to-definition, and
  document symbols
- recovery while editing incomplete functions, member access, comprehensions,
  slices, and numeric arrays

## Install

Install **Aura Programming Language** from the
[Visual Studio Marketplace](https://marketplace.visualstudio.com/items?itemName=JohnOlafenwa.vscode-aura-lang)
or [Open VSX](https://open-vsx.org/extension/JohnOlafenwa/vscode-aura-lang).
The matching `.vsix` is also attached to each
[Aura release](https://github.com/johnolafenwa/Aura/releases) for manual
installation through **Extensions: Install from VSIX...**.

The extension includes the editor client and JavaScript language-server
transport. Semantic analysis is performed by the actual Aura compiler server,
the `aura lsp` subcommand, so the `aura` executable must also be installed and
available on `PATH`.

Download a prebuilt CLI archive from
[Aura Releases](https://github.com/johnolafenwa/Aura/releases), or install from
source with Rust:

```bash
git clone https://github.com/johnolafenwa/Aura.git
cd Aura
cargo install --path crates/aura --locked --force
aura --version
```

Reload VS Code after installing the CLI, then open any `.au` file. To select a
specific compiler binary, launch VS Code with:

```bash
AURA_LSP_AURA_PATH="/absolute/path/to/aura" code /path/to/aura-project
```

## Language Support

The compiler-backed language server understands the maintained Aura surface,
including classes, enums, traits, generics, modules, `Result` and `Option`,
structured concurrency, closures, FFI, owned slices, and numeric arrays. It
also preserves useful editor assistance while the current buffer is incomplete.

The extension stays intentionally thin: syntax assets and snippets live here,
while semantic rules and diagnostics remain owned by the Aura compiler.

## Development

Contributors building the extension from the monorepo should follow the
[development installation guide](INSTALL.md). Issues belong in the
[Aura issue tracker](https://github.com/johnolafenwa/Aura/issues).

## License

Aura and this extension are available under the MIT License.
