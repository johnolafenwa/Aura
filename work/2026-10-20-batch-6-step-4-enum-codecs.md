# Batch 6 Step 4: Derived Codecs For Enums And Unions

## Goal

Derive `json.Codec` for enums, and accept union fields whose members have
different JSON kinds, as the ratified Batch 6 checkpoint describes, on both
backends.

## Work Completed

- `sema/json_derive.rs` accepts a bodyless `impl json.Codec for E` for a
  non-generic enum of the same module.
- An enum whose variants have no payloads encodes as the variant name and
  decodes from a string. Its schema is `{"enum": [...]}`.
- Any other enum encodes as an object with a `"type"` key and one key per
  named payload. A unit variant is `{"type": "Name"}`. The decoder takes the
  tag with the new `json.__tagged` helper, then decodes the remaining
  entries with the same field loop a class uses, in a helper function per
  variant. The schema is a `oneOf` with one closed object per variant, and
  `"type"` fixed by `const`.
- An unknown variant is `UnknownVariant`, at `$` for a name and at `$.type`
  for a tag. A missing tag is `MissingField` for `type`.
- A union field with members of different JSON kinds encodes through
  `case T as member` arms, decodes by `json.__kind`, and has an `anyOf`
  schema. An integer selects a float member when no integer member exists.
  A `WrongType` error lists the accepted kinds, such as
  `integer, string, or null`.
- `AU2046` rejects positional payloads with a help line that names the
  payload, a payload named `type`, generic enums, unions whose members share
  a JSON kind, and union members whose kind the pass cannot see:
  `json.Value`, and types with explicit or imported codecs.

## Verification

- Run-pass fixture `json_codec_derived_enums`, with identical output on the
  direct backend.
- Check-fail fixtures `json_derive_positional_payload`,
  `json_derive_generic_enum`, `json_derive_payload_named_type`,
  `json_derive_payload_type`, `json_derive_union_field`, and
  `json_derive_union_codec_member`. `json_derive_enum` is removed, because
  enums are now supported.
- CLI test `derived_json_enum_codecs_work_across_modules_on_both_backends`.
- `examples/json/derived_codec.au` gains a unit enum field.

## Follow-Up

- Step 5: schema descriptions from docstrings and the schema agreement test.
- A union member with an imported or explicit codec has no known JSON kind.
  Supporting it needs the kind in the exported codec metadata.
