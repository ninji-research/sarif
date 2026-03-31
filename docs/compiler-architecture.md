# Sarif Compiler Architecture

The Sarif compiler follows a strict three-layer implementation model designed for simplicity, auditability, and self-hosting.

## 1. Syntax (CST / AST)

-   **Lexer:** Lossless lexing via `logos`, preserving trivia for the formatter.
-   **Parser:** Hand-written recursive descent parser producing a Concrete Syntax Tree (CST).
-   **AST:** A simplified Abstract Syntax Tree lowered from the CST, representing only valid language constructs.

## 2. Frontend (HIR / Semantic Analysis)

-   **HIR:** High-level Intermediate Representation lowered from the AST. This layer performs name resolution, type checking, and effect inference.
-   **Semantic Analysis:** Profile enforcement (Core, Total, RT) and affine ownership checking occur here.
-   **Diagnostics:** Rich, actionable error messages with canonical fixes, rendered via `ariadne`.

## 3. Codegen (MIR / Backends)

-   **MIR:** Mid-level Intermediate Representation. A single-exit point, expression-oriented form that serves as the basis for all execution.
-   **Interpreter:** The normative semantic oracle. Every maintained backend must match the interpreter's behavior.
-   **Native Backend:** Emits optimized object files via `cranelift` and links them into executables.
-   **Wasm Backend:** Emits binary WebAssembly (.wasm) via `wat` and executes it via `wasmtime`.
-   **Runtime Support:** The native runtime is a small C support layer for text, lists, scoped allocation, and entrypoint adaptation.

## Design Constraints

-   **Self-Hosting Target:** The compiler is being structured to allow self-hosting the formatter, documentation generator, and a subset of the checker.
-   **Deterministic Codegen:** Programs inside the maintained stage-0 subset must lower to exactly one executable behavior across the interpreter, native backend, and binary Wasm backend.
-   **Rigid Boundaries:** Crates (`sarif_syntax`, `sarif_frontend`, `sarif_codegen`, `sarif_tools`) protect stability and internal implementation details.
