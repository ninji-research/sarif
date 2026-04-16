<p align="center">
  <img src="sarif-logo.svg" alt="Sarif Logo" width="200" />
</p>

# Sarif

Sarif is a minimal, single-style, memory-safe systems language and stage-0 self-hosting-oriented compiler/toolchain. The maintained implementation is still Rust-hosted, but the repository is organized around one stable workflow, one semantic core, and one compiler pipeline: syntax -> HIR -> MIR.

The maintained stage-0 surface now accepts compact expression-bodied functions (`fn add(a: I32, b: I32) -> I32 = a + b;`), record-field punning (`Pair { left, right }`), compound assignments (`+=`, `-=`, `*=`, `/=`), integer bitwise operators (`&`, `|`, `^`, `<<`, `>>`), richer `match` patterns through literal alternatives (`65 | 97`) and half-open integer ranges (`0..37792`), maintained line and field scanning through `text_line_end(...)`, `text_next_line(...)`, `text_field_end(...)`, and `text_next_field(...)`, direct list-growth through `list_push(...)`, integer text emission through `text_builder_append_i32(...)`, and a raw `Bytes` substrate through `stdin_bytes(...)`, `bytes_len(...)`, `bytes_byte(...)`, `bytes_slice(...)`, and `bytes_find_byte_range(...)` so byte-heavy code does not need to pretend it is UTF-8 text.

The maintained direction for notebook-like and reactive systems is runtime-first: Sarif may host a zero-copy reactive DAG environment, but dependency tracking, recomputation, transport, and UI integration belong in a runtime/platform layer rather than in the core language surface. See [docs/reactive-runtime.md](/home/user/sarif/docs/reactive-runtime.md), [docs/reactive-runtime-checklist.md](/home/user/sarif/docs/reactive-runtime-checklist.md), and [docs/execution-checklist.md](/home/user/sarif/docs/execution-checklist.md).

Platform support is documented explicitly in [docs/platforms.md](/home/user/sarif/docs/platforms.md). The short version is: Linux native is the maintained host path, macOS native is feasible but less exercised, wasm is maintained with explicit builtin exclusions, and native builds are host-native rather than cross-targeted.

## Current State

Sarif is still in Stage 0.

What is real today:

- `sarifc format`, `check`, `run`, `build`, and `doc` are the maintained CLI surface
- the MIR interpreter is the normative semantic oracle
- native build output is the primary deployment target
- Wasm build output exists with an explicitly smaller supported builtin surface
- `bootstrap-format`, `bootstrap-check`, and `bootstrap-doc` remain retained bootstrap bridge commands
- `~/bnch` carries a full main-track Sarif lane across all 10 retained benchmarks

What is not complete today:

- self-hosted release authority for `format`, `check`, or `doc`
- self-hosted HIR lowering, MIR generation, or backend ownership
- a full standard library
- a maintained async, multithreaded, or parallel runtime model

## Build Profiles

The workspace keeps multiple build profiles so the repo can support both fast iteration and aggressive release optimization.

- `dev`: fast local iteration
- `test`: fast local test iteration
- `release`: shipping build with `opt-level=3`, thin LTO, one codegen unit, and stripped symbols
- `release-fast`: cheaper release-like build for iteration on optimized binaries
- `release-max`: higher-cost release build with fat LTO
- `profiling`: release-like build that keeps debug info for profile collection

Cargo aliases are defined in `.cargo/config.toml`:

```bash
cargo xfmt
cargo xtest
cargo xlint
cargo xbuild
cargo xbuild-fast
cargo xbuild-max
```

## Verification Baseline

```bash
cargo test
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo build --release -p sarifc
```

## Workflow

```bash
sarifc format main.sarif
sarifc check main.sarif
sarifc run main.sarif
sarifc build main.sarif -o my_app
sarifc build main.sarif --target wasm -o my_app.wasm
sarifc check main.sarif --dump-ir=resolve
sarifc check main.sarif --dump-ir=typecheck
sarifc build main.sarif --dump-ir=lower -o my_app
sarifc build main.sarif --target wasm --dump-ir=codegen -o my_app.wasm
sarifc doc main.sarif
sarifc bootstrap-format bootstrap/sarif_syntax/Sarif.toml
sarifc bootstrap-doc bootstrap/sarif_syntax/Sarif.toml
sarifc bootstrap-check bootstrap/sarif_syntax/Sarif.toml
```

## Repository Layout

- `apps/`: executable entrypoints
- `crates/`: compiler and tooling layers
- `runtime/`: small native C runtime
- `bootstrap/`: retained bootstrap corpora
- `examples/`: shipped example programs
- `docs/`: maintained project-level docs

## License

Sarif is licensed under the [Mozilla Public License 2.0 (MPL-2.0)](LICENSE.md).
