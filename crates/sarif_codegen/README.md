# `sarif_codegen`

Owns MIR lowering, the normative interpreter, and maintained backend emission.

Rules:

- interpreter semantics are the reference
- native and wasm backends must follow MIR behavior
- performance primitives must stay explicit and reviewable
