# Aura Programming Language

Aura is a compiled, statically typed programming language with
Python-familiar syntax, deterministic ownership, native executables, and
structured concurrency. The language, compiler, and editor tooling are
developed in the [Aura repository](https://github.com/johnolafenwa/Aura).

Aura's current focus includes reliable applications, agents, and ML
infrastructure. Its long-term goal is a general-purpose systems language for
every layer of software, including operating systems and device drivers.

This preview extension is the maintained VS Code support for `.au` source
files. It provides:

- Aura syntax highlighting and language-aware indentation, with a distinct
  `string.quoted.docstring` scope for statement-position triple-quoted strings
- snippets for functions, classes, matching, concurrency, lambdas, and the
  foreign function interface (FFI)
- compiler-backed diagnostics, completions, hover, signature help,
  go-to-definition, and document symbols, with docstrings in hover,
  completions, and signature help
- recovery while you edit incomplete functions, member access, comprehensions,
  slices, and numeric arrays

## Install

Install **Aura Programming Language** from the
[Visual Studio Marketplace](https://marketplace.visualstudio.com/items?itemName=JohnOlafenwa.vscode-aura-lang)
or [Open VSX](https://open-vsx.org/extension/JohnOlafenwa/vscode-aura-lang).
Each [Aura release](https://github.com/johnolafenwa/Aura/releases) also has
the matching `.vsix` attached. Install it by hand with
**Extensions: Install from VSIX...**.

The extension contains the editor client and the JavaScript language-server
transport. The Aura compiler does the semantic analysis through its
`aura lsp` subcommand, so the `aura` executable must also be installed and on
`PATH`.

Download a prebuilt CLI archive from
[Aura Releases](https://github.com/johnolafenwa/Aura/releases), or install from
source with Rust:

```bash
git clone https://github.com/johnolafenwa/Aura.git
cd Aura
cargo install --path crates/aura --locked --force
aura --version
```

Reload VS Code after you install the CLI, then open any `.au` file. The
extension also looks in `~/.local/bin`, `~/.cargo/bin`, `/opt/homebrew/bin`,
and `/usr/local/bin` when VS Code does not see `aura` on `PATH`. To use a
specific compiler binary, set **Aura: Compiler Path** (`aura.compilerPath`)
in your user settings; the server restarts on its own. **Aura: Restart
Language Server** restarts it by hand.

The [complete installation guide](https://johnolafenwa.github.io/Aura/install/vscode)
covers command-line installation, WSL remote setup, custom compiler paths, and
verification.

## Language Support

The compiler-backed language server understands the maintained Aura surface.
That includes classes, enums, traits, generics, modules, `Result`, optional
`T | None` unions, `Lookup` and `Poll`, structured concurrency, closures, FFI,
owned slices, and numeric arrays. It keeps giving useful help while the
current buffer is incomplete.

Format Document uses the compiler's formatter, the same one `aura fmt` runs.
To format on save, add this to your settings:

```json
"[aura]": {
  "editor.formatOnSave": true
}
```

The extension stays thin on purpose. Syntax assets and snippets live here. The
Aura compiler owns semantic rules and diagnostics.

## Development

Contributors who build the extension from the monorepo should follow the
[development installation guide](INSTALL.md). Report issues in the
[Aura issue tracker](https://github.com/johnolafenwa/Aura/issues).

## License

Aura and this extension are available under the MIT License.
