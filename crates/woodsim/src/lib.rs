//! The woodworking simulation behind Woodshop Simulator.
//!
//! This crate is the source of truth for every board's real dimensions and for what
//! each tool does to them. It has no engine dependency: the game only draws what
//! this crate reports, so everything here can be tested with plain `cargo test`.

mod board;
mod kerf;
mod length;
mod saw;

pub use board::Board;
pub use kerf::Kerf;
pub use length::Length;
pub use saw::{Blade, Cut, CutError, MiterSaw, Pass, TableSaw};
