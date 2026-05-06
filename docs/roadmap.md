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

### Stage-1 Memory Model Requirements

Before self-hosting can be achieved, the memory model must be fully sound:

**Text Arena Integration (Technical Debt)**

Most owned native `Text` results now allocate through the scoped arena system instead of unmanaged one-off text allocations. This includes text builder finish, concatenation, slicing, fixed-precision float formatting, and runtime argument text. This removes the most direct Stage-0 leak path for scoped text-heavy workloads.

Required remaining work:
- finish auditing runtime text ownership, including long-lived cached runtime input
- decide whether long-lived text needs explicit ownership, interning, or a separate process-lifetime arena
- keep `alloc_pop()` reclaiming scoped owned text without invalidating intentionally process-lifetime text
- add measurement coverage for long-running scoped text workflows

**Escape Analysis for [alloc] (Stage-1 Hard Error)**

Stage-0 emits a `semantic.alloc-escape` warning when `[alloc]` functions return types that could reference arena-allocated memory and the function body actually allocates, including transitive calls to other `[alloc]` functions. Non-allocating compatibility declarations no longer produce escape warnings. This is still a Stage-0 approximation rather than full proof.

Required implementation: Add MIR-level escape analysis in Stage-1 that:
- Detects when a pointer to arena-allocated data would escape the scope where it was created
- Emits a hard error (not just a warning) when allocations would escape their scope
- Distinguishes between allocations created inside an `[alloc]` function (which cannot be safely returned) and parameters passed into the function (which can be returned)

This eliminates the "trust the programmer" model and brings Sarif's memory safety guarantees in line with its performance goals.

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
