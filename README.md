# Aura

Aura is a compiled, statically typed programming language for reliable
software. It pairs Python-inspired syntax with deterministic ownership,
structured concurrency, typed failure, and native executables. It has no
garbage collector.

```aura
def scale(values: mut list[int64], factor: int64):
    for value in mut values:
        value *= factor

def total(values: list[int64]) -> int64:
    mut sum = 0
    for value in values:
        sum += value
    return sum

mut scores = [10, 20, 30]
scale(scores, 3)

print(f"scores: {scores}")
print(f"total: {total(scores)}")
```

Save this as `scores.au` and run it with `aura run scores.au`. It prints
`scores: [30, 60, 90]` and `total: 180`. The signatures state what each
function does to its argument: `scale` asks for `mut` access, and `total` only
reads. The compiler enforces both.

## Install

Install the Aura 0.3 technical preview on Linux x64, macOS x64, or macOS arm64:

```bash
curl -fsSL https://johnolafenwa.github.io/Aura/install.sh | sh
```

To upgrade the compiler and bundled runtime of an existing install, run:

```bash
aura upgrade
```

Step-by-step guides cover
[macOS](https://johnolafenwa.github.io/Aura/install/macos),
[Linux](https://johnolafenwa.github.io/Aura/install/linux), and
[Windows through WSL 2](https://johnolafenwa.github.io/Aura/install/windows-wsl).
The [VS Code guide](https://johnolafenwa.github.io/Aura/install/vscode) covers
Marketplace, Open VSX, VSIX, and WSL remote installation.
[SUPPORTED_PLATFORMS.md](SUPPORTED_PLATFORMS.md) lists supported hosts and
pinned tools.

## What Aura Is For

Aura 0.3 targets applications for agents, ML infrastructure, and the control
planes around models. Before a program runs, the compiler checks types,
access, mutation, ownership transfer, resource cleanup, and task boundaries.

The long-term goal is a general-purpose systems language for every kind of
software: applications, services, databases, language runtimes, embedded
software, operating systems, and device drivers. Freestanding targets,
low-level memory facilities, hardware interfaces, and the platform controls
that scope needs are not implemented yet. [Why Aura](docs/positioning.md)
explains the project direction.

## Documentation

- [The Aura book](docs/index.md): the guided Learn track plus the normative
  language and API reference.
- [Tutorials](tutorials/README.md) and [examples](examples/README.md): the
  tutorial track and the categorized library of runnable programs.
- [Language Specification](docs/manual/language-specification.md) and
  [complete grammar](docs/manual/grammar.md): the implemented contract, together
  with the rest of the [Manual](docs/manual/index.md).
- [CLI and Tooling](docs/manual/cli-and-tooling.md): every `aura` command,
  backend, and output format.
- [Concurrency](docs/manual/concurrency.md): tasks, queues, the scheduler, and
  its environment settings.
- [Performance](docs/manual/performance.md): current measurements and the
  optimization roadmap.
- [ML systems roadmap](docs/ml_systems_support_plan.md): planned support for ML
  systems work.

## Monorepo layout

| Path | Contents |
| --- | --- |
| `crates/` | Rust compiler, runtime, and CLI |
| `tools/` | Editor integrations and other developer tools |
| `package.json` | npm workspace manifest for the repository's tools |
| `examples/` | Categorized sample Aura programs |
| `tutorials/` | Markdown tutorials for the implemented language |
| `docs/` | VitePress book, language proposal, and supporting documents |
| `architecture_docs/` | Architecture guides and component deep dives for the current implementation |
| `work/` | Task board and implementation notes |

Each component has its own README:

- [crates/aura](crates/aura/README.md): building the compiler and using the
  binary directly.
- [crates/aura-compiler](crates/aura-compiler/README.md): compiler library
  tests.
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
