# Sarif Execution Checklist

This checklist records the highest-leverage remaining work across language design, compiler implementation, runtime boundaries, benchmark coverage, and self-hosting.

## Hard Current Truth

- Sarif is currently first on the retained `~/bnch` main track for overall score, speed, memory, and build time on this machine, and second on deploy size (0.9241 overall).
- Sarif is still materially behind Nim and Go on retained source concision (947 canonical lines vs Nim's 560).
- The maintained compiler is still Rust-hosted and the maintained native runtime still depends on C.
- The reactive zero-copy DAG direction is now documented as a runtime-layer direction, not a language fork.
- The text builder integer path now formats directly into reserved space without an intermediate scratch buffer, recovering the small speed regression from the prior refactor.

## Phase 1: Language Surface Compression

- reduce repeated retained benchmark scaffolding through general-purpose primitives only
- prefer primitives that remove entire classes of source boilerplate across multiple workloads
- reject benchmark-specific builtins or one-off shortcuts that weaken the language model
- keep naming compact, regular, and consistent with existing maintained surfaces

Current best targets:

1. zero-copy `Bytes` and view types
2. binary-pattern parsing over contiguous byte storage
3. compact dataflow-friendly destructuring where it materially removes source weight

## Phase 2: Fresh Build And Backend Throughput

- remove redundant native ABI and metadata recomputation
- reuse backend setup and shared signatures where it reduces clean-build wall time
- keep Cranelift lowering deterministic while reducing repeated setup cost
- validate wins from fresh clean builds, not only warm caches

Current best targets:

1. further object-backend setup deduplication
2. Cranelift context or signature reuse where it does not complicate correctness
3. self-hosted build-path replacement only when it does not regress maintained coverage

## Phase 3: Runtime And Failure Provenance

- improve runtime fault provenance and stack traces
- isolate failures to the smallest possible execution path
- keep panic and invalid-state reporting precise across interpreter and native paths
- preserve explicit effect boundaries and deterministic semantics

## Phase 4: Reactive Runtime Foundation

- add only the language and runtime primitives needed to host zero-copy reactive execution
- keep dependency tracking, invalidation, and scheduling outside the language core
- require determinism and explicitness at the runtime boundary
- do not encode one notebook, UI, or dataframe stack into the spec

## Phase 5: Benchmark Coverage Discipline

- retain the current 10-benchmark main track unless a new category closes a real coverage hole
- add new retained categories only if they are not already represented by the current suite
- keep implementations canonical, modern, and comparable across languages
- treat source concision as a first-class tracked metric, but not as a reason to compromise correctness or fairness

Current likely future additions only if supported by real language/runtime features:

1. binary parsing or packet parsing
2. byte-oriented zero-copy transformation
3. explicit incremental or dependency-graph workloads

## Phase 6: Self-Hosting

- move maintained authority from Rust-hosted tools to Sarif-hosted tools in the declared order
- keep release authority gated by retained outputs and verification, not by aspiration
- remove Rust or C dependencies only when Sarif replacements are complete and measured

Current maintained order:

1. formatter
2. semantic `check`
3. semantic `doc`
4. HIR lowering
5. MIR generation
6. backend ownership
7. runtime replacement

## Promotion Gate

No item here should be considered complete unless it has:

- spec and docs coverage
- tests or retained corpus coverage
- benchmark or profile evidence where performance is the claim
- naming and semantics aligned with Sarif's existing philosophy
- no hidden fallback path that contradicts maintained authority
