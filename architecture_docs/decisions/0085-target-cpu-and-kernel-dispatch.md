# ADR-0085: A program picks its target CPU, and Array kernels pick theirs at run time

- Status: Accepted
- Date: 2026-11-11
- Builds on: ADR-0041 (numeric Arrays), ADR-0064 (LLVM backend)

## Context

LLVM compiles every program for a generic CPU, so a build is the same on
every host of one architecture. On x86-64 that means 128-bit SSE2
instructions. A CPU with AVX2 has 256-bit registers that the generated code
and the Array kernels never use. On arm64 the baseline already has NEON,
and there is no wider set every Apple or server CPU shares.

Two kinds of code are involved, and they ship differently:

- **The program.** `aura build` writes it for one machine or for many,
  and only the person who builds knows which.
- **The runtime.** One prebuilt library is linked into every program, and
  its Array kernels do the same work on every program's Arrays.

## Decision

- **`--target-cpu <name>` on `aura build` and `aura run`.** The name is
  `generic`, `native`, or on x86-64 one of `x86-64-v2`, `x86-64-v3`, and
  `x86-64-v4`. `generic` stays the default. `native` uses every
  instruction the build machine's CPU has. Another name ends the command
  with exit status 2 and lists the names the host accepts.
- **`AURA_TARGET_CPU`** names the CPU when the option is absent. The
  option wins.
- **A build for a CPU runs only on a CPU that has its instructions.** On
  an older CPU the program stops with an illegal-instruction signal, as a
  program from any compiler does. Aura adds no start-up check.
- **The run cache keys on the target CPU.** For `native` the key holds
  the CPU name and feature list LLVM detects, so a cache directory shared
  by two machines does not hand one the other's build.
- **Array kernels pick a copy at run time.** Each kernel in the runtime
  is compiled twice on x86-64: once for the baseline and once for AVX2.
  The first Array operation of a process asks the CPU which it has. A
  program built for `generic` therefore runs 256-bit kernels on a CPU
  with AVX2, and runs on one without.
- **`AURA_ARRAY_KERNELS=baseline`** makes a process use the baseline
  copies. Any other value uses what the CPU has. It exists to compare the
  copies and to rule the wide ones out when a result is in doubt.
- **The vector width never changes a result.** No kernel uses fused
  multiply-add, and no kernel lets the width decide the order of its
  floating-point operations. The backend does not contract a multiply and
  an add either, so a `native` build prints the same floats as a
  `generic` one.

## Alternatives rejected

- **Make `native` the default.** A binary that stops on the next machine
  is a bad default, and reproducible builds would end.
- **Free-form LLVM CPU names.** LLVM accepts a name it does not know and
  prints a warning. A short list that the command checks is a surface
  Aura can document and test.
- **A separate runtime library per CPU level.** It multiplies the
  release archives and the run cache, and still leaves `generic`
  programs on the narrow kernels.
- **Fused multiply-add in the wide copies.** It is faster and changes
  the low bits, so one program would print different sums on two
  machines.
- **AVX-512 copies.** They would add a third copy of every kernel for
  CPUs that are uncommon outside servers. The reordered reductions of
  ADR-0086 are laid out so a 512-bit copy can be added without changing
  a result.

## Consequences

- A program that uses Arrays gets 256-bit kernels on most x86-64
  machines with no build option.
- Generated code keeps to the baseline until the build asks for more,
  and `--target-cpu native` is the one switch for a program that runs
  where it is built.
- The runtime library carries two copies of each Array kernel on x86-64.
  A program that uses no Array links neither.
- `aura build --target-cpu native` is not reproducible across machines,
  and the documentation says so.
