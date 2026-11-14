#!/usr/bin/env bash
# Downloads the Debian packages of the newest Aura release that has them and
# builds the signed apt repository in the given directory. The Docs workflow
# runs it before it uploads the site.
#
# Without `APT_SIGNING_KEY`, or before any release has a Debian package, it
# writes nothing and succeeds, so the site still deploys.
set -euo pipefail

if [[ $# -ne 1 ]]; then
  echo "usage: scripts/publish-apt-repository.sh <output-directory>" >&2
  exit 2
fi
output="$1"
repository="${GITHUB_REPOSITORY:-johnolafenwa/Aura}"

if [[ -z "${APT_SIGNING_KEY:-}" ]]; then
  echo "::notice::APT_SIGNING_KEY is not configured; the site is deployed without the apt repository."
  exit 0
fi

tag="$(gh api "repos/$repository/releases?per_page=30" \
  --jq '[.[] | select(.draft | not) | select(any(.assets[]; .name | endswith(".deb")))][0].tag_name // empty')"
if [[ -z "$tag" ]]; then
  echo "::notice::No release has a Debian package yet; the site is deployed without the apt repository."
  exit 0
fi

script_directory="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd -P)"
downloads="$(mktemp -d "${TMPDIR:-/tmp}/aura-apt-debs.XXXXXX")"
trap 'rm -rf "$downloads"' EXIT HUP INT TERM
gh release download "$tag" --repo "$repository" --pattern 'aura-*.deb' --pattern SHA256SUMS --dir "$downloads"
(cd "$downloads" && grep -E '  aura-[^/]*\.deb$' SHA256SUMS | sha256sum -c - && rm SHA256SUMS)

if ! command -v apt-ftparchive >/dev/null 2>&1 || ! command -v dpkg-scanpackages >/dev/null 2>&1; then
  sudo apt-get update
  sudo apt-get install -y apt-utils dpkg-dev
fi
"$script_directory/build-apt-repository.sh" "$downloads" "$output"
echo "apt repository built from $tag"
