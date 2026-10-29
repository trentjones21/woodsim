//! Where a board came from, so a project made here can be made again in a real shop the way it
//! was made: every board is either new lumber or a piece of another board a saw cut, with the
//! saw's every setting kept with the cut. The pieces of one cut share it, so the boards in a
//! project trace back through the cuts between them to the stock they came from, and each cut,
//! and each joint, has a number saying when it happened. Making the cuts again from the stock
//! makes the board again, exactly (`Origin::replay`).
//!
//! Where a board came from doesn't change what it is: two boards the same size and shape are
//! equal however they were made.

use std::{
    collections::HashMap,
    fmt,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
};

use crate::{Board, Kerf, Length, MiterSaw, Orientation, Piece, Rotation, TableSaw};

static NEXT: AtomicU64 = AtomicU64::new(1);

/// The next number in the order things happen: every saw pass and every joint takes one, so a
/// later one has a bigger number.
pub(crate) fn next_number() -> u64 {
    NEXT.fetch_add(1, Ordering::Relaxed)
}

/// Notes that `number` has been used, by wood read back from a recipe, so everything after it
/// comes later, even after the game starts again.
pub(crate) fn number_used(number: u64) {
    NEXT.fetch_max(number + 1, Ordering::Relaxed);
}

/// Where a board came from. Cloning a board keeps it, since it's the same wood; the pieces of
/// one cut share the cut, and where the board they were cut from came from.
#[derive(Clone)]
pub struct Origin(Arc<Source>);

#[derive(Debug)]
pub enum Source {
    /// New lumber, this size.
    Stock {
        length: Length,
        width: Length,
        thickness: Length,
    },
    /// Sawn from the board `from` says: `operation` cut `kerf` into it, which left it in one
    /// piece with a slot in it, or split it, and this is one of the pieces, with its corner at
    /// `at` on that board, along its length, width, and thickness, on the `side` of the cut it
    /// was.
    Sawn {
        operation: Arc<Operation>,
        from: Origin,
        kerf: Kerf,
        part: Part,
        at: [Length; 3],
        side: Side,
    },
}

/// Which side of a cut a piece was on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Side {
    /// The board with a slot in it, still in one piece.
    Whole,
    /// The measured piece: against the fence, or the gauge's or the miter saw's stop, the size
    /// the saw was set to.
    Measured,
    /// What was on the blade's other side.
    Offcut,
    /// A strip a slot already in the board set loose.
    Loose,
}

/// Which of what a cut left a board is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Part {
    /// The board it was cut into, still in one piece, with the slot in it.
    Whole,
    /// The `n`th of the pieces it was cut apart into, in the order `Board::pieces` gives them.
    Piece(usize),
}

/// A pass of a saw: when it happened, the saw as it was set, and how the wood went through it.
#[derive(Clone, Debug, PartialEq)]
pub struct Operation {
    pub number: u64,
    pub tool: Tool,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Tool {
    /// Fed past a table saw's blade, its center traveling from the first length to the second
    /// along the wood, from its leading end.
    TableSaw {
        saw: TableSaw,
        lie: Lie,
        travel: (Length, Length),
    },
    /// Chopped by a miter saw, `length` from the stop, the blade's lowest point coming down to
    /// `bottom` above the table.
    MiterSaw {
        saw: MiterSaw,
        lie: Lie,
        length: Length,
        bottom: Length,
    },
}

/// How the wood lay on the saw: a board as its `Orientation` says, or a glue-up turned square
/// onto the saw's axes.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Lie {
    Board(Orientation),
    GlueUp(Rotation),
}

impl Operation {
    /// A pass of `tool`, numbered now.
    pub(crate) fn new(tool: Tool) -> Arc<Operation> {
        Arc::new(Operation {
            number: next_number(),
            tool,
        })
    }
}

impl Origin {
    /// New lumber, this size.
    pub(crate) fn new_stock(length: Length, width: Length, thickness: Length) -> Origin {
        Origin(Arc::new(Source::Stock {
            length,
            width,
            thickness,
        }))
    }

    /// Sawn from `from`, as `Source::Sawn` says.
    pub(crate) fn sawn(
        operation: &Arc<Operation>,
        from: &Origin,
        kerf: Kerf,
        part: Part,
        at: [Length; 3],
        side: Side,
    ) -> Origin {
        Origin(Arc::new(Source::Sawn {
            operation: Arc::clone(operation),
            from: from.clone(),
            kerf,
            part,
            at,
            side,
        }))
    }

    /// The same, but on `side` of the cut: once the saw has sorted out which piece is which.
    pub(crate) fn on_side(&self, side: Side) -> Origin {
        match self.source() {
            Source::Stock { .. } => self.clone(),
            Source::Sawn {
                operation,
                from,
                kerf,
                part,
                at,
                ..
            } => Origin::sawn(operation, from, *kerf, *part, *at, side),
        }
    }

    /// Which side of the cut that made it the board was on, if a cut made it.
    pub fn side(&self) -> Option<Side> {
        match self.source() {
            Source::Stock { .. } => None,
            Source::Sawn { side, .. } => Some(*side),
        }
    }

    pub fn source(&self) -> &Source {
        &self.0
    }

    /// Whether `other` is the very same: the same new board, or the same part of the same cut
    /// of it. Two boards cut the same way from two pieces of stock aren't.
    pub fn same(&self, other: &Origin) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }

    /// The new lumber it all started as.
    pub fn stock(&self) -> &Origin {
        match self.source() {
            Source::Stock { .. } => self,
            Source::Sawn { from, .. } => from.stock(),
        }
    }

    /// The saw passes it went through, from the stock on, oldest first.
    pub fn operations(&self) -> Vec<Arc<Operation>> {
        match self.source() {
            Source::Stock { .. } => Vec::new(),
            Source::Sawn {
                operation, from, ..
            } => {
                let mut operations = from.operations();
                operations.push(Arc::clone(operation));
                operations
            }
        }
    }

    /// Where the board's corner is on the stock it came from, along the stock's length, width,
    /// and thickness: a board's axes never turn as it's cut, so it's where each piece was on
    /// the board it was cut from, added up.
    pub fn at_on_stock(&self) -> [Length; 3] {
        match self.source() {
            Source::Stock { .. } => [Length::ZERO; 3],
            Source::Sawn { from, at, .. } => {
                let [l, w, t] = from.at_on_stock();
                [l + at[0], w + at[1], t + at[2]]
            }
        }
    }

    /// The board the cut that made it cut, and everything that cut left of it, each where it
    /// was on that board: the pieces it was cut apart into, this one among them, in the order
    /// `Part::Piece` counts them, or the board with the slot in it, if it held together. `None`
    /// for new lumber.
    pub fn cut_from(&self) -> Option<(Board, Vec<Piece>)> {
        let Source::Sawn {
            from, kerf, part, ..
        } = self.source()
        else {
            return None;
        };
        let board = from.replay();
        let kerfed = board.with_kerf(*kerf);
        let pieces = match part {
            Part::Whole => vec![Piece::whole(kerfed)],
            Part::Piece(_) => kerfed.pieces(),
        };
        Some((board, pieces))
    }

    /// The board, made again: the new lumber it started as, cut as it was cut. It comes out
    /// exactly the same.
    pub fn replay(&self) -> Board {
        match self.source() {
            Source::Stock {
                length,
                width,
                thickness,
            } => Board::new(*length, *width, *thickness),
            Source::Sawn {
                from, kerf, part, ..
            } => {
                let kerfed = from.replay().with_kerf(*kerf);
                match part {
                    Part::Whole => kerfed,
                    Part::Piece(n) => {
                        let mut pieces = kerfed.pieces();
                        assert!(
                            *n < pieces.len(),
                            "the cut left only {} pieces",
                            pieces.len()
                        );
                        pieces.swap_remove(*n).board
                    }
                }
            }
        }
    }
}

/// Histories copied afresh, for wood made again the same way from new lumber of its own: each
/// copy starts from stock of its own and goes through passes of its own, shared wherever the
/// originals were, and every pass and joint gets a new number, after everything so far, in the
/// order the originals had.
pub(crate) struct Copies {
    numbers: HashMap<u64, u64>,
    origins: Vec<(Origin, Origin)>,
    operations: Vec<(Arc<Operation>, Arc<Operation>)>,
}

impl Copies {
    /// Ready to copy histories whose passes and joints are numbered `numbers`.
    pub(crate) fn numbering(numbers: impl IntoIterator<Item = u64>) -> Copies {
        let mut numbers: Vec<u64> = numbers.into_iter().collect();
        numbers.sort_unstable();
        numbers.dedup();
        Copies {
            numbers: numbers
                .into_iter()
                .map(|number| (number, next_number()))
                .collect(),
            origins: Vec::new(),
            operations: Vec::new(),
        }
    }

    /// The new number for what was numbered `number`.
    pub(crate) fn number(&self, number: u64) -> u64 {
        self.numbers[&number]
    }

    /// The copy of `origin`: the same copy for the same origin, every time.
    pub(crate) fn origin(&mut self, origin: &Origin) -> Origin {
        if let Some((_, copy)) = self.origins.iter().find(|(seen, _)| seen.same(origin)) {
            return copy.clone();
        }
        let copy = match origin.source() {
            Source::Stock {
                length,
                width,
                thickness,
            } => Origin::new_stock(*length, *width, *thickness),
            Source::Sawn {
                operation,
                from,
                kerf,
                part,
                at,
                side,
            } => {
                let from = self.origin(from);
                let operation = self.operation(operation);
                Origin::sawn(&operation, &from, *kerf, *part, *at, *side)
            }
        };
        self.origins.push((origin.clone(), copy.clone()));
        copy
    }

    fn operation(&mut self, operation: &Arc<Operation>) -> Arc<Operation> {
        if let Some((_, copy)) = self
            .operations
            .iter()
            .find(|(seen, _)| Arc::ptr_eq(seen, operation))
        {
            return Arc::clone(copy);
        }
        let copy = Arc::new(Operation {
            number: self.number(operation.number),
            tool: operation.tool,
        });
        self.operations
            .push((Arc::clone(operation), Arc::clone(&copy)));
        copy
    }
}

/// Says where it came from in a line, back to the stock: "piece 1 of pass #12, from new 3/4" x
/// 1-1/2" x 48"".
impl fmt::Debug for Origin {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.source() {
            Source::Stock {
                length,
                width,
                thickness,
            } => write!(f, "new {thickness} x {width} x {length}"),
            Source::Sawn {
                operation,
                from,
                part,
                side,
                ..
            } => {
                match part {
                    Part::Whole => write!(f, "slotted by pass #{}", operation.number)?,
                    Part::Piece(n) => {
                        write!(f, "piece {n} ({side:?}) of pass #{}", operation.number)?
                    }
                }
                write!(f, ", from {from:?}")
            }
        }
    }
}
