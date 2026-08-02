# Release Process

This page records the maintained release and extension-publishing procedure.
Publishing is performed only by GitHub Actions; local verification packages
the extension but never sends it to either registry.

## Before Publishing

1. Run the complete local `npm run ci` gate.
2. Push the candidate commit and confirm hosted CI on Linux and macOS.
3. Require hosted CI to be reliably green before enabling or relying on the
   planned on-merge auto-release automation, because that automation gates
   releases on CI success.
4. Inspect current and recent results with `gh run list`, and investigate every
   environment-conditional failure before reporting the release ready.
5. Confirm the release tag resolves to the intended immutable commit.

Timing assertions keep their calibrated local margins and use proportionally
scaled discrimination windows under `GITHUB_ACTIONS`; ordering tests use
explicit handshakes. This keeps the hosted Rust suite parallel without making
shared-runner scheduling noise part of the product contract.

## Extension Secrets

Configure these GitHub Actions repository secrets:

- `VSCE_PAT` publishes `JohnOlafenwa.vscode-aura` to the Visual Studio
  Marketplace.
- `OVSX_TOKEN` publishes the same VSIX to the `JohnOlafenwa` Open VSX
  namespace.

Each registry step emits a visible notice and skips successfully when its
secret is absent. This keeps the release workflow green while secrets are
being configured or rotated.

VSCE_PAT: global PATs are unsupported after 2026-12-01; renew as an org-scoped token (Marketplace -> Manage) and verify with `npx @vscode/vsce verify-pat JohnOlafenwa`.

Renew `OVSX_TOKEN` from the Open VSX account settings, update the GitHub secret,
and confirm that the account is authorized for the `JohnOlafenwa` namespace.
Tokens belong only in GitHub Actions secrets and must never be passed to local
packaging commands or committed files.

## Local Extension Verification

From the repository root:

```bash
npm ci
npm --prefix tools/vscode-aura run build
cd tools/vscode-aura
npx @vscode/vsce ls
npx @vscode/vsce package --out aura-language.vsix --no-dependencies
```

Inspect the resulting VSIX and confirm the identity is
`JohnOlafenwa.vscode-aura` and the Marketplace version is plain `0.2.0`.

## Publish An Existing Release Extension

Once the secrets exist, publish the VSIX for the current preview release
without moving or recreating its tag:

```bash
gh auth login
gh auth status
gh workflow run release.yml --ref main \
  -f source_ref=main \
  -f release_tag=v0.2.0-preview \
  -f publish=false \
  -f publish_extension=true
```

The workflow resolves the immutable implementation source from `source_ref`,
confirms that the GitHub Release already exists, builds a fresh VSIX from
`main`, verifies the packaged publisher and plain version, and then publishes
only to registries whose secrets are configured. The release tag remains the
version identity and is neither moved nor recreated. Omit `source_ref` only
when the VSIX already attached to the GitHub Release is the exact artifact to
publish.

For a new tag, the normal Release workflow publishes the GitHub Release first
and then runs the same extension-publishing job.
