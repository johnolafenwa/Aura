# Batch 6 Step 7: Generic Decorators And The Retry Rule

## Goal

Allow generic decorators inferred per use, and enforce the rule that a retry
decorator cannot wrap a function that consumes its argument, as sections E3
and E5 of the Batch 6 checkpoint describe, on both backends.

## Work Completed

- A generic callable alias packs or adapts a value with its type arguments
  written out: `Fn[A, R](lambda ...)` for `Callable[...]` storage and
  `Check[int64](is_even)` for a thin `def` alias. Inside a generic function
  the arguments may name its type parameters. A generic alias called without
  type arguments reports `AU2002`. `AliasInfo::specialized_target` and
  `specialized_alias_callee` are shared by sema and MIR.
- MIR lowers written type arguments with the enclosing function's type
  parameters, which it collects from the return type and local types, so the
  packed contract matches the closure's own type.
- `factory(...)(args)` infers a generic factory's type parameters from the
  outer call when the factory's arguments leave them open. The checker tries
  the plain call first and only uses the outer call after a "cannot infer"
  error. MIR mirrors the inference in lowering and in `infer_expr_type`, and
  the callable validator takes a type parameter no argument fixes from the
  call's destination. This makes `@retry(3)` work, including stacked
  factories such as `@retry(2) @retry(2)`.
- The packed function is the innermost decorator's argument, so a mismatch
  in passing it reports at that decorator's `@` line. A renamed parameter
  gets help that the decorated function must use the decorator's names. A
  mismatch that involves an `own` parameter gets help pointing to an
  explicit loop.
- `examples/basics/retry_decorator.au` shows a generic `@retry(3)`. The
  Manual covers generic decorators and generic alias packing, and the
  functions tutorial has a generic decorator example.

## Deviation

The E3 example writes the decorator over a thin `def` type and does not say
whether parameter names take part in inference. Callable contracts keep
parameter names, and renaming needs an explicit wrapper, so a generic
decorator fixes the names of the parameters it accepts. A name-polymorphic
decorator would need a new rule for callable contracts.

## Verification

- run-pass `decorators_generic` and `callable_alias_generic_pack` on both
  backends.
- check-fail `decorator_generic_renamed`, `decorator_generic_result`,
  `decorator_retry_own_parameter`, `callable_alias_generic_without_arguments`,
  and `callable_alias_generic_wrong_arity`.
- `generic_decorators_infer_each_use_across_modules_on_both_backends`, with
  stacked module-qualified factories and recursion through the decorated
  slot.
- `examples/basics/retry_decorator.au` in the example smoke tests.
- Locally: clippy, `cargo test --workspace` (only the root-user
  `package_io_helpers_cover_local_error_paths` fails), all 402 CLI tests,
  803 documentation examples, the language server suite, and the hygiene
  check pass.

## Follow-Up

- Step 8 rewrites the reference agent example with codecs, schemas, and
  `@traced`.
