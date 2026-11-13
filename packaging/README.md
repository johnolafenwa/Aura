# Packaging

Templates for the three package managers Aura installs with. The scripts
that fill them in are in `scripts/`, and the Release workflow runs them.
`docs/release-process.md` lists the secrets each publishing job needs.

| Package manager | Template | Built by | Checked by |
| --- | --- | --- | --- |
| Homebrew | `homebrew/aura.rb.in` | `scripts/render-homebrew-formula.py`, from the release's `SHA256SUMS` | `scripts/smoke-homebrew.sh` |
| apt | `debian/control.in`, `debian/copyright.in` | `scripts/package-deb.sh`, from a Linux release archive, then `scripts/build-apt-repository.sh` | `scripts/smoke-deb.sh` |
| Chocolatey | `chocolatey/aura.nuspec.in`, `chocolatey/VERIFICATION.txt.in` | `scripts/package-chocolatey.sh`, from the Windows release archive | `scripts/smoke-chocolatey.sh` |

Every package installs the files of a release archive unchanged, with
`bin/aura` and `lib/aura` side by side, because `aura` finds its runtime
in `../lib/aura` from its own directory. Each one adds the file
`lib/aura/install-manager`, which holds `homebrew`, `apt` or `chocolatey`.
`aura upgrade` reads it and prints the package manager's upgrade command
in place of running the installer.

The smoke scripts install a package on the machine they run on, so they
are for CI. `scripts/test_package_managers.py` tests the rest and runs
anywhere.

## Try One Locally

```bash
# Debian package and a signed apt repository, on Debian or Ubuntu.
scripts/package-deb.sh aura-v0.16.0-preview-x86_64-unknown-linux-gnu.tar.gz
APT_SIGNING_KEY="$(gpg --armor --export-secret-keys)" \
  scripts/build-apt-repository.sh . /tmp/aura-apt

# Homebrew formula for a published release.
gh release download v0.16.0-preview --pattern SHA256SUMS
python3 scripts/render-homebrew-formula.py --tag v0.16.0-preview --sha256sums SHA256SUMS

# Chocolatey package, from Git Bash on Windows.
scripts/package-chocolatey.sh aura-v0.16.0-preview-x86_64-pc-windows-msvc.zip
```
