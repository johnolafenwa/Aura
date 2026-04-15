# Modules And Visibility

Aurora supports local file modules with `import`, `from ... import ...`, and `public` visibility boundaries. Modules let you organize code across files and control what is exposed to other parts of your project.

## Importing A Module

Use Python-style import syntax to bring in a module by its file path:

```python
import helpers.math
```

This resolves to `helpers/math.au` relative to the current source root. Call public functions through the module path:

```python
print(helpers.math.double(value=5))
```

Namespace imports also work for classes and enums:

```python
import pkg.types

counter = pkg.types.Counter(value=4)
status = pkg.types.Status.Ready
```

Module-qualified type annotations are supported:

```python
counter: pkg.types.Counter = pkg.types.Counter(value=4)
```

## Importing Names Directly

Use `from ... import ...` to bring a name into the local scope:

```python
from helpers.counter import Counter
```

This is the most concise way to use types without repeating module paths. You can import functions, classes, enums, and traits.

## `public` Visibility

Top-level items are private by default. Mark items with `public` to make them available to other modules:

```python
public def double(value: int32) -> int32:
    return value * 2
```

For classes, both the class itself and its fields/methods have independent visibility:

```python
public class Counter:
    public value: int32

    public def read(borrow self) -> int32:
        return self.value

    def internal_reset(borrow mut self):
        self.value = 0
```

Across module boundaries:

- importing a private top-level item is rejected
- reading a private field is rejected
- calling a private method is rejected
- keyword construction only exposes `public` fields -- you cannot set a private field from another module
- trait impls defined in imported modules still participate in generic bounds and method lookup

Within the same module, all members are accessible regardless of visibility.

## Packages And Dependency Imports

When a file lives under a package with `Aurora.toml`, the package's `src/` directory is the source root. Local imports work the same way:

```python
import helpers.math    # resolves to src/helpers/math.au
```

Dependencies declared in the manifest are imported by package name:

```python
import util.math       # resolves to the util dependency's src/math.au
```

See [18-packages-and-workspaces.md](18-packages-and-workspaces.md) for the full package system.

## Maintained Examples

- [examples/modules/simple_import.au](../examples/modules/simple_import.au) with helpers under [examples/modules/helpers](../examples/modules/helpers)
- [examples/modules/namespace_import_types.au](../examples/modules/namespace_import_types.au) with modules under [examples/modules/pkg](../examples/modules/pkg)
- [examples/modules/trait_impl_imports.au](../examples/modules/trait_impl_imports.au) with modules under [examples/modules/pkg](../examples/modules/pkg)
- [examples/packages/local_path_dependencies/app/src/main.au](../examples/packages/local_path_dependencies/app/src/main.au) with a sibling dependency

## Current Limits

- module resolution is local-file based plus package dependencies from local paths or git repositories
- registry-style version resolution and publishing are not implemented yet
