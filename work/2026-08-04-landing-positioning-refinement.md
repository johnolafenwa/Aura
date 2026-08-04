# Aura landing-page positioning refinement

## Goal

Make the landing page explain Aura through three familiar language ideas:
Python-like syntax, Rust-like ownership, and Go-like task-based concurrency.
State clearly that Aura is a simple and safe compiled systems language for
agents and frontier ML systems.

## Work completed

- Replaced the hero promise with “Simple, safe systems programming.”
- Rewrote the hero description around native compilation, agents and frontier
  ML systems, Python-like syntax, Rust-like ownership, and Go-like tasks.
- Reframed the three homepage benefits around syntax, ownership, and tasks.
- Kept the compact Python/Rust comparison table and made Go's influence
  explicit in the hero, task benefit, and supporting positioning copy.
- Updated the page metadata and regenerated `llms.txt` and `llms-full.txt`.
- Extended the landing-page regression to pin the complete positioning.

## Verification

- `python3 -m unittest scripts.test_release_packaging`: 36 passed.
- Generated LLM documentation is current.
- The VitePress production build is green.
- Desktop and 390-by-844 browser renders show a clear hero hierarchy, readable
  supporting copy, and no horizontal page overflow.

## Follow-up

No compiler, runtime, language-server, or extension behavior changed. The
documentation-only commit can deploy through the Docs workflow without the
full compiler CI gate.
