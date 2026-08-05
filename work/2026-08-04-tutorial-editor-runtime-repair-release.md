# Aura Tutorial, Editor, And Runtime Repair Release

## Goal

Close every defect found by the 2026-08-04 tutorial/manual/compiler audit,
including the subsequently reported VS Code highlighting and member-completion
failures, then publish the necessary Aura and extension patch releases.

The user authorized implementation, pushes, pull requests, releases, and
marketplace publication without another approval stop. The release identities
are Aura `v0.3.2-preview` and VS Code extension `0.3.3`.

## Scope

- module-constant collection algorithm execution on MIR and direct backends
- module-constant comprehension lowering without panics
- compiler-backed completion through incomplete and diagnostic-bearing buffers
- TextMate builtin-function highlighting
- VS Code language-client activation and disposal lifecycle
- first-class `aura upgrade` installed-toolchain command
- all tutorial contradictions identified by the audit
- executable tutorial-fence integrity coverage
- coordinated compiler, CLI, documentation, GitHub, and marketplace releases

## Work Completed

- Inventoried the 27 tutorial chapters and 336 Aura fences.
- Verified all 107 tutorial-linked `.au` examples type-check.
- Reproduced the module-constant algorithm backend failures and comprehension
  panic on both execution paths.
- Confirmed the precise installed-editor completion failures in the Aura
  Language Server log and independently proved current list-member inference.
- Confirmed the grammar has no builtin-function scope and the extension stores
  a non-disposable `client.start()` result in `context.subscriptions`.
- Recorded the public release baseline: Aura `v0.3.1-preview`, extension
  `0.3.2`, and green latest release/documentation workflows.
- Added both-backend run-pass regressions and repaired module-constant
  collection algorithms and comprehension metadata/lowering.
- Added `aura upgrade`, backed by the published checksum-verifying installer,
  with an offline CLI regression and no `aura update` alias.
- Documented upgrades in the root README, platform install guides, Learn
  track, and CLI reference.
- Repaired compiler-backed member completion when an earlier diagnostic and a
  closing delimiter coexist with a dangling member expression.
- Added maintained builtin-function TextMate scopes and corrected the VS Code
  language-client activation/disposal lifecycle.
- Corrected tutorial claims about indexed ownership, equality obligations,
  constructors and receivers, Result propagation, concurrency errors, network
  resources, byte conversions, numeric casts, operators, and the current CLI.
- Classified all 336 Aura tutorial fences and added an executable tutorial gate:
  211 standalone accepted programs, five pinned expected diagnostics, and 120
  explicitly contextual fragments.
- Restamped the compiler, CLI, LSP, Manual, installer, and release metadata for
  Aura 0.3.2, and the independently versioned extension for 0.3.3.
- Updated the release workflow to validate the VSIX against its source manifest
  version, allowing compiler and extension patch identities to advance
  independently.

## Verification

Baseline checks completed before implementation:

- identity checks: green
- Manual reference integrity: green
- generated LLM documentation: green
- documentation build: green
- compiler-backed LSP bridge tests: 93 green
- tutorial-linked `.au` examples: 107 green

Post-repair focused verification:

- module-constant collection algorithms and comprehensions: MIR/direct green
- compiler analysis unit tests: 111 green
- Aura CLI upgrade tests: green; unsupported `aura update` remains rejected
- language-server tests: 109 green
- VS Code extension tests: 24 green
- release metadata and packaging tests: 45 green
- tutorial fence gate: 336 green
- Manual reference integrity: 271 classified blocks, 132 verified, green
- generated LLM documentation and production documentation build: green
- locally packaged VSIX identity: `JohnOlafenwa.vscode-aura-lang` 0.3.3
- one clean complete `npm run ci`: green
- compiler coverage: 96.31% lines, 97.22% functions, 94.74% regions
- LSP coverage: 100% lines, functions, branches, and statements
- forced MIR/direct fixture parity: green in 908.94 seconds

## Remaining

- run one hosted candidate gate
- merge, tag, publish, and verify Aura `v0.3.2-preview` and extension `0.3.3`

## Follow-Up

None yet. This note will be finalized with commit, gate, hosted-run, release,
marketplace, and checksum evidence.
