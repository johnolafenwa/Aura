# Supported Platforms

Aura 0.3 is a technical preview with this supported distribution matrix:

| Host | Architecture | CI | Release archive | Native `aura build` |
| --- | --- | --- | --- | --- |
| Ubuntu 24.04 / compatible glibc Linux | x86-64 | Yes | Yes | Yes, with a host C compiler |
| macOS 15 | x86-64 | Release smoke | Yes | Yes, with Xcode command-line tools |
| macOS 15 | Apple silicon | Yes | Yes | Yes, with Xcode command-line tools |
| Windows 11 with WSL 2 and Ubuntu 24.04 | x86-64 | Linux behavior inherited; no dedicated WSL runner | Use the Linux archive inside WSL | Yes, with `build-essential` inside WSL |

Aura does not publish a native Windows executable. The documented Windows
path runs the Linux x86-64 archive inside WSL 2. Native Windows, Windows on
ARM, musl Linux, cross-compilation, and other architectures are not supported
by the 0.3 preview archives. Source builds on those hosts are experimental
until they have CI, runtime-link manifests, and packaged-archive smoke tests.

The pinned development toolchain is Rust 1.95.0, Cargo LLVM coverage 0.8.4, Node.js 22.14.0, and npm 11.4.2. See `rust-toolchain.toml` and `package.json`.
