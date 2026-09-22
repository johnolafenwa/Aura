# Install Aura

Aura 0.3 ships as a self-contained command-line tool with its own private
native runtime. Pick the guide for the operating system where `aura` will run:

| Platform | Release archive | Guide |
| --- | --- | --- |
| macOS 15, Apple silicon | `aarch64-apple-darwin` | [Install on macOS](/install/macos) |
| macOS 15, Intel | `x86_64-apple-darwin` | [Install on macOS](/install/macos) |
| Ubuntu 24.04 or compatible glibc Linux, x86-64 | `x86_64-unknown-linux-gnu` | [Install on Linux](/install/linux) |
| Windows 11, x86-64 | Linux archive inside WSL 2 | [Install on Windows with WSL](/install/windows-wsl) |

Every guide follows the same three steps.

1. Run the installer. It detects the supported archive, verifies its SHA-256
   checksum, and installs under `~/.local`.

```bash
curl -fsSL https://johnolafenwa.github.io/Aura/install.sh | sh
```

2. Add `~/.local/bin` to `PATH`.

3. Verify the result in the same terminal:

```bash
aura --version
```

The output begins with `aura 0.3.4-preview`. The rest of the line is the
source commit the binary was built from.

## What Gets Installed

The default layout is:

```text
~/.local/
├── bin/aura
├── lib/aura/
│   ├── libaura_compiler.a
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

Each platform guide gives the exact commands.

## Next Step

Continue with [Getting Aura Running](/learn/install-and-run) to create a
source file, run it, check it, and build a native executable.
