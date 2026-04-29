<p align="center">
  <img src="sarif-logo.svg" alt="Sarif Logo" width="200" />
</p>

# Sarif

Sarif is a minimal, single-style, memory-safe systems language and compiler/toolchain oriented toward readable code, auditable semantics, predictable performance, and eventual self-hosting.

The repository is organized around one compiler pipeline and one maintained semantic authority:

- one syntax and one canonical formatting style
- one semantic oracle: the MIR interpreter
- one maintained stage-0 CLI: `sarifc`
- one benchmark discipline: clean results in `~/bnch`

## Current State

Sarif is still in Stage 0. The maintained implementation is Rust-hosted.

What is real today:
- `sarifc format`, `check`, `run`, `build`, and `doc` are the maintained CLI surface
- the stage-0 language includes expression-bodied functions, record-field punning, compound assignments, fixed arrays, bitwise operators, richer `match` patterns, `Bytes`, and maintained text/list helpers
- the MIR interpreter is the normative semantic oracle for backend correctness
- native Linux builds are the primary deployment path
- Wasm output exists with explicit builtin exclusions
- `~/bnch` carries a full main-track Sarif lane across the retained benchmark suite

What is not complete today:
- self-hosted release authority for `format`, `check`, or `doc`
- self-hosted HIR lowering, MIR generation, or backend ownership
- a full standard library
- a maintained async, multithreaded, or parallel runtime model

For exact status, recent benchmark results, and hard boundaries, see:
- [docs/status.md](docs/status.md)
- [docs/roadmap.md](docs/roadmap.md)
- [docs/platforms.md](docs/platforms.md)
- [docs/language-spec.md](docs/language-spec.md)

## Quick Start

Build the maintained CLI:

```bash
cargo build --release -p sarifc
```

Typical workflow:

```bash
sarifc format main.sarif
sarifc check main.sarif
sarifc run main.sarif
sarifc build main.sarif -o my_app
sarifc build main.sarif --target wasm -o my_app.wasm
sarifc doc main.sarif
```

Useful debug workflow:

```bash
sarifc check main.sarif --dump-ir=resolve
sarifc check main.sarif --dump-ir=typecheck
sarifc build main.sarif --dump-ir=lower -o my_app
sarifc build main.sarif --target wasm --dump-ir=codegen -o my_app.wasm
```

Retained bootstrap bridge commands:

```bash
sarifc bootstrap-format bootstrap/sarif_syntax/Sarif.toml
sarifc bootstrap-check bootstrap/sarif_syntax/Sarif.toml
sarifc bootstrap-doc bootstrap/sarif_syntax/Sarif.toml
```

## Verification

Treat a change as incomplete until the maintained baseline passes:

```bash
cargo test
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo build --release -p sarifc
```

For cargo aliases, build profiles, verification discipline, and retained-corpus policy, see [docs/engineering-practices.md](docs/engineering-practices.md) and [docs/performance.md](docs/performance.md).

## Repository Layout

- `apps/sarifc/`: maintained CLI surface
- `crates/sarif_syntax/`: lexing, parsing, formatting, retained syntax corpora
- `crates/sarif_frontend/`: HIR lowering, semantic analysis, ownership rules
- `crates/sarif_codegen/`: MIR, interpreter, native backend, Wasm backend
- `crates/sarif_tools/`: formatter/docs/report tooling support
- `runtime/`: small native C runtime linked into native artifacts
- `bootstrap/`: retained bootstrap corpora
- `examples/`: shipped examples
- `docs/`: status, roadmap, architecture, performance, platforms, and references
- `spec/`: grammar-level specification artifacts

The crate split exists to keep one-directional boundaries rigid, not to create alternate compiler pipelines.

## Design Docs

Sarif’s maintained direction is narrow and explicit:

- keep one right way instead of multiple competing idioms
- prefer smaller surfaces and stronger semantics over broad feature sprawl
- push reactive, notebook-like, and platform-specific concerns into runtime layers rather than core syntax
- keep benchmark, documentation, tooling, and implementation authority aligned

Relevant design docs:
- [docs/compiler-architecture.md](docs/compiler-architecture.md)
- [docs/directives.md](docs/directives.md)
- [docs/performance.md](docs/performance.md)
- [docs/reactive-runtime.md](docs/reactive-runtime.md)
- [docs/stdlib-roadmap.md](docs/stdlib-roadmap.md)

## Legal

Source code, including but not limited to implementation files, scripts, and configurations, is licensed under the [MPL-2.0](LICENSE.md) license. Documentation and informational content, such as but not limited to specifications, guides, and reports, are licensed under the [CC-BY-4.0](LICENSE-CONTENT.md) license.

Brand identity, including but not limited to the NINJI name, logos, graphics, and visual assets, is strictly proprietary. All rights are reserved. Usage, modification, or distribution of these assets is prohibited without prior written consent.

See [NOTICE.md](NOTICE.md) for full attribution details.
