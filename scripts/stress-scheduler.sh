#!/usr/bin/env bash
set -euo pipefail

runs="${AURORA_STRESS_RUNS:-25}"
tests=(
  "queue_consumers_share_work_without_starvation"
  "cancelled_sleeping_children_resume_and_can_observe_cancellation"
  "scheduler_mixed_wakeups_complete_in_mir_and_direct_backends"
)

for ((iteration = 1; iteration <= runs; iteration += 1)); do
  echo "scheduler stress iteration $iteration/$runs"
  for test_name in "${tests[@]}"; do
    cargo test -q -p aura "$test_name" --test cli -- --exact --test-threads=1
  done
done
