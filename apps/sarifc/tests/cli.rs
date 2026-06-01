#[path = "cli/artifacts.rs"]
mod artifacts;
#[cfg(feature = "c-backend")]
#[path = "cli/c_backend.rs"]
mod c_backend;
#[cfg(feature = "codegen")]
#[path = "cli/execution.rs"]
mod execution;
#[path = "cli/formatting.rs"]
mod formatting;
#[path = "cli/support.rs"]
mod support;
#[path = "cli/validation.rs"]
mod validation;
