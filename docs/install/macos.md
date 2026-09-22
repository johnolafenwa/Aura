# Install Aura On macOS

Aura publishes separate macOS 15 archives for Apple silicon and Intel Macs.
The installer uses `uname` to pick the right one.

## 1. Confirm The Mac Architecture

Open Terminal and run:

```bash
uname -m
```

- `arm64` means Apple silicon.
- `x86_64` means Intel.

Both are supported. No other macOS architecture has a release archive.

## 2. Install Aura

The installer uses `curl`, `tar`, and `shasum`, which macOS already includes:

```bash
curl -fsSL https://johnolafenwa.github.io/Aura/install.sh | sh
```

The script downloads the matching `v0.3.4-preview` archive. It checks the
archive against the release's `SHA256SUMS` file before it copies anything
into the installation prefix.

## 3. Add Aura To zsh

If `aura --version` already works, `~/.local/bin` is already on `PATH` and you
can skip this step.

Aura installs to `~/.local/bin/aura` by default. Add that directory to your
zsh login environment. You only need to do this once:

```bash
grep -qxF 'export PATH="$HOME/.local/bin:$PATH"' "$HOME/.zshrc" || \
  printf '%s\n' 'export PATH="$HOME/.local/bin:$PATH"' >> "$HOME/.zshrc"
source "$HOME/.zshrc"
```

## 4. Verify The Installation

```bash
command -v aura
aura --version
```

The command path ends in `.local/bin/aura`. The version begins with:

```text
aura 0.3.4-preview
```

## 5. Run A Program

Create `hello.au`:

```aura
def main():
    print("hello from Aura on macOS")
```

Run it:

```bash
aura run hello.au
```

## 6. Enable Native Builds

`aura run` uses the default MIR execution path, which does not need Xcode.
MIR is Aura's mid-level intermediate representation. Direct native execution
and `aura build` need Apple's linker and C toolchain. Install them:

```bash
xcode-select --install
```

When the installer finishes, verify the toolchain, then build and run:

```bash
xcode-select -p
aura build -o hello hello.au
./hello
```

## Upgrade Aura

To upgrade the installed CLI and runtime to the current published preview:

```bash
aura upgrade
aura --version
```

`aura upgrade` keeps the active install prefix. It uses the same verified
installer as a fresh installation.

## Troubleshooting

### `aura: command not found`

Check that the file exists, then reload the shell:

```bash
ls -l "$HOME/.local/bin/aura"
source "$HOME/.zshrc"
```

### The installer reports an unsupported architecture

Run `uname -m`. Aura publishes macOS archives only for `arm64` and `x86_64`.

### Native linking fails

Run `xcode-select -p`. If it fails, install or repair the Xcode command-line
tools before you use `aura build` or `--backend direct`.

Continue with the [VS Code extension guide](/install/vscode) or
[Getting Aura Running](/learn/install-and-run).
