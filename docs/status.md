# Sarif Stage-0 Status

Sarif has completed its initial stabilization and minimization pass. The project is currently in the **Bootstrap Window**.

## Completed

-   **Language Simplification:** Removed `interface` and `return` keywords. Functions use unified tail-expression returns.
-   **Strict Declaration Order:** Mandatory top-level order (Types -> Consts -> Functions) enforced in the parser.
-   **Consolidated Workspace:** Ten redundant crates merged into four core crates (`sarif_syntax`, `sarif_frontend`, `sarif_codegen`, `sarif_tools`).
-   **Compiler Model:** Full CST -> HIR -> MIR pipeline implemented.
-   **Execution Path:** 
    -   MIR Interpreter (Normative Oracle)
    -   Native Target (linked executables via Cranelift)
    -   Wasm Target (binary `.wasm` via `wat` and `wasmtime`)
-   **Float Substrate (Stage-0):** `F64` literals, arithmetic/comparisons, `sqrt(F64)`, `f64_from_i32(value)`, and `text_from_f64_fixed(value, digits)` are supported on interpreter + native. Wasm currently rejects `F64` programs explicitly with a deterministic stage-0 limitation error.
-   **CLI Workflow:** `sarifc` exposes one maintained workflow (`format`, `check`, `run`, `build`, `doc`) over the current stage-0 surface.
-   **Experimental Self-Host Tool Path:** `sarifc bootstrap-format`, `sarifc bootstrap-doc`, and `sarifc bootstrap-check` expose the current self-host transition surface without pretending it is the maintained authority. `bootstrap-doc` and `bootstrap-check` now bridge to maintained semantic doc/check rendering; `bootstrap-format` is still the Sarif-hosted formatter path.
-   **Package Layout:** Package directories and `Sarif.toml` manifests are accepted uniformly, with an optional ordered `package.sources` list for flat multi-file stage-0 packages.
-   **Technical Integrity:** The maintained stage-0 surface is limited to the interpreter, native backend, and binary Wasm backend, and is kept compile-clean with `cargo test` plus `cargo clippy --workspace --all-targets --all-features -- -D warnings`.

## In Progress (Bootstrap Milestone)

-   **Language Specification:** Maintained numbered stage-0 spec draft in progress.
-   **Self-Hosting substrate:** `bootstrap/sarif_syntax` is pinned as the maintained stage-0 parser substrate, with ordered top-level structure, function-body item shape, and ordered syntax events preserved for follow-on tooling ports.
-   **Conformance Corpus:** Expanding golden tests for format and semantic compliance, plus retained exact-output coverage for the maintained semantic `doc` and semantic `check` paths on shipped inputs and for the Sarif-hosted formatter, the bridged semantic `bootstrap-doc`, and the bridged semantic `bootstrap-check` surfaces.
-   **Benchmark Coverage:** `~/bnch` currently carries an experimental Sarif lane for `mandelbrot`, `fasta`, `nbody`, `revcomp`, and `spectralnorm`.
-   **Robustness Tooling:** Investigating `cargo-fuzz` integration for parser and lowering layers.

## Authority Boundary

-   **Maintained authority:** Rust-hosted `format`, semantic `check`, `run`, `build`, and semantic `doc`.
-   **Experimental self-host path:** `bootstrap-format`, `bootstrap-doc`, and `bootstrap-check`, all executed through the maintained compiler/runtime.
-   **Current formatter state:** `bootstrap-format` now matches the maintained formatter on the retained shipped parity corpus, including the shipped bootstrap packages. `bootstrap-doc` and `bootstrap-check` are now pinned against maintained semantic outputs on their CLI surfaces, but all three remain experimental tool paths rather than maintained release authority.
-   **Current runtime-input state:** `stdin_text()`, `stdout_write(text)`, the opaque `TextBuilder` builtins, and the opaque `F64Vec` builtins are available on interpreter and native maintained paths and are rejected explicitly on Wasm stage-0.
-   **Not authority yet:** the Sarif-hosted `doc` output is syntax-outline documentation, not the maintained semantic markdown generator with const-value rendering and package grouping.
-   **Not exposed as maintained CLI:** `check_text` inside `bootstrap/sarif_tools` remains a syntax-outline helper and is no longer the `bootstrap-check` authority path.
-   **Stage-0 unsupported surface:** `handle` parses in the live syntax tree but is still rejected by maintained stage-0 semantic/codegen paths.

## Next Steps

1.  Finalize the numbered language spec.
2.  Keep widening retained parity coverage for `bootstrap-format` beyond the current shipped examples/packages and keep it exact as the bootstrap surface evolves.
3.  Replace the remaining Rust-owned release authority with Sarif-hosted semantic tooling, not just bridged CLI surfaces.
4.  Port the non-backend `check` subset to Sarif and expose it honestly once it covers real semantic checking.
5.  Only then promote Sarif-hosted tools from experimental parity paths to maintained release authority.
