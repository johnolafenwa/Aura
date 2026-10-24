# Contributing

Aura uses test-first development. Before you change compiler, runtime, CLI, or
language-server behavior, add a failing fixture or a focused regression test.
Update the maintained examples and the manual in the same change.

Run the full local gate with:

```sh
npm ci
npm run ci
```

`rust-toolchain.toml` and `package.json` pin the supported tool versions.

## Guidelines

- Keep each change focused and reviewable.
- Do not commit generated executables, coverage output, editor caches, or
  scratch evaluation corpora.
- Turn useful reproductions into named fixtures under
  `crates/aura-compiler/tests/fixtures/`.
- The manual and its Status and Compatibility page define what Aura
  implements. The language proposal is design history. It does not license an
  undocumented syntax feature.

## Development Commands

Run these from the repository root.

| Command | What it does |
| --- | --- |
| `npm run ci` | Runs the whole repository gate: formatting, the main Rust suite at default parallelism, the separately serialized backend-parity gate, which compares every fixture on the MIR interpreter and on LLVM, Node tests, coverage floors, the docs build, audits, Clippy with warnings as errors, and diff hygiene. |
| `npm run test:rust` | Runs the Rust test suite at Cargo/libtest's default parallelism. It uses a larger test stack so deep parser-limit regressions do not overflow the host test harness. |
| `npm run coverage:compiler` | Measures Rust compiler coverage with `cargo-llvm-cov`. It runs the full Rust workspace tests and reports compiler production files. |
| `npm run coverage:compiler:check` | Enforces the compiler coverage floor. |
| `npm run coverage:lsp:check` | Enforces the language-server coverage floor. |
| `npm run check:format` | Verifies Rust formatting. |
| `npm run check:clippy` | Runs the Rust lint gate with warnings treated as errors. |
| `npm run check:audit` | Runs the npm and RustSec vulnerability gates. |
| `npm run check:hygiene` | Rejects whitespace errors, tracked generated executables, editor metadata, and scratch evaluation corpora. |
| `npm run docs:dev` | Starts the VitePress Aura book locally. |
| `npm run docs:build` | Builds the VitePress Aura book. |

The instrumented compiler-coverage wrapper keeps its own single-threaded test
setting, so coverage collection stays stable.

## LLVM

The native backend generates code with LLVM 22 through inkwell, and every
build of `aura-compiler` links LLVM's static libraries. Install them once
with:

```sh
scripts/install-llvm.sh
export LLVM_SYS_221_PREFIX="$HOME/.cache/aura/llvm/22.1.2"
```

On Linux x86-64, the script downloads the official LLVM 22.1.2 release
archive, checks its SHA-256, and keeps only `llvm-config`, the headers,
and the static libraries, about 360 MB. `llvm-config` also names
`libzstd` and `libxml2`, so on Ubuntu install `libzstd-dev` and
`libxml2-dev`.

On macOS, arm64 or x86-64, the script installs Homebrew's `llvm@22` and
prints the settings to export instead. The official macOS archive's static
libraries hold LLVM bitcode, which Apple's linker cannot read. The script
stops when Homebrew has no `llvm@22` bottle for the Mac, since building
LLVM from source takes hours. It also prints an
`LLVM_SYS_SYSTEM_LIBRARY_DIRS` setting that makes the build link
Homebrew's static `libzstd.a`, so `aura` loads no Homebrew library.

`scripts/check-aura-libraries.py target/release/aura` lists the libraries
an `aura` loads, and fails on one outside the system set. The release
workflow and the native binaries CI job run it.

## GitHub Actions

| Workflow | What it does |
| --- | --- |
| `.github/workflows/ci.yml` | Runs the repository gate on Linux and macOS. |
| `.github/workflows/docs.yml` | Builds the VitePress book and deploys it to GitHub Pages from `main`. |
| `.github/workflows/release.yml` | Builds Linux and macOS CLI archives, packages the VS Code extension and docs, and publishes them for pushed `v*` tags. Manual runs only build by default. Publishing needs an explicit opt-in. |
