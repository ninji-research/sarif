# Sarif References

This document records the external references that inform the current v1 architecture. They justify the direction; they do not override the repository spec.

## Language And Trust Model

- Lean reference: small trusted kernel with automation around it
- Hylo: value semantics as the user model for systems programming
- Koka: effect typing
- Austral: explicit capabilities and resource discipline
- Carbon generics design: checked generics over template-style late checking
- SPARK Ravenscar guidance: analyzable restricted concurrency for hard real-time

## Compiler And Toolchain

- rustc query model
- logos
- ariadne
- Rust 1.95.0 release line
- Cranelift settings documentation
- cranelift-isle documentation

## Verification Strategy

- CompCert
- Crocus and related lowering-rule verification work

## Reactive Runtime And Data Plane

These references inform the runtime direction only. They do not automatically become language commitments:

- Salsa and related incremental-query systems
- Apache Arrow columnar memory model
- Polars engine design
- Arrow Flight transport model
