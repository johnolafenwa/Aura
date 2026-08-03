# ADR-0056: Docstrings and documentation metadata

- Status: Proposed
- Date: 2026-08-02
- Version target: Aura 0.4
- Implementation: Not started
- Roadmap decision: Batch S1, design-only checkpoint
- Related: ADR-0025, ADR-0037, ADR-0046, ADR-0050, ADR-0053, and ADR-0055

## Decision boundary

This ADR is a proposed compiler, interface, and language-server design.
Docstring recognition, metadata serialization, hover presentation, and API-doc
inputs are not implemented. Implementation requires separate authorization
after ratification.

## Context

Aura APIs need documentation next to the declarations they describe. Agent
tools and ML systems particularly benefit when hover text can explain input
contracts, ownership, errors, and side effects without requiring a separate
website. Triple-quoted strings already provide a readable multiline source
form, so a declaration's first statement slot can carry static documentation
without adding runtime reflection or executable initialization.

The compiler must define exactly which literal is documentation, how it is
stored across module boundaries, how decorators and properties retain it, and
how size limits protect compiler and editor memory.

## Goals

- attach concise or multiline documentation directly to supported declarations
- preserve documentation in public checked interfaces
- show local and imported documentation in language-server hover
- give future API-documentation tooling one stable metadata source
- keep docstrings out of runtime evaluation and MIR
- retain documentation through function decoration and property lowering

## Non-goals

- runtime `__doc__` reflection or mutable documentation
- executing a string expression to register documentation
- f-string interpolation, raw-string processing, or dynamic content
- mandatory parameter-tag, return-tag, or doctest syntax
- compiling examples found inside prose
- generating the API-documentation site in the first implementation
- attaching docstrings to fields, enum variants, local bindings, parameters,
  imports, implementation blocks, or arbitrary statements
- using docstrings to change visibility, typing, ownership, or code generation

## Eligible declarations and first slots

A docstring is one plain triple-quoted string literal in the first statement
slot of any of these bodies:

- module
- class
- enum
- trait
- function
- instance, static, associated, trait, or implementation method

Both `"""..."""` and `'''...'''` are valid. The literal has no prefix and is
not an f-string or raw string. It must be the complete first statement; string
concatenation, a conditional expression, a name referring to a string, or any
other expression is not a docstring.

Comments and blank physical lines are trivia and do not occupy the first
statement slot. At module scope, the docstring must precede every import,
constant, and declaration. In a declaration body, it must precede every field,
variant, method, nested statement, or `pass`.

```aura
"""Agent orchestration primitives and typed tool contracts."""

class ToolCall:
    """One validated request to a registered tool."""

    name: str
    arguments: dict[str, str]

    def label(self) -> str:
        """Return a concise label for logs and diagnostics."""
        return self.name.clone()
```

Only the first eligible literal is metadata. A later triple-quoted literal is
an ordinary string expression under the statement grammar and does not attach
documentation to any declaration. A declaration may omit its docstring.

## Content contract

The stored text is the literal's exact logical UTF-8 content after the ordinary
triple-string escape rules. Opening and closing delimiters are excluded. No
automatic indentation removal, common-margin calculation, leading/trailing
newline removal, whitespace normalization, Unicode normalization, or line
wrapping occurs.

Each docstring is limited to 65,536 UTF-8 bytes after escape processing. A
checked module interface may contain at most 4,194,304 UTF-8 bytes of exported
docstring content in total. Exceeding either limit is a compile-time error at
the literal and reports the measured and permitted byte counts. The total
limit does not count private declarations because their documentation is not
serialized into the public interface.

Empty docstrings are valid metadata but produce no prose section in hover.
NUL and noncharacter Unicode scalar values remain valid string content, but
presentation layers escape control characters other than tab and newline so
they cannot corrupt the editor protocol or generated output.

Docstrings may contain CommonMark. The compiler stores text and does not parse
or validate markup. The language server passes sanitized Markdown to the
client, preserving code fences and links while disabling raw HTML execution.
Future API tooling consumes the same stored text.

## Semantic metadata, not execution

The parser associates the recognized literal with its owning declaration. The
semantic model validates its size and stores its decoded bytes. The literal
does not produce an expression node for evaluation, does not allocate a
runtime `str`, does not appear in MIR, and has no native data symbol.

Adding, changing, or removing documentation changes semantic/interface and
editor cache identities but cannot change executable MIR or program output.
Backends receive no docstring payload. Dead-code analysis, reachability,
constant initialization, ownership checking, and runtime coverage ignore the
metadata node.

Source maps retain the literal span so hover, definition, rename-adjacent
previews, and diagnostics can point to it. Documentation text is not searched
for symbol references and does not participate in rename.

## Interface and visibility rules

The checked interface for a public module records:

- the module docstring
- the docstring of each exported class, enum, trait, and function
- the docstring of each exported member reachable through that public API
- stable association by declaration identity, not source offset

Private declaration docstrings remain in the current compilation's semantic
database for local hover and are omitted from exported interfaces. A public
declaration whose signature exposes another public declaration does not copy
that other declaration's prose into its own record.

Interface hashing has separate semantic and executable components. A
docstring-only change invalidates importers' documentation/LSP metadata and
future API-doc output, while allowing executable object reuse when no semantic
signature changed. Release packages include the public documentation metadata
needed for hover without including private source text.

## Decorated functions

A decorated function's docstring belongs to the source declaration and is
attached to the final decorated binding. Every reference, hover, completion
item, and future API entry for that binding shows the source declaration's
documentation. A decorator-returned wrapper cannot replace, concatenate, or
erase it through runtime behavior.

The undecorated intermediate has no separate public documentation identity.
Definition navigation from the final binding reaches the source `def` and its
docstring. Decorator functions retain their own independent docstrings on
their declarations.

## Properties

The docstring in a property getter's first statement slot is the property's
documentation:

```aura
@property
def total_tokens(self) -> int64:
    """Total prompt and completion tokens in this reply."""
    return self.prompt_tokens + self.completion_tokens
```

Member completion and hover show it on `value.total_tokens`, and definition
navigation reaches the getter. The property descriptor does not synthesize
prose from its return type or containing class. The getter has no second
independently visible documentation entry.

## Traits and implementations

A trait method may define a docstring whether it has a default body or only a
signature. For a signature-only method, the docstring occupies the body slot
that follows the signature and does not make the method a default
implementation; grammar and semantic representation distinguish documentation
from executable statements. A docstring-only suite is signature-only. A suite
with executable statements after the docstring is a default implementation.

An explicit implementation method may provide its own docstring. Hover on
dispatch resolved to that concrete implementation uses:

1. the implementation method's non-empty docstring, when present
2. otherwise the corresponding trait method's docstring
3. otherwise no method prose

Hover on a generic trait-bound call uses the trait method's docstring because
no concrete implementation is selected there. Inherited default methods use
the trait method documentation. A trait's own declaration docstring is shown
for the trait symbol and is not automatically prepended to every method.

## Language-server presentation

Hover content appears in this order:

1. one fenced Aura signature using canonical type spellings
2. the declaration's docstring, when non-empty
3. existing compiler-generated ownership, inferred-obligation, or provenance
   notes that are independently part of hover

The language server preserves the docstring's logical line breaks. It does not
reflow paragraphs or remove indentation. Control characters are escaped before
forming the protocol payload, and raw HTML is sanitized. Relative links are
resolved only by a future API-doc generator; editor hover leaves them as text
unless the client has a safe source-document base URI.

Completion detail may show the first non-empty logical line, truncated at 160
Unicode scalar values with an ellipsis. The full hover remains available up to
the compile-time size limit. Signature help for a callable shows its full
declaration docstring and does not attempt to parse parameter sections.

## Future API documentation

The stable input to an API-doc generator is the checked public-interface
record: declaration identity, kind, canonical signature, visibility,
containment, source link metadata, and exact docstring text. The generator may
render navigation, summaries, and CommonMark, but it may not alter compiler or
language-server semantics.

Examples inside fenced `aura` blocks are documentation content. Executing them
as doctests would require a separate decision defining imports, hidden setup,
expected output, ownership of fixture resources, and backend selection.

## Diagnostics

Focused diagnostics must identify:

- an f-string, raw string, concatenation, or nonliteral expression used where
  documentation was intended
- a docstring after the first statement slot when declaration-only placement
  is syntactically required
- per-docstring or per-interface byte-limit overflow with exact counts
- invalid UTF-8 or escape processing through the ordinary string diagnostic
- malformed property/decorator placement while retaining the docstring's
  declaration association
- interface metadata that fails version, size, UTF-8, or declaration-identity
  validation on import

Docstring content does not create unknown-name, type, ownership, or unused-value
diagnostics. Markup mistakes are not compiler errors.

## Backend and tooling contract

MIR and direct compiler snapshots must prove that docstring edits do not add
instructions, constants, cleanup, or runtime allocations. The semantic and LSP
layers must nevertheless observe the edit immediately, including through an
imported checked interface.

The formatter preserves docstring content bytes and delimiter choice. It may
adjust the declaration's surrounding blank lines but cannot reindent content
inside the literal. Syntax highlighting assigns a documentation-string scope
distinct from an ordinary runtime string while retaining normal string escape
highlighting.

Source archives retain docstrings naturally. Stripped runtime binaries need no
documentation section. A future opt-in embedded-doc format requires another
decision and cannot silently enlarge ordinary binaries.

## Consequences

Aura source can carry useful API guidance without runtime cost. Public
interfaces become sufficient for imported hover and future API documentation,
while private prose stays within the current source compilation.

Exact whitespace preservation is simple and predictable, but authors are
responsible for source indentation that reads well in hover. The explicit size
limits make editor and interface memory bounded.

## Implementation adoption

Docstrings are additive declaration metadata and require no source rewrite.
When implemented, first-statement plain triple strings in eligible declarations
receive the one metadata meaning defined here; other triple strings retain the
ordinary string-expression rules. Parser association, semantic storage,
checked interfaces, hover, formatting, reference material, examples, and
tutorials land as one coordinated feature.

Adoption depends on the implemented triple-string lexer, stable declaration
identities, public/private visibility, checked-interface serialization,
sanitized Markdown hover, and ADR-0053/ADR-0055 association rules for decorated
bindings and properties. Runtime backends depend only on a verified guarantee
that metadata is absent from executable MIR.

The semantic/documentation schema, checked-interface format, language-server
index, completion cache, and generated-reference identity are bumped together.
Executable native cache keys remain reusable only when their independently
versioned semantic contract proves that a metadata-only edit cannot change
MIR; otherwise they are conservatively invalidated.

## Completion-test matrix

- lexer/parser: both plain triple delimiters in every eligible declaration,
  comments and blank lines before the first slot, and exact owner association
- exclusions: raw strings, f-strings, concatenation, names, ordinary strings,
  fields, variants, parameters, imports, locals, and later statements do not
  become docstrings
- content: escapes, Unicode, tabs, leading/trailing newlines, indentation,
  empty content, delimiter preservation by formatter, and no normalization
- limits: exactly 65,536 bytes, one-byte overflow, multibyte boundaries,
  exactly 4,194,304 exported bytes, total overflow, and private-doc exclusion
- semantics: no expression/MIR node, no allocation or constant, no ownership
  effects, unchanged executable cache identity, and changed metadata identity
- interfaces: every public declaration/member kind round-trips exact content;
  private docs remain local; malformed metadata is rejected safely
- decorators: final binding retains source docs through one/many decorators,
  capturing wrappers, recursion, imports, completion, and definition
- properties: getter docs attach to member access, completion, hover, and
  definition with no duplicate getter entry
- traits: declaration and method docs, docstring-only signature methods,
  documented default methods, explicit implementation override, fallback to
  trait prose, and bound calls
- LSP: local/imported hover ordering, canonical signature, CommonMark fences,
  HTML sanitization, control-character escaping, completion-summary truncation,
  signature help, edits, and incremental invalidation
- tooling: syntax scopes, formatter idempotence and byte preservation, source
  links, future API-doc fixture input, examples, tutorials, and reference text
- parity: identical absence from MIR/direct execution and byte-identical
  semantic/interface metadata and diagnostics on both compiler paths

## Ratification questions

1. Ratify exact logical content preservation with no indentation or blank-line
   normalization?
2. Ratify plain triple-quoted first-statement slots for module, class, enum,
   trait, function, and every method kind?
3. Are 65,536 bytes per docstring and 4,194,304 exported docstring bytes per
   checked interface appropriate limits?
4. Ratify public-interface serialization and local-only retention for private
   declaration docs?
5. Ratify source-definition documentation surviving decorators and property
   lowering unchanged?
6. Ratify implementation-method documentation first, trait-method fallback
   second, and trait-method documentation for generic bound dispatch?
7. Should CommonMark be the documented presentation format immediately, or
   should hover treat all content as escaped plain text in the first version?
