# Security Policy

Aura is an early technical preview. It has not received an independent security audit and must not yet be treated as a hardened sandbox for untrusted Aura programs.

## Supported Version

Only the latest 0.1 development line receives security fixes.

## Reporting

Do not open a public issue for a suspected vulnerability. Use GitHub's private vulnerability reporting for this repository and include a minimal reproducer, affected host and architecture, and whether the issue affects the compiler, generated programs, package handling, editor tooling, or documentation server.

## Current Boundaries

- Generated native programs link a Rust static runtime through the host C compiler.
- Package git dependencies execute the host `git` client against user-selected repositories.
- The language server processes workspace source and launches the packaged `aura` compiler service.
- Memory safety claims remain provisional while the native C ABI and generated binaries are gaining fuzzing and sanitizer coverage.
