# Sarif Language Specification

This document describes the maintained stage-0 language surface that the current compiler accepts today.

## Core Rules

- one syntax for each construct
- explicit mutation through `let mut`
- explicit effects through `effects [...]`
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
- `Unit`
- named `struct`
- named `enum`
- fixed arrays `[T; N]`
- `TextBuilder` through maintained runtime builtins
- `List[T]` through maintained runtime builtins

## Maintained Stage-0 Control Flow

- `if` / `else`
- `match`
- `while`
- `repeat n`
- `repeat i in n`
- implicit tail-expression returns

## Maintained Stage-0 Runtime Builtins

- `arg_count() -> I32`
- `arg_text(index: I32) -> Text`
- `stdin_text() -> Text`
- `stdout_write(text: Text) -> Unit`
- `alloc_push() -> Unit`
- `alloc_pop() -> Unit`
- `text_builder_new() -> TextBuilder`
- `text_builder_append(builder: TextBuilder, piece: Text) -> TextBuilder`
- `text_builder_append_codepoint(builder: TextBuilder, codepoint: I32) -> TextBuilder`
- `text_builder_finish(builder: TextBuilder) -> Text`
- `list_new(len: I32, value: F64) -> List[F64]`
- `list_len(vec: List[F64]) -> I32`
- `list_get(vec: List[F64], index: I32) -> F64`
- `list_set(vec: List[F64], index: I32, value: F64) -> List[F64]`
- `f64_from_i32(value: I32) -> F64`
- `parse_i32(text: Text) -> I32`
- `parse_i32_range(text: Text, start: I32, end: I32) -> I32`
- `text_cmp(left: Text, right: Text) -> I32`
- `text_eq_range(text: Text, start: I32, end: I32, expected: Text) -> Bool`
- `sqrt(value: F64) -> F64`
- `text_from_f64_fixed(value: F64, digits: I32) -> Text`

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
