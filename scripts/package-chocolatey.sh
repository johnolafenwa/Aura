#!/usr/bin/env bash
# Builds the Chocolatey package from the Windows release archive.
#
#   scripts/package-chocolatey.sh aura-v0.16.0-preview-x86_64-pc-windows-msvc.zip
#
# writes aura.0.16.0-preview.nupkg in the current directory. The package
# carries the archive's files, so installing it downloads nothing else.
# Run it from Git Bash on Windows, where `choco pack` exists. Elsewhere, set
# AURA_CHOCOLATEY_STAGE_ONLY=1 to stop after writing release/chocolatey.
set -euo pipefail

if [[ $# -ne 1 ]]; then
  echo "usage: scripts/package-chocolatey.sh <archive.zip>" >&2
  exit 2
fi

archive="$1"
archive_name="$(basename "$archive" .zip)"
if [[ ! "$archive_name" =~ ^aura-v([0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z]+)?)-x86_64-pc-windows-msvc$ ]]; then
  # A Chocolatey prerelease label holds only letters and digits.
  echo "invalid Windows release archive name: $archive_name" >&2
  exit 2
fi
version="${BASH_REMATCH[1]}"
if [[ ! -f "$archive" ]]; then
  echo "archive does not exist: $archive" >&2
  exit 2
fi

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd -P)"
python="$(command -v python3 || command -v python)"
stage="release/chocolatey"
rm -rf "$stage"
mkdir -p "$stage/tools"
"$python" -m zipfile -e "$archive" "$stage/tools"
mv "$stage/tools/$archive_name" "$stage/tools/aura"
for file in bin/aura.exe lib/aura/aura_runtime.lib lib/aura/native-link-args.json LICENSE; do
  if [[ ! -f "$stage/tools/aura/$file" ]]; then
    echo "the archive is missing $file" >&2
    exit 1
  fi
done
# Tells `aura upgrade` that Chocolatey owns these files.
printf 'chocolatey\n' > "$stage/tools/aura/lib/aura/install-manager"
cp "$stage/tools/aura/LICENSE" "$stage/tools/LICENSE.txt"

sha256_of() {
  "$python" -c 'import hashlib, sys; print(hashlib.sha256(open(sys.argv[1], "rb").read()).hexdigest())' "$1"
}
archive_sha256="$(sha256_of "$archive")"
aura_exe_sha256="$(sha256_of "$stage/tools/aura/bin/aura.exe")"
sed -e "s/@TAG@/v$version/g" -e "s/@ARCHIVE@/$archive_name.zip/g" \
  -e "s/@SHA256@/$archive_sha256/" -e "s/@AURA_EXE_SHA256@/$aura_exe_sha256/" \
  "$repo_root/packaging/chocolatey/VERIFICATION.txt.in" > "$stage/tools/VERIFICATION.txt"
sed -e "s/@VERSION@/$version/" -e "s/@TAG@/v$version/" \
  "$repo_root/packaging/chocolatey/aura.nuspec.in" > "$stage/aura.nuspec"

if [[ "${AURA_CHOCOLATEY_STAGE_ONLY:-}" == "1" ]]; then
  echo "$stage"
  exit 0
fi
rm -f "aura.$version.nupkg"
choco pack "$stage/aura.nuspec" --outputdirectory . --limit-output >&2
if [[ ! -f "aura.$version.nupkg" ]]; then
  echo "choco pack did not write aura.$version.nupkg" >&2
  exit 1
fi
echo "aura.$version.nupkg"
