# 0140. Mutable Module Storage

Freestanding programs own their execution environment. A module can allocate a mutable cell with a literal initializer. The cell occupies writable, naturally aligned storage in the object. The target's startup code zeroes .bss and installs the contents of .data before calling an export. Calling a second export reaches the same cell. The defining module owns the storage even when several modules import its name.

A hosted task may change state through an immutable binding to a sync handle. Replacing a module binding would race with reads of the handle itself. Hosted module declarations reject mut and point to sync.AtomicInt and sync.Mutex in the diagnostic. In executable scripts, the existing task-owned mut local keeps its scope. A task-local module binding would require per-task storage, initialization and cleanup across suspension. Defer that feature.

Safe code can snapshot inline values and assign to the cell. A view into a cell could survive a call that modifies it through another name or a foreign callback. Reject views of the cell and its projections, including exclusive call arguments. A local snapshot supplies a stable place for code that takes a view. The verifier treats each read as fresh. Freestanding callers arrange synchronization across interrupt and core entry points.

A module cell's initializer has no startup function. Scalars, structural field constructors and fixed array literals supply constant storage images. A constructor with an explicit initialization method or an invariant requires build-time evaluation before it can initialize a cell. General constant expressions use the project's shared evaluator once available.
