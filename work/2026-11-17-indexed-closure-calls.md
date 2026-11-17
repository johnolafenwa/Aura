# Indexed Closure Calls

The reported field call fails when a single name between brackets can
denote either a type or an index. The compiler must resolve that name
against the collection being called. A literal index on a list field
already works. A shared callable held in a tuple element must support
repeated calls while the tuple continues to own it.

The release benchmark and matching checksums for the six original
workloads are recorded in `benchmarks/closures/README.md`. Three
single-core runs put the original mutable-list loop at 6,765 ms and the
optimized loop at 1,133 ms. A shared list element used only for calls can
stay in place. A mutable element call with a bit mask in its argument can
also use its existing loan. The shared and mutable list calls and mutable
local still exceed the three-times-Rust target on this host.
