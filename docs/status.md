# Sarif Status

As of March 31, 2026, Sarif is still in the bootstrap window.

## Verified

- `cargo test` passes
- `cargo clippy --workspace --all-targets --all-features -- -D warnings` passes
- `cargo build --release -p sarifc` passes
- `~/bnch` manifest validation and harness unit tests pass
- `~/bnch` full 70-case main-track run passes with a complete Sarif lane

## Benchmark Snapshot

Latest local `~/bnch` run on this machine:

- overall rank: `5/7`
- speed rank: `5/7`
- memory rank: `1/7`

That is a real current measurement, not a roadmap claim.

## Important Current Truth

- Sarif now covers the full retained main-track benchmark suite in `~/bnch`
- the `binarytrees` lane no longer exhibits the prior pathological temporary-tree retention
- the maintained compiler is still Rust-hosted
- self-hosted tooling authority is not complete
- a full standard library is not complete
- async, parallel, and multithreaded runtime support are not complete
