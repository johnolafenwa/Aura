# Compile-Time Evaluation And Conditional Compilation

This checkpoint fixes the interfaces for implementation. The manual will describe the shipped behavior once the implementation passes its tests.

    import build
    const lanes: int64 = 2 * 4
    if build.arch == "aarch64" and lanes == 8:
        import arm_kernels
    else:
        import portable_kernels

## Bindings And Purity

Use `[public] const name[: Type] = expression` at module scope. Preserve declaration order and visibility. Ordinary module bindings also qualify for evaluation. An ordinary impure initializer retains its runtime initialization order. The marker requires evaluation. An evaluated value has the same Aura type and sharing rules as the original binding.

Use the typed MIR interpreter. Admit target-defined arithmetic, casts, reads of earlier frozen bindings, aggregate construction and deterministic pure builtins. Follow reachable user functions transitively. Permit mutation of locally owned temporaries. Reject external effects before executing them, including I/O, environment or time access, randomness, tasks, shared state, foreign calls, and opaque handles. Never probe purity by executing an effect. Classify impurity before surfacing speculative traps for ordinary initializers.

Charge each initializer up to 100,000 MIR steps and 8 MiB of cumulative logical allocation. Charge before work, including builtin allocation, and include nested calls in the same budget. Measure Aura payloads using target sizes. Exhaustion or a trap in a pure initializer is a compile error. Report the binding, source position and Aura call frames. Use target integer widths, IEEE binary32/binary64 rounding, and deterministic NaN handling. Host pointer addresses and host library variation cannot appear in frozen data.

The evaluator owns the frozen-value representation, the required-binding flag and emission. Carry values through MIR and emit immutable target-layout objects with relocations for references. Scalar, text, byte, tuple, list, dictionary, set, class and enum values must support normal reads on both backends. Omit initializer execution for frozen bindings, including in freestanding objects.

## Configuration Interface

Use `import build`. Expose `os`, `arch`, `pointer_width`, `endianness`, `freestanding`, `hosted`, `debug`, `release` and `cfg`. The last member is a dictionary from string to string. Test a flag with `"tracing" in build.cfg` and use indexing for a value. A bare flag has the value `"true"`. Use lowercase OS and architecture spellings from Aura's target inventory, `none` for a freestanding OS and `little` or `big` for byte order.

The configuration owner supplies a public compiler `BuildConfig` in `build_config.rs`, with corresponding fields, a sorted cfg map, and a scoped `with_build_config` / `current_build_config` API. The CLI installs the configuration before loading any source. Scope changes restore the caller's configuration even on unwind. Key every configuration-dependent cache, including builtin checked-source caches and native caches, by target identity, execution environment, profile, and sorted length-delimited cfg pairs.

Read string or boolean values from the manifest's `[build.cfg]` table. Command-line `--cfg name[=value]` entries override the manifest. Later CLI entries override earlier ones. Names follow Aura identifier spelling. Preserve the value after the first equals sign. Apply one configuration to the complete dependency graph.

## Conditional Suites

Parse every suite with the grammar for its surrounding context, including module items and class members. Select before import discovery, foreign declarations, type checking or lowering. Preserve source coordinates. Conditions may use literals, build constants, marked bindings and pure operators on those values. Resolve import aliases and lexical shadowing. Function conditions involving runtime values retain their existing runtime behavior and lexical scopes.

Selection needs evaluated values of marked bindings. The selection and evaluation owners share a typed path for obtaining already-visible values, including computed predicates. Preserve use-before-initialization diagnostics and reject circular dependencies. Imports present only in discarded suites never reach the module loader.

Check all supported target configurations with `aura check --all-targets`, including native hosted platforms and freestanding targets. Reuse identical configurations when possible. Attach the target to each error. Keep the selected feature map and build profile fixed across this run.

## Tooling And Decisions

| Question | Decision |
| --- | --- |
| How does verification handle a conditional? | Verify obligations for the selected target and active declarations. |
| What does formatting do? | Parse and preserve both suites and format their source regardless of target. |
| Which items appear in documentation? | Include public items active for the documented configuration. |
| How does the editor handle discarded code? | Publish inactive source ranges for dimming. Keep best-effort syntax navigation and suppress inactive semantic errors. |
| How do class drop hooks interact with evaluation? | Check local hooks as implicit calls and charge the initializer budget. A returned value carrying an armed hook needs a runtime owner. |
| Does an ordinary pure failure fall back to startup? | Report the evaluation error. Only impurity preserves runtime initialization. |
| How are repeated or missing flags handled? | Later CLI values win. A missing dictionary key follows ordinary membership and indexing semantics. |
| Which configurations does all-targets cover? | Supported targets at the selected profile and cfg, without inventing feature combinations. |
| Who owns the diagnostic namespace? | Reserve AU2070–AU2079 for selection and configuration, AU3025–AU3029 for evaluation and AU4025–AU4029 for emission. Allocate only unused codes within these ranges. |

Design records 0135 through 0139 cover configuration, evaluation and limits, frozen data, conditional suites, and tooling respectively.
