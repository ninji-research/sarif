# Sarif Reactive Runtime Direction

This document records the maintained direction for a zero-copy reactive execution environment around Sarif. It is a runtime and platform direction, not a second language.

## Design Rule

Sarif stays a compact, explicit systems language.

The reactive DAG environment must be built as a runtime layer that hosts Sarif code, not as syntax that hardcodes one notebook or UI stack into the language core.

## What Fits Sarif Core

These are good language-level additions because they improve general expressiveness, preserve explicitness, and remain useful outside any specific reactive product:

- zero-copy views over contiguous memory
- compact binary and byte-oriented pattern matching
- explicit effect boundaries around I/O, mutation, and scheduling
- cheap sum/product types and compact destructuring
- precise panic provenance and stack traces
- analyzable concurrency primitives only if they remain explicit and deterministic

## What Belongs In The Runtime Layer

These are platform capabilities and should not become base-language doctrine:

- DAG dependency tracking
- incremental recomputation and invalidation
- cache ownership and replay
- node-local fault isolation
- scheduler-controlled parallel execution of independent pure nodes
- Arrow or other columnar-memory interop
- transport layers such as Arrow Flight or similar zero-copy data movement
- notebook, dashboard, or web UI integrations

## Accepted Direction

The maintained direction is:

1. keep Sarif pure-function friendly and explicit at effect boundaries
2. add a small set of language/runtime primitives that make zero-copy dataflow possible
3. build the reactive engine as a runtime subsystem around compiled Sarif code
4. treat UI, transport, and data-engine integrations as replaceable platform layers

## Rejected Or Deferred

These are currently rejected or explicitly deferred because they would blur Sarif's boundaries or create stack-specific language debt:

- a language mode that turns Sarif into a notebook-specific dialect
- hidden global state or implicit ambient sessions
- automatic parallelism with scheduler behavior hidden from the program model
- pointer-heavy persistent collections as default runtime containers
- hardcoding Polars, Salsa, Arrow Flight, Perspective, or Svelte into the language surface
- broad imperative fallback semantics that weaken analyzability

## Concrete Next Actions

The best next actions are:

1. add `Bytes` and zero-copy view types for contiguous memory
2. add maintained binary-pattern parsing for byte streams and structured wire formats
3. improve runtime panic provenance so failures can be isolated to one evaluation node or call path
4. define a maintained runtime interface for incremental execution around pure Sarif functions
5. design a deterministic scheduler boundary for independent pure-node execution

The tracked implementation checklist for those steps lives in [reactive-runtime-checklist.md](/home/user/sarif/docs/reactive-runtime-checklist.md).

## Proposed Runtime Shape

The intended layering is:

1. Sarif core language and compiler
2. small maintained runtime for text, lists, scoped allocation, and future byte/view primitives
3. optional reactive runtime that owns dependency graphs, caches, and scheduling
4. optional product integrations such as data engines, transports, and frontends

That keeps the language reusable even if one reactive product direction changes or disappears.

## Gate For Future Features

No reactive-runtime feature should be promoted unless it satisfies all of these:

1. it is useful outside one product stack
2. it reduces conceptual weight instead of increasing it
3. it preserves explicitness, auditability, and debuggability
4. it has a clear performance case on real retained workloads
5. it can be specified without hidden ambient state
