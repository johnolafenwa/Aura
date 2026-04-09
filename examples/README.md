# Aurora Examples

This directory contains runnable Aurora programs for the current compiler bootstrap.

The examples are organized by topic so they can serve both as quick references and as a companion to the `tutorials/` directory.

## Categories

### `basics/`

- `top_level_script.au`
  - top-level executable statements with inferred bindings
  - prints `156`
- `main_function.au`
  - `main` with an omitted `None` return type
  - prints `16`
- `none_values.au`
  - bare `None` as both the unit type and unit value
  - prints `1`
- `mutable_bindings.au`
  - `mut`, reassignment, and compound assignment
  - prints `5`
- `named_arguments.au`
  - named arguments on functions, instance methods, and associated methods
  - prints:
    - `hello, aurora`
    - `7`
- `named_builtin_arguments.au`
  - named arguments on supported builtins like `print(...)` and `range(...)`
  - prints `10`
- `default_arguments.au`
  - default parameter values evaluated on each call
  - prints:
    - `hello world`
    - `hello aurora`
    - `6`
    - `12`
- `borrow_parameters.au`
  - free-function `borrow` and `borrow mut` parameters with caller-visible mutation
  - prints:
    - `41`
    - `42`
    - `42`
- `pass_keyword.au`
  - the `pass` no-op statement in empty classes and functions
  - prints `0`

### `collections/`

- `vec_basics.au`
  - list literals, indexed reads, `Vec[T]` methods, and indexed mutation through `set(...)`
  - prints:
    - `3`
    - `1`
    - `2`
    - `2`
    - `20`
    - `1`
    - `99`
    - `false`
- `vec_iteration.au`
  - empty-vector construction with `Vec[T]()`, `extend(...)`, explicit `Vec[T]` annotations, and iteration by value or `borrow`
  - prints:
    - `Ada`
    - `Grace`
    - `2`
    - `9`
- `vec_polish.au`
  - non-copy index reads, `borrow mut` iteration, `insert(...)`, `reverse()`, `extend(...)`, `clear()`, and the richer `Vec[T]` method surface with equality
  - prints:
    - `Ada`
    - `Grace`
    - `true`
    - `false`
    - `4`
    - `1`
    - `14`
    - `13`
    - `12`
    - `11`
    - `true`
    - `100`
    - `true`
    - `true`
- `map_basics.au`
  - `Map[K, V]` literals, `extend(...)`, `items()` / `entries()`, indexed reads/writes, and the maintained map method surface
  - prints:
    - `3`
    - `true`
    - `1`
    - `1`
    - `5`
    - `aurora`
    - `3`
    - `3`
    - `3`
    - `3`
    - `true`
- `set_basics.au`
  - `Set[T]` literals, shared-borrow iteration, deduplication, and the maintained set method surface
  - prints:
    - `3`
    - `true`
    - `false`
    - `true`
    - `true`
    - `9`
    - `true`
    - `true`
    - `1`

### `classes/`

- `point_distance.au`
  - class fields, member access, functions, and `float64.sqrt()`
  - prints `5.0`
- `default_fields.au`
  - field default values and keyword construction
  - prints:
    - `localhost`
    - `8080`
- `methods.au`
  - instance methods with `borrow self` plus associated methods
  - prints:
    - `4`
    - `8`
    - `0`
- `mutating_methods.au`
  - `borrow mut self`, field mutation, and compound assignment through `self`
  - prints:
    - `6`
    - `1`
- `copy_class.au`
  - `copy class` for explicit copy semantics on fully copyable fields
  - prints:
    - `1`
    - `2`
- `indirect_recursive.au`
  - recursive fields with `indirect Node?` and optional children
  - prints `2`

### `control_flow/`

- `boolean_logic.au`
  - boolean operators `and`, `or`, and `not`
  - prints:
    - `ready`
    - `true`
- `if_elif_else.au`
  - boolean conditions and branching
  - prints `high`
- `for_range.au`
  - `for` loops over `range(...)`, plus `break` and `continue`
  - prints `7`
  - current bootstrap note: `range(...)` bounds must fit the signed index space used by the compiler/runtime
- `match_literals.au`
  - statement-form `match` over literal `bool`, integer, and `String` cases
  - prints:
    - `negative`
    - `zero`
    - `many`
    - `yes`
    - `no`
    - `repo`
    - `other`
- `while_break_continue.au`
  - loops, `break`, `continue`, and compound assignment
  - prints `ok`

### `enums/`

- `result_match.au`
  - enum declarations, payload variants, and exhaustive `match`
  - prints:
    - `42`
    - `bad`
    - `0`
- `result_option.au`
  - built-in `Result[T, E]` and `Option[T]` values with exhaustive `match`
  - prints:
    - `4`
    - `division by zero`
    - `7`
- `explicit_type_args.au`
  - explicit type arguments on built-in enum constructors like `Result[int32, String].Ok(...)`
  - prints:
    - `7`
    - `bad`
- `match_borrow.au`
  - `match borrow ...:` plus unqualified built-in enum variants like `case Ok(value):`
  - prints `ok`
- `wildcard_match.au`
  - wildcard `case _:` arms in statement-form `match`
  - prints `2`

### `generics/`

- `box_and_wrapper.au`
  - user-defined generic classes, enums, and functions
  - prints:
    - `7`
    - `ok`
- `generic_method_calls.au`
  - method calls on generic class instances inside generic functions
  - prints `7`
- `generic_constructor_specialization.au`
  - explicit type arguments on class and channel constructors such as `Box[int32](...)`
  - prints `42`
- `bounded_types.au`
  - trait bounds on generic class and enum type parameters
  - prints:
    - `aurora`
    - `empty`

### `traits/`

- `greeter.au`
  - trait declarations, `impl Trait for Type`, and bounded generic calls
  - prints:
    - `hello aurora`
    - `hello aurora`
- `generic_dispatch_multiple_types.au`
  - bounded generic trait dispatch across multiple concrete implementors
  - prints:
    - `dog`
    - `cat`
- `multiple_bounds.au`
  - bounded generic calls with `T: A + B`
  - prints `9`
- `marker_trait.au`
  - empty marker traits declared with `pass`
  - prints `1`
- `specialized_generic_impl.au`
  - specialized trait impls for concrete generic instances
  - prints `hello`
- `specialized_trait_dispatch.au`
  - bounded generic dispatch over specialized generic trait impls
  - prints:
    - `7`
    - `hi`
- `generic_trait_impl.au`
  - generic trait declarations plus generic impl headers for generic classes
  - prints `11`
- `trait_associated_factory.au`
  - associated trait methods called through the implementing type name
  - prints `7`

### `modules/`

- `simple_import.au`
  - local file modules with `import ...`, `from ... import ...`, and `public` module boundaries
  - prints:
    - `10`
    - `2`
- `namespace_import_types.au`
  - namespace-qualified class construction, enum variants, and qualified `match` arms through `import ...`
  - prints:
    - `4`
    - `true`
    - `1`
- `trait_impl_imports.au`
  - trait impls imported across package modules, including bounded generic calls and direct trait-method use
  - prints:
    - `Ada`
    - `Ada`

Helper modules under `modules/pkg/` support the maintained module examples above and are not standalone entrypoints.

### `error_handling/`

- `try_result.au`
  - `try expr` over `Result[T, E]` with propagated errors
  - prints:
    - `6`
    - `division by zero`

### `resources/`

- `with_resource.au`
  - deterministic cleanup with `with` and `close(borrow mut self)`
  - prints:
    - `demo`
    - `closed demo`
    - `done`

### `concurrency/`

- `channels_spawn.au`
  - typed channels, `channel()`, `spawn`, `recv()`, and `join()`
  - prints:
    - `2`
    - `4`
- `channel_iteration.au`
  - `for value in jobs:` iteration over a `Channel[T]` until close
  - prints:
    - `1`
    - `2`
- `send_result.au`
  - `Channel.send()` returning `Result[None, SendError[T]]`
  - prints `7`
- `spawn_detached.au`
  - explicit detached background work with `spawn detached`
  - prints `9`
- `select_send.au`
  - `select` with a channel send arm and a timer fallback
  - prints:
    - `sent`
    - `4`
- `task_group_select.au`
  - `with task_group() as group:`, channel handle cloning, and `select`
  - prints `3`
- `task_group_cancel.au`
  - cooperative cancellation with `group.cancel()` and `cancelled()`
  - prints:
    - `0`
    - `1`
- `select_timeout.au`
  - timer-based `select` without channels
  - prints `timeout`
- `select_timeout_named.au`
  - timer-based `select` using `after(duration=...)`
  - prints `timeout`
- `sleep_builtin.au`
  - blocking sleep with a `Duration` argument
  - prints:
    - `start`
    - `end`
- `minute_duration.au`
  - duration literals with the `m` suffix
  - prints `120000ms`

### `numbers/`

- `float_sqrt.au`
  - `float64` values and `.sqrt()`
  - prints `9.0`
- `float32_values.au`
  - `float32` values introduced through annotated bindings, parameters, returns, and class fields
  - prints:
    - `3.25`
    - `2.0`
    - `5.0`
- `numeric_casts.au`
  - explicit numeric conversions with `expr as Type`
  - prints:
    - `7`
    - `3.0`
    - `1.25`
    - `2.0`
- `numeric_builtins.au`
  - builtin numeric helpers `abs(...)`, `min(...)`, `max(...)`, plus `sqrt(...)` and `float64.sqrt()`
  - prints:
    - `7`
    - `3.5`
    - `2`
    - `12`
    - `9.0`
    - `9.0`
- `uint128_values.au`
  - full-range `uint128` literals and arithmetic through the current runtimes and direct backend
  - prints:
    - `340282366920938463463374607431768211455`
    - `340282366920938463463374607431768211455`
- `unary_minus.au`
  - unary minus for integer and floating-point expressions
  - prints:
    - `-5`
    - `-3.5`
    - `2`

### `strings/`

- `greeting.au`
  - string concatenation and equality
  - prints `hello, aurora`
- `string_clone.au`
  - `String.clone()` on owned strings
  - prints `aurora`
- `string_methods.au`
  - the maintained `String` method surface: `len()`, `contains(...)`, `starts_with(...)`, `ends_with(...)`, `split(...)`, `replace(...)`, `to_lower()`, `to_upper()`, `strip_prefix(...)`, `strip_suffix(...)`, `trim()`, and `clone()`
  - prints:
    - `15`
    - `true`
    - `true`
    - `true`
    - `aurora repo`
    - `2`
    - `aurora`
    - `repo`
    - `aurora lang`
    - `aurora repo`
    - `AURORA REPO`
    - `repo`
    - `none`
    - `aurora`
    - `none`
    - `11`
- `string_parsing_and_formatting.au`
  - parsing builtins, scalar and boolean `.to_string()`, and `String.join(...)`
  - prints:
    - `42`
    - `-9000000000`
    - `3.5`
    - `true`
    - `aurora-lang-tests`
    - `true`
    - `12`
    - `4`
    - `9`
    - `3.0`
- `borrow_str.au`
  - borrowed string parameters with `borrow str`
  - prints `Hello, Aurora`
- `f_strings.au`
  - interpolated `f"..."` strings producing owned `String` values
  - prints `Hello, Aurora 42`

## Stable Bootstrap Examples

The original top-level files are still present as stable bootstrap references:

- `point.au`
- `basic_addition.au`
- `top_level_addition.au`
- `control_flow.au`
- `simple_addition.au`

## Run Examples

From the repo root:

```bash
cargo run -p aura -- run examples/basics/top_level_script.au
cargo run -p aura -- run examples/basics/named_arguments.au
cargo run -p aura -- run examples/basics/named_builtin_arguments.au
cargo run -p aura -- run examples/basics/default_arguments.au
cargo run -p aura -- run examples/basics/borrow_parameters.au
cargo run -p aura -- run examples/basics/pass_keyword.au
cargo run -p aura -- run examples/collections/vec_basics.au
cargo run -p aura -- run examples/collections/vec_iteration.au
cargo run -p aura -- run examples/collections/vec_polish.au
cargo run -p aura -- run examples/collections/map_basics.au
cargo run -p aura -- run examples/collections/set_basics.au
cargo run -p aura -- run examples/classes/point_distance.au
cargo run -p aura -- run examples/classes/methods.au
cargo run -p aura -- run examples/classes/mutating_methods.au
cargo run -p aura -- run examples/control_flow/for_range.au
cargo run -p aura -- run examples/control_flow/match_literals.au
cargo run -p aura -- run examples/control_flow/boolean_logic.au
cargo run -p aura -- run examples/control_flow/while_break_continue.au
cargo run -p aura -- run examples/enums/result_match.au
cargo run -p aura -- run examples/enums/result_option.au
cargo run -p aura -- run examples/enums/wildcard_match.au
cargo run -p aura -- run examples/generics/box_and_wrapper.au
cargo run -p aura -- run examples/generics/generic_method_calls.au
cargo run -p aura -- run examples/generics/bounded_types.au
cargo run -p aura -- run examples/traits/greeter.au
cargo run -p aura -- run examples/traits/multiple_bounds.au
cargo run -p aura -- run examples/traits/marker_trait.au
cargo run -p aura -- run examples/traits/specialized_generic_impl.au
cargo run -p aura -- run examples/error_handling/try_result.au
cargo run -p aura -- run examples/resources/with_resource.au
cargo run -p aura -- run examples/concurrency/channels_spawn.au
cargo run -p aura -- run examples/concurrency/send_result.au
cargo run -p aura -- run examples/concurrency/spawn_detached.au
cargo run -p aura -- run examples/concurrency/select_send.au
cargo run -p aura -- run examples/concurrency/task_group_select.au
cargo run -p aura -- run examples/concurrency/task_group_cancel.au
cargo run -p aura -- run examples/concurrency/select_timeout.au
cargo run -p aura -- run examples/concurrency/select_timeout_named.au
cargo run -p aura -- run examples/concurrency/sleep_builtin.au
cargo run -p aura -- run examples/concurrency/minute_duration.au
cargo run -p aura -- run examples/numbers/float32_values.au
cargo run -p aura -- run examples/numbers/numeric_casts.au
cargo run -p aura -- run examples/numbers/numeric_builtins.au
cargo run -p aura -- run examples/numbers/unary_minus.au
cargo run -p aura -- run examples/strings/string_clone.au
cargo run -p aura -- run examples/strings/string_methods.au
cargo run -p aura -- run examples/strings/string_parsing_and_formatting.au
```

## Build Standalone Artifacts

The CLI can also package a runnable standalone native binary for a checked program:

```bash
cargo run -p aura -- build -o ./target/aurora-point examples/point.au
./target/aurora-point
cargo run -p aura -- build --backend direct -o ./target/aurora-direct examples/basic_addition.au
./target/aurora-direct
```

`aura build` now supports:

- `--backend auto`
  - default
  - uses the direct native backend for the maintained Aurora surface
- `--backend direct`
  - forces the current direct native backend
  - now covers the full currently implemented Aurora language surface

The built binary does not depend on the original `.au` source file at runtime, but the build step still needs Cargo/Rust and a host C compiler.

## Run Through The Backend Path

The CLI also exposes the current MIR-first backend path directly:

```bash
cargo run -p aura -- run-mir examples/classes/point_distance.au
cargo run -p aura -- run-mir examples/classes/methods.au
cargo run -p aura -- run-mir examples/enums/result_match.au
```

Current `run-mir` limits:

- it now covers the current implemented Aurora surface, including `spawn`, `select`, channels, task groups, `try`, and `with`
- the direct backend now covers the maintained Aurora surface, so `run-mir` is mainly useful as an alternate execution path and backend-debugging tool

## Check, AST, and MIR

```bash
cargo run -p aura -- check examples/classes/default_fields.au
cargo run -p aura -- ast examples/classes/point_distance.au
cargo run -p aura -- mir examples/control_flow/while_break_continue.au
cargo run -p aura -- run-mir examples/classes/methods.au
cargo run -p aura -- mir examples/enums/result_match.au
cargo run -p aura -- mir examples/error_handling/try_result.au
```

## Maintenance

The categorized examples are part of the supported development workflow.

When the implemented language subset changes:

1. update the relevant example
2. update the matching tutorial chapter
3. keep the example set runnable under `cargo test`
