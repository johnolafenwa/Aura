# Hosted CI hotfix and voice cleanup

## Goal

Close the environment-conditional failures exposed by hosted GitHub Actions,
harden release packaging against terminal control data, make the archive smoke
harness portable to POSIX `sh`, update the hosted workflows, and remove
defensive measurement disclaimers from maintained user-facing prose. The fixed
tree remains local until the user chooses to push it; no release is published
by this task.

## Hosted evidence and process correction

Every CI push run from 29 July 2026 through the preview tag push failed even
though the corresponding local gates were reported green:

- [30460142030](https://github.com/johnolafenwa/Aura/actions/runs/30460142030)
  at `4c9d9a2f1251` on 29 July;
- [30565090912](https://github.com/johnolafenwa/Aura/actions/runs/30565090912)
  at `8131ebe58ec0` on 30 July;
- [30671738746](https://github.com/johnolafenwa/Aura/actions/runs/30671738746)
  at `9c0d5abeeb8d` on 31 July;
- [30701396806](https://github.com/johnolafenwa/Aura/actions/runs/30701396806)
  for the main push at `5d181e1704b6` on 1 August;
- [30701406705](https://github.com/johnolafenwa/Aura/actions/runs/30701406705)
  for the preview-tag push at the same commit.

The macOS matrix consistently inherited `CARGO_TERM_COLOR=always` from Rust
setup and captured ANSI reset bytes in native linker arguments. The latest
macOS job reached 80 passing and 256 failing CLI tests before reporting
`library 'm\x1b[0m' not found`. Ubuntu exposed the non-POSIX smoke executable
from 31 July onward. The preview
[Release run](https://github.com/johnolafenwa/Aura/actions/runs/30701406723)
built the tools and docs but all three CLI archive smoke jobs failed; its
publish job was skipped. The GitHub Releases API reported no release.

The Docs builds succeeded. Their deploy jobs separately returned HTTP 404,
and the old `actions/deploy-pages` pin emitted a Node 20 deprecation warning.
The repository's current Pages API state reports `build_type: workflow`; the
next pushed commit must recheck deployment rather than treating the historical
404 as resolved locally.

Hosted results are now part of every batch's definition of done. After a
commit is pushed, completion requires this audit sequence:

```sh
sha=$(git rev-parse HEAD)
gh run list --commit "$sha" --event push --limit 20 \
  --json databaseId,workflowName,headBranch,status,conclusion,url
gh run watch <run-id> --exit-status
gh run list --commit "$sha" --event push --limit 20 \
  --json databaseId,workflowName,headBranch,status,conclusion,url \
  --jq '.[] | [.databaseId,.workflowName,.headBranch,.status,.conclusion,.url] | @tsv'
```

Failures are inspected with `gh run view <run-id> --log-failed`. This hotfix
explicitly stops before pushing the replacement commit and tag, so its hosted
verification remains pending and must not be reported green from local
evidence alone.

## H1: native link arguments

- Aura's runtime build/artifact-discovery and native-link capture Cargo
  subprocesses now force `CARGO_TERM_COLOR=never`.
- Release packaging's independent `cargo rustc` capture also forces terminal
  color off.
- The Rust capture path and packaging writer strip only well-formed ANSI
  control sequences. Malformed sequences remain visible to validation.
- Every remaining control-bearing token is rejected with a hard error that
  safely names the token. Installed `native-link-args.json` data receives the
  same validation before use, and packaging writes no manifest before
  validation succeeds.
- The regressions run the real capture path under inherited
  `CARGO_TERM_COLOR=always`, reproduce the hosted `-lc<ESC>[0m` shape, verify
  clean JSON, and pin malformed/control-bearing rejection.

## H2: POSIX archive smoke harness

The hosted diagnosis required one correction. Release archives contain a
native ELF or Mach-O `bin/aura`; the product does not ship a shell wrapper.
The failing file was the fake executable created by
`test_archive_smoke_uses_copied_sources_without_cargo`. An embedded unescaped
newline prevented `textwrap.dedent` from putting its intended Bash shebang at
byte zero, so Linux fell back to `/bin/sh` and Dash rejected `pipefail` while
macOS concealed the defect.

The test double is now genuine POSIX `sh`: exact `#!/bin/sh`, `set -eu`, POSIX
`test`/`case`/argument iteration, and no Bash-only syntax. The regression
asserts the shebang, rejects `pipefail`, runs `dash -n` when Dash is available,
and retains the complete outside-checkout smoke behavior.

## H3: workflow hardening

- `.github/workflows/ci.yml` and `.github/workflows/release.yml` set
  workflow-level `CARGO_TERM_COLOR: never` as defense in depth.
- `.github/workflows/docs.yml` pins the official Node 24
  `actions/deploy-pages` v5.0.0 commit.
- Release-packaging tests pin both workflow contracts.

## H5: plain factual measurement voice

The maintained README, docs, tutorials, CHANGELOG, and benchmark pages now
state measurements and supported scope directly. Numbers, hardware, dates,
commits, hashes, methodology links, and factual comparison boundaries remain.
Historical work notes and ADR bodies were not edited.

| Surface | Disclaimer sentences removed or rewritten |
| --- | ---: |
| README | 2 |
| Docs | 36 |
| Tutorials | 10 |
| Examples | 0 |
| CHANGELOG | 4 |
| Benchmark pages | 7 |
| Release-notes text | 0 |
| `llms` / marketplace copy | 0 |
| **Total** | **59** |

The disclaimer qualifier was also removed from the README measurement heading;
it is not included in the sentence count. File-level docs counts are: index 2,
positioning 11, Learn concurrency 5, Manual concurrency 5, current limits 6,
execution model 2, numeric arrays 4, and status/compatibility 1. Tutorial 13
and Tutorial 14 contribute five each. Benchmark counts are direct integer
loops 1, numeric arrays 3, release performance 1, and scalable runtime 2.

Three representative rewrites:

1. Before: “These are exact-workload observations, not portable speed
   promises.” After: the paragraph begins directly with the positioning and
   methodology link that records the hardware, reboot, commit, evidence
   hashes, and workload details.
2. Before: the 100,000-sleeper result was retained “without becoming a product
   claim” and followed by a denial of portable guarantees. After: the text
   states the maintained 10,000-sleeper bound, the three exact RSS peaks, the
   passing controls, and the four-worker measurements on the named Mac14,9
   host.
3. Before: the numeric-Array results were followed by denials of a portable
   guarantee, general NumPy comparison, compatibility, and vectorization
   claim. After: the text states that release disassembly showed scalar
   floating-point kernels and that the table covers the named operations while
   Aura's Array API is narrower than NumPy's.

`test_public_measurements_use_plain_factual_voice` prevents the deleted voice
patterns from returning. Reference wording pins moved with the prose;
executable-fence hashes did not change.

## Verification

Focused verification is green:

- Aura binary unit tests, including the inherited-color capture regression;
- release-packaging, POSIX/Dash smoke, and workflow-hardening tests;
- Aura identity and public-voice tests;
- reference integrity over 37 pages, 260 fences, and 126 verified blocks;
- VitePress docs build, Rust formatting, Python compilation, workflow lint,
  and scoped diff checks.

The exact full local `npm run ci` gate is green:

- 54 scalable-runtime benchmark tests, 10 numeric-Array benchmark tests, 23
  release-performance tests, 17 release-packaging tests, and six identity and
  public-voice tests;
- 31 Aura binary unit tests, 336 CLI/runtime tests, and 1,499 compiler-library
  tests, plus the complete forced MIR/direct backend parity matrix;
- 101 LSP tests and 19 extension tests;
- compiler coverage at 96.28% lines, 97.21% functions, and 94.62% regions,
  with LSP statements, branches, functions, and lines all at 100%;
- reference integrity over 37 pages, 260 fences, and 126 verified blocks, plus
  all 683 package manifests and maintained-source migration checks;
- the VitePress build, both dependency audits, warning-denied Clippy, Rust
  formatting, extension build checks, and repository hygiene.

Hosted verification remains pending until the user pushes the fixed commit.

## Tag and release disposition

The remote `v0.2.0-preview` tag currently identifies `5d181e1704b6`; no release
was published. After the full local gate and commit, the authorized sequence is
to delete that stale remote tag, replace the local annotated tag at the fixed
commit, rebuild and verify all local archives and `SHA256SUMS`, then stop
without pushing the replacement commit or tag. A future replacement-tag push
will invoke the release workflow and can publish the prerelease, so the branch
commit and its hosted CI/Docs runs must be green before that tag is pushed.

Protected user state remains untouched: `personal/file_ops.au`, the untracked
ADR-0022 draft, and the untracked empty `fc2_direct.out` discovered at entry.
