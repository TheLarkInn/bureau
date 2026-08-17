//! The `bureau watch` view-model: a read-only projection of local state.
//!
//! The adopted config, state.db budget counters, and run directories
//! become one screen-sized frame. Every read is tolerant — degradation
//! is a note on the frame, never a panic. Terminal I/O lives in the
//! binary; everything here is pure data plus offline reads.

mod load;
mod model;
mod render;

pub use load::{Roots, load};
pub use model::{BudgetRow, Frame, Header, RunRow, resolve_selection};
pub use render::{age_text, clock_text, money, render_plain};
