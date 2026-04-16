# Sarif Platform Matrix

This document records the real current platform contract for Sarif stage-0 artifacts.

## Current Maintained Targets

| Target | Status | Notes |
| --- | --- | --- |
| Linux native executable | maintained | Primary stage-0 deployment path. Requires a working C toolchain and linker on the host. |
| macOS native executable | feasible, lightly exercised | The native runtime is POSIX/C-oriented and the linker path now uses Mach-O dead-strip flags, but this host is not covered by the same regression volume as Linux. |
| Wasm artifact (`--target wasm`) | maintained with explicit exclusions | Emits runnable `.wasm` modules for the supported stage-0 builtin surface. Runtime-input builtins remain excluded. |

## Architecture Reality

- `x86-64`: feasible as a native host architecture
- `ARM64`: feasible as a native host architecture
- `RISC-V`: theoretically feasible through Cranelift and the C runtime model, but not maintained or exercised here yet

Current native codegen is host-native, not cross-targeted. `sarifc build --target native` lowers through `cranelift_native`, so Sarif produces native artifacts for the architecture of the machine running `sarifc`; it does not yet expose a maintained cross-compilation triple surface.

## What `SARIF_NATIVE_CPU` Changes

The native artifact path defaults to host tuning:

- `SARIF_NATIVE_CPU=native` enables `-march=native -mtune=native`
- `SARIF_NATIVE_CPU=baseline` removes those host-specific CPU flags

`baseline` is the correct choice when you want a more portable build across machines that share the same operating-system ABI and toolchain family. It is not a substitute for full cross-compilation.

## Not Maintained Yet

- Windows native executable output
- Android native executable output
- iOS native executable output
- maintained cross-compilation triples
- maintained multithreaded or async runtime deployment targets

The current native runtime and linker path are still POSIX-host oriented. Windows and mobile targets need a deliberate runtime-abstraction pass rather than ad hoc flag tweaks.
