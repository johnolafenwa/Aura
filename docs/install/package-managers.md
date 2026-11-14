# Install Aura With A Package Manager

Aura has packages for Homebrew, apt and Chocolatey. They are built and
tested with every change, but they are not published yet, so the commands
on this page do not work today. Until they are, use the installer in the
[platform guides](/install/).

A package installs the same `aura` command and native runtime as the
installer. The package manager owns the files, so upgrade and remove Aura
through it. `aura upgrade` prints the command to use.

| Package manager | Hosts | Install |
| --- | --- | --- |
| [Homebrew](#homebrew) | macOS 15 on Apple silicon and Intel, glibc Linux on x86-64 and arm64 | `brew install johnolafenwa/aura/aura` |
| [apt](#apt) | Ubuntu 24.04 or later and Debian 13 or later, on x86-64 and arm64 | `sudo apt-get install aura` |
| [Chocolatey](#chocolatey) | Windows 11 on x86-64 | `choco install aura --pre` |

## Homebrew

```bash
brew install johnolafenwa/aura/aura
aura --version
```

The formula lives in the `johnolafenwa/homebrew-aura` tap, and `brew`
adds the tap on first use. It downloads the release archive for the host
and checks it against the SHA-256 checksum in the formula.

`aura build` and native `aura run` link programs with the system C
compiler. On macOS, install the Xcode command-line tools with
`xcode-select --install`. On Linux, install `build-essential` or your
distribution's equivalent.

To upgrade or remove Aura:

```bash
brew upgrade aura
brew uninstall aura
```

## apt

Add Aura's signing key and repository once:

```bash
sudo apt-get update
sudo apt-get install -y curl ca-certificates
curl -fsSL https://johnolafenwa.github.io/Aura/apt/aura-archive-keyring.gpg |
  sudo tee /usr/share/keyrings/aura-archive-keyring.gpg >/dev/null
echo "deb [signed-by=/usr/share/keyrings/aura-archive-keyring.gpg] https://johnolafenwa.github.io/Aura/apt stable main" |
  sudo tee /etc/apt/sources.list.d/aura.list >/dev/null
```

Then install Aura:

```bash
sudo apt-get update
sudo apt-get install -y aura
aura --version
```

The package installs `/usr/bin/aura`, the runtime under `/usr/lib/aura`,
and the examples under `/usr/share/aura/examples`. It recommends `gcc` and
`libc6-dev`, which `aura build` and native `aura run` link programs with,
so apt installs them too unless you pass `--no-install-recommends`.

The repository holds the newest release. Its package needs glibc 2.39 or
later, which Ubuntu 24.04 and Debian 13 have.

To upgrade or remove Aura:

```bash
sudo apt-get update && sudo apt-get install --only-upgrade aura
sudo apt-get remove aura
```

### Install One Package File

Each release also carries its Debian packages as files, named like
`aura-v0.16.0-preview-amd64.deb` and `aura-v0.16.0-preview-arm64.deb`.
To install one without adding the repository, download it from the
[releases page](https://github.com/johnolafenwa/Aura/releases) and run:

```bash
sudo apt-get install ./aura-v0.16.0-preview-amd64.deb
```

## Chocolatey

In an administrator PowerShell:

```powershell
choco install aura --pre
aura --version
```

Aura is a technical preview, so its versions are prereleases and
Chocolatey needs `--pre` to see them. The package carries the files of the
Windows release archive and downloads nothing else.

`aura build` and native `aura run` link programs with the Microsoft
linker. If the machine does not have the Visual Studio C++ build tools,
install them too:

```powershell
choco install visualstudio2022buildtools visualstudio2022-workload-vctools
```

To upgrade or remove Aura:

```powershell
choco upgrade aura --pre
choco uninstall aura
```

Continue with the [VS Code extension guide](/install/vscode) or
[Hello, world](/book/ch01-02-hello-world).
