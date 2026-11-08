#!/usr/bin/env bash
# Installs the pinned Z3 that `aura verify` and its tests run against.
#
# Z3 comes from the `z3-solver` Python wheel, which carries a prebuilt `z3`
# executable for Linux and macOS on x86-64 and arm64, into a private
# virtual environment. pip checks the wheel's hash against PyPI's index.
#
# Usage: scripts/install-z3.sh [prefix]
#
# The default prefix is ~/.cache/aura/z3/<version>. The script prints the
# directory to put on PATH, and appends it to $GITHUB_PATH under GitHub
# Actions. That directory holds only `z3`, so the environment's own
# `python3` stays off PATH. It does nothing when the prefix already holds
# this version.
set -euo pipefail

version=5.3.3.0
prefix="${1:-$HOME/.cache/aura/z3/$version}"

if [ ! -x "$prefix/bin/z3" ]; then
  python3 -m venv "$prefix"
  "$prefix/bin/python" -m pip install --quiet --disable-pip-version-check "z3-solver==$version"
fi

mkdir -p "$prefix/solver"
ln -sf "$prefix/bin/z3" "$prefix/solver/z3"
"$prefix/solver/z3" --version
echo "export PATH=$prefix/solver:\$PATH"
if [ -n "${GITHUB_PATH:-}" ]; then
  echo "$prefix/solver" >> "$GITHUB_PATH"
fi
