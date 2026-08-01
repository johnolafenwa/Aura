# Downloads

Aura 0.2.0 is a technical preview. The compiler, command-line tools, editor
extension, reference manual, and source are distributed from the
[Aura GitHub repository](https://github.com/johnolafenwa/Aura).

## Aura CLI

Download the archive for your platform from the
[v0.2.0-preview release](https://github.com/johnolafenwa/Aura/releases/tag/v0.2.0-preview).
Each release includes Linux x64, macOS x64, and macOS arm64 archives together
with a `SHA256SUMS` manifest.

After extracting an archive, put its `bin` directory on `PATH` and verify the
installation:

```bash
aura --version
```

## VS Code Extension

Install **Aura Programming Language** from either public extension registry:

- [Visual Studio Marketplace](https://marketplace.visualstudio.com/items?itemName=JohnOlafenwa.vscode-aura)
- [Open VSX](https://open-vsx.org/extension/JohnOlafenwa/vscode-aura)

The registry packages are identical and carry the plain extension version
`0.2.0`. The extension needs the `aura` executable on `PATH` because semantic
editor features run through the compiler-owned `aura lsp` server.

For a manual installation, download
[`aura-language.vsix`](https://github.com/johnolafenwa/Aura/releases/download/v0.2.0-preview/aura-language.vsix)
from the GitHub Release, then choose **Extensions: Install from VSIX...** in
VS Code.

## Documentation And Source

The release also includes the static Aura documentation archive. The current
book is available on [GitHub Pages](https://johnolafenwa.github.io/Aura/), and
the complete source is available from the
[Aura repository](https://github.com/johnolafenwa/Aura).
