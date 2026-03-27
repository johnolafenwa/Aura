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

## Maintained Examples

See [examples/modules/simple_import.au](../examples/modules/simple_import.au) with its helper modules under [examples/modules/helpers](../examples/modules/helpers).

See [examples/modules/namespace_import_types.au](../examples/modules/namespace_import_types.au) with its helper module under [examples/modules/pkg](../examples/modules/pkg).

See [examples/modules/trait_impl_imports.au](../examples/modules/trait_impl_imports.au) with helper modules under [examples/modules/pkg](../examples/modules/pkg).

## Current Limits

- module resolution is local-file based for now
- package manifests and external dependency resolution are still proposal-only
