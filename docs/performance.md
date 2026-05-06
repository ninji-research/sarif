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

`sarifc build` emits native binaries by lowering through Cranelift, then linking the Sarif runtime with a C toolchain.

Current native policy:

- generated native code keeps the maintained performance-oriented path
- shared runtime C code is compiled with a size-oriented `-Os` path
- `-O3` for generated-code-facing C compilation and link steps
- `-march=native`
- `-mtune=native`
- `-fomit-frame-pointer`
- `-fno-math-errno`
- `-fno-trapping-math`
- `-pipe`
- linker preference: `mold`, then `lld`, then system default
- cached runtime objects so unchanged native runtime code is not recompiled on every build

For same-ABI portability instead of host-specific tuning, set `SARIF_NATIVE_CPU=baseline`. That removes `-march=native` and `-mtune=native`, but it still does not turn the native path into maintained cross-compilation. See [platforms.md](platforms.md).

These are current implementation facts, not an abstract wish list.

## Benchmark And Runtime Position

Performance work is driven by retained outputs and `~/bnch`, not one-off microbenchmarks.

Current verified state:

- the Sarif lane in `~/bnch` covers the full retained main-track suite
- allocation-scope builtins now exist so short-lived native allocations can be reclaimed deterministically
- the latest local `~/bnch` report places Sarif `1/7` overall, `1/7` speed, `1/7` memory, `1/7` build, and `2/7` deploy size on this machine
- native owned text-producing helpers now use the scoped arena for common `Text` results, reducing leak pressure in repeated scoped allocation workflows while preserving the small native runtime model

## Reactive Runtime Performance Boundary

Sarif's zero-copy reactive direction should improve runtime behavior by moving less data and recomputing less work, not by hiding mutable global state behind notebook semantics.

The maintained direction is:

- zero-copy boundaries where data remains contiguous and explicit
- incremental recomputation in a runtime layer rather than implicit language mutation
- scheduler-driven parallelism only where purity and dependency information make behavior auditable
- benchmark validation through retained workloads rather than product demos

## What Is Not Finished

- no parallel codegen pipeline yet
- no async runtime yet
- no maintained multithreaded scheduler yet
- no maintained reactive DAG runtime yet
- no final self-hosted optimizer pipeline yet

Those remain roadmap items, not hidden partial features.
