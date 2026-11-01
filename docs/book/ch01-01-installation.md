# Installation

The first step is to install `aura`. Aura ships as a single command-line tool
together with a small native runtime that it links into the programs you
build. There is no separate interpreter, package manager, or language server
to install: the one `aura` binary does all of those jobs.

Aura 0.11 is a technical preview, and the set of platforms it ships for is
small. Release archives exist for these hosts:

| Platform | Release archive |
| --- | --- |
| macOS 15 on Apple silicon | `aarch64-apple-darwin` |
| macOS 15 on Intel | `x86_64-apple-darwin` |
| Ubuntu 24.04 or another glibc-based Linux, x86-64 | `x86_64-unknown-linux-gnu` |
| Windows 11, x86-64 | the Linux archive, inside WSL 2 |

There is no native Windows build, no Linux ARM64 build, and no build for
musl-based distributions such as Alpine. If your machine is not in the table,
you can still build Aura from source, which we cover at the end of this
section, but you are outside the set of hosts the project tests on.

## Installing with the install script

On macOS and Linux, one command downloads the right archive for your machine
and installs it:

```console
$ curl -fsSL https://johnolafenwa.github.io/Aura/install.sh | sh
```

The script looks at `uname` to pick the archive, downloads it along with the
release's `SHA256SUMS` file, and refuses to install anything if the checksum
does not match. If everything checks out, it installs under `~/.local`:

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

`bin/aura` is the tool itself. The files under `lib/aura` are the runtime
that `aura build` links into native executables, and `aura` finds them
relative to its own location, so keep the layout as it is. If you would
rather install somewhere other than `~/.local`, set `AURA_INSTALL_PREFIX`
when you run the script:

```console
$ AURA_INSTALL_PREFIX="$HOME/tools/aura" \
    sh -c "$(curl -fsSL https://johnolafenwa.github.io/Aura/install.sh)"
```

The script needs `curl`, `tar`, and a SHA-256 tool. macOS has all three
already. On Ubuntu, install them with
`sudo apt install -y curl ca-certificates tar coreutils` if they are missing;
`coreutils` provides the `sha256sum` command the script uses.

### Putting `aura` on your path

The installer puts the binary in `~/.local/bin`, and your shell needs to look
there. Many Linux setups already include that directory in `PATH`. If yours
does not, the installer prints the line to add. On Linux with bash, add it to
`~/.profile`:

```console
$ echo 'export PATH="$HOME/.local/bin:$PATH"' >> ~/.profile
$ export PATH="$HOME/.local/bin:$PATH"
```

On macOS, where the default shell is zsh, use `~/.zshrc` instead:

```console
$ echo 'export PATH="$HOME/.local/bin:$PATH"' >> ~/.zshrc
$ source ~/.zshrc
```

The first line makes the change permanent for new terminals, and the second
applies it to the terminal you have open now.

### Windows with WSL 2

Aura does not run natively on Windows. On a Windows 11 machine, you install
Ubuntu under the Windows Subsystem for Linux and use the Linux release inside
it. From an administrator PowerShell, install Ubuntu 24.04:

```console
> wsl --install -d Ubuntu-24.04
```

Restart if Windows asks you to, launch **Ubuntu 24.04 LTS** from the Start
menu, and create a Linux user when it prompts you. From then on, run every
command in this book in the Ubuntu terminal, not in PowerShell. Follow the
Linux steps above inside Ubuntu, and keep your projects under your Linux home
directory (for example `~/projects`) rather than under `/mnt/c`, so that file
permissions and other filesystem behavior match what Linux tools expect.

The [Windows with WSL guide](/install/windows-wsl) covers checking that you
are on WSL 2 rather than WSL 1, and connecting VS Code to the Ubuntu side.

## Checking the installation

Open a new terminal and ask `aura` for its version:

```console
$ aura --version
```

A release build prints a line that starts with `aura 0.11.0-preview`,
followed by the source commit it was built from in parentheses. If you built
Aura yourself, the line starts with `aura 0.11.0-dev` instead. Either way, if
you see a version, you are ready to go. `aura help` lists every command the
tool understands; we will meet the important ones over the next two sections.

If the shell says `aura: command not found`, check that the binary exists with
`ls -l ~/.local/bin/aura`, and then check that `~/.local/bin` is in your
`PATH`. That is almost always the problem.

## Installing a C toolchain for native builds

`aura` can run your programs as soon as it is installed. It can also compile
them to native executables with `aura build`, and for that last step it
needs your system's C compiler and linker, because it hands the final link to
them. If you only want to run programs for now, you can skip this and come
back later.

On macOS, install Apple's command-line tools:

```console
$ xcode-select --install
```

On Ubuntu, including Ubuntu under WSL, install `build-essential`:

```console
$ sudo apt install -y build-essential
```

You can check the result with `xcode-select -p` on macOS or `cc --version` on
Linux. We will build our first native binary in the next section.

## Updating

To move to a newer release, run:

```console
$ aura upgrade
```

`aura upgrade` downloads the current installer, verifies the release
checksums the same way a fresh install does, and replaces both the compiler
and its runtime in the place they are already installed. If you used a custom
prefix, set `AURA_INSTALL_PREFIX` again when you upgrade.

## Building from source

If you want to work on Aura itself, or your platform has no release archive,
you can build it from a checkout. You need three things: Rust through
[rustup](https://rustup.rs) (the repository's `rust-toolchain.toml` pins the
exact version), a host C compiler, and the static libraries of LLVM 22, which
Aura uses to generate native code. The repository includes a script that
fetches LLVM for you:

```console
$ git clone https://github.com/johnolafenwa/Aura.git
$ cd Aura
$ scripts/install-llvm.sh
$ export LLVM_SYS_221_PREFIX="$HOME/.cache/aura/llvm/22.1.2"
$ cargo build --release -p aura
$ export PATH="$PWD/target/release:$PATH"
```

On Linux, the LLVM script also needs the `libzstd-dev` and `libxml2-dev`
packages. On macOS it installs Homebrew's `llvm@22`, and it prints the
`LLVM_SYS_221_PREFIX` value to export, which is different from the Linux path
above.

## Editor support

You can write Aura in any text editor, but you will have a better time with
one that shows the compiler's errors as you type. The **Aura Programming
Language** extension for VS Code (and for editors that use Open VSX, such as
VSCodium) provides highlighting, diagnostics, completion, hover, and
go-to-definition. All of the language knowledge comes from `aura` itself,
through the `aura lsp` command, so the editor and the command line always
agree. We set it up at the end of
[Hello, packages](/book/ch01-03-hello-packages), once we have a project to
open.

## Getting help

The [installation guides](/install/) have a page per platform with
troubleshooting notes. The [Aura Manual](/manual/) is the reference for the
language and the tools, and the
[repository](https://github.com/johnolafenwa/Aura) is the place to report a
problem. With `aura` installed, let's write a program.
