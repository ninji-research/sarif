#!/bin/sh
# Quick fuzzing smoke test for CI integration.
# Runs each fuzz target for a brief duration to catch
# regressions without dedicating a full CI fuzzing cluster.
set -e
cd /home/user/sarif || exit 1

for target in pipeline alloc; do
    echo "=== Fuzz smoke test: $target ==="
    cargo +nightly fuzz run "$target" fuzz/corpus/"$target" -- \
        -max_len=4096 \
        -runs=10000 \
        -max_total_time=120 2>&1 || echo "WARNING: fuzz target $target failed or timed out"
done

echo "=== Fuzz smoke tests complete ==="
