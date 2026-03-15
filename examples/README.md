# Aurora Examples

This directory contains runnable Aurora programs for the current compiler bootstrap.

## Current Examples

- `point.au`
  - first milestone sample
  - defines `Point`, computes the distance between `(0, 0)` and `(3, 4)`, and prints `5`

## Run An Example

From the repo root:

```bash
cargo run -p aura -- run examples/point.au
```

Expected output:

```text
5
```

## Type Check An Example

```bash
cargo run -p aura -- check examples/point.au
```

Expected output:

```text
ok
```

## Print The Parsed AST

```bash
cargo run -p aura -- ast examples/point.au
```

This prints the parsed Aurora module as Rust debug output. It is useful when working on the frontend and checking how the current parser understands an example.

## CLI Summary

The current Aurora CLI supports:

- `aura run <file.au>`
  - run the program
- `aura check <file.au>`
  - parse and type check the program
- `aura ast <file.au>`
  - print the parsed syntax tree

## Notes

- Run commands from the repo root so the example paths above resolve directly.
- The compiler is still in an early bootstrap phase, so the examples track the subset of Aurora implemented today rather than the full language proposal.
