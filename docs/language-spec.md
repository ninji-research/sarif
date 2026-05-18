# Sarif Language Specification

This document describes the maintained stage-0 language surface that the current compiler accepts today.

## Core Rules

- one syntax for each construct
- explicit mutation through `let mut`
- explicit effects through `effects [...]`
- compact expression-bodied functions through `fn name(...) ... = expr;`
- record-field punning through `Pair { left, right }`
- compound mutation through `+=`, `-=`, `*=`, and `/=`
- integer bitwise operators through `&`, `|`, `^`, `<<`, and `>>`
- one semantic oracle: the MIR interpreter
- profiles restrict the same language rather than creating dialects

## Top-Level Declaration Order

Sarif keeps one declaration order:

1. `enum` and `struct`
2. `const`
3. `fn`

## Maintained Stage-0 Types

- `I32`
- `F64`
- `Bool`
- `Text`
- `Bytes`
- `Unit`
- named `struct`
- named `enum`
- fixed arrays `[T; N]`
- repeat fixed-array literals `[value; N]` for duplicate-safe fixed-array elements
- const-generic array length names such as `N` are available as immutable `I32` values inside the same generic function body and contracts
- `TextBuilder` through maintained runtime builtins
- `List[T]` through maintained runtime builtins

## Maintained Stage-0 Control Flow

- `if` / `else`
- chained `else if`
- `match` with literal alternatives through `a | b | c`
- `match` with half-open integer ranges through `lo..hi`
- `while`
- `repeat n`
- `repeat i in n`
- implicit tail-expression returns

## Maintained Stage-0 Runtime Builtins

- `arg_count() -> I32`
- `arg_text(index: I32) -> Text`
- `stdin_text() -> Text`
- `stdin_bytes() -> Bytes`
- `stdout_write(text: Text) -> Unit`
- `stdout_write_builder(builder: TextBuilder) -> TextBuilder`
- `alloc_push() -> Unit`
- `alloc_pop() -> Unit`
- `text_builder_new() -> TextBuilder`
- `text_builder_append(builder: TextBuilder, piece: Text) -> TextBuilder`
- `text_builder_append_codepoint(builder: TextBuilder, codepoint: I32) -> TextBuilder`
- `text_builder_append_ascii(builder: TextBuilder, byte: I32) -> TextBuilder`
- `text_builder_append_slice(builder: TextBuilder, source: Text, start: I32, end: I32) -> TextBuilder`
- `text_builder_append_i32(builder: TextBuilder, value: I32) -> TextBuilder`
- `text_builder_finish(builder: TextBuilder) -> Text`
- `text_index_new() -> TextIndex`
- `text_index_get(index: TextIndex, key: Text) -> I32`
- `text_index_get_or_insert(index: TextIndex, key: Text, next: I32) -> I32`
- `text_index_set(index: TextIndex, key: Text, value: I32) -> TextIndex`
- `list_new(len: I32, value: T) -> List[T]`
- `list_len(vec: List[T]) -> I32`
- `list_get(vec: List[T], index: I32) -> T`
- `list_set(vec: List[T], index: I32, value: T) -> List[T]`
- `list_push(vec: List[T], len: I32, value: T) -> List[T]`
- `list_sort_text(vec: List[Text], len: I32) -> List[Text]`
- `list_sort_by_text_field(vec: List[T], len: I32, field: Text) -> List[T]`
- `f64_from_i32(value: I32) -> F64`
- `parse_i32(text: Text) -> I32`
- `parse_i32_range(text: Text, start: I32, end: I32) -> I32`
- `text_len(text: Text) -> I32`
- `bytes_len(bytes: Bytes) -> I32`
- `text_byte(text: Text, index: I32) -> I32`
- `bytes_byte(bytes: Bytes, index: I32) -> I32`
- `text_concat(left: Text, right: Text) -> Text`
- `text_slice(text: Text, start: I32, end: I32) -> Text`
- `bytes_slice(bytes: Bytes, start: I32, end: I32) -> Bytes`
- `bytes_find_byte_range(bytes: Bytes, start: I32, end: I32, byte: I32) -> I32`
- `text_cmp(left: Text, right: Text) -> I32`
- `text_eq_range(text: Text, start: I32, end: I32, expected: Text) -> Bool`
- `text_find_byte_range(text: Text, start: I32, end: I32, byte: I32) -> I32`
- `text_line_end(text: Text, start: I32) -> I32`
- `text_next_line(text: Text, start: I32) -> I32`
- `text_field_end(text: Text, start: I32, end: I32, byte: I32) -> I32`
- `text_next_field(text: Text, start: I32, end: I32, byte: I32) -> I32`
- `sqrt(value: F64) -> F64`
- `text_from_f64_fixed(value: F64, digits: I32) -> Text`
- `parse_f64(text: Text) -> F64`

`TextIndex` is the maintained dense text-keyed indexing primitive for stage-0 aggregation and lookup. Misses return `-1`; `text_index_get_or_insert(...)` returns the existing slot or inserts `next`; and `text_index_set(...)` mutates the maintained slot-backed handle in place while returning the handle for expression-level composition.

## Stage-0 Affine State Pattern

Owned runtime handles such as `Text`, `Bytes`, `List[T]`, `TextBuilder`, and `TextIndex` are affine. Stage-0 permits the maintained slot-backed handles as direct mutable locals:

```sarif
let mut rows = list_new(8, 0);
let mut len = 0;
rows = list_push(rows, len, 42);
len += 1;
```

Do not wrap those handles in a mutable record and repeatedly assign the record. Keep the affine handle as its own mutable local, then keep scalar metadata such as lengths, counters, and heat scores as separate mutable locals. Immutable records may still be useful as one-shot return values or snapshots.

See `examples/affine_state.sarif` for a complete list/index state example.

## Profiles

- `Core`: maintained base language
- `Total`: stricter profile intended to remove partiality and unbounded execution
- `RT`: stricter profile intended to bound resource use and preserve predictability

The current compiler does not yet provide a complete production-ready `Total` or `RT` authority path. Those remain maintained design directions, not completed release surfaces.

## Explicit Current Boundary

The maintained stage-0 language does not yet provide:

- a full standard library
- threads
- async tasks
- channels
- sockets
- a maintained package/import system beyond the current simple package boundary
