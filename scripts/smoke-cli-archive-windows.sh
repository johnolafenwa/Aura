#!/usr/bin/env bash
# Runs a packaged Windows archive outside the source tree, with no Cargo.
set -euo pipefail

if [[ $# -ne 1 ]]; then
  echo "usage: scripts/smoke-cli-archive-windows.sh <archive.zip>" >&2
  exit 2
fi

archive="$1"
if [[ ! -f "$archive" ]]; then
  echo "archive does not exist: $archive" >&2
  exit 2
fi

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd -P)"
archive="$(cd "$(dirname "$archive")" && pwd -P)/$(basename "$archive")"
python="$(command -v python3 || command -v python)"
work_root="$(mktemp -d "${TMPDIR:-/tmp}/aura-cli-archive-smoke.XXXXXX")"
install_root="$work_root/install"
source_root="$work_root/sources"
cache_root="$work_root/cache"

cleanup() {
  rm -rf "$work_root"
}
trap cleanup EXIT HUP INT TERM

case "$(cd "$work_root" && pwd -P)/" in
  "$repo_root"/*)
    echo "smoke work directory must be outside the source checkout" >&2
    exit 1
    ;;
esac

top_level="$(basename "$archive" .zip)"
if [[ ! "$top_level" =~ ^aura-v([0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z.-]+)?)-x86_64-pc-windows-msvc$ ]]; then
  echo "archive has an invalid release identity: $top_level" >&2
  exit 1
fi
release_version="${BASH_REMATCH[1]}"

mkdir -p "$install_root" "$source_root/examples/agents"
"$python" -m zipfile -e "$(cygpath -w "$archive")" "$(cygpath -w "$install_root")"
if [[ "$(ls "$install_root")" != "$top_level" ]]; then
  echo "archive must contain exactly one top-level directory named $top_level" >&2
  exit 1
fi
packaged="$install_root/$top_level/bin/aura.exe"
runtime_dir="$install_root/$top_level/lib/aura"
if [[ ! -f "$packaged" || ! -f "$runtime_dir/aura_runtime.lib" || ! -f "$runtime_dir/native-link-args.json" ]]; then
  echo "packaged aura or its runtime is missing" >&2
  exit 1
fi
cp "$install_root/$top_level/examples/basic_addition.au" "$source_root/examples/basic_addition.au"
cp "$install_root/$top_level/examples/agents/retrying_network_worker.au" \
  "$source_root/examples/agents/retrying_network_worker.au"

missing_cargo="$work_root/tooling/cargo-must-not-exist"
cd "$work_root"

expected_commit="${AURA_EXPECTED_COMMIT:-}"
if [[ -z "$expected_commit" ]]; then
  expected_commit="$(git -C "$repo_root" rev-parse --verify --short=12 HEAD^{commit})"
fi
expected_version="aura $release_version ($expected_commit)"

run_packaged() {
  local seconds="$1"
  shift
  env AURA_CACHE_DIR="$cache_root" CARGO="$missing_cargo" timeout "$seconds" "$packaged" "$@" | tr -d '\r'
}

version="$(run_packaged 15 --version)"
echo "$version"
if [[ "$version" != "$expected_version" ]]; then
  echo "packaged aura --version returned an unexpected value, expected: $expected_version" >&2
  exit 1
fi

basic="$(run_packaged 120 run --backend direct "$source_root/examples/basic_addition.au")"
echo "$basic"
if [[ "$basic" != "16" ]]; then
  echo "packaged direct basic example returned unexpected stdout" >&2
  exit 1
fi

retry="$(run_packaged 120 run --backend direct "$source_root/examples/agents/retrying_network_worker.au")"
echo "$retry"
expected_retry=$'recover request 1\nrecover retry 4ms\nrecover request 2\nrecover result 200\nrate request 1\nrate retry 6ms\nrate request 2\nrate result 429\nexhaust request 1\nexhaust retry 3ms\nexhaust request 2\nexhaust retry 5ms\nexhaust request 3\nexhaust result 503\nrequests 7'
if [[ "$retry" != "$expected_retry" ]]; then
  echo "packaged retrying worker returned unexpected stdout" >&2
  exit 1
fi

run_packaged 120 build -o "$work_root/basic_addition.exe" "$source_root/examples/basic_addition.au"
built="$(timeout 30 "$work_root/basic_addition.exe" | tr -d '\r')"
if [[ "$built" != "16" ]]; then
  echo "an executable built by the packaged aura returned unexpected stdout" >&2
  exit 1
fi
