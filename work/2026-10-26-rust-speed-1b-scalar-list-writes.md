# Rust-speed plan step 1b: scalar list writes in place

Plan: https://claude.ai/artifact/Jx47RTWBdr1tNdCcNcaLwC (Phase 1, step 1b).

## What changed

- MIR resolves `__set_index` on a `list[int64]`, `list[float64]`, or
  `list[bool]` to `ListOp::SetIndexScalar(kind)`, like the existing
  `PushScalar` and `IndexScalar` specializations. The interpreter dispatches
  by member name, so it is unaffected.
- Native code checks the list view's access code, bounds, and the `room`
  word, then stores the element bits straight into the unboxed buffer. When
  the storage is shared or the list is in another form it calls the new
  `aura_direct_vec_set_index_scalar(list, index, bits, line, column, kind)`,
  which writes the element and refreshes the view.
- In-place element writes and appends no longer store the receiver back
  into a plain local or parameter. A `mut` parameter's caller holds the same
  handle; the write-through sink (for `with` managers and cleanup places) is
  fetched once at function entry, and only when the frame push went through
  the runtime, since an inline push implies no pending sinks.

## Measurements (callgrind, Linux container, check size)

- Probe `fl.au write` (float64 `*=` in a loop): 1,318 -> 26 instructions per
  update. The remaining cost is the access/len re-check after slow-path
  merges, the room check, the store, and the sink test; the plan's 10
  instruction gate needs the typed list layouts of step 2d.
- nbody 132.1M -> 35.4M (-73%), astar_pathfinding -35%, bytecode_vm -22%,
  text_diff -8%, regex_match -8%, lru_cache -8%. No program regressed:
  http_api showed +10% in the first run, but re-runs of the base binary give
  32.4M-32.7M, the same as the head.

## Found on the way

- Pre-existing: natively, a `with` manager's `__exit__` was skipped when a
  function holding a `mut` borrow of the manager's list field trapped. Repro
  in /mnt/project-files/rust-speed-plan/found-bugs/; reported, not fixed
  here. The fixture `manager_exit_sees_in_place_list_writes.au` covers the
  case this change touches.
