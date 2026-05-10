# Sarif Status

As of May 9, 2026 (updated frequently), Sarif is still in the bootstrap window.

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
- overall score: `0.9171`
- speed score: `0.9171`
- memory score: `0.9751`
- build score: `1.0000`
- deploy-size score: `0.6962`

Individual benchmark ratios vs C (lower is better, <1.0 means FASTER than C):
- fasta: 0.89x (FASTER)
- mandelbrot: 1.11x
- spectralnorm: 1.04x
- nbody: 1.62x (numeric compute)
- revcomp: 2.62x (text streaming - uses per-byte match expression)

That is a real current measurement, not a roadmap claim.

## Source Concision Snapshot

Latest local `~/bnch` source totals for the retained 10-benchmark set:

- Nim: `560` lines / `15821` chars
- Go: `846` lines / `17701` chars
- Sarif: `947` lines / `34869` chars

Sarif is still materially behind the best concise baselines on source size. The recent syntax/runtime work and maintained sort builtins cut retained benchmark source substantially, but the language is not yet at its target concision frontier.

## Important Current Truth

- **Conditional runtime compilation**: `RuntimeFeatures::detect()` scans the program for text builder, text index, and sort usage. The native object backend only declares runtime helpers for features the program actually uses. The C runtime has `#ifndef` guards (`SARIF_NO_TEXT_BUILDER`, `SARIF_NO_TEXT_INDEX`, `SARIF_NO_SORT`) so unused subsystems are excluded from compiled runtime objects, reducing binary size for programs that don't use text builders, text indices, or sort.
- **AllocPush/AllocPop wired up**: The MIR interpreter now properly delegates `AllocPush`/`AllocPop` to `self.alloc_push()` / `self.alloc_pop()`. The C runtime's arena allocator correctly pushes and pops allocation scopes, validated by CLI regression tests.
- Sarif now covers the full retained main-track benchmark suite in `~/bnch`
- Sarif is currently first overall, first on speed, first on memory, first on build time, and second on deploy size in the latest local clean `~/bnch` run
- Sarif is currently first on build time; the native artifact path now reuses cached runtime objects instead of recompiling the static runtime every build, compiles the shared C runtime with a size-oriented flag set while leaving generated code on the maintained performance-oriented path, skips record/enum metadata glue entirely for scalar `main` results, compiles out structured-result pretty-printing when scalar mains do not need it, avoids libc integer formatting on the scalar print path, routes stage-0 text/int/bool/record/enum output through one direct-write runtime path instead of the wider stdio surface, removes extra runtime hardening/ident baggage Sarif does not need in release mode, and the native linker path garbage-collects unused sections so stage-0 artifacts stay lean by default
- maintained integer bitwise operators `&`, `|`, `^`, `<<`, and `>>` are now available in stage-0 and remove arithmetic-emulation overhead from hot integer/text kernels
- MIR-level constant folding for integer and float binary operations reduces runtime arithmetic in numeric workloads; 21 tests cover add, sub, mul, div, bitwise ops, and comparisons at compile time
- float formatting fast-path for integer-valued doubles avoids snprintf overhead and improves performance of floating-point output
- chained `else if` is again accepted as maintained stage-0 syntax, with parser/AST/runtime regression coverage instead of relying on benchmark-local nesting workarounds
- unary `not` now binds over full postfix expressions such as `not flag()`, eliminating another source-level workaround path and restoring the expected compact boolean style
- maintained `match` pattern alternatives `a | b | c` and half-open integer ranges `lo..hi` are now available in stage-0 and remove nested byte/CDF ladders from retained kernels without introducing benchmark-specific builtins
- maintained line-scanning builtins `text_line_end(...)` and `text_next_line(...)` are now available in stage-0 and remove duplicated CRLF and line-advance scaffolding from retained text workloads
- maintained field-scanning builtins `text_field_end(...)` and `text_next_field(...)` are now available in stage-0 as the coherent delimiter-scanning surface for retained structured-text workloads
- the text builder integer path now formats directly into the builder's reserved space without an intermediate scratch buffer, eliminating an extra memcpy and recovering the small speed regression introduced by the prior scratch-buffer refactor
- native owned text results from text-builder finish, text concatenation, text/bytes slicing, and fixed-precision float formatting now allocate through the scoped arena path; runtime argument text (`arg_text()`) uses process-lifetime malloc since argv is OS-provided process-lifetime memory, with native regression coverage for repeated `alloc_push`/`alloc_pop` text allocation
- the wasm backend now supports the pure stage-0 text helper tier `text_cmp(...)`, `text_eq_range(...)`, `text_find_byte_range(...)`, `text_line_end(...)`, `text_next_line(...)`, `text_field_end(...)`, `text_next_field(...)`, `text_slice(...)`, and `parse_i32_range(...)`, with runnable CLI parity coverage
- duplicated frontend semantic handling for `bytes_byte(...)`, `bytes_slice(...)`, and `bytes_find_byte_range(...)` has been collapsed so the maintained builtin surface now has one diagnostic path per primitive instead of drift-prone copies
- retained `knucleotide` now uses one maintained percent-line path and one maintained count-line path instead of duplicated formatting helpers; canonical source formatting restored (183 lines vs minified 1 line) for honest concision tracking
- retained `revcomp` and `csvgroupby` had redundant source-level temporary/slice scaffolding removed without changing benchmark behavior; canonical source formatting restored for honest concision tracking
- retained `joinagg` now uses one maintained row-cut helper, natural `else if` chains, and direct boolean negation instead of parser and unary-workaround scaffolding; canonical source formatting restored for honest concision tracking
- mutable stage-0 fixed-array locals are now scalarized into element slots during MIR lowering, so hot indexed reads and writes no longer rebuild whole synthetic array records on every mutation
- immutable stage-0 fixed-array parameters now lower onto the same slot-backed path, so repeated indexing in helper functions no longer pays whole-array extraction cost on every access
- fixed-array slot selection and update now lower through balanced decision trees instead of linear `index == k` ladders, shrinking retained native code for array-heavy kernels
- fixed-array accesses driven by proven `repeat` indices now skip redundant bounds-assert MIR scaffolding, so retained numeric kernels no longer pay dynamic safety code for statically safe loop-indexed accesses
- fixed-array accesses with compile-time constant indices now lower directly to slot/field operations instead of flowing through the generic decision-tree path
- retained `nbody` now benefits from that slot-backed, balanced, bounds-eliding, constant-folded fixed-array path; in the latest clean `~/bnch` run it remains correct at `1.5511s`, `89.85 MiB`, and `18.84 KiB`
- the stage-0 object backend now exports only the runtime entrypoint symbol instead of every user helper function, keeping native symbol policy closer to the actual execution model
- the stage-0 object backend now emits with Cranelift `speed_and_size` tuning instead of a pure `speed` bias, which restored first place on build time and slightly reduced native artifact size without giving back first place on speed or memory
- Sarif still materially trails Nim and Go on retained benchmark source concision; canonical formatting discipline restored (947 lines vs the prior minified 10-line snapshot) to keep concision metrics honest
- the maintained `TextIndex` primitive is now promoted as the dense text-keyed aggregation/indexing path used by the strongest retained Sarif benchmark lanes
- the maintained `TextIndex` surface now includes `text_index_get_or_insert(...)`, which removes the repeated stage-0 `get`/`set` upsert boilerplate from retained aggregation workloads without introducing a second indexing abstraction
- the native stage-0 backend now correctly lowers fixed array value types such as `[I32; 4]` and `[F64; 5]`, with regression coverage in the CLI build tests
- signature-only stage-0 fixed arrays are now registered before native/object ABI emission, with regression coverage for array-typed function parameters that do not rely on body literals
- inferred const-generic fixed-array helpers now build cleanly on the native backend, and their array length parameters are now available as immutable `I32` values inside the same generic function body and contracts
- repeat fixed-array literals `[value; N]` are now maintained stage-0 syntax for duplicate-safe fixed-array elements, reusing the same fixed-length array model instead of introducing a second dynamic array form
- the `binarytrees` lane no longer exhibits the prior pathological temporary-tree retention
- **Stage-1 MIR-level escape analysis** now uses interprocedural fixed-point iteration instead of conservative `has_alloc` heuristic; each function's result is analyzed via data-flow through the call graph until stable. Eliminates false positives from pass-through wrappers and non-allocating callees.
- `arg_text` corrected from arena to `malloc` allocation: argv strings are process-lifetime, could invalidate results across `alloc_pop`. Now consistent with `stdin_cache`.
- **`ExecFlow::Return` dead code removed**: the variant was never constructed; the entire enum removed, `execute_insts` return type simplified to `Result<(), RuntimeError>`, all unreachable match arms eliminated. Net −48 lines.
- **Total profile** now accepts `repeat N` with a compile-time constant integer count as statically terminating; `while` and non-constant repeats remain rejected.
- **MIR interpreter Call trampoline**: `Call` handling made fully iterative via `callee_stack: Vec<CalleeFrame>` in `execute_insts`, eliminating unbounded recursion proportional to call chain depth.
- **Performance: eliminated `values.clone()`** in If/While/Repeat/Perform — branches now execute directly on the real `values`/`slots` vecs instead of cloning 200+ RuntimeValues per branch/loop iteration.
- **Performance: internalized `args`** into `execute_insts` state — `active_args` managed by callee stack inside the loop, eliminating all `args.clone()` in branches.
- **C runtime fuzzing**: clang libFuzzer + ASan harness (13 opcodes), caught and fixed `sarif_slice_blob` UTF-8 backward continuation-byte underflow. 20M iterations: **0 crashes, 0 memory safety violations**, RSS stable at 216MB. Extended 100M-iteration run now in progress (~200K exec/s).
- **Rust-side pipeline fuzzing**: `cargo +nightly fuzz` target covering lexer → parser → AST → HIR → semantic → MIR → escape analysis. 8,504 coverage blocks in first 10 seconds. Long-term run in progress. Found 1 timeout artifact (NUL byte infinite loop in logos lexer).
- **Null byte infinite loop fixed**: The `logos` lexer entered an infinite loop on NUL bytes (`U+0000`) because logos returns `Err` with a zero-length span and doesn't advance. Fix adds a defense-in-depth guard: detects zero-length spans on `Err` tokens, emits a `lex.null-byte` diagnostic, and recreates the lexer past the offending byte. This prevents hangs from any unparseable byte, not just NUL.
- **Semantic infinite loop fixed**: `infer_param_modes` fixpoint iteration in `ownership.rs` could oscillate forever when duplicate function definitions with different parameter names existed. Fixed by skipping duplicate `functions.insert()` in `resolve.rs` and deduplicating by name in the `while changed` loop as defense-in-depth.
- **3 new native build tests**: `list_sort_text` sort feature, no-text-builder, and no-text-index conditional compilation verification.
- **Binary size reduced 57%**: Added `-g0` to compile flags and `-Wl,-s` to ELF link flags — hello binary went from 11,112 B to 4,792 B.
- **CI workflow** defined in `.github/workflows/ci.yml` (build, test, clippy, formatting check) — `on: push` and `on: pull_request` to `main`. Pushed and active.
- **C runtime overflow guards**: text builder reserve growth guard against UINT64_MAX wraparound; list push capacity guard `used > UINT64_MAX/2` before doubling.
- the maintained compiler is still Rust-hosted
- alloc-escape diagnostics now require actual body-level allocation, including transitive calls to `[alloc]` functions, so non-allocating compatibility declarations no longer produce false Stage-0 escape warnings; runtime text ownership audit complete: text_concat and text_slice no longer return original scoped arena pointers (always allocate), arg_text uses process-lifetime malloc, stdin_cache uses process-lifetime malloc; MIR-level escape analysis (Stage-1) is complete as interprocedural fixed-point iteration, replacing the earlier conservative `has_alloc` approach
- the native executable path is maintained on Linux, feasible but less exercised on macOS, and not yet maintained on Windows or mobile hosts; the current platform matrix is recorded in `docs/platforms.md`
- Stage-1 bootstrap HIR→MIR lowering is now complete; remaining work is self-hosting the tools themselves
- Repository audit complete (May 2026): empty `bootstrap/sarif_compiler` directory removed as cruft; no dead code, no TODOs; codebase is lean and well-organized

## Stage-1 Completion Requirements

Stage-1 self-hosting requires bootstrap HIR→MIR lowering for control flow and data access.
The Rust frontend handles these correctly via Cranelift JIT. The bootstrap compiler
(sarif_syntax) is reference infrastructure showing the lowering concepts.

**Completed bootstrap HIR→MIR lowering:**
- If/While/Repeat/Match (control flow) - NOW COMPLETE
- Field/Index/Record/Array (data access) - NOW COMPLETE
- Record creation (mir_inst_make_record) - COMPLETE
- Array creation (mir_inst_list_new) - COMPLETE

**Completed Stage-1 infrastructure:**
- Runtime memory model is sound (text arena, escape analysis)
- Constant folding at MIR lowering (24 tests covering int/float operators)
- Bootstrap bitwise operator lowering
- RT profile escape analysis as hard error
- MIR-level interprocedural escape analysis with fixed-point iteration (replaces conservative `has_alloc` heuristic)
- MIR interpreter Call trampoline (iterative, eliminates unbounded recursion)
- ExecFlow::Return dead code removed (simplified interpreter control flow)
- Conditional runtime compilation: `RuntimeFeatures::detect()` + `Option<FuncId>` fields + C `#ifndef` guards
- 122 CLI tests pass, 149+ total tests, clippy clean

**Remaining Stage-1 work:**
- Self-host check/doc using the bootstrap compiler as maintained authority
  (bootstrap format is flipped to Sarif host; check/doc remain Rust aliases because the
  Sarif bootstrap does not have full semantic analysis parity — assessed as not practical
  to implement for the current bootstrap runtime; requires ~4150 lines of type inference
  + ownership tracking and is constrained by bootstrap fixed-size tuple limits)

**Current bnch scores (May 2026):**
- overall rank: `1/7`
- speed rank: `1/7`
- memory rank: `1/7`
- build rank: `1/7`
- overall score: `0.9189`
- speed score: `0.9189`
- memory score: `0.9730`
- build score: `1.0000`
- deploy-size score: `0.6962`

A full standard library is not complete:
- async, parallel, and multithreaded runtime support are not complete
