#!/usr/bin/env bash
set -euo pipefail

runs="${AURORA_STRESS_RUNS:-25}"
for ((iteration = 1; iteration <= runs; iteration += 1)); do
  echo "scheduler stress iteration $iteration/$runs"
  cargo test -q -p aura queue_consumers_share_work_without_starvation --test cli -- \
    --exact --test-threads=1
done
