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
- An extension-only dispatch may reuse the VSIX attached to an existing GitHub
  Release, or provide `source_ref` to build a fresh VSIX from an immutable
  source commit. Both paths keep the release tag fixed and do not republish the
  GitHub Release.
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

Policy: preserve calibrated local timing margins and scale the complete
wall-clock upper-bound family by four under `GITHUB_ACTIONS`. Each deliberately
slow comparison operation is scaled with its upper limit, retaining the gap
that catches blocked scheduler, reactor, DNS, and safepoint behavior. The
bounded-queue test no longer estimates ordering from sleeps: the Aura consumer
waits for an explicit host release file, so the host observes the second put
blocked before allowing the first receive. Ordinary timeouts used only as
deadlock guards remain unchanged, and Rust tests stay parallel on both hosted
systems.

This replaces two policies disproved by hosted evidence. Per-binary guards
could not isolate a measurement from unrelated tests, and run `30717422681`
still failed four macOS cases with the complete Rust suite restricted to one
test thread. The revised hosted path passes the full 1,501-test compiler
library at `--test-threads=64`, the four previously failing compiler cases,
the deterministic queue test, and both CLI safepoint probes under the same
parallel load.

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
- macOS still failed timing assertions because a per-binary guard could not
  isolate them from ordinary suite and host load. The next corrective run also
  disproved whole-suite single-thread execution, so the final policy uses
  proportionally widened hosted discrimination windows and explicit ordering
  handshakes while keeping both hosted systems parallel.

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

Corrective branch run
[30718486470](https://github.com/johnolafenwa/Aura/actions/runs/30718486470)
then exposed a separate Linux benchmark-monitor race before reaching Rust. A
naturally completed child can remain in `/proc` as a zombie until reaped, and
Linux omits `VmRSS` from a zombie's status record. The monitor now recognizes
the `Z` state as natural completion, while malformed RSS from a live process
remains an error. The pure zombie-record regression and the original
short-lived sleepers execution test both pass; the complete harness has 56
green tests.

Three standard-capacity proof runs were started from `b67c142`. Run
[30719290315](https://github.com/johnolafenwa/Aura/actions/runs/30719290315)
proved the revised macOS timing policy under hosted load: all 337 CLI tests
passed, including the two safepoint probes and the deterministic bounded-queue
case. It then exposed an independent test-isolation defect in
`ffi_acceptance.rs`. Every temporary FFI package used only the process ID and
wall-clock nanoseconds for its directory name. Parallel tests that observed the
same macOS clock tick therefore shared `src/main.au`; the test-only program
could overwrite the runnable program and produce AU4001 (`no main function`).

Temporary FFI packages now include a process-local atomic sequence and claim
their root with exclusive `create_dir`, retrying a stale-name collision. The
regression injects an identical timestamp into two packages and verifies that
their paths and source contents remain isolated. The focused five-test FFI
acceptance binary is green with 16 test threads. This finding does not require
larger runners: the standard-capacity timing family had already passed before
the isolation failure.

The first exact local gate on this correction passed every behavioral stage,
including 337 CLI tests, 1,501 compiler tests, the 1,064-second forced parity
matrix, 101 LSP tests, and 20 extension tests. Instrumented tests were also all
green, but the report stopped on the frozen ratchet at 96.28% lines, 97.14%
functions, and 94.61% regions. The new native flag configuration used three
separate `map_err` closures around hard-coded, valid Cranelift settings; their
dependency-invariant error paths cannot be reached behaviorally and were
counted once per crate artifact. The code now routes all three settings through
one exercised helper with one preserved defensive error arm. No synthetic
coverage test was added.

The behavior-focused coverage closure then removed two genuinely unreachable
defensive shapes instead of manufacturing line-execution tests. Direct trait
dispatch already resolves the exact concrete type before code generation; the
class-name-only fallback could not be reached by valid lowered MIR and was
removed while the observable specific-implementation and ordinary-dispatch
tests stayed green. Package timeout cleanup was folded into its caller, and
the child process group is now configured with `CommandExt::process_group(0)`
instead of a child-side `pre_exec` closure that some linked artifacts could
never execute. The ten-second descendant-kill regression still completes in
about 60 ms. The exact instrumented replay is green with 337 CLI tests, five
FFI acceptance tests, 1,500 compiler-library tests, and final coverage of
96.29% lines, 97.21% functions, and 94.62% regions. No synthetic coverage test
or coverage exclusion was added.

The final exact `npm run ci` replay is green on the corrective tree: all 337
CLI tests, five FFI acceptance tests, 1,500 compiler-library tests, the complete
forced MIR/direct matrix (752.92 seconds), 101 LSP tests at 100% coverage, 20
extension tests, the 96.29% / 97.21% / 94.62% compiler coverage ratchet,
reference integrity, docs, audits, warning-denied Clippy, and hygiene passed.
The repository remained on standard local and hosted capacity throughout this
proof.

The first three-run streak from `f795eab` was invalidated immediately by proof
run [30727321064](https://github.com/johnolafenwa/Aura/actions/runs/30727321064).
Its macOS job failed before Rust in
`test_idle_waits_for_exact_natural_completion`: the shell fixture advertised
`READY`, slept only 50 ms, and exited while `run_idle` was still collecting its
initial process-statistics sample. A contended hosted `ps` call could therefore
consume the fixture's entire lifetime before the asserted 10 ms stability
window began. This is a test-fixture margin, not an Aura runtime or benchmark
failure. The fixture now remains alive for one second while preserving the
same 10 ms asserted behavior. Twenty consecutive focused runs under
`GITHUB_ACTIONS=true` and the complete 56-test harness are green locally. The
other five jobs from the invalidated streak remain useful fresh-eyes evidence
but cannot count toward the required three-run streak.

Those five jobs exposed two further test-infrastructure constraints. Four
otherwise-progressing standard-runner jobs reached the workflow's 45-minute
wall-clock cap during the complete gate, so CI now has a 90-minute job budget;
a workflow regression pins the budget while Rust tests remain parallel. Proof
run [30727320065](https://github.com/johnolafenwa/Aura/actions/runs/30727320065)
also exposed a Linux package-test environment race. Multiple package tests
mutated process-global `XDG_CACHE_HOME`, `HOME`, or `AURA_GIT_TIMEOUT_MS`
concurrently, allowing the wrong-name resolver test to inspect a sibling's
cache and receive the wrong earlier diagnostic. The complete environment-
mutating package-test family now shares one documented lock. Product package
resolution is unchanged. All 16 package tests passed together 100 consecutive
times at 64 test threads under `GITHUB_ACTIONS=true`; the 22-test workflow and
packaging suite plus `github-actionlint` also pass with the 90-minute budget.

The next exact-SHA streak crossed the retired 45-minute limit on all six
standard-runner jobs. Proof-2 macOS run
[30730386579](https://github.com/johnolafenwa/Aura/actions/runs/30730386579)
then reached reference integrity after all Rust, parity, extension, and
coverage gates had passed and failed with exit 127: the current `macos-15`
image does not provide `rg`, although the reference checker requires it.
This is a missing workflow prerequisite, not a capacity or product failure.
CI now installs pinned `ripgrep@14.1.1` on every matrix OS through the existing
cross-platform installer. A workflow regression pins the prerequisite and
`github-actionlint` accepts the corrected workflow.

Primary macOS observation job `91449588741` then exposed one final test-only
timing assumption under single-threaded LLVM coverage. The Phase-5.8 select
registration-race test expected a one-millisecond deadline to beat a task that
slept for 20 milliseconds. Instrumented setup could consume the entire sleep,
making the task legitimately ready before selection and yielding the
source-ordered task outcome. The deadline and cancellation cases now hold
their losing tasks behind explicit release channels until after selection;
the registration-race assertions are unchanged and no wall-clock ordering is
involved. The focused test passes 100/100 hosted-mode repetitions and the
exact instrumented `cargo llvm-cov` invocation.

Proof-3 Ubuntu job `91449593795` completed all 1,501 instrumented library tests
and then found an ELF-specific integration-test link omission. The
`native_runtime_ffi` coverage binary defines no-mangle C helpers and exercises
the product adapter's real name-based `RTLD_DEFAULT` lookup. Keeping a function
address alive was sufficient on macOS but did not place the helpers in a Linux
executable's dynamic symbol table, so lookup reported an undefined symbol. The
compiler crate now passes `-Wl,--export-dynamic` only to Linux test targets via
its build script. The maintained FFI integration test remains unchanged and a
cross-platform regression pins the Linux linker contract; all seven
instrumented FFI tests pass locally. This is test-binary linkage, not a change
to generated Aura programs or the runtime adapter.

The first exact-SHA proof after those fixes completed every substantive macOS
gate, including reference integrity, docs, audits, and Clippy, then failed only
at `check:hygiene`. The hosted checkout had the default one-commit shallow
history, so `git show --check HEAD` treated `HEAD` as a root commit and scanned
legacy whitespace across the full tracked tree; the same command locally saw
the parent and checked only the new commit. CI now fetches depth two. A
workflow regression pins that parent-history prerequisite, the complete
25-test packaging suite is green, and `github-actionlint` accepts the workflow.
No legacy examples or user-owned personal files were reformatted to hide the
checkout defect.

The same proof's Ubuntu job `91459871235` ran every pre-coverage stage green
and was still progressing normally through the instrumented compiler library
when the initial 90-minute allowance canceled it. No test failed. The complete
cold gate therefore has a 120-minute hosted job budget on the unchanged
standard runner class. This changes only the workflow wall-clock ceiling;
Rust remains parallel and every per-test timeout, assertion, coverage floor,
parity requirement, and reference gate is unchanged. The workflow regression
now pins 120 minutes.
