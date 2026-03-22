# Sarif Semantic Docs


## struct Greeting

- ownership: `contains affine fields`
- rt status: `blocked in rt`

## fn add

- signature: `fn add(left: I32, right: I32) -> I32`
- ownership: `consumes affine arguments`
- rt status: `profile-compatible`

## fn main

- signature: `fn main() -> I32 effects [io]`
- ownership: `affine-safe in stage-0`
- rt status: `blocked in rt`


