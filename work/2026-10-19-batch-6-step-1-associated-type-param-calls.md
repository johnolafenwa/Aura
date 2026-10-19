# Batch 6 Step 1: Associated Methods Through A Type Parameter

## Goal

Let a generic function call a trait's associated method through its bounded
type parameter, such as `T.decode(value)`. `json.decode[T]` in step 2 needs
this. Step 1 adds it for every trait. John ratified the Batch 6 checkpoint on
2026-10-19.

## Work Completed

- Generic bodies are erased, and bound methods dispatch on the runtime
  receiver. An associated method has no receiver, so this step specializes in
  MIR.
- The checker records each `T.m(...)` call and each use of a generic free
  function with its checked type arguments. Records are keyed by module and
  call position and exported through `ModuleNamespace`, so imports see them.
- `specialized_functions` computes, as a fixpoint, the functions that call
  `T.m(...)` or pass an open type argument to such a function.
- Lowering emits a placeholder callee at each such call or use.
  `specialize_type_param_calls` then copies each template family, including
  its lambdas, once per concrete type list, names the copy like
  `build[Widget]`, resolves `T.m` to the most specific implementation, and
  drops the generic original. The pass stops at 4096 copies.
- `AU2005` rejects a method that takes `self`, a method with its own type
  parameters, a call outside a module-level function, a specialized function
  used as a task target, and an open use outside a free function. A missing
  method is `AU2002`.
- Docs: the Manual's generics page, tutorial 16, and a new example.

## Verification

- Run-pass fixtures `trait_associated_method_through_type_param.au` and
  `trait_associated_method_positions.au`, on the MIR runtime and the direct
  backend.
- Five `associated_type_param_*` check-fail fixtures.
- CLI test `associated_methods_through_type_params_specialize_across_modules_on_both_backends`.
- `examples/traits/associated_through_type_param.au` in the example smoke
  tests and the direct backend's example surface.

## Follow-Up

- Methods of classes and implementations cannot call `T.m(...)` yet.
- Specialized functions cannot be task targets.
- Step 2 adds the `json.Codec` trait and `json.decode` dispatch.
