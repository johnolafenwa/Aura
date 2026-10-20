# Batch 6 Step 5: Schema Descriptions And The Agreement Test

## Goal

Carry docstrings into derived JSON Schemas as `description`, and show that
each derived schema accepts exactly the inputs its derived decoder accepts,
as the ratified Batch 6 checkpoint describes.

## Work Completed

- `sema/json_derive.rs` reads the normalized docstring of each derived class
  and enum, and of each class field. A documented field's property schema
  goes through the new `json.__describe` helper. A documented type's object
  schema is built by an undescribed helper and then described, so the text
  appears under `$defs` and at the root alike.
- `json.__describe` adds `description` to an object schema. Any other schema
  is wrapped in `allOf`.
- Docstring text becomes an Aura string literal through an escaping helper
  that handles quotes, backslashes, newlines, tabs, and control characters.
- `crates/aura-compiler/tests/json_schema_agreement.rs` takes nine types from
  the codec fixtures and `examples/json/derived_codec.au`. For each one it
  mutates a valid input at every position: replaced values of every JSON
  kind, removed and added keys, and shorter and longer arrays. It runs one
  program that prints the schema and each decoder verdict, then checks each
  verdict against a JSON Schema 2020-12 validator for the keywords derived
  schemas use. An unknown keyword fails the test.

## Verification

- Run-pass fixture `json_codec_described`, with identical output on the
  direct backend.
- The agreement test passes on about 1,500 inputs. With the validator's
  `required` rule disabled, it fails on the first `Scalars` input.

## Follow-Up

- Step 6: decorator syntax, concrete decorators, order, and recursion
  through the decorated slot.
- An explicit codec may be stricter than its own schema. The agreement test
  leaves such fields alone, such as `Node.price`.
