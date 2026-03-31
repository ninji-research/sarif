# Sarif Semantic Docs


## enum Flag

- variants: `2`
- ownership: `plain tag`
- rt status: `profile-compatible`

## const answer

- type: `I32`
- value: `42`
## fn compute

- signature: `fn compute(flag: Flag) -> I32`
- ownership: `consumes affine arguments`
- rt status: `profile-compatible`

## fn main

- signature: `fn main() -> I32`
- ownership: `affine-safe in stage-0`
- rt status: `profile-compatible`


