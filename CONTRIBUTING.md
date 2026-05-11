# Contributing to Sarif

Thank you for your interest in contributing. We are committed to maintaining a clean, well-documented, and high-performance compiler toolchain.

## Quick Start

```bash
git clone https://github.com/ninji-research/sarif.git
cd sarif
cargo build --release -p sarifc
```

## Development Setup

- **Rust toolchain**: 1.95.0 (set in `rust-toolchain.toml`, installed automatically via `rustup`)
- **Formatter**: Run `cargo fmt` before committing (Rust 2024 edition)
- **Linter**: `cargo clippy --workspace --all-targets --all-features -- -D warnings`
- **Tests**: `cargo test` run the full suite (122+ CLI tests, 149+ total tests across workspace)
- **Release build**: `cargo build --release -p sarifc` produces the `sarifc` binary

Run specific test subsets:
```bash
cargo test -p sarif_syntax   # lexer, parser, formatter
cargo test -p sarif_codegen  # MIR, escape analysis, backends
cargo test -p sarifc         # CLI integration tests
```

## Pull Request Process

1. **Fork and Branch:** Fork the repository and create a descriptive branch name.
2. **Ensure Code Quality:** All changes must pass the full verification suite: `cargo fmt --check`, `cargo clippy --workspace --all-targets --all-features -- -D warnings`, `cargo test`, and `cargo build --release -p sarifc`.
3. **Follow Architecture:** Read `docs/compiler-architecture.md` for the three-layer model (Syntax, Frontend, Codegen). Read `docs/directives.md` for standing engineering directives.
4. **Benchmark Discipline:** If changes affect codegen or runtime, run `~/bnch` to verify no regressions against the retained 70-case suite.
5. **Submit PR:** Open a pull request against the `main` branch detailing the intent, design rationale, and testing strategy.

## Reporting Bugs

Open a GitHub issue with:
- The Sarif source file that triggers the bug (if applicable)
- The command and flags used (`sarifc check`, `sarifc build`, etc.)
- Expected vs actual behavior
- Rust toolchain version (`rustc --version`)

## Documentation

Documentation lives in `docs/`. When adding language features, update:
- `docs/language-spec.md` — language surface
- `docs/status.md` — current state and benchmarks
- `docs/roadmap.md` — stage progression

*Note: NINJI retains the right to reject contributions that do not align with our architectural directives or quality standards.*
