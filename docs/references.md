# Sarif References

This document records the external technical references that inform Sarif's v1 architecture. These references justify the design shape; they do not override the repository spec.

## Language And Trust Model

- Lean reference: small trusted kernel with automation around it
- Hylo: value semantics as the user model for systems programming
- Koka: effect typing with row-polymorphic effects
- Austral: explicit capabilities and resource discipline
- Carbon generics design: checked generics over template-style late checking
- SPARK Ravenscar guidance: analyzable restricted concurrency for hard real-time

## Compiler And Toolchain

- Rust compiler query model: demand-driven incremental compilation
- logos documentation: DFA-style generated lexer approach
- ariadne documentation: rich multi-span diagnostics
- Rust `1.94.0` release announcement
- Cranelift settings documentation for `single_pass` and `backtracking` regalloc tradeoffs
- `cranelift-isle` documentation for typed lowering rules

## Verification Strategy

- CompCert: proof where verified compilation materially improves trust
- Crocus and related ISLE-rule verification work: verify dangerous lowering logic instead of attempting whole-compiler proof first

## Source URLs

- Lean: <https://lean-lang.org/doc/reference/latest/Introduction/>
- Hylo: <https://hylo-lang.org/>
- Koka: <https://koka-lang.github.io/koka/doc/book.html>
- Austral: <https://austral-lang.org/tutorial/>
- Carbon generics: <https://docs.carbon-lang.dev/docs/design/generics/overview.html>
- SPARK Ravenscar: <https://docs.adacore.com/spark2014-docs/html/ug/en/source/concurrency.html>
- rustc query guide: <https://github.com/rust-lang/rustc-dev-guide/blob/master/src/query.md>
- logos: <https://docs.rs/logos/latest/logos/>
- ariadne: <https://docs.rs/ariadne/latest/ariadne/>
- Rust 1.94.0: <https://blog.rust-lang.org/2026/03/05/Rust-1.94.0/>
- Cranelift flags: <https://docs.rs/cranelift/latest/cranelift/prelude/settings/struct.Flags.html>
- cranelift-isle: <https://docs.rs/cranelift-isle>
- CompCert: <https://compcert.org/>
