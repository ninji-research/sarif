<p align="center">
  <img src="sarif-logo.svg" alt="Sarif Logo" width="200" />
</p>

# Sarif

Sarif is a deliberately minimal, highly maintainable, single-style, memory-safe systems language and stage-0 self-hosting-oriented compiler/toolchain. Its purpose is to make secure, auditable, deterministic, resource-efficient, high-performance software the default by giving everyone one syntax, one semantic core, one formatter, one package layout, one stable workflow (`format`, `check`, `run`, `build`, `doc`), and one clear implementation model (CST → HIR → MIR).

In Sarif, Core, Total, and RT are not separate languages but progressively stricter profiles of the same language. The MIR interpreter is the normative semantic oracle, native build output is the primary deployment target, and Wasm build output is the portable target. The codebase exists to keep that promise mechanically enforceable and easy to evolve—small stable surface, low maintenance burden, explicit semantics, internal complexity hidden behind rigid boundaries.

Users should choose Sarif when they want maximum readability, auditability, predictability, safety, and performance without feature sprawl, and contributors should treat every change as valid only if it strengthens that single-language contract, simplifies the implementation, improves correctness or ergonomics, and avoids creating a second way to write, build, or reason about programs.

## Core Mandates

-   **One Syntax:** Exactly one way to express any given construct.
-   **Single Style:** A canonical formatter ensures a unified codebase.
-   **Memory Safety:** Guaranteed without garbage collection or explicit lifetime ceremony.
-   **Rigid Architecture:** CST -> HIR -> MIR compiler model with strictly defined crate boundaries.
-   **Deterministic Semantics:** The MIR interpreter is the normative semantic oracle for all backends.

## Current State

Sarif is currently in its stage-0 bootstrap window.

### Implemented
-   **Unified Syntax:** Implicit tail-expression returns, strict top-level declaration order.
-   **Consolidated Workspace:** Core crates (`sarif_syntax`, `sarif_frontend`, `sarif_codegen`, `sarif_tools`).
-   **Stable Execution Paths:**
    -   MIR Interpreter (Reference Oracle)
    -   Native Target (Linked executables via Cranelift)
    -   Wasm Target (Binary `.wasm` via `wat`, including stage-0 text builtins and bootstrap package execution)
-   **Tooling:** Stable `sarifc` commands for `format`, `check`, `run`, `build`, and `doc`.
-   **Bootstrap Self-Host Commands:** `sarifc bootstrap-format` runs the current Sarif-hosted formatter through the maintained compiler/runtime, and `sarifc bootstrap-doc` plus `sarifc bootstrap-check` bridge to the maintained semantic doc/check surfaces on the same CLI boundary.
-   **Technical Integrity:** Default workspace verification is kept green with a small supported backend surface.
-   **Benchmark Coverage:** `~/bnch` currently carries an experimental Sarif lane for `mandelbrot`, `fasta`, `nbody`, `revcomp`, and `spectralnorm`.
-   **Retained Bootstrap Corpora:** `bootstrap-format` is pinned by manifest-backed exact retained outputs over shipped examples and bootstrap packages. `bootstrap-doc` is pinned against retained semantic markdown outputs, and `bootstrap-check` is pinned against retained maintained semantic diagnostics.
-   **Retained Maintained Corpora:** The Rust-authoritative semantic `doc` and semantic `check` paths also have exact retained-output coverage on shipped inputs.

### Goals
-   **Self-Hosting:** Rewrite the formatter, documentation generator, and a subset of the checker in Sarif.
-   **Production Readiness:** Focus on robustness, diagnostics, and conformance corpora.
-   **Minimalism:** Defer complex features (generics, async, etc.) until the core is proven and stable.

### Not Ready Yet
-   **Release-authority self-hosting:** Rust-hosted `format`, semantic `check`, and semantic `doc` are still the maintained authorities.
-   **Semantic self-host checker:** the Sarif-hosted `check_text` helper is still syntax-outline only and is not the maintained semantic checker.
-   **Experimental self-host checker:** `bootstrap-check` now bridges to the maintained semantic checker on the CLI surface, but the maintained release authority is still Rust-owned rather than Sarif-hosted.
-   **Semantic self-host docs:** `bootstrap-doc` now reuses the maintained semantic markdown renderer; the remaining doc authority gap is that the maintained release path is still Rust-owned rather than Sarif-hosted.
-   **Formatter authority:** `bootstrap-format` now matches the maintained formatter on the retained shipped parity corpus, including the shipped bootstrap packages, but it is still an experimental tool path rather than the maintained authority.
-   **Rust archival:** the Rust implementation is still the release and backend authority, so it is not ready to archive.

## Verification Baseline

-   `cargo test`
-   `cargo clippy --workspace --all-targets --all-features -- -D warnings`

## Workflow

```bash
# Format source code
sarifc format main.sarif

# Verify semantic correctness
sarifc check main.sarif

# Run via the normative MIR interpreter
sarifc run main.sarif

# Build a native executable
sarifc build main.sarif -o my_app

# Build binary WebAssembly
sarifc build main.sarif --target wasm -o my_app.wasm

# Generate documentation
sarifc doc main.sarif

# Experimental Sarif-hosted formatter
sarifc bootstrap-format main.sarif

# Experimental bridged semantic docs
sarifc bootstrap-doc main.sarif

# Experimental bridged semantic checker
sarifc bootstrap-check main.sarif
```

Sarif package inputs are either a directory containing `Sarif.toml`, the manifest path itself, or a standalone `.sarif` file. Stage-0 packages stay deliberately simple: they are one flat namespace, and a manifest may optionally list ordered source files under `package.sources`. If `package.sources` is omitted, the default entry is `src/main.sarif`.

```toml
[package]
name = "demo"
version = "0.1.0"
sources = ["src/types.sarif", "src/consts.sarif", "src/main.sarif"]
```

## License

Sarif is licensed under the [Mozilla Public License 2.0 (MPL-2.0)](LICENSE.md).
