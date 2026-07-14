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
grep -Fq '+ += - -= * *= / /= // //= % %=' docs/manual/lexical-structure.md
grep -Fq 'assignment-operator = "=" | "+=" | "-=" | "*=" | "/=" | "//=" | "%=" ;' docs/manual/grammar.md
grep -Fq '{ ("*" | "/" | "//" | "%"), prefix-expression } ;' docs/manual/grammar.md
grep -Fq 'integer `/` is not supported; use `//` for floor division, or call `.to_float()` on both operands for true division' docs/manual/static-semantics.md
grep -Fq 'CPython-compatible divmod correction' docs/manual/execution-model.md
grep -Fq 'integer `.to_float()` converts to `float64`' docs/manual/execution-model.md
grep -Fq 'There is no `FloorDiv` operator trait.' docs/manual/generics-and-traits.md
test -s examples/basics/numbers.au
grep -Fq '`numbers.au`' examples/README.md
grep -Fq '[examples/basics/numbers.au]' tutorials/07-strings-and-numbers.md
grep -Fq 'Ordinary string literals use matching single or double quote delimiters' docs/manual/lexical-structure.md
grep -Fq 'F-strings themselves' docs/manual/lexical-structure.md
grep -Fq 'Counts Unicode scalar values in O(n)' docs/manual/api-index.md
grep -Fq 'Returns the UTF-8 byte count in O(1)' docs/manual/api-index.md
grep -Fq 'negative index `i` is' docs/manual/collections.md
grep -Fq 'normalized once as `len + i`' docs/manual/collections.md
grep -Fq 'does not clamp' docs/manual/collections.md
grep -Fq "unicode = 'A🎉'" examples/strings/string_methods.au
grep -Fq 'unicode.len()' examples/strings/string_methods.au
grep -Fq 'unicode.byte_len()' examples/strings/string_methods.au
grep -Fq 'values.insert(index=-1, value=2)' examples/collections/vec_polish.au
grep -Fq 'mut borrow own indirect' docs/manual/lexical-structure.md
grep -Fq '| "own", "self"' docs/manual/grammar.md
grep -Fq 'Bare `self` and `borrow self` are the two spellings of a shared receiver' docs/manual/grammar.md
grep -Fq '| `own self` | Consuming receiver.' docs/manual/classes.md
grep -Fq '`self: Type` is not a method receiver' architecture_docs/decisions/0005-method-receivers.md
grep -Fq '`own self` for by-value consumption' docs/aurora_language_proposal.md
grep -Fq '<code>own self</code> for by-value consumption' docs/aurora_language_proposal.html
grep -Fq '`value: T` | Shared borrow when `T` is non-copy' docs/manual/functions.md
grep -Fq '`value: own T` | Owned argument' docs/manual/functions.md
grep -Fq 'caller-invisible temporary' docs/manual/functions.md
grep -Fq 'declaration-stable' docs/manual/generics-and-traits.md
grep -Fq 'define or inherit a trait method whose name is a builtin member of that handle.' docs/manual/generics-and-traits.md
grep -Fq 'builtin handle members always retain builtin dispatch' docs/manual/generics-and-traits.md
grep -Fq 'for value in own values' docs/manual/statements.md
grep -Fq 'Queue iteration receives values' docs/manual/concurrency.md
grep -Fq 'parameter `x` is borrowed; declare it as `own String`' docs/manual/diagnostics.md
grep -Fq 'default temporary lives until the call completes' docs/manual/execution-model.md
grep -Fq 'push(value: own T)' docs/manual/api-index.md
grep -Fq 'set(key: own K, value: own V)' docs/manual/api-index.md
grep -Fq 'put(value: own T' docs/manual/api-index.md
grep -Fq 'result_or(default: own T' docs/manual/api-index.md
grep -Fq 'start(function, own ...) -> Task[T]' docs/manual/api-index.md
grep -Fq 'restart: own process.RestartPolicy' docs/manual/api-index.md
grep -Fq 'bare `for value in queue:` form' architecture_docs/decisions/0006-parameter-and-loop-ownership-defaults.md
grep -Fq 'borrow mut' architecture_docs/decisions/0006-parameter-and-loop-ownership-defaults.md
grep -Fq 'for name in own names' tutorials/06-ownership-and-borrowing.md
grep -Fq 'def handle(stream: own net.TcpStream)' docs/manual/network.md
grep -Fq 'def handle(stream: own net.TcpStream)' docs/learn/io-process-networking.md
grep -Fq 'def serve(listener: own net.TcpListener)' tutorials/19-io-and-networking.md
grep -Fq 'def process_file(handle: own FileHandle)' tutorials/12-error-propagation.md
grep -Fq 'Queue and task handles are cheap copy-like values' tutorials/06-ownership-and-borrowing.md
grep -Fq 'declaration-stable' docs/aurora_language_proposal.html
grep -Fq 'Queue iteration receives each item already owned' docs/aurora_language_proposal.html

if grep -Fq -- '- `Queue[T]`, `Task[T]`, `TaskGroup`' tutorials/06-ownership-and-borrowing.md; then
  echo "ownership tutorial still classifies Queue and Task copy handles as move types" >&2
  exit 1
fi

if rg -n 'no (integer )?floor division|integer division truncates toward zero|Result\.Ok\([^)]* / [^)]*\)' \
  docs/manual \
  tutorials \
  docs/learn; then
  echo "reference still describes retired integer division behavior" >&2
  exit 1
fi

if rg -ni 'defaults? to (`|<code>)?int32|default for most integer work|use (`|<code>)?int32[^[:space:]]* and (`|<code>)?float64[^[:space:]]* by default|no (unsuffixed|bare) (`|<code>)?int' \
  docs/manual \
  tutorials \
  docs/aurora_language_proposal.md \
  docs/aurora_language_proposal.html; then
  echo "reference still describes the retired int32 default or rejects the int alias" >&2
  exit 1
fi

if rg -n 'Strings use double quotes|Strings are double-quoted|`STRING` is a double-quoted|Single-quoted, triple-quoted' \
  docs/manual \
  tutorials \
  docs/learn; then
  echo "reference still describes ordinary strings as double-quoted only" >&2
  exit 1
fi

if rg -n '`self` -- by-value|plain `self` receiver|`self` consumes|\| `self` \| Consume' \
  docs/manual \
  tutorials \
  docs/learn; then
  echo "reference still describes bare self as a consuming receiver" >&2
  exit 1
fi

if rg -n 'for x in expr:` consumes|for value in vec:` \| Consumes|`for value in names` iterates by value|Map\.get[^\n]*(takes|consumes) (its )?key by value|Every task target parameter must be by value|target.s ordinary parameters must be by value|TaskGroup[^\n]*(do not|does not) yet support borrowed parameters' \
  docs/manual \
  tutorials \
  docs/learn \
  docs/aurora_language_proposal.md \
  docs/aurora_language_proposal.html; then
  echo "reference still describes retired parameter, loop, lookup, or task-capture ownership behavior" >&2
  exit 1
fi

if rg -n 'maintained interpreter|tree-walk interpreter' docs/manual; then
  echo "manual still describes the removed interpreter as maintained" >&2
  exit 1
fi

python3 scripts/test_reference_integrity.py
python3 scripts/reference_integrity.py
