#!/bin/sh
exec >> /home/user/sarif/fuzz/rust_fuzz.log 2>&1
cd /home/user/sarif || exit 1
exec cargo +nightly fuzz run pipeline /home/user/sarif/fuzz/corpus/pipeline -- -max_len=8192 -runs=100000000 -max_total_time=86400
