# AGENTS.md

This repository is a monorepo for the Aura language, compiler, examples, tutorials, and editor tooling.

The default engineering mode for this repo is test-first development.

## Core Rule

Before implementing a new feature or fixing a bug:

1. add or update a failing test
2. implement the change
3. run the relevant verification commands
4. update examples, tutorials, and work logs when the user-facing surface changes

Do not treat examples and tutorials as passive documentation. They are part of the maintained surface of the project.

## Security-Sensitive Agent Delegation

For authorized defensive work that is likely to encounter cybersecurity
safeguards, delegate the security-sensitive subtask to a Daybreak Blue
sub-agent (`gpt-daybreak-blue-latest`) when that approved model is available.
This is the user's standing preference for this repository across sessions.

- Apply this to vulnerability investigation, security patch validation,
  adversarial or malformed-input regression tests, resource-exhaustion
  analysis, and related security-sensitive compiler/runtime review.
- Give the sub-agent a concrete, bounded task, the authorized repository scope,
  relevant findings, and explicit file ownership. Keep ordinary implementation,
  documentation, release preparation, and coordination with the primary agent.
- Preserve the test-first workflow, existing coverage gates, user changes, and
  applicable security-skill requirements. Review the sub-agent's evidence and
  changes before integrating them.
- Use only approved access. Delegation does not broaden authorization, bypass
  safeguards, or guarantee that a request will be accepted. If Daybreak Blue
  is unavailable or the task is restricted, report the limitation rather than
  silently substituting another model or attempting to evade the restriction.

## Persistence And Stop Conditions

Treat explicit completion conditions literally. Agents may work for as long as
needed to complete the authorized task. This repository imposes no duration
cap, continuous-session limit, start-time requirement, elapsed-time tracking
requirement, or timer-based stop condition.

When the user specifies a completion target:

- do not stop at an internal milestone because the remaining work is large
- do not substitute a "reasonable stopping point" for the stated target
- do not pause only to report partial progress unless the user requests a checkpoint
- do not split the remaining work into a separate project unless the user requests it
- do not treat elapsed time, a date boundary, a turn boundary, or partial quantitative improvement as completion

Only stop before the target is reached when:

- the user redirects or cancels the work
- a genuine blocker cannot be resolved through normal implementation, testing, local investigation, or safe alternatives
- required authorization, user input, or an external resource is unavailable and meaningful progress cannot continue
- the next step would be destructive, irreversible, or otherwise unsafe without confirmation

If blocked, report exactly what is blocked, what was attempted, what remains,
and what decision, authorization, resource, or external-state change is needed.

For quantitative targets, partial improvement is not completion. For example,
raising a coverage floor is progress, but it does not complete a request to
reach 100% coverage.

## Required Updates When Behavior Changes

If a language or tooling behavior changes, update these in the same pass when relevant:

- compiler tests
- language-server tests
- examples under `examples/`
- tutorials under `tutorials/`
- package or root README files
- `work/task-board.md`
- a dated note under `work/`

## Package Expectations

### `crates/aura-compiler`

Use layered tests:

- unit tests for lexer, parser, checker, runtime-value helpers, and MIR helpers
- fixture tests for parse/check/run/diagnostic behavior
- regression tests for every reported compiler bug
- example smoke tests for runnable language features

When adding a feature, prefer adding fixtures first.

### `crates/aura`

Treat CLI behavior as product behavior:

- validate command success paths
- validate annotated diagnostic output
- keep command examples in README files current

### `tools/aura-language-server`

The LSP must have regression tests for:

- diagnostics
- completions
- hover
- go-to-definition
- scope handling
- real example files that previously broke

Use `npm run coverage:lsp` regularly and move the package toward enforced 100% coverage before expanding the semantic surface further.

### `tools/vscode-aura`

Keep the extension thin and test packaging/build behavior whenever the LSP surface changes.

## Build Artifact Hygiene

Rust test, coverage, benchmark, and alternate-flag profiles can make `target/`
grow very quickly. Build outputs are disposable and must not be allowed to
consume the workstation indefinitely.

- Check `du -sh target` and available disk space before and after heavyweight
  coverage, parity, benchmark, or full-CI runs.
- If `target/` exceeds 20 GiB, available disk space falls below 25 GiB, or
  repeated profiles are no longer needed, clean obsolete build artifacts before
  continuing.
- Prefer the narrowest sufficient cleanup while artifacts are still reusable.
  Use `cargo llvm-cov clean --workspace` after coverage-only outputs are no
  longer needed, and use `cargo clean` when the accumulated Rust build tree is
  no longer worth preserving.
- Remove stale Aura-generated temporary linker/test artifacts after an
  interrupted or failed gate. Do not remove a live process's files while the
  process is still running.
- After a broad cleanup, rebuild only the minimal binary or profile needed for
  the next step. Do not immediately recreate every previous profile.
- Never treat source files, fixtures, examples, user-created files,
  `Cargo.lock`, `package-lock.json`, or dependency caches outside this
  repository as disposable build output.

## Hosted CI Completion

One complete green hosted CI run is sufficient evidence for a change. Do not
require repeated or consecutive reruns unless the user explicitly requests
them for a particular task.

## Documentation Style

Aura's documentation is written for a programmer who knows another language
and wants to get something done. Write so that reader can find the rule, see
an example, and move on.

### Shape

- Lead with the point. A page opens by saying what it covers. A section opens
  with the rule or the answer, then the example, then the details and edge
  cases.
- Show code early. Put a short example right after the rule it illustrates.
  Prefer one small complete example over several fragments.
- Use a table for reference data: signatures, options, diagnostic codes,
  platform lists. Use a bulleted list for parallel items. Use prose for an
  argument or an explanation.
- Keep headings short and plain. Title Case, as the existing pages do.
- Say each thing once. Link to the page that owns a topic instead of
  restating it.

### Sentences

- One idea per sentence. Aim for about 20 words. Split long sentences instead
  of joining clauses with semicolons.
- Use the active voice and the present tense: "The checker rejects...", not
  "It will be rejected by the checker".
- Use plain words. Prefer "use" to "utilize", "so" to "consequently", "about"
  to "approximately".
- No em-dashes. Avoid parentheses; if an aside matters, give it its own
  sentence.
- No filler or hedging: drop "simply", "just", "note that", "it is worth
  noting", "basically", "in order to".
- No marketing adjectives: "powerful", "seamless", "robust", "blazing".
  State what the feature does and let the reader judge.

### Content

- State what Aura does today. Do not narrate history ("now supports",
  "previously", "no longer") outside the CHANGELOG. Do not describe planned
  work as if it exists; say plainly that something is not implemented, once,
  where the reader would look for it.
- Define a term the first time a page uses it. Expand an acronym the first
  time it appears.
- Keep internal vocabulary out of user pages: batch, phase, checkpoint,
  question numbers, internal type names. Cite an ADR only as a closing
  "Design record" link when the reader may want the rationale.
- For an error, give the code, what causes it, and how to fix it.
- Bold only the first words of a list item or paragraph, never a whole
  sentence.
- Every ```` ```aura ```` block must compile: `npm run check:doc-examples`
  runs `aura check` on each one. Mark a partial snippet ```` ```aura fragment ````
  and an error example ```` ```aura check-fail:AU3004 ````, with the exact
  code it must fail with. For a long example, keep the program under
  `examples/` and embed it with `<<< @/examples/path.au`, so the page shows
  the file the CLI tests run.
- Every claim must be true of the current compiler. When a fact is precise,
  such as a diagnostic code, a signature, a default, or a limit, keep it
  exactly.

## Tutorials And Examples

The `tutorials/` directory should track the implemented subset of Aura, not just the proposal.

The `examples/` directory should stay categorized, runnable, and aligned with tutorial chapters.

If a feature is not implemented in the compiler, do not teach it as if it exists.

## Work Tracking

Keep `work/task-board.md` current.

For substantial work, maintain a status entry containing:

- the current target and authorized scope
- material work completed
- current verification state
- remaining work and any genuine blockers

Do not require or maintain start times, elapsed-time counters, active-session
timers, duration caps, or timer-based stop rules. Dates may be used for work-note
names and historical context, but they are not stop conditions.

When work is complete, mark the target complete and remove or update stale
in-progress status.

For substantial work, add a dated note under `work/` describing:

- goal
- work completed
- verification
- follow-up
