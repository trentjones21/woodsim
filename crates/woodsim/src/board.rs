use std::fmt;

use crate::{
    Angle, Axis, Kerf, Length,
    face::{Direction, Edge, Region, merge_all},
};

/// A board: stock of some length, width, and thickness, less any slots saws have cut into it
/// without cutting it apart, and less wherever saws have cut it short of its full rectangle:
/// a mitered end, or a corner cut out. Its length runs along the grain.
///
/// Positions on a board are in its own coordinates: `l` along its length from one end, `w`
/// across its width from one edge, and `t` up through its thickness from its bottom face.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Board {
    length: Length,
    width: Length,
    thickness: Length,
    face: Vec<Region>,
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
            face: vec![Region::rectangle(
                (Length::ZERO, length),
                (Length::ZERO, width),
            )],
            kerfs: Vec::new(),
        }
    }

    /// How far the board reaches along its length: from long point to long point, if its ends
    /// are mitered.
    pub fn length(&self) -> Length {
        self.length
    }

    pub fn width(&self) -> Length {
        self.width
    }

    pub fn thickness(&self) -> Length {
        self.thickness
    }

    /// The board's face, as convex regions that together cover its wood, all the way through
    /// it. A rectangular board is one rectangle.
    pub fn face(&self) -> &[Region] {
        &self.face
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

    /// Whether any of `kerf` is inside the board's wood.
    pub(crate) fn touched_by(&self, kerf: &Kerf) -> bool {
        self.face
            .iter()
            .any(|region| kerf.touches(region, self.thickness))
    }

    /// Whether the board has wood all the way through it at `(l, w)`: inside its face and not
    /// where a slot goes through. Points on an edge don't count.
    pub fn has_wood_at(&self, l: Length, w: Length) -> bool {
        self.face.iter().any(|region| region.contains(l, w))
            && !self.kerfs.iter().any(|kerf| {
                kerf.through_hole(self.thickness)
                    .is_some_and(|hole| hole.contains(l, w))
            })
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
        let mut wood = self.face.clone();
        for hole in self
            .kerfs
            .iter()
            .filter_map(|kerf| kerf.through_hole(self.thickness))
        {
            wood = wood.iter().flat_map(|region| region.minus(&hole)).collect();
        }

        // Regions of wood that share a stretch of edge hold together.
        let mut group: Vec<usize> = (0..wood.len()).collect();
        fn root(group: &mut [usize], mut i: usize) -> usize {
            while group[i] != i {
                group[i] = group[group[i]];
                i = group[i];
            }
            i
        }
        for i in 0..wood.len() {
            for j in i + 1..wood.len() {
                if wood[i].joins(&wood[j]) {
                    let (a, b) = (root(&mut group, i), root(&mut group, j));
                    group[a] = b;
                }
            }
        }
        let mut pieces: Vec<Vec<Region>> = Vec::new();
        let mut roots: Vec<usize> = Vec::new();
        for (i, region) in wood.into_iter().enumerate() {
            let r = root(&mut group, i);
            match roots.iter().position(|&seen| seen == r) {
                Some(k) => pieces[k].push(region),
                None => {
                    roots.push(r);
                    pieces.push(vec![region]);
                }
            }
        }

        let bounds = |regions: &[Region]| {
            regions.iter().map(Region::bounds).fold(
                (
                    (Length::from_nm(i64::MAX), Length::from_nm(i64::MIN)),
                    (Length::from_nm(i64::MAX), Length::from_nm(i64::MIN)),
                ),
                |((l0, l1), (w0, w1)), ((a0, a1), (b0, b1))| {
                    ((l0.min(a0), l1.max(a1)), (w0.min(b0), w1.max(b1)))
                },
            )
        };
        let whole = ((Length::ZERO, self.length), (Length::ZERO, self.width));
        if let [regions] = pieces.as_slice()
            && bounds(regions) == whole
        {
            // Still in one piece and as big as ever: the slots are just slots.
            return vec![Piece {
                board: self.clone(),
                l: Length::ZERO,
                w: Length::ZERO,
            }];
        }

        let mut pieces: Vec<Piece> = pieces
            .into_iter()
            .map(|regions| {
                let ((l0, l1), (w0, w1)) = bounds(&regions);
                let mut board = Board::new(l1 - l0, w1 - w0, self.thickness);
                board.face = merge_all(regions.iter().map(|r| r.moved(l0, w0)).collect());
                board.kerfs = self
                    .kerfs
                    .iter()
                    .map(|kerf| kerf.moved_to(l0, w0))
                    .filter(|kerf| board.touched_by(kerf))
                    .collect();
                Piece {
                    board,
                    l: l0,
                    w: w0,
                }
            })
            .collect();
        pieces.sort_by_key(|piece| (piece.l, piece.w));
        pieces
    }

    /// How `kerf`, one of this board's slots, measures in it.
    pub fn slot(&self, kerf: &Kerf) -> SlotSize {
        let in_slot: Vec<Region> = self
            .face
            .iter()
            .filter_map(|region| region.clip_all(kerf.faces()))
            .collect();
        let range = |regions: &[Region], direction: Direction| {
            regions
                .iter()
                .map(|region| region.extent(direction))
                .fold((f64::MAX, f64::MIN), |(low, high), (a, b)| {
                    (low.min(a), high.max(b))
                })
        };
        let measured = |(low, high): (f64, f64), direction: Direction| {
            Length::from_nm(((high - low).max(0.0) / direction.scale()).round() as i64)
        };
        let [near_face, far_face] = kerf.faces();
        let wood_beyond = |face: Edge| {
            self.face
                .iter()
                .any(|region| region.clip(face.flipped()).is_some())
        };
        let run = |t: Length| {
            let (start, end) = kerf.span_at(t)?;
            let nm = |length: Length| length.as_nm() as i128;
            let band = [
                Edge::new(kerf.along, nm(end)),
                Edge::new(kerf.along.reversed(), -nm(start)),
            ];
            let reached: Vec<Region> = in_slot
                .iter()
                .filter_map(|region| region.clip_all(band))
                .collect();
            Some(Run {
                length: measured(range(&reached, kerf.along), kerf.along),
                end_to_end: reached == in_slot,
            })
        };
        SlotSize {
            width: measured(range(&in_slot, kerf.across), kerf.across),
            at_edge: !wood_beyond(near_face) || !wood_beyond(far_face),
            bottom: run(Length::ZERO),
            top: run(self.thickness),
        }
    }

    /// The angles of the board's slanted ends, from square: what a miter saw cut them at. The
    /// end at `l = 0` comes first.
    pub fn miters(&self) -> Vec<Angle> {
        let edges: Vec<Edge> = self.face.iter().flat_map(Region::edges).copied().collect();
        // An edge between two of the face's regions isn't one of the board's.
        let mut ends: Vec<Edge> = edges
            .iter()
            .filter(|edge| !edges.contains(&edge.flipped()))
            .filter(|edge| edge.out.axis().is_none() && is_end(edge.out))
            .copied()
            .collect();
        ends.sort_by_key(|edge| edge.out.l > 0);
        ends.dedup();
        ends.into_iter()
            .map(|edge| Angle::nearest(slant(edge.out)))
            .collect()
    }
}

/// How far `out`, the way an edge faces, is turned from the board's length, in radians, either
/// way.
fn slant(out: Direction) -> f64 {
    (out.w.abs() as f64).atan2(out.l.abs() as f64)
}

/// Whether an edge facing `out` is one of the board's ends rather than its sides: within 60° of
/// square to its length. A miter saw turns no further than that.
fn is_end(out: Direction) -> bool {
    slant(out) < 60f64.to_radians()
}

/// How a slot in a board measures, to the nanometer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SlotSize {
    /// How wide it is in the board, square to its faces.
    pub width: Length,
    /// Whether it's at an edge or end of the board, with wood on one side of it only.
    pub at_edge: bool,
    /// How far it runs along the board on its bottom face and on its top, if the blade reached
    /// that face.
    pub bottom: Option<Run>,
    pub top: Option<Run>,
}

/// How far a slot runs along a board on one face.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Run {
    pub length: Length,
    /// Whether it runs from one side of the board to the other.
    pub end_to_end: bool,
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

/// Formats the way woodworkers write it: thickness x width x length.
impl fmt::Display for Board {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} x {} x {}", self.thickness, self.width, self.length)
    }
}
