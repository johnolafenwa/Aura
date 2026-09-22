# Modules And Visibility

This chapter shows how to split a program across files. A module is one local
`.au` file. You bring it in with `import` or `from ... import ...`, and you
choose what it exposes with `public`.

## Importing A Module

Import a module by its dotted file path:

```aura fragment
import helpers.math
```

This resolves to `helpers/math.au` relative to the current source root. Call
its public functions through the module path:

```aura fragment
print(helpers.math.double(value=5))
```

Classes and enums work the same way through a namespace import:

```aura fragment
import pkg.types

counter = pkg.types.Counter(value=4)
status = pkg.types.Status.Ready
```

A type annotation can use the module-qualified name too:

```aura fragment
counter: pkg.types.Counter = pkg.types.Counter(value=4)
```

## Importing Names Directly

Use `from ... import ...` to bring one name into local scope:

```aura fragment
from helpers.counter import Counter
```

After this line you write `Counter` without the module path. You can import
public functions, classes, enums, traits, and module constants this way.

## Module Constants

A module constant is a top-level binding. Use one for stable configuration or
an immutable value that the module's functions share:

```aura check-pass
service_name = "planner"
public max_attempts: int64 = 3
retry_budget = max_attempts + 2

def main():
    print(service_name)
    print(retry_budget)
```

Constants initialize eagerly, before `main` runs. The order is fixed:

- Imported dependencies initialize before the module that imports them.
- Imports are visited in source order.
- Each module initializes once.
- A constant may use functions and earlier constants. It cannot read itself or
  a later constant.

Module bindings cannot use `mut`, and you cannot reassign them. Reading a Copy
value gives you an ordinary copy. A non-Copy value stays owned by the defining
module, and each read gives shared access. Call `.clone()` when the type
supports it and you need independent owned data.

Mark a constant `public` to export it. Other modules can then import it
either way:

```aura fragment
import settings
from settings import max_attempts as configured_attempts

def main():
    print(settings.max_attempts)
    print(configured_attempts)
```

## Import Aliases

Use `as` to give a module a shorter local name, or one that does not collide
with another name:

```aura fragment
import helpers.math as integer_math

print(integer_math.double(value=5))
```

You can alias each entry of a from-import too:

```aura fragment
from helpers.counter import Counter as ReadableCounter

counter = ReadableCounter(value=2)
```

One from-import may mix plain and aliased entries. An alias changes only the
local spelling. Visibility, type identity, trait implementations, and module
resolution all still use the original declaration.

## `public` Visibility

Top-level items are private by default. Mark an item `public` so other
modules can use it:

```aura check-pass
public def double(value: int32) -> int32:
    return value * 2
```

A class has its own visibility, and so does each of its fields and methods:

```aura check-pass
public class Counter:
    public value: int32

    public def read(self) -> int32:
        return self.value

    def internal_reset(mut self):
        self.value = 0
```

From another module, the checker enforces these rules:

- Importing a private top-level item is rejected.
- Reading a private field is rejected.
- Calling a private method is rejected.
- Keyword construction exposes only `public` fields. You cannot set a private
  field from another module.
- Trait impls defined in imported modules still take part in generic bounds
  and method lookup.
- Inferred clone-safety obligations on public generic functions and methods
  carry across both namespace imports and direct imports.

Inside one module, every member is accessible whatever its visibility.

## Packages And Dependency Imports

When a file lives in a package with an `Aura.toml` manifest, the package's
`src/` directory is the source root. Local imports work the same way:

```aura fragment
import helpers.math    # resolves to src/helpers/math.au
```

You import a dependency declared in the manifest by its package name:

```aura fragment
import util.math       # resolves to the util dependency's src/math.au
```

[18-packages-and-workspaces.md](18-packages-and-workspaces.md) covers the
package system.

## Maintained Examples

- [examples/modules/simple_import.au](../examples/modules/simple_import.au), with helpers under [examples/modules/helpers](../examples/modules/helpers)
- [examples/modules/import_aliases.au](../examples/modules/import_aliases.au): module and from-import aliases
- [examples/modules/constants.au](../examples/modules/constants.au): inferred, annotated, public, and dependent constants beside `main`
- [examples/modules/namespace_import_types.au](../examples/modules/namespace_import_types.au), with modules under [examples/modules/pkg](../examples/modules/pkg)
- [examples/modules/trait_impl_imports.au](../examples/modules/trait_impl_imports.au), with modules under [examples/modules/pkg](../examples/modules/pkg)
- [examples/packages/local_path_dependencies/app/src/main.au](../examples/packages/local_path_dependencies/app/src/main.au), with a sibling dependency

## Current Limits

- Modules resolve from local files, plus package dependencies from local paths
  or git repositories.
- Registry-style version resolution and publishing are not implemented.
