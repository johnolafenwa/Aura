#!/usr/bin/env bash
set -euo pipefail

git diff --check
git show --check --format= HEAD

for forbidden in .DS_Store eval_aurora; do
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
