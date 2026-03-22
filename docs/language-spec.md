# Sarif Language Specification

Sarif is a minimal, single-style, memory-safe systems language. This specification describes the maintained stage-0 syntax and semantics draft for the current bootstrap window.

## Core Principles

1.  **One Syntax:** Exactly one way to write any given construct.
2.  **Single Style:** Enforced by a canonical formatter.
3.  **Memory Safety:** Guaranteed without garbage collection or explicit lifetime annotations in common code.
4.  **Deterministic Semantics:** No hidden allocations, blocking, or complex runtime behavior in ordinary constructs.
5.  **Profile-Based Restriction:** `Core`, `Total`, and `RT` are progressively stricter profiles of the same language.

## Declaration Order

Sarif enforces a strict top-level declaration order to ensure predictability and ease of parsing:

1.  **Types:** `enum` and `struct` declarations.
2.  **Constants:** `const` declarations.
3.  **Functions:** `fn` declarations.

## Syntax

### Types

-   `I32`: 32-bit signed integer.
-   `Bool`: `true` or `false`.
-   `Text`: UTF-8 string (affine).
-   `Unit`: The empty type, written as an empty block `{}` or inferred.
-   `struct`: Product type with named fields.
-   `enum`: Sum type with named variants, optionally carrying payloads.
-   `[T; N]`: Fixed-size internal array.

### Functions

Functions are declared with the `fn` keyword. Returns are implicit: the tail expression of the function body is the return value. The `return` keyword is not supported.

```sarif
fn add(left: I32, right: I32) -> I32 {
    left + right
}
```

### Mutation

Mutation is explicit. Use `let mut` to declare a mutable local and plain assignment to update it.
Mutable locals are slot-backed, so they may carry `Text` and other affine values through repeated assignments in loop bodies.
Mutable local arrays also support indexed assignment with `name[index] = value;`.

```sarif
fn counter() -> I32 {
    let mut x = 0;
    x = x + 1;
    x
}
```

### Control Flow

-   `if ... { ... } [else { ... }]`: Conditional expression. If `else` is omitted, the missing branch is an empty `Unit` block.
-   `match ... { ... }`: Exhaustive pattern matching. Enums match on `Enum.variant` arms. `Bool` matches may use `true`, `false`, or `_`. `I32` and `Text` matches may use literal arms and must end with `_ => { ... }`.
-   `not expr`: Unary boolean negation.
-   `repeat n { ... }`: Counted loop. `n` must be an `I32`.
-   `repeat i in n { ... }`: Counted loop with an immutable index binding `i`.
-   `while cond { ... }`: Condition-driven loop. `cond` must be `Bool`.

### Contracts and Effects

-   `requires <expr>`: Precondition.
-   `ensures <expr>`: Postcondition. Can use the `result` keyword to refer to the return value.
-   `effects [<effect>, ...]`: Explicit effect declarations (e.g., `io`, `alloc`).

### Stage-0 Runtime Builtins

-   `arg_count() -> I32`: Process argument count, available only in executable function bodies.
-   `arg_text(index: I32) -> Text`: Process argument text by index, available only in executable function bodies.
-   `stdin_text() -> Text`: Entire stdin payload as `Text`, available only in executable function bodies.
-   `stdout_write(text: Text) -> Unit`: Writes `text` directly to stdout, available only in executable function bodies.
-   `text_builder_new() -> TextBuilder`: Allocates an opaque runtime text builder, available only in executable function bodies.
-   `text_builder_append(builder: TextBuilder, piece: Text) -> TextBuilder`: Appends `piece` to `builder`, available only in executable function bodies.
-   `text_builder_finish(builder: TextBuilder) -> Text`: Finalizes a builder into immutable `Text`, available only in executable function bodies.
-   `f64_vec_new(len: I32, value: F64) -> F64Vec`: Allocates an opaque runtime `F64` vector filled with `value`, available only in executable function bodies.
-   `f64_vec_len(vec: F64Vec) -> I32`: Returns the vector length.
-   `f64_vec_get(vec: F64Vec, index: I32) -> F64`: Returns the element at `index`; traps on out-of-bounds access.
-   `f64_vec_set(vec: F64Vec, index: I32, value: F64) -> F64Vec`: Stores `value` at `index` and returns the updated vector handle; traps on out-of-bounds access.
-   `f64_from_i32(value: I32) -> F64`: Converts `value` to `F64`.
-   `parse_i32(text: Text) -> I32`: Base-10 integer parse.
-   `sqrt(value: F64) -> F64`: Floating-point square root.
-   `text_from_f64_fixed(value: F64, digits: I32) -> Text`: Fixed-decimal float formatting.

## Profiles

Profiles restrict the language to guarantee specific properties:

-   **Core:** The base language with affine types and memory safety.
-   **Total:** Core minus recursion and unbounded loops, guaranteeing termination.
-   **RT:** Total minus `Text` and other non-deterministic or unbounded resource usage, guaranteeing real-time predictability.

## Standard Workflow

1.  `format`: Pretty-print source code to canonical style.
2.  `check`: Verify semantic correctness and profile compliance.
3.  `run`: Execute the program via the normative MIR interpreter.
4.  `build --target [native|wasm]`: Compile to a native executable or binary WebAssembly.
5.  `doc`: Generate markdown documentation.
