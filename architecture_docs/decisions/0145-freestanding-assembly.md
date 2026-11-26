# ADR-0145: Freestanding Assembly

Accepted 2026-11-24. See the [machine code checkpoint](../37-systems-machine-design-checkpoint.md).

## Decision

An assembly statement binds general-purpose registers to integer and boolean
values. The floating-point register class `fpreg` binds `float32` and
`float64`. Kernel mode excludes this class and its physical registers.
Boolean inputs use a byte holding zero or one. A boolean output is true
when its low byte is nonzero. Each input is evaluated once before assembly
runs. Number the outputs first and inputs second with LLVM's `$0`, `$1`
syntax. Write `$$` for a literal dollar. A braced placeholder can select a
register spelling, such as `${0:w}` for an AArch64 32-bit general register
or `${0:s}` for a single-precision floating-point register. The checker validates the register
class or fixed register against the selected target and operand width. An
output uses an early-clobber constraint because a template may contain
several instructions and overwrite it before reading every input.

Assembly can read and write memory and condition codes by default.
`pure` promises the outputs depend only on the inputs, with no memory
access or observable effects. `preserves_flags` excludes condition-code
clobbers. `noreturn` ends the source path. Fixed registers mentioned only
in the template need a corresponding operand or clobber declaration.

A separate module declaration contributes assembly text once per defining
module, including a module reached along several import paths. A module
declaration can supply the only entry routine. The formatter preserves
assembly strings as it preserves other string literals.

Both forms require a freestanding target and the package's FFI grant.
The source checker knows register and operand contracts. LLVM's target
assembler checks instruction spelling and encodings when writing the
object. The verifier treats assembly as a boundary that precludes proof.
