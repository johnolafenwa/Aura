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
  assertions
  bytes
  json
  randomness
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
grep -Fq 'assert-statement' docs/manual/grammar.md
grep -Fq 'A failed assertion is `AU4001` at the `assert` keyword location.' docs/manual/diagnostics.md
grep -Fq 'An assertion evaluates its condition exactly once.' docs/manual/execution-model.md
grep -Fq 'An `assert` condition must have exactly type `bool`.' docs/manual/static-semantics.md
grep -Fq -- '- Status: Provisional' architecture_docs/decisions/0024-assertion-evaluation-and-diagnostic-policy.md
grep -Fq '0024-assertion-evaluation-and-diagnostic-policy.md' architecture_docs/decisions/README.md
test -s examples/basics/assertions.au
grep -Fq '`assertions.au`' examples/README.md
grep -Fq '[23-assertions.md]' tutorials/README.md
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
grep -Fq '| `left // right` | `FloorDiv.floor_div` |' docs/manual/generics-and-traits.md
grep -Fq 'trait FloorDiv[Rhs, Out]:' docs/manual/generics-and-traits.md
grep -Fq '`Duration` stores a signed 128-bit count of nanoseconds.' docs/manual/types.md
grep -Fq '| `Duration.ms` | `Duration.ms(value: int64) -> Duration`' docs/manual/api-index.md
grep -Fq '`Duration // int64`' docs/manual/expressions.md
grep -Fq '`//=` uses the builtin numeric or' docs/manual/statements.md
grep -Fq 'attempt * 1ms' docs/manual/concurrency.md
grep -Fq 'using at most six fractional digits and trimming trailing fractional zeros' docs/manual/execution-model.md
grep -Fq 'exact low and high 64-bit' docs/manual/execution-model.md
grep -Fq 'Deadline overflow never' docs/manual/execution-model.md
grep -Fq 'Omitting `process.run(timeout=...)` uses an internal absence marker' architecture_docs/decisions/0019-duration-conversion-and-timer-policy.md
grep -Fq -- '- Status: Provisional' architecture_docs/decisions/0019-duration-conversion-and-timer-policy.md
grep -Fq '0019-duration-conversion-and-timer-policy.md' architecture_docs/decisions/README.md
grep -Fq '| `random.Rng.next_int` | `next_int(lo: int64, hi: int64) -> int64`' docs/manual/api-index.md
grep -Fq 'result = rotl(s1 * 5, 7) * 9' docs/manual/randomness.md
grep -Fq 'threshold = 2^64 mod span' docs/manual/randomness.md
grep -Fq 'secure_bytes(0)' docs/manual/randomness.md
grep -Fq 'stable throughout the Aurora 0.1.x' docs/manual/randomness.md
grep -Fq '3321214725393783201' docs/manual/randomness.md
grep -Fq 'The no-clone rule is transitive.' docs/manual/randomness.md
grep -Fq '`AU3007` rejects an operation that would duplicate non-cloneable state.' docs/manual/diagnostics.md
grep -Fq 'Generic clone-safety obligations are inferred from clone-producing operations in callable bodies.' docs/manual/generics-and-traits.md
grep -Fq 'A generic-to-generic call propagates the obligation to the caller.' docs/manual/generics-and-traits.md
grep -Fq "An explicit implementation MUST NOT strengthen its trait method's clone-safety contract." docs/manual/generics-and-traits.md
grep -Fq 'Clone-safety obligations survive module imports as part of the callable contract.' docs/manual/packages.md
grep -Fq 'Task and Queue handles are clone barriers' docs/manual/randomness.md
grep -Fq 'unsafe concrete specialization' docs/manual/diagnostics.md
grep -Fq 'code: "AU3007"' crates/aurora-compiler/src/diag.rs
grep -Fq -- '- Status: Provisional' architecture_docs/decisions/0020-randomness-algorithm-and-security-boundary.md
grep -Fq '0020-randomness-algorithm-and-security-boundary.md' architecture_docs/decisions/README.md
grep -Fq '| `json.parse` | `parse(text: String) -> Result[json.Value, json.Error]` |' docs/manual/api-index.md
grep -Fq '| `json.dumps` | `dumps(value: json.Value, indent: Option[int64] = None) -> String` |' docs/manual/api-index.md
grep -Fq '`json.Value` is a move type' docs/manual/types.md
grep -Fq 'JSON input-data failures are typed `json.Error` values' docs/manual/diagnostics.md
grep -Fq 'recursive JSON parse/dump semantics' docs/manual/conformance.md
grep -Fq -- '- Status: Provisional' architecture_docs/decisions/0021-json-value-model-and-codec-policy.md
grep -Fq '0021-json-value-model-and-codec-policy.md' architecture_docs/decisions/README.md
test -s examples/json/dynamic_values.au
grep -Fq '`dynamic_values.au`' examples/README.md
grep -Fq '[21-json.md]' tutorials/README.md
grep -Fq '| `String.to_bytes` | `to_bytes() -> Vec[uint8]`' docs/manual/api-index.md
grep -Fq '| `String.from_bytes` | `from_bytes(bytes: Vec[uint8]) -> Result[String, bytes.Error]`' docs/manual/api-index.md
grep -Fq '| `bytes.base64_decode` | `base64_decode(text: String) -> Result[Vec[uint8], bytes.Error]`' docs/manual/api-index.md
grep -Fq '| `bytes.sha256_string` | `sha256_string(text: String) -> Vec[uint8]`' docs/manual/api-index.md
grep -Fq 'ordinary shared-borrow default' docs/manual/bytes.md
grep -Fq 'standard alphabet and canonical padding' docs/manual/bytes.md
grep -Fq 'InvalidHexDigit(index: int32, byte: uint8)' architecture_docs/decisions/0023-byte-vector-codecs-and-hashing-policy.md
grep -Fq -- '- Status: Provisional' architecture_docs/decisions/0023-byte-vector-codecs-and-hashing-policy.md
grep -Fq '0023-byte-vector-codecs-and-hashing-policy.md' architecture_docs/decisions/README.md
test -s examples/bytes/codecs_and_hashing.au
grep -Fq '`codecs_and_hashing.au`' examples/README.md
grep -Fq '[22-bytes.md]' tutorials/README.md
test -s examples/randomness/deterministic_rng.au
grep -Fq 'shuffle_rng.shuffle(values)' examples/randomness/deterministic_rng.au
grep -Fq '`deterministic_rng.au`' examples/README.md
test -s examples/generics/clone_safety_obligations.au
grep -Fq '`clone_safety_obligations.au`' examples/README.md
test -s examples/traits/clone_safety_contract.au
grep -Fq '`clone_safety_contract.au`' examples/README.md
grep -Fq '[20-randomness.md]' tutorials/README.md
test -s examples/concurrency/duration_arithmetic.au
grep -Fq 'Duration.minutes(-1) < 0ms' examples/concurrency/duration_arithmetic.au
grep -Fq 'Duration.seconds(2).to_ms()' examples/concurrency/duration_arithmetic.au
grep -Fq '`duration_arithmetic.au`' examples/README.md
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
grep -Fq 'NOT explicitly define or inherit a trait method whose name is a builtin member' docs/manual/generics-and-traits.md
grep -Fq 'builtin target members always retain builtin dispatch' docs/manual/generics-and-traits.md
grep -Fq 'for value in own values' docs/manual/statements.md
grep -Fq 'Queue iteration receives values' docs/manual/concurrency.md
grep -Fq 'parameter `x` is borrowed; declare it as `own String`' docs/manual/diagnostics.md
grep -Fq 'the current compiler emits at most one' docs/manual/diagnostics.md
grep -Fq '`AU3005` rejects a direct `Vec` or `Map` indexed read' docs/manual/diagnostics.md
grep -Fq '`AU3006` rejects the corresponding indexed compound' docs/manual/diagnostics.md
grep -Fq 'code: "AU3005"' crates/aurora-compiler/src/diag.rs
grep -Fq 'code: "AU3006"' crates/aurora-compiler/src/diag.rs
grep -Fq 'or: aura build -o <output>' crates/aura/src/main.rs
if grep -Fq 'aura build [-o <output>]' crates/aura/src/main.rs; then
  echo 'aura help still presents required build output as optional' >&2
  exit 1
fi
if grep -Fq '<check|run|build' crates/aura/src/main.rs; then
  echo 'aura help still advertises build through a form without required output' >&2
  exit 1
fi
grep -Fq 'Class field defaults cannot call user-defined functions' docs/manual/current-limits.md
test -s crates/aurora-compiler/tests/fixtures/check-fail/class_field_default_user_function_not_supported.au
grep -Fq 'Structured frame-list fields are deferred to Batch 3' docs/manual/current-limits.md
grep -Fq 'notes as prose rather than parse them.' docs/manual/diagnostics.md
grep -Fq -- '- Status: Accepted' architecture_docs/decisions/0014-map-literals-and-indexing.md
grep -Fq -- '- Status: Accepted' architecture_docs/decisions/0015-explicit-and-default-argument-order.md
grep -Fq -- '- Status: Accepted' architecture_docs/decisions/0017-iteration-source-selection.md
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
grep -Fq 'const MAX_FILESYSTEM_READ_BYTES: usize = 256 * 1024 * 1024;' crates/aurora-compiler/src/runtime_value.rs
grep -Fq 'const MAX_STREAM_READ_BYTES: usize = 64 * 1024 * 1024;' crates/aurora-compiler/src/runtime_value.rs
grep -Fq 'const MAX_HTTP_MESSAGE_BYTES: usize = 16 * 1024 * 1024;' crates/aurora-compiler/src/runtime_value.rs
grep -Fq 'capped at 256 MiB of remaining content' docs/manual/filesystem.md
grep -Fq 'Incoming parsed HTTP messages are capped at 16 MiB of wire data and 64 headers.' docs/manual/network.md
grep -Fq 'This stream ceiling is independent of the larger filesystem whole-read limit.' docs/manual/process.md
grep -Fq -- '- Status: Provisional' architecture_docs/decisions/0018-fixed-resource-read-limits.md
grep -Fq 'remains Provisional under ADR-0018 pending the Batch 2 checkpoint review' docs/manual/filesystem.md
grep -Fq 'remains Provisional pending the Batch 2 checkpoint review' docs/manual/network.md
grep -Fq 'remains Provisional pending the Batch 2 checkpoint review' docs/manual/control-plane.md
grep -Fq 'remains Provisional pending the Batch 2 checkpoint review' docs/manual/process.md

if rg -n '64 MiB' \
  docs/manual/filesystem.md \
  tutorials/19-io-and-networking.md \
  tutorials/14-current-language-surface.md \
  docs/learn/io-process-networking.md; then
  echo "filesystem reference still describes the retired 64 MiB whole-read limit" >&2
  exit 1
fi

if rg -ni '(http|parser|message)[^\n]*1 MiB|1 MiB[^\n]*(http|parser|message)' \
  docs/manual \
  tutorials \
  docs/learn; then
  echo "reference still describes the retired 1 MiB HTTP parser limit" >&2
  exit 1
fi

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

if rg -n 'There is no `FloorDiv`|has no `FloorDiv`|no `FloorDiv` operator trait|Duration arithmetic[^.\n]*(not implemented|unavailable)|signed 128-bit milliseconds|normalized to milliseconds|DurationLiteral\(i128\)[^.\n]*milliseconds' \
  architecture_docs \
  docs/manual \
  tutorials \
  examples; then
  echo "reference still describes the retired Duration or FloorDiv surface" >&2
  exit 1
fi

if rg -n 'secure_float' \
  architecture_docs \
  docs/manual \
  tutorials \
  examples; then
  echo "reference exposes the unapproved secure_float API" >&2
  exit 1
fi

if rg -n 'not a dynamic JSON tree|Dynamic JSON trees[^.\n]*unavailable|runtime integration[^.\n]*in progress|executable-reference integration[^.\n]*in progress|target contract rather than claiming' \
  architecture_docs/decisions/0021-json-value-model-and-codec-policy.md \
  docs/manual/json.md \
  docs/manual/control-plane.md \
  tutorials/21-json.md; then
  echo "reference still describes the implemented recursive JSON surface as unavailable or integration-only" >&2
  exit 1
fi

if rg -U -n '`//`[^\n]*has no\s+operator trait|`//` is deliberately absent' \
  docs/manual \
  tutorials; then
  echo "reference still rejects the FloorDiv extension point" >&2
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

if rg -U -n 'rejects an unconstrained clone-producing generic operation|A polymorphic\s+clone-producing operation[^.]*is rejected|`\.clone\(\)` produces an explicit independent copy of a move type|Use `get\([^`]*\)` for an explicit cloned optional read or `remove\([^`]*\)` to transfer' \
  docs/manual \
  tutorials \
  docs/learn; then
  echo "reference still describes the retired eager generic rejection or blanket clone/get behavior" >&2
  exit 1
fi

if rg -n 'maintained interpreter|tree-walk interpreter' docs/manual; then
  echo "manual still describes the removed interpreter as maintained" >&2
  exit 1
fi

python3 scripts/test_reference_integrity.py
python3 scripts/reference_integrity.py
