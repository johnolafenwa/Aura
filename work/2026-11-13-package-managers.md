# Homebrew, apt And Chocolatey Packages

## Goal

On 2026-11-13 John asked: "Can we get install for homebrew, and chocolately
and apt-get".

## What Landed

- `packaging/` holds the templates: a Homebrew formula, a Debian control
  file, and a Chocolatey nuspec. `packaging/README.md` maps each to its
  scripts.
- `scripts/package-deb.sh` turns a Linux release archive into a Debian
  package that installs under `/usr`. `scripts/build-apt-repository.sh`
  builds a signed apt repository (`dists/stable`, component `main`) from
  the packages, and `scripts/publish-apt-repository.sh` does so for the
  newest release during a site deployment, into `/Aura/apt`.
- `scripts/package-chocolatey.sh` turns the Windows release archive into
  a Chocolatey package that carries the archive's files. Chocolatey makes
  the `aura` command from `aura.exe`, so there is no install script.
- `scripts/render-homebrew-formula.py` writes the formula for a release
  from its `SHA256SUMS`. The formula installs the release archive for
  macOS or Linux on either architecture.
- The Release workflow builds the Debian and Chocolatey packages in the
  `cli` job, installs each on the runner, and attaches them to the
  release. The jobs `publish-homebrew`, `publish-chocolatey` and
  `publish-apt` run after the GitHub Release. The first two need a secret
  and skip with a notice without it. The third starts the Docs workflow.
- CI installs every package: the `native-binaries` job builds and
  installs the Debian package on both Linux runners and the formula from
  a local tap on macOS, and the `windows` job packs and installs the
  Chocolatey package.
- `aura upgrade` reads `lib/aura/install-manager`, which each package
  writes, and prints the package manager's upgrade command. Before this
  it would have copied the installer's files over the package's.

## Decisions

- Homebrew uses a tap, `johnolafenwa/homebrew-aura`. The main Homebrew
  repository takes stable versions built from source, and Aura ships
  previews as prebuilt archives.
- The apt repository lives on the documentation site, so it needs no
  other host. The packages are about 40 MB each and the repository keeps
  only the newest release, which stays far below the 1 GB limit of the
  site.
- The Debian version of `v0.16.0-preview` is `0.16.0~preview`, which
  sorts before `0.16.0`. The release asset is named
  `aura-v0.16.0-preview-amd64.deb`, because GitHub rewrites `~` in asset
  names.
- The Chocolatey package holds the files. A package that downloads the
  archive at install time cannot be installed in the Release workflow
  before the release exists.

## Verification

- `python3 -m unittest scripts/test_release_packaging.py
  scripts/test_package_managers.py`: 58 tests pass.
- With the published 0.15.0 Linux archive: `scripts/package-deb.sh` built
  a 40 MB package, `scripts/build-apt-repository.sh` built a repository
  signed with a throwaway key, and `apt-get update` and
  `apt-get install aura` from that repository over HTTP installed it.
  The installed `aura` ran and built `basic_addition.au`, and
  `apt-get remove` left nothing behind.
- `cargo test -p aura --bin aura upgrade_defers` passes.
- Homebrew and Chocolatey could not be run in the cloud container. Their
  installs are checked by the CI jobs above.

## Waiting On John

Publishing needs three things only he can create: the tap repository and
a token for it, a Chocolatey account and API key, and an apt signing key.
`docs/release-process.md` gives the steps. The first release that ships
packages is the one after 0.15.
