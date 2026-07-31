# Fresh-eyes corpus lane D results

Date: 2026-07-31

## Method

Lane D contains exactly seven new day-4/day-5-style programs, numbered 24
through 30. They were authored from the maintained root README, manual,
learning pages, and tutorials only. Compiler tests, test fixtures, and existing
example programs were not inspected.

Each final source was checked and then executed with both forced routes:

```text
target/debug/aura run --backend mir PROGRAM
target/debug/aura run --backend direct PROGRAM
```

Program 30 binds `127.0.0.1:0`, uses the returned ephemeral loopback address,
and closes its listener, exchange, response, task group, and task result through
their lexical/structured scopes. The network runs required host loopback access;
the restricted command sandbox correctly returned `io.Error.PermissionDenied`,
while the authorized host run below passed on both backends. No fixed port,
external service, or persistent file is used.

## Exact results

### 24 - Array construction, indexing, and update

Source: `24_array_construction_index_update.au`

MIR status: `0`

MIR stdout:

```text
[2, 3]
9
150
18
12
```

MIR stderr: empty.

Direct status: `0`

Direct stdout:

```text
[2, 3]
9
150
18
12
```

Direct stderr:

```text
aura: building native program...
```

Parity: status and program stdout match exactly; no program diagnostic was
emitted.

### 25 - Array elementwise and scalar forms

Source: `25_array_elementwise_scalar.au`

MIR status: `0`

MIR stdout:

```text
1.5
9.0
5.0
1.0
```

MIR stderr: empty.

Direct status: `0`

Direct stdout:

```text
1.5
9.0
5.0
1.0
```

Direct stderr:

```text
aura: building native program...
```

Parity: status and program stdout match exactly; no program diagnostic was
emitted.

### 26 - Array map and reductions

Source: `26_array_map_reductions.au`

MIR status: `0`

MIR stdout:

```text
9.0
30.0
1.0
16.0
7.5
```

MIR stderr: empty.

Direct status: `0`

Direct stdout:

```text
9.0
30.0
1.0
16.0
7.5
```

Direct stderr:

```text
aura: building native program...
```

Parity: status and program stdout match exactly; no program diagnostic was
emitted.

### 27 - Integer Array wrapping and saturating modes

Source: `27_integer_array_modes.au`

MIR status: `0`

MIR stdout:

```text
-2147483648
2147483647
2147483647
-2147483648
2147483647
```

MIR stderr: empty.

Direct status: `0`

Direct stdout:

```text
-2147483648
2147483647
2147483647
-2147483648
2147483647
```

Direct stderr:

```text
aura: building native program...
```

Parity: status and program stdout match exactly; no program diagnostic was
emitted.

### 28 - Structured Queue work distribution

Source: `28_queue_work_distribution.au`

MIR status: `0`

MIR stdout:

```text
91
```

MIR stderr: empty.

Direct status: `0`

Direct stdout:

```text
91
```

Direct stderr:

```text
aura: building native program...
```

Parity: status and program stdout match exactly; no program diagnostic was
emitted. The two workers may receive different subsets, while the printed sum
of squares is deterministic and proves that the closed Queue distributed all
six jobs exactly once.

### 29 - Retrying worker with a typed error

Source: `29_retry_typed_error.au`

MIR status: `0`

MIR stdout:

```text
ready
3
```

MIR stderr: empty.

Direct status: `0`

Direct stdout:

```text
ready
3
```

Direct stderr:

```text
aura: building native program...
```

Parity: status and program stdout match exactly; no program diagnostic was
emitted. The worker returns the user-defined `RetryError.Waiting` on its first
two calls and succeeds on the third call.

### 30 - Ephemeral loopback HTTP worker/client workflow

Source: `30_loopback_http_worker.au`

MIR status: `0`

MIR stdout:

```text
200
POST /double 21
server complete
```

MIR stderr: empty.

Direct status: `0`

Direct stdout:

```text
200
POST /double 21
server complete
```

Direct stderr:

```text
aura: building native program...
```

Parity: status and program stdout match exactly; no program diagnostic was
emitted. The worker binds an ephemeral listener, publishes its address through
a Queue, handles one request, and is observed through its `TaskResult`.

## Findings

- All seven final programs check and run successfully on both forced backends.
- Exit status and user-program stdout have exact MIR/direct parity for every
  program. Neither backend emitted a program diagnostic.
- The direct invocations also emitted the documented human-mode launcher
  progress line `aura: building native program...` on stderr because each
  program had a cold content key. This is CLI progress, not user-program
  stderr; the MIR route has no native-program artifact build step.
- No compiler bug or undocumented language limitation was found in lane D.
- Terminology friction: Array/scalar arithmetic applies one scalar to every
  element, but the numeric-array reference correctly avoids calling this
  general broadcasting because shape broadcasting is not implemented.
- Bootstrap friction was self-correcting: a fallible network workflow cannot
  itself be `main`, because bootstrap `main` accepts only `int32` or `None`.
  The checker reported this directly, and program 30 uses a small `int32`
  wrapper around its `Result[None, io.Error]` workflow.

No compiler production code was changed for this lane.
