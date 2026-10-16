# Batch 5 design checkpoint: lean native binaries

Status: **Proposed 2026-10-16, awaiting owner ratification**

Date: 2026-10-16. Source baseline: `b516bd5` (main after
[PR #54](https://github.com/johnolafenwa/Aura/pull/54)).

This checkpoint settles the design that
[roadmap Batch 5](14-priority-roadmap.md#priority-batches) leaves open:
separate the runtime from compiler-only code, partition dependencies, and
link only the components a program uses. Release-profile tuning, dead
stripping, and the first size table were pre-Batch-1 item 4 and are not
repeated here.

The checkpoint first records what a native executable contains today and
why the linker cannot remove it. Each section then states the chosen rule,
the alternatives considered, and the evidence the implementation must pin.
[Owner questions](#owner-questions) lists the choices that need a ruling
before implementation starts.

## Measured Starting Point

All numbers below are from a release build of `b516bd5` on Linux x86-64
with the pinned Rust 1.95.0 toolchain, GCC 13.3, and GNU ld 2.42. Programs
were built with `aura build --backend direct`.

| Artifact | Bytes |
| --- | ---: |
| `aura` compiler | 16,630,416 |
| `libaura_compiler.a`, the runtime archive user programs link | 132,235,406 |
| Native hello world, `examples/basics/hello_world.au` | 5,057,904 |
| Reference agent, `examples/agents/tool_runner/` | 5,384,768 |
| Rust hello world, same profile, for scale | 305,880 |

The agent is only 6% larger than hello world. Nearly all of a native
executable is fixed runtime cost that every program pays.

The published macOS arm64 table in the
[Performance chapter](../docs/manual/performance.md#executable-size) records
1,586,968 bytes for hello world at `d9fc799`. That is an older commit on a
different platform and linker, so it is not compared with the Linux numbers
here. Batch 5 re-measures both platforms from one commit.

### What Hello World Contains

The table attributes the 3,628,894 bytes of named symbols in hello world,
built with `AURA_NATIVE_KEEP_SYMBOLS=1`, by crate or module. The remaining
file bytes are read-only data, unwind tables, and relocations.

| Component | Symbol bytes | Needed by hello world |
| --- | ---: | --- |
| `rustls` | 454,787 | no |
| `ring` | 425,852 | no |
| `aura_compiler::runtime_value` | 357,757 | partly |
| `aura_compiler::builtin_modules`, the checker's module registry | 209,150 | no |
| `gimli`, `addr2line`, `rustc_demangle`, from the std panic printer | 218,413 | yes, see [non-goals](#goals-and-non-goals) |
| `toml`, `toml_edit`, `winnow` | 174,593 | no |
| `aura_direct_*` runtime entry points | 120,209 | partly |
| `serde_json` | 87,491 | no |
| `aura_compiler::sema` type helpers | 83,599 | partly |
| `url`, `idna` | 65,150 | no |
| `aura_compiler::native_runtime` | 64,781 | partly |
| `tungstenite`, `http` | 66,658 | no |
| Generated Aura code | about 40,000 | partly |

Roughly 1.5 MB of hello world is TLS, cryptography, TOML, URL, WebSocket,
HTTP, and JSON code that the program never calls.

## Why The Linker Keeps It

User executables already link with `--gc-sections` on Linux and
`-dead_strip` on macOS. Those flags remove only code that nothing reachable
refers to. Today, six structural links make almost the whole runtime
reachable from `main`.

| ID | Cause | Evidence |
| --- | --- | --- |
| R1 | One `Value` enum holds every resource inline: `TlsStream`, `WebSocket`, `ProcessChild`, `FfiHandle`, and more. Any `Value` drop, clone, or format reaches the drop glue for all of them. | The call graph of hello world has the direct edge `main` → `aura_direct_run_root` → `drop_in_place<Box<dyn rustls::State<ClientConnectionData>>>`. |
| R2 | Host builtins dispatch through one string-keyed function, `evaluate_host_builtin_with_args`, a 23,862-byte `match` over names such as `"toml::is_valid"`. Calling any host builtin keeps every builtin. | `runtime_value.rs`, 70 string arms. |
| R3 | Codegen emits a wrapper, a thunk, and a default-argument binder for every builtin-module function, used or not. | Hello world contains 110 `aura_fn_*` functions, including `process.start`, `net`, `fs`, and `json` wrappers. |
| R4 | The runtime rebuilds the checker's builtin namespace registry at startup to find host metadata. This pulls checker type construction into every program. | `native_runtime.rs` calls `builtin_modules::host_builtin_metadata`, which builds every `*_namespace` table. |
| R5 | Types cross from generated code to the runtime as JSON text, which the runtime parses with `serde_json` into the checker's `Type`. | `native_runtime.rs` decodes signatures and union types with `serde_json::from_str::<Type>`. |
| R6 | Dependency features build more than Aura uses. `rustls` default features compile `aws-lc-rs` beside the `ring` provider Aura installs. `tungstenite`'s `rustls-tls-native-roots` feature adds `rustls-native-certs`. | `cargo tree -e features -i rustls`. |

A seventh cost is not about reachability. The shipped runtime archive is
the whole compiler as a staticlib, 132 MB with Cranelift, the parser, and
the checker. Every installation downloads it, and every user link reads it.

## Goals And Non-Goals

Goals:

- A runtime crate that contains no compiler-only code, and a shipped
  runtime archive built from it alone.
- Executables whose contents follow the program: a component's code is
  present only when the program can reach it.
- Gates that fail when an unused component reappears, on Linux and macOS.

Non-goals:

- No backend change. Cranelift stays; the backend decision is Batch 7 under
  [ADR-0064](decisions/0064-native-backend-strategy-and-codegen-boundary.md).
- No `panic=abort` and no rebuilt standard library. The runtime unwinds to
  implement task failure and cancellation, so the std panic printer and its
  symbolizer stay. A Rust hello world with the same profile is 305,880
bytes, which sets the floor.
- No change to language semantics, diagnostics, exit codes, or standalone
  behavior. Executables keep their embedded source for diagnostics.
- No freestanding profile. That is Batch 12.

## A. Crate Layout

**Rule.** Split `aura-compiler` into three crates with a one-way dependency
order.

| Crate | Kind | Contents | Depends on |
| --- | --- | --- | --- |
| `aura-core` | rlib | Types the runtime must see: `Type`, `UnionType`, `FunctionParamContract`, `ClosureCallKind`, integer bounds, `Diagnostic`, spans, runtime frames, limits, and the `BinaryOp`, `UnaryOp`, and `ReceiverKind` operator enums | `serde`, behind a feature only the compiler enables |
| `aura-runtime` | staticlib and rlib | `native_runtime`, `runtime_value`, `runtime_reactor`, `union_runtime`, the JSON and byte codecs, text methods, randomness, runtime configuration, FFI, and task stacks | `aura-core` and the runtime dependencies |
| `aura-compiler` | rlib | Lexer, parser, AST, checker, analysis, MIR, the MIR interpreter, native codegen, packages, formatter, and builtin module signatures | `aura-core`, `aura-runtime` as an rlib, Cranelift |

The `aura` CLI depends on `aura-compiler`. Release archives ship
`lib/aura/libaura_runtime.a` in place of `libaura_compiler.a`, next to the
existing `native-link-args.json`. The native cache format version changes
because the runtime identity changes.

The MIR interpreter stays in `aura-compiler`. It serves `aura run --backend
mir` in-process and the forced parity matrix, so it never needs to be in a
user executable. [Section E](#e-mir-runtime-launcher-fallback) covers the
one place it is linked into executables today.

**Gate.** A test runs `cargo tree -p aura-runtime -e normal` and fails if
it lists `cranelift-*`, `aura-compiler`, or any other compiler-only crate.
The dependency order makes a compiler import from the runtime a build
error, so no source scan is needed.

**Alternatives.**

- One crate with a `runtime-only` Cargo feature was rejected. Workspace
  feature unification builds the CLI with both halves, so the gate could
  not see a regression.
- One staticlib per component, such as TLS or WebSocket, was rejected.
  Each Rust staticlib carries its own copy of std, so two cannot be linked
  into one executable.
- Linking user programs through `rustc` against rlibs was rejected. It
  would need a Rust toolchain on every user machine.

## B. Reachability Follows Use

The crate split shrinks the archive but not the executables. Executables
shrink only when R1 through R5 are removed, so that the linker's existing
unused-section removal can do its job.

### B1. Resources Behind Handles

**Rule.** Resource variants leave the `Value` enum. One variant,
`Value::Resource`, holds a reference-counted trait object. Each resource
kind implements the trait in its own module. Only that module's
constructor creates the concrete type, so its drop glue and methods are
reachable only when the program can call the constructor.

Each resource value is already a shared `Arc` handle, such as
`TlsStreamValue { inner: Arc<TlsStreamState> }`, so the change replaces a
concrete `Arc` with a trait-object `Arc` and adds no allocation. Ownership,
move, close, and cleanup behavior stay exactly as they are. Display text
and runtime type names for each resource stay byte-identical.

**Alternative.** Boxing each resource variant separately was rejected. It
keeps each variant's drop glue in the `Value` drop function, so the link
from R1 remains.

### B2. One Entry Point Per Host Builtin

**Rule.** Each host builtin becomes its own runtime function. Generated
code calls it by symbol, so a program that never calls `toml.parse` never
references it. The MIR interpreter calls the same Rust functions through a
table in `aura-compiler`, where size does not matter. The string-keyed
dispatcher is removed.

### B3. Static Host Metadata

**Rule.** The runtime never rebuilds a checker registry. Codegen emits the
metadata a call site needs, such as parameter names and defaults for
runtime diagnostics, as constant data in the program's object. The checker
keeps one source of builtin signatures, and codegen derives the emitted
data from it.

### B4. Emit Only Reachable Functions

**Rule.** Before native emission, a MIR reachability pass starts from
`main` and module initializers. It keeps a function when a call, a
callable value, or a trait implementation for a constructed type refers to
it. Codegen skips the rest, including unused builtin-module wrappers,
thunks, and default binders.

The pass is a backend input, so a later backend gets it for free. It must
not change evaluation order, module initialization, or which diagnostics a
program can raise.

### B5. Binary Type Descriptors

**Rule.** Types cross the codegen and runtime boundary as a compact,
versioned binary descriptor that codegen emits as constant data. A small
decoder in `aura-core` reads it. The runtime no longer parses JSON to
learn a type, so `serde_json` is present only in programs that use the
`json` module.

**Alternative.** Deferring descriptors to Batch 7 was considered. It would
leave `serde_json` and the checker's type deserializer in every program,
so Batch 5 includes it.

## C. Dependency Partition

**Rule.** Every runtime dependency enables only the features Aura uses.

- `rustls` uses `default-features = false` with the `ring` provider that
  the runtime already installs. `aws-lc-rs` leaves the build.
- `tungstenite` keeps the trust behavior it has today. Secure WebSocket
  clients pass no connector, so `tungstenite` builds its root store from
  the platform's certificates. Aura's own TLS client uses the bundled
  `webpki-roots` set. Batch 5 records this
  difference and does not change it.
- Compiler-only dependencies move to `aura-compiler`: `cranelift-*` and
  the cache's use of `sha2`. `toml` and `sha2` also stay in the runtime,
  because the `toml` module and the `bytes` hashing functions use them.

The work note records, for each Aura module, which dependencies an import
of it may bring into an executable.

| Aura module | Dependencies it may bring in |
| --- | --- |
| `net` with TLS | `rustls`, `ring`, `webpki-roots` |
| `net` WebSocket | `tungstenite`, `http`, `url`, `rustls-native-certs` |
| `net` HTTP | `httparse` |
| `toml` | `toml`, `toml_edit`, `winnow` |
| `json` | `serde_json` |
| `bytes` encodings | `base64`, `sha2` |
| extern functions | `libffi` |

## D. Measurement And Gates

**Rule.** Size work is measured on both maintained platforms and guarded
in CI.

- `scripts/bench-binary-size.py` gains a component attribution mode. It
  builds each subject with `AURA_NATIVE_KEEP_SYMBOLS=1` and groups symbol
  bytes as the table above does.
- An absence gate in the CLI tests builds hello world with symbols kept
  and fails if it contains symbols from `rustls`, `ring`, `aws_lc`,
  `tungstenite`, `toml`, `toml_edit`, `url`, `idna`, `httparse`, `libffi`,
  `serde_json`, or the builtin namespace registry.
- Presence gates prove that each component still arrives when used: a
  program that imports only `json` contains `serde_json` and no `rustls`,
  and a TLS client program contains `rustls`.
- A size budget fails CI when hello world or the reference agent grows
  more than 10% over the recorded baseline for its platform. The recorded
  baseline moves only in a reviewed change.
- The Performance chapter's executable-size table is re-measured from one
  clean commit on Linux x86-64 and macOS arm64, with the archive size.

The expected result for hello world is between 1.0 and 1.5 MB on Linux.
That figure is an estimate from the attribution above, not a target. See
[owner question 3](#owner-questions).

**Preserved gates.** The forced MIR and direct parity matrix, standalone
execution with Cargo unavailable, exact source diagnostics in standalone
executables, native cache identity, the compiler coverage floors of
96.46% lines, 97.33% functions, and 95.23% regions, and 100% language
server coverage.

## E. MIR-Runtime Launcher Fallback

Today `aura build --backend auto` tries the direct backend and, if
codegen fails, links a launcher that embeds serialized MIR and runs it with
the MIR interpreter. That launcher needs the interpreter and the MIR data
model in the executable, so it cannot be built from a runtime-only archive.

**Rule, proposed.** Retire the launcher. `aura build` uses the direct
backend only, and the `--backend` flag accepts `direct` as its one value
so Batch 7 can add another. `aura run --backend mir` stays, because the
parity matrix depends on it.

**Precondition.** Before the launcher is removed, a census builds every
example, tutorial program, fixture, and benchmark input with `auto`. Any
program that falls back is either fixed in the direct backend in an
earlier PR or proven unreachable from checked programs. The direct backend
still has explicit "does not support" paths, such as some wide-integer
operations, so this census is real work.

**Alternative.** Keep the launcher and ship a second archive that holds
the compiler and runtime together, used only for fallback executables.
Fallback executables stay at today's size, and the release ships both
archives.

## F. Delivery Plan

Each step is one pull request with a green hosted run on Linux and macOS.

| Step | Change | Behavior change |
| --- | --- | --- |
| 0 | Baseline and gates: attribution mode, absence and presence tests recorded against today's output, dependency feature trim from section C, and removal of the tracked 1 MB `crates/aura-compiler/src/mir.rs.orig` | none |
| 1 | Extract `aura-core` | none |
| 2 | Create `aura-runtime`, ship `libaura_runtime.a`, update packaging, installer, smoke tests, and cache format | archive name |
| 3 | Resources behind handles, B1 | none |
| 4 | Per-builtin entry points and static metadata, B2 and B3 | none |
| 5 | Reachability pass and binary type descriptors, B4 and B5 | none |
| 6 | Fallback census and retirement from section E, final measurements, Performance chapter, roadmap, and task board | `aura build` backends |

The absence gate starts by recording today's contents and tightens as
steps 3 through 5 land. It reaches the full list in step 5.

## Owner Questions

1. **Crate layout.** Three crates as in [section A](#a-crate-layout).
   Recommended. The alternative is two crates, with the shared types left
   inside the runtime crate and the compiler depending on it.
2. **Launcher fallback.** Retire it after the census, as in
   [section E](#e-mir-runtime-launcher-fallback). Recommended. The
   alternative keeps it with a second shipped archive.
3. **Size acceptance.** Use the absence and presence gates plus the 10%
   regression budget, and publish the measured sizes. Recommended. The
   alternative is a hard byte target, such as 1.5 MB for hello world on
   Linux, which the estimate above cannot yet support.
4. **Binary type descriptors in Batch 5.** Include them, as in
   [B5](#b5-binary-type-descriptors). Recommended. The alternative defers
   them to Batch 7.

## Ratification Record

Pending. The owner's rulings on the questions above will be recorded here,
and the status line will change to Ratified.
