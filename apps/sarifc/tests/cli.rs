#[cfg(feature = "docs")]
#[path = "cli/artifacts.rs"]
mod artifacts;
#[cfg(feature = "codegen")]
#[path = "cli/execution.rs"]
mod execution;
#[cfg(feature = "format")]
#[path = "cli/formatting.rs"]
mod formatting;
#[path = "cli/support.rs"]
mod support;
#[path = "cli/validation.rs"]
mod validation;
