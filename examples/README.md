# Aura Examples

This directory holds runnable Aura programs, grouped by topic. They serve as
quick references and as a companion to the chapters in `tutorials/`.

Every table lists the file, what it shows, and what it prints. Run any example
from the repository root with `cargo run -p aura -- run <path>`. See
[Run Examples](#run-examples) for the full command list.

Aura has two backends. The MIR backend runs Aura's mid-level intermediate
representation, and the direct backend compiles native code.

## Categories

### `basics/`

| File | Shows | Prints |
| --- | --- | --- |
| `top_level_script.au` | Top-level executable statements with inferred bindings | `156` |
| `main_function.au` | `main` with an omitted `None` return type | `16` |
| `none_values.au` | Bare `None` as both the unit type and the unit value | `1` |
| `mutable_bindings.au` | `mut`, reassignment, and compound assignment | `5` |
| `numbers.au` | Signed integer floor division, divisor-sign remainder, exact float-context integer literals, floating true division through integer `.to_float()`, exact `as float64`, and shortest-roundtrip printing at the rounded `2^53 + 1` conversion boundary | `2`, `-3`, `2`, `-3`, `-2`, `3.5`, `2.0`, `true`, `true`, `42.0`, `9007199254740992.0` |
| `named_arguments.au` | Named arguments on functions, instance methods, and associated methods, including an explicit `own` constructor parameter | `hello, aura`, `7` |
| `named_builtin_arguments.au` | Named arguments on supported builtins such as `print(...)` and `range(...)` | `10` |
| `default_arguments.au` | Shared-borrow default parameter values, evaluated fresh on each call | `hello world`, `hello aura`, `6`, `12` |
| `function_values.au` | Capture-free named function values in bindings, parameters, fields, and `list`. Covers copy semantics, explicit generic specialization, and a statically known indirect call that uses a default and a named argument. Also shows explicit `def(mut T) -> R` and `def(own T) -> R` contracts through fields and `list`. | `2`, `3`, `6`, `5`, `5`, `12`, `11`, `21`, `2`, `owned` |
| `closures.au` | Contextually typed expression lambdas with a Copy snapshot, a repeatable read-only non-Copy capture, and a consuming single-use capture | `42`, `12`, `4`, `4`, `owned` |
| `views.au` | Shared and mutable local views, returned view provenance, mutable reborrowing, inferred final-use release, fixed tuple-place identity, and a mutable-repeatable loan closure | `Ada`, `2`, `9`, `(10, 21)`, `(11, 32)` |
| `borrow_parameters.au` | Free-function bare shared and `mut` parameters with mutation the caller can see | `41`, `42`, `42` |
| `copy_field_returns.au` | Owned `int32` copies returned from shared class input | `7`, `7` |
| `copy_return_selection.au` | Choosing and forwarding owned Copy values | `7` |
| `pass_keyword.au` | The `pass` no-op statement in empty classes and functions | `0` |
| `assertions.au` | Comparison and membership assertions whose failure diagnostics capture both operands. Default and custom assertion statements on exact boolean conditions. Runs only the passing path. | `checking`, `all assertions passed` |
| `multiline_expressions.au` | A function signature, grouped arithmetic, calls, list literals, and a dictionary literal split across physical lines inside `()`, `[]`, and `{}`. The no-trailing-comma and single-line-string rules still apply. | `80`, `20` |
| `len_and_str.au` | The `int64` results of `str.len()`, `str.byte_len()`, `list.len()`, `dict.len()`, and `set.len()`. Shows `len(value) == value.len()`, Unicode scalar length versus UTF-8 byte length, and `str(value)` producing the print rendering. | `2`, `4`, `2`, `[alpha, beta]` |
| `tuples.au` | Fixed tuple values and return types, whole-source unpacking, copy-only constant indexing, tuple-target iteration, recursive tuple patterns, and same-type recursive `==` and `!=` that keep both operands. Ordering stays rejected. | `Aura`, `7`, `20`, `ready:2`, `done:3`, `3`, `true` |

`assertions.au` is also an `aura test` module. It has one ordinary case, two
registered labeled cases, and per-case setup and teardown output. Run
`aura test --format json -k '[unicode]' examples/basics/assertions.au` to see
schema-versioned output and filtering after registration.

`basics/hello_world.au` prints a single line. It is the baseline for
executable size.

### `collections/`

| File | Shows | Prints |
| --- | --- | --- |
| `comprehensions.au` | Eager owned list, set, and dictionary comprehensions with filters, nested outer-major clauses, target-local scope, and ordinary bare-loop ownership | `[1, 4, 9, 16]`, `[4, 16]`, `{3: 30, 4: 40}`, `[11, 12, 21, 22]` |
| `slices.au` | Owned list slices and Unicode-scalar `str` slices with every omitted-endpoint form, negative endpoints, and source/result independence | `[20, 30]`, `[10, 20]`, `[30, 40]`, `[10, 20, 30, 40]`, `[10, 20, 30, 40]`, `[99, 30]`, `🎉`, `A🎉`, `🎉Z`, `A🎉Z`, `A🎉Z` |
| `list_basics.au` | List literals, indexed reads, `list[T]` methods, and indexed mutation through `set(...)` | `3`, `1`, `2`, `2`, `20`, `1`, `99`, `false` |
| `list_iteration.au` | Empty-list construction with `list[T]()`, `extend(...)`, explicit `list[T]` annotations, bare shared iteration, and consuming `own` iteration | `Ada`, `Grace`, `2`, `9` |
| `list_polish.au` | Negative direct and method indexes, cloned reads of non-copy values, `mut` iteration, cast-free `list.len()` with `range(...)`, `insert(...)`, `swap(...)`, `reverse()`, `extend(...)`, `clear()`, and list equality | `Ada`, `Grace`, `true`, `4`, `1`, `14`, `13`, `12`, `11`, `true`, `100`, `true`, `true` |
| `list_algorithms.au` | Eager shared `map` and `filter`, stable natural sorting, stable key sorting that calls the key once per element, and source retention | `[6, 2, 4, 8]`, `[2, 4]`, `[1, 2, 3, 4]`, `[4, 3, 2, 1]`, `second`, `first`, `third`, `[3, 1, 2, 4]` |
| `dict_basics.au` | `dict[K, V]` literals, `update(...)`, tuple-valued `items()`, indexed writes, indexed reads, and typed optional lookup and removal | `3`, `true`, `1`, `1`, `5`, `(aura, 5)`, `(repo, 3)`, `3`, `3`, `3`, `true` |
| `lookup_in_place.au` | Arm-scoped `lookup` on lists and dictionaries: a mutable `Found` view that appends in place, the `Missing` arm for an invalid position or absent key, and a shared read | `no team at 2`, `core: 1`, `2`, `1`, `1` |
| `set_basics.au` | `set[T]` literals, shared-borrow iteration, deduplication, and the set method surface | `3`, `true`, `false`, `true`, `true`, `9`, `true`, `true`, `1` |

### `classes/`

| File | Shows | Prints |
| --- | --- | --- |
| `point_distance.au` | Class fields, member access, functions, and `float64.sqrt()` | `5.0` |
| `default_fields.au` | Field default values and keyword construction | `localhost`, `8080` |
| `methods.au` | Shared `self` methods, the explicit `self` synonym, and associated methods | `4`, `8`, `0` |
| `mutating_methods.au` | `mut self`, field mutation, and compound assignment through `self` | `6`, `1` |
| `copy_class.au` | `copy class` for explicit copy semantics on fully copyable fields | `1`, `2` |
| `indirect_recursive.au` | Recursive fields with `indirect Node \| None` and optional children | `2` |
| `positional_constructors.au` | Positional class constructor arguments with optional trailing named fields | `1`, `2`, `7`, `9` |

### `agents/`

| File | Shows | Prints |
| --- | --- | --- |
| `tool_runner/` | The version-1 reference agent package. See the notes below. | Matches `tool_runner/stdout.txt` |
| `control_plane_foundations.au` | Typed JSON and TOML metadata, path operations, process-local counters, and structured log and trace events | The artifact path, deterministic JSON, TOML validity, and the counter value |
| `retry_with_backoff.au` | `control.retry` with an immediate first attempt, zero-delay retries, eventual success, and exact final-error exhaustion | `42`, `attempt 2`, `3`, `2` |
| `retrying_network_worker.au` | An application-level HTTP retry policy over a loopback server. See the notes below. | `recover request 1`, `recover retry 4ms`, `recover request 2`, `recover result 200`, `rate request 1`, `rate retry 6ms`, `rate request 2`, `rate result 429`, `exhaust request 1`, `exhaust retry 3ms`, `exhaust request 2`, `exhaust retry 5ms`, `exhaust request 3`, `exhaust result 503`, `requests 7` |

**`tool_runner/`** shows:

- a `Callable[...]` registry contract that holds a factory-owned closure and a
  packed bound method
- remove, call, and reinsert dispatch through a `mut` dictionary
- typed JSON request and result methods, and Result errors
- `control.retry`
- Queue events that a TaskGroup child produces and the parent consumes with
  `for`
- user-resource cleanup

It uses no network, filesystem, or process access. Both backends match
`tool_runner/stdout.txt`. Run it with
`aura run --backend mir examples/agents/tool_runner/src/main.au`, or select
`direct`.

**`retrying_network_worker.au`** retries only `503`, keeps a terminal `429`,
and returns the final `503` once the attempt budget runs out. At that point it
draws no jitter and does not sleep. It uses:

- `random.Rng(42)` and exponential `Duration` backoff with deterministic jitter
- explicit five-second network and task deadlines
- a scoped `TaskGroup` and worker-owned listener, exchange, and response
  resources

The live listener stays on the task that owns it. A `Queue[str]` carries its
transferable bound address to the client task. A CLI regression test runs the
example through both the MIR backend and the forced direct backend and pins
seven real loopback requests.

### `json/`

| File | Shows | Prints |
| --- | --- | --- |
| `dynamic_values.au` | Parses a recursive `json.Value`, uses an exact scalar accessor, builds a mixed Object/Array tree, and dumps sorted compact JSON and two-space-indented JSON | |

### `bytes/`

| File | Shows | Prints |
| --- | --- | --- |
| `codecs_and_hashing.au` | Converts `str` to strict UTF-8 bytes and back, encodes and decodes binary data with canonical base64, renders lowercase hex, computes raw SHA-256, and reuses shared inputs afterward | `4175726120f09f8c8c`, `Aura 🌌`, `AAH+/w==`, `0001feff`, `ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad`, `4175726120f09f8c8c`, `[0, 1, 254, 255]` |

### `control_flow/`

| File | Shows | Prints |
| --- | --- | --- |
| `boolean_logic.au` | Boolean operators `and`, `or`, and `not` | `ready`, `true` |
| `if_elif_else.au` | Boolean conditions and branching | `high` |
| `conditional_expressions.au` | Python-style `value if condition else alternative` selection, including right-associated nesting | `ready`, `high`, `mid`, `low` |
| `membership_and_chains.au` | `in` and `not in` over `list`, `set`, `dict` keys, and `str` substrings, plus a chained comparison bound check | `true`, `true`, `true`, `true`, `true`, `true`, `false` |
| `enumerate_and_zip.au` | `for` over `enumerate(...)` and `zip(...)`. `zip` stops at the shorter sequence. | `0: alpha`, `1: beta`, `alpha:80`, `beta:443`, `3` |
| `for_range.au` | `for` loops over `range(...)`, plus `break` and `continue`. `range(...)` bounds must fit the signed index space that the compiler and runtime use. | `7` |
| `match_literals.au` | Statement-form `match` over literal `bool`, integer, and `str` cases | `negative`, `zero`, `many`, `yes`, `no`, `repo`, `other` |
| `while_break_continue.au` | Loops, `break`, `continue`, and compound assignment | `ok` |

### `enums/`

| File | Shows | Prints |
| --- | --- | --- |
| `union_type_patterns.au` | A transparent union alias, exhaustive type arms, and mutable member access | `42` twice |
| `union_narrowing.au` | `is None` and `is not None` tests that narrow a parameter and a class field, an early `return` edge, and an `and` composition | `missing`, `4`, `anonymous or new`, `aura`, `anonymous or new` |
| `result_match.au` | Enum declarations, owned payload variants, an explicit `own` parameter, and exhaustive consuming `match` | `42`, `bad`, `0` |
| `result_option.au` | Built-in `Result[T, E]` and optional `T \| None` values with exhaustive `match` | `4`, `division by zero`, `7` |
| `explicit_type_args.au` | Explicit type arguments on built-in enum constructors such as `Result[int32, str].Ok(...)` | `7`, `bad` |
| `constructor_ergonomics.au` | Keyword payload arguments on enum variants, bare `Ok(...)` constructors, and a bare value injected into a `T \| None` binding with expected types | `Status.Count(4)`, `7`, `9` |
| `match_borrow.au` | `match ...:` with unqualified built-in enum variants such as `case Ok(value):` | `ok` |
| `match_borrow_mut_fields.au` | Mutable matching through a field place while a proven-disjoint sibling field changes | `9`, `11` |
| `rich_match.au` | Multi-payload enum variants, nested patterns, named payload fields, and expression-form `match` | `7`, `30`, `0` |
| `match_expression_positions.au` | Expression-form `match` in binding and argument positions, including nested block-form arm values | `1`, `10`, `3`, `20` |
| `match_guards_and_or_patterns.au` | Exact-Boolean guards, binding-compatible enum or-patterns, and top-level complete-value bindings | `positive`, `non-positive`, `missing`, `4` |
| `wildcard_match.au` | Wildcard `case _:` arms in statement-form `match` | `2` |

### `generics/`

| File | Shows | Prints |
| --- | --- | --- |
| `box_and_wrapper.au` | User-defined generic classes, enums, and an `own T` identity function | `7`, `ok` |
| `generic_method_calls.au` | Method calls on generic class instances inside an `own` generic function | `7` |
| `generic_constructor_specialization.au` | Explicit type arguments on class and queue constructors such as `Box[int32](...)` | `42` |
| `bounded_types.au` | Trait bounds on generic class and enum type parameters | `aura`, `empty` |
| `clone_safety_obligations.au` | Inferred clone-safety obligations on a generic clone helper and a generic-to-generic forwarding helper | `[1, 2, 3]` twice |

### `traits/`

| File | Shows | Prints |
| --- | --- | --- |
| `greeter.au` | Trait declarations, `impl Trait for Type`, and bounded generic calls | `hello aura`, `hello aura` |
| `generic_dispatch_multiple_types.au` | Bounded generic trait dispatch across several concrete implementors | `dog`, `cat` |
| `generic_trait_bounds.au` | Generic trait bounds such as `T: Mapper[int32]` with an owned mapper input | `20` |
| `multiple_bounds.au` | Bounded generic calls with `T: A + B` | `9` |
| `supertraits.au` | Supertrait declarations, inherited bounds, and default methods that call through a parent trait | `name=aura`, `aura` |
| `self_parameters.au` | Trait methods that use `Self` in parameter and return positions | `9` |
| `marker_trait.au` | Empty marker traits declared with `pass` | `1` |
| `builtin_target_traits.au` | Trait impls for builtin targets such as `list[int32]` and `str`, using method names that do not collide with a builtin member | `list of 2`, `text of 5` |
| `specialized_generic_impl.au` | Specialized trait impls for concrete generic instances | `hello` |
| `specialized_trait_dispatch.au` | Bounded generic dispatch over specialized generic trait impls | `7`, `hi` |
| `generic_trait_impl.au` | Generic trait declarations and generic impl headers with `own T` inputs | `11` |
| `default_trait_methods.au` | Default trait method bodies with per-impl overrides | `name=aura`, `team=infra` |
| `operator_traits.au` | Operator traits for `+` and unary `-` through `Add[...]` and `Neg[...]` | `6`, `8`, `-6`, `-8` |
| `ordering_traits.au` | Ordering traits with shared right-hand operands and an explicitly consuming generic selector | `true`, `true`, `true`, `true`, `2` |
| `trait_associated_factory.au` | Associated trait methods called through the implementing type name | `7` |
| `clone_safety_contract.au` | An inferred clone-safety contract from a generic trait default method, kept through a bounded generic call | `[4, 5]` twice |

### `modules/`

| File | Shows | Prints |
| --- | --- | --- |
| `constants.au` | Inferred, annotated, public, and source-ordered module constants beside a local `main` | `planner`, `5` |
| `simple_import.au` | Local file modules with `import ...`, `from ... import ...`, and `public` module boundaries | `10`, `2` |
| `import_aliases.au` | Local module aliases and from-import aliases with canonical target identity | `10`, `2` |
| `function_values.au` | Stores a namespace-qualified imported function, then calls it directly and through a `def(int32) -> int32` parameter. Also specializes an imported zero-argument generic function from context and uses the result as a `TaskGroup.start` target. | `10`, `12`, `none` |
| `namespace_import_types.au` | Namespace-qualified class construction, enum variants, and qualified `match` arms through `import ...` | `4`, `true`, `1` |
| `trait_impl_imports.au` | Trait impls imported across package modules, including bounded generic calls and direct trait-method use | `Ada`, `Ada` |

The helper modules under `modules/pkg/` support these examples. They are not
standalone entrypoints.

### `packages/`

Package examples are complete package trees with manifest-rooted entrypoints.
Where dependency resolution creates an `Aura.lock` file, the committed lock
file is part of the example. FFI examples also pin their explicit unsafe
package authorization.

| Package | Shows | Prints |
| --- | --- | --- |
| `local_path_dependencies/app/` | `Aura.toml`, `src/`, a sibling path dependency, and a package-local helper module | `12` |
| `workspace/app/` | A workspace-root `Aura.toml`, member packages, and a sibling path dependency resolved through the member package manifest | `8` |
| `ffi_getpid/` | An FFI v0 package with `[package] allow_ffi = true` and a bodyless `extern "C" def getpid() -> int32` declaration | `true` on both backends |

Run commands:

- `local_path_dependencies/app/`:
  `cargo run -p aura -- run examples/packages/local_path_dependencies/app/src/main.au`
- `workspace/app/`:
  `cargo run -p aura -- run examples/packages/workspace/app/src/main.au`
- `ffi_getpid/`, on a Unix-family host, through both backends:
  - `cargo run -p aura -- run --backend mir examples/packages/ffi_getpid/src/main.au`
  - `cargo run -p aura -- run --backend direct examples/packages/ffi_getpid/src/main.au`

`Aura.toml` also supports git dependencies with `git`, `rev`, `tag`, or
`branch`. The default branch is `main`. Git dependencies need cached git
checkouts, which a static package tree in this repository cannot hold, so
compiler, CLI, and language-server regression tests cover them instead.

### `error_handling/`

| File | Shows | Prints |
| --- | --- | --- |
| `try_result.au` | `try expr` over `Result[T, E]` with propagated errors | `6`, `division by zero` |

### `resources/`

| File | Shows | Prints |
| --- | --- | --- |
| `with_resource.au` | Deterministic cleanup with `with` and `close(mut self)` | `demo`, `closed demo`, `done` |

### `io/`

| File | Shows | Prints |
| --- | --- | --- |
| `read_text_file.au` | Builtin `fs.exists(...)` and `fs.read_to_string(...)` | `true`, `true` |
| `bytes_file_io.au` | Binary file helpers and `fs.File.read_bytes()` / `write_bytes(...)` | `4`, `65`, `67`, `5`, `68` |
| `process_run.au` | Shell-free `process.run(..., group=true)`, UTF-8 and raw captured stdout and stderr, and `process.Completed.check()` | `aura process`, `13`, `0`, `ExitStatus.Exited(0)` |
| `process_pipes.au` | Interactive `process.start(..., group=true)`, `process.Pipe`, and child waiting with a timeout | `ping`, `ExitStatus.Exited(0)` |
| `process_supervisor.au` | `process.supervisor()`, restart policies, restart backoff, and group-aware supervisor shutdown | `SupervisorEvent.Restarted(flaky, ExitStatus.Exited(1), 1)`, `SupervisorEvent.Exited(flaky, ExitStatus.Exited(1), 1)`, `true`, `false`, `true` |
| `tcp_echo.au` | Builtin `net.listen(...)`, `net.connect(...)`, `TcpListener.accept()`, `TcpStream.read_line()`, `TcpStream.write_all(...)`, and `with` cleanup on network resources | `echo:ping` |
| `tcp_bytes.au` | TCP byte reads and writes with timeouts through `connect_timeout(...)`, `read_exact(...)`, `read_bytes(...)`, and `write_bytes(...)` | `4`, `116` |
| `udp_echo.au` | UDP binding, datagram receive and send, and `net.UdpDatagram` | `udp:ping`, `ping` |
| `http_roundtrip.au` | HTTP listener and request helpers on the shared evented runtime scheduler, plus `HttpExchange` and `HttpResponse` | `200`, `POST:/hello:body:ok` |
| `websocket_roundtrip.au` | WebSocket listener and connect helpers with timeouts on the nonblocking socket runtime | `ws:hi` |
| `unix_tls_roundtrip.au` | A Unix-socket and TLS roundtrip with an embedded self-signed certificate. Unix only. | `unix:ping`, `9` |

### `concurrency/`

Concurrency examples run on Aura's pinned-worker scheduler on both backends.
By default the runtime uses all of the host's available cores. Set
`AURA_WORKERS=<positive integer>` to pick a worker count for testing or
deployment. This override is provisional.

The runtime assigns a task to a worker when the task is spawned. The task's
coroutine stack never migrates, and it never takes part in work stealing.
Queue and Task handles are the channels between workers. Every other task
capture and result is an owned `Transfer` value that the compiler checks.

These examples use only structurally `Transfer` task captures, results, and
Queue payloads:

- Queue handles are copy values.
- Task handles are copyable when the result is repeatable. Repeatable results
  are copy values, Queue handles, and Task handles that are themselves
  repeatable.
- For any other transferable result, the first attempt to observe it consumes
  the task's single result right.

The examples never depend on task scheduling, completion order, or
printed-output order unless they coordinate that order explicitly.

| File | Shows | Prints |
| --- | --- | --- |
| `task_group_start.au` | Structured task startup with `TaskGroup.start(...)`, `Queue[T]().poll()`, and `Task.result_or(...)`. Without a timeout, `poll()` and `result_or(...)` are immediate non-blocking checks. | `2`, `4`, `6` |
| `queue_iteration.au` | Bare `for value in jobs:` receive iteration until close. Every item arrives owned. | `1`, `2` |
| `queue_timeout.au` | `Queue.get_or(default, timeout=...)` for the ordinary timeout case | `timeout` |
| `bounded_queue.au` | `Queue[T](capacity=...)` and `Queue.put(...)` waiting for bounded-capacity space on the pinned-worker scheduler | `queued 1`, `queued 2`, `3` |
| `send_result.au` | `Queue.put()` returning `Result[None, SendError[T]]`, including `Closed(...)`, `Cancelled(...)`, `TimedOut(...)`, and `Full(...)` | `7` |
| `task_group_start_soon.au` | Structured background work with `TaskGroup.start_soon(...)` | `9` |
| `task_group_associated_method.au` | Starting associated methods without `self` through `TaskGroup.start(...)` | `5`, `7` |
| `queue_put_timeout.au` | `Queue.put(timeout=...)` with explicit send-failure handling | `sent`, `4` |
| `task_group_queue_sum.au` | Queue-driven coordination inside a `TaskGroup()` scope | `3` |
| `task_group_cancel.au` | Cooperative cancellation with `group.cancel()` and `cancelled()` | `0`, `1` |
| `yield_now.au` | Explicit cooperative scheduling between bounded chunks of CPU work. Ordinary loop backedges also get amortized scheduling checks that the compiler inserts. | Three numbered steps each for `alpha` and `beta`. The exact interleaving is unspecified on purpose. |
| `typed_select.au` | Typed heterogeneous selection over Queue, Task, and relative-Duration sources, including deterministic lowest-index priority | `SelectOutcome.Queue(0, QueueReceive.Item(queued))`, `TaskResult.Ready(42)`, `SelectOutcome.Task(0, TaskResult.Ready(42))`, `SelectOutcome.Deadline(0)` |
| `task_group_wait_helpers.au` | Consuming `own` outcome helpers for `TaskResult[T]`, `WaitAny[T]`, `WaitAll[T]`, and bounded `Queue[T]` coordination. Covers `wait_any(...)`, `wait_all(...)`, `Task.result(timeout=...)`, and bounded queue send and receive outcomes. | `Result.Ok(None)`, `Result.Err(SendError.Full(2))`, `1`, `closed`, `ready`, `1`, `6`, `8`, `6`, `8` |
| `queue_get_timeout.au` | Short timeout handling through `Queue.poll(timeout=...)` | `Poll.Unavailable` |
| `queue_get_timeout_named.au` | Named timeout arguments on `Queue.poll(timeout=...)` | `Poll.Unavailable` |
| `sleep_builtin.au` | Blocking sleep with a `Duration` argument | `start`, `end` |
| `minute_duration.au` | Duration literals with the `m` suffix | `120000ms` |
| `duration_arithmetic.au` | Signed Duration constructors, runtime `int64` scaling, floor division, comparison, and floating unit conversion | `375ms`, `0.333333ms`, `2500ms`, `true`, `2000.0`, `1.5` |

### `randomness/`

| File | Shows | Prints |
| --- | --- | --- |
| `deterministic_rng.au` | A seeded `random.Rng`, unbiased half-open integer calls, and an in-place Fisher-Yates shuffle | `2`, `2`, `[3, 5, 4, 1, 2, 0]` |

### `numbers/`

| File | Shows | Prints |
| --- | --- | --- |
| `bit_packing.au` | Hexadecimal and binary literals, fixed-width masks and shifts, wrapping and saturating shift modes, power, ties-to-even `round`, and `divmod` | `16744448`, `255`, `128`, `0`, `0`, `255`, `64`, `64`, `81`, `2`, `4`, `-4`, `3` |
| `float_sqrt.au` | `float64` values and `.sqrt()` | `9.0` |
| `float32_values.au` | `float32` values in annotated bindings, parameters, returns, and class fields | `3.25`, `2.0`, `5.0` |
| `numeric_casts.au` | Explicit numeric conversions with `expr as Type` | `7`, `3.0`, `1.25`, `2.0` |
| `numeric_builtins.au` | Builtin numeric helpers `abs(...)`, `min(...)`, `max(...)`, `sqrt(...)`, and `float64.sqrt()` | `7`, `3.5`, `2`, `12`, `9.0`, `9.0` |
| `scalar_math.au` | Exact `float64` constants and scalar rounding, power, exponential, logarithmic, and trigonometric functions from the `math` module | `3.141592653589793`, `2.718281828459045`, `inf`, `NaN`, `-2`, `-1`, `-1`, `0.125`, `1.0`, `0.0`, `3.0`, `3.0`, `0.0`, `1.0`, `0.0` |
| `numeric_arrays.au` | Global contiguous row-major `Array[T]` over the four supported dtypes. Covers construction, multidimensional indexing, mutable replacement, owned slices along the first axis, mapping, reductions, scalar kernels, and explicit wrapping and saturating integer arithmetic. | `2`, `[1, 2, 3, 4]`, `[2, 2]`, `20`, `46`, `25.0`, `-2147483648`, `2147483647`, `16.0` |
| `uint128_values.au` | Full-range `uint128` literals and arithmetic through the runtimes and the direct backend | `340282366920938463463374607431768211455`, `340282366920938463463374607431768211455` |
| `unary_minus.au` | Unary minus for integer and floating-point expressions | `-5`, `-3.5`, `2` |

### `strings/`

| File | Shows | Prints |
| --- | --- | --- |
| `greeting.au` | String concatenation and equality | `hello, aura` |
| `string_clone.au` | `str.clone()` on owned strings | `aura` |
| `string_methods.au` | Single-quoted strings, an owned `str \| None` match helper, and the `str` methods: `int64` Unicode-scalar `len()`, `int64` UTF-8 `byte_len()`, `contains(...)`, `starts_with(...)`, `ends_with(...)`, `split(...)`, `replace(...)`, `to_lower()`, `to_upper()`, `strip_prefix(...)`, `strip_suffix(...)`, `trim()`, and `clone()` | `13`, `true`, `true`, `true`, `aura repo`, `2`, `aura`, `repo`, `aura lang`, `aura repo`, `AURA REPO`, `repo`, `none`, `aura`, `none`, `9` |
| `string_parsing_and_formatting.au` | Parsing builtins, scalar and boolean `.to_string()`, and `str.join(...)` | `42`, `-9000000000`, `3.5`, `true`, `aura-lang-tests`, `true`, `12`, `4`, `9`, `3.0` |
| `borrow_str.au` | Borrowed string parameters with `str` | `Hello, Aura` |
| `f_strings.au` | Interpolated `f"..."` strings that produce owned `str` values | `Hello, Aura 42` |
| `literal_forms_and_formatting.au` | Exact triple-quoted prompts, raw backslash-heavy paths, and statically checked width, sign-aware zero padding, grouping, and percentage formatting. Shows Unicode-aware formatting through the f-string grammar. | |

## Top-Level Examples

Five small reference programs sit directly in `examples/`:

- `point.au`
- `basic_addition.au`
- `top_level_addition.au`
- `control_flow.au`
- `simple_addition.au`

## Run Examples

`aura run` executes a program through the MIR runtime by default. Run examples
from the repository root:

```bash
cargo run -p aura -- run examples/classes/point_distance.au
cargo run -p aura -- run examples/classes/methods.au
cargo run -p aura -- run examples/enums/result_match.au
```

Run commands for the categorized examples:

```bash
cargo run -p aura -- run examples/basics/top_level_script.au
cargo run -p aura -- run examples/basics/named_arguments.au
cargo run -p aura -- run examples/basics/named_builtin_arguments.au
cargo run -p aura -- run examples/basics/default_arguments.au
cargo run -p aura -- run examples/basics/function_values.au
cargo run -p aura -- run examples/basics/borrow_parameters.au
cargo run -p aura -- run examples/basics/numbers.au
cargo run -p aura -- run examples/basics/pass_keyword.au
cargo run -p aura -- run examples/collections/list_basics.au
cargo run -p aura -- run examples/collections/list_iteration.au
cargo run -p aura -- run examples/collections/list_polish.au
cargo run -p aura -- run examples/collections/slices.au
cargo run -p aura -- run examples/collections/dict_basics.au
cargo run -p aura -- run examples/collections/lookup_in_place.au
cargo run -p aura -- run examples/collections/set_basics.au
cargo run -p aura -- run examples/classes/point_distance.au
cargo run -p aura -- run examples/classes/methods.au
cargo run -p aura -- run examples/classes/mutating_methods.au
cargo run -p aura -- run examples/control_flow/for_range.au
cargo run -p aura -- run examples/control_flow/match_literals.au
cargo run -p aura -- run examples/control_flow/boolean_logic.au
cargo run -p aura -- run examples/control_flow/conditional_expressions.au
cargo run -p aura -- run examples/control_flow/membership_and_chains.au
cargo run -p aura -- run examples/control_flow/enumerate_and_zip.au
cargo run -p aura -- run examples/basics/len_and_str.au
cargo run -p aura -- run examples/control_flow/while_break_continue.au
cargo run -p aura -- run examples/enums/result_match.au
cargo run -p aura -- run examples/enums/result_option.au
cargo run -p aura -- run examples/enums/wildcard_match.au
cargo run -p aura -- run examples/generics/box_and_wrapper.au
cargo run -p aura -- run examples/generics/generic_method_calls.au
cargo run -p aura -- run examples/generics/bounded_types.au
cargo run -p aura -- run examples/generics/clone_safety_obligations.au
cargo run -p aura -- run examples/traits/greeter.au
cargo run -p aura -- run examples/traits/multiple_bounds.au
cargo run -p aura -- run examples/traits/marker_trait.au
cargo run -p aura -- run examples/traits/specialized_generic_impl.au
cargo run -p aura -- run examples/traits/clone_safety_contract.au
cargo run -p aura -- run examples/traits/builtin_target_traits.au
cargo run -p aura -- run examples/error_handling/try_result.au
cargo run -p aura -- run examples/resources/with_resource.au
cargo run -p aura -- run examples/io/read_text_file.au
cargo run -p aura -- run examples/io/bytes_file_io.au
cargo run -p aura -- run examples/io/process_run.au
cargo run -p aura -- run examples/io/process_pipes.au
cargo run -p aura -- run examples/io/process_supervisor.au
cargo run -p aura -- run examples/io/tcp_echo.au
cargo run -p aura -- run examples/io/tcp_bytes.au
cargo run -p aura -- run examples/io/udp_echo.au
cargo run -p aura -- run examples/io/http_roundtrip.au
cargo run -p aura -- run examples/io/websocket_roundtrip.au
cargo run -p aura -- run examples/io/unix_tls_roundtrip.au
cargo run -p aura -- run examples/concurrency/task_group_start.au
cargo run -p aura -- run examples/concurrency/bounded_queue.au
cargo run -p aura -- run examples/concurrency/send_result.au
cargo run -p aura -- run examples/concurrency/task_group_start_soon.au
cargo run -p aura -- run examples/concurrency/queue_put_timeout.au
cargo run -p aura -- run examples/concurrency/task_group_queue_sum.au
cargo run -p aura -- run examples/concurrency/task_group_cancel.au
cargo run -p aura -- run examples/concurrency/yield_now.au
cargo run -p aura -- run examples/concurrency/typed_select.au
cargo run -p aura -- run examples/concurrency/queue_timeout.au
cargo run -p aura -- run examples/concurrency/queue_get_timeout.au
cargo run -p aura -- run examples/concurrency/queue_get_timeout_named.au
cargo run -p aura -- run examples/concurrency/sleep_builtin.au
cargo run -p aura -- run examples/concurrency/minute_duration.au
cargo run -p aura -- run examples/concurrency/duration_arithmetic.au
cargo run -p aura -- run examples/numbers/bit_packing.au
cargo run -p aura -- run examples/numbers/float32_values.au
cargo run -p aura -- run examples/numbers/numeric_casts.au
cargo run -p aura -- run examples/numbers/numeric_builtins.au
cargo run -p aura -- run examples/numbers/scalar_math.au
cargo run -p aura -- run examples/numbers/numeric_arrays.au
cargo run -p aura -- run examples/numbers/unary_minus.au
cargo run -p aura -- run examples/strings/string_clone.au
cargo run -p aura -- run examples/strings/string_methods.au
cargo run -p aura -- run examples/strings/string_parsing_and_formatting.au
cargo run -p aura -- run examples/strings/literal_forms_and_formatting.au
```

## Build Standalone Artifacts

`aura build` packages a checked program as a standalone native binary:

```bash
cargo run -p aura -- build -o ./target/aura-point examples/point.au
./target/aura-point
cargo run -p aura -- build --backend direct -o ./target/aura-direct examples/basic_addition.au
./target/aura-direct
```

| Flag | Behavior |
| --- | --- |
| `--backend auto` | The default. Uses the direct native backend for the supported Aura surface. |
| `--backend direct` | Forces the direct native backend. It covers the full implemented Aura language surface. |

The built binary does not need the original `.au` source file at runtime. The
build step still needs Cargo, Rust, and a host C compiler.

## Check, AST, and MIR

Use `check`, `ast`, and `mir` to check a program or print its syntax tree or
MIR:

```bash
cargo run -p aura -- check examples/classes/default_fields.au
cargo run -p aura -- ast examples/classes/point_distance.au
cargo run -p aura -- mir examples/control_flow/while_break_continue.au
cargo run -p aura -- mir examples/enums/result_match.au
cargo run -p aura -- mir examples/error_handling/try_result.au
```

## Maintenance

The categorized examples are part of the supported development workflow. When
the implemented language subset changes:

1. Update the relevant example.
2. Update the matching tutorial chapter.
3. Keep the example set runnable under `cargo test`.
