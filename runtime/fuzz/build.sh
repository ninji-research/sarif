#!/bin/sh
# Build the C runtime fuzz target using clang's libFuzzer and AddressSanitizer.
# Usage: ./build.sh [fuzz_target] [corpus_dir]
set -e

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
FUZZ_TARGET="${1:-$SCRIPT_DIR/fuzz_target}"
CORPUS_DIR="${2:-$SCRIPT_DIR/corpus}"

mkdir -p "$CORPUS_DIR"

clang -fsanitize=fuzzer,address \
    -g -O1 \
    -o "$FUZZ_TARGET" \
    "$SCRIPT_DIR/fuzz_target.c"

echo "Built fuzz target: $FUZZ_TARGET"
echo "Run with: $FUZZ_TARGET $CORPUS_DIR"
