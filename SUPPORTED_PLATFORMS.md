# Supported Platforms

Aura 0.2 is a technical preview with this supported distribution matrix:

| Host | Architecture | CI | Release archive | Native `aura build` |
| --- | --- | --- | --- | --- |
| Ubuntu 24.04 / compatible glibc Linux | x86-64 | Yes | Yes | Yes, with a host C compiler |
| macOS 15 | x86-64 | Release smoke | Yes | Yes, with Xcode command-line tools |
| macOS 15 | Apple silicon | Yes | Yes | Yes, with Xcode command-line tools |

Windows, musl Linux, cross-compilation, and other architectures are not supported by the 0.2 archives. Source builds on those hosts are experimental until they have CI, runtime-link manifests, and packaged-archive smoke tests.

The pinned development toolchain is Rust 1.95.0, Cargo LLVM coverage 0.8.4, Node.js 22.14.0, and npm 11.4.2. See `rust-toolchain.toml` and `package.json`.
