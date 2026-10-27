# Rust Speed Step 2e Part 2: Element Loans In Generated Code

## Goal

Part 2 of step 2e of the Rust-speed plan (part 1 is in
`2026-10-27-rust-speed-2e-class-layouts.md`): make
`counters[i].count += 1` change the field where it lies. MIR lowers it to
an element loan of `counters[i].count`: `BeginElementLoan`, a `ReadLoan`,
a `WriteLoan` of the sum, and `EndLoan`. Each of these walked the element
path in the runtime by name, so the `field_write` probe cost 6,765
instructions per write after part 1.

The step plan proposed keeping class instances inline in a list's buffer,
as plain records are (2c). Instances own boxes for their `str` and
collection fields, so that form needs a drop and a clone per element and
every list operation to learn it. Lists already keep any element in a box
of its own once generated code lends it (the `Boxes` form of 2d), and a
typed instance keeps its fields in one block (part 1), so this part
follows loans through those boxes instead.

## Changes

- `native_codegen_element_loans.rs` follows an element loan in generated
  code when the function uses the loan only through its place: reads and
  writes of it, or one lend of it to one runtime operation. The loan must
  select one element of a local list, or of a list in a field box of a
  local instance kept in place. When it begins, generated code checks the
  list's access code and the position, and keeps the address of the place
  in a variable:
  - an element of a list of scalars lies in the list's storage;
  - a scalar or plain-record place inside a class element lies at an
    offset in the block of the element's box, after the element's class
    check;
  - a box slot (an element of a list of boxes, or a field box of a class
    element) is lent as it is, as a field lend lends a field box.
  A loan that writes also checks that the list's storage is its own (see
  below). Each read, write, and lend uses the address, and a loan that
  missed (address 0) takes the runtime walk as before.
- `aura_direct_element_path_probe`, which a missed loan calls, counts the
  miss on the lists the path selects from. A list of `Value`s, or one
  whose boxes another list shares, moves its elements into boxes of its
  own once the misses reach an eighth of its length, as
  `aura_direct_vec_lend` does. The probe edits the collection only when
  that applies, so a loan of a list of strings costs no edit.
- A typed instance appended with one reference joins the list in its own
  box (`aura_direct_vec_push_in_place`, `ListElements::push_box`), so a
  list built by appends is in the `Boxes` form from the start and its
  loans hit on the first pass. The box was allocated anyway, so the
  append also frees one box fewer.
- MIR: an assignment to a field of a list element whose right side reads
  the same list (`cells[0].value = cells[0].value + cells[1].value`)
  failed with AU4001, invalid loan flow. MIR now evaluates the right side
  before it lends the element for the write.

## Shared Storage

Two lists share storage after `copy()` until either changes, and a list
of boxes shares its boxes too. Generated code that changes an element in
place must not reach the other list. Scalar lists already had the rule:
a list's room, which the runtime sets on each write only when no other
value shares the storage, and which every runtime access takes back. This
part applies it to the other forms and closes two gaps:

- The `Boxes` form now reports room (it reported none, since generated
  code never appends a box), and a loan that writes through it checks the
  position against the room.
- A box its holder owns (a field box, or an element box) is read through
  the holder without an access of its own, so a copy made that way left
  the box's room in place while its storage became shared. A copy of a
  list of instances copies each instance and shares each field's list:
  writing `self.totals[i]` through the original then changed the copy as
  well. This was already reachable after part 1, through a method that
  writes a field's list element in place. A read of a held box now takes
  back its room, as an access does. The room is an atomic word, since a
  shared holder's readers can do this on several threads at once.

## Verification

- Runtime tests: `Boxes` room in the list form tests, `push_box`, a held
  read taking back room (`native_element_boxes_tests.rs`), appended
  typed instances keeping their boxes (`native_class_layouts_tests.rs`),
  and probes moving lists into boxes, giving shared boxes back, and
  leaving lists of strings alone (`native_runtime_tests.rs`).
- Emitter test: which loans generated code follows and which it refuses.
- New fixtures: `run-pass/typed_element_loans_in_place.au` (fields of
  every scalar kind, nested classes, snapshots, negative positions, lends
  of element and field boxes, lists of lists, scalar lists, lists in
  fields, and copies of lists of instances and of lists of lists changed
  on either side), `run-pass/element_field_assign_reads_list.au`, and
  `run-fail/typed_element_loan_out_of_bounds.au`.

## Measurements

Callgrind instruction counts, Linux container, release builds,
cross-language suite at the check size. Before is part 1, after adds this
part.

| Program | Before | After | Change | After vs Rust |
| --- | ---: | ---: | ---: | ---: |
| `pubsub_broker` | 146.8M | 124.4M | -15.3% | 11.8x |
| `order_book` | 18.9M | 16.2M | -14.4% | 21.2x |
| `binary_trees` | 16.7M | 15.0M | -10.2% | 10.2x |
| `spreadsheet` | 33.2M | 31.1M | -6.2% | 18.1x |
| `mini_browser` | 773.1M | 739.5M | -4.3% | 22.7x |
| `sql_engine` | 1,808.4M | 1,758.2M | -2.8% | 37.1x |

`file_io`, `kv_server`, and `http_api` also drop by 2% to 5%, though
they move by several percent between runs. The largest increases are
`task_pool` (+2.8%, which moves with thread scheduling: three more runs
of each build gave 40.7M to 41.2M) and `template_render` (+0.4%).

Operation probes: `field_write` (`counters[i].count += 1` on a list of
instances with a `str` field) goes from 6,765 instructions per write to
28.0, against 7.0 for Rust. `class_field_read` stays at 22.0 and
`list_push` at 30.3.

Before the probe split, every missed loan counted its miss, and the edit
that counting takes made `template_render` 3.0% slower: its
`template.texts[i].clone()` loans of `str` elements, which generated code
does not follow, paid for an edit that could never help. Only loans that
generated code follows count misses now (`aura_direct_element_path_promote`),
and the count edits the collection only when a list on the path could
move.

## Gates

- The `field_write` probe is 28 instructions per write, not the plan's
  5. The fast path is about 24 instructions: the list's access code,
  bounds, and room, the element box and its class check, the block
  address check, then the load, add, and store. LLVM keeps the list
  checks in the loop because it cannot tell that the field store does not
  change the list's header. Hoisting them needs alias information the
  backend interface does not pass yet.
- `order_book` is 21.2x Rust, `regex_match` 14.6x, and `bank_ledger`
  5.5x, against 3x. In all three, a large part of what is left is making
  new lists: each `[]` or `list[int64]()` is a box, a type tag that costs
  about 200 instructions (`set_explicit_runtime_type_tag`: 12.5% of
  `regex_match`, 5.5% of `bank_ledger`, 4.1% of `order_book`), and a
  runtime call for the first append. That is the next change after
  part 3.
