# Runtime

`runtime/` contains the small C runtime linked into native Sarif executables.

Current responsibilities:

- record allocation and allocation scopes
- text and text-builder helpers
- list helpers
- argument and stdin plumbing
- direct stdout write support

This directory exists to keep the native runtime explicit, auditable, and small. Backend-specific policy should stay in Rust; shared runtime primitives belong here.
