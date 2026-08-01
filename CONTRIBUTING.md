# Contributing

Aura uses test-first development. Add a failing fixture or focused regression before changing compiler, runtime, CLI, or language-server behavior, then update the maintained examples and manual in the same change.

Run the full local gate with:

```sh
npm ci
npm run ci
```

The supported tool versions are pinned in `rust-toolchain.toml` and `package.json`. Keep changes focused and reviewable; do not commit generated executables, coverage output, editor caches, or scratch evaluation corpora. Promote useful reproductions into named fixtures under `crates/aura-compiler/tests/fixtures/`.

The implemented contract is the manual and its Status and Compatibility page. The language proposal is design history, not permission to implement an otherwise undocumented syntax feature.
