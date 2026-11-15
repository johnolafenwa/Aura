# Install Aura

Aura 0.16 ships as a self-contained command-line tool with its own private
native runtime. Pick the guide for the operating system where `aura` will run:

| Platform | Release archive | Guide |
| --- | --- | --- |
| macOS 15, Apple silicon | `aarch64-apple-darwin` | [Install on macOS](/install/macos) |
| macOS 15, Intel | `x86_64-apple-darwin` | [Install on macOS](/install/macos) |
| Ubuntu 24.04 or compatible glibc Linux, x86-64 | `x86_64-unknown-linux-gnu` | [Install on Linux](/install/linux) |
| Ubuntu 24.04 or compatible glibc Linux, arm64 | `aarch64-unknown-linux-gnu` | [Install on Linux](/install/linux) |
| Windows 11, x86-64 | `x86_64-pc-windows-msvc` | [Install on Windows](/install/windows) |
| Windows 11, x86-64 | Linux archive inside WSL 2 | [Install on Windows with WSL](/install/windows-wsl) |

Packages for Homebrew, apt and Chocolatey are built but not published
yet. See [Install with a package manager](/install/package-managers).

Every guide follows the same three steps.

1. Run the installer. It detects the supported archive, verifies its SHA-256
   checksum, and installs under `~/.local`, or under
   `%LOCALAPPDATA%\Programs\Aura` on Windows.

```bash
curl -fsSL https://johnolafenwa.github.io/Aura/install.sh | sh
```

On Windows, in PowerShell:

```powershell
irm https://johnolafenwa.github.io/Aura/install.ps1 | iex
```

2. Add the `bin` directory, `~/.local/bin` on macOS and Linux, to `PATH`.

3. Verify the result in the same terminal:

```bash
aura --version
```

The output begins with `aura 0.16.0-preview`. The rest of the line is the
source commit the binary was built from.

## What Gets Installed

The default layout is:

```text
~/.local/
├── bin/aura
├── lib/aura/
│   ├── libaura_runtime.a
│   └── native-link-args.json
└── share/aura/
    ├── examples/
    ├── README.md
    └── LICENSE
```

To install under a different prefix, set `AURA_INSTALL_PREFIX`:

```bash
AURA_INSTALL_PREFIX="$HOME/tools/aura" \
  sh -c "$(curl -fsSL https://johnolafenwa.github.io/Aura/install.sh)"
```

Then add that prefix's `bin` directory to `PATH`.

## Editor Setup

Once the CLI works, install the
[Aura Programming Language extension](/install/vscode). The extension
provides the editor client, syntax grammar, and snippets. Diagnostics,
completion, hover, definitions, and symbols come from the compiler through the
installed `aura lsp` server.

## Native Builds

`aura run` and `aura check` work as soon as the archive is installed. Direct
native execution and `aura build` also need a host C toolchain:

- macOS: Xcode command-line tools
- Ubuntu and WSL: `build-essential`
- Windows: the Visual Studio Build Tools with the **Desktop development with
  C++** workload

Each platform guide gives the exact commands.

## Next Step

Continue with [Hello, world](/book/ch01-02-hello-world) in the book to
write a program, run it, and build a native executable.
