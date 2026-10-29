//! The woodworking simulation behind Woodshop Simulator.
//!
//! This crate is the source of truth for every board's real dimensions and for what
//! each tool does to them. It has no engine dependency: the game only draws what
//! this crate reports, so everything here can be tested with plain `cargo test`.

mod angle;
mod assembly;
mod board;
mod face;
mod glue_up;
mod history;
mod kerf;
mod length;
mod orientation;
mod saw;

pub use angle::Angle;
pub use assembly::{
    Assembly, Contact, Extent, Fastener, Joint, Mating, Member, Motion, Rotation, Surface, outline,
    surfaces,
};
pub use board::{Board, Piece, Run, SlotSize};
pub use face::{Direction, Edge, Region};
pub use glue_up::{GlueUpCut, GlueUpPass};
pub use history::{Lie, Operation, Origin, Part, Side, Source, Tool};
pub use kerf::{Across, Kerf};
pub use length::Length;
pub use orientation::{Axis, Orientation};
pub use saw::{Blade, Cut, CutError, MAX_GAUGE, MiterSaw, Pass, TableSaw};
