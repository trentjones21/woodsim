//! The woodworking simulation behind Woodshop Simulator.
//!
//! This crate is the source of truth for every board's real dimensions and for what
//! each tool does to them. It has no engine dependency: the game only draws what
//! this crate reports, so everything here can be tested with plain `cargo test`.

mod angle;
mod assembly;
mod board;
mod exposed;
mod face;
mod glue_up;
mod history;
mod jointer;
mod kerf;
mod length;
mod orientation;
mod recipe;
mod remake;
mod saw;
mod solid;
mod wide;

pub use angle::Angle;
pub use assembly::{
    Assembly, Contact, Extent, Fastener, Joint, Mating, Member, Motion, Rotation, Surface,
    Unsteady, outline, same_surface, surfaces,
};
pub use board::{Board, Piece, Run, SlotSize};
pub use exposed::{exposed, slot_bottoms, slot_surfaces};
pub use face::{Direction, Edge, Region, Vector};
pub use glue_up::{GlueUpCut, GlueUpPass};
pub use history::{Lie, Operation, Origin, Part, Side, Source, Tool};
pub use jointer::{Jointer, MAX_FENCE_TILT};
pub use kerf::{Across, Kerf, Opening};
pub use length::Length;
pub use orientation::{Axis, Orientation};
pub use recipe::{Recipe, Wood};
pub use remake::{Refusal, Remake};
pub use saw::{Blade, Cut, CutError, MAX_GAUGE, MAX_TILT, MiterSaw, Pass, TableSaw};
