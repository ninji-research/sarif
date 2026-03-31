# `sarifc`

`sarifc` is the maintained Sarif CLI.

Primary commands:

- `format`
- `check`
- `run`
- `build`
- `doc`

Retained bootstrap commands:

- `bootstrap-format`
- `bootstrap-check`
- `bootstrap-doc`

This crate owns the external CLI contract. Backend behavior belongs in the library crates, not here.
