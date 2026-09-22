# Downloads

Aura 0.3.4 is a technical preview. The compiler, command-line tools, editor
extension, reference manual, and source all ship from the
[Aura GitHub repository](https://github.com/johnolafenwa/Aura).

## Aura CLI

Install the current preview with one command. This works on a supported macOS
or Linux host, including x86-64 Ubuntu 24.04 inside Windows WSL 2:

```bash
curl -fsSL https://johnolafenwa.github.io/Aura/install.sh | sh
```

The installer downloads the matching release archive and verifies it against
the published `SHA256SUMS`. It installs Aura under `~/.local` by default. Set
`AURA_INSTALL_PREFIX` to choose another prefix.

For step-by-step instructions, pick a platform guide:

- [Installation overview](/install/)
- [macOS: Apple silicon and Intel](/install/macos)
- [Linux: Ubuntu 24.04 and compatible x86-64 glibc hosts](/install/linux)
- [Windows 11 through Ubuntu on WSL 2](/install/windows-wsl)

To install by hand, download the archive for your platform from the
[v0.3.4-preview release](https://github.com/johnolafenwa/Aura/releases/tag/v0.3.4-preview).
Each release includes Linux x64, macOS x64, and macOS arm64 archives and a
`SHA256SUMS` manifest.

Extract the archive, add its `bin` directory to `PATH`, and verify the
installation:

```bash
aura --version
```

## VS Code Extension

Install **Aura Programming Language** from either public extension registry:

- [Visual Studio Marketplace](https://marketplace.visualstudio.com/items?itemName=JohnOlafenwa.vscode-aura-lang)
- [Open VSX](https://open-vsx.org/extension/JohnOlafenwa/vscode-aura-lang)

Both registries carry the same package, with the plain extension version
`0.3.5`. The extension needs the `aura` executable on `PATH`. Its semantic
editor features run through the `aura lsp` server that the compiler provides.

To install from a terminal:

```bash
code --install-extension JohnOlafenwa.vscode-aura-lang
```

To install by hand, download
[`aura-language.vsix`](https://github.com/johnolafenwa/Aura/releases/download/v0.3.4-preview/aura-language.vsix)
from the GitHub Release. Then run **Extensions: Install from VSIX...** in
VS Code.

The [complete VS Code guide](/install/vscode) covers the Marketplace, Open
VSX, manual VSIX installation, custom compiler paths, verification, and
installing into a WSL remote extension host.

## Documentation And Source

The release also includes an archive of the static Aura documentation. The
current book is on [GitHub Pages](https://johnolafenwa.github.io/Aura/), and
the complete source is in the
[Aura repository](https://github.com/johnolafenwa/Aura).
