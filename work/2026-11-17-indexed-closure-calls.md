# Indexed Closure Calls

The reported field call fails when a single name between brackets can
denote either a type or an index. The compiler must resolve that name
against the collection being called. A literal index on a list field
already works. A shared callable held in a tuple element must support
repeated calls while the tuple continues to own it.

The release benchmark and matching checksums for the six original
workloads are recorded in `benchmarks/closures/README.md`. The original
mutable list loop takes longer than Python's on this worker. Runtime
and compiler improvements under investigation must be measured against
that loop as well as against calls through fields and locals.
