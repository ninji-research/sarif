# Sarif Performance and Build Modes

Sarif keeps one maintained codebase with explicit build modes instead of ad hoc local flag sets.

## Cargo Profiles

- `dev`: fast local edit/compile loops
- `test`: fast local test loops
- `release`: default production profile for the Rust workspace itself
- `release-fast`: faster local iteration when rebuilding `sarifc`
- `release-max`: higher-cost Rust-side optimization profile when peak tool performance matters more than build time
- `profiling`: release-like build that keeps debug info for profiling

Useful commands:

```bash
cargo xtest
cargo xlint
cargo xbuild
cargo xbuild-fast
cargo xbuild-max
```

## Native Artifact Policy

`sarifc build` emits native binaries by lowering through Cranelift, then compiling and linking the Sarif runtime with a C toolchain.

Current native policy:

- `-O3`
- `-march=native`
- `-mtune=native`
- `-fomit-frame-pointer`
- `-fno-math-errno`
- `-fno-trapping-math`
- `-pipe`
- linker preference: `mold`, then `lld`, then system default

These are current implementation facts, not an abstract wish list.

## Benchmark And Runtime Position

Performance work is driven by retained outputs and `~/bnch`, not one-off microbenchmarks.

Current verified state:

- the Sarif lane in `~/bnch` covers the full retained main-track suite
- allocation-scope builtins now exist so short-lived native allocations can be reclaimed deterministically
- the latest local `~/bnch` report places Sarif `1/7` in memory and `5/7` overall on this machine

## What Is Not Finished

- no parallel codegen pipeline yet
- no async runtime yet
- no maintained multithreaded scheduler yet
- no final self-hosted optimizer pipeline yet

Those remain roadmap items, not hidden partial features.
