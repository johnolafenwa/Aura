# ADR-0139 Tooling For Conditional Programs

## Context

Formatting, editor navigation and semantic checking use different views of the same source file.

## Decision

Format the complete token stream. Validate the input and formatted output using the syntax of every arm. Produce documentation and verification obligations from declarations selected for the current configuration. Use `aura check --all-targets` to run independent checks under every supported target, carrying the selected features and profile into each check.

Expose inactive source intervals in editor analysis. Mark these ranges with the Language Server Protocol's unnecessary diagnostic tag. Keep syntax-based information for hover and go-to-definition when an inactive declaration can be identified.

## Consequences

The formatter preserves inactive code and its comments. Editor clients can dim source without changing it. Cross-target checking needs the compiler's target descriptions and source dependencies. It can run without installing a linker for each target.
