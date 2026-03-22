# Sarif Semantic Docs


## examples/multi-file-package/src/types.sarif

### struct Numbers

- ownership: `plain value`
- rt status: `profile-compatible`

## examples/multi-file-package/src/consts.sarif

### const bonus

- type: `I32`
- value: `2`
## examples/multi-file-package/src/functions.sarif

### fn add

- signature: `fn add(pair: Numbers) -> I32`
- ownership: `consumes affine arguments`
- rt status: `profile-compatible`

### fn main

- signature: `fn main() -> I32`
- ownership: `affine-safe in stage-0`
- rt status: `profile-compatible`


