#!/usr/bin/env bash
# Installs the Homebrew formula from a local tap that points at one release
# archive on disk, runs the installed command, and uninstalls it. It changes
# the machine's Homebrew, so it is for CI machines.
set -euo pipefail

if [[ $# -ne 1 || ! -f "$1" ]]; then
  echo "usage: scripts/smoke-homebrew.sh <archive.tar.gz>" >&2
  exit 2
fi
archive="$(cd "$(dirname "$1")" && pwd -P)/$(basename "$1")"
archive_name="$(basename "$archive" .tar.gz)"
if [[ ! "$archive_name" =~ ^aura-(v[0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z.-]+)?)-(x86_64|aarch64)-(apple-darwin|unknown-linux-gnu)$ ]]; then
  echo "invalid release archive name: $archive_name" >&2
  exit 2
fi
release_tag="${BASH_REMATCH[1]}"

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd -P)"
work_root="$(mktemp -d "${TMPDIR:-/tmp}/aura-brew-smoke.XXXXXX")"
tap=aura-ci/local
export HOMEBREW_NO_AUTO_UPDATE=1 HOMEBREW_NO_INSTALL_CLEANUP=1 HOMEBREW_NO_ANALYTICS=1 HOMEBREW_NO_ENV_HINTS=1
cleanup() {
  cd /
  brew uninstall --force "$tap/aura" >/dev/null 2>&1 || true
  brew untap "$tap" >/dev/null 2>&1 || true
  rm -rf "$work_root"
}
trap cleanup EXIT HUP INT TERM

# The formula names an archive for every platform. Only this machine's is
# downloaded, so the others can share its checksum.
mkdir "$work_root/release"
cp "$archive" "$work_root/release/"
if command -v sha256sum >/dev/null 2>&1; then
  checksum="$(sha256sum "$archive" | cut -d' ' -f1)"
else
  checksum="$(shasum -a 256 "$archive" | cut -d' ' -f1)"
fi
for target in aarch64-apple-darwin x86_64-apple-darwin aarch64-unknown-linux-gnu x86_64-unknown-linux-gnu; do
  printf '%s  aura-%s-%s.tar.gz\n' "$checksum" "$release_tag" "$target"
done > "$work_root/release/SHA256SUMS"
python3 "$repo_root/scripts/render-homebrew-formula.py" --tag "$release_tag" \
  --sha256sums "$work_root/release/SHA256SUMS" --base-url "file://$work_root/release" \
  --output "$work_root/aura.rb"

brew tap-new --no-git "$tap"
mkdir -p "$(brew --repository "$tap")/Formula"
cp "$work_root/aura.rb" "$(brew --repository "$tap")/Formula/aura.rb"
brew install "$tap/aura"
brew test "$tap/aura"

installed="$(brew --prefix)/bin/aura"
cd "$work_root"
if [[ "$("$installed" --version | awk '{ print $2 }')" != "${release_tag#v}" ]]; then
  echo "installed aura --version does not report ${release_tag#v}" >&2
  exit 1
fi
export AURA_CACHE_DIR="$work_root/cache"
example="$(brew --prefix)/share/aura/examples/basic_addition.au"
if [[ "$("$installed" run --backend direct "$example")" != "16" ]]; then
  echo "the installed aura did not run the packaged example" >&2
  exit 1
fi
"$installed" build -o "$work_root/basic_addition" "$example"
if [[ "$("$work_root/basic_addition")" != "16" ]]; then
  echo "the program built by the installed aura returned unexpected output" >&2
  exit 1
fi
if upgrade_output="$("$installed" upgrade 2>&1)" || [[ "$upgrade_output" != *"brew upgrade aura"* ]]; then
  echo "aura upgrade did not point at Homebrew: $upgrade_output" >&2
  exit 1
fi
brew uninstall "$tap/aura"
if [[ -e "$installed" ]]; then
  echo "uninstalling the formula left $installed behind" >&2
  exit 1
fi
echo "Homebrew formula smoke test passed"
