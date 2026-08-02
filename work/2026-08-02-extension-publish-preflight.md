# 2026-08-02 Extension Publish Preflight

## Goal

Publish Aura 0.2.0 to the Visual Studio Marketplace and Open VSX from GitHub
Actions after the standard-capacity hosted-CI sign-off, without moving the
`v0.2.0-preview` tag or republishing its GitHub Release.

## Entry Evidence

- Main CI run `30742009895` is green at
  `cd93f221a117314d57981fd036e165114afe5cb1`.
- The GitHub Release is an existing public prerelease and its VSIX has SHA-256
  `b268ae153dadb93afc987110631bb414d19ac99edcc8840432dd25285b842cb9`.
- Inspecting that released VSIX before dispatch found the obsolete identity
  `aura-lang.vscode-aura`, display name `Aura Language`, and no preview or
  listing metadata. Publishing was not attempted.
- The maintained package on `main` has identity
  `JohnOlafenwa.vscode-aura`, version `0.2.0`, display name
  `Aura Programming Language`, preview status, and the ratified listing
  metadata.

## Correction

An extension-only dispatch with an explicit `source_ref` now runs the tools
job and publishes its freshly built VSIX even though GitHub-release publishing
is skipped. The job still confirms that `release_tag` names an existing GitHub
Release and validates the VSIX publisher, extension ID, and plain Marketplace
version before either registry command runs. Omitting `source_ref` preserves
the existing-release-asset path.

The regression was added first and failed because the tools job ignored
`source_ref` during extension-only dispatch. It now pins the fresh-build path,
the skipped GitHub-release dependency, and the documented dispatch command.

## Verification

- The new workflow regression was red before the correction and is green
  afterward; all 26 release/packaging tests pass.
- `github-actionlint` accepts the corrected Release workflow.
- Extension build/check and all 20 extension tests pass. `vsce package`
  produced a 12-file VSIX with identity `JohnOlafenwa.vscode-aura`, version
  `0.2.0`, `Public Preview`, and SHA-256
  `9fc5941d0345ce7fd97eec67bccede0cc20457fd51e3649e94a62b59f33ca2f2`.
- The docs build, npm audit, Cargo audit under the maintained allowlist, and
  repository hygiene check pass.
- A broad local gate additionally passed the benchmark suites, identity suite,
  31 CLI unit tests, 337 CLI integration tests, all remaining Rust integration
  suites, and all 1,500 compiler-library tests. It was stopped during the
  already-established forced-backend parity matrix at the user's direction to
  prioritize publishing speed; the publish-relevant checks above replaced the
  remaining local stages.

Hosted CI and registry publication remain pending for the corrected commit.

## Dispatch

```bash
gh workflow run release.yml --ref main \
  -f source_ref=main \
  -f release_tag=v0.2.0-preview \
  -f publish=false \
  -f publish_extension=true
```
