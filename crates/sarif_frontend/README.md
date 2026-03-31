# `sarif_frontend`

Owns HIR lowering, semantic analysis, diagnostics, effect checks, and ownership checks.

Rules:

- semantic authority lives here
- diagnostics should stay mechanical and actionable
- backend-specific lowering does not belong here
