use std::fmt;

use crate::{Kerf, Length};

/// A board: rectangular stock, plus any slots saws have cut into it without cutting it apart.
/// Its length runs along the grain.
///
/// Positions on a board are in its own coordinates: `l` along its length from one end, `w`
/// across its width from one edge, and `t` up through its thickness from its bottom face.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Board {
    length: Length,
    width: Length,
    thickness: Length,
    kerfs: Vec<Kerf>,
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

    /// This board with one more slot cut into it.
    pub(crate) fn with_kerf(&self, kerf: Kerf) -> Board {
        let mut board = self.clone();
        board.kerfs.push(kerf);
        board
    }

    /// The part of this board from `from` to `to` along its length, as a board of its own.
    pub(crate) fn along(&self, from: Length, to: Length) -> Board {
        self.part(from, Length::ZERO, to - from, self.width)
    }

    /// The separate pieces this board is in, in order across it from its `w = 0` edge. Wherever
    /// slots, alone or together, go through the board's whole length and thickness, they divide
    /// it; wood the slots took entirely is sawdust. Each piece is trimmed to its wood.
    ///
    /// Every slot so far reaches in from an end of the board (a saw can't start one in the
    /// middle), so a slice of the board between slot faces has at most one stretch of wood
    /// along it, and pieces divide only across the width.
    pub(crate) fn pieces(&self) -> Vec<Piece> {
        // Slice the board across its width wherever a slot has a face.
        let mut faces = vec![Length::ZERO, self.width];
        faces.extend(
            self.kerfs
                .iter()
                .flat_map(|kerf| [kerf.near, kerf.far])
                .map(|w| w.clamp(Length::ZERO, self.width)),
        );
        faces.sort();
        faces.dedup();

        // Neighboring slices hold together where both have wood at the same point along the
        // board. Each group is (from, to) across the board and (start, end) along it.
        let mut groups: Vec<[Length; 4]> = Vec::new();
        let mut previous: Vec<(Length, Length)> = Vec::new();
        for slice in faces.windows(2) {
            let (from, to) = (slice[0], slice[1]);
            let through: Vec<(Length, Length)> = self
                .kerfs
                .iter()
                .filter(|kerf| kerf.near <= from && kerf.far >= to)
                .filter_map(|kerf| kerf.through_span(self.thickness))
                .collect();
            let wood = wood_along(through, self.length);
            let (Some(&(start, _)), Some(&(_, end))) = (wood.first(), wood.last()) else {
                previous.clear();
                continue;
            };
            match groups.last_mut() {
                Some(group) if overlap(&previous, &wood) => {
                    group[1] = to;
                    group[2] = group[2].min(start);
                    group[3] = group[3].max(end);
                }
                _ => groups.push([from, to, start, end]),
            }
            previous = wood;
        }
        groups
            .into_iter()
            .map(|[from, to, start, end]| Piece {
                board: self.part(start, from, end - start, to - from),
                along: start,
                across: from,
            })
            .collect()
    }

    /// The part of this board with its corner at `(l, w)` = (`along`, `across`) and the given
    /// length and width. It keeps the slots that reach into it.
    fn part(&self, along: Length, across: Length, length: Length, width: Length) -> Board {
        let mut part = Board::new(length, width, self.thickness);
        part.kerfs = self
            .kerfs
            .iter()
            .map(|kerf| kerf.moved_to(along, across))
            .filter(|kerf| kerf.touches(length, width, self.thickness))
            .collect();
        part
    }
}

/// A board cut from a bigger one, and where its corner was on that board: `along` its length
/// and `across` its width.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Piece {
    pub board: Board,
    pub along: Length,
    pub across: Length,
}

/// The stretches of `0..length` that the spans in `through` leave uncovered, in order. Spans
/// that only touch leave nothing between them.
fn wood_along(mut through: Vec<(Length, Length)>, length: Length) -> Vec<(Length, Length)> {
    through.sort();
    let mut wood = Vec::new();
    let mut reached = Length::ZERO;
    for (start, end) in through {
        if start > reached {
            wood.push((reached, start.min(length)));
        }
        reached = reached.max(end);
        if reached >= length {
            return wood;
        }
    }
    wood.push((reached, length));
    wood
}

/// Whether any stretch in `a` shares some length with one in `b`.
fn overlap(a: &[(Length, Length)], b: &[(Length, Length)]) -> bool {
    a.iter().any(|&(a_start, a_end)| {
        b.iter()
            .any(|&(b_start, b_end)| a_start.max(b_start) < a_end.min(b_end))
    })
}

/// Formats the way woodworkers write it: thickness x width x length.
impl fmt::Display for Board {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} x {} x {}", self.thickness, self.width, self.length)
    }
}
