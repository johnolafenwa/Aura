# brace-expansion advisory closure

## Goal

Restore the `npm run ci` audit gate after GHSA-mh99-v99m-4gvg was published
against every `brace-expansion` release up to and including `5.0.7`. The
advisory reached the tree only through development tooling, but the audit gate
is fail-closed, so no logical commit could be admitted while it was open.

## Work completed

- Confirmed the advisory has no lockfile-only closure: the vulnerable copies are
  pulled in by `minimatch@^2` under `glob` -> `test-exclude` -> `c8` and by
  `vscode-languageclient@9`, and only `brace-expansion@5.0.8` is patched.
- Raised the language-server coverage tool from `c8@^10.1.3` to `c8@^12.0.0`,
  which resolves `test-exclude@^8` -> `glob@^11` -> `minimatch@^10` ->
  `brace-expansion@5.0.8`. The repository's pinned Node 22.14.0 satisfies c8
  12's `^20.19.0 || ^22.12.0 || >=23` engine requirement.
- Raised the editor client from `vscode-languageclient@^9.0.1` to `^10.1.0`,
  which resolves the same patched `minimatch@^10` chain. The extension uses only
  the stable `LanguageClient` and `TransportKind` surface, which is unchanged
  across that major.
- Raised the extension's declared `engines.vscode` from `^1.90.0` to `^1.91.0`
  to match `vscode-languageclient@10`, and corrected the extension README's
  stated minimum editor version in the same pass.

## Verification

- `npm audit --audit-level=moderate` reports zero vulnerabilities.
- The 67-test language-server suite passes.
- The language-server coverage gate still reports enforced 100% statements,
  branches, functions, and lines under c8 12.
- `npm run check:extension` and the 13-test extension suite pass.
- The complete `npm run ci` gate that admits this change is the same gate run
  for the conditional-expression packet that follows it; both commits are cut
  from that one green tree, with this dependency change ordered first so the
  semantic packet's commit matches the gated tree exactly.

## Follow-up

None. The advisory is closed at the root of the dependency graph rather than
suppressed.
