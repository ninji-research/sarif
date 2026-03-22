# Sarif Roadmap

## 1. Core Release Rule

No feature lands until it is fully supported by the specification, formatter, diagnostics, documentation generator, and conformance corpus. Features that introduce multiple ways to express the same logic are strictly rejected.

## 2. Bootstrap Window (Current)

The focus is on a stable, minimal substrate for self-hosting.

### Stage 0: Rust-Hosted Substrate
- CST -> HIR -> MIR pipeline.
- MIR Interpreter as the normative oracle.
- Native and Binary Wasm as stable targets.
- Strict declaration order and tail-expression returns.
- Flat package manifests with ordered `package.sources`.
- Query-style incremental caching in the frontend.

### Stage 1: Self-Hosted Tooling
- Build a minimal self-hosting substrate in Sarif (bootstrap syntax events plus deterministic collections and text handling).
- Port the `format` tool to Sarif.
- Port the `doc` generator to Sarif.
- Port the non-backend `check` subset to Sarif.

## 3. Post-Bootstrap Milestones

### Milestone A: Ownership and Safety
- Finalize affine ownership and borrow inference.
- Capability-based resource discipline.
- Destruction scheduling.

### Milestone B: Contracts and Refinement
- Full contract checking for bounded structural facts.
- Panic-freedom tracking in `RT` and `Total` profiles.

### Milestone C: Abstraction (Post-Self-Hosting)
- One complete design for checked generics.
- Stable package boundary and import/export model.
- Analyzable restricted concurrency for `RT`.

## 4. Non-Goals

Sarif does not promise:
- Arbitrary automatic proof of all logic.
- Unrestricted hard real-time.
- Multiple programming paradigms.
- Hidden runtime complexity.

Sarif does promise:
- One readable language.
- One obvious style.
- One small semantic core.
- One stable implementing toolchain.
