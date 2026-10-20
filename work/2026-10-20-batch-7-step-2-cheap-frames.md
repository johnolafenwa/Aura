# Batch 7 Step 2: Cheap Call Frames

## Goal

Cut the runtime cost of entering and leaving a native call, which was about
87% of `fib(30)`, without changing the frame contract.

## Work Completed

- Codegen emits one writable `DirectFrameDescriptor` per function. The
  prologue calls `aura_direct_enter_frame(descriptor)` with one argument in
  place of six. The runtime validates the descriptor's UTF-8 on the first
  call and sets its flag.
- The call stack stores a 16-byte entry per call. It builds the full frame
  record only when a diagnostic captures frames. A stack that spills to the
  heap keeps its allocation.
- `current_lightweight_task_id` reads a thread-local cell set beside the task
  context, in place of cloning the context.
- The per-worker task state map caches the last entry it returned.
  Insertion, removal, and teardown clear the cache.
- Returned-view records exist only for calls that receive mutable sinks or
  hand a view back. A plain call stores nothing there.
- The legacy `aura_direct_enter_call_with_frame` entry point stays for its
  tests and pushes a boxed frame.

## Verification

- New runtime tests cover the descriptor layout against the codegen offsets,
  first-call validation, rejection of invalid or null descriptors before the
  frame activates, the depth limit, nested scope state, and the sparse
  returned-view stack.
- The codegen test checks that function entry uses the descriptor ABI.
- `cargo test --no-fail-fast` passes except the known root-user failure in
  `package_io_helpers_cover_local_error_paths`.
- The backend parity matrix, clippy, fmt, and the native binaries gate pass.
  Hello world is 964,136 bytes and the reference agent 1,900,768 bytes.
- Callgrind: `fib(30)` runs 568,735,408 instructions against 1,917,708,454
  at `aa52bfc`. Paired GO to DONE medians on Linux: `fib(30)` 34.7 ms
  against 129.3 ms. 10,000 tasks: 448 ms against 446 ms.

## Follow-Up

- Step 3 adds explicit retain and release in MIR.
- About 340 instructions per call remain in entry and exit. Passing the task
  state pointer through generated calls would remove the remaining lookup.
