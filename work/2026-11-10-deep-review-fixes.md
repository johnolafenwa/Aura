# Deep security, soundness, and functionality review remediation

Date: 2026-11-10. Full-repository static audit and live adversarial testing
across `aura-core`, `aura-runtime`, `aura-compiler`, `aura`,
`aura-language-server`, `vscode-aura`, packaging, docs, and CI scripts,
followed by test-first remediation of all 18 findings (`HIGH-01`..`HIGH-04`,
`MED-01`..`MED-10`, `LOW-01`..`LOW-04`).

## Goal

1. Eliminate verification soundness bypasses, MIR vs. Direct LLVM semantic
   divergences, and recursive `spec def` unfolding blowups (`HIGH-01`,
   `HIGH-02`, `HIGH-04`, `MED-08`).
2. Harden package resolution, Git dependency materialization, FFI dynamic
   library isolation, reference-count header atomics, CLI link flags, private
   run workspaces, HTTPS transport, and directory walks (`HIGH-03`, `MED-03`,
   `MED-04`, `MED-05`, `MED-06`, `LOW-01`, `LOW-02`).
3. Enforce RFC 9110 HTTP token/status validation, TLS stream byte bounds, and
   shared test-lock invariants in `aura-runtime` (`MED-01`, `MED-02`, `MED-09`).
4. Harden editor command quoting, Windows CWD compiler discovery, background
   `aura verify` process lifecycle/offline execution, and hermetic LSP/extension
   test suites (`MED-07`, `MED-10`).
5. Repair repository hygiene, stale workspace lockfiles, `llms.txt` checks,
   `--all-targets` Clippy gating, VitePress `ebnf` highlighting, and
   non-interactive Homebrew LLVM installation (`LOW-03`, `LOW-04`).

## Work completed

### Verifier soundness, `extern "C"` boundary checks, and MIR/Direct parity

- **`HIGH-01` (`verify.rs`, `verify_exec.rs`, `mir.rs`, `sema/contracts.rs`, `lib.rs`)**:
  `verify_program` builds its `VerifyContext` across all loaded modules and
  verifies imported `lemma def` bodies rather than trusting unverified cross-file
  `ensures` clauses. Omitted default arguments check parameter `requires` and
  expression well-definedness at the call site. `extern "C"` `requires` and
  `ensures` clauses lower to runtime `AU4009` checks around every foreign call,
  and `--remove-proved-checks` never elides foreign boundary contracts.
- **`MED-08` (`verify_spec.rs`, `verify_exec.rs`)**: Conditional expressions in
  `spec def` short-circuit when the branch condition simplifies to a concrete
  boolean, and symbolic recursive `spec def` unfolding stops cleanly at depth 8
  instead of overflowing the stack.
- **`HIGH-02` (`mir.rs`, `union_runtime.rs`)**: Runtime nominal class and enum
  identities carry canonical module qualification when disambiguating imported
  types with identical leaf names, and `UnionValue` equality and ordering
  compare active payloads semantically across distinct union type descriptors.
- **`HIGH-04` (`generator.rs`, `mir.rs`, `mir_runtime.rs`, `native_codegen.rs`)**:
  Suspended generators holding active `with`-statement exit obligations run
  their `__exit__` cleanup deterministically on `break`, early `return`, or
  rebinding in the MIR interpreter, matching the Direct LLVM backend and
  propagating traps thrown inside `__exit__`.

### Package resolution, FFI loader isolation, refcount atomics, and CLI hardening

- **`HIGH-03` (`package.rs`, `lib.rs`, `main.rs`)**: Git fetches and clones set
  `core.symlinks=false` and restrict remote protocols via
  `GIT_ALLOW_PROTOCOL=https:ssh` (`https://`, `ssh://`, and `git@host:path`).
  Post-checkout verification inspects both the working tree and `git ls-tree -r`
  for `120000` symlink entries and confines `src` within `manifest_dir`. Global
  `--offline` and `AURA_OFFLINE=1` block network Git queries for uncached remote
  URLs while permitting local path dependencies, local Git repositories, and
  cached revisions.
- **`MED-03` (`package.rs`, `main.rs`)**: Dependency `library_paths` entries
  reject `..` traversal and canonicalize inside the dependency directory.
  `aura build --no-rpath` omits embedding `library_paths` as `-Wl,-rpath`
  entries in built binaries.
- **`MED-04` (`main.rs`)**: Uncached native runs stage compiled binaries inside
  a `0o700` `PrivateRunWorkspace` directory rather than world-readable `/tmp`
  filenames.
- **`MED-05` (`ffi.rs`)**: Foreign libraries load with `RTLD_LOCAL`, cache by
  `(kind, name, search_paths)` and canonical path, resolve symbols strictly from
  declared library handles rather than `RTLD_DEFAULT`, hide host compiler and
  `libLLVM` symbols via `dladdr`, and sort Linux versioned SONAME candidates
  numerically descending.
- **`MED-06` (`native_owned_values.rs`)**: `generated_retain` and
  `generated_release` use 32-bit `AtomicU32` operations matching the LLVM ABI's
  `{ i32 refcount, i32 unique_or_shared }` header layout instead of 64-bit loads
  that clobbered adjacent uniqueness flags.
- **`LOW-01` & `LOW-02` (`main.rs`, `docs/public/install.sh`)**: `aura upgrade`
  and `install.sh` pass `--proto '=https' --tlsv1.2` to `curl` and require
  `https://` for non-loopback `AURA_RELEASE_BASE_URL` overrides.
  `collect_aura_source_paths` uses `entry.file_type()` without following
  directory symlinks and bounds recursion depth.

### Runtime HTTP/TLS bounds and concurrency test guards

- **`MED-01` (`runtime_value.rs`)**: HTTP client requests validate methods
  against RFC 9110 `tchar` tokens, rejecting CR/LF, whitespace, and control
  bytes, and require 3-digit `100..=999` status codes in responses.
- **`MED-02` (`runtime_value.rs`)**: `TlsStreamValue::read_line` and
  `read_exact` enforce `MAX_STREAM_READ_BYTES` before and during buffering, and
  `tls_read_step` caps per-step plaintext drains with `MAX_TLS_READ_STEP_BYTES`.
- **`MED-09` (`native_runtime_tests.rs`)**: All direct-task start unit tests
  acquire `direct_task_claim_flag_test_guard()`, enforced by structural test
  `every_direct_task_start_test_holds_claim_flag_test_guard`.

### Language server, VS Code extension, docs, and CI hygiene

- **`MED-07` & `MED-10` (`tools/aura-language-server`, `tools/vscode-aura`)**:
  `CompilerService` and `verifyDocument` refuse bare unresolved `"aura"`
  commands (preventing Windows CWD binary planting), pass `AURA_OFFLINE=1` by
  default, bound `aura verify` stdout to 16 MiB, abort superseded verification
  subprocesses on edit or close, and skip `untitled:` buffers. VS Code terminal
  tasks prefer structured `vscode.ShellExecution(executable, args)` with
  PowerShell-safe single-quoted literal fallback, and `escapeLldbPath` escapes
  backslashes and quotes in LLDB script imports. LSP and extension tests isolate
  `HOME`, `USERPROFILE`, `PATH`, and `AURA_LSP_SYSTEM_BIN_DIRS` so 100% LSP
  coverage holds on developer machines with globally installed `aura` binaries.
- **`LOW-03` & `LOW-04` (`.gitignore`, `package.json`, `docs.yml`, `config.mts`, `install-llvm.sh`, `Aura.lock`, `docs/`)**:
  Deduplicated `.DS_Store` and ignored `*.swp`, `*.swo`, `*~`; added
  `--all-targets` to `check:clippy` and removed two unused `unsafe` blocks in
  `native_enum_layouts_tests.rs`; fixed `test_release_metadata.py` and
  `test_generate_llms.py` and wired `test:release-metadata` and
  `docs:llms:check` into `npm run ci` and `docs.yml`; aliased `ebnf` in
  VitePress; made `install-llvm.sh` non-interactive on macOS; refreshed
  `examples/packages/workspace/Aura.lock`; updated manual chapters and
  regenerated `docs/public/llms.txt` and `docs/public/llms-full.txt`.

## Verification

- `npm run check:format`
- `npm run check:clippy` (`--all-targets`)
- `npm run check:hygiene`
- `npm run check:audit`
- `RUST_MIN_STACK=33554432 PATH="$HOME/.cache/aura/z3/5.3.3.0/solver:$PATH" npm run test:rust`
- `PATH="$HOME/.cache/aura/z3/5.3.3.0/solver:$PATH" npm run test:backend-parity`
- `npm run check:cross-language` (30/30) and `npm run check:operation-probes` (24/24)
- `python3 -m unittest discover -s scripts -p 'test_*.py'` and `npm run test:rust-baselines`
- `npm run coverage:lsp:check` (100% statements, branches, functions, and lines)
- `npm run check:extension && npm run test:extension && npm run package:extension`
- `npm run check:doc-examples` (892/892)
- `npm run test:docs-site`
- `npm run docs:llms:check`
- `npm run docs:build` (zero `ebnf` warnings)
- Pending when resumed: `PATH="$HOME/.cache/aura/z3/5.3.3.0/solver:$PATH" npm run coverage:compiler:check`

## Follow-up

- Run `npm run coverage:compiler:check` when verification resumes.
- Cryptographic signature or GitHub Artifact Attestation verification over
  release `SHA256SUMS` remains a separate release-pipeline enhancement beyond
  transport-level HTTPS enforcement.
