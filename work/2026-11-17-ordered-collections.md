# Ordered collections

The collections checkpoint specifies a deque, min/max heap, B-tree map and
sorted set. The implementation lives in the source module
`crates/aura-compiler/std/collections.au`. ADR-0117 records the public
contracts, ownership and storage choices.

## Verification

The first fixture failed with an unresolved `collections` import before
implementation. The independent runtime stress fixture inserted 215 keys in
permuted order, then removed them in a second permutation. Each removal
checked its value, membership and length. The same fixture exercised
transfers between deque ends and heap push/pop order.

The full combined source fixture passed on MIR. The focused language-server
test passed for all four types. A separate fixture checked a user-defined
`Ord` implementation, including an update with an equal but distinct key.

On the release baseline at 8,192 keys the source implementation took
24 ms for the deque, 732 ms for the heap, 94,036 ms for the map and
119,877 ms for the set. These measurements missed the target. Runtime
storage work and final native parity checks are in progress.
