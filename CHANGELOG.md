# Changelog

All notable user-facing changes will be recorded here. Aurora has not made its first tagged release.

## Unreleased — 0.1.0 technical preview

### Breaking changes

- Replaced the source `borrow` capability syntax with three declaration-stable forms: bare parameters and receivers provide logical shared access for every type, including copy types; `mut` provides exclusive mutable access; and `own` transfers ownership. Code that requires the old bare-copy snapshot contract must spell it as `own CopyType`.
- Changed bare `match` to shared matching. Use `match mut value` for mutable matching and `match own value` to consume a scrutinee or its payloads.
- Retired the old spellings: `value: borrow T` becomes `value: T`, `value: borrow mut T` becomes `value: mut T`, `borrow self` becomes `self`, `borrow mut self` becomes `mut self`, and the same removal applies after `match` and `for value in`. During one compatibility release, `borrow` remains reserved solely so the compiler can report these exact replacements; it is not accepted as an alias for the new syntax.
- Migrate a checkout with `python3 scripts/capability_migrate.py apply`, then verify the recorded migration with `python3 scripts/capability_migrate.py check`.
- Removed borrowed-return labels and `borrow`/`borrow mut` return capabilities, superseding the borrowed-return contract. Copy-valued borrowed returns become ordinary owned returns; APIs returning access into non-copy owners must instead return an owned result, handle, index, or expose the operation on the owner.
- Bumped the native artifact-cache format to `aurora-native-cache-v4`, preventing native artifacts compiled with the old capability metadata from being reused.

- Built a typed bootstrap compiler, MIR runtime, direct native backend, package/workspace support, structured concurrency, file/network/process APIs, LSP, VS Code extension, and maintained book.
- Froze syntax expansion while the 0.1 distribution, safety validation, editor responsiveness, and control-plane standard library are hardened.
- Made release archives carry a relocatable native runtime and linker manifest.
