# Sarif Directives

This document records the standing engineering directives for Sarif work so they remain explicit, reviewable, and durable.

## Core Standard

- deeply understand the codebase before changing it
- enforce one right way per concern across syntax, tooling, implementation, tests, and docs
- prefer fewer concepts, files, flags, helpers, and APIs without sacrificing capability or clarity
- keep semantics deterministic, explicit, and auditable
- keep Sarif low-verbosity, natural, and idiomatic
- treat correctness, maintainability, predictability, and long-term evolution as non-negotiable

## Implementation Standard

- fully implement maintained features or remove them from the maintained surface
- do not keep hacks, workaround-only paths, partial authority, or knowingly inferior implementations
- identify and eliminate bad historical decisions instead of building around them
- deduplicate logic so each responsibility has one canonical implementation path
- keep module boundaries explicit: parsing, typing, MIR, codegen, runtime, stdlib, tooling
- keep data flow, naming, and APIs small, direct, and easy to reason about

## Performance And Energy

- optimize for both speed and energy efficiency
- reduce allocations, copying, temporary structures, branching, and unnecessary synchronization
- focus on hot paths and production-shaped workloads
- treat regressions as bugs to be understood and fixed

## Benchmark Discipline

- benchmark all performance-relevant Sarif changes in `~/bnch`
- keep Sarif honest against the full retained suite rather than narrow wins
- add regression coverage for every fixed performance or correctness issue
- maintain competitiveness across speed, memory, build, and deploy size
- continue pushing source concision and implementation quality where Sarif still trails

## Tooling And Docs

- keep formatter, compiler, docs, examples, benchmarks, and retained corpora aligned with the same maintained language
- keep markdown concise and authoritative; link instead of repeat
- remove stale, redundant, and non-idiomatic examples
- ensure all tooling reflects the latest maintained semantics and workflow

## Roadmap Bias

- prioritize the highest-leverage work that safely closes real gaps
- aggressively advance self-hosting, standard library readiness, production readiness, and benchmark leadership
- complete work to a stable, verified state instead of leaving partial progress

## Definition Of Done

Work is only done when it is:

- semantically correct with tests for normal, edge, and error paths
- benchmarked where performance is relevant
- consistent with the maintained language and tooling surface
- documented with concise, current, non-duplicative docs
- free of known hacks, regressions, and unresolved inferior decisions
