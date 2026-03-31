# Crates

`crates/` contains the maintained compiler and tooling layers.

- `sarif_syntax`: lexing, parsing, formatting, and syntax-facing retained corpora support
- `sarif_frontend`: HIR lowering, semantic analysis, profile enforcement, and ownership rules
- `sarif_codegen`: MIR, interpreter, native backend, object emission, and Wasm backend
- `sarif_tools`: retained tooling support such as replay and bootstrap helpers

Cross-crate dependencies should stay one-directional: syntax -> frontend -> codegen, with tools layered on top.
