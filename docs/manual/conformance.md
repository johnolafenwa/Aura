# Conformance

Aurora keeps the language reference and implementation aligned through executable conformance layers. This page identifies which tests substantiate each part of the specification and what a conforming implementation is expected to do.

## Conforming Programs And Implementations

A **conforming Aurora program** uses only syntax and APIs defined by this Manual and satisfies all static rules.

A **conforming Aurora implementation**:

- accepts every conforming program within documented implementation limits
- rejects programs that violate a MUST-level lexical, grammatical, name, type, ownership, or entrypoint rule
- preserves the observable evaluation and cleanup behavior defined by the Manual
- produces the specified typed outcomes or runtime failures
- provides the maintained public API surface
- does not expose proposal-only constructs as accepted 0.1 language features

Exact diagnostic prose is normative only where a fixture or this Manual explicitly requires it. A conforming implementation otherwise needs a clear diagnostic with an accurate source location and the same stable `AU####` code; message wording may differ without changing that code's documented meaning.

## Executable Reference Map

| Reference area | Primary executable evidence |
| --- | --- |
| UTF-8, indentation, tokens, literals, escapes | `crates/aurora-compiler/src/lexer_tests.rs` |
| Delimiter continuation, ignored continuation indentation, expression-match layout islands, trailing-comma/backslash/single-line-string boundaries, and pairing diagnostics | focused lexer/parser tests; `newline_continuation*` and delimiter parse/run fixtures; `examples/basics/multiline_expressions.au`; compiler-bridge and extension indentation tests; and the MIR/direct parity matrix |
| Parenthesized tuple values/types, recursive assignment/loop targets and patterns, function returns, left-to-right capture, recursive Copy, whole-source non-Copy moves, shared leaf provenance, copy-only constant indexing, canonical rendering, equality/order rejection, and mutable-writeback rejections | focused lexer/parser/sema/MIR/native/runtime tests; `tuple_*` parse/check/run fixtures; `examples/basics/tuples.au`; the executable `docs/manual/tuples.md` fence; language-server tests; and the MIR/direct parity matrix |
| grammar and parser limits | `crates/aurora-compiler/src/parser_tests.rs`, `tests/fixtures/parse-pass`, `tests/fixtures/parse-fail` |
| Conditional-expression precedence, exact-bool conditions, arm unification/context, lazy condition-first selection, conservative branch moves, analysis, and backend parity | focused conditional parser/sema/MIR/analysis tests; `conditional_expression_*` check/parse fixtures; `conditional_expressions` run fixture and example; compiler-bridge coverage; and the MIR/direct parity matrix |
| Membership containers and delegation, chained-comparison precedence, at-most-once operand evaluation, short-circuiting, conservative chain checking, and backend parity | `comparison_chains_keep_every_operator_at_one_precedence_level`, `membership_tests_read_supported_containers_and_reject_the_rest`, and `comparison_chains_evaluate_each_operand_once_and_short_circuit`; `membership_*` check-fail and `membership_and_comparison_chains` check-pass fixtures; the `membership_and_comparison_chains` run fixture and example; the five retired python-hint acceptance fixtures; and the MIR/direct parity matrix |
| `enumerate`/`zip` loop-form recognition, operand domain, borrow default, `int64` positions, shortest-operand `zip` termination, shadowing, function-wide per-loop binding-slot isolation under heterogeneous binding-name reuse, and backend parity | `enumerate_and_zip_iterate_in_lockstep_over_the_bare_loop_default`, `heterogeneous_ordinary_for_bindings_use_distinct_scoped_typed_slots`, `every_ordinary_for_form_uses_a_fresh_scoped_target_slot`, and `ordinary_for_target_scope_starts_after_iterable_evaluation`; `enumerate_requires_indexable_iterable` and `zip_rejects_ownership_modifiers` check-fail fixtures; the reversed heterogeneous `enumerate_and_zip` fixture; `tuple_for_pattern_queue` for recursive target reuse; `vec_borrow_mut_iteration` for fallthrough/`continue`/`break`/explicit-return writeback; the maintained example; and the MIR/direct parity matrix |
| `len` delegation domain and `int64` result, `str` rendering equality with `print` and f-strings, and the reservation of both names | `len_delegates_to_the_value_and_str_renders_it`; the `len_requires_a_len_member` check-fail fixture; the `len_and_str` run fixture and example; the two retired python-hint acceptance fixtures; and the MIR/direct parity matrix |
| names, types, calls, traits, patterns, moves, and borrows | `crates/aurora-compiler/src/sema_tests.rs`, `tests/fixtures/check-pass`, `tests/fixtures/check-fail` |
| integer `/` rejection, floor division/remainder, exact float-context integer literals, `.to_float()`, and shortest-roundtrip float printing | lexer/parser/integer/runtime-value unit tests plus `integer_true_division_*`, `floor_division_and_modulo`, `float_context_integer_literals`, `integer_to_float_rounding`, and `float_shortest_roundtrip_printing` fixtures |
| signed-i128-nanosecond Duration literals, exact two-limb direct ABI, constructors, checked arithmetic, `FloorDiv`, comparison, conversion, rendering, and invalid host timers | `duration_literals_scale_to_nonnegative_i128_nanoseconds_at_each_unit_boundary`, parser/MIR/native-codegen/runtime unit tests, `docs/manual/concurrency.md#aurora-1`, Duration run/failure fixtures, native-runtime FFI tests, and the MIR/direct parity matrix |
| deterministic xoshiro256** seeding/output, unbiased half-open integers, 53-bit floats, Fisher-Yates writeback, identity/rendering, direct and transitive no-clone ownership, inferred generic and trait clone-safety contracts, and OS-secure integer/byte boundaries | `src/randomness.rs`; `random_rng_clone_safety_defers_generic_obligations_to_use_sites`, `imported_rng_clone_obligations_and_qualified_wrapper_identity_survive_namespaces`, and focused trait/operator/`From` semantic tests; `random_deterministic_sequences`, `random_projected_shuffle`, `random_identity_and_render`, `random_transitive_clone_rejected`, `random_secure_smoke`, `random_invalid_*`, and `random_secure_allocation_failure` fixtures; verified clone-safety examples in `docs/manual/generics-and-traits.md`; native-runtime FFI and language-server tests; and the MIR/direct parity matrix |
| `Vec[uint8]` bytes, strict UTF-8 conversion, lowercase/mixed-case hex, canonical padded base64, typed malformed-input offsets, raw SHA-256, shared inputs, and output-size preflights | `src/bytes_codec.rs` unit tests; `bytes_codecs_and_hashing`, `bytes_typed_errors`, and reserved-encoding fixtures; `examples/bytes/codecs_and_hashing.au`; the executable `docs/manual/bytes.md` fence; language-server tests; allocation-boundary tests; and the MIR/direct parity matrix |
| Assertions: exact operand types, once-only condition, lazy once-only message, exact default/custom/empty/whitespace text, `AU4001` keyword span, operand and cleanup precedence, top-level scripts, and no stripping | `assert_*` parse/check/run fixtures and compiler unit tests; assertion CLI tests for forced MIR/direct execution and file-level `aura test`; `examples/basics/assertions.au`; the executable `docs/manual/assertions.md` fence; language-server and extension packaging tests; and the MIR/direct parity matrix |
| Application-level HTTP retry composition: retry only `503`, deterministic seed-42 jitter, exponential `Duration` backoff, final-attempt no-RNG/no-sleep behavior, explicit deadlines, and scoped resource cleanup | `examples/agents/retrying_network_worker.au` and `retrying_network_worker_runs_with_computed_backoff_on_both_backends` in `crates/aura/tests/cli.rs`, which pin the exact seven-request loopback trace on the MIR and forced-direct backends |
| recursive JSON parse/dump semantics, exact numeric classification, typed parse errors, deterministic formatting, accessors, ownership, and resource limits | JSON codec/runtime-value unit tests, including exact materialized-node boundaries and deterministic allocation-failure injection; `json_dynamic_values`, JSON ownership and run-fail fixtures; `examples/json/dynamic_values.au`; the executable `docs/manual/json.md` fence; language-server tests; and the MIR/direct parity matrix |
| Map duplicate-key replacement, key-before-value effects, indexed-read/simple-write ownership, and missing-key traps | `map_literal_duplicate_keys`, `map_index_non_copy_requires_explicit_clone`, `map_index_assignment_consumes_noncopy_key`, and `map_index_missing_key` fixtures plus the MIR/native parity matrix |
| Supplied/default order and named enum-argument source order with declaration-slot binding | `explicit_and_default_argument_order` plus the MIR/native parity matrix |
| Copy-value capture, immediate f-string rendering, and receiver-before-argument effects | `left_to_right_value_snapshotting` plus the MIR/native parity matrix |
| Compound binary dispatch for root/projected targets, copy-target capture, retained non-copy `AU3002`, and copy-only Vec/Map indexed targets | `operator_traits`, `left_to_right_value_snapshotting`, `compound_noncopy_target_rejects_rhs_mutation`, `vec_compound_assignment_noncopy_element_rejected`, and `map_compound_assignment_noncopy_value_rejected` fixtures plus the MIR/native parity matrix |
| Dedicated `AU3005`/`AU3006` indexed ownership codes, `AU3003` mutable-receiver classification, and `AU2005` String-constructor guidance | `vector_index_non_copy_requires_explicit_clone`, `map_index_non_copy_requires_explicit_clone`, `vec_compound_assignment_noncopy_element_rejected`, `map_compound_assignment_noncopy_value_rejected`, `immutable_mutating_method`, and `string_constructor_not_supported` fixtures plus the compiler-bridge tests |
| Current class-field-default callable limit | `class_field_default_user_function_not_supported` fixture |
| Retained non-copy binary/index/method-receiver/call-argument/indexed-assignment borrows, nested-consumption containment, `AU3002` overlap rejection, and no hidden deep clone | `binary_left_borrow_rejects_later_mutation`, `projected_binary_left_borrow_rejects_later_mutation`, `index_base_borrow_rejects_index_mutation`, `indexed_assignment_target_rejects_index_mutation`, `method_receiver_borrow_rejects_nested_argument_mutation`, `retained_receiver_nested_consumption_repro`, `retained_argument_nested_consumption_repro`, `method_receiver_rejects_nested_argument_consumption`, and `retained_parameter_rejects_nested_argument_consumption` fixtures |
| Declaration-stable call/operator passing, directional exclusive-access checks, and the distinct task-capture boundary | `generic_borrow_specialization_retains_copy_argument`, `call_borrow_mut_then_copy_read_rejected`, `call_own_then_projected_copy_read_rejected`, `trait_operator_borrow_mut_receiver_requires_mutable`, `trait_operator_copy_left_retains_borrow`, `trait_operator_own_receiver_moves_value`, `trait_operator_own_receiver_rejects_rhs_read`, `trait_operator_own_rhs_moves_value`, `trait_unary_operator_own_receiver_moves_value`, `operator_trait_value_receiver_snapshot`, `task_capture_snapshots_copy_arguments`, and `task_group_receiver_rejects_owned_variadic_capture` fixtures plus the MIR/native parity matrix |
| Accepted ADR-0017 one-time Vec/Set own-iteration selection and Queue handle capture without source-binding retargeting | `own_iteration_captures_collection`, `queue_iteration_captures_handle`, and the MIR/native parity matrix |
| Queue receive-item ownership and rejected explicit iteration modifiers | `queue_bare_iteration_ownership`, `queue_own_iteration_rejected`, `queue_borrow_iteration_rejected`, and `queue_borrow_mut_iteration_rejected` fixtures |
| Builtin trait-method no-shadowing across every builtin target, inherited-default containment, and direct builtin precedence | `builtin_queue_trait_method_collision`, `builtin_task_inherited_trait_method_collision`, `builtin_task_group_trait_method_collision`, `builtin_vec_trait_method_collision`, `builtin_string_trait_method_collision`, and `builtin_file_trait_method_collision` fixtures plus `builtin_method_names_cannot_be_shadowed_on_any_builtin_target` and `direct_backend_prefers_builtin_handle_member_if_collision_reaches_mir` |
| Fixed 256 MiB filesystem, 64 MiB stream/TLS-configuration, and 16 MiB incoming HTTP limits | injectable-limit and sparse-file tests in `src/runtime_value_tests.rs` plus MIR/forced-direct filesystem and HTTP tests in `crates/aura/tests/cli.rs` |
| module and package resolution | `crates/aurora-compiler/tests/modules.rs`, `tests/packages.rs`, `src/package_tests.rs` |
| MIR semantics and runtime behavior | `src/mir_tests.rs`, `src/mir_runtime_tests.rs`, `tests/fixtures/run-pass`, `tests/fixtures/run-fail` |
| native semantics and resource ABI | `src/native_codegen_tests.rs`, `src/native_runtime_tests.rs`, `tests/native_runtime_ffi.rs` |
| MIR/native observable equivalence | `crates/aura/tests/backend_parity.rs` |
| CLI, entrypoints, diagnostics, and installed builds | `crates/aura/tests/cli.rs`, `crates/aura/tests/packages.rs` |
| analysis, completion, hover, definitions, invalidation | `tools/aurora-language-server/test` |
| maintained examples | compiler example smoke tests and CLI product tests |

The exact repository gate is `npm run ci`. It runs formatting, Rust tests, backend parity, language-server and extension tests, compiler and LSP coverage gates, this reference check, the documentation build, dependency audits, Clippy with warnings denied, and repository hygiene.

## Fixture Classes

The compiler fixture directories have distinct contracts:

- `parse-pass`: source MUST form a valid AST; later static checking is not implied.
- `parse-fail`: source MUST be rejected during lexing or parsing with the stored diagnostic.
- `check-pass`: source MUST parse and satisfy the static semantics.
- `check-fail`: source MUST parse and then be rejected by static checking with the stored diagnostic.
- `run-pass`: source MUST check and produce the stored standard output through the maintained execution path.
- `run-fail`: source MUST check far enough to reach the intended runtime failure and produce the stored diagnostic behavior.

Regression tests supplement fixtures when a case needs multiple files, temporary packages, local sockets, processes, timing, cancellation, or comparison of execution backends.

## Backend Equivalence

Aurora 0.1 has two maintained semantic runtime representations:

- `aura run` lowers checked source to MIR and executes it in the MIR runtime.
- `aura build --backend direct` lowers checked source to native code through the direct backend and links the native runtime.
- the default `aura build --backend auto` first attempts direct emission and may instead build a native launcher containing serialized MIR plus the MIR runtime.

For the maintained source subset, the paths MUST agree on:

- standard output and integer exit status
- return values and pattern results
- checked arithmetic and collection failures
- move/borrow-sensitive mutation and writeback
- `with` cleanup order and primary runtime diagnostics
- task, queue, cancellation, process, filesystem, and network outcomes within platform constraints

The parity matrix executes every eligible runtime fixture through both paths. A fixture may be excluded only through the explicit exclusion list, with a reason that corresponds to an intentional harness or platform boundary rather than an unexplained semantic divergence.

## Documentation Conformance

Reference changes are checked by `npm run check:reference`. The gate retains
the normative-page, navigation, grammar-anchor, execution-order,
migration-wording, and deleted-evaluator guards. It inventories every fenced
block in `docs/manual`. Fences labeled `aurora` or the historically used
`python` label are Aurora source; Bash, EBNF, JSON, text, TOML, and any future
fence language still require an explicit contract.

Every fenced block has a source-hash-pinned contract in
`scripts/reference-integrity.json`:

| Contract | Gate behavior |
| --- | --- |
| `check` | extract the exact block and require `aura check` to succeed with the pinned output |
| `run` | extract the exact block and require `aura run` to produce the exact pinned standard output and standard error |
| `check-fail` | require rejection with the pinned exit status and diagnostic fragment |
| `package-check` | place the exact Aurora block in a metadata-pinned local package layout and require `aura check` to succeed without network access |
| `command` | parse one exact Bash command without a shell and execute only the gate's allowlisted side-effect-free `aura check`/`aura run` form for a maintained `examples/*.au` path, with pinned output |
| `illustrative` | do not execute the block; require a specific reason explaining why it is notation, output, a dependent fragment, an unsafe command, or otherwise not a standalone executable unit |

The command contract never invokes a shell, follows pipes or continuations, or
runs build, network, dependency-update, server, or recursive repository-gate
commands. A documented `cargo run -p aura -- ...` prefix is normalized to the
already-built `aura` binary before the allowlisted subcommand runs. The proof is
therefore about the displayed Aurora CLI behavior, not Cargo itself. Unsafe or
orchestration-only command blocks remain illustrative with their boundary
stated explicitly.

The source hash makes changes fail closed: editing, replacing, or reordering
fenced blocks requires an explicit review of their contracts. Adding a Manual
page also requires classifying it as a feature page or as a structural page
with a reason. Structural pages organize cross-cutting contracts. Every
feature page MUST contain these non-empty level-two sections:

- `Grammar`
- `Typing Rules`
- `Runtime Semantics`
- `Ownership And Evaluation Order`
- `Diagnostics`
- `Backend Support`
- `Limits And Implementation-Defined Behavior`
- `Status`

The `Diagnostics` section MUST name each applicable stable `AU` code. If a feature introduces no feature-specific diagnostic, it states exactly `No feature-specific diagnostics.` instead. This is an explicit audited claim, not permission to omit general diagnostics that apply to examples on the page.

Every feature page MUST also contain at least one verified fenced example in a
non-illustrative mode. A page cannot satisfy that rule with a stale source hash
or with an explanation-only fragment. This ensures that all current feature
chapters have a live compiler, package, or safe CLI proof rather than
relying only on prose.

The gate reports the total page and all-language fence inventory,
verified-versus-illustrative counts, per-page example counts, every missing
normative section, and every feature page without a verified example before
failing. Its focused Python tests pin all-language fence extraction,
stale-metadata rejection, illustrative-reason enforcement, the feature-section
and executable-example contracts, safe command/package preparation, and
compiler outcome matching.

The documentation build separately checks links and rendering. Language-facing changes still require compiler fixtures or maintained examples as directed by `AGENTS.md`; a checked Manual block proves the documented example's stated outcome, not every edge of the underlying rule.

## Adding Or Changing A Rule

A language or tooling behavior change is complete only when the same pass updates, where relevant:

1. a failing compiler, runtime, CLI, or LSP test
2. the implementation
3. the normative Manual page and grammar when syntax changes
4. the API Index when public APIs change
5. Current Limits when a boundary is added or removed
6. categorized examples and Learn/tutorial material
7. the task board and dated work note

Syntax expansion is frozen for the 0.1 hardening cycle. A new construct therefore needs an explicit compatibility decision rather than being accepted solely because it is easy to parse.

## Deriving A Book

A book may treat this reference as its factual source. It may introduce concepts in a different order, add motivation, diagrams, exercises, and larger examples, or omit advanced details from early chapters. It must preserve these constraints:

- every taught syntax form appears in the complete grammar
- every claimed type or ownership behavior agrees with the static semantics
- every runtime/API claim links back to a maintained contract
- proposal-only features are labeled as future design, not current Aurora
- examples are compiled or run as part of the maintained repository surface

This division lets the reference remain precise while the book remains readable.
