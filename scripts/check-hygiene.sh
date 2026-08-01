#!/usr/bin/env bash
set -euo pipefail

# The personal scratch source is user-owned and intentionally outside repository
# maintenance tasks; do not make its local formatting a release-gate input.
git diff --check -- . ':(exclude)personal/file_ops.au'
git show --check --format= HEAD

for forbidden in .DS_Store eval_aura; do
  if git ls-files --error-unmatch "$forbidden" >/dev/null 2>&1 ||
    git ls-files "$forbidden/**" | grep -q .; then
    echo "tracked repository artifact is forbidden: $forbidden" >&2
    exit 1
  fi
done

while IFS= read -r -d '' path; do
  description="$(file -b "$path")"
  case "$description" in
    *Mach-O*|*ELF*|*PE32*)
      echo "tracked compiled executable is forbidden: $path ($description)" >&2
      exit 1
      ;;
  esac
done < <(git ls-files -z)

compiler_source="crates/aura-compiler/src"

if rg --pcre2 --line-number \
  'scheduler\s*:\s*\*mut\s+LightweightTaskScheduler' \
  "$compiler_source" --glob '*.rs'; then
  echo "raw LightweightTaskScheduler pointers are forbidden in maintained compiler source" >&2
  echo "use the scheduler-owned spawn-request broker instead" >&2
  exit 1
fi

if rg --pcre2 --line-number \
  '&mut\s*\*\s*(?:\(\s*)?scheduler\b' \
  "$compiler_source" --glob '*.rs'; then
  echo "unsafe mutable scheduler reconstruction is forbidden in maintained compiler source" >&2
  echo "route scheduler mutations through an owned safe interface instead" >&2
  exit 1
fi
