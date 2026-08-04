# Aura f-string interpolation highlighting

## Goal

Correct the VS Code syntax-highlighting defect where an Aura interpolation
such as `{lang}` inherited the surrounding f-string color. Keep the canonical
grammar shared by VS Code and VitePress.

## Test-first evidence

The new Dark+ Shiki regression initially failed because
`f"Lang: {lang}"` was emitted as one string-colored token. In particular, no
standalone opening-brace token existed.

## Implementation

- Assign the standard `constant.character.format.placeholder.other` and
  `punctuation.definition.interpolation` scopes to interpolation braces.
- Assign `meta.embedded.expression` to interpolation contents.
- Recognize embedded identifiers as `variable.other.readwrite` and reuse the
  maintained Aura operator rules inside an interpolation.
- Consume `{{` and `}}` as literal f-string escapes before interpolation
  matching.
- Pin the TextMate structure in the extension package tests and the visible
  color result through the documentation site's Shiki integration.

Under VS Code Dark+, the regression now observes:

- f-string text: `#CE9178`
- interpolation braces: `#569CD6`
- embedded identifier: `#9CDCFE`

## Verification

- `npm run test:extension`: 22/22 green.
- `node --test docs/.vitepress/aura-language.test.mjs`: 3/3 green.
- `npm run package:extension`: green; the VSIX contains the corrected grammar.
- Release metadata tests: 9/9 green with Aura at 0.3.1 and the independently
  versioned extension at 0.3.2.
- Release packaging tests: 36/36 green, including the source-built
  extension-only dispatch path.
- Identity tests: 15/15 green.
- `npm ci --ignore-scripts`: green with zero vulnerabilities.
- Documentation production build: green.

## Publication

The user authorized an extension-only 0.3.2 publication. Visual Studio
Marketplace and Open VSX already contain immutable extension version 0.3.1,
so the VSIX version moves independently to 0.3.2 while the compiler, language
server protocol identity, documentation release, and CLI remain Aura
0.3.1-preview. The source-built workflow dispatch skips native CLI builds and
GitHub Release publication. The workflow requires an existing GitHub Release
only when it downloads an already-built VSIX; a VSIX built from an explicit
`source_ref` no longer hits that unrelated guard.

## Protected files

The existing `personal/file_ops.au` modification and untracked ADR-0022 draft
remain unstaged and unchanged.
