# ADR-0018: Fixed resource read limits

- Status: Accepted
- Date: 2026-07-15
- Roadmap decision: Phase 3 practical control-plane resource caps

## Context

Aurora previously used one 64 MiB limit for filesystem reads, process pipes,
network streams, and TLS configuration files, while its HTTP parser used a
separate 1 MiB message limit. Raising the shared value would silently raise
unrelated process, network, and TLS limits. The language instead needs explicit
limits whose scope is stable and visible to programs.

## Decision

- `fs.read_to_string`, `fs.read_bytes`, `fs.File.read_all`, and
  `fs.File.read_bytes` accept at most 268,435,456 remaining bytes (256 MiB) per
  call.
- Process capture and pipe reads plus TCP, Unix, and TLS whole, line, exact, and
  bounded reads retain their 67,108,864-byte (64 MiB) limit. TLS certificate,
  private-key, and CA-file loading also retains that limit and does not inherit
  the public filesystem ceiling.
- Incoming parsed HTTP/1.1 requests and responses accept at most 16,777,216
  wire bytes (16 MiB), including the start line, headers, transfer framing,
  trailers, and body. The existing maximum of 64 headers is unchanged.
- The HTTP limit is a receive/parser limit. It does not add a separate cap to
  outbound request or response writers.
- These are fixed Aurora 0.1 limits. No runtime or project configuration surface
  changes them.

An oversized filesystem or whole-stream read returns the existing typed
`InvalidData` outcome. An explicitly requested bounded stream count above the
stream limit returns `InvalidInput`. An oversized HTTP client response returns
`io.Error.InvalidData`; an HTTP listener replies with status 413 when protocol
handling permits, then continues accepting requests.

## Completion tests

- Small injectable-limit runtime tests pin file, stream, content-length,
  chunked, and close-delimited enforcement without allocating at production
  limits.
- Sparse-file tests pin the 256 MiB filesystem boundary and the independent
  64 MiB TLS-configuration boundary.
- Forced MIR/direct CLI tests accept an HTTP response above the retired 1 MiB
  ceiling, reject a declared response above 16 MiB, and reject filesystem reads
  above 256 MiB with the same typed outcomes.
