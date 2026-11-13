#!/usr/bin/env bash
# Builds a signed apt repository from Aura's Debian packages.
#
#   scripts/build-apt-repository.sh <directory of .deb files> <output directory>
#
# The output directory is served as static files, for example at
# https://johnolafenwa.github.io/Aura/apt. `APT_SIGNING_KEY` holds the
# ASCII-armoured private OpenPGP key that signs the repository. The public
# half is written to the repository as `aura-archive-keyring.gpg`.
#
# Needs dpkg-scanpackages (dpkg-dev), apt-ftparchive (apt-utils) and gpg.
set -euo pipefail

if [[ $# -ne 2 ]]; then
  echo "usage: scripts/build-apt-repository.sh <deb-directory> <output-directory>" >&2
  exit 2
fi
deb_directory="$1"
output="$2"
if [[ -z "${APT_SIGNING_KEY:-}" ]]; then
  echo "APT_SIGNING_KEY must hold the private signing key" >&2
  exit 2
fi
shopt -s nullglob
packages=("$deb_directory"/*.deb)
if (( ${#packages[@]} == 0 )); then
  echo "no .deb files in $deb_directory" >&2
  exit 2
fi

GNUPGHOME="$(mktemp -d "${TMPDIR:-/tmp}/aura-apt-gnupg.XXXXXX")"
export GNUPGHOME
chmod 700 "$GNUPGHOME"
trap 'gpgconf --kill all >/dev/null 2>&1 || true; rm -rf "$GNUPGHOME"' EXIT HUP INT TERM
gpg --batch --quiet --import <<<"$APT_SIGNING_KEY"
mapfile -t fingerprints < <(gpg --batch --list-secret-keys --with-colons | awk -F: '$1 == "sec" { getline; print $10 }')
if (( ${#fingerprints[@]} != 1 )); then
  echo "APT_SIGNING_KEY must hold exactly one private key" >&2
  exit 2
fi
fingerprint="${fingerprints[0]}"

rm -rf "$output"
pool="$output/pool/main/a/aura"
mkdir -p "$pool"
architectures=()
for package in "${packages[@]}"; do
  if [[ "$(dpkg-deb -f "$package" Package)" != "aura" ]]; then
    echo "not an Aura package: $package" >&2
    exit 1
  fi
  architecture="$(dpkg-deb -f "$package" Architecture)"
  version="$(dpkg-deb -f "$package" Version)"
  # dpkg-scanpackages picks an architecture's packages by this file name.
  cp "$package" "$pool/aura_${version}_${architecture}.deb"
  architectures+=("$architecture")
done
mapfile -t architectures < <(printf '%s\n' "${architectures[@]}" | sort -u)

for architecture in "${architectures[@]}"; do
  binary="$output/dists/stable/main/binary-$architecture"
  mkdir -p "$binary"
  (cd "$output" && dpkg-scanpackages --multiversion --arch "$architecture" pool) > "$binary/Packages" 2>/dev/null
  if [[ ! -s "$binary/Packages" ]]; then
    echo "no package index was written for $architecture" >&2
    exit 1
  fi
  gzip -9 -n -k "$binary/Packages"
done

release="$output/dists/stable/Release"
(cd "$output/dists/stable" && apt-ftparchive \
  -o APT::FTPArchive::Release::Origin=Aura \
  -o APT::FTPArchive::Release::Label=Aura \
  -o APT::FTPArchive::Release::Suite=stable \
  -o APT::FTPArchive::Release::Codename=stable \
  -o "APT::FTPArchive::Release::Architectures=${architectures[*]}" \
  -o APT::FTPArchive::Release::Components=main \
  -o "APT::FTPArchive::Release::Description=The Aura programming language" \
  release .) > "$release.new"
mv "$release.new" "$release"
gpg --batch --yes --local-user "$fingerprint" --clearsign --output "$output/dists/stable/InRelease" "$release"
gpg --batch --yes --local-user "$fingerprint" --detach-sign --armor --output "$release.gpg" "$release"
gpg --batch --yes --export "$fingerprint" > "$output/aura-archive-keyring.gpg"

echo "apt repository for ${architectures[*]} signed by $fingerprint"
