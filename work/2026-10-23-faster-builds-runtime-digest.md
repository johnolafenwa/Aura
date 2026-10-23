# Faster Builds: Reuse the Runtime Archive Digest

## Goal

Step 8g found that every `aura build` hashed the whole runtime archive.
The archive is 62 MB, and SHA-256 over it took 3.34 billion instructions,
about half of a repeat build of hello world on this host. This change
stops the repeat work without weakening the native cache key.

## Design

- A native build stages a private copy of the runtime archive and returns
  the archive's digest, which the native cache stores the binary under.
- The runtime identity memo already records a digest beside the
  archive's file stamp: path, device, inode, length, and modification and
  change times. A warm `aura run` trusts that memo whenever the stamp
  still matches.
- The build now trusts it the same way. When the archive's stamp is the
  same before and after the build reads it, and the authoritative memo
  records that stamp, the build takes the memo's digest. Otherwise it
  hashes the bytes it read, as before, and refreshes the memo.
- `authoritative_runtime_identity_memo` names the memo a warm lookup
  reads: the cache root for an installed runtime, and the target
  directory for a source checkout. Building and writing memos share it.
- A cold `aura run` used to hash twice: once to establish the memo and
  again inside the build. It now hashes once.
- The first build after the runtime changes, and the first build into an
  empty cache with an installed runtime, still hash the archive once.

## Tests

- `native_build_reuses_the_runtime_identity_memo_while_the_archive_stamp_holds`
  in `crates/aura/tests/cli.rs` builds through an installed layout. It
  replaces the memo's digest with zeros and checks that the next build
  keeps them, so the build did not rehash. It then rewrites the archive,
  which changes its stamp, and checks that the next build records the
  real digest again. The test failed before the change, at the reuse
  check.

## Measurements

`scripts/bench-compile-time.py hello_world --repetitions 7`, release
builds of `2914ecfd` (base) and this change (head), run twice each on the
Linux container. Medians in seconds:

| Run | Cold | Warm | Edit |
| --- | ---: | ---: | ---: |
| base, run 1 | 0.819 | 0.642 | 0.681 |
| head, run 1 | 0.678 | 0.354 | 0.351 |
| base, run 2 | 0.746 | 0.681 | 0.675 |
| head, run 2 | 0.719 | 0.368 | 0.348 |

Warm and edit builds take about half as long. Cold builds still hash once
and stay within this host's noise.

## Verification

- The new CLI test passes.
- `cargo fmt --check`, `npm run check:clippy`,
  and `scripts/check-hygiene.sh` pass.
- The full `aura` CLI suite passes.
- `crates/aura` is outside the compiler coverage report, so the coverage
  floors do not move.
