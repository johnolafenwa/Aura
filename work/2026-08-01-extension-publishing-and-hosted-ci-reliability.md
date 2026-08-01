# 2026-08-01 Extension Publishing And Hosted-CI Reliability

## Goal

Prepare `JohnOlafenwa.vscode-aura` for the Visual Studio Marketplace and Open
VSX, add secret-safe CI-only publishing plus an existing-release dispatch path,
and close the two environment-conditional hosted CI failure classes at
`fd6fec1e0376d06873491253cf03861382041824`.

## Entry Evidence

- Main-branch CI run
  [30709107997](https://github.com/johnolafenwa/Aura/actions/runs/30709107997)
  failed on Linux and macOS.
- Preview-tag CI run
  [30709113123](https://github.com/johnolafenwa/Aura/actions/runs/30709113123)
  failed independently on Linux and macOS.
- Ubuntu failed two different members of the
  `private_native_diagnostic_channel_*` family across the two runs. Both logs
  contained `/bin/sh: eval: Syntax error: Bad fd number`.
- The macOS failures were independent calibrated timing assertions:
  `loop_backedge_safepoints_prevent_socket_readiness_starvation`, both
  `runtime_scheduler_wakes_*_on_cancellation` tests,
  `tcp_connect_timeout_offloads_resolution_without_a_lightweight_task_context`,
  and `bounded_queue_blocks_second_put_until_capacity_frees`.

## E1-E4: Extension Distribution

- Publisher identity is `JohnOlafenwa`; the display name is **Aura Programming
  Language** and the package is marked as a preview.
- The manifest includes the requested category and five keywords, public
  repository/bugs/homepage links, a packaged MIT license, and a Marketplace
  description that distinguishes Aura from unrelated products.
- The Marketplace icon is a 256 px PNG rendered from
  `docs/public/aura-mark.svg`. The deployed
  `https://johnolafenwa.github.io/Aura/aura-mark.svg` and repository source were
  verified byte-identical at SHA-256
  `13689d79af01d42c3ff6213009a9fac97d5acd836d31dc9553abe85144dfc912`.
  An initial generated icon candidate was discarded after the existing website
  mark was selected; it is not present in the tree or VSIX.
- The extension README now stands alone as a registry listing, opens with the
  language description and repository link, and explains that editor semantics
  require the actual compiler-owned `aura lsp` server.
- `vsce ls --no-dependencies --tree` reports only the 11 intended extension
  source files. `vsce package` produced a 12-entry VSIX whose manifest pins
  `Publisher="JohnOlafenwa"`, `Id="vscode-aura"`, `Version="0.2.0"`,
  `GalleryFlags="Public Preview"`, license, and PNG icon.
- The Release workflow validates the same identity and plain `x.y.z` version,
  publishes through `VSCE_PAT` and `OVSX_TOKEN`, and emits a visible successful
  skip for each absent secret.
- A dispatch with `release_tag` and `publish_extension=true` downloads the VSIX
  from the existing GitHub Release. It skips CLI/docs rebuilds, does not move
  the tag, and does not republish the GitHub Release.
- `docs/downloads.md` links both registries and the GitHub VSIX fallback.
  `docs/release-process.md` records local verification, CI-only publishing,
  token renewal, the exact dispatch command, and the hosted-green precondition
  for future on-merge releases.

## F1: Linux Diagnostic-Channel Family

Disposition: **test-only portability defect**.

The product creates inherited diagnostic and intent pipes, clears `CLOEXEC`,
and writes through their numeric descriptors directly from the native runtime.
The failing unit helper instead constructed `/bin/sh` scripts with
`eval "... >&$FD"`. Ubuntu's `/bin/sh` is Dash, whose numeric redirection
syntax rejects descriptors above 9. Parallel tests made those arbitrary pipe
descriptors large enough to expose the helper defect; no process-global Aura
state or product framing race was involved.

The regression now holds 16 `/dev/null` files before creating the product pipes,
forcing the tested channel above Dash's limit. Every helper write uses
`/dev/fd/$FD`, including signal-only, data-only, malformed, multiple, and
oversized cases.

Direct Linux proof:

- old FD-10 command: status 2 with `Syntax error: Bad fd number`
- `/dev/fd/10` command: status 0 and exact byte delivered
- repaired Rust family: 100/100 consecutive green loops on Linux arm64 with
  `--test-threads=64` and the deliberate high-descriptor guard

The temporary Docker image, Cargo registry volume, and Linux target volume were
removed after the proof.

## F2: macOS Timing Assertions

Policy: preserve the calibrated timing margins and serialize the complete
wall-clock assertion family. A shared compiler-test lock covers every test that
asserts `Instant::elapsed()`, plus the bounded-queue ordering and DNS sibling
progress probes. A matching CLI lock covers both safepoint latency probes.
Hosted macOS additionally runs the complete Rust test surface with one test
thread so unrelated suite work cannot load those measurements.
Timeouts used only as deadlock guards remain parallel.

This applies one criterion to the family instead of naming only the four tests
that happened to fail. The compiler library completed all 1,499 tests at
`--test-threads=64`; the guarded cancellation, DNS, queue, socket, TLS, reactor,
and protocol timing tests were green. Both CLI safepoint probes were green at
`--test-threads=64`.

The policy and the requirement to inspect hosted results with `gh run list` are
recorded in `docs/testing_strategy.md` and `docs/release-process.md`.

## Verification

Green locally:

- 20 extension tests
- 16 release-workflow and release-metadata tests
- `github-actionlint` for `release.yml`
- Rust formatting and focused diff checks
- focused F1 test family on macOS and the 100-run Linux high-thread loop
- focused F2 cancellation, DNS, bounded-queue, and both-backend safepoint tests
- all 1,499 compiler-library tests with 64 test threads
- `vsce ls --no-dependencies --tree`
- `vsce package --out aura-language.vsix --no-dependencies`
- exact full `npm run ci`:
  - 336 CLI/runtime tests
  - 1,499 compiler-library tests
  - complete forced MIR/direct fixture parity matrix
  - 101 LSP tests and 100% statement/branch/function/line coverage
  - 20 extension tests
  - compiler coverage at 96.28% lines, 97.21% functions, and 94.62%
    regions
  - reference integrity over 37 pages, 260 fenced blocks, and 126 verified
    blocks
  - all 683 package manifests migrated
  - docs build, npm and Rust audits, warning-denied Clippy, and hygiene
- repository secret names `VSCE_PAT` and `OVSX_TOKEN` are both configured;
  values were neither read nor printed

Pending before completion:

- complete the corrective branch's local gate
- three consecutive corrective-tree hosted CI runs green on both Linux and macOS
- land the proven correction on main

## Follow-Up

After the hosted proof, the user can run the documented extension-only
dispatch. Both required repository secrets are already configured. No local
publishing is authorized.

## First Hosted Attempt And Corrective Branch

CI run [30716430428](https://github.com/johnolafenwa/Aura/actions/runs/30716430428)
tested `24e048c2ef8c3af75b8de628485c0999aed3c354` on both hosted systems. It
confirmed the Linux diagnostic-channel regression and the named macOS timing
regressions were active in the real gate, then exposed two remaining
environment-dependent facts:

- Ubuntu x86-64 rejected direct functions whose mutable receiver writeback
  flattened to three integer results. Cranelift's System V ABI has only two
  integer return registers and reported that the overflow needed a structure
  return area. Aura's generated-call ABI is private within one object, so the
  native backend now enables Cranelift's implicit stack-return area for every
  caller and callee. The setting has a failing-first unit pin, and an x86-64
  object-emission regression reproduces the exact `bool` plus two-field
  receiver-writeback shape. The two observable CLI cases from the hosted log
  remain the end-to-end pins.
- macOS still failed three timing assertions because the per-binary guard could
  not isolate them from ordinary tests running concurrently in the same Rust
  suite. Hosted macOS now sets `RUST_TEST_THREADS=1` before `npm run ci`, while
  Linux remains parallel. A release-workflow regression pins that conditional
  policy.

The user authorized an isolated branch for faster hosted proof. The correction
is being validated on `codex/hosted-ci-x64-writeback` before main moves again.

Corrective branch run
[30717422681](https://github.com/johnolafenwa/Aura/actions/runs/30717422681)
then proved the x86-64 regression itself green. Ubuntu progressed through all
1,501 compiler tests except
`package::tests::command_timeout_terminates_hung_git_helpers`. That failure was
a product defect rather than a timing-margin flake: timeout cleanup killed the
direct `sh`, but its `sleep` descendant retained the captured stdout/stderr
pipes, so joining the reader threads waited for the helper to exit naturally.

Timed package commands now enter a fresh Unix process group before exec.
Timeout and wait-error cleanup signal the entire group, retain a direct-child
kill fallback, reap the child, and only then join its output readers. The
regression now uses a ten-second helper with a five-second anti-wait ceiling;
the repaired path completes in roughly 60 ms and leaves no descendant process.
