# Systems data for 0.18

The freestanding kernel stores two samples inline, writes the second sample through a typed pointer and prints its value on the serial console. The boot test compares the complete transcript, including the deliberate overflow and source position, on the RISC-V and AArch64 boards.

The systems data benchmark runs the same 16-element rotating checksum in Aura, C and Rust. Before timing, each implementation must return 0x13579bdf for zero rounds and 0x82d80971 for 64 rounds.

Records 0140 through 0144 cover module storage, fixed arrays, typed pointers, C layout and integer operations. The design checkpoint gives the ownership and verification rules. Hosted shared state uses immutable bindings to synchronization handles. Raw operations use the package FFI permission. Static module initialization starts with literals and structural constructors.

Combined validation passed all 22 freestanding CLI integration tests, including both QEMU boot transcripts, and all 1,130 checked documentation examples. All 169 language-server tests passed with full statement, branch and function coverage. The six x86-64 freestanding routines returned matching values across all four builds. The largest Aura time relative to the faster of C and Rust was 1.47× for Collatz. [Freestanding measurements](../benchmarks/freestanding/README.md)
