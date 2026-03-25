# Sarif Status

Sarif is still in the **Bootstrap Window** (Stage 0). The project has a working self-hosting substrate and maintained bootstrap corpora, but it has not yet crossed the release-authority boundary into Stage 1.

## Verified Today

-   **Maintained authority is green:** `cargo test` and `cargo clippy --workspace --all-targets --all-features -- -D warnings` pass.
-   **Rust-hosted CLI is stable:** `sarifc format`, `check`, `run`, `build`, and `doc` are the maintained release surfaces.
-   **Bootstrap package execution works on both backends:** `bootstrap/sarif_syntax` runs to its retained score on the interpreter and now also builds and runs on the Wasm target.
-   **Wasm stage-0 coverage improved materially:** payload-enum equality, packed text constants, text length/byte/concat/slice, parsing helpers, and allocator-backed memory growth are all implemented well enough for the retained bootstrap package surface.
-   **Bootstrap parity surfaces are retained:** `bootstrap-format` matches the maintained formatter on the retained shipped corpus, while `bootstrap-check` and `bootstrap-doc` are bridged to the maintained semantic surfaces and pinned by retained outputs.
-   **`~/bnch` remains clean:** manifest validation and `tests.test_run` both pass.

## Authority Boundary

-   **Normative semantic oracle:** the MIR interpreter.
-   **Maintained release authority:** the Rust implementation.
-   **Experimental self-host surface:** the Sarif-hosted formatter plus the bridged `bootstrap-check` and `bootstrap-doc` commands.

## Not Yet Complete

-   **Stage-1 self-hosted tooling authority:** Sarif-hosted `format`, `check`, and `doc` are not the maintained release authorities yet.
-   **Full self-hosted compiler:** HIR lowering, MIR generation, and backend ownership remain Rust-hosted.
-   **Rust archival:** the Rust code is still required for release authority, backend generation, and verification, so it is not ready to archive.

## Immediate Next Steps

1.  Promote the Sarif-hosted formatter from retained parity to maintained authority.
2.  Replace the bridged `bootstrap-check` and `bootstrap-doc` paths with Sarif-hosted semantic implementations.
3.  Continue moving compiler pipeline ownership from Rust into Sarif only after the tooling authority boundary is actually closed.
