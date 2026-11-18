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
119,877 ms for the set. These measurements missed the target. The checkpoint
therefore authorized the runtime implementation for primitive keys.

Ubuntu verification detected that the collection naming check treated
`Heap.push` as a list method. The scanner now recognizes imported heap
receiver types and still rejects `list.push`. The ordered-key fixture uses
a name outside the historical map-compatibility checks.

The runtime holds primitive elements in a deque or binary heap and
primitive map keys in a B-tree. A shared snapshot detaches on mutation.
A source module can lower calls to its own private helpers. User imports
cannot call the type-erased helpers or construct their hidden store.

Five fixtures passed with identical expected output on MIR and the direct
backend. They cover independent deque and map snapshots, consuming stored
RNGs, trait-defined class keys, B-tree rebalance and numeric list swaps.
Both negative storage diagnostics matched their snapshots. The four
runtime storage tests, native host argument-count checks and all 18
manual examples passed. The benchmark runner checks a checksum for every
collection and language before reporting medians.

The first direct ABI stored each concrete int64 in one word. Against
Rust on 8,192 keys, this reduced sorted map to 2.46 times Rust and
sorted set to 2.06 times Rust. Heap still took 4.46 times Rust and
deque took 31.27 times Rust. All four workloads beat the Python
references. A call profile attributed 6.55 million instructions to
class field lookups in the four workloads. The next optimization opens a
local deque or heap once when its only uses are standard pushes, pops
and release.

With the confined storage optimization, all four checksums matched Rust
and Python in every sample. Median Aura times for deque, heap, map and set
were 0.132, 0.485, 4.941 and 2.963 ms. Their Rust ratios were 2.10, 0.90,
2.23 and 2.17. The timings, reference implementations and workload
definitions are in `benchmarks/ordered_collections/README.md`.

Empty-pop tests compared the complete diagnostics. The optimized
native operations initially omitted the standard method from the call
chain. A pop now carries its method's definition position so the error
can record the same frame as MIR. The fixtures exercise an empty deque
and a drained heap. Ubuntu also caught two missing sibling-module
qualifiers in the confinement test.

The dedicated GitHub Ubuntu runner measured the release built at
`1171e6c0` on 2026-11-18. Aura took 2.80 times Rust for deque, 1.03
for heap, 2.02 for map and 1.87 for set. All four beat Python. On a
shared-host repeat of the `c9fc4b7c` release, the wall-time ratios for
map and set exceeded three, at 3.08 and 3.98. The full measurements
and clocks are in the benchmark README. [Hosted results](https://github.com/johnolafenwa/Aura/actions/runs/44201719086/job/134465583204)
