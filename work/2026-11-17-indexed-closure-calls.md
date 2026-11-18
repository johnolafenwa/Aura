# Indexed Closure Calls

The reported field call fails when a single name between brackets can
denote either a type or an index. The compiler must resolve that name
against the collection being called. A literal index on a list field
already works. A shared callable held in a tuple element must support
repeated calls while the tuple continues to own it.

The final release benchmark, checksums and instruction counts for all six
workloads are in `benchmarks/closures/README.md`. On the same pinned CPU,
three runs put the original mutable-list loop at 6,765 ms and the final
loop at 250 ms. A shared list element used only for calls can stay in
place. A mutable element call with a bit mask in its argument can also use
its existing loan. The release wall-time ratios were 2.28 times Rust for
the shared list, 2.74 for the field, 2.19 for the mutable list and 1.11
for the mutable local. Creation and a fresh function argument took 0.15
and 0.66 times Rust. Seven paired CPU-time runs per workload put the
largest ratio at 2.49. All six Aura loops also finished ahead of Python.

A module-level decorator that captured a string reached task shutdown with
that string still on the creating task's native ledger. The release WIP
and the extended stored-call branch both reproduced it. The compiler boxes
handle-owning captures into runtime values at the storage boundary. A
cleanup test exercises the decorated function to shutdown and checks its
output, exit status and allocation/free counts.

The rebuilt compiler runs the decorated-functions program to completion
and frees all 57 allocated boxes. The tool runner matches its pinned
output and frees all 742. The numeric-call path now reads live callable
words directly from confined function boxes and list tables. Mutable
closures that exactly forward to a plain receiver's leaf method defer
both frame records until a trap. The overflow regression retains the
method, closure and caller locations on both backends.

The scalar receiver in the forwarded overflow case can stay entirely in
the callable's inline words. The test checks both backtraces and balanced
native allocation counts, including the zero-allocation case seen on ARM64.

The matching Linux compiler and runtime passed all 27 representation tests
and all nine cleanup tests. The Windows compiler, runtime and CLI suites
passed 2,128, 1,249 and 393 tests. Both indexed places passed the maintained
fixture suites on Windows.
