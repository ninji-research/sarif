# Sarif Roadmap

## Release Rule

No feature lands as maintained authority until it is covered by the specification, formatter, diagnostics, documentation surface, and retained corpus.

## Stage 0

Current maintained authority:

- Rust-hosted compiler and CLI
- MIR interpreter as oracle
- native backend
- stage-0 Wasm backend with explicit host-input imports and remaining builtin exclusions

## Stage 1

Promote Sarif-hosted tooling to maintained authority.

Completed:
- **Formatter**: `sarifc format` now runs the Sarif-hosted formatter by default, passing parity tests against the Rust formatter for all shipped inputs. This validates the bootstrap compiler against serious production use.
- **C Runtime Subsystem Cleanup**: Removed all conditional compilation flags (`RuntimeFeatures::detect` and C runtime `#ifndef` blocks), allowing the native link stage (`-Wl,--gc-sections` or equivalent) to naturally prune unused runtime functions from generated binaries.
- **Semantic fixpoint cycle fixed**: `infer_param_modes` no longer oscillates on duplicate function definitions.
- **Binary size reduced 57%**: `-g0` and `-Wl,-s` added to compile/link flags; hello binary 4,792 B stripped.
- **Cranelift `speed` tuning**: Changed from `speed_and_size` to `speed` opt_level and removed `regalloc_algorithm` override, letting Cranelift use its default register allocator for better generated code quality.
- **Null trap gated behind debug_assertions**: Release builds skip the unnecessary null-pointer check on every call result.
- **C runtime I/O optimized**: `sarif_write_all` rewritten to use `fwrite()` and `setvbuf` for full stdout buffering.

Remaining (blocked — see Memory Model section):
- **Semantic `check`**: Requires a Sarif-hosted semantic analysis pass that does type checking, name resolution, and borrow inference. Not yet implemented despite bootstrap tuple limits being resolved (all MIR/HIR list types now support 16 slots). Requires ~4150 lines of type inference + ownership tracking infrastructure.
- **Semantic `doc`**: Shares the same semantic analysis dependency as `check`.

Rust remains required until those authority paths are actually replaced without reducing correctness or coverage.

### Stage-1 Memory Model Requirements

Before self-hosting can be achieved, the memory model must be fully sound:

**Text Arena Integration (Technical Debt)**

Most owned native `Text` results now allocate through the scoped arena system instead of unmanaged one-off text allocations. This includes text builder finish, concatenation, slicing, and fixed-precision float formatting. Runtime argument text (`arg_text()`) uses process-lifetime malloc since argv is OS-provided process-lifetime memory. stdin_cache also uses process-lifetime malloc. This removes the most direct Stage-0 leak path for scoped text-heavy workloads.

Completed:
- audit of runtime text ownership: text_concat and text_slice no longer return original scoped arena pointers
- arg_text uses process-lifetime malloc (argv is OS-provided)
- stdin_cache uses process-lifetime malloc
- text_concat always allocates fresh memory
- text_slice always allocates fresh memory

Required remaining work:
- decide whether long-lived text needs explicit ownership, interning, or a separate process-lifetime arena
- add measurement coverage for long-running scoped text workflows

**Escape Analysis for [alloc] (Stage-1 Hard Error — Complete)**

Stage-0 emits a `semantic.alloc-escape` warning when `[alloc]` functions return types that could reference arena-allocated memory and the function body actually allocates, including transitive calls to other `[alloc]` functions. Non-allocating compatibility declarations no longer produce escape warnings.

MIR-level escape analysis (interprocedural fixed-point iteration) now implements the full Stage-1 proof:
- Detects when a pointer to arena-allocated data would escape the scope where it was created
- Emits a hard error (blocks compilation) for RT profile; warnings for Core and Total profiles
- Distinguishes between allocations created inside an `[alloc]` function (which cannot be safely returned) and parameters passed into the function (which can be returned)
- Analysis is monotonic (false → true only), converging in at most N passes for N functions
- Eliminates false positives from pass-through wrappers and non-allocating callees

This eliminates the "trust the programmer" model and brings Sarif's memory safety guarantees in line with its performance goals.

## Stage 2

Move compiler pipeline ownership into Sarif:

- HIR lowering
- MIR generation
- backend ownership

## Standard Library Roadmap

Sarif does not yet ship a full standard library. The maintained surface today is a stage-0 builtin substrate plus formatting, checking, docs, and runtime support.

### Maintained Today

- scalar arithmetic and comparisons
- text construction and slicing
- direct parse helpers
- list allocation and indexed access
- deterministic runtime input/output builtins on native/interpreter paths and wasm host-import paths
- Bounded memory arena scopes (`with_arena { ... }`) for temporary allocations

### Planned Standard Library Layers

1. `core`
   - scalar types
   - text views and builders
   - list and fixed-shape collection primitives
   - result/option-style control data

2. `alloc`
   - owned collections
   - maps and sets with stable semantics
   - arena and scoped allocation interfaces where justified

3. `io`
   - file and process interfaces
   - text and byte streams
   - explicit capability-gated resource handles

4. `rt`
   - restricted concurrency primitives
   - explicit task and scheduling model
   - Bounded synchronization primitives

### Core Design Rules

- No duplicated APIs for the same job.
- No hidden global runtime.
- No async surface until the task/resource model is mechanically defined.
- No parallel surface until determinism and memory rules are specified together.
- The next standard-library boundary is content-aware text/map support for aggregation, not a broad grab-bag of convenience APIs.

## Reactive Runtime Direction

Sarif's maintained direction for reactive and notebook-like systems is runtime-first, not syntax-first.

The intended rule is:

- keep the language core general-purpose
- keep pure-function semantics and explicit effects as the foundation
- add zero-copy runtime-facing data surfaces where they remain broadly useful
- build DAG invalidation, recomputation, and scheduling as a maintained runtime layer
- avoid hardcoding one dataframe, transport, or UI stack into the language

This allows Sarif to host a zero-copy reactive environment without turning the language into a product-specific DSL.

## Concurrency And Scheduling

Sarif does not have a maintained async or multithreaded story yet.

The intended direction is:

- one concurrency model, not multiple competing ones
- analyzable task spawning and channels first
- bounded executor semantics
- `RT` restricted to deterministic, bounded scheduling rules
- any future reactive scheduler must reuse that same explicit task model instead of introducing hidden parallelism

Async syntax is only acceptable if it lowers to that same maintained task model instead of creating a second runtime.

## Performance And Build Tooling

The maintained direction is:

- fast local iteration profiles
- small, highly optimized release builds
- explicit profiling workflow
- benchmark-driven runtime and backend work
- deterministic MIR behavior even as codegen quality improves

## Current Hard Boundaries

Sarif does not yet ship:

- a full standard library
- maintained async support
- maintained multithreading support
- maintained parallel runtime primitives
- a maintained reactive DAG runtime
- self-hosted release authority for `check` or `doc`

Format is flipped and self-hosted (`bootstrap-format` is the default `sarifc format` path).

Platform reality is tracked separately in [platforms.md](platforms.md): Linux native is the maintained host target, macOS native is feasible but less exercised, wasm is maintained with explicit host-input imports and remaining exclusions, and Windows/mobile/cross-compilation remain future work rather than implied support.
