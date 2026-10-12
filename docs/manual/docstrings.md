# Docstrings And API Docs

A docstring is a plain triple-quoted string that documents a declaration.
This page covers where a docstring goes, how editors and `aura doc` present
it, and the limits the compiler enforces.

```aura
"""Plane shapes and their measurements."""

public class Square:
    """A square with equal sides."""

    public side: float64
    """Length of one side."""

    public def area(self) -> float64:
        """Return the area."""
        return self.side * self.side

public def scale(shape: Square, factor: float64) -> Square:
    """Return a scaled copy.

    Args:
        shape: The square to scale.
        factor: The multiplier for each side.
    """
    return Square(side=shape.side * factor)
```

Docstrings are metadata. They produce no runtime value, no allocation, and no
instruction. Changing a docstring does not change the program's behavior or
its compiled code.

## Where A Docstring Goes

A docstring is the complete first statement of one of these bodies:

| Declaration | First slot |
| --- | --- |
| Module | The first statement of the file, before every import. |
| `class` | The first statement of the class body. |
| `enum` | The first statement of the enum body, before the variants. |
| `trait` | The first statement of the trait body. |
| Function or method | The first statement of the body. |
| Trait or `impl` method | The first statement of the body. |

Comments and blank lines before the docstring do not count as statements.
Both `"""..."""` and `'''...'''` work.

A function whose body is only a docstring is a function with an empty body.
It returns `None`:

```aura
def reserved():
    """Not implemented yet."""
```

## Fields

A field docstring is a plain triple-quoted string on the line directly after
the field declaration:

```aura
class Config:
    """Settings for one run."""

    retries: int64 = 3
    """How many times a failed request is sent again."""
```

A blank line between the field and the string makes the string misplaced.
Enum variants have no documentation.

## Parameters

Document parameters in the function docstring under an `Args:` heading. Each
entry is `name: description`, indented under the heading. Indent continuation
lines further:

```aura
def clamp(value: int64, low: int64, high: int64) -> int64:
    """Limit a value to a closed range.

    Args:
        value: The number to limit.
        low: The smallest result.
        high: The largest result. It must not be
            smaller than `low`.
    """
    if value < low:
        return low
    if value > high:
        return high
    return value
```

The section is presentation data. Hover, signature help, and `aura doc` read
it. The compiler does not check that the names match the parameters.

## Trait Methods

A trait method whose body is only a docstring stays signature-only. Each
implementation must still define it:

```aura
trait Shape:
    """A plane figure."""

    def area(self) -> float64:
        """Return the area in square units."""

    def describe(self) -> str:
        """Return a short description. This method has a default body."""
        return "a shape"

class Square:
    side: float64

impl Shape for Square:
    def area(self) -> float64:
        return self.side * self.side
```

Hover on a call that resolves to an implementation shows the implementation
method's docstring. When the implementation has none, hover shows the trait
method's docstring.

## What Is Not A Docstring

These strings are ordinary expression statements, with no documentation
meaning:

- a triple-quoted string after the first statement of a function body
- a module string after an import
- an f-string, a raw string, or a single-line string
- a triple-quoted string that is part of a larger expression

A triple-quoted string in a class, enum, trait, or `impl` body that is not
in a documentation position is an error, because those bodies have no other
use for a string. The error is `AU2042`:

```aura check-fail:AU2042
class Point:
    x: int64

    """This string is not directly after a field."""
```

An `impl` block has no docstring of its own. Document its methods instead.

## Presentation

Editors and `aura doc` show a normalized form of the text. The stored text
does not change. Normalization does this:

1. Split the text into lines.
2. Remove leading and trailing blank lines.
3. Keep the first line's text after its leading spaces.
4. Find the smallest indentation of the remaining non-blank lines, counting
   spaces only. Remove that many leading spaces from each of them.
5. Remove trailing spaces from every line.

Tabs are content, not indentation. Docstrings may use Markdown. Code blocks
and lists keep their relative indentation.

In the language server:

- **Hover** shows the fenced signature, then the docstring, then any other
  notes. Raw HTML is escaped, and control characters other than tab and
  newline are shown as `\u{...}` escapes.
- **Completion** items carry the first non-empty line of the docstring as
  their documentation, cut at 160 characters with `…`.
- **Signature help** shows the whole docstring, and each parameter shows its
  `Args:` entry.

Docstrings of imported public declarations appear in hover the same way as
local ones.

## Generated API Docs

`aura doc` checks each source and writes one page per module:

```bash
aura doc src --out docs/api
```

| Option | Effect |
| --- | --- |
| `path ...` | Files or directories to document. The default is the current directory. |
| `--out DIR` | Write one file per module into `DIR`. Without it, pages go to standard output. |
| `--json` | Write one JSON document per module instead of Markdown. |

A page lists the module docstring and every public declaration: constants,
type aliases, classes with their public fields and methods, enums with their
variants, traits with their methods, and functions. Each entry has its
signature and its normalized docstring. Parameters with an `Args:` entry are
listed with their descriptions. Private declarations are omitted.

A module's name is the file stem for a file argument. For a directory
argument, it is the dotted path below the directory, such as
`geometry.shapes` for `geometry/shapes.au`. Two modules with the same name
are an error.

The JSON form has `schema_version`, `module`, `doc`, and `declarations`.
Each declaration has `kind`, `name`, `signature`, `doc`, and, when present,
`parameters`, `fields`, `variants`, and `methods`.

`aura doc` does not run code in the documentation.

## Limits

| Limit | Value |
| --- | --- |
| One docstring | 65,536 UTF-8 bytes after escape processing |
| A module's public documentation | 4,194,304 UTF-8 bytes in total |

Public documentation counts the module docstring and the docstrings of
public declarations and their public members, trait methods, and `impl`
methods. Private documentation does not count. Exceeding a limit is
`AU2042` at the docstring, with the measured and permitted byte counts.

Design record: [ADR-0056: Docstrings and documentation metadata](https://github.com/johnolafenwa/Aura/blob/main/architecture_docs/decisions/0056-docstrings-and-documentation-metadata.md).
