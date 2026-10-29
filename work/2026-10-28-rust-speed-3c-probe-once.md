# Rust Speed Step 3c Part 2: Dictionary Lookups Probe Once

Date: 2026-10-28

## Problem

`match mut counts.lookup(key)` lowers to a `contains_key` test, then, on
the `Found` side, an entry loan that probes the dict again. A found key was
hashed twice. A `Missing` arm such as `counts[key] = 1` boxed the `1` only
for the runtime to take it out of the box again.

## Changes

- `element_loans::fused_entry_probes` finds each `contains_key` temporary
  that is read only by the branch after it, where the branch's `true`
  block has that block as its only predecessor and begins with an entry
  loan of the same dict and key.
- For those tests the emitter calls `aura_direct_map_probe_slot` (or its
  `int64` key form). It returns the value's slot, 1 for a key that is
  present in a map shared with another thread, or null for a missing key.
  The test is `slot != 0`, and the loan follows a slot above 1 or takes
  the runtime walk.
- `d[key] = value` on a `dict` with `int64` values passes the value as its
  word to `aura_direct_map_set_index_int64`.

## Measurements

Callgrind instructions, Linux, base = step 3b. All 30 programs print the
same output.

| Program | Before | After | Change |
| --- | ---: | ---: | ---: |
| `lru_cache` | 24.5M | 22.7M | -7.6% |
| `interpreter` | 33.3M | 31.0M | -7.1% |
| `lzw_compress` | 3.6M | 3.3M | -6.9% |
| `bank_ledger` | 10.9M | 10.2M | -6.7% |
| `word_freq` | 26.2M | 25.2M | -4.2% |
| `csv_aggregate` | 33.0M | 31.8M | -3.4% |
| `text_diff` | 5.5M | 5.3M | -2.8% |

## Gate

3c's gate (word_freq and lru_cache at most 2x Rust) is still missed. At the
bench size (50,000) word_freq runs 377M instructions against Rust's 139M
(2.7x) and lru_cache 248M against 67M (3.7x). What
remains is mostly per-`str` box work: allocating, tracking, and freeing a
box for every word, and moving values into and out of boxes on list
appends.
