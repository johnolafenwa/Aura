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
- `pass_keyword.au`
  - the `pass` no-op statement in empty classes and functions
  - prints `0`

### `classes/`

- `point_distance.au`
  - class fields, member access, functions, and `float64.sqrt()`
  - prints `5`
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

### `generics/`

- `box_and_wrapper.au`
  - user-defined generic classes, enums, and functions
  - prints:
    - `7`
    - `ok`

### `traits/`

- `greeter.au`
  - trait declarations, `impl Trait for Type`, and bounded generic calls
  - prints:
    - `hello aurora`
    - `hello aurora`
- `multiple_bounds.au`
  - bounded generic calls with `T: A + B`
  - prints `9`

### `modules/`

- `simple_import.au`
  - local file modules with `import ...`, `from ... import ...`, and `public` module boundaries
  - prints:
    - `10`
    - `2`

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

### `numbers/`

- `float_sqrt.au`
  - `float64` values and `.sqrt()`
  - prints `9`
- `float32_values.au`
  - `float32` values introduced through annotated bindings, parameters, returns, and class fields
  - prints:
    - `3.25`
    - `2`
    - `5`
- `numeric_casts.au`
  - explicit numeric conversions with `expr as Type`
  - prints:
    - `7`
    - `3`
    - `1.25`
    - `2`
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
cargo run -p aura -- run examples/basics/pass_keyword.au
cargo run -p aura -- run examples/classes/point_distance.au
cargo run -p aura -- run examples/classes/methods.au
cargo run -p aura -- run examples/classes/mutating_methods.au
cargo run -p aura -- run examples/control_flow/for_range.au
cargo run -p aura -- run examples/control_flow/boolean_logic.au
cargo run -p aura -- run examples/control_flow/while_break_continue.au
cargo run -p aura -- run examples/enums/result_match.au
cargo run -p aura -- run examples/enums/result_option.au
cargo run -p aura -- run examples/generics/box_and_wrapper.au
cargo run -p aura -- run examples/traits/greeter.au
cargo run -p aura -- run examples/traits/multiple_bounds.au
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
cargo run -p aura -- run examples/numbers/float32_values.au
cargo run -p aura -- run examples/numbers/numeric_casts.au
cargo run -p aura -- run examples/numbers/unary_minus.au
cargo run -p aura -- run examples/strings/string_clone.au
```

## Build Standalone Artifacts

The bootstrap CLI can also package a runnable standalone binary for a checked program:

```bash
cargo run -p aura -- build -o ./target/aurora-point examples/point.au
./target/aurora-point
```

Today this build path is still bootstrap-oriented. It generates and compiles a small Rust launcher linked against `aurora-compiler`, so it is useful for packaging and smoke-testing programs but it is not yet the final MIR-native backend.
The generated launcher now embeds checked MIR and executes it directly through `run_mir(...)`.

## Run Through The Backend Path

The CLI also exposes the current MIR-first backend path directly:

```bash
cargo run -p aura -- run-mir examples/classes/point_distance.au
cargo run -p aura -- run-mir examples/classes/methods.au
cargo run -p aura -- run-mir examples/enums/result_match.au
```

Current `run-mir` limits:

- it now covers the current implemented Aurora surface, including `spawn`, `select`, channels, task groups, `try`, and `with`
- it is still a bootstrap backend path, not the final MIR-native code generation backend

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
