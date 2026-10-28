# Rust Speed Step 3d Part 1: String Operations Read In Place

Date: 2026-10-28

## Goal

Stop copying string operands that the operation only reads, and keep short
string results inside the value instead of on the heap.

## Changes

- `str + str` reads both texts where they lie and builds the result once
  with `Text::concat` (`concat_texts_checked` keeps the size limit).
- `starts_with`, `ends_with`, and `contains` read their argument in place.
  A literal argument goes through `aura_direct_string_affix_literal`, so no
  box is made for it.
- `replace` finds matches with `memchr::memmem` and copies a text without
  the pattern as it is.
- A copy of a `list[str]` element reads the list's str view
  (`aura_direct_string_from_view`) or the element
  (`aura_direct_vec_str_element`), and the following `clone()` of that
  fresh copy is dropped.
- String literals, `format_begin`, `split_whitespace`, `lines`, and the
  ASCII case maps build `Text` from `&str`, so results of at most 29 bytes
  stay inline.

## Measurements

Callgrind instructions, Linux, base = step 3c part 1.

| Program | Before | After | Change |
| --- | ---: | ---: | ---: |
| `word_freq` | 35.3M | 26.2M | -25.6% |
| `sort_records` | 35.2M | 30.0M | -15.0% |
| `template_render` | 24.9M | 21.6M | -13.2% |
| `http_api` | 28.5M | 24.9M | -12.7% |
| `text_diff` | 8.3M | 7.3M | -12.4% |
| `csv_aggregate` | 40.9M | 36.5M | -10.8% |
| `markdown_render` | 6.2M | 5.6M | -9.6% |
| `pubsub_broker` | 63.5M | 58.5M | -7.8% |
| `mini_browser` | 432.7M | 402.1M | -7.1% |
| `bytecode_vm` | 6.3M | 5.9M | -6.7% |

All 30 programs print the same output.

## Tried And Dropped

A per-dict lookup hint slot (remember the last probed index) made programs
1% to 3% slower and was removed.

## Remaining

Each `str` still lives in its own box with ledger and release work. Keeping
`str` values out of boxes in generated code is the next lever for the 2x
gates.
