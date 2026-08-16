//! Read-only diagnostics over injected observations.

mod machine;
mod model;
mod render;

pub use machine::{Effects, Error, Machine, run};
pub use model::{Area, Diagnostic, Observation, Report, Status};
