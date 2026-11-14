# Release Process

This page is the procedure for releasing Aura and publishing the extension.
Only GitHub Actions publishes. Local verification packages the extension but
never sends it to either registry.

## Before Publishing

1. Run the focused release-metadata, packaging, documentation, and extension
   checks locally.
2. Push the candidate commit. Complete one full hosted continuous integration
   (CI) run on Linux and macOS for the exact release commit.
3. Hosted CI must be reliably green before you enable or rely on the planned
   on-merge auto-release automation. That automation gates releases on CI
   success.
4. Inspect current and recent results with `gh run list`. Investigate every
   environment-conditional failure before you report the release ready.
5. Confirm that the release tag resolves to the intended immutable commit.

Timing assertions keep their calibrated local margins. Under
`GITHUB_ACTIONS`, they use proportionally scaled discrimination windows.
Ordering tests use explicit handshakes. This keeps the hosted Rust suite
parallel, and it keeps shared-runner scheduling noise out of the product
contract.

## Extension Secrets

Configure these GitHub Actions repository secrets:

- `VSCE_PAT` publishes `JohnOlafenwa.vscode-aura-lang` to the Visual Studio
  Marketplace.
- `OVSX_TOKEN` publishes the same VSIX to the `JohnOlafenwa` Open VSX
  namespace.

If a secret is absent, its registry step prints a visible notice and skips
successfully. The release workflow stays green while you configure or rotate
secrets.

`VSCE_PAT`: global personal access tokens (PATs) are unsupported after
2026-12-01. Renew it as an org-scoped token from Marketplace -> Manage. Then
verify it with `npx @vscode/vsce verify-pat JohnOlafenwa`.

Renew `OVSX_TOKEN` from the Open VSX account settings and update the GitHub
secret. Confirm that the account is authorized for the `JohnOlafenwa`
namespace.

Tokens belong only in GitHub Actions secrets. Never pass them to local
packaging commands or commit them to files.

## Package Managers

Each release builds a Debian package for x86-64 and arm64 and a Chocolatey
package. The Release workflow installs each one on its runner before it
uploads it, and attaches them to the GitHub Release. Three jobs then
publish to the package managers. `packaging/README.md` describes the
files.

| Job | Publishes | Needs |
| --- | --- | --- |
| `publish-homebrew` | `Formula/aura.rb` to the tap repository `johnolafenwa/homebrew-aura` | The tap repository, and the secret `HOMEBREW_TAP_TOKEN` |
| `publish-chocolatey` | `aura.<version>.nupkg` to the Chocolatey community repository | The secret `CHOCOLATEY_API_KEY` |
| `publish-apt` | Starts the Docs workflow, which builds the apt repository into the site at `/Aura/apt` | The secret `APT_SIGNING_KEY` |

If a secret is absent, its job prints a notice and succeeds, as the
extension registries do. Set each one up once:

- **Homebrew.** Create the public repository `johnolafenwa/homebrew-aura`
  with a README. Create a fine-grained personal access token limited to
  that repository with read and write access to its contents, and store
  it as `HOMEBREW_TAP_TOKEN`.
- **Chocolatey.** Create an account at `community.chocolatey.org`, copy
  the API key from the account page, and store it as
  `CHOCOLATEY_API_KEY`. The first version of a package waits for a
  moderator, so `choco install aura --pre` can lag the release by days.
  Later versions pass the automated checks first and are usually listed
  sooner.
- **apt.** Create a signing key with no passphrase and store the private
  half as `APT_SIGNING_KEY`:

  ```bash
  export GNUPGHOME="$(mktemp -d)"
  gpg --batch --passphrase '' --quick-generate-key \
    'Aura apt repository <35049820+johnolafenwa@users.noreply.github.com>' ed25519 sign never
  gpg --armor --export-secret-keys | gh secret set APT_SIGNING_KEY --repo johnolafenwa/Aura
  ```

  Keep a copy of the private key somewhere safe before you delete
  `$GNUPGHOME`. Users trust the public half, so a new key means every
  user must download the keyring again.

The apt repository holds the Debian packages of the newest release that
has them. Every deployment of the site rebuilds it from the release
assets, after checking them against the release's `SHA256SUMS`. To
rebuild it by hand, run `gh workflow run docs.yml --ref main`.

To publish packages for a release whose jobs skipped, add the secret and
re-run the skipped jobs of that release's workflow run.

## Local Extension Verification

From the repository root:

```bash
npm ci
npm --prefix tools/vscode-aura run build
cd tools/vscode-aura
npx @vscode/vsce ls
npx @vscode/vsce package --out aura-language.vsix --no-dependencies
```

Inspect the resulting VSIX. Confirm that the identity is
`JohnOlafenwa.vscode-aura-lang` and the Marketplace version is plain `0.15.0`.

## Publish An Extension-Only Patch

This path builds and publishes an intentionally newer extension from an
explicit source commit. It does not rebuild or replace the Aura CLI release.

```bash
gh auth login
gh auth status
gh workflow run release.yml --ref main \
  -f source_ref=main \
  -f release_tag=v0.15.0 \
  -f publish=false \
  -f publish_extension=true
```

With an explicit `source_ref`, the workflow builds the VSIX from that source.
It reads the expected plain Marketplace version from that source's
`tools/vscode-aura/package.json`. The CLI release tag and the extension
version can advance independently.

When the VSIX is built in the same run, `release_tag` does not create or
require a GitHub Release. The CLI build and GitHub Release publication jobs
stay skipped.

## Publish An Existing Release Extension

Once the secrets exist, you can publish the VSIX for the current preview
release without moving or recreating its tag:

```bash
gh auth login
gh auth status
gh workflow run release.yml --ref main \
  -f release_tag=v0.15.0-preview \
  -f publish=false \
  -f publish_extension=true
```

Omit `source_ref` to make the workflow download the exact VSIX attached to the
release. The workflow confirms that the GitHub Release exists and verifies the
packaged publisher and plain version. It then publishes only to registries
whose secrets are configured. The release tag remains the immutable version
identity.

An explicit `source_ref` selects the extension-only patch path above. Use it
only when the rebuilt source is intentionally the artifact you publish.

For a new tag, the normal Release workflow publishes the GitHub Release first.
It then runs the same extension-publishing job.

## Release Tags

Release tags are annotated. Sign them when a repository signing identity is
configured. Otherwise, record in the release work note that the tag is
unsigned by choice.
