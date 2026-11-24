# Aura

Aura is a compiled, statically typed programming language for reliable
software. It pairs Python-inspired syntax with deterministic ownership,
structured concurrency, typed failure, and native executables. Functions
state what they need and what they promise, and `aura verify` proves those
contracts for every input before the program runs. Aura has no garbage
collector.

```aura
def average(scores: list[int64]) -> int64:
    requires 0 < len(scores) and len(scores) <= 1000
    requires forall(s in scores, 0 <= s and s <= 100)
    ensures 0 <= result and result <= 100
    mut total = 0
    for i in range(len(scores)):
        invariant 0 <= total and total <= i * 100
        total += scores[i]
    return total // len(scores)

def main():
    print(average([70, 85, 100]))
```

Save this as `scores.au` and run it with `aura run scores.au`. It prints
`85`. The `requires` lines say what a caller must pass, and the `ensures`
line says what comes back. `aura run` checks them while the program runs.
`aura verify` proves them, with an SMT solver, for every list a caller could
pass:

```console
$ aura verify scores.au
verified scores.au: 10 of 10 checks proved in 2 functions (/usr/bin/z3 (Z3 version 5.3.3 - 64 bit))
```

The ten checks cover the contract and the traps the body could hit: the sum
cannot overflow, the index stays in range, and the divisor is never zero.
Delete the first `requires` line and the proof fails. Among the checks
`aura verify` can no longer prove, it reports the input that breaks the
division:

```text
error[AU5101]: cannot prove divisor is not zero
 --> scores.au:8:12
  |
8 |     return total // len(scores)
  |            ^
  = note: in `average` (division by zero check)
  = note: counterexample: len(scores) = 0
```

[Contracts](docs/manual/contracts.md) and
[Verification](docs/manual/verification.md) are the reference, and
[the book](docs/book/ch11-04-contracts-and-proofs.md) teaches both.

## Install

Install the Aura 0.17 technical preview on Linux x64, Linux arm64, macOS x64,
or macOS arm64:

```bash
curl -fsSL https://johnolafenwa.github.io/Aura/install.sh | sh
```

On Windows x64, in PowerShell:

```powershell
irm https://johnolafenwa.github.io/Aura/install.ps1 | iex
```

To upgrade the compiler and bundled runtime of an existing install, run:

```bash
aura upgrade
```

Step-by-step guides cover
[macOS](https://johnolafenwa.github.io/Aura/install/macos),
[Linux](https://johnolafenwa.github.io/Aura/install/linux),
[Windows](https://johnolafenwa.github.io/Aura/install/windows), and
[Windows through WSL 2](https://johnolafenwa.github.io/Aura/install/windows-wsl).
The [VS Code guide](https://johnolafenwa.github.io/Aura/install/vscode) covers
Marketplace, Open VSX, VSIX, and WSL remote installation.
[SUPPORTED_PLATFORMS.md](SUPPORTED_PLATFORMS.md) lists supported hosts and
pinned tools.

## What Aura Is For

Aura 0.17 targets applications for agents, ML infrastructure, and the control
planes around models. Before a program runs, the compiler checks types,
access, mutation, ownership transfer, resource cleanup, and task boundaries.
`aura verify` then proves what types cannot express: that contracts hold,
arithmetic stays in range, and indexes and keys exist.

The long-term goal is a general-purpose systems language for every kind of
software: applications, services, databases, language runtimes, embedded
software, operating systems, and device drivers. A
[freestanding build](docs/manual/freestanding.md) compiles code over
numbers and plain classes with no runtime, and the
[`mem` module](docs/manual/mem.md) reads and writes device registers. Heap
values in freestanding code, inline assembly, and portable layout controls
are not implemented yet. [Why Aura](docs/positioning.md)
explains the project direction.

## Documentation

- [The Aura Programming Language](docs/book/index.md): the book. It teaches
  Aura from installation to a concurrent job runner, one program at a time.
- [Contracts](docs/manual/contracts.md) and
  [Verification](docs/manual/verification.md): `requires`, `ensures`, and
  invariants, checked at run time and proved for every input by
  `aura verify`.
- [Examples](examples/README.md): the categorized library of runnable
  programs.
- [Language Specification](docs/manual/language-specification.md) and
  [complete grammar](docs/manual/grammar.md): the implemented contract, together
  with the rest of the [Manual](docs/manual/index.md).
- [CLI and Tooling](docs/manual/cli-and-tooling.md): every `aura` command,
  backend, and output format.
- [Concurrency](docs/manual/concurrency.md): tasks, queues, the scheduler, and
  its environment settings.
- [Tensors](docs/manual/tensors.md): `Tensor[T]` on the CPU, Metal, and CUDA,
  with gradients, layers, and optimizers.
- [FFI](docs/manual/ffi.md): calling C libraries from an Aura package.
- [Performance](docs/manual/performance.md): current measurements and the
  optimization roadmap.
- [ML systems design notes](docs/ml_systems_support_plan.md): the earlier
  design document for ML systems work.

## Monorepo layout

| Path | Contents |
| --- | --- |
| `crates/` | Rust compiler, runtime, and CLI |
| `tools/` | Editor integrations and other developer tools |
| `package.json` | npm workspace manifest for the repository's tools |
| `examples/` | Categorized sample Aura programs |
| `benchmarks/` | Benchmark programs, baselines in other languages, and recorded results |
| `scripts/` | Build, release, benchmark, and repository check scripts |
| `packaging/` | Homebrew, Debian, and Chocolatey package definitions |
| `docs/` | VitePress site: the book (`docs/book/`), the Manual, install guides, and supporting documents |
| `architecture_docs/` | Architecture guides and component deep dives for the current implementation |
| `work/` | Task board and implementation notes |

Each component has its own README:

- [crates/aura](crates/aura/README.md): building the compiler and using the
  binary directly.
- [crates/aura-compiler](crates/aura-compiler/README.md): compiler library
  tests.
- `crates/aura-core`: semantic types, diagnostics, and integer helpers
  shared by the compiler and the native runtime.
- `crates/aura-runtime`: the native runtime that executables link, with
  runtime values, the scheduler, host builtins, codecs, tensors, and FFI.
- `crates/aura-mem`: the memory copy and compare routines that musl programs
  link.
- `tools/vscode-aura`: the VS Code extension for syntax highlighting and the
  language client.
- `tools/aura-language-server`: the Aura Language Server Protocol
  implementation.

## Contributing

[CONTRIBUTING.md](CONTRIBUTING.md) covers the test-first workflow, the local
gate, and the development commands. The
[testing strategy](docs/testing_strategy.md) explains how the repository is
tested. The [architecture guide](architecture_docs/README.md) describes how the
implementation fits together. Report vulnerabilities as described in
[SECURITY.md](SECURITY.md).

## VS Code install

Install the extension from the Marketplace or Open VSX as the
[VS Code guide](https://johnolafenwa.github.io/Aura/install/vscode) describes.
The extension starts the compiler's `aura lsp` service, so `aura` must be on
`PATH`, or `AURA_LSP_AURA_PATH` must name it. To build and install the
extension from this checkout, follow
[tools/vscode-aura/INSTALL.md](tools/vscode-aura/INSTALL.md).
