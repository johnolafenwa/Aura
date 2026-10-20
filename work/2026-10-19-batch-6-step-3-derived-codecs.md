# Batch 6 Step 3: Derived Codecs For Classes

## Goal

Derive `encode`, `decode`, and `schema` for a class from a bodyless
`impl json.Codec for C`, as the ratified Batch 6 checkpoint describes, on
both backends.

## Work Completed

- The parser accepts `impl Trait for Type` with no `:` and suite, and marks
  the implementation `derived`.
- `sema/json_derive.rs` runs before checking. For each derived class it
  writes Aura source: the three impl methods delegate to private module
  functions, and each distinct field type gets one encoder, decoder, and
  schema helper. The source is parsed with its lines shifted past
  1,000,000,000, so its spans never meet the module's span-keyed tables. A
  checker error inside it is reported at the `impl` line, prefixed with
  `derived json.Codec for C:`.
- The generated source reaches `json` through a hidden module binding, so a
  derived class works whether the module wrote `import json`,
  `import json as j`, or `from json import Codec`.
- The helpers it calls live in `crates/aura-compiler/std/json.au` under
  `__` names: kind names, error construction, path re-rooting with `__at`,
  checked integer and float decoders, object entries in document order, a
  deep `json.Value` copy, and the `$defs` builders.
- Decoding takes each object entry in document order, rejects an unknown
  key, decodes each field into a one-slot list, then fills defaults and
  reports a missing field in declaration order. A field default is cloned
  into its own helper function.
- The schema is an object with `properties`, `required`, and
  `additionalProperties: false`. Every class reached through a field sits
  once under `$defs`. A class of another module, or with an explicit
  implementation, has its own schema lifted into `$defs`.
- `AU2046` rejects a bodyless implementation of another trait, a class from
  another module, a generic class, an enum, a class with `__init__`, and
  fields with no JSON form.

## Fixes Found On The Way

- `json.decode` called from an imported module failed at run time. A
  module's recorded generic uses named their type arguments with
  module-local names, and the entry program could not find the
  implementation. Exported records now carry qualified type names.
- A temporary passed to `json.encode`, as in `json.encode(Point(x=1))`, was
  typed `T` in MIR, so dispatch failed. Calls of generic module functions now
  infer argument types the same way local generic calls do.
- `case j.Value.Object(fields)` after `import json as j` was rejected. Module
  lookup now also resolves an import alias.

## Verification

- Run-pass `json_codec_derived`, `json_codec_derived_import_forms`, and
  `json_aliased_module_patterns`, with identical output on the direct
  backend.
- Eleven `json_derive_*` check-fail fixtures.
- CLI test `derived_json_codecs_work_across_modules_on_both_backends`.
- `examples/json/derived_codec.au` in the example smoke tests and the direct
  backend's example surface.

## Follow-Up

- Step 4 derives codecs for enums and for unions of distinct JSON kinds.
- Step 5 adds docstring descriptions to schemas and the schema agreement
  test.
- `1.0` parses as an integer, so a derived integer field accepts it. The
  Manual says so.
