# Sarif Standard Library Roadmap

Sarif does not yet ship a full standard library. The maintained surface today is a stage-0 builtin substrate plus formatting, checking, docs, and runtime support.

## Maintained Today

- scalar arithmetic and comparisons
- text construction and slicing
- direct parse helpers
- list allocation and indexed access
- deterministic runtime input/output builtins on native/interpreter paths
- deterministic allocation scopes for temporary native allocations

## Planned Standard Library Layers

1. `core`
   - scalar types
   - text views and builders
   - list and fixed-shape collection primitives
   - result/option-style control data

2. `alloc`
   - owned collections
   - maps and sets with stable semantics
   - arena and scoped allocation interfaces where justified

3. `io`
   - file and process interfaces
   - deterministic text and byte streams
   - explicit capability-gated resource handles

4. `rt`
   - restricted concurrency primitives
   - explicit task and scheduling model
   - bounded synchronization primitives

## Rules

- no duplicated APIs for the same job
- no hidden global runtime
- no async surface until the task/resource model is mechanically defined
- no parallel surface until determinism and memory rules are specified together

The next real standard-library blocker is content-aware text/map support for retained workloads, not a broad grab-bag of convenience APIs.
