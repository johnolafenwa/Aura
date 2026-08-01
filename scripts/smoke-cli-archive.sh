#!/usr/bin/env bash
set -euo pipefail

if [[ $# -ne 1 ]]; then
  echo "usage: scripts/smoke-cli-archive.sh <archive.tar.gz>" >&2
  exit 2
fi

archive="$1"
if [[ ! -f "$archive" ]]; then
  echo "archive does not exist: $archive" >&2
  exit 2
fi

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd -P)"
archive="$(cd "$(dirname "$archive")" && pwd -P)/$(basename "$archive")"
work_root="$(mktemp -d "${TMPDIR:-/tmp}/aura-cli-archive-smoke.XXXXXX")"
install_root="$work_root/install"
source_root="$work_root/sources"
output_root="$work_root/output"
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

mkdir -p "$install_root" "$source_root/examples/agents" "$output_root"

top_levels="$(tar -tzf "$archive" | awk -F/ 'NF && $1 != "." { print $1 }' | sort -u)"
if [[ -z "$top_levels" || "$top_levels" == *$'\n'* ]]; then
  echo "archive must contain exactly one top-level directory" >&2
  exit 1
fi
top_level="$top_levels"

tar -xzf "$archive" -C "$install_root"
packaged="$install_root/$top_level/bin/aura"
if [[ ! -x "$packaged" ]]; then
  echo "packaged aura is missing or not executable: $packaged" >&2
  exit 1
fi
packaged_basic="$install_root/$top_level/examples/basic_addition.au"
packaged_retry="$install_root/$top_level/examples/agents/retrying_network_worker.au"
if [[ ! -f "$packaged_basic" || ! -f "$packaged_retry" ]]; then
  echo "packaged release examples are missing" >&2
  exit 1
fi
cp "$packaged_basic" "$source_root/examples/basic_addition.au"
cp "$packaged_retry" "$source_root/examples/agents/retrying_network_worker.au"

missing_cargo="$work_root/tooling/cargo-must-not-exist"
version_stdout="$output_root/version.stdout"
version_stderr="$output_root/version.stderr"
basic_stdout="$output_root/basic.stdout"
basic_stderr="$output_root/basic.stderr"
retry_stdout="$output_root/retry.stdout"
retry_stderr="$output_root/retry.stderr"

run_owned() {
  local stdout_path="$1"
  local stderr_path="$2"
  local timeout_seconds="$3"
  shift 3

  python3 - "$stdout_path" "$stderr_path" "$timeout_seconds" "$@" <<'PY'
import os
from pathlib import Path
import signal
import subprocess
import sys
import time

stdout_path = Path(sys.argv[1])
stderr_path = Path(sys.argv[2])
timeout_seconds = float(sys.argv[3])
command = sys.argv[4:]


def interrupt(_signum, _frame):
    raise KeyboardInterrupt


def terminate_group(process):
    try:
        os.killpg(process.pid, signal.SIGTERM)
    except ProcessLookupError:
        return
    deadline = time.monotonic() + 1.0
    while time.monotonic() < deadline:
        try:
            os.killpg(process.pid, 0)
        except ProcessLookupError:
            return
        time.sleep(0.02)
    try:
        os.killpg(process.pid, signal.SIGKILL)
    except ProcessLookupError:
        pass


signal.signal(signal.SIGINT, interrupt)
signal.signal(signal.SIGTERM, interrupt)
process = subprocess.Popen(
    command,
    stdin=subprocess.DEVNULL,
    stdout=subprocess.PIPE,
    stderr=subprocess.PIPE,
    start_new_session=True,
)
try:
    stdout, stderr = process.communicate(timeout=timeout_seconds)
except subprocess.TimeoutExpired as error:
    terminate_group(process)
    stdout, stderr = process.communicate()
    stdout_path.write_bytes(stdout)
    stderr_path.write_bytes(stderr)
    print(
        f"archive smoke command timed out after {timeout_seconds:g}s: {command!r}",
        file=sys.stderr,
    )
    raise SystemExit(124) from error
except KeyboardInterrupt:
    terminate_group(process)
    stdout, stderr = process.communicate()
    stdout_path.write_bytes(stdout)
    stderr_path.write_bytes(stderr)
    raise SystemExit(130)

# A successful parent must not leave any descendants in its owned process group.
terminate_group(process)
stdout_path.write_bytes(stdout)
stderr_path.write_bytes(stderr)
raise SystemExit(process.returncode)
PY
}

run_and_show() {
  local stdout_path="$1"
  local stderr_path="$2"
  local timeout_seconds="$3"
  shift 3

  if run_owned "$stdout_path" "$stderr_path" "$timeout_seconds" "$@"; then
    cat "$stdout_path"
    cat "$stderr_path" >&2
  else
    local status=$?
    cat "$stdout_path"
    cat "$stderr_path" >&2
    return "$status"
  fi
}

cd "$work_root"

expected_commit="${AURA_EXPECTED_COMMIT:-}"
if [[ -z "$expected_commit" ]]; then
  expected_commit="$(git -C "$repo_root" rev-parse --verify --short=12 HEAD^{commit})"
fi
if [[ ! "$expected_commit" =~ ^[0-9a-fA-F]{12}$ ]]; then
  echo "expected Aura build commit must be exactly 12 hexadecimal digits" >&2
  exit 2
fi
expected_version="aura 0.2.0-preview ($expected_commit)"

run_and_show "$version_stdout" "$version_stderr" 15 \
  env CARGO="$missing_cargo" "$packaged" --version
if ! grep -Fxq "$expected_version" "$version_stdout"; then
  echo "packaged aura --version returned an unexpected value" >&2
  exit 1
fi

if [[ -e "$cache_root" ]]; then
  echo "archive smoke cache must be absent before the first direct run" >&2
  exit 1
fi

run_and_show "$basic_stdout" "$basic_stderr" 60 \
  env AURA_CACHE_DIR="$cache_root" CARGO="$missing_cargo" \
  "$packaged" run --backend direct \
  "$source_root/examples/basic_addition.au"
if [[ "$(<"$basic_stdout")" != "16" ]]; then
  echo "packaged direct basic example returned unexpected stdout" >&2
  exit 1
fi

run_and_show "$retry_stdout" "$retry_stderr" 90 \
  env AURA_CACHE_DIR="$cache_root" CARGO="$missing_cargo" \
  "$packaged" run --backend direct \
  "$source_root/examples/agents/retrying_network_worker.au"

expected_retry=$'recover request 1\nrecover retry 4ms\nrecover request 2\nrecover result 200\nrate request 1\nrate retry 6ms\nrate request 2\nrate result 429\nexhaust request 1\nexhaust retry 3ms\nexhaust request 2\nexhaust retry 5ms\nexhaust request 3\nexhaust result 503\nrequests 7'
if [[ "$(<"$retry_stdout")" != "$expected_retry" ]]; then
  echo "packaged retrying worker returned unexpected stdout" >&2
  exit 1
fi
