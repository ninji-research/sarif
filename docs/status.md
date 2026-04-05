# Sarif Status

As of April 5, 2026, Sarif is still in the bootstrap window.

## Verified

- `cargo test` passes
- `cargo clippy --workspace --all-targets --all-features -- -D warnings` passes
- `cargo build --release -p sarifc` passes
- `~/bnch` manifest validation and harness unit tests pass
- `~/bnch` full 70-case main-track run passes with a complete Sarif lane

## Benchmark Snapshot

Latest local `~/bnch` run on this machine:

- overall rank: `1/7`
- speed rank: `1/7`
- memory rank: `1/7`
- build rank: `4/7`
- deploy-size rank: `2/7`
- overall score: `1.0000`

That is a real current measurement, not a roadmap claim.

## Source Concision Snapshot

Latest local `~/bnch` source totals for the retained 10-benchmark set:

- Nim: `560` lines / `15821` chars
- Go: `846` lines / `17701` chars
- Sarif: `1174` lines / `39973` chars

Sarif is still materially behind the best concise baselines on source size. The recent syntax/runtime work and maintained sort builtins cut retained benchmark source substantially, but the language is not yet at its target concision frontier.

## Important Current Truth

- Sarif now covers the full retained main-track benchmark suite in `~/bnch`
- Sarif is currently first overall, first on speed, and first on memory in the latest local `~/bnch` run
- Sarif still materially trails Nim and Go on retained benchmark source concision
- the maintained `TextIndex` primitive is now promoted as the dense text-keyed aggregation/indexing path used by the strongest retained Sarif benchmark lanes
- the `binarytrees` lane no longer exhibits the prior pathological temporary-tree retention
- the maintained compiler is still Rust-hosted
- self-hosted tooling authority is not complete
- a full standard library is not complete
- async, parallel, and multithreaded runtime support are not complete
