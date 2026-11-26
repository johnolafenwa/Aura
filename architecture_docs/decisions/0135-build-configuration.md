# ADR-0135 Target Configuration

## Context

A program can be checked on Linux and compiled for a machine with no operating system. Choosing a declaration or reusing a binary requires the properties of the destination.

## Decision

Expose configuration as ordinary typed values in the builtin `build` module. Bind the target, profile and feature map for the entire entry package and its imports. CLI `--cfg` values override the manifest. Repeating a name on the command line takes its last value. A bare name has the string value `true`.

Include the target triple, OS, architecture, pointer width, byte order, freestanding flag, debug flag and sorted cfg entries in the native fingerprint. Prefix variable-length fields with their lengths. A scope guard restores the previous compiler configuration when a check finishes. The checked builtin-source cache follows that scope too.

## Consequences

Two packages can be checked with different configurations in the same process. A change to the entry manifest or the command line can select a different binary for unchanged Aura source.
