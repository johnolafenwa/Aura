# Install The VS Code Extension

The **Aura Programming Language** extension provides `.au` syntax
highlighting, indentation, snippets, diagnostics, completion, hover,
go-to-definition, and document symbols.

The extension bundles its JavaScript editor client and the language-server
transport. Semantic analysis comes from the compiler's own server, which
`aura lsp` starts. Install the [Aura CLI](/install/) first, then check that it
works:

```bash
aura --version
aura help
```

Next, install the extension with one of the methods below. Then
[verify language support](#verify-language-support).

## Install From Visual Studio Marketplace

1. Open the Extensions view in VS Code.
2. Search for **Aura Programming Language** from publisher **JohnOlafenwa**.
3. Select **Install**.
4. Reload VS Code.

- [Open the Visual Studio Marketplace listing](https://marketplace.visualstudio.com/items?itemName=JohnOlafenwa.vscode-aura-lang)

To install from a terminal instead, run this command and then reload VS Code:

```bash
code --install-extension JohnOlafenwa.vscode-aura-lang
```

## Install From Open VSX

Editors that use Open VSX, such as VSCodium, can install the same extension:

- [Open the Open VSX listing](https://open-vsx.org/extension/JohnOlafenwa/vscode-aura-lang)

In VSCodium, search for **Aura Programming Language** in Extensions, or run:

```bash
codium --install-extension JohnOlafenwa.vscode-aura-lang
```

## Install The Release VSIX Manually

A VSIX file is a packaged VS Code extension.

1. Download `aura-language.vsix` from the
   [v0.3.4-preview release](https://github.com/johnolafenwa/Aura/releases/tag/v0.3.4-preview).
2. Open the Command Palette and choose **Extensions: Install from VSIX...**.

To install from a terminal instead:

```bash
code --install-extension ./aura-language.vsix
```

By default, VS Code does not update extensions installed from a VSIX. To
upgrade, install the next release's VSIX by hand.

## Install In WSL

The Aura extension and the `aura` CLI must both run in the Windows Subsystem
for Linux (WSL) environment. An extension installed only in the local Windows
extension host cannot reliably reach the Linux compiler server.

1. Complete [Install Aura On Windows With WSL](/install/windows-wsl).
2. From the Ubuntu terminal, open the project with `code .`.
3. Check that the remote status bar shows **WSL: Ubuntu**.
4. In that remote window, open Extensions, find
   **Aura Programming Language**, and select **Install in WSL: Ubuntu**.
5. In VS Code's integrated terminal, verify the remote environment:

```bash
command -v aura
aura --version
```

## Use A Specific Aura Binary

The extension launches `aura` from `PATH`. To use a different binary, start
VS Code with `AURA_LSP_AURA_PATH` set to that binary's absolute path:

```bash
AURA_LSP_AURA_PATH="$HOME/tools/aura/bin/aura" code /path/to/project
```

In WSL, run this command from the WSL terminal so the path is a Linux path.

## Verify Language Support

Create or open a file ending in `.au`:

```aura
def greet(name: str) -> str:
    return f"hello {name}"

print(greet("Aura"))
```

Check each of the following:

1. The language mode in the lower-right corner reads **Aura**.
2. Keywords, strings, types, and interpolation get Aura highlighting.
3. An incomplete or invalid expression shows an `AU####` diagnostic.
4. Completion appears after a binding or member-access prefix.
5. Hover shows type information from the compiler.

## Troubleshooting

### The file opens as plain text

Check that the filename ends in `.au`. Click the language mode in the
lower-right corner and choose **Aura**.

### Syntax colors work but semantic features do not

Syntax highlighting is bundled with the extension. Semantic features need
`aura lsp`. Open VS Code's integrated terminal and run:

```bash
command -v aura
aura --version
```

After you fix `PATH`, restart VS Code. You can also launch it with
`AURA_LSP_AURA_PATH` as shown above.

### Inspect the language-server output

Open **View → Output**, then select **Aura Language Server**. Startup and
request failures appear there. You do not need a separate server package.
