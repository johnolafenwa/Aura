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
if [[ ! -f "$downloads/SHA256SUMS" ]]; then
  echo "publish-apt-repository.sh: SHA256SUMS was not downloaded for $tag" >&2
  exit 1
fi
shopt -s nullglob
debs=("$downloads"/*.deb)
if (( ${#debs[@]} == 0 )); then
  echo "publish-apt-repository.sh: no .deb files were downloaded for $tag" >&2
  exit 1
fi
for deb in "${debs[@]}"; do
  if [[ ! -f "$deb" || -L "$deb" ]]; then
    echo "publish-apt-repository.sh: expected a regular .deb file: $deb" >&2
    exit 1
  fi
  name="$(basename "$deb")"
  entry="$(awk -v name="$name" '
    NF == 2 && $1 ~ /^[0-9a-fA-F]{64}$/ {
      file = $2
      sub(/^\*/, "", file)
      if (file == name) {
        print $1 "  " file
        count++
      }
    }
    END {
      if (count != 1) exit 1
    }
  ' "$downloads/SHA256SUMS")" || {
    echo "publish-apt-repository.sh: $name does not have a unique valid entry in SHA256SUMS" >&2
    exit 1
  }
  (cd "$downloads" && printf '%s\n' "$entry" | sha256sum -c -)
done
rm -f "$downloads/SHA256SUMS"

if ! command -v apt-ftparchive >/dev/null 2>&1 || ! command -v dpkg-scanpackages >/dev/null 2>&1; then
  sudo apt-get update
  sudo apt-get install -y apt-utils dpkg-dev
fi
"$script_directory/build-apt-repository.sh" "$downloads" "$output"
echo "apt repository built from $tag"
