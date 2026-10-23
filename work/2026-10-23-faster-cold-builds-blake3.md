# Faster Cold Builds: BLAKE3 for the Runtime Archive

## Goal

PR #117 reuses the runtime archive's recorded digest while the archive's
stamp holds. A build into an empty cache, the first build after the runtime
changes, and every `aura build` of an installed runtime with a fresh cache
directory still hashed all 62 MB of it. Step 9c measured that hash at about
3.3 billion of the 3.5 billion instructions in a cold build of hello world.

## Design

- `runtime_archive_digest` names the archive's content digest. It is BLAKE3,
  where it was SHA-256. On this container BLAKE3 hashes the archive in about
  16 ms and SHA-256 in about 340 ms, because BLAKE3 uses the CPU's vector
  units and this Xeon has no SHA extensions.
- BLAKE3 is a cryptographic hash with a 256-bit digest, so the cache key
  keeps the same collision resistance. The memo still stores 64 hex digits.
- `NATIVE_CACHE_FORMAT` moves from `aura-native-cache-v8` to
  `aura-native-cache-v9`, so program entries keyed on SHA-256 digests are
  never reused. A memo written by an older `aura` still matches its stamp
  and keeps giving its old digest, which only means one rebuild of each
  program under the new format.
- The cached program's own digest stays SHA-256. Programs are about 1 MB,
  so that hash is cheap.

## Tests

- `runtime_archive_digest_is_blake3` pins the digest to the BLAKE3 test
  vector for `abc`, so a change of algorithm has to change the cache format
  on purpose.
- `native_cache_format_carries_the_runtime_archive_epoch` now pins `v9` and
  checks that `v8` is gone.

## Measurements

`scripts/bench-compile-time.py hello_world --repetitions 7`, release builds,
alternated twice on the Linux container. `head` is main with PR #117's
memo reuse. Medians in seconds:

| Run | Cold | Warm | Edit |
| --- | ---: | ---: | ---: |
| head, run 1 | 0.819 | 0.359 | 0.385 |
| BLAKE3, run 1 | 0.462 | 0.417 | 0.402 |
| head, run 2 | 0.687 | 0.396 | 0.355 |
| BLAKE3, run 2 | 0.444 | 0.348 | 0.349 |

Cold builds drop below the 0.6 s phase 2 target for hello world. Warm and
edit builds are unchanged, since they already reused the memo.

## Verification

- The two tests above pass, with the `aura` unit tests and the CLI suite.
  One CLI test, `loop_backedge_safepoints_prevent_socket_readiness_starvation`,
  failed once under the full parallel suite. It runs on `aura run`, which
  never computes the runtime digest, and it passed three runs out of three
  alone.
- `cargo fmt --check`, `npm run check:clippy`, and
  `scripts/check-hygiene.sh` pass.
