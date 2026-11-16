# ADR-0107: C unions

- Status: Accepted
- Date: 2026-11-16
- Builds on: ADR-0077 (C structs), ADR-0106 (C array fields)
- Design checkpoint: `architecture_docs/29-ffi-v2-graphics-design-checkpoint.md`

## Context

SDL reports every event through `SDL_Event`, a union of about thirty
structs that share a first field. GLFW and raylib avoid unions in their
main interfaces, but SDL cannot be used without one: there is no other
way to learn that a key was pressed. Smaller unions appear across C
libraries wherever a value has more than one reading.

Aura's own union types, such as `int32 | str`, carry a tag and know which
member they hold. A C union has no tag. Which member has meaning is a
convention of each library, so the two cannot be the same thing.

## Decision

- **`extern "C" union Name:`** declares a C union with a block of members,
  written as the fields of an `extern "C"` class are. A member is a scalar,
  an `extern "C"` class, another union, or an `array[T, N]` of those.
  `union` is a word only between the ABI string and the name.
- **A union value is its bytes.** In Aura the union is a class with one
  field, `bytes: list[uint8]`, that holds exactly `sizeof` bytes. `Name()`
  holds zero bytes.
- **Each member is a read-only property.** `value.member` reads the
  leading bytes as the member's type and returns a new value. A `bool`
  read from a byte other than 0 or 1 fails with `AU4001`.
- **Each member has an associated function `Name.from_member(value)`**,
  which returns a union with that member stored and every other byte
  zero.
- **A union crosses wherever an `extern "C"` class does:** by value, by
  `mut` pointer, as a field, as an array element, and through
  `ptr[Name]`. By value it is classified as the platform classifies a C
  union: on x86-64 each eightbyte takes the class its overlapping members
  merge to, and on AArch64 a union is a homogeneous float aggregate only
  when every member is one of the same base type.
- **A `bytes` list of another length does not cross.** It fails with
  `AU4005` at the call or the member read.
- **A union is not Copy**, and neither is a class that holds one.

## How It Is Built

The parser turns the declaration into ordinary declarations, so no later
stage has a rule for unions beyond their layout:

- A class `Name` with the field `bytes`, a property for each member, and a
  `from_` function for each member.
- A class `__Name_members` whose fields are the members. The checker lays
  the union out from this class, and reports a member with no C layout
  against the union's name.
- For each scalar or array member, a class `__Name_box_member` with the
  one field `value`. A class member needs none.
- For each member, two extern functions with the symbol
  `aura.reinterpret`: one from the union to the member, one back.

A call of `aura.reinterpret` is not a call into C. Both backends write the
argument's C bytes into a zeroed buffer as large as the larger of the two
types, and read the result's type from the same buffer. The interpreter
does it in `ffi::reinterpret`. A built program does it in a stack slot,
with the runtime's packing routines for a type that is not a class of
scalars. The object file has no reference to the symbol.

The generated declarations take the place of the `union` word in the
source. Each generated token has the line of that word and a column that
is the word's column plus a multiple of `GENERATED_COLUMN_STRIDE`, the
scheme the fast iteration forms use. A diagnostic or a call chain that
lands in one therefore points at the union's declaration. The editor
analysis leaves the generated declarations out of the outline and of
hovers, lists the members as the union's children, and sends "go to
definition" on a member to the union.

The parser cannot know the size of a union, because a member can be a
class from another module. It writes the default of `bytes` as a
comprehension over `range(0)`, and the checker replaces the `0` with the
size once every class has its layout.

When a module exports an extern function, the names in the C layouts of
its parameters and result are qualified as its types are. Before, a struct
read from C by an imported function carried the unqualified class name.
Nothing read that name for a class with no methods, but a union has
properties, and the interpreter finds them by it.

## Alternatives

- **Members as assignable fields.** Rejected for now. `event.key.code = 3`
  would need a place that aliases part of another place under a different
  type, which Aura's ownership rules do not have. The program writes
  `bytes` or builds a new union.
- **Mapping a C union to an Aura enum or union type.** Rejected. There is
  no tag to read, and the convention for which member is live differs by
  library.
- **Reading members with methods, `event.get_key()`.** Rejected.
  Bindings mirror C headers, and `event.key.code` is what the header's
  reader expects.
- **`Name(member=value)` as the constructor.** Rejected. The class's real
  constructor takes `bytes`, and a second set of keyword arguments on the
  same call would need a rule of its own in the checker. An associated
  function is an ordinary declaration.
- **Teaching the checker and both backends about union members.**
  Rejected. The generated declarations reuse the property rewrite, the
  class machinery, and the struct paths of the FFI, and the one new
  operation is small in each backend.

## Consequences

- SDL's event loop can be written in Aura. Any C union whose members are
  scalars, structs, arrays, and unions can be declared.
- A member read copies the union's bytes and builds the member, through a
  runtime call in a built program. A loop that reads several members pays
  for each read.
- A module that declares a union also holds declarations whose names begin
  with `__Name_`. They are private to the module and hidden in the editor,
  but they appear in `aura mir` output and in the names of failed calls as
  `Name.member`.
- `symbol "aura.reinterpret"` can be written by hand on an extern function
  from one `extern "C"` class to another. It is no less safe than the
  foreign call it replaces, and it is behind the same `allow_ffi` switch.
- Bit-fields, packed unions, and anonymous members that C flattens into
  the enclosing struct have no spelling. A binding gives an anonymous
  union a name.
