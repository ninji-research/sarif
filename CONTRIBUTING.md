# Contributing to Sarif

Thank you for your interest in contributing. We are committed to maintaining a clean, well-documented, and high-performance compiler toolchain.

## Pull Request Process

1. **Fork and Branch:** Fork the repository and create a descriptive branch name.
2. **Ensure Code Quality:** All changes must pass `cargo test`, `cargo clippy --workspace --all-targets --all-features -- -D warnings`, and `cargo build --release -p sarifc`. Follow the existing Rust code style and the compiler architecture documented in `docs/compiler-architecture.md`.
3. **Submit PR:** Open a pull request against the `main` branch detailing the intent, design rationale, and testing strategy.

*Note: NINJI retains the right to reject contributions that do not align with our architectural directives or quality standards.*
