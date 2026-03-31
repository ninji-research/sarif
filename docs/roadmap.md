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

## Concurrency And Scheduling

Sarif does not have a maintained async or multithreaded story yet.

The intended direction is:

- one concurrency model, not multiple competing ones
- analyzable task spawning and channels first
- bounded executor semantics
- `RT` restricted to deterministic, bounded scheduling rules

Async syntax is only acceptable if it lowers to that same maintained task model instead of creating a second runtime.

## Performance And Build Tooling

The maintained direction is:

- fast local iteration profiles
- small, highly optimized release builds
- explicit profiling workflow
- benchmark-driven runtime and backend work
- deterministic MIR behavior even as codegen quality improves

## Current Hard Boundaries

As of March 31, 2026, Sarif does not yet ship:

- a full standard library
- maintained async support
- maintained multithreading support
- maintained parallel runtime primitives
- self-hosted release authority for `format`, `check`, or `doc`
