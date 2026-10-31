# VS Code debugger

- Requested by John on 2026-10-31, after the sequence builtins and before
  the 0.11.0 release.
- Specified in ADR-0067.

## What was built

- `aura build --debug`: `SourceLocation` and `SourceName` MIR markers,
  lowered only for a debug build. The LLVM backend writes a compile unit,
  one subprogram per Aura function, statement locations, and variables at
  their stack slots. It runs no passes and writes at `OptimizationLevel::None`
  in one unit. The link keeps every section, forces in
  `aura_debug_render`, skips `strip`, and runs `dsymutil` on macOS.
- `aura_debug_render` in the runtime: the `print` text of a value, with a
  `str` quoted. It never runs `Display` code and never fails.
- `tools/vscode-aura/debug/aura_lldb.py`: an lldb summary for `AuraValue`
  that calls `aura_debug_render` by address, because the runtime's own debug
  information also names the function and makes the name ambiguous.
- `tools/vscode-aura/src/debug.js`: the `aura` debug type, which builds the
  file and starts `lldb-dap`, plus **Debug File** and the Debug lens.

## Checks

- `run_pass_fixtures_lower_the_same_mir_with_source_locations`: every
  run-pass fixture's debug MIR, with its markers removed, serializes the
  same as its normal MIR.
- `run_pass_fixtures_emit_debug_objects`: every run-pass fixture emits a
  debug object that LLVM verifies.
- `build_debug_keeps_dwarf_and_the_debugger_renderer` (CLI).
- `debug_session.test.js`: a real Debug Adapter Protocol session with
  `lldb-dap` that stops at two breakpoints and reads ints, a `str`, a
  `list[str]`, and a class's fields. It is skipped where no `lldb-dap` or
  `target/debug/aura` exists. It passed locally with lldb-dap 18.

## Gotchas

- Since LLVM 19, inkwell's `insert_declare_at_end` asserts, because it
  treats the debug record LLVM returns as an instruction. The backend calls
  `LLVMDIBuilderInsertDeclareRecordAtEnd` directly.
- lldb reads a C base type by its name. Two base types with one name, such
  as an 8-bit signed and an 8-bit bool, confuse it, so each size and
  encoding has its own `__aura_<kind><bits>` name.
- `DW_ATE_unsigned` is 0x07. 0x08 is `DW_ATE_unsigned_char`.
- lldb-dap shows a pointer's address before its summary. A runtime value is
  therefore a one-field record with a summary and no children.
- lldb-dap reports a stop once for each stopped thread. A client must drop
  the queued stops after `continue`.

## Not yet

- Unions, callables, and classes with a field of more than one word.
- Lexical scopes. Two sibling-block bindings of one name both show.
