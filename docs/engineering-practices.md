# Sarif Engineering Practices

This guide records the current verification workflow and retained-corpus discipline.

## Maintained Verification

Treat a change as incomplete until these pass:

```bash
cargo test
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo build --release -p sarifc
```

Useful aliases:

```bash
cargo xfmt
cargo xtest
cargo xlint
cargo xbuild
cargo xbuild-fast
cargo xbuild-max
```

## Build Profile Policy

- use `dev` and `test` for fast local iteration
- use `release` for maintained shipping verification
- use `release-fast` for cheaper optimized iteration
- use `release-max` only when checking a higher-cost optimized build
- use `profiling` when you need debug info in a release-like binary

## Retained Corpora

Retained corpora are inputs whose outputs must stay exact across maintained surfaces:

- shipped examples
- bootstrap packages
- retained semantic diagnostics
- retained semantic markdown outputs

When behavior changes, update the retained assertion only if the new behavior is intentionally correct and explained by the maintained spec.

## IR Dump Workflow

```bash
sarifc check main.sarif --dump-ir=resolve
sarifc check main.sarif --dump-ir=typecheck
sarifc build main.sarif --dump-ir=lower -o main
sarifc build main.sarif --target wasm --dump-ir=codegen -o main.wasm
```

Use these dumps when bisecting regressions. Keep fixes pinned by tests or retained outputs rather than informal notes.
