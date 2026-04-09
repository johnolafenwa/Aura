#!/usr/bin/env bash
set -u -o pipefail

ROOT="$(cd "$(dirname "$0")" && pwd)"
AURA="/Users/johnolafenwa/source2/Aurora/target/release/aura"
OUT_DIR="$ROOT/bin"
LOG_DIR="$ROOT/logs"
mkdir -p "$OUT_DIR" "$LOG_DIR"

programs=(
  00_basic_addition.au
  01_mutation.au
  02_control_flow.au
  03_for_range.au
  04_functions_defaults.au
  05_recursion.au
  06_class_methods.au
  07_class_copy.au
  08_enum_match.au
  09_result_try.au
  10_string_ops.au
  11_numeric_casts.au
  12_uint128_boundary.au
  13_channel_spawn.au
  14_channel_iteration.au
  15_spawn_detached.au
  16_task_group_select.au
  17_task_group_cancel.au
  18_select_timeout.au
  19_with_resource.au
  20_module_import.au
  21_nested_module_import.au
  22_sleep.au
  23_runtime_error_div_zero.au
  24_runtime_error_cast_overflow.au
  25_runtime_error_neg_cast.au
  26_select_closed_timeout.au
  27_send_closed_result.au
)

capture() {
  local log_file="$1"
  shift
  local out rc
  set +e
  out="$("$@" 2>&1)"
  rc=$?
  set -e
  printf '%s\t%s\n' "$rc" "$out" > "$log_file"
}

run_file_mode() {
  local mode="$1"
  local file="$2"
  local label="$3"
  local log_file="$LOG_DIR/${label}.${mode}.log"
  case "$mode" in
    run)
      capture "$log_file" "$AURA" run "$file"
      ;;
    run-mir)
      capture "$log_file" "$AURA" run-mir "$file"
      ;;
    auto)
      local bin="$OUT_DIR/${label}_auto"
      local build_log="$LOG_DIR/${label}.auto.build.log"
      local build_out build_rc
      set +e
      build_out="$("$AURA" build --backend auto -o "$bin" "$file" 2>&1)"
      build_rc=$?
      set -e
      printf '%s\t%s\n' "$build_rc" "$build_out" > "$build_log"
      if [ "$build_rc" -eq 0 ]; then
        capture "$log_file" "$bin"
      else
        printf '%s\t%s\n' "$build_rc" "BUILD_FAIL" > "$log_file"
      fi
      ;;
    direct)
      local bin="$OUT_DIR/${label}_direct"
      local build_log="$LOG_DIR/${label}.direct.build.log"
      local build_out build_rc
      set +e
      build_out="$("$AURA" build --backend direct -o "$bin" "$file" 2>&1)"
      build_rc=$?
      set -e
      printf '%s\t%s\n' "$build_rc" "$build_out" > "$build_log"
      if [ "$build_rc" -eq 0 ]; then
        capture "$log_file" "$bin"
      else
        printf '%s\t%s\n' "$build_rc" "BUILD_FAIL" > "$log_file"
      fi
      ;;
    *)
      printf 'unknown mode %s\n' "$mode" >&2
      return 2
      ;;
  esac
}

for file in "${programs[@]}"; do
  label="${file%.au}"
  src="$ROOT/$file"
  run_file_mode run "$src" "$label"
  run_file_mode run-mir "$src" "$label"
  run_file_mode auto "$src" "$label"
  run_file_mode direct "$src" "$label"
done
