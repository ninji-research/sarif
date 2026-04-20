# Sarif Roadmap

## Release Rule

No feature lands as maintained authority until it is covered by the specification, formatter, diagnostics, documentation surface, and retained corpus.

## Stage 0

Current maintained authority:

- Rust-hosted compiler and CLI
- MIR interpreter as oracle
- native backend
- stage-0 Wasm backend with explicit builtin exclusions

## Stage 1

Promote Sarif-hosted tooling to maintained authority:

- formatter
- semantic `check`
- semantic `doc`

Rust remains required until those authority paths are actually replaced without reducing correctness or coverage.

## Stage 2

Move compiler pipeline ownership into Sarif:

- HIR lowering
- MIR generation
- backend ownership

## Standard Library

Sarif does not have a full maintained standard library yet. The next real standard-library boundary should be small and explicit:

- deterministic text
- deterministic lists and maps
- stable filesystem/process boundaries
- versioned library surface instead of ad hoc builtin growth

## Reactive Runtime Direction

Sarif's maintained direction for reactive and notebook-like systems is runtime-first, not syntax-first.

The intended rule is:

- keep the language core general-purpose
- keep pure-function semantics and explicit effects as the foundation
- add zero-copy runtime-facing data surfaces where they remain broadly useful
- build DAG invalidation, recomputation, and scheduling as a maintained runtime layer
- avoid hardcoding one dataframe, transport, or UI stack into the language

This allows Sarif to host a zero-copy reactive environment without turning the language into a product-specific DSL.

## Concurrency And Scheduling

Sarif does not have a maintained async or multithreaded story yet.

The intended direction is:

- one concurrency model, not multiple competing ones
- analyzable task spawning and channels first
- bounded executor semantics
- `RT` restricted to deterministic, bounded scheduling rules
- any future reactive scheduler must reuse that same explicit task model instead of introducing hidden parallelism

Async syntax is only acceptable if it lowers to that same maintained task model instead of creating a second runtime.

## Performance And Build Tooling

The maintained direction is:

- fast local iteration profiles
- small, highly optimized release builds
- explicit profiling workflow
- benchmark-driven runtime and backend work
- deterministic MIR behavior even as codegen quality improves

## Current Hard Boundaries

Sarif does not yet ship:

- a full standard library
- maintained async support
- maintained multithreading support
- maintained parallel runtime primitives
- a maintained reactive DAG runtime
- self-hosted release authority for `format`, `check`, or `doc`

Platform reality is tracked separately in [platforms.md](platforms.md): Linux native is the maintained host target, macOS native is feasible but less exercised, wasm is maintained with explicit exclusions, and Windows/mobile/cross-compilation remain future work rather than implied support.
