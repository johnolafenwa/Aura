# Three compiler defects from the book sections

Date: 2026-11-16

## Target

John's ask of 2026-11-16: "fix the bugs". The bugs are the three defects
that `2026-11-15-book-library-sections.md` lists as found and not fixed.
The older findings from the book rewrite of 2026-11-01 were run again with
the released 0.16.0 compiler. All of the ones marked as bugs pass.

## Changes

1. `[ids[i].clone() for i in range(2)]` failed with AU4001 "invalid MIR
   in `main` ends inactive loan". A statement's body ends its element
   loans through the loan scope of its statement list. A comprehension
   lowers its filters and its output straight into the loop body with no
   such scope, so the loan went to the enclosing statement and ended after
   the loop, on a path where no iteration had begun it.
   `lower_comprehension_filters` now opens a loan scope around each filter
   and around the output and ends it in the same block. A filter's value
   is copied to a `bool` local before its loans end.
2. `socket.send_bytes(address, [100, 105], timeout=1s)` passed the checker
   and stopped at run time with "expects `list[uint8]`". The checker gives
   the literal the parameter's element type. The lowering asked
   `builtin_member_argument_type` only when the argument's own type was
   open, and a list of numbers infers as `list[int64]`. A list literal now
   takes the parameter's type from that table too. This covers every
   member in the table: the byte writers, `respond_bytes`, `join`,
   `extend`, and the supervisor's argument list.
3. A static musl program without `/proc` had an empty `sys.args()`. The
   runtime keeps `environ` when `aura_direct_runtime_init` runs, before a
   `setenv` can move it. When `/proc/self/cmdline` cannot be read,
   `initial_stack_args` walks down from that array over the argument
   pointers to the word that holds their count. It gives up on a word
   that is neither a pointer nor the count. The code is compiled for musl
   only, except the walk itself, which a unit test runs on every host.

## Verification

- `crates/aura-compiler/tests/fixtures/run-pass/comprehension_indexed_element_loans.au`
  on both backends, and the two repros from the book work.
- Unit tests: `comprehension_element_loans_end_inside_their_iteration`,
  `a_number_list_passed_to_a_byte_parameter_of_a_method_holds_bytes`,
  `first_stack_gives_the_arguments_after_the_program_name`.
- `cargo test -p aura-compiler --lib`: 2,140 pass. The one failure,
  `package_io_helpers_cover_local_error_paths`, fails for the superuser
  only.
- `cargo test -p aura --test cross_targets` with the x86-64 musl target.
  The test now also runs the static program in a bare `chroot`. It skips
  that step when it can neither be the superuser nor use `sudo` without a
  password.
- `cargo clippy --all-targets` on the four crates.
- Not run locally: the full integration suites and the arm64 musl target.
  CI runs both.
