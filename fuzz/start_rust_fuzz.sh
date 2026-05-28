#!/bin/sh
exec >> /home/user/sarif/fuzz/rust_fuzz.log 2>&1
cd /home/user/sarif || exit 1

# Pipeline fuzz target: full compiler pipeline with randomly generated inputs
# Uses larger max_len to cover complex parse trees
exec cargo +nightly fuzz run pipeline /home/user/sarif/fuzz/corpus/pipeline -- -max_len=16384 -runs=200000000 -max_total_time=86400 &
PID1=$!

# Alloc fuzz target: parser/allocator robustness under memory constraints
exec cargo +nightly fuzz run alloc /home/user/sarif/fuzz/corpus/alloc -- -max_len=8192 -runs=100000000 -max_total_time=86400 &
PID2=$!

wait $PID1 $PID2
