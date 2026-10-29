//! Plans to print, drawn up from an assembly: a web page with a drawing of the whole thing, the
//! lumber to buy, a cut list, where each part comes out of the lumber, a drawing of each part,
//! and the steps to build it (`plan`); the cutting half of them, from where each part came from
//! (`cutting`); a picture of each cut on its saw (`setup`); and wood put into words (`words`),
//! which the game's statuses use too. It's plain Rust on woodsim, wood in and a page out, with
//! no Bevy and nothing that needs a desktop, so the game and a web server can both draw them up.

pub mod cutting;
pub mod plan;
mod setup;
mod words;

pub use cutting::LUMBER;
pub use plan::Plan;
pub use words::{
    describe_bevels, describe_board, describe_cut, describe_kerf, describe_miter, describe_miters,
    describe_size, describe_tapers, measured, name,
};
