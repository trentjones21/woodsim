use std::{collections::VecDeque, fmt};

use crate::{Axis, Kerf, Length};

/// A board: rectangular stock, less any slots saws have cut into it without cutting it apart,
/// and any cutaways where other pieces used to be. Its length runs along the grain.
///
/// Positions on a board are in its own coordinates: `l` along its length from one end, `w`
/// across its width from one edge, and `t` up through its thickness from its bottom face.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Board {
    length: Length,
    width: Length,
    thickness: Length,
    kerfs: Vec<Kerf>,
    cutaways: Vec<Cutaway>,
}

impl Board {
    /// Panics if any dimension is zero or negative.
    pub fn new(length: Length, width: Length, thickness: Length) -> Board {
        assert!(
            length > Length::ZERO && width > Length::ZERO && thickness > Length::ZERO,
            "board dimensions must be positive, got {thickness} x {width} x {length}"
        );
        Board {
            length,
            width,
            thickness,
            kerfs: Vec::new(),
            cutaways: Vec::new(),
        }
    }

    pub fn length(&self) -> Length {
        self.length
    }

    pub fn width(&self) -> Length {
        self.width
    }

    pub fn thickness(&self) -> Length {
        self.thickness
    }

    /// The slots cut into this board, oldest first.
    pub fn kerfs(&self) -> &[Kerf] {
        &self.kerfs
    }

    /// Where other pieces were, before this board came apart from them around a corner.
    pub fn cutaways(&self) -> &[Cutaway] {
        &self.cutaways
    }

    /// This board with one more slot cut into it.
    pub(crate) fn with_kerf(&self, kerf: Kerf) -> Board {
        let mut board = self.clone();
        board.kerfs.push(kerf);
        board
    }

    /// The separate pieces this board is in. Wherever slots, alone or together, go all the
    /// way through the board, they divide it: a slot the length of the board splits it in two,
    /// and a rip and a crosscut that meet cut out the corner between them. Wood the slots took
    /// entirely is sawdust. Each piece is trimmed to its wood, and in order by where its corner
    /// was.
    ///
    /// Only slots that go through on their own divide a board. Two slots from opposite faces
    /// whose depths together go through don't yet.
    pub(crate) fn pieces(&self) -> Vec<Piece> {
        // Rectangles of the board's face with no wood through its thickness: where a slot goes
        // through, and where other pieces were.
        let holes: Vec<Cutaway> = self
            .kerfs
            .iter()
            .filter_map(|kerf| self.through_hole(kerf))
            .chain(
                self.cutaways
                    .iter()
                    .filter_map(|cutaway| cutaway.within(self.length, self.width)),
            )
            .collect();
        if holes.is_empty() {
            return vec![Piece {
                board: self.clone(),
                l: Length::ZERO,
                w: Length::ZERO,
            }];
        }

        // A grid over the face, split at every hole's edges, so each cell is wholly in a hole
        // or wholly wood.
        let ls = grid_lines(
            self.length,
            holes.iter().flat_map(|hole| [hole.l.0, hole.l.1]),
        );
        let ws = grid_lines(
            self.width,
            holes.iter().flat_map(|hole| [hole.w.0, hole.w.1]),
        );
        let (columns, rows) = (ls.len() - 1, ws.len() - 1);
        let cell = |i: usize, j: usize| j * columns + i;
        let wood: Vec<bool> = (0..rows)
            .flat_map(|j| (0..columns).map(move |i| (i, j)))
            .map(|(i, j)| {
                !holes.iter().any(|hole| {
                    hole.l.0 <= ls[i]
                        && ls[i + 1] <= hole.l.1
                        && hole.w.0 <= ws[j]
                        && ws[j + 1] <= hole.w.1
                })
            })
            .collect();

        // Cells of wood that share an edge hold together.
        let mut piece_of: Vec<Option<usize>> = vec![None; columns * rows];
        let mut count = 0;
        for first in 0..columns * rows {
            if !wood[first] || piece_of[first].is_some() {
                continue;
            }
            piece_of[first] = Some(count);
            let mut queue = VecDeque::from([first]);
            while let Some(here) = queue.pop_front() {
                let (i, j) = (here % columns, here / columns);
                let neighbors = [
                    (i > 0).then(|| cell(i - 1, j)),
                    (i + 1 < columns).then(|| cell(i + 1, j)),
                    (j > 0).then(|| cell(i, j - 1)),
                    (j + 1 < rows).then(|| cell(i, j + 1)),
                ];
                for next in neighbors.into_iter().flatten() {
                    if wood[next] && piece_of[next].is_none() {
                        piece_of[next] = Some(count);
                        queue.push_back(next);
                    }
                }
            }
            count += 1;
        }

        // Each piece is its cells' bounding rectangle, less the cells of other pieces inside it.
        let mut pieces: Vec<Piece> = (0..count)
            .map(|piece| {
                let cells: Vec<(usize, usize)> = (0..columns * rows)
                    .filter(|&c| piece_of[c] == Some(piece))
                    .map(|c| (c % columns, c / columns))
                    .collect();
                let i0 = cells.iter().map(|c| c.0).min().expect("a piece has cells");
                let i1 = cells.iter().map(|c| c.0).max().expect("a piece has cells");
                let j0 = cells.iter().map(|c| c.1).min().expect("a piece has cells");
                let j1 = cells.iter().map(|c| c.1).max().expect("a piece has cells");
                let (l, w) = (ls[i0], ws[j0]);
                let mut board = self.part(l, w, ls[i1 + 1] - l, ws[j1 + 1] - w);
                let mut others = Vec::new();
                for j in j0..=j1 {
                    for i in i0..=i1 {
                        if piece_of[cell(i, j)].is_some_and(|other| other != piece) {
                            others.push(Cutaway {
                                l: (ls[i], ls[i + 1]),
                                w: (ws[j], ws[j + 1]),
                            });
                        }
                    }
                }
                board.cutaways.extend(
                    merged(others)
                        .into_iter()
                        .map(|cutaway| cutaway.moved_to(l, w)),
                );
                Piece { board, l, w }
            })
            .collect();
        pieces.sort_by_key(|piece| (piece.l, piece.w));
        pieces
    }

    /// Where `kerf` goes all the way through this board, trimmed to it, if anywhere.
    fn through_hole(&self, kerf: &Kerf) -> Option<Cutaway> {
        let (start, end) = kerf.through_span(self.thickness)?;
        let along = (start.max(Length::ZERO), end.min(kerf.along().of(self)));
        let across = (
            kerf.near.max(Length::ZERO),
            kerf.far.min(kerf.across.of(self)),
        );
        if along.0 >= along.1 || across.0 >= across.1 {
            return None;
        }
        Some(match kerf.across {
            Axis::Width => Cutaway {
                l: along,
                w: across,
            },
            Axis::Length => Cutaway {
                l: across,
                w: along,
            },
        })
    }

    /// The part of this board with its corner at `(l, w)` and the given length and width. It
    /// keeps the slots and cutaways that reach into it.
    fn part(&self, l: Length, w: Length, length: Length, width: Length) -> Board {
        let mut part = Board::new(length, width, self.thickness);
        part.kerfs = self
            .kerfs
            .iter()
            .map(|kerf| kerf.moved_to(l, w))
            .filter(|kerf| kerf.touches(length, width, self.thickness))
            .collect();
        part.cutaways = self
            .cutaways
            .iter()
            .map(|cutaway| cutaway.moved_to(l, w))
            .filter(|cutaway| cutaway.within(length, width).is_some())
            .collect();
        part
    }
}

/// A rectangle of a board's face, all the way through it, where the board has no wood because
/// another piece was there: the corner a rip and a crosscut cut out of it, say. Spans `l` along
/// the board's length and `w` across its width.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Cutaway {
    pub l: (Length, Length),
    pub w: (Length, Length),
}

impl Cutaway {
    /// The part of this cutaway on a board of this length and width, if any of it is.
    fn within(self, length: Length, width: Length) -> Option<Cutaway> {
        let l = (self.l.0.max(Length::ZERO), self.l.1.min(length));
        let w = (self.w.0.max(Length::ZERO), self.w.1.min(width));
        (l.0 < l.1 && w.0 < w.1).then_some(Cutaway { l, w })
    }

    fn moved_to(self, l: Length, w: Length) -> Cutaway {
        Cutaway {
            l: (self.l.0 - l, self.l.1 - l),
            w: (self.w.0 - w, self.w.1 - w),
        }
    }
}

/// A board cut from a bigger one, and where its corner was on that board: `l` along its length
/// and `w` across its width.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Piece {
    pub board: Board,
    pub l: Length,
    pub w: Length,
}

impl Piece {
    /// Where the piece's corner was on `axis` of the board it came from.
    pub fn at(&self, axis: Axis) -> Length {
        axis.pick(self.l, self.w)
    }
}

/// `0`, `extent`, and every value in `cuts` that falls between them, in order, once each.
fn grid_lines(extent: Length, cuts: impl Iterator<Item = Length>) -> Vec<Length> {
    let mut lines: Vec<Length> = [Length::ZERO, extent]
        .into_iter()
        .chain(cuts.map(|cut| cut.clamp(Length::ZERO, extent)))
        .collect();
    lines.sort();
    lines.dedup();
    lines
}

/// Joins cells side by side along the board, then rows of the same span across it, so a
/// cutaway is a few rectangles rather than many cells.
fn merged(mut cells: Vec<Cutaway>) -> Vec<Cutaway> {
    cells.sort_by_key(|cell| (cell.w, cell.l));
    let mut runs: Vec<Cutaway> = Vec::new();
    for cell in cells {
        match runs.last_mut() {
            Some(run) if run.w == cell.w && run.l.1 == cell.l.0 => run.l.1 = cell.l.1,
            _ => runs.push(cell),
        }
    }
    runs.sort_by_key(|run| (run.l, run.w));
    let mut blocks: Vec<Cutaway> = Vec::new();
    for run in runs {
        match blocks.last_mut() {
            Some(block) if block.l == run.l && block.w.1 == run.w.0 => block.w.1 = run.w.1,
            _ => blocks.push(run),
        }
    }
    blocks
}

/// Formats the way woodworkers write it: thickness x width x length.
impl fmt::Display for Board {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} x {} x {}", self.thickness, self.width, self.length)
    }
}
