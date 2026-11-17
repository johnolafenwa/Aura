# Collections, text, and numbers: step 1

Date: 2026-11-16

## Target

John's 2026-11-16 go-ahead for item 6 of the missing-features survey:
"Only list, dict and set, with no deque, heap, ordered map or set
algebra. Strings have no integer indexing or byte literals, and only
UTF-8. There is no big integer, decimal or complex type, and `math`
lacks inverse trig."

The plan for the whole item is
`architecture_docs/32-collections-text-numbers-design-checkpoint.md`.
This note covers step 1, the parts that live in the runtime.

## Done

- `math`: 27 functions and `tau` (ADR-0110).
- Byte strings, `b"..."` (ADR-0111).
- `text[index]` and `str.char_at` (ADR-0111).
- `bytes.encode` and `bytes.decode` for eleven encodings (ADR-0111).
- Set algebra: eight methods and `| & - ^` (ADR-0112).

## How each is built

- The `math` functions are host functions in
  `crates/aura-runtime/src/host_builtins.rs`, one table shared by the
  interpreter and compiled code.
- A byte string is a lexer token. The parser turns ASCII bytes into
  `"text".to_bytes()` and anything else into a list of `uint8` casts,
  so nothing after the parser knows about it.
- `text[i]` is rewritten to `text.char_at(i)` when MIR is built.
  `char_at` is one more row of the text-method table. Compiled code
  passes the string's cached ASCII flag, so ASCII text is indexed
  without a scan.
- The encodings are `crates/aura-runtime/src/text_encodings.rs`, with
  no new dependency.
- Set algebra is `set_algebra` in `collection_index.rs`, reached by
  collection-operation codes 8 to 15 from compiled code and by method
  name from the interpreter. The operators are rewritten to the method
  calls when MIR is built.

## Verification

- Runtime library tests and compiler library tests pass. One compiler
  test, `package_io_helpers_cover_local_error_paths`, fails only where
  the tests run as root, as on the cloud machine, and is unrelated.
- New run fixtures were recorded from the interpreter and compared
  byte for byte with compiled output.
- `check_doc_examples.py` passes on all 1,095 documentation blocks.
- `aura verify` accepts functions that use `text[i]` and set operators.
- The extension grammar tests pass against the edited grammar.

Integration test binaries were not built locally. CI runs them.

## Found and not fixed here

These are the measurements that shape step 3. They are compiler costs
that affect any program, not only the planned `collections` module.

- `self.items.append(value)` on a list field costs about 340
  instructions, and `self.items.pop()` about 650. The same calls on a
  local list cost about 16.
- Generic code over `items: mut list[T]` with comparisons is far
  slower than the same code for one element type. A generic binary
  heap ran 400 times slower than Rust.
- `list.swap` is a runtime call and is not inlined.
- A runtime trap from a host function or a text method has no source
  line, only the call chain. `math.log(0.0)` behaved this way before.

## Next

Step 2 is the `numbers` module. Step 3 is `collections`, after the
field-list and generic-list costs are removed.

## Editor Verification On 2026-11-17

Both hosted verify jobs failed the same exact-list assertion for set
members. The compiler returned all eight new methods. The test now
expects those names and checks the signatures for producing a set,
testing a relation, and updating a set in place. All 158 language server
tests passed on Linux with the compiler built from this branch.
