# Self-Hosting Progress

## Overview

Sarif's self-hosting effort aims to have the Sarif compiler compiled by Sarif itself.
Currently, the maintained compiler is Rust-hosted. The bootstrap compiler is written
in Sarif and lives in `bootstrap/`.

## What's Self-Hosted

### Formatter (Bootstrap Format)

The source formatter is fully self-hosted via `sarifc bootstrap-format`. The bootstrap
compiler in `bootstrap/sarif_tools/` formats Sarif source text identically to the Rust
formatter. Parity is verified by CLI regression tests (`bootstrap_format_parity_paths.txt`).

Status: **Complete** — flipped to Sarif host by default.

### Doc Generator (Bootstrap Doc)

The semantic documentation generator is self-hosted via `sarifc bootstrap-doc`.

Status: **Complete** — flipped to Sarif host by default.

## What's In Progress

### Type Checking (Bootstrap Check)

The type checker is partially self-hosted via `sarifc bootstrap-check`. The bootstrap
compiler performs:

- [x] Parsing (lexer → parser → AST)
- [x] Duplicate definition detection
- [x] Call resolution (unknown function calls)
- [x] Const type validation (basic type existence)
- [x] Function return type validation
- [x] Basic expression type inference (integer, float, bool, string literals)
- [x] Name resolution from locals, consts, and known names
- [x] Binary operator argument type checking
- [x] Match arm type checking
- [x] If expression type inference
- [ ] Full expression body type inference (all expression kinds)
- [ ] Generic parameter resolution
- [ ] Array type inference with const generic lengths
- [ ] List element type tracking
- [ ] Field access type resolution through nested structs
- [ ] Error recovery (Type::Error propagation)
- [ ] Span-accurate diagnostic messages

### Ownership Tracking

The ownership checker is under active development:

- [x] Affine type detection (Text, Bytes, TextBuilder, TextIndex, File, List[...], Optional[...])
- [x] Builtin borrow/consume classification
- [x] Parameter affine usage detection (reuse tracking)
- [ ] Field-level access path tracking
- [ ] Match arm payload alias tracking
- [ ] Repeat body protection (protected roots)
- [ ] Branch union moves (if/else, match arms)
- [ ] Mutable local tracking
- [ ] Contract clause ownership checking
- [ ] Param mode inference with fixpoint iteration

## What's Remaining

### HIR Lowering (Stage-1)

The HIR representation and lowering are handled by the Rust frontend. Bootstrap HIR→MIR
lowering for control flow and data access is complete. Remaining work:

- [x] If/While/Repeat/Match control flow
- [x] Field/Index/Record/Array data access
- [x] Record creation
- [x] Array creation (list_new)
- [ ] Self-host the HIR lowering pass itself

### MIR

- [x] Constant folding (int/float binary ops)
- [x] Bitwise operator lowering
- [x] Escape analysis (interprocedural fixed-point)
- [x] MIR interpreter Call trampoline (iterative)
- [ ] Self-host MIR analysis passes

### Backends

- [x] C backend (maintained, used for native builds)
- [x] Cranelift JIT backend (used for bootstrap execution)
- [ ] wasm backend (feasible but less exercised)
- [ ] Self-host code generation

## How to Run Parity Tests

### Bootstrap Format Parity

```bash
# Test format parity between Rust and Sarif formatters
cargo test bootstrap_format_parity 2>&1 | tail -20
```

### Bootstrap Check

```bash
# Run Rust semantic checker on a source file
cargo run -- check path/to/file.sarif

# Run bootstrap (Sarif) checker on a source file
cargo run -- bootstrap-check path/to/file.sarif

# Run bootstrap self-check (bootstrap checks itself)
cargo run -- bootstrap-check bootstrap/sarif_tools/Sarif.toml
```

### Ownership Parity Test

```bash
# Run the parity test comparing Rust and bootstrap ownership checking
cargo test bootstrap_ownership_parity 2>&1 | tail -20
```

### Full Test Suite

```bash
cargo test 2>&1 | tail -20
```

## Current Gaps Between Rust and Sarif Implementations

### Type Inference (exprcore.rs vs typecheck.sarif)

| Feature | Rust | Sarif Bootstrap |
|---------|------|-----------------|
| Expression type inference | ~4500 lines | ~424 lines |
| Builtin function signatures | ~120 entries | ~50 entries |
| Generic type params | Full support | Not supported |
| Array length inference | Const expr support | Not supported |
| List element type tracking | Full | Basic |
| Error recovery with Type::Error | Yes | No |
| Span-accurate diagnostics | Yes | Limited |
| Context-aware checking (Body/Contract) | Yes | No |

### Ownership Tracking (ownership.rs vs ownership.sarif)

| Feature | Rust | Sarif Bootstrap |
|---------|------|-----------------|
| Implementation size | ~2069 lines | ~220 lines |
| Affine type detection | Recursive through structs/enums | Flat type list |
| Field-level path tracking | Full access paths | Not implemented |
| Match arm payload aliasing | Yes | No |
| Repeat body protection | Protected roots | Not implemented |
| Branch union moves | If/else + match arms | Not implemented |
| Mutable local tracking | Yes | No |
| Contract clause checking | Separate mode | Not implemented |
| Param mode inference | Fixpoint iteration | Static per-function |
| Diagnostic messages | Detailed with spans | Basic text messages |
