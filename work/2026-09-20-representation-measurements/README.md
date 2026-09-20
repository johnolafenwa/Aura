# Representation measurements

Raw report from `scripts/bench-representation.py` at commit `47826c6218cc0a428c1d871bbe7f6a90bdee4099` on the
first representation pull request (inline-member unions). The counters are
the runtime's own allocation counters on both backends and are
deterministic for these programs; wall times and binary sizes are
diagnostic. The measuring checkout carried this workstation's three
untracked or modified files that no phase touches (an editor swap file, a
draft decision record, and a personal script), which is why the runner ran
with `--allow-dirty`; every tracked source matched the commit. `SHA256SUMS`
covers the raw report. The second pull request re-measures the same
programs.
