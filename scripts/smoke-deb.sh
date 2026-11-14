#!/usr/bin/env bash
# Installs an Aura Debian package with apt-get, runs the installed command,
# and removes the package again. Needs sudo, so it is for CI machines.
set -euo pipefail

if [[ $# -ne 1 || ! -f "$1" ]]; then
  echo "usage: scripts/smoke-deb.sh <package.deb>" >&2
  exit 2
fi
package="$(cd "$(dirname "$1")" && pwd -P)/$(basename "$1")"
work_root="$(mktemp -d "${TMPDIR:-/tmp}/aura-deb-smoke.XXXXXX")"
cleanup() {
  cd /
  sudo apt-get remove -y aura >/dev/null 2>&1 || true
  rm -rf "$work_root"
}
trap cleanup EXIT HUP INT TERM

# apt-get reads the file as its own user, which cannot enter every directory.
cp "$package" "$work_root/aura.deb"
chmod 755 "$work_root"
sudo apt-get install -y "$work_root/aura.deb"
cd "$work_root"
if [[ "$(command -v aura)" != /usr/bin/aura ]]; then
  echo "the package did not put aura at /usr/bin/aura" >&2
  exit 1
fi
expected_version="$(dpkg-deb -f aura.deb Version | tr '~' '-')"
if [[ "$(aura --version | awk '{ print $2 }')" != "$expected_version" ]]; then
  echo "installed aura --version does not report $expected_version" >&2
  exit 1
fi
export AURA_CACHE_DIR="$work_root/cache"
if [[ "$(aura run --backend direct /usr/share/aura/examples/basic_addition.au)" != "16" ]]; then
  echo "the installed aura did not run the packaged example" >&2
  exit 1
fi
aura build -o "$work_root/basic_addition" /usr/share/aura/examples/basic_addition.au
if [[ "$("$work_root/basic_addition")" != "16" ]]; then
  echo "the program built by the installed aura returned unexpected output" >&2
  exit 1
fi
if upgrade_output="$(aura upgrade 2>&1)" || [[ "$upgrade_output" != *"apt-get install --only-upgrade aura"* ]]; then
  echo "aura upgrade did not point at apt: $upgrade_output" >&2
  exit 1
fi
sudo apt-get remove -y aura
if [[ -e /usr/bin/aura || -e /usr/lib/aura ]]; then
  echo "removing the package left files behind" >&2
  exit 1
fi
echo "Debian package smoke test passed"
