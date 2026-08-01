# ADR-0042: Aura product identity

- Status: Accepted
- Date: 2026-08-01
- Release decision: pre-publication identity migration for v0.2.0-preview
- Semantic effect: none

## Context

The language, compiler, runtime, documentation, and editor tooling were
developed under the working name Aurora. The command-line executable already
used `aura`, and source files already used `.au`. No package, tag, or release
has been published, so the first technical-preview release can use one identity
without a compatibility layer.

## Decision

The product and language name is **Aura**. The CLI remains `aura`, and the
source extension remains `.au`.

All maintained technical identities move atomically:

- the compiler crate is `aura-compiler`, with Rust library name
  `aura_compiler` and static archive `libaura_compiler.a`;
- direct-runtime symbols use the `aura_direct_*` prefix;
- native cache identity strings use Aura and the default cache root is
  `~/.cache/aura`; an old cache directory is ignored and is neither migrated
  nor deleted;
- public environment variables use the `AURA_*` prefix, and no old-name
  fallback is accepted;
- package manifests are `Aura.toml` and lockfiles are `Aura.lock`; only those
  names participate in package discovery;
- diagnostics, the language server, VS Code extension, publisher identity,
  TextMate scope, snippets, package metadata, repository URLs, documentation,
  and release artifacts use Aura;
- release archives use `aura-v<version>-<target>` and the documentation archive
  uses `aura-docs-v<version>`.

This is an identity migration only. It changes no source-language syntax,
typing rule, ownership rule, runtime result, scheduling contract, diagnostic
code, or backend behavior.

## History policy

Existing ADR bodies, work notes, the language proposal, and changelog history
retain the name that truthfully describes their period. The ADR index tells
readers that documents before ADR-0042 use the former working name. Maintained
product documentation describes the current Aura surface directly.

## Compatibility

Because no public release exists, there is no compatibility shim. Old
environment-variable names and old manifest filenames are not recognized. A
stale cache under the old default directory is ignored in place. Tool package
names, server identity strings, scopes, archive layouts, and repository links
all expose only the Aura identity.

## Verification

The repository identity gate inventories tracked paths and text outside the
explicit history zones. Package tests prove discovery uses only `Aura.toml` and
`Aura.lock`; CLI tests prove old environment-variable names are ignored; cache
tests pin the Aura cache identity and default directory; compiler and parity
tests pin the renamed ABI; LSP and extension tests pin their server, package,
publisher, scope, and bundled paths. Full CI, reference integrity, and the
release-artifact checksum pass are required before the preview tag moves.

## Consequences

The first published preview has one user-facing name. Pre-ADR-0042 material
remains searchable and truthful without leaking its working identity into the
maintained product surface.
