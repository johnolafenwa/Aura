# Aura landing page and documentation pitch

## Goal

Make Aura's public entry points explain one clear idea: systems programming
should be approachable to Python developers and reliable enough for ML
infrastructure and agent runtimes. The pitch must state that Aura is compiled,
statically typed, ownership-based, and free from garbage collection. The
landing must provide a working curl installer and a concise comparison with
Python and Rust. Performance evidence belongs in the Manual.

## Work Completed

### Landing and positioning

- Replaced the old “Python-shaped agent control plane” hero with
  “Python-like code with Rust-style safety.”
- Added a direct hero description covering native compilation, static typing,
  deterministic ownership, no garbage collector, ML systems, and reliable
  agents.
- Added three focused benefit statements: familiar systems programming,
  compiled reliability, and ML/agent infrastructure.
- Added a seven-row Python/Rust/Aura table covering syntax, type system,
  execution, memory management, failure, concurrency, and primary strength.
- Added plain-language sections for democratizing systems programming and the
  operational work around models and agents.
- Rewrote `docs/positioning.md` and the root README around the same product
  thesis without expanding the implemented feature claims.

### Installation

- Added a responsive install command to the VitePress hero:
  `curl -fsSL https://johnolafenwa.github.io/Aura/install.sh | sh`.
- Added an accessible copy button with visible copied state.
- Added a POSIX `docs/public/install.sh` that detects Linux x64, macOS x64, and
  macOS arm64; downloads the corresponding `v0.2.0-preview` archive and
  `SHA256SUMS`; verifies the archive; and installs the executable, native
  runtime, manifest, examples, license, and packaged documentation under a
  conventional prefix.
- Documented the same route in Downloads, Learn, and the repository README.
- Added regression tests for Dash syntax, required security contracts, host
  target selection, checksum verification, binary execution, and exact native
  runtime placement.

### Performance documentation

- Removed the complete measured snapshot from the landing page and repository
  README.
- Added `docs/manual/performance.md` with the existing protocol, integer-loop,
  and numeric-Array evidence, hardware and source provenance, current task and
  Array gaps, and the performance direction for later releases.
- Added the page to Manual navigation and reference-integrity metadata as a
  structural, non-semantic chapter.

### Voice sweep

- Initial inventory across README, landing, positioning, Downloads, Learn,
  tutorials, extension-facing docs, and examples found two literal English
  contractions and 81 instances of `rather than` or `instead of`.
- Rewrote all 83 reader-facing instances in direct language.
- Final inventory across those surfaces is zero contractions and zero of the
  two comparison constructions.
- Normative Manual text keeps comparison wording when it is required to define
  one semantic result in contrast to another.

## Verification

- Failing-first landing and installer regressions were observed before the
  implementation.
- `python3 -m unittest scripts/test_release_packaging.py`: 32 passed.
- `npm run docs:build`: passed.
- `npm run check:reference`: passed; 38 Manual pages, 261 fences, 126 verified
  fences, 135 illustrative fences, and no missing feature-page sections or
  executable examples.
- `git diff --check -- . ':!personal/file_ops.au'`: passed. The excluded file
  is a pre-existing protected user edit with whitespace findings.
- Browser verification through the built VitePress preview:
  - 1280x720 desktop hero and install command;
  - 390x844 mobile hero and wrapped install command;
  - copy interaction reached the visible `Copied` state;
  - the four-column comparison is contained in a horizontal mobile scroll
    region; and
  - `/manual/performance` renders and appears in Manual navigation.

The normal VitePress warnings for the existing unloaded `aura`/`ebnf`
highlighters and large bundled search chunk remain unchanged.

## Follow-Up

- Publish the source and Pages build when the surrounding release workflow is
  ready. Until then, the new curl URL becomes live only after the updated
  `docs/public/install.sh` reaches GitHub Pages.
- Continue performance work through the measured task, scheduler, direct
  backend, and numeric-Array targets recorded in the Performance chapter.

Protected user files were not edited or staged: `personal/file_ops.au`, the
untracked ADR-0022 draft, and `fc2_direct.out`.
