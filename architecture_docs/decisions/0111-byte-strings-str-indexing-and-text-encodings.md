# ADR-0111: Byte-string literals, integer indexing of `str`, and text encodings

- Status: Accepted
- Date: 2026-11-16

## Context

Three gaps in text handling came up together in the 0.16 survey.

- A protocol constant such as an HTTP line ending had to be written as
  `"\r\n".to_bytes()` or as a list of numbers.
- `text[0]` was a compile error. A program wrote `text[0:1]` or built
  `text.chars()` to read one character.
- Bytes became text only as UTF-8. Files and protocols in UTF-16,
  Latin-1, and Windows-1252 could not be read without hand-written
  tables.

## Decision

### Byte strings

`b"..."` and `b'...'` are literals of type `list[uint8]`. The body
holds printable ASCII and the escapes of an ordinary string except
`\u{...}`. `\xHH` is the byte `HH`, for all 256 values. A non-ASCII
character, a `\u{...}` escape, and a triple-quoted byte string are
lexical errors (`AU1001`).

There is still one bytes type. A byte string is a fresh mutable list
each time it is evaluated, exactly as the list literal it replaces.

### Integer indexing of `str`

`text[index]` and `text.char_at(index)` return the character at a
position as a one-character `str`. Positions count Unicode scalar
values, as `len` and slicing do. A negative index counts from the end.
An index outside the string traps with `AU4003`.

Reading position `i` scans to it unless the string is ASCII, which the
runtime already knows, so indexing ASCII text takes constant time and
other text takes time proportional to the position. A `str` is not a
place: `text[0] = "x"` is a compile error (`AU2003`).

### Text encodings

`bytes.encode(text, encoding)` and `bytes.decode(value, encoding)`
convert through a named encoding and return
`Result[..., bytes.EncodingError]`. The names are `utf-8`, `utf-8-sig`,
`utf-16`, `utf-16-le`, `utf-16-be`, `utf-32`, `utf-32-le`,
`utf-32-be`, `latin-1`, `ascii`, and `windows-1252`, compared without
regard to case, `-`, `_`, or spaces.

`bytes.EncodingError` has three variants: `Unknown(name: str)`,
`Invalid(index: int64)` with the offset of the first byte that does
not decode, and `Unencodable(index: int64)` with the position of the
first character the encoding cannot hold.

`utf-16` and `utf-32` write a byte-order mark and little-endian units,
and read either order by the mark, little-endian without one. The
explicit `-le` and `-be` forms neither write nor strip a mark.
`utf-8-sig` writes the mark and strips one if present. Decoding is
strict: there is no replacement character.

## Why not other ways

- **A separate `bytes` type for literals.** Every byte API takes
  `list[uint8]`. A second type would need conversions at each call.
- **A `char` type for `text[i]`.** Aura has no character type, and
  `chars()`, `ord`, and `chr` already use one-character strings.
- **Byte indexing.** It would make `text[i]` disagree with `len(text)`
  and with slicing, and could split a character.
- **An `encoding` argument on `to_bytes` and `from_bytes`.** Those two
  cannot fail on encoding and return different types. A second pair of
  functions keeps their signatures unchanged.
- **Every encoding of a large table library.** The eleven names cover
  Unicode and the two legacy encodings that still turn up in Western
  files. Others can be added by name without a new API.

## Consequences

- The formatter keeps a byte string as written.
- The editor grammar highlights the `b` prefix with its own scope.
- `str` still has no slice assignment and no grapheme indexing.
