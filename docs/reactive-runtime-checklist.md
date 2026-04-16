# Sarif Reactive Runtime Checklist

This checklist turns the reactive-runtime direction into concrete implementation work. Items here are ordered by leverage and by how strongly they preserve Sarif's current philosophy.

## Phase 1: Core Data Surfaces

- define a maintained `Bytes` type for contiguous byte storage
- define zero-copy slice or view types for `Bytes` and other contiguous runtime values
- specify ownership, mutation, and lifetime rules for those view types
- ensure interpreter, native, and Wasm boundaries either support the same semantics or reject them explicitly
- add retained tests for zero-copy slicing, bounds behavior, and effect restrictions

## Phase 2: Parsing And Matching

- add maintained binary-pattern parsing for byte streams
- keep the surface compact and composable with existing `match`
- support wire-format and file-format parsing without benchmark-specific builtins
- verify diagnostics are precise when binary patterns are ill-typed, overlapping, or partial
- retain representative parsing examples in shipped examples or corpora

## Phase 3: Failure Isolation And Provenance

- improve runtime panic reporting so one failing computation path can be identified precisely
- record function or node provenance without introducing hidden mutable ambient state
- preserve deterministic interpreter and backend behavior for faulting programs
- add retained tests for divide-by-zero, bounds faults, and invalid runtime states

## Phase 4: Reactive Runtime Boundary

- define a runtime API for evaluating pure Sarif functions as dependency-graph nodes
- define cache keys, invalidation rules, and recomputation semantics outside the language core
- keep all scheduling and node execution policy in the runtime layer
- require explicit effect boundaries so runtime caching never depends on hidden global state
- add replay and inspection hooks suitable for retained debugging workflows

## Phase 5: Deterministic Scheduling

- design one explicit task model that can also serve future async work
- allow parallel execution only for independent pure nodes
- keep scheduler behavior analyzable and bounded under `RT`
- reject hidden automatic parallelism that changes semantics or debuggability
- validate scheduler wins on retained workloads, not demos

## Phase 6: Optional Platform Integrations

- define Arrow or similar columnar-memory interop as a runtime integration layer
- define transport integrations such as Flight-like streaming outside the language core
- keep dataframe engines and UI frameworks replaceable
- do not encode one product architecture into the base language or spec

## Promotion Gate

No item above should be promoted to maintained authority until it has:

- specification coverage
- parser and formatter coverage where syntax is involved
- semantic diagnostics coverage
- interpreter or backend coverage as appropriate
- retained examples or corpus coverage
- a clear performance case on real maintained workloads
