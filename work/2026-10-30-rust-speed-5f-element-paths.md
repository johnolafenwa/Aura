# Rust Speed Step 5f: Element Paths

Date: 2026-10-30

## Problem

After 5e, the runtime walk of element paths (`aura_direct_element_path_*`,
which parse the path string and walk it for every read, take, and store)
was 30-40% of three programs and 10-13% of a fourth:

| Program | Walk share | Source |
| --- | ---: | --- |
| mini_browser | 10.7% | `view node = nodes[index]`, then `node.tag`, `node.classes`, `node.children` |
| template_render | 40% | `template.texts[i]`, a `str` element of a list field |
| graph_paths | 42% | `adjacency[edge.source].append(Arc(...))` |
| docdb | 30% | `case json.Value.Object(fields)` payload views, then `fields.lookup(name)` |
| sql_engine | 13% | mixed |

Generated code already followed some element loans to their place
(`native_codegen_element_loans.rs`), but only a loan whose every use was
a `ReadLoan`, `WriteLoan`, a lend of the loan itself, or a pass to a
borrowing parameter. Each of the shapes above was refused:

- a read of a field of a view (`Place("node.tag")` in an rvalue) names a
  place under the loan, not the loan;
- a `str` element of a list field had no copy path of its own, so it
  walked `texts.[i]` from the instance;
- an append to an element is a runtime call whose receiver is the loan.
  The emitter loaded the receiver (a copy, which shares the element's
  storage), appended (which then copied the whole storage), and stored the
  copy back over the element: two walks and a copy of the element per
  append.

## Changes

- **Fields of a read-only class-element view.** The analysis accepts, for
  a read-only loan, a read of `loan.field` in an `Assign` and a read-only
  `Lend`/`EndLend` of `loan.field`, and marks the loan as one that lends
  its box (a `Slot` loan of the element box). `load_place` asks
  `load_native_loan_projection` first: the element box from the loan's
  slot, then the field read in its block by `load_class_slot` (the reader
  `load_class_place` now shares), with the view walk as the slow path. A
  read-only lend of such a field lends the field box as
  `lend_class_field` does for a local instance. With 5e, comparisons and
  `in` tests read the field without copying it.
- **`str` elements of list fields.** `load_field_str_element` copies
  `template.texts[i]` out of the field's box with the same string-view
  copy a local `list[str]` uses, after the class check; the index check
  at the loan's beginning calls `aura_direct_vec_check_index` on the
  field's box (`check_field_list_index`) instead of probing the path.
- **Adding to an element.** A runtime append, insert, extend, set add, or
  dict index write whose receiver is a loan (`added_receiver_loan`) makes
  the loan a `Slot` loan too, and `add_to_native_element` compiles the
  operation on the element's box where the outer list keeps it, as it does
  on a field box (no inline append, since a held box never lets its list
  view run ahead of its list). A list of plain records pushed through a
  held box now takes `aura_direct_vec_push_plain`, which keeps the records
  in the list's typed form; before, a field box's push boxed the record
  and turned the list into one of `Value`s, which every later copy of the
  list rebuilt record by record. A loan that missed takes the element out
  and puts it back, and so does any other view receiver of these
  operations (`adds_to_receiver`), instead of copying it.

## Fix found on the way

The backend parity run built `element_view_fields.au` natively and it
trapped with "a module constant cannot change". 0.9.0 does the same: a
read-only lend of a field through a `view` of an element of a frozen
module constant took the field out of the constant on the runtime path.
`frozen_lend_root` now also checks the value a one-element view selects
from, and the frozen branch copies the field through the view.

## Results

Callgrind, check size, release builds, 5e vs 5f, identical output:

| Program | 5e | 5f | Change |
| --- | ---: | ---: | ---: |
| graph_paths | 34,354,277 | 18,181,462 | -47.08% |
| template_render | 15,247,234 | 11,447,488 | -24.92% |
| mini_browser | 290,695,721 | 233,405,402 | -19.71% |
| sql_engine | 442,075,509 | 417,574,447 | -5.54% |
| lru_cache | 20,561,761 | 19,512,160 | -5.10% |
| interpreter | 25,779,182 | 25,506,436 | -1.06% |

The other thirteen programs measured moved by under 0.2%.

Path walks are now 0% of graph_paths and mini_browser and 4% of
template_render (`context[name]` through a borrowed `dict` parameter).

## Gates

- Path walks under 2% in graph_paths, docdb, and template_render: met for
  graph_paths, MISSED for template_render (4%) and docdb (29%). docdb's
  walks are reads through `json.Value` payload views (`case
  json.Value.Object(fields)` and then `fields.lookup(name)`), which this
  step does not follow.
- The 5e gate for template_render (at least 15% fewer instructions) is met
  by this step: -24.9%.

## Not done

- Payload views of enum variants (docdb).
- Mutable views read field by field (`view mut row = rows[i]`).
