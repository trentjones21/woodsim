//! The woodworking simulation behind Woodshop Simulator.
//!
//! This crate is the source of truth for every board's real dimensions and for what
//! each tool does to them. It has no engine dependency: the game only draws what
//! this crate reports, so everything here can be tested with plain `cargo test`.

mod angle;
mod board;
mod face;
mod kerf;
mod length;
mod orientation;
mod saw;

pub use angle::Angle;
pub use board::{Board, Piece, Run, SlotSize};
pub use face::{Direction, Edge, Region};
pub use kerf::Kerf;
pub use length::Length;
pub use orientation::{Axis, Orientation};
pub use saw::{Blade, Cut, CutError, MiterSaw, Pass, TableSaw};
