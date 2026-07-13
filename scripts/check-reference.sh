#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

required_pages=(
  language-specification
  grammar
  names-and-scopes
  static-semantics
  execution-model
  diagnostics
  conformance
)

for page in "${required_pages[@]}"; do
  path="docs/manual/${page}.md"
  if [[ ! -s "$path" ]]; then
    echo "missing normative reference page: $path" >&2
    exit 1
  fi
  if ! grep -Fq "/manual/${page}" docs/manual/index.md; then
    echo "manual index does not link normative page: $page" >&2
    exit 1
  fi
  if ! grep -Fq "/manual/${page}" docs/.vitepress/config.mts; then
    echo "VitePress sidebar does not link normative page: $page" >&2
    exit 1
  fi
done

grep -Fq 'module = { module-element }' docs/manual/grammar.md
grep -Fq 'postfix-expression' docs/manual/grammar.md
grep -Fq 'left-to-right' docs/manual/execution-model.md
grep -Fq 'compiler fixtures' docs/manual/conformance.md
grep -Fq 'MUST' docs/manual/language-specification.md
grep -Fq '`int` is an alias for `int64`' docs/manual/types.md
grep -Fq 'Existing fixed `int32` contracts remain `int32`' docs/manual/types.md
grep -Fq 'otherwise the literal defaults to `int64`' docs/manual/lexical-structure.md
grep -Fq 'otherwise it defaults to `int64`' docs/manual/static-semantics.md
grep -Fq '= "int" | "int8"' docs/manual/grammar.md
grep -Fq 'Integer literals default to `int64`' tutorials/02-bindings-and-types.md
grep -Fq '`int` is an alias for `int64`' docs/aurora_language_proposal.md
grep -Fq '<code>int</code> is an alias for <code>int64</code>' docs/aurora_language_proposal.html

if rg -ni 'defaults? to (`|<code>)?int32|default for most integer work|use (`|<code>)?int32[^[:space:]]* and (`|<code>)?float64[^[:space:]]* by default|no (unsuffixed|bare) (`|<code>)?int' \
  docs/manual \
  tutorials \
  docs/aurora_language_proposal.md \
  docs/aurora_language_proposal.html; then
  echo "reference still describes the retired int32 default or rejects the int alias" >&2
  exit 1
fi

if rg -n 'maintained interpreter|tree-walk interpreter' docs/manual; then
  echo "manual still describes the removed interpreter as maintained" >&2
  exit 1
fi
