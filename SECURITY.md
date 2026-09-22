# Security Policy

Aura is an early technical preview. It has had no independent security audit.
Do not treat it as a hardened sandbox for untrusted Aura programs.

## Supported Version

Only the latest 0.3 preview receives security fixes.

## Reporting

Do not open a public issue for a suspected vulnerability. Use GitHub's private
vulnerability reporting for this repository. Include:

- a minimal reproducer
- the affected host and architecture
- which part is affected: the compiler, generated programs, package handling,
  editor tooling, or the documentation server

## Current Boundaries

- Generated native programs link a Rust static runtime through the host C
  compiler.
- Package git dependencies run the host `git` client against repositories the
  user selects.
- The language server processes workspace source and launches the packaged
  `aura` compiler service.
- Memory safety claims are provisional. The native C ABI and generated
  binaries do not yet have full fuzzing and sanitizer coverage.
