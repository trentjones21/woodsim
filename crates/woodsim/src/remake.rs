//! Wood made again with one saw pass made another way, the way a CAD program rebuilds a part
//! when a feature in its history changes: every board whose history goes through the pass is
//! cut again from the wood that pass cut, with the saw set the new way, and every pass after it
//! made again as it was, on what that leaves. Glue-ups with such boards in them go back
//! together the way they were put together (`Assembly::rebuilt`).
//!
//! Each pass keeps its number, so it still sorts where it was made, and stays one pass for
//! every board it cut: wood remade with the same `Remake` shares what it shared.

use std::{fmt, sync::Arc};

use crate::{
    Assembly, Board, Lie, Pass, Tool,
    history::{Operation, Origin, Side, Source},
};

/// Why wood can't be made again with the pass changed.
#[derive(Clone, Debug, PartialEq)]
pub enum Refusal {
    /// The pass `number` went through a glue-up, whose boards' places in it aren't kept, so it
    /// can't be made again.
    GlueUpPass(u64),
    /// The pass `number`, made on the wood as it comes out now, doesn't leave what it left.
    Pass(u64, String),
    /// A glue-up won't go back together.
    Joint(String),
}

impl fmt::Display for Refusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Refusal::GlueUpPass(_) => write!(f, "a later cut went through a glue-up"),
            Refusal::Pass(_, why) => write!(f, "{why}"),
            Refusal::Joint(why) => write!(f, "the glue-up won't go back together: {why}"),
        }
    }
}

/// Wood made again with `pass` made with `tool` instead.
pub struct Remake {
    pass: Arc<Operation>,
    tool: Tool,
    /// Each origin seen, and the board it makes now, if the pass is in its history.
    origins: Vec<(Origin, Option<Board>)>,
    operations: Vec<(Arc<Operation>, Arc<Operation>)>,
}

impl Remake {
    /// Ready to make wood again with `pass` made with `tool`: the same saw, set another way.
    pub fn new(pass: &Arc<Operation>, tool: Tool) -> Remake {
        Remake {
            pass: Arc::clone(pass),
            tool,
            origins: Vec::new(),
            operations: Vec::new(),
        }
    }

    /// The pass being changed.
    pub fn pass(&self) -> &Arc<Operation> {
        &self.pass
    }

    /// The saw, set as the pass is made now.
    pub fn tool(&self) -> Tool {
        self.tool
    }

    /// `board` made again: `None` if the pass isn't in its history, so it's as it was.
    pub fn board(&mut self, board: &Board) -> Result<Option<Board>, Refusal> {
        self.origin(board.origin())
    }

    /// `assembly` made again, its boards made again and put back together: `None` if the pass
    /// isn't in any of its boards' histories.
    pub fn assembly(&mut self, assembly: &Assembly) -> Result<Option<Assembly>, Refusal> {
        let mut changed = false;
        let mut boards = Vec::new();
        for member in assembly.members() {
            let made = self.board(&member.board)?;
            changed |= made.is_some();
            boards.push(made.unwrap_or_else(|| member.board.clone()));
        }
        if !changed {
            return Ok(None);
        }
        assembly.rebuilt(boards).map(Some).map_err(Refusal::Joint)
    }

    fn origin(&mut self, origin: &Origin) -> Result<Option<Board>, Refusal> {
        if let Some((_, made)) = self.origins.iter().find(|(seen, _)| seen.same(origin)) {
            return Ok(made.clone());
        }
        let made = self.make(origin)?;
        self.origins.push((origin.clone(), made.clone()));
        Ok(made)
    }

    fn make(&mut self, origin: &Origin) -> Result<Option<Board>, Refusal> {
        let Source::Sawn {
            operation,
            from,
            at,
            side,
            ..
        } = origin.source()
        else {
            return Ok(None);
        };
        let changed = Arc::ptr_eq(operation, &self.pass);
        let cut = self.origin(from)?;
        if !changed && cut.is_none() {
            return Ok(None);
        }
        let was = || from.replay().with_origin(from.clone());
        let board = cut.unwrap_or_else(was);
        let tool = if changed { self.tool } else { operation.tool };
        let refuse = |why: &str| Refusal::Pass(operation.number, why.to_string());
        let pass = match (tool, operation.tool) {
            (
                Tool::TableSaw {
                    lie: Lie::GlueUp(_),
                    ..
                }
                | Tool::MiterSaw {
                    lie: Lie::GlueUp(_),
                    ..
                }
                | Tool::Jointer {
                    lie: Lie::GlueUp(_),
                    ..
                }
                | Tool::Planer {
                    lie: Lie::GlueUp(_),
                    ..
                },
                _,
            ) => return Err(Refusal::GlueUpPass(operation.number)),
            (
                Tool::TableSaw {
                    saw,
                    lie: Lie::Board(lie),
                    travel: (start, end),
                },
                Tool::TableSaw { saw: before, .. },
            ) => {
                // A pass that went all the way through still does, however far that is now.
                let through = end >= before.reach(&was(), lie) + before.blade.radius();
                let end = match through {
                    true => end.max(saw.reach(&board, lie) + saw.blade.radius()),
                    false => end,
                };
                saw.feed(&board, lie, start, end)
            }
            (
                Tool::MiterSaw {
                    saw,
                    lie: Lie::Board(lie),
                    length,
                    bottom,
                },
                Tool::MiterSaw { .. },
            ) => saw.chop(&board, lie, length, bottom),
            (
                Tool::Jointer {
                    jointer,
                    lie: Lie::Board(lie),
                    travel: (start, end),
                },
                Tool::Jointer {
                    jointer: before, ..
                },
            ) => {
                // A pass that went all the way across still does, however far that is now.
                let across = end >= before.reach(&was(), lie) + before.radius();
                let end = match across {
                    true => end.max(jointer.reach(&board, lie) + jointer.radius()),
                    false => end,
                };
                jointer.feed(&board, lie, start, end)
            }
            (
                Tool::Planer {
                    planer,
                    lie: Lie::Board(lie),
                    travel: (start, end),
                },
                Tool::Planer { planer: before, .. },
            ) => {
                // A pass that went all the way through still does, however far that is now.
                let through = end >= before.reach(&was(), lie) + before.radius();
                let end = match through {
                    true => end.max(planer.reach(&board, lie) + planer.radius()),
                    false => end,
                };
                planer.feed(&board, lie, start, end)
            }
            _ => return Err(refuse("it's a different saw")),
        }
        .map_err(|error| Refusal::Pass(operation.number, error.to_string()))?;
        let piece = match (side, pass) {
            (_, Pass::Missed) => return Err(refuse("the blade misses it")),
            (Side::Whole, Pass::Kerfed(board)) => board,
            (Side::Whole, Pass::Cut(_)) => {
                return Err(refuse("it cuts it apart, where it only cut a slot"));
            }
            (_, Pass::Kerfed(_)) => return Err(refuse("it doesn't cut it apart")),
            (Side::Measured, Pass::Cut(cut)) => cut.piece.board,
            (Side::Offcut, Pass::Cut(cut)) => match cut.offcut {
                Some(offcut) => offcut.board,
                // Nothing on the measured side, the piece is what's on the other: a trim.
                None if cut.piece.board.origin().side() == Some(Side::Offcut) => cut.piece.board,
                None => return Err(refuse("it leaves no offcut")),
            },
            (Side::Loose, Pass::Cut(cut)) => {
                let off = |piece: &crate::Piece| {
                    [piece.l - at[0], piece.w - at[1], piece.t - at[2]]
                        .map(|d| (d.as_nm() as f64).powi(2))
                        .iter()
                        .sum::<f64>()
                };
                match cut
                    .loose
                    .into_iter()
                    .min_by(|a, b| off(a).total_cmp(&off(b)))
                {
                    Some(strip) => strip.board,
                    None => return Err(refuse("it sets no strip loose")),
                }
            }
        };
        let Source::Sawn { kerf, part, at, .. } = piece.origin().source() else {
            unreachable!("a saw cut it");
        };
        let (kerf, part, at) = (*kerf, *part, *at);
        let operation = self.operation(operation, tool);
        let origin = Origin::sawn(&operation, board.origin(), kerf, part, at, *side);
        Ok(Some(piece.with_origin(origin)))
    }

    /// The pass `operation` as it's made now: its number kept, and the same for every board it
    /// cut.
    fn operation(&mut self, operation: &Arc<Operation>, tool: Tool) -> Arc<Operation> {
        if let Some((_, made)) = self
            .operations
            .iter()
            .find(|(seen, _)| Arc::ptr_eq(seen, operation))
        {
            return Arc::clone(made);
        }
        let made = Arc::new(Operation {
            number: operation.number,
            tool,
        });
        self.operations
            .push((Arc::clone(operation), Arc::clone(&made)));
        made
    }
}
