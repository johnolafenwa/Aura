# Modules And Visibility

Aurora now supports local file modules plus `public` module boundaries.

## Importing A Module

Use Python-style import syntax:

```python
import helpers.math
```

Then call public functions through the module path:

```python
print(helpers.math.double(value=5))
```

Namespace imports also work for public classes and enums:

```python
import pkg.types

counter = pkg.types.Counter(value=4)
status = pkg.types.Status.Ready
```

Module-qualified type annotations now work too:

```python
counter: pkg.types.Counter = pkg.types.Counter(value=4)
```

## Importing Names Directly

Use `from ... import ...` when you want a direct local binding:

```python
from helpers.counter import Counter
```

This is also the most concise way to bring types into annotations and constructors without repeating a module path.

## `public`

Top-level items are private by default. Mark exported APIs explicitly:

```python
public def double(value: int32) -> int32:
    return value * 2
```

```python
public class Counter:
    public value: int32

    public def read(borrow self) -> int32:
        return self.value
```

Across module boundaries:

- importing a private top-level item is rejected
- reading a private field is rejected
- calling a private method is rejected
- keyword construction only exposes participating `public` fields
- trait impls defined in imported modules still participate in generic bounds and method lookup

Within the same module, private members remain usable.

## Packages And Dependency Imports

When a file lives under a package with `Aurora.toml`, Aurora now treats the package's `src/` directory as the source root.

Local imports still look the same inside the package:

```python
import helpers.math
```

Local path dependencies are mounted by package name:

```toml
[dependencies]
util = { path = "../util" }
```

```python
import util.math
```

The manifest directory owns dependency-path resolution, so `path = "../util"` is resolved relative to the package's own `Aurora.toml`.

Workspace roots are also supported:

```toml
[workspace]
members = ["app", "util"]
```

The current CLI writes `Aurora.lock` beside the active package manifest, or at the workspace root when the current package is part of a workspace.

## Maintained Examples

See [examples/modules/simple_import.au](../examples/modules/simple_import.au) with its helper modules under [examples/modules/helpers](../examples/modules/helpers).

See [examples/modules/namespace_import_types.au](../examples/modules/namespace_import_types.au) with its helper module under [examples/modules/pkg](../examples/modules/pkg).

See [examples/modules/trait_impl_imports.au](../examples/modules/trait_impl_imports.au) with helper modules under [examples/modules/pkg](../examples/modules/pkg).

See [examples/packages/local_path_dependencies/app/src/main.au](../examples/packages/local_path_dependencies/app/src/main.au) with its sibling dependency package under [examples/packages/local_path_dependencies/util](../examples/packages/local_path_dependencies/util).

See [examples/packages/workspace/app/src/main.au](../examples/packages/workspace/app/src/main.au) with its workspace root under [examples/packages/workspace/Aurora.toml](../examples/packages/workspace/Aurora.toml).

## Current Limits

- module resolution is local-file based plus local path dependencies
- registry-style version resolution, git dependencies, and publishing are not implemented yet
