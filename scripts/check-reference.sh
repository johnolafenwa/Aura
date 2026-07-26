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
  tuples
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
grep -Fq 'contracts remain `int32`, including `main()` exit statuses' docs/manual/types.md
grep -Fq 'otherwise the literal defaults to `int64`' docs/manual/lexical-structure.md
grep -Fq 'otherwise it defaults to `int64`' docs/manual/static-semantics.md
grep -Fq 'assert-statement' docs/manual/grammar.md
grep -Fq 'A failed assertion is `AU4001` at the `assert` keyword location.' docs/manual/diagnostics.md
grep -Fq 'An assertion evaluates its condition exactly once.' docs/manual/execution-model.md
grep -Fq 'An `assert` condition must have exactly type `bool`.' docs/manual/static-semantics.md
grep -Fq -- '- Status: Accepted' architecture_docs/decisions/0024-assertion-evaluation-and-diagnostic-policy.md
grep -Fq '0024-assertion-evaluation-and-diagnostic-policy.md' architecture_docs/decisions/README.md
test -s examples/basics/assertions.au
grep -Fq '`assertions.au`' examples/README.md
grep -Fq '[23-assertions.md]' tutorials/README.md
test -s examples/agents/retrying_network_worker.au
grep -Fq 'random.Rng(42)' examples/agents/retrying_network_worker.au
grep -Fq 'if status != 503:' examples/agents/retrying_network_worker.au
grep -Fq 'if attempt == max_attempts:' examples/agents/retrying_network_worker.au
grep -Fq 'while total_requests < 7:' examples/agents/retrying_network_worker.au
grep -Fq 'request_with_retry(address, "/rate", "rate", 3, 4ms, rng)' examples/agents/retrying_network_worker.au
grep -Fq 'retrying_network_worker.au' README.md
grep -Fq '`retrying_network_worker.au`' examples/README.md
grep -Fq 'retrying_network_worker.au' tutorials/13-concurrency.md
grep -Fq 'retrying_network_worker.au' tutorials/19-io-and-networking.md
grep -Fq 'retrying_network_worker_runs_with_computed_backoff_on_both_backends' docs/manual/conformance.md
grep -Fq 'fn retrying_network_worker_runs_with_computed_backoff_on_both_backends()' crates/aura/tests/cli.rs
grep -Fq 'Inside an unmatched `(`, `[`, or `{`, an ordinary physical newline does not' docs/manual/lexical-structure.md
grep -Fq 'Backslash continuation is not implemented.' docs/manual/lexical-structure.md
grep -Fq 'Ordinary strings and f-strings remain single-line' docs/manual/lexical-structure.md
grep -Fq 'it does not add a trailing comma to any list form' docs/manual/grammar.md
grep -Fq -- '- Status: Accepted' architecture_docs/decisions/0025-newline-continuation-and-delimited-layout.md
grep -Fq '0025-newline-continuation-and-delimited-layout.md' architecture_docs/decisions/README.md
test -s examples/basics/multiline_expressions.au
grep -Fq '`multiline_expressions.au`' examples/README.md
grep -Fq 'examples/basics/multiline_expressions.au' README.md
grep -Fq '[24-multiline-expressions.md]' tutorials/README.md
grep -Fq 'Delimiter continuation, ignored continuation indentation' docs/manual/conformance.md
grep -Fq 'compiler bridge analyzes and completes inside continued delimiters' tools/aurora-language-server/test/compiler_bridge.test.js
grep -Fq 'Aurora newline indentation handles source delimiters' tools/vscode-aurora/test/package.test.js
grep -Fq 'tuple-expression' docs/manual/grammar.md
grep -Fq 'tuple-type' docs/manual/grammar.md
grep -Fq 'unpack-target' docs/manual/grammar.md
grep -Fq 'tuple-pattern' docs/manual/grammar.md
grep -Fq 'Unpacking a non-copy tuple consumes the whole source exactly once' docs/manual/tuples.md
grep -Fq 'Mutable-borrow iteration with a tuple target is rejected.' docs/manual/tuples.md
grep -Fq 'no empty tuple, multi-element trailing tuple comma, tuple' docs/manual/tuples.md
grep -Fq -- '- Status: Accepted' architecture_docs/decisions/0026-minimal-tuples.md
grep -Fq '## 2026-07-26 Amendment: Tuple Equality' architecture_docs/decisions/0026-minimal-tuples.md
grep -Fq '0026-minimal-tuples.md' architecture_docs/decisions/README.md
grep -Fq 'Tuple value `==` and `!=` require both operands to have the same static tuple' docs/manual/tuples.md
grep -Fq 'comparison reads the two resulting tuple values and consumes neither' docs/manual/tuples.md
grep -Fq 'Tuple ordering remains a static error.' docs/manual/tuples.md
test -s examples/basics/tuples.au
grep -Fq '`tuples.au`' examples/README.md
grep -Fq 'examples/basics/tuples.au' README.md
grep -Fq '[25-tuples.md]' tutorials/README.md
grep -Fq 'assert baseline == same' examples/basics/tuples.au
grep -Fq 'assert baseline != changed' examples/basics/tuples.au
grep -Fq 'assert same != changed' examples/basics/tuples.au
test -s crates/aurora-compiler/tests/fixtures/run-pass/tuple_structural_equality.au
test -s crates/aurora-compiler/tests/fixtures/run-pass/tuple_structural_equality.stdout
grep -Fq 'nested_with_score' crates/aurora-compiler/tests/fixtures/run-pass/tuple_structural_equality.au
grep -Fq 'generic_equal' crates/aurora-compiler/tests/fixtures/run-pass/tuple_structural_equality.au
grep -Fq 'trace_singleton' crates/aurora-compiler/tests/fixtures/run-pass/tuple_structural_equality.au
grep -Fq 'trace_text' crates/aurora-compiler/tests/fixtures/run-pass/tuple_structural_equality.au
test -s crates/aurora-compiler/tests/fixtures/check-pass/tuple_equality_contextual_literals.au
test -s crates/aurora-compiler/tests/fixtures/check-fail/tuple_ordering_rejected.au
test -s crates/aurora-compiler/tests/fixtures/check-fail/tuple_ordering_rejected.diag
test -s crates/aurora-compiler/tests/fixtures/check-fail/tuple_comparison_chain_left_borrow_rejects_later_mutation.au
test -s crates/aurora-compiler/tests/fixtures/check-fail/tuple_comparison_chain_middle_borrow_rejects_later_mutation.au
grep -Fq 'tuple ordering is not supported; use `==` or `!=`, or compare tuple elements explicitly' crates/aurora-compiler/tests/fixtures/check-fail/tuple_ordering_rejected.diag
grep -Fq 'fn tuple_equality_and_inequality_are_structural_and_non_consuming()' crates/aurora-compiler/src/sema_tests.rs
grep -Fq 'fn tuple_equality_requires_the_same_static_tuple_type()' crates/aurora-compiler/src/sema_tests.rs
grep -Fq 'fn tuple_ordering_rejects_all_four_operators_with_the_teaching_diagnostic()' crates/aurora-compiler/src/sema_tests.rs
grep -Fq 'fn tuple_value_equality_uses_elements_not_runtime_type_metadata()' crates/aurora-compiler/src/runtime_value_tests.rs
grep -Fq 'fn analysis_exposes_structural_tuple_equality_without_consuming_operands()' crates/aurora-compiler/src/analysis_tests.rs
grep -Fq 'compiler bridge exposes structural tuple equality and ordering diagnostics' tools/aurora-language-server/test/compiler_bridge.test.js
grep -Fq 'same-type recursive structural `==`/`!=`' docs/manual/conformance.md
grep -Fq 'Tuple `==` and `!=` compare same-typed values structurally and' docs/manual/status-and-compatibility.md
grep -Fq 'the executable `docs/manual/tuples.md` fence' docs/manual/conformance.md
if [[ -e crates/aurora-compiler/tests/fixtures/check-fail/tuple_equality_rejected.au ||
      -e crates/aurora-compiler/tests/fixtures/check-fail/tuple_equality_rejected.diag ]]; then
  echo "retired tuple-equality rejection fixture is still present" >&2
  exit 1
fi
grep -Fq 'conditional-expression' docs/manual/grammar.md
grep -Fq 'The condition is evaluated first, exactly once' docs/manual/expressions.md
grep -Fq 'The unselected arm performs no' docs/manual/execution-model.md
grep -Fq -- '- Status: Accepted' architecture_docs/decisions/0027-conditional-expressions.md
grep -Fq -- '- Status: Accepted' architecture_docs/decisions/0028-membership-and-comparison-chains.md
grep -Fq '0027-conditional-expressions.md' architecture_docs/decisions/README.md
test -s examples/control_flow/conditional_expressions.au
grep -Fq '`conditional_expressions.au`' examples/README.md
grep -Fq 'examples/control_flow/conditional_expressions.au' README.md
grep -Fq 'examples/control_flow/conditional_expressions.au' tutorials/04-control-flow.md
grep -Fq 'Conditional-expression precedence' docs/manual/conformance.md
grep -Fq '`value if condition else alternative`' tutorials/14-current-language-surface.md
grep -Fq 'conditional expressions' tutorials/README.md
grep -Fq 'conditional expressions' docs/manual/index.md
grep -Fq 'ADR-0027' docs/manual/status-and-compatibility.md
grep -Fq 'compiler bridge preserves conditional operands and bool diagnostics' tools/aurora-language-server/test/compiler_bridge.test.js
test -s crates/aurora-compiler/tests/fixtures/check-pass/conditional_expression_contexts.au
test -s crates/aurora-compiler/tests/fixtures/run-pass/conditional_expressions.au
test -s crates/aurora-compiler/tests/fixtures/check-fail/conditional_expression_condition_must_be_bool.au
test -s crates/aurora-compiler/tests/fixtures/check-fail/conditional_expression_arm_type_mismatch.au
test -s crates/aurora-compiler/tests/fixtures/check-fail/conditional_expression_conditional_move.au
grep -Fq -- '- Status: Accepted' architecture_docs/decisions/0029-enumerate-and-zip-loop-forms.md
grep -Fq '0029-enumerate-and-zip-loop-forms.md' architecture_docs/decisions/README.md
grep -Fq 'distinct typed binding identities' architecture_docs/decisions/0029-enumerate-and-zip-loop-forms.md
grep -Fq 'ADR-0028, and ADR-0029.' docs/manual/status-and-compatibility.md
grep -Fq 'function-wide per-loop binding-slot isolation' docs/manual/conformance.md
grep -Fq 'mut numbers = Vec[int64]()' crates/aurora-compiler/tests/fixtures/run-pass/enumerate_and_zip.au
grep -Fq 'for number, word in zip(numbers, words):' crates/aurora-compiler/tests/fixtures/run-pass/enumerate_and_zip.au
grep -Fq 'for number, word in zip(words, numbers):' crates/aurora-compiler/tests/fixtures/run-pass/enumerate_and_zip.au
grep -Fxq 'one=1' crates/aurora-compiler/tests/fixtures/run-pass/enumerate_and_zip.stdout
grep -Fxq 'two=2' crates/aurora-compiler/tests/fixtures/run-pass/enumerate_and_zip.stdout
grep -Fq 'fn every_ordinary_for_form_uses_a_fresh_scoped_target_slot()' crates/aurora-compiler/src/mir_tests.rs
grep -Fq 'for label, value in jobs:' crates/aurora-compiler/tests/fixtures/run-pass/tuple_for_pattern_queue.au
grep -Fq 'def update_first(values: borrow mut Vec[int64]) -> int64:' crates/aurora-compiler/tests/fixtures/run-pass/vec_borrow_mut_iteration.au
test "$(grep -Fxc '24' crates/aurora-compiler/tests/fixtures/run-pass/vec_borrow_mut_iteration.stdout)" -eq 3
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
grep -Fq -- '- Status: Accepted' architecture_docs/decisions/0019-duration-conversion-and-timer-policy.md
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
grep -Fq -- '- Status: Accepted' architecture_docs/decisions/0020-randomness-algorithm-and-security-boundary.md
grep -Fq '0020-randomness-algorithm-and-security-boundary.md' architecture_docs/decisions/README.md
grep -Fq '| `json.parse` | `parse(text: String) -> Result[json.Value, json.Error]` |' docs/manual/api-index.md
grep -Fq '| `json.dumps` | `dumps(value: json.Value, indent: Option[int64] = None) -> String` |' docs/manual/api-index.md
grep -Fq '`json.Value` is a move type' docs/manual/types.md
grep -Fq 'JSON input-data failures are typed `json.Error` values' docs/manual/diagnostics.md
grep -Fq 'recursive JSON parse/dump semantics' docs/manual/conformance.md
grep -Fq -- '- Status: Accepted' architecture_docs/decisions/0021-json-value-model-and-codec-policy.md
grep -Fq '0021-json-value-model-and-codec-policy.md' architecture_docs/decisions/README.md
grep -Fq 'Derived class/enum schemas and generated codecs remain deferred beyond Phase 6.' docs/manual/json.md
grep -Fq 'Derived class/enum schemas and generated codecs remain deferred beyond Phase 6.' tutorials/21-json.md
grep -Fq 'Derived class/enum schemas and generated codecs remain deferred beyond Phase 6.' architecture_docs/decisions/0021-json-value-model-and-codec-policy.md
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
grep -Fq -- '- Status: Accepted' architecture_docs/decisions/0023-byte-vector-codecs-and-hashing-policy.md
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
grep -Fq -- '- Status: Accepted' architecture_docs/decisions/0030-len-and-str-builtins.md
grep -Fq '## B3.0-d amendment and ratification' architecture_docs/decisions/0030-len-and-str-builtins.md
grep -Fq '`len(value)` and `value.len()` are the same operation with the same static' architecture_docs/decisions/0030-len-and-str-builtins.md
grep -Fq 'result type, value, and ownership behavior: both produce `int64`' architecture_docs/decisions/0030-len-and-str-builtins.md
grep -Fq '0030-len-and-str-builtins.md' architecture_docs/decisions/README.md
grep -Fq -- '- Amended: 2026-07-26 (B3.0-d int64 String length results)' architecture_docs/decisions/0004-string-semantics.md
grep -Fq '`String.len() -> int64` returns the number of Unicode scalar values' architecture_docs/decisions/0004-string-semantics.md
grep -Fq '`String.byte_len() -> int64` returns the number of' architecture_docs/decisions/0004-string-semantics.md
grep -Fq -- '- Amended: 2026-07-26 (B3.0-d codec output safety ceiling clarification)' architecture_docs/decisions/0023-byte-vector-codecs-and-hashing-policy.md
grep -Fq 'The 2026-07-26 B3.0-d amendment preserves both the exact codec destination' architecture_docs/decisions/0023-byte-vector-codecs-and-hashing-policy.md
grep -Fq 'neither narrows the public String or `Vec` length domain.' architecture_docs/decisions/0023-byte-vector-codecs-and-hashing-policy.md
grep -Fq '| `String.len` | `len() -> int64` | Counts Unicode scalar values in O(n). |' docs/manual/api-index.md
grep -Fq '| `String.byte_len` | `byte_len() -> int64` | Returns the UTF-8 byte count in O(1). |' docs/manual/api-index.md
grep -Fq '| `Vec.len` | `len() -> int64` | Element count. |' docs/manual/api-index.md
grep -Fq '| `Map.len` | `len() -> int64` | Entry count. |' docs/manual/api-index.md
grep -Fq '| `Set.len` | `len() -> int64` | Unique value count. |' docs/manual/api-index.md
grep -Fq 'so `len(value)` and `value.len()` have the' docs/manual/expressions.md
grep -Fq 'same static type and value. `String.byte_len()` likewise produces `int64`' docs/manual/expressions.md
grep -Fq 'Self::StringLen => "len() -> int64"' crates/aurora-compiler/src/call.rs
grep -Fq 'Self::StringByteLen => "byte_len() -> int64"' crates/aurora-compiler/src/call.rs
grep -Fq 'Self::VecLen => "len() -> int64"' crates/aurora-compiler/src/call.rs
grep -Fq 'Self::MapLen => "len() -> int64"' crates/aurora-compiler/src/call.rs
grep -Fq 'Self::SetLen => "len() -> int64"' crates/aurora-compiler/src/call.rs
grep -Fq 'if len(text) != text_length:' crates/aurora-compiler/tests/fixtures/run-pass/len_and_str.au
grep -Fq 'if len(values) != values_length:' crates/aurora-compiler/tests/fixtures/run-pass/len_and_str.au
grep -Fq 'if len(ages) != ages_length:' crates/aurora-compiler/tests/fixtures/run-pass/len_and_str.au
grep -Fq 'if len(tags) != tags_length:' crates/aurora-compiler/tests/fixtures/run-pass/len_and_str.au
grep -Fq 'unicode_length: int64 = unicode_text.len()' crates/aurora-compiler/tests/fixtures/run-pass/len_and_str.au
grep -Fq 'unicode_byte_length: int64 = unicode_text.byte_len()' crates/aurora-compiler/tests/fixtures/run-pass/len_and_str.au
grep -Fq 'fn len_delegates_to_the_value_and_str_renders_it()' crates/aurora-compiler/src/sema_tests.rs
grep -Fq 'fn mir_types_public_length_members_as_int64()' crates/aurora-compiler/src/mir_tests.rs
grep -Fq 'fn analysis_and_completion_report_public_length_members_as_int64()' crates/aurora-compiler/src/analysis_tests.rs
grep -Fq 'test("compiler bridge exposes all public length members as int64"' tools/aurora-language-server/test/compiler_bridge.test.js
grep -Fq '"free len(...) and the corresponding member length must have the same int64 type"' tools/aurora-language-server/test/compiler_bridge.test.js
grep -Fq '"```aurora\nlen(value: String|Vec[T]|Map[K, V]|Set[T]) -> int64\n```"' tools/aurora-language-server/test/compiler_bridge.test.js
grep -Fq 'test("compiler bridge includes Vec collection members in completions"' tools/aurora-language-server/test/compiler_bridge.test.js
grep -Fq 'test("compiler bridge includes String and Map builtin members in completions"' tools/aurora-language-server/test/compiler_bridge.test.js
grep -Fq 'test("compiler bridge includes Set collection members and MapEntry fields"' tools/aurora-language-server/test/compiler_bridge.test.js
grep -Fq 'assert.equal(details.get("len"), "len() -> int64");' tools/aurora-language-server/test/compiler_bridge.test.js
grep -Fq '"byte_len() -> int64"' tools/aurora-language-server/test/compiler_bridge.test.js
test "$(grep -Fc '"len() -> int64"' tools/aurora-language-server/test/compiler_bridge.test.js)" -ge 4
grep -Fq 'end_index = values.len() as int32' docs/manual/collections.md
grep -Fq 'end_index: int32 = items.len() as int32' tutorials/02-bindings-and-types.md
grep -Fq 'for index in range(values.len() as int32):' examples/collections/vec_polish.au
test -s crates/aurora-compiler/tests/fixtures/run-pass/vec_len_range.au
grep -Fq 'for index in range(values.len() as int32):' crates/aurora-compiler/tests/fixtures/run-pass/vec_len_range.au
grep -Fq 'fn direct_member_length_explicit_int32_cast_keeps_checked_narrowing()' crates/aurora-compiler/src/native_codegen_tests.rs
grep -Fq 'execute `int64` member lengths, `len(value) == value.len()`' README.md
grep -Fq 'checked `int64`-length to `int32`-index' README.md
grep -Fq 'the `int64` results of `String.len()`, `String.byte_len()`, `Vec.len()`' examples/README.md
grep -Fq '`Map.len()`, and `Set.len()`; `len(value) == value.len()`' examples/README.md
grep -Fq 'an explicit checked `as int32` conversion from `Vec.len()`' examples/README.md
grep -Fq '`String.byte_len()`, `Vec.len()`, `Map.len()`, and `Set.len()` all return' tutorials/README.md
grep -Fq 'host_count: int64 = hosts.len()' examples/basics/len_and_str.au
grep -Fq 'assert len(hosts) == host_count' examples/basics/len_and_str.au
grep -Fq 'byte_count: int64 = text.byte_len()' examples/basics/len_and_str.au
grep -Fq '`Vec.len()`, `Map.len()`, and `Set.len()` all return `int64`.' tutorials/02-bindings-and-types.md
grep -Fq '`len()` and therefore satisfies `len(value) == value.len()`' tutorials/14-current-language-surface.md
grep -Fq '## Lengths Are `int64`' docs/learn/collections.md
grep -Fq 'All five maintained length members return `int64`:' docs/learn/collections.md
grep -Fq 'The free builtin delegates to the member, so `len(value) == value.len()`' docs/learn/collections.md
grep -Fq 'values.insert(values.len() as int32, 40)' docs/learn/collections.md
grep -Fq 'for index in range(values.len() as int32):' docs/learn/collections.md
grep -Fq 'pub(crate) const MAX_CODEC_OUTPUT_LEN: usize = i32::MAX as usize;' crates/aurora-compiler/src/bytes_codec.rs
grep -Fq 'fn checked_codec_output_len(output_len: Option<usize>) -> Result<usize, BytesResourceError>' crates/aurora-compiler/src/bytes_codec.rs
grep -Fq 'Some(output_len) if output_len <= MAX_CODEC_OUTPUT_LEN => Ok(output_len)' crates/aurora-compiler/src/bytes_codec.rs
grep -Fq 'RequestExceedsCeiling { requested: usize, maximum: usize }' crates/aurora-compiler/src/randomness.rs
grep -Fq 'SecureRandomError::RequestExceedsCeiling' crates/aurora-compiler/src/mir_runtime.rs
grep -Fq 'SecureRandomError::RequestExceedsCeiling' crates/aurora-compiler/src/native_runtime.rs
for fixture in \
  random_secure_bytes_request_ceiling \
  random_secure_bytes_request_ceiling_i64_max; do
  test -s "crates/aurora-compiler/tests/fixtures/run-fail/${fixture}.au"
  test -s "crates/aurora-compiler/tests/fixtures/run-fail/${fixture}.diag"
  grep -Fq "\`${fixture}\`" docs/manual/conformance.md
  grep -Fq 'exceeds the secure-random request ceiling `2147483647`' \
    "crates/aurora-compiler/tests/fixtures/run-fail/${fixture}.diag"
done
grep -Fq 'fn bytes_error_index_retains_the_int32_bytes_error_payload_boundary()' crates/aurora-compiler/src/runtime_value_tests.rs
grep -Fq 'byte-codec error metadata exceeds the `bytes.Error` int32 payload range' crates/aurora-compiler/src/runtime_value.rs
grep -Fq 'byte-codec error metadata exceeds the `bytes.Error` int32 payload range' crates/aurora-compiler/src/runtime_value_tests.rs
grep -Fq 'Required malformed-data metadata above the `int32` maximum traps with' docs/manual/bytes.md
grep -Fq 'whose exact reported offset or length exceeds `2147483647` also traps with' docs/manual/bytes.md
grep -Fq 'secure-random request and resource ceiling. This ceiling bounds allocation' architecture_docs/decisions/0020-randomness-algorithm-and-security-boundary.md
grep -Fq 'or narrow the public `Vec` length domain or the result of `Vec.len()`.' docs/manual/randomness.md
grep -Fq 'independent of the public String and `Vec` length domains.' docs/manual/bytes.md
grep -Fq 'Its offsets and lengths remain `int32` as the current error-payload' docs/manual/bytes.md
grep -Fq 'Crossing this codec output/resource cap' docs/manual/current-limits.md
grep -Fq 'the public String and `Vec` length domains.' docs/manual/current-limits.md
grep -Fq 'resource and safety ceiling, independently of the public `Vec` length' docs/manual/current-limits.md

if rg -n '(byte_)?len\(\) (->|-&gt;) int32' \
  architecture_docs/decisions \
  docs/manual \
  docs/learn \
  tutorials \
  examples \
  README.md \
  examples/README.md \
  tutorials/README.md; then
  echo "maintained length surface still exposes an int32 len or byte_len result" >&2
  exit 1
fi

if rg -n -i '\b(?:len|byte_len)(?:\([^)]*\))?\b[^.\n]{0,80}\b(?:returns?|produces?|result type is|has (?:the )?(?:static )?(?:result )?type)\b[^.\n]{0,40}\bint32\b' \
  architecture_docs/decisions \
  docs/manual \
  docs/learn \
  tutorials \
  examples \
  README.md \
  examples/README.md \
  tutorials/README.md; then
  echo "maintained prose still describes len or byte_len as returning int32" >&2
  exit 1
fi

# Historical work notes and the explicitly historical language proposal retain
# superseded wording. Maintained surfaces must use operation-specific names and
# describe these numeric ceilings as resource boundaries, never as the maximum
# representable Vec/collection size.
if rg -n 'MAX_(VEC|VECTOR|COLLECTION)(_OUTPUT)?_(LEN|LENGTH|SIZE)|checked_(vec|vector|collection)(_output)?_(len|length|size)|SecureRandomError::(LengthTooLarge|RequestTooLarge)|Self::(LengthTooLarge|RequestTooLarge)|BytesResourceError::((Vec|Vector|Collection)(Length|Output)?TooLarge)' \
  crates/aurora-compiler/src \
  crates/aurora-compiler/tests; then
  echo "retired collection-limit implementation names remain in maintained code" >&2
  exit 1
fi

if rg -U -n -i 'maximum (representable )?(Vec|collection) (length|size)|(?:maximum|largest)[^.\n]{0,80}(?:Vec|collection)[^.\n]{0,50}(?:length|size)|(?:Vec|collection) (?:length|size)[^\n]{0,100}(?:is |are )?(?:capped|limited|bounded) (?:at|to|by) (?:2,147,483,647|2147483647)|(?:Vec|collection)[^.\n]{0,50}(?:length|size)[^.\n]{0,80}(?:i32::MAX|int32 maximum)|(?:2,147,483,647|2147483647)[^\n]{0,80}(?:maximum|representable)[^\n]{0,50}(?:Vec|collection)' \
  architecture_docs/decisions \
  docs/manual \
  docs/learn \
  tutorials \
  examples \
  README.md \
  examples/README.md \
  tutorials/README.md; then
  echo "maintained reference still derives a Vec or collection-length limit from a resource ceiling" >&2
  exit 1
fi
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
grep -Fq 'An `impl` targeting any builtin type MUST' docs/manual/generics-and-traits.md
grep -Fq 'a collision' docs/manual/generics-and-traits.md
grep -Fq 'does not collide still implements and dispatches normally on a builtin target' docs/manual/generics-and-traits.md
grep -Fq 'NOT explicitly define or inherit a trait method whose name is a builtin member' docs/manual/generics-and-traits.md
grep -Fq 'builtin target members always retain builtin dispatch' docs/manual/generics-and-traits.md
grep -Fq 'for value in own values' docs/manual/statements.md
grep -Fq 'Queue iteration receives values' docs/manual/concurrency.md
grep -Fq 'parameter `x` is borrowed; declare it as `own String`' docs/manual/diagnostics.md
grep -Fq 'the current compiler emits at most one' docs/manual/diagnostics.md
grep -Fq 'constant tuple indexing that selects a non-copy element' docs/manual/diagnostics.md
grep -Fq 'corresponding `Vec` or `Map` indexed compound assignment' docs/manual/diagnostics.md
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
grep -Fq -- '- Status: Accepted' architecture_docs/decisions/0031-cli-backend-defaults.md
grep -Fq '0031-cli-backend-defaults.md' architecture_docs/decisions/README.md
grep -Fq '`aura run --backend mir` executes the lowered MIR and is the default.' docs/manual/cli-and-tooling.md
grep -Fq 'run_backend_parsing_defaults_to_mir_and_accepts_every_selector' crates/aura/src/main.rs
grep -Fq 'Structured frame-list fields are deferred to Batch 3' docs/manual/current-limits.md
grep -Fq 'notes as prose rather than parse them.' docs/manual/diagnostics.md
grep -Fq -- '- Status: Accepted' architecture_docs/decisions/0014-map-literals-and-indexing.md
grep -Fq -- '- Status: Accepted' architecture_docs/decisions/0015-explicit-and-default-argument-order.md
grep -Fq -- '- Status: Accepted' architecture_docs/decisions/0016-retained-noncopy-expression-borrows.md
grep -Fq -- '- Status: Accepted' architecture_docs/decisions/0017-iteration-source-selection.md
grep -Fq 'Checkpoint disposition (historical): ADR-0014 through ADR-0017 are Accepted.' work/task-board.md
grep -Fq 'retained_receiver_nested_consumption_repro' docs/manual/conformance.md
grep -Fq 'retained_argument_nested_consumption_repro' docs/manual/conformance.md
grep -Fq 'method_receiver_rejects_nested_argument_consumption' docs/manual/conformance.md
grep -Fq 'retained_parameter_rejects_nested_argument_consumption' docs/manual/conformance.md
for fixture in \
  retained_receiver_nested_consumption_repro \
  retained_argument_nested_consumption_repro \
  method_receiver_rejects_nested_argument_consumption \
  retained_parameter_rejects_nested_argument_consumption; do
  test -s "crates/aurora-compiler/tests/fixtures/check-fail/${fixture}.au"
  test -s "crates/aurora-compiler/tests/fixtures/check-fail/${fixture}.diag"
done
for namespace in io fs net process bytes json sys path toml log trace metrics random; do
  grep -Fq -- "- \`${namespace}\`" tutorials/14-current-language-surface.md
done
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
grep -Fq 'Filesystem one-shot reads and `fs.File` whole-file reads are capped at 256 MiB' docs/manual/current-limits.md
grep -Fq 'Process-pipe and captured-output reads plus TCP, Unix, and TLS whole/bounded reads remain capped at 64 MiB.' docs/manual/current-limits.md
grep -Fq 'Incoming HTTP parsing accepts at most 64 headers and 16 MiB of wire data per message' docs/manual/current-limits.md
grep -Fq 'Each `process.run` captured stream and each whole-pipe read is capped at 64 MiB' docs/manual/process.md
grep -Fq 'Whole TCP text reads, TCP line reads, and individual byte-count reads are capped at 64 MiB' docs/manual/network.md
grep -Fq 'Incoming parsed HTTP messages are capped at 16 MiB of wire data and 64 headers.' docs/manual/network.md
grep -Fq 'This stream ceiling is independent of the larger filesystem whole-read limit.' docs/manual/process.md
grep -Fq 'one-shot and `fs.File` whole-file reads are capped at 256 MiB' tutorials/14-current-language-surface.md
grep -Fq 'capped at 64 MiB; TLS certificate, private-key, and CA-file loading uses the' tutorials/14-current-language-surface.md
grep -Fq 'incoming HTTP parsing is capped at 16 MiB of wire data per message' tutorials/14-current-language-surface.md
grep -Fq 'One-shot `fs.read_to_string` and `fs.read_bytes` are capped at 256 MiB.' docs/learn/io-process-networking.md
grep -Fq -- '- Status: Accepted' architecture_docs/decisions/0018-fixed-resource-read-limits.md
grep -Fq 'fixed 256 MiB whole-read policy is accepted under ADR-0018' docs/manual/filesystem.md
grep -Fq 'fixed resource-cap policy recorded by ADR-0018 is Accepted' docs/manual/network.md
grep -Fq 'cap is Accepted under ADR-0018' docs/manual/control-plane.md
grep -Fq 'fixed stream-cap policy recorded by ADR-0018 is Accepted' docs/manual/process.md

if rg -U -n -i '(filesystem|fs\.File|whole[- ]file reads?|file reads?)[^.\n]{0,120}(?:is |are )?(?:capped|limited|bounded) (?:at|to) 64 MiB|64 MiB (?:filesystem|fs\.File|whole[- ]file|whole[- ]read|file-read) (?:cap|ceiling|limit)' \
  docs/manual \
  tutorials \
  docs/learn; then
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

if rg -U -n 'Newlines are not continuation|ordinary calls remain on one\s+physical line|Collection literals[^.]*remain on one\s+physical line|Because general delimiter continuation does not exist|only maintained multiline accommodation inside a surrounding delimiter|general multiline\s+continuation is unavailable|general multiline literals are\s+unavailable|general\s+delimiter-based line continuation is unavailable|general multiline delimiters' \
  docs/manual \
  tutorials \
  docs/learn; then
  echo "reference still describes delimiter continuation as unavailable" >&2
  exit 1
fi

if rg -n 'expressions do not include tuples|tuples, callable types|Callable, closure, tuple|tuples and destructuring|Destructuring assignment or loop targets|detached spawn, tuples, attributes|tuple punctuation' \
  docs/manual \
  tutorials; then
  echo "reference still describes the implemented tuple kernel as unavailable" >&2
  exit 1
fi

# Historical work notes intentionally retain the earlier provisional boundary.
if rg -U -n -i 'identity rule does not add tuple value|does not add tuple equality|tuple equality or ordering|tuple equality, ordering|methods, equality, ordering|equality/order rejection|Provisional\s+under ADR-0026|provisional\s+extent recorded by ADR-0026|ADR-0026[^.\n]*Provisional|These choices remain Provisional|^- Status: Provisional$' \
  architecture_docs/decisions/0026-minimal-tuples.md \
  docs/manual \
  tutorials \
  examples \
  README.md \
  examples/README.md \
  tutorials/README.md; then
  echo "maintained reference still describes tuple equality as rejected or ADR-0026 as provisional" >&2
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
