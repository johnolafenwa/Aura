# Supported Platforms

Aura 0.6 is a technical preview. It supports these hosts:

| Host | Architecture | CI | Release archive | Native `aura build` |
| --- | --- | --- | --- | --- |
| Ubuntu 24.04 / compatible glibc Linux | x86-64 | Yes | Yes | Yes, with a host C compiler |
| macOS 15 | x86-64 | Release smoke | Yes | Yes, with Xcode command-line tools |
| macOS 15 | Apple silicon | Yes | Yes | Yes, with Xcode command-line tools |
| Windows 11 with WSL 2 and Ubuntu 24.04 | x86-64 | Inherits Linux behavior. No dedicated WSL runner | Use the Linux archive inside WSL | Yes, with `build-essential` inside WSL |

## Libraries `aura` Loads

The `aura` compiler links LLVM statically. On Linux it also loads
`libstdc++.so.6`, `libz.so.1`, and `libzstd.so.1`, which every supported
glibc distribution installs. On macOS it loads only system libraries.

## Windows

Aura does not publish a native Windows executable. On Windows, run the Linux
x86-64 archive inside WSL 2.

## Unsupported Hosts

The 0.6 preview archives do not support:

- native Windows
- Windows on ARM
- musl Linux
- cross-compilation
- other architectures

Source builds on those hosts are experimental. They have no CI, runtime-link
manifests, or packaged-archive smoke tests.

## Development Toolchain

| Tool | Pinned version |
| --- | --- |
| Rust | 1.95.0 |
| LLVM | 22.1.2 on Linux, Homebrew `llvm@22` on macOS |
| Cargo LLVM coverage | 0.8.4 |
| Node.js | 22.14.0 |
| npm | 11.4.2 |

`rust-toolchain.toml` and `package.json` hold the pins, and
`scripts/install-llvm.sh` holds the LLVM one.
