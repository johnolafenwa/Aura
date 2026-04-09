#!/bin/bash
# Run a single .au file through all 3 backends and compare output
# Usage: ./run_test.sh <file.au>
set -e

AURA="/Users/johnolafenwa/source2/Aurora/target/release/aura"
BINDIR="/Users/johnolafenwa/source2/Aurora/eval_aurora/new_features_backends"
FILE="$1"
BASENAME=$(basename "$FILE" .au)

echo "=== Testing: $BASENAME ==="
echo ""

# Backend 1: run (tree-walk interpreter)
echo "--- run (interpreter) ---"
RUN_OUT=$($AURA run "$FILE" 2>&1) || true
echo "$RUN_OUT"
echo ""

# Backend 2: run-mir (MIR runtime)
echo "--- run-mir (MIR) ---"
MIR_OUT=$($AURA run-mir "$FILE" 2>&1) || true
echo "$MIR_OUT"
echo ""

# Backend 3: build + execute (native codegen)
echo "--- build (native) ---"
BUILD_OUT=""
if $AURA build -o "$BINDIR/${BASENAME}_bin" "$FILE" 2>&1; then
    BUILD_OUT=$("$BINDIR/${BASENAME}_bin" 2>&1) || true
else
    BUILD_OUT=$($AURA build -o "$BINDIR/${BASENAME}_bin" "$FILE" 2>&1) || true
fi
echo "$BUILD_OUT"
echo ""

# Compare
if [ "$RUN_OUT" = "$MIR_OUT" ] && [ "$MIR_OUT" = "$BUILD_OUT" ]; then
    echo "RESULT: ALL MATCH"
else
    echo "RESULT: *** DISCREPANCY ***"
    if [ "$RUN_OUT" != "$MIR_OUT" ]; then
        echo "  run != run-mir"
    fi
    if [ "$MIR_OUT" != "$BUILD_OUT" ]; then
        echo "  run-mir != build"
    fi
    if [ "$RUN_OUT" != "$BUILD_OUT" ]; then
        echo "  run != build"
    fi
fi
echo "==============================="
echo ""
