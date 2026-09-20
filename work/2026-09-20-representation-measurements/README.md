# Representation measurements

Raw report from `scripts/bench-representation.py` at commit `6f19ac88d681115a5eb70a20d583bb6efa135038` on the
first representation pull request (inline-member unions). The counters are
the runtime's own allocation counters on both backends and are
deterministic for these programs; wall times and binary sizes are
diagnostic. The measuring checkout carried this workstation's three
untracked or modified files that no phase touches (an editor swap file, a
draft decision record, and a personal script), which is why the runner ran
with `--allow-dirty`; every tracked source matched the commit. `SHA256SUMS`
covers the raw report. The report is archived in the commit that follows the measured one. The
second pull request re-measures the same programs.

`representation-pr2-raw.json` is the second pull request's report (owned-handle
union members) at commit `aba80f87d49b0d1992cf08ec3286b55e8836a862`, taken the same way; the archive lands in the
commit that follows the measured one.
