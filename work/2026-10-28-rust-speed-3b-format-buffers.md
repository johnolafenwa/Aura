# Rust Speed Step 3b: f-Strings Build One Text

Date: 2026-10-28

## Goal

Gate from the Phase 3 brief: no `format_*` box allocations in
`template_render` or `text_diff`. Each f-string now makes one box, its
result, and nothing else.

## What made boxes before

- `aura_direct_format_push_value` rendered every part into a new `String`
  first, so a `str` part was copied twice.
- A part with a format spec, such as `{cents % 100:02}`, boxed its `int64`
  value to reach `aura_direct_format_push_formatted`.
- In the profiles, most `aura_direct_format_begin` calls were not
  f-strings. The linker folds `aura_direct_string_literal` into the same
  code, and callgrind shows both under one name. `template_render` boxed
  ten literals per `escape` call (five `replace` calls), and `text_diff`
  boxed the twelve literals of `make_line`'s word list on every call.

## Changes

- `aura_direct_format_push_value` appends a `str` part in place and
  renders only other values.
- `aura_direct_format_push_i64_formatted` formats an `int64` part with a
  spec from its word.
- `aura_direct_string_replace_literal` reads both literal arguments of
  `replace` where the program keeps them.
- `aura_direct_vec_push_str_literal` adds a `str` literal of a typed list
  literal as an element with no box.

## Measurements

Callgrind instructions, Linux, base = step 3d part 1. All 30 programs print
the same output. `kv_server` and `task_pool` vary by up to 20% between
runs of identical binaries and are left out.

| Program | Before | After | Change |
| --- | ---: | ---: | ---: |
| `template_render` | 21.6M | 15.2M | -29.8% |
| `text_diff` | 7.3M | 5.5M | -24.5% |
| `spreadsheet` | 12.9M | 11.3M | -13.0% |
| `sort_records` | 30.0M | 26.5M | -11.5% |
| `csv_aggregate` | 36.5M | 33.0M | -9.6% |
| `markdown_render` | 5.6M | 5.1M | -8.7% |
| `sql_engine` | 650.8M | 624.3M | -4.1% |

Gate: met. In both programs the only f-string box left is each result.
