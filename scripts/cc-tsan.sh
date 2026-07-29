#!/usr/bin/env bash
set -euo pipefail

exec "${CC_REAL:-clang}" -fsanitize=thread "$@"
