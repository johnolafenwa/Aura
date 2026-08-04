# Platform installation and Aura documentation highlighting

## Goal

Publish detailed installation instructions for macOS, Linux, Windows through
WSL 2, and the VS Code extension. Replace Python syntax highlighting on Aura
examples with a custom VitePress integration backed by the maintained Aura
TextMate grammar. This batch changes no language or compiler behavior.

## Inventory

- The documentation site had no Aura language registration. VitePress used
  Shiki's built-in Python grammar because maintained Aura examples were fenced
  as `python`.
- Maintained rendered documentation contained 294 Python-labeled Aura blocks;
  repository tutorials contained another 325. The 619 labels spanned 55 files.
- Fifty-one blocks already used the canonical `aura` label.
- The historical, non-rendered language proposal contains 61 Python fences and
  remains unchanged.
- The existing Downloads page provided one short installation path. It did not
  provide platform prerequisites, persistent PATH setup, WSL setup, native
  toolchain setup, or end-to-end editor verification.
- Two maintained platform passages still referred to 0.2 archives after the
  0.3 release.

## Work completed

- Added an installation hub and dedicated guides for macOS, Linux, Windows 11
  through Ubuntu 24.04 on WSL 2, and VS Code.
- Documented prerequisites, architecture checks, verified installation,
  persistent PATH configuration, first execution, native build toolchains,
  upgrades, custom prefixes, WSL filesystem placement, and focused recovery
  steps.
- Documented Marketplace, Open VSX, command-line, VSIX, and WSL remote
  extension installation. The guide distinguishes the bundled syntax/client
  components from compiler-owned semantic service through `aura lsp`.
- Registered Aura as a custom VitePress Shiki language. The loader consumes
  `tools/vscode-aura/syntaxes/aura.tmLanguage.json` directly, so documentation
  and editor highlighting share one grammar.
- Converted all 619 maintained Python-labeled Aura blocks to the canonical
  `aura` label without changing their source contents. Maintained docs and
  tutorials now contain 673 Aura fences and zero Python fences.
- Retired `python` as an Aura fence language in the reference-integrity gate.
  A new identity test rejects `python` or `au` labels in maintained Aura
  documentation while preserving the historical proposal exclusion.
- Updated Downloads, Learn, the root README, the extension listing README,
  Supported Platforms, VitePress navigation, and generated LLM artifacts.
- Added a prominent homepage briefing for AI agents. It links directly to
  `llms.txt` and `llms-full.txt`, explains their roles, supplies a copyable
  instruction, and makes the Manual's normative status explicit. The homepage
  hero now links to the briefing.
- Added a compact installation rail directly below the homepage hero actions,
  with dedicated links for macOS, Linux, Windows through WSL 2, and the VS Code
  extension.
- Expanded Learn's installation chapter with a platform-guide directory and a
  complete VS Code setup path covering CLI discovery, Marketplace installation,
  language-server behavior, and WSL remote installation.

## Verification

- Test-first red evidence covered the five missing installation pages, absent
  highlighter module, Python-as-Aura reference classification, and all 619
  stale maintained fence labels.
- Focused installation/release tests: green.
- Reference-integrity unit tests: green.
- Identity tests, including the canonical-fence guard: green.
- Shiki loaded `source.aura` and tokenized a representative Aura function:
  green.
- `VITEPRESS_BASE=/Aura/ npm run docs:build`: green.
- Manual inventory: 39 pages, 270 fences, 210 canonical Aura blocks, 131
  verified blocks, 128 verified Aura blocks, and no missing normative section
  or executable feature example.
- Desktop rendering verified platform navigation, macOS instructions, and the
  visible `Aura` code-block label. A 390 by 844 browser pass verified the WSL
  page with responsive navigation, readable commands, no horizontal overflow,
  and no console error.
- Complete reference replay: green. It executed all 128 verified Aura blocks
  against the current compiler and retained exact Manual inventory and
  diagnostics.
- Extension package suite: 22/22 green, including the canonical grammar and
  bundled language-server behavior.
- All six local installation/download routes returned HTTP 200. Generated LLM
  documentation is current, the custom highlighter tests are green, and the
  post-run build tree remains 19 GiB with 31 GiB free.
- The agent briefing has three focused tests covering endpoint prominence,
  generated-summary quality, and containment of long URLs. Desktop and
  390-by-844 browser checks verified the anchored hero action, both document
  links, copy feedback, zero horizontal overflow, and responsive text wrapping.
- Three focused tests pin the homepage installation routes, responsive
  four/two/one-column behavior, and the Learn chapter's VS Code instructions.
  Desktop and 390-by-844 browser checks verified all four homepage links in the
  rendered hero, the Learn headings and commands, and zero horizontal overflow.
  Every linked platform guide and the Learn chapter returned HTTP 200 locally.

## Follow-up

PR #4 merged at `d9cac8a`. The homepage agent-docs and installation-navigation
follow-ups were authorized for direct pushes to `main`. No compiler release or
extension republish is required because the installed binaries and VSIX are
unchanged.
