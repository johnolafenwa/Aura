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
| `npm run ci` | Runs the whole repository gate: formatting, the main Rust suite at default parallelism, the separately serialized backend-parity gate, Node tests, coverage floors, the docs build, audits, Clippy with warnings as errors, and diff hygiene. |
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

## GitHub Actions

| Workflow | What it does |
| --- | --- |
| `.github/workflows/ci.yml` | Runs the repository gate on Linux and macOS. |
| `.github/workflows/docs.yml` | Builds the VitePress book and deploys it to GitHub Pages from `main`. |
| `.github/workflows/release.yml` | Builds Linux and macOS CLI archives, packages the VS Code extension and docs, and publishes them for pushed `v*` tags. Manual runs only build by default. Publishing needs an explicit opt-in. |
