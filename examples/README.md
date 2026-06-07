# Sarif Language Examples

This directory contains example Sarif programs demonstrating various language features.

## Running Examples

```bash
# Check/run an example (native execution)
sarifc run examples/01_hello.sarif

# Build to native executable
sarifc build examples/02_math.sarif --target native -o examples/02_math

# Build to WebAssembly
sarifc build examples/07_wasm_basic.sarif --target wasm -o examples/07_wasm_basic.wasm
```

## Examples

| File | Features Demonstrated |
|------|----------------------|
| `01_hello.sarif` | String literals, escape sequences (`\t`, `\n`), stdout output, main entrypoint |
| `02_math.sarif` | Integer arithmetic, `while` loops, accumulator pattern, function calls |
| `03_text_processing.sarif` | `text_len`, `text_cmp`, `text_concat`, escape sequences in output |
| `04_record_types.sarif` | `struct` definition, record construction, field access, multiple records |
| `05_template_literals.sarif` | Template literals (`"hello {expr}"`), string interpolation |
| `06_match_expressions.sarif` | `match` expressions, integer/string patterns, guards, fallthrough |
| `07_wasm_basic.sarif` | Minimal program compiling to WASM target |

## Prerequisites

Build the `sarifc` CLI first:

```bash
cargo build --release -p sarifc
```

For WASM compilation, build with the WASM feature enabled:

```bash
cargo build --release -p sarifc --features wasm
```

## Language Documentation

See [language-spec.md](../docs/language-spec.md) for the full language specification.
