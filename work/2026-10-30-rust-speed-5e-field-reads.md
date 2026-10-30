# Rust Speed Step 5e: Field Reads

Date: 2026-10-30

## Problem

After 5d, `aura_direct_instance_copy_field` was 12.7% of `mini_browser`
(inclusive), plus the release of each copy. Nearly all of it came from
`compute_style`, which runs for every node and every rule:

```aura
if rule.tag != "" and rule.tag != node.tag:
    continue
if rule.class_name != "" and not (rule.class_name in node.classes):
    continue
```

In MIR these are `Binary` comparisons and a `List(Contains)` call whose
operands are places such as `%t1.tag` and `node.classes`. The emitter's
`load_class_place` read a box field of an in-place instance by calling
`aura_direct_instance_copy_field`, which builds a new box with a copy of
the value (a whole list for `node.classes`), and the copy was released
right after the test.

## Change

`compile_rvalue_for_target` sets `borrow_field_reads` while it compiles
an rvalue that only reads its operands and gives back a scalar holding
none of them (`reads_operands_only`): a comparison, or a `Str`
`contains`/`starts_with`/`ends_with`/`len`, `List`
`len`/`is_empty`/`contains`/`index_of`/`count`, `Dict`
`len`/`is_empty`/`contains_key`, or `Set` `len`/`is_empty`/`contains`.

While it is set, `load_class_place` gives back a box field of an
instance that passes the class check as the field's own box, borrowed,
not marked as a temporary the instruction owns. The runtime path for an
instance kept out of place still makes an owned copy; the join passes
that copy, or null on the in-place path, in a second block parameter,
and `compile_rvalue_for_target` releases it once the rvalue is done (a
release of null does nothing).

The field cannot change during the read: operands are places or
constants, and nothing in these rvalues writes a field or runs user
code. A field that was moved out (a null slot) still takes the runtime
read, which reports it.

## Results

Callgrind, check size, release builds, 5d vs 5e:

| Program | 5d | 5e | Change |
| --- | ---: | ---: | ---: |
| mini_browser | 349,842,448 | 290,791,228 | -16.88% |
| http_api | 19,397,062 | 19,243,104 | -0.79% |
| template_render | 15,239,968 | 15,239,968 | 0.00% |
| sql_engine | 442,076,588 | 442,080,822 | 0.00% |

The other ten programs measured (docdb, markdown_render, spreadsheet,
interpreter, graph_paths, lru_cache, order_book, kv_server,
csv_aggregate, bank_ledger) moved by under 0.2%, all with identical
output.

`mini_browser` at bench size (500): Rust 0.21-0.23 s, 5d 1.35-1.51 s, 5e
1.14-1.29 s, so about 5.5x Rust.

## Gates

- mini_browser at most 4x Rust: MISSED (about 5.5x). What is left:
  `aura_direct_element_path_load` 10.7% inclusive (from `render`), list
  appends through fields (`vec_push_in_place`, `vec_push_scalar`), and
  dropping instance field blocks.
- template_render at least 15% fewer instructions: MISSED (unchanged). Its
  field reads go through element paths (`items[i].name`), which 5f
  compiles.

## Fix found on the way

The new fixture reads list fields of a module constant
(`SHARED.names.len()` inside a function that takes the instance as a
parameter). That already stopped 0.9.0 with "a module constant cannot
change": a read-only lend of a field to one runtime operation takes the
field out of an instance that is not kept in place and puts it back
after, and taking it out of a frozen instance is a write. Now the
runtime path of a read-only lend whose root is a local instance asks
`aura_direct_value_is_frozen` first, and for a frozen instance lends a
copy of the field (read by the runtime) and releases it when the lend
ends. Other instances still take the field out, so nothing else copies.

A first version released each runtime-path copy at the end of the
rvalue from the block parameter of its join, which LLVM rejected when
the read sat inside a branch of the rvalue (`callable_view_result_shared`).
The copies now go through up to four variables set to null when the
rvalue begins (`field_read_copies`), and a fifth read in one rvalue copies
as before.

## Not done

- A field passed as a function argument is still copied. A callee may
  keep a borrowed parameter (by retaining it), and a field box is linked
  by no ledger, so lending it needs the callee-side proof the element
  loans use.
- Appends to list fields still take the runtime path, because a field box
  never lets its list view run ahead of its list.
