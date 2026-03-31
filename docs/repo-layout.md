# Sarif Repo Layout

The repo is intentionally shallow.

- `apps/sarifc`: maintained CLI
- `crates/sarif_syntax`: lexer, parser, syntax tree
- `crates/sarif_frontend`: HIR, semantic analysis, ownership
- `crates/sarif_codegen`: MIR, interpreter, native backend, wasm backend
- `crates/sarif_tools`: formatter, docs, reports, replay support
- `runtime/`: native runtime C support code linked into emitted native binaries
- `examples/`: small maintained language examples
- `docs/`: status, roadmap, architecture, performance, and repo references
- `spec/`: grammar-level specification artifacts

There is one maintained compiler pipeline. The crate split is there to keep boundaries rigid, not to create alternate toolchains.
