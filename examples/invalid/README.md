# Invalid Examples

This directory contains Sarif programs that demonstrate restrictions enforced by the `Total` and `RT` profiles.

## Profile Restrictions

- **Core** (default): Base language with no additional restrictions. Programs here should compile and run normally.

- **Total**: Stricter profile that forbids potentially non-terminating constructs like `while` loops and `repeat` without compile-time-bounded iteration.

- **RT**: Stricter profile that forbids effects (I/O, allocation) and certain types (`Text`, `List`) to ensure deterministic, resource-bounded execution.

## File Naming Convention

- `total_forbidden_*`: Programs that are valid Core but rejected by the Total profile
- `rt_forbidden_*`: Programs that are valid Core but rejected by the RT profile
- `contract_*`: Programs demonstrating contract/ownership restrictions
- Other files: Programs demonstrating semantic errors (affine reuse, etc.)

## Example

```bash
# This compiles with Core profile
sarifc run examples/invalid/total_forbidden_while.sarif --profile core

# But fails with Total profile (as expected)
sarifc run examples/invalid/total_forbidden_while.sarif --profile total
```

## Running Invalid Examples

These programs are not meant to execute successfully - they demonstrate restrictions:

- `total_forbidden_while.sarif`: Contains a `while` loop (forbidden in Total)
- `total_forbidden_repeat.sarif`: Contains `repeat` without termination proof (forbidden in Total)
- `rt_forbidden_text.sarif`: Contains `Text` type (forbidden in RT)
- `rt_forbidden_effects.sarif`: Declares effects (forbidden in RT)

See `docs/language-spec.md` for more information about profiles.