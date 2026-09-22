# Install Aura On Linux

The Aura 0.3 preview supports x86-64 Ubuntu 24.04 and compatible Linux
distributions that use glibc. The release has no Linux ARM64 or musl
archives.

## 1. Confirm The Host

```bash
uname -s
uname -m
```

On a supported host, the output is `Linux`, then `x86_64` or `amd64`.

## 2. Install Download And Verification Tools

On Ubuntu 24.04:

```bash
sudo apt update
sudo apt install -y curl ca-certificates tar coreutils
```

`coreutils` provides `sha256sum`, which the installer uses to verify the
downloaded archive.

## 3. Install Aura

```bash
curl -fsSL https://johnolafenwa.github.io/Aura/install.sh | sh
```

The installer selects `x86_64-unknown-linux-gnu` and downloads the archive
and `SHA256SUMS`. It rejects the archive if the checksum does not match, and
otherwise installs under `~/.local`.

## 4. Add Aura To Bash

Add `~/.local/bin` to your login environment. You only need to do this once:

```bash
grep -qxF 'export PATH="$HOME/.local/bin:$PATH"' "$HOME/.profile" || \
  printf '%s\n' 'export PATH="$HOME/.local/bin:$PATH"' >> "$HOME/.profile"
export PATH="$HOME/.local/bin:$PATH"
```

New login shells read `~/.profile`. The last command updates the current
terminal right away.

## 5. Verify The Installation

```bash
command -v aura
aura --version
```

The version begins with `aura 0.3.4-preview`.

## 6. Run A Program

Create `hello.au`:

```aura
def main():
    print("hello from Aura on Linux")
```

Run it:

```bash
aura run hello.au
```

## 7. Enable Native Builds

Direct native execution needs a C compiler and linker. Install them:

```bash
sudo apt install -y build-essential
```

Check the toolchain, then build and run the example:

```bash
cc --version
aura build -o hello hello.au
./hello
```

## Upgrade Aura

```bash
aura upgrade
aura --version
```

`aura upgrade` downloads the current installer and verifies the published
release checksums. It then replaces the compiler and bundled runtime in the
same install prefix. If Aura lives in a custom location, set
`AURA_INSTALL_PREFIX` when you upgrade.

## Troubleshooting

### `aura: command not found`

```bash
ls -l "$HOME/.local/bin/aura"
export PATH="$HOME/.local/bin:$PATH"
```

To make the fix permanent, add the export to the startup file your shell
reads.

### `sha256sum` is missing

Install `coreutils`, then run the installer again:

```bash
sudo apt install -y coreutils
```

### The archive will not start on the distribution

The published Linux binary targets x86-64 glibc systems. Check the
architecture with `uname -m` and the C library with `ldd --version`. Alpine
Linux and other musl systems are not in the current distribution matrix.

Continue with the [VS Code extension guide](/install/vscode) or
[Getting Aura Running](/learn/install-and-run).
