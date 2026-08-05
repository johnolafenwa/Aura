# Modules And Visibility

Aura supports local file modules with `import`, `from ... import ...`, and `public` visibility boundaries. Modules let you organize code across files and control what is exposed to other parts of your project.

## Importing A Module

Use Python-style import syntax to bring in a module by its file path:

```aura fragment
import helpers.math
```

This resolves to `helpers/math.au` relative to the current source root. Call public functions through the module path:

```aura fragment
print(helpers.math.double(value=5))
```

Namespace imports also work for classes and enums:

```aura fragment
import pkg.types

counter = pkg.types.Counter(value=4)
status = pkg.types.Status.Ready
```

Module-qualified type annotations are supported:

```aura fragment
counter: pkg.types.Counter = pkg.types.Counter(value=4)
```

## Importing Names Directly

Use `from ... import ...` to bring a name into the local scope:

```aura fragment
from helpers.counter import Counter
```

This is the most concise way to use names without repeating module paths. You
can import public functions, classes, enums, traits, and module constants.

## Module Constants

Declare stable configuration and constructed immutable values beside the
functions that use them:

```aura check-pass
service_name = "planner"
public max_attempts: int64 = 3
retry_budget = max_attempts + 2

def main():
    print(service_name)
    print(retry_budget)
```

Constants initialize eagerly before `main`. Imported dependencies initialize
before the importing module, imports are visited in source order, and each
module initializes once. Within a module, a constant may use functions and
earlier constants. It cannot read itself or a later constant.

Module bindings cannot use `mut` and cannot be reassigned. Copy values read as
ordinary copies. Non-Copy values stay owned by the defining module and each
read grants shared access. Call `.clone()` when the type supports it and the
program needs independent owned data.

Export a constant with `public` and import it through either form:

```aura fragment
import settings
from settings import max_attempts as configured_attempts

def main():
    print(settings.max_attempts)
    print(configured_attempts)
```

## Import Aliases

Use `as` to choose a concise or collision-free local name for a module:

```aura fragment
import helpers.math as integer_math

print(integer_math.double(value=5))
```

Individual from-import entries may also be aliased:

```aura fragment
from helpers.counter import Counter as ReadableCounter

counter = ReadableCounter(value=2)
```

A from-import may mix direct and aliased entries. The alias changes only the
local spelling. Visibility, type identity, trait implementations, and module
resolution continue to use the original declaration.

## `public` Visibility

Top-level items are private by default. Mark items with `public` to make them available to other modules:

```aura check-pass
public def double(value: int32) -> int32:
    return value * 2
```

For classes, both the class itself and its fields/methods have independent visibility:

```aura check-pass
public class Counter:
    public value: int32

    public def read(self) -> int32:
        return self.value

    def internal_reset(mut self):
        self.value = 0
```

Across module boundaries:

- importing a private top-level item is rejected
- reading a private field is rejected
- calling a private method is rejected
- keyword construction only exposes `public` fields -- you cannot set a private field from another module
- trait impls defined in imported modules still participate in generic bounds and method lookup
- inferred clone-safety obligations on public generic functions and methods
  survive both namespace and direct imports

Within the same module, all members are accessible regardless of visibility.

## Packages And Dependency Imports

When a file lives under a package with `Aura.toml`, the package's `src/` directory is the source root. Local imports work the same way:

```aura fragment
import helpers.math    # resolves to src/helpers/math.au
```

Dependencies declared in the manifest are imported by package name:

```aura fragment
import util.math       # resolves to the util dependency's src/math.au
```

See [18-packages-and-workspaces.md](18-packages-and-workspaces.md) for the full package system.

## Maintained Examples

- [examples/modules/simple_import.au](../examples/modules/simple_import.au) with helpers under [examples/modules/helpers](../examples/modules/helpers)
- [examples/modules/import_aliases.au](../examples/modules/import_aliases.au) demonstrates module and from-import aliases
- [examples/modules/constants.au](../examples/modules/constants.au) demonstrates inferred, annotated, public, and dependent constants beside `main`
- [examples/modules/namespace_import_types.au](../examples/modules/namespace_import_types.au) with modules under [examples/modules/pkg](../examples/modules/pkg)
- [examples/modules/trait_impl_imports.au](../examples/modules/trait_impl_imports.au) with modules under [examples/modules/pkg](../examples/modules/pkg)
- [examples/packages/local_path_dependencies/app/src/main.au](../examples/packages/local_path_dependencies/app/src/main.au) with a sibling dependency

## Current Limits

- module resolution is local-file based plus package dependencies from local paths or git repositories
- registry-style version resolution and publishing are not implemented yet
