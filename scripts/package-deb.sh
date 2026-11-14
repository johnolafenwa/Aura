#!/usr/bin/env bash
# Builds the Debian package from a Linux release archive.
#
#   scripts/package-deb.sh aura-v0.16.0-preview-x86_64-unknown-linux-gnu.tar.gz
#
# writes aura-v0.16.0-preview-amd64.deb in the current directory. The package
# installs the archive's files under /usr.
set -euo pipefail

if [[ $# -ne 1 ]]; then
  echo "usage: scripts/package-deb.sh <archive.tar.gz>" >&2
  exit 2
fi

archive="$1"
archive_name="$(basename "$archive" .tar.gz)"
if [[ ! "$archive_name" =~ ^aura-v([0-9]+\.[0-9]+\.[0-9]+)(-([0-9A-Za-z.]+))?-(x86_64|aarch64)-unknown-linux-gnu$ ]]; then
  echo "invalid Linux release archive name: $archive_name" >&2
  exit 2
fi
if [[ ! -f "$archive" ]]; then
  echo "archive does not exist: $archive" >&2
  exit 2
fi
# `0.16.0~preview` sorts before `0.16.0`, as a preview should.
deb_version="${BASH_REMATCH[1]}${BASH_REMATCH[3]:+~${BASH_REMATCH[3]}}"
release_tag="v${BASH_REMATCH[1]}${BASH_REMATCH[2]}"
case "${BASH_REMATCH[4]}" in
  x86_64) deb_architecture=amd64 ;;
  aarch64) deb_architecture=arm64 ;;
esac

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd -P)"
work_root="$(mktemp -d "${TMPDIR:-/tmp}/aura-deb.XXXXXX")"
trap 'rm -rf "$work_root"' EXIT HUP INT TERM

tar -xzf "$archive" -C "$work_root"
source_root="$work_root/$archive_name"
package_root="$work_root/package"
for file in bin/aura lib/aura/libaura_runtime.a lib/aura/native-link-args.json LICENSE; do
  if [[ ! -f "$source_root/$file" ]]; then
    echo "the archive is missing $file" >&2
    exit 1
  fi
done

install -d "$package_root/DEBIAN" "$package_root/usr/bin" "$package_root/usr/lib/aura" \
  "$package_root/usr/share/aura" "$package_root/usr/share/doc/aura"
install -m 755 "$source_root/bin/aura" "$package_root/usr/bin/aura"
install -m 644 "$source_root/lib/aura/libaura_runtime.a" "$source_root/lib/aura/native-link-args.json" \
  "$package_root/usr/lib/aura/"
# Tells `aura upgrade` that apt owns these files.
printf 'apt\n' > "$package_root/usr/lib/aura/install-manager"
cp -R "$source_root/examples" "$package_root/usr/share/aura/examples"
install -m 644 "$source_root/README.md" "$source_root/AURA_CLI_README.md" "$package_root/usr/share/doc/aura/"
{
  cat "$repo_root/packaging/debian/copyright.in"
  # The copyright format indents the license text and marks blank lines.
  sed -e '1,/^$/d' -e 's/^$/./' -e 's/^/ /' "$source_root/LICENSE"
} > "$package_root/usr/share/doc/aura/copyright"
find "$package_root" -type d -exec chmod 755 {} +
find "$package_root/usr/share" "$package_root/usr/lib" -type f -exec chmod 644 {} +

installed_size="$(du -sk "$package_root/usr" | cut -f1)"
sed -e "s/@VERSION@/$deb_version/" -e "s/@ARCHITECTURE@/$deb_architecture/" \
  -e "s/@INSTALLED_SIZE@/$installed_size/" \
  "$repo_root/packaging/debian/control.in" > "$package_root/DEBIAN/control"
(cd "$package_root" && find usr -type f -print0 | LC_ALL=C sort -z | xargs -0 md5sum) \
  > "$package_root/DEBIAN/md5sums"
chmod 644 "$package_root/DEBIAN/control" "$package_root/DEBIAN/md5sums"

output="aura-$release_tag-$deb_architecture.deb"
rm -f "$output"
dpkg-deb --root-owner-group -Zxz --build "$package_root" "$output" >/dev/null
echo "$output"
