//! Wood made again with one saw pass made another way, the way a CAD program rebuilds a part
//! when a feature in its history changes: every board whose history goes through the pass is
//! cut again from the wood that pass cut, with the saw set the new way, and every pass after it
//! made again as it was, on what that leaves. Each pass keeps the wood it went through
//! (`Operation::wood`), a board or a glue-up alike, so a pass is made again on that wood made
//! again: through a glue-up, its boards are put back together first, then cut. Glue-ups with
//! such boards in them go back together the way they were put together (`Assembly::rebuilt`).
//!
//! Each pass keeps its number, so it still sorts where it was made, and stays one pass for
//! every board it cut: wood remade with the same `Remake` shares what it shared.

use std::{fmt, sync::Arc};

use crate::{
    Assembly, Board, GlueUpPass, Length, Tool,
    glue_up::{Laid, Traced},
    history::{Operation, Origin, Side, Source},
};

/// Why wood can't be made again with the pass changed.
#[derive(Clone, Debug, PartialEq)]
pub enum Refusal {
    /// The pass `number` went through a glue-up read from a recipe written before passes kept
    /// the wood they went through, so it can't be made again.
    GlueUpPass(u64),
    /// The pass `number`, made on the wood as it comes out now, doesn't leave what it left.
    Pass(u64, String),
    /// A glue-up won't go back together.
    Joint(String),
}

impl fmt::Display for Refusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Refusal::GlueUpPass(_) => write!(
                f,
                "a later cut went through a glue-up saved before cuts kept what they cut"
            ),
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
    /// Each pass seen, and how it's made now, if it's the pass or went through wood the pass is
    /// in the history of.
    passes: Vec<(Arc<Operation>, Option<Arc<Made>>)>,
}

/// A pass made again: numbered as it was, through the wood as it comes out now, and what it
/// leaves; and what it left, made again as it was.
struct Made {
    operation: Arc<Operation>,
    traced: Traced,
    was: Traced,
}

impl Remake {
    /// Ready to make wood again with `pass` made with `tool`: the same saw, set another way.
    pub fn new(pass: &Arc<Operation>, tool: Tool) -> Remake {
        Remake {
            pass: Arc::clone(pass),
            tool,
            origins: Vec::new(),
            passes: Vec::new(),
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

    /// The board `origin` makes now: the pass that cut it made again, and of what that leaves
    /// of the board it was cut from, the piece on the same side of the cut, nearest where it
    /// was.
    fn make(&mut self, origin: &Origin) -> Result<Option<Board>, Refusal> {
        let Source::Sawn {
            operation,
            from,
            part,
            side,
            ..
        } = origin.source()
        else {
            return Ok(None);
        };
        let Some(wood) = &operation.wood else {
            // What else the pass went through isn't kept.
            if !Arc::ptr_eq(operation, &self.pass) && self.origin(from)?.is_none() {
                return Ok(None);
            }
            return Err(Refusal::GlueUpPass(operation.number));
        };
        let Some(made) = self.made(operation, wood)? else {
            return Ok(None);
        };
        // What the pass left of it, made again as it was, and which board on the saw that came
        // from: boards made alike can share a history, but not the slot the pass cut in each.
        let refuse = |why: &str| Refusal::Pass(operation.number, why.to_string());
        let Some((was, member)) =
            cut_boards(&made.was).find(|(board, _)| same_cut(board.origin(), origin))
        else {
            return Err(refuse(
                "it doesn't come out as it did, made again as it was",
            ));
        };
        let pass = &made.traced;
        // Of everything the pass leaves of that board now, the piece on the same side of the cut,
        // where it was: its box nearest the one it had, then the same of the pieces.
        let pieces: Vec<&Board> = cut_boards(pass)
            .filter(|&(_, from)| from == member)
            .map(|(board, _)| board)
            .collect();
        let off = |board: &&Board| {
            let (a, b) = (reach(board), reach(was));
            (0..6).map(|k| (a[k] - b[k]).powi(2)).sum::<f64>()
        };
        let other = |board: &&Board| match board.origin().source() {
            Source::Sawn { part: other, .. } => other != part,
            Source::Stock { .. } => true,
        };
        let piece = pieces
            .iter()
            .filter(|board| board.origin().side() == Some(*side))
            .min_by(|a, b| off(a).total_cmp(&off(b)).then(other(a).cmp(&other(b))));
        let Some(piece) = piece else {
            let whole = pieces
                .iter()
                .any(|board| board.origin().side() == Some(Side::Whole));
            let offcut = matches!(&pass.pass, GlueUpPass::Cut(cut) if cut.offcut.is_some());
            let why = match side {
                _ if pieces.is_empty() => "the blade misses it",
                Side::Whole => "it cuts it apart, where it only cut a slot",
                _ if whole => "it doesn't cut it apart",
                Side::Offcut if !offcut => "it leaves no offcut",
                Side::Loose => "it sets no strip loose",
                _ => "it leaves it on the other side of the cut",
            };
            return Err(refuse(why));
        };
        let Source::Sawn {
            from,
            kerf,
            part,
            at,
            ..
        } = piece.origin().source()
        else {
            unreachable!("the pass cut it");
        };
        let origin = Origin::sawn(&made.operation, from, *kerf, *part, *at, *side);
        Ok(Some((*piece).clone().with_origin(origin)))
    }

    /// The pass `operation` through `wood` as it's made now, the same for every board it cut:
    /// `None` if it's as it was.
    fn made(
        &mut self,
        operation: &Arc<Operation>,
        wood: &Assembly,
    ) -> Result<Option<Arc<Made>>, Refusal> {
        if let Some((_, made)) = self
            .passes
            .iter()
            .find(|(seen, _)| Arc::ptr_eq(seen, operation))
        {
            return Ok(made.clone());
        }
        let made = self.make_pass(operation, wood)?.map(Arc::new);
        self.passes.push((Arc::clone(operation), made.clone()));
        Ok(made)
    }

    /// The pass `operation` made again: with the saw set the new way if it's the pass being
    /// changed, through `wood` made again, keeping its number.
    fn make_pass(
        &mut self,
        operation: &Arc<Operation>,
        wood: &Assembly,
    ) -> Result<Option<Made>, Refusal> {
        let changed = Arc::ptr_eq(operation, &self.pass);
        let remade = self.assembly(wood)?;
        if !changed && remade.is_none() {
            return Ok(None);
        }
        let now = remade.unwrap_or_else(|| wood.clone());
        let refuse = |why: &str| Refusal::Pass(operation.number, why.to_string());
        let mut tool = if changed { self.tool } else { operation.tool };
        if std::mem::discriminant(&tool) != std::mem::discriminant(&operation.tool) {
            return Err(refuse("it's a different saw"));
        }
        // A pass that went all the way through still does, however far that is now.
        if let (Some((start, end)), Some(was), Some(through)) = (
            travel(tool),
            through(operation.tool, wood),
            through(tool, &now),
        ) && end >= was
        {
            tool = traveling(tool, (start, end.max(through)));
        }
        let traced = cut(tool, &now).map_err(|error| refuse(&error.to_string()))?;
        if matches!(traced.pass, GlueUpPass::Missed) {
            return Err(refuse("the blade misses it"));
        }
        // What it left, made again as it was, to find each board it cut in.
        let was = cut(operation.tool, wood).map_err(|error| refuse(&error.to_string()))?;
        // Through a glue-up, it has to cut the boards it cut, and leave every board in the part
        // with the others it left it with, since a board the blade missed keeps its own history.
        if wood.members().len() > 1 {
            let touched = |traced: &Traced| {
                let mut cut = vec![false; wood.members().len()];
                for (_, from) in cut_boards(traced) {
                    cut[from] = true;
                }
                cut
            };
            let (before, after) = (touched(&was), touched(&traced));
            if let Some(k) = (0..before.len()).find(|&k| before[k] != after[k]) {
                return Err(refuse(match before[k] {
                    true => "it misses a board of the glue-up it cut",
                    false => "it cuts a board of the glue-up it missed",
                }));
            }
            if leaves(&was) != leaves(&traced) {
                return Err(refuse("it leaves the glue-up in other parts than it did"));
            }
        }
        let operation = Arc::new(Operation {
            number: operation.number,
            tool,
            wood: Some(now),
        });
        Ok(Some(Made {
            operation,
            traced,
            was,
        }))
    }
}

/// Whether `a` and `b` were cut the same way from the same board: the same slot, the same part
/// of what it left, there, and on the same side.
fn same_cut(a: &Origin, b: &Origin) -> bool {
    match (a.source(), b.source()) {
        (
            Source::Sawn {
                from,
                kerf,
                part,
                at,
                side,
                ..
            },
            Source::Sawn {
                from: f,
                kerf: k,
                part: p,
                at: a,
                side: s,
                ..
            },
        ) => from.same(f) && (kerf, part, at, side) == (k, p, a, s),
        _ => false,
    }
}

/// The boards `traced`'s pass cut, each with which board on the saw it was cut from.
fn cut_boards(traced: &Traced) -> impl Iterator<Item = (&Board, usize)> {
    traced
        .pass
        .parts()
        .into_iter()
        .zip(&traced.from)
        .flat_map(|(part, from)| part.members().iter().zip(from))
        .map(|(member, &from)| (&member.board, from))
        .filter(|(board, _)| cut_by(board, &traced.operation))
}

/// The box a piece fills on the board it was cut from, its near corner and its far, in
/// nanometers.
fn reach(board: &Board) -> [f64; 6] {
    let Source::Sawn { at, .. } = board.origin().source() else {
        unreachable!("a pass cut it");
    };
    let size = [board.length(), board.width(), board.thickness()];
    let near = at.map(|length| length.as_nm() as f64);
    [0, 1, 2, 3, 4, 5].map(|k| match k {
        0..3 => near[k],
        _ => near[k - 3] + size[k - 3].as_nm() as f64,
    })
}

/// Whether `operation` cut `board`.
fn cut_by(board: &Board, operation: &Arc<Operation>) -> bool {
    matches!(board.origin().source(), Source::Sawn { operation: by, .. } if Arc::ptr_eq(by, operation))
}

/// What a pass left, as the parts it left, each as which board on the saw its boards came from
/// and which side of the cut each is on (or none, if the blade missed it), sorted.
fn leaves(traced: &Traced) -> Vec<Vec<(usize, Option<u8>)>> {
    let mut parts: Vec<Vec<(usize, Option<u8>)>> = traced
        .pass
        .parts()
        .into_iter()
        .zip(&traced.from)
        .map(|(part, from)| {
            let mut part: Vec<_> = part
                .members()
                .iter()
                .zip(from)
                .map(|(member, &from)| {
                    let board = &member.board;
                    let side = cut_by(board, &traced.operation)
                        .then(|| board.origin().side().map(|side| side as u8))
                        .flatten();
                    (from, side)
                })
                .collect();
            part.sort_unstable();
            part
        })
        .collect();
    parts.sort_unstable();
    parts
}

/// A pass of `tool` through `wood`, lying on the saw as the pass has it.
fn cut(tool: Tool, wood: &Assembly) -> Result<Traced, crate::CutError> {
    match tool {
        Tool::TableSaw { saw, lie, travel } => {
            let laid = Laid::new(wood, lie.rotation());
            laid.traced(&saw.sweep(&laid, lie, travel)?.0)
        }
        Tool::MiterSaw {
            saw,
            lie,
            length,
            bottom,
        } => {
            let laid = Laid::new(wood, lie.rotation());
            laid.traced(&saw.sweep(&laid, tool, length, bottom)?.0)
        }
        Tool::Jointer {
            jointer,
            lie,
            travel,
        } => {
            let laid = Laid::new(wood, jointer.leaned(lie.rotation()));
            laid.traced(&jointer.sweep(&laid, lie, travel)?)
        }
        Tool::Planer {
            planer,
            lie,
            travel,
        } => {
            let laid = Laid::new(wood, lie.rotation());
            laid.traced(&planer.sweep(&laid, lie, travel)?)
        }
    }
}

/// How far a pass of `tool` travels to go all the way through `wood`: as far as the wood
/// reaches along its travel, and the blade's radius. `None` for the miter saw, which chops.
fn through(tool: Tool, wood: &Assembly) -> Option<Length> {
    Some(match tool {
        Tool::TableSaw { saw, lie, .. } => {
            saw.reach_of(&Laid::new(wood, lie.rotation())) + saw.blade.radius()
        }
        Tool::MiterSaw { .. } => return None,
        Tool::Jointer { jointer, lie, .. } => {
            Laid::new(wood, jointer.leaned(lie.rotation())).extent[0] + jointer.radius()
        }
        Tool::Planer { planer, lie, .. } => {
            Laid::new(wood, lie.rotation()).extent[0] + planer.radius()
        }
    })
}

/// How far a pass of `tool` traveled, from and to, if it's fed rather than chopped.
fn travel(tool: Tool) -> Option<(Length, Length)> {
    match tool {
        Tool::TableSaw { travel, .. }
        | Tool::Jointer { travel, .. }
        | Tool::Planer { travel, .. } => Some(travel),
        Tool::MiterSaw { .. } => None,
    }
}

/// `tool`, fed `to` instead.
fn traveling(tool: Tool, to: (Length, Length)) -> Tool {
    match tool {
        Tool::TableSaw { saw, lie, .. } => Tool::TableSaw {
            saw,
            lie,
            travel: to,
        },
        Tool::Jointer { jointer, lie, .. } => Tool::Jointer {
            jointer,
            lie,
            travel: to,
        },
        Tool::Planer { planer, lie, .. } => Tool::Planer {
            planer,
            lie,
            travel: to,
        },
        Tool::MiterSaw { .. } => tool,
    }
}
