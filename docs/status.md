# Sarif Status

As of April 15, 2026, Sarif is still in the bootstrap window.

## Verified

- `cargo test` passes
- `cargo clippy --workspace --all-targets --all-features -- -D warnings` passes
- `cargo build --release -p sarifc` passes
- `~/bnch` manifest validation and harness unit tests pass
- `~/bnch` full 70-case main-track run completes cleanly with no excluded build-fail, run-fail, or mismatch rows
- `sarifc run` now executes retained bootstrap packages without the prior interpreter stack-overflow failure

## Benchmark Snapshot

Latest local `~/bnch` run on this machine:

- overall rank: `1/7`
- speed rank: `1/7`
- memory rank: `1/7`
- build rank: `1/7`
- deploy-size rank: `2/7`
- overall score: `1.0000`
- deploy-size score: `0.7050`

That is a real current measurement, not a roadmap claim.

## Source Concision Snapshot

Latest local `~/bnch` source totals for the retained 10-benchmark set:

- Nim: `560` lines / `15821` chars
- Go: `846` lines / `17701` chars
- Sarif: `891` lines / `30464` chars

Sarif is still materially behind the best concise baselines on source size. The recent syntax/runtime work and maintained sort builtins cut retained benchmark source substantially, but the language is not yet at its target concision frontier.

## Important Current Truth

- Sarif now covers the full retained main-track benchmark suite in `~/bnch`
- Sarif is currently first overall, first on speed, first on memory, first on build time, and second on deploy size in the latest local clean `~/bnch` run
- Sarif is currently first on build time; the native artifact path now reuses cached runtime objects instead of recompiling the static runtime every build, compiles the shared C runtime with a size-oriented flag set while leaving generated code on the maintained performance-oriented path, skips record/enum metadata glue entirely for scalar `main` results, compiles out structured-result pretty-printing when scalar mains do not need it, avoids libc integer formatting on the scalar print path, routes stage-0 text/int/bool/record/enum output through one direct-write runtime path instead of the wider stdio surface, removes extra runtime hardening/ident baggage Sarif does not need in release mode, and the native linker path garbage-collects unused sections so stage-0 artifacts stay lean by default
- maintained integer bitwise operators `&`, `|`, `^`, `<<`, and `>>` are now available in stage-0 and remove arithmetic-emulation overhead from hot integer/text kernels
- chained `else if` is again accepted as maintained stage-0 syntax, with parser/AST/runtime regression coverage instead of relying on benchmark-local nesting workarounds
- unary `not` now binds over full postfix expressions such as `not flag()`, eliminating another source-level workaround path and restoring the expected compact boolean style
- maintained `match` pattern alternatives `a | b | c` and half-open integer ranges `lo..hi` are now available in stage-0 and remove nested byte/CDF ladders from retained kernels without introducing benchmark-specific builtins
- maintained line-scanning builtins `text_line_end(...)` and `text_next_line(...)` are now available in stage-0 and remove duplicated CRLF and line-advance scaffolding from retained text workloads
- maintained field-scanning builtins `text_field_end(...)` and `text_next_field(...)` are now available in stage-0 as the coherent delimiter-scanning surface for retained structured-text workloads
- the wasm backend now supports the pure stage-0 text helper tier `text_cmp(...)`, `text_eq_range(...)`, `text_find_byte_range(...)`, `text_line_end(...)`, `text_next_line(...)`, `text_field_end(...)`, `text_next_field(...)`, and `parse_i32_range(...)`, with runnable CLI parity coverage
- the wasm backend now supports read-only `Bytes` values and pure `bytes_len(...)`, `bytes_byte(...)`, `bytes_slice(...)`, and `bytes_find_byte_range(...)` operations; the remaining wasm boundary is runtime-input builtins such as `stdin_bytes(...)`, not the `Bytes` substrate itself
- duplicated frontend semantic handling for `bytes_byte(...)`, `bytes_slice(...)`, and `bytes_find_byte_range(...)` has been collapsed so the maintained builtin surface now has one diagnostic path per primitive instead of drift-prone copies
- retained `knucleotide` now uses one maintained percent-line path and one maintained count-line path instead of duplicated formatting helpers
- retained `revcomp` and `csvgroupby` had redundant source-level temporary/slice scaffolding removed without changing benchmark behavior
- retained `joinagg` now uses one maintained row-cut helper, natural `else if` chains, and direct boolean negation instead of parser and unary-workaround scaffolding
- mutable stage-0 fixed-array locals are now scalarized into element slots during MIR lowering, so hot indexed reads and writes no longer rebuild whole synthetic array records on every mutation
- immutable stage-0 fixed-array parameters now lower onto the same slot-backed path, so repeated indexing in helper functions no longer pays whole-array extraction cost on every access
- fixed-array slot selection and update now lower through balanced decision trees instead of linear `index == k` ladders, shrinking retained native code for array-heavy kernels
- fixed-array accesses driven by proven `repeat` indices now skip redundant bounds-assert MIR scaffolding, so retained numeric kernels no longer pay dynamic safety code for statically safe loop-indexed accesses
- fixed-array accesses with compile-time constant indices now lower directly to slot/field operations instead of flowing through the generic decision-tree path
- retained `nbody` now benefits from that slot-backed, balanced, bounds-eliding, constant-folded fixed-array path; in the latest clean `~/bnch` run it remains correct at `1.5511s`, `89.85 MiB`, and `18.84 KiB`
- the stage-0 object backend now exports only the runtime entrypoint symbol instead of every user helper function, keeping native symbol policy closer to the actual execution model
- the stage-0 object backend now emits with Cranelift `speed_and_size` tuning instead of a pure `speed` bias, which restored first place on build time and slightly reduced native artifact size without giving back first place on speed or memory
- Sarif still materially trails Nim and Go on retained benchmark source concision
- the maintained `TextIndex` primitive is now promoted as the dense text-keyed aggregation/indexing path used by the strongest retained Sarif benchmark lanes
- the native stage-0 backend now correctly lowers fixed array value types such as `[I32; 4]` and `[F64; 5]`, with regression coverage in the CLI build tests
- signature-only stage-0 fixed arrays are now registered before native/object ABI emission, with regression coverage for array-typed function parameters that do not rely on body literals
- inferred const-generic fixed-array helpers now build cleanly on the native backend, and their array length parameters are now available as immutable `I32` values inside the same generic function body and contracts
- repeat fixed-array literals `[value; N]` are now maintained stage-0 syntax for duplicate-safe fixed-array elements, reusing the same fixed-length array model instead of introducing a second dynamic array form
- the `binarytrees` lane no longer exhibits the prior pathological temporary-tree retention
- the maintained compiler is still Rust-hosted
- the native executable path is maintained on Linux, feasible but less exercised on macOS, and not yet maintained on Windows or mobile hosts; the current platform matrix is recorded in `docs/platforms.md`
- self-hosted tooling authority is not complete
- a full standard library is not complete
- async, parallel, and multithreaded runtime support are not complete
