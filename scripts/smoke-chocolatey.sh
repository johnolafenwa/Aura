#!/usr/bin/env bash
# Installs an Aura Chocolatey package from the current directory, runs the
# installed command, and uninstalls it. Run it from Git Bash on a Windows CI
# machine with the Microsoft linker.
set -euo pipefail

if [[ $# -ne 1 || ! -f "$1" ]]; then
  echo "usage: scripts/smoke-chocolatey.sh <aura.VERSION.nupkg>" >&2
  exit 2
fi
package_directory="$(cd "$(dirname "$1")" && pwd -P)"
version="$(basename "$1" .nupkg)"
version="${version#aura.}"
work_root="$(mktemp -d "${TMPDIR:-/tmp}/aura-choco-smoke.XXXXXX")"
cleanup() {
  cd /
  choco uninstall aura --yes --limit-output >/dev/null 2>&1 || true
  rm -rf "$work_root"
}
trap cleanup EXIT HUP INT TERM

choco install aura --version "$version" --pre --yes --no-progress --limit-output \
  --source "$(cygpath -w "$package_directory")"
choco_root="$(cygpath -u "${ChocolateyInstall:-C:\\ProgramData\\chocolatey}")"
shim="$choco_root/bin/aura.exe"
installed="$choco_root/lib/aura/tools/aura"
if [[ ! -f "$shim" ]]; then
  echo "Chocolatey did not create $shim" >&2
  exit 1
fi
cd "$work_root"
if [[ "$("$shim" --version | tr -d '\r' | awk '{ print $2 }')" != "$version" ]]; then
  echo "installed aura --version does not report $version" >&2
  exit 1
fi
export AURA_CACHE_DIR="$work_root/cache"
example="$(cygpath -w "$installed/examples/basic_addition.au")"
if [[ "$("$shim" run --backend direct "$example" | tr -d '\r')" != "16" ]]; then
  echo "the installed aura did not run the packaged example" >&2
  exit 1
fi
"$shim" build -o basic_addition.exe "$example"
if [[ "$(./basic_addition.exe | tr -d '\r')" != "16" ]]; then
  echo "the program built by the installed aura returned unexpected output" >&2
  exit 1
fi
if upgrade_output="$("$shim" upgrade 2>&1)" || [[ "$upgrade_output" != *"choco upgrade aura --pre"* ]]; then
  echo "aura upgrade did not point at Chocolatey: $upgrade_output" >&2
  exit 1
fi
choco uninstall aura --yes --limit-output
if [[ -e "$shim" ]]; then
  echo "uninstalling the package left the aura shim behind" >&2
  exit 1
fi
echo "Chocolatey package smoke test passed"
