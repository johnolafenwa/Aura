# Freestanding Machine Code

Status: design choices for implementation, 2026-11-24.

## Assembly

Use a contextual statement with a literal template and zero or more
operand clauses. The architecture is the freestanding target. Source can
use a triple-quoted string for a sequence of instructions.

```aura fragment
mut result: uint64 = 0
asm("movq $1, $0", output("reg", result), input("reg", value), options("pure", "preserves_flags"))
```

An input evaluates a scalar expression. An output names an existing mutable
scalar destination. Its declared or inferred type gives the operand width.
Use fixed-width integers for general registers. A register class selects a
suitable register, or a register name pins the operand. The checker knows
which names, widths and aliases the target accepts. Reject conflicting
constraints, unsupported types and invalid operand references before LLVM.
Number outputs first, then inputs, as LLVM does. Use AT&T syntax on x86.
Use LLVM's literal dollar escape in instruction text.

`clobber("rax", "memory")` declares additional registers or memory.
Conservatively assume memory and condition codes can change unless the
contract excludes them. `options("pure")` requires an output and promises
no observable effects or memory access. `preserves_flags` promises to
preserve condition codes. `noreturn` ends control flow and forbids outputs
and pure. Emit a side-effecting instruction by default.
`module_asm("...")` at module scope contributes text once to the object
and accepts no operands. Linker-visible definitions and external references
follow the object's ordinary symbol rules.

## Declarations

Reserve a qualified attribute namespace on module-level functions. Ordinary
decorators retain their existing meaning.

```aura fragment
@machine.interrupt("x86_64")
@machine.section(".text.irq")
@machine.align(16)
@machine.used
export "C" def tick() -> None:
    pass
```

- `@machine.interrupt("x86_64_error")` binds the hardware error code
  as one uint64 parameter. The ordinary x86-64 handler has no parameters.
  Both return through IRETQ. The other forms are `"aarch64"` for an EL1
  vector and `"riscv64"` for a machine-mode trap. Preserve interrupted
  state across arbitrary calls and use ERET or MRET. Interrupt entries
  cannot be called as ordinary Aura functions.
- `@machine.naked` requires one operand-free assembly statement. Its author
  owns the entire register and stack discipline, including the return.
- `@machine.link_name("symbol")` specifies the object symbol.
  `@machine.noinline` and `@machine.always_inline` are mutually exclusive.
  Reject conflicting entry conventions and invalid attribute arguments.
- `@machine.section("name")` and `@machine.align(power_of_two)` describe
  function or module storage placement. Connect to module state when its
  declaration is available on integration. `@machine.used` makes a
  definition reachable and retained even under linker section collection.

## Target And Trust

- **Which builds?** Restrict these constructs and new machine operations
  to freestanding builds. Assembly and hardware operations require the
  package's existing FFI grant. A source check must use the same target
  facts as object emission.
- **How does verification work?** Refuse to prove through assembly, even
  when marked pure. Report the boundary at the source and keep checks.
  A machine entry convention supplies no proof about incoming registers.
  A placement or inlining attribute supplies no logical premise.
- **How does editing work?** Preserve template spelling and whitespace in
  the formatter. Recognize clauses, attributes and Aura operand bindings
  in the language server. Treat literal assembly as opaque text.
- **How does kernel mode work?** Use an explicit `--kernel` option with
  x86-64 freestanding check/build. Select the kernel code model, disable
  vector and floating-point registers and disable the red zone throughout
  the object. Check the actual target settings and generated leaf routines
  when documenting the default. Keep soft-float ABI and any external
  helper requirements explicit.
- **How are hardware registers named?** Add typed operations to `mem`.
  Use `in_uint8/16/32` and `out_uint8/16/32` with a uint16 port. Use
  named operations for individual control, system and status registers
  so target and access direction are checkable without parsing arbitrary
  assembly strings.
- **What proves the ABI?** Inspect relocatable objects and retained linked
  sections. Boot the existing RISC-V and AArch64 boards and a long-mode
  x86 board. The x86 board must return from a software breakpoint and a
  real timer interrupt, write to serial, and exit with the expected device
  status. Use C storage when integration has no module-state implementation.
