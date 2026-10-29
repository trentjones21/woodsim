use std::{
    fmt,
    hash::{Hash, Hasher},
    sync::Arc,
};

use crate::{
    Angle, Axis, Kerf, Length,
    face::{Direction, Edge, Region, merge_all},
    history::{Operation, Origin, Part, Side},
    kerf::Across,
};

/// A board: stock of some length, width, and thickness, less any slots saws have cut into it
/// without cutting it apart, and less wherever saws have cut it short of its full rectangle:
/// a mitered end, or a corner cut out. Its length runs along the grain.
///
/// Positions on a board are in its own coordinates: `l` along its length from one end, `w`
/// across its width from one edge, and `t` up through its thickness from its bottom face.
///
/// A board also knows where it came from (`origin`), which doesn't change what it is: boards
/// the same size and shape are equal however they were made.
#[derive(Clone)]
pub struct Board {
    length: Length,
    width: Length,
    thickness: Length,
    face: Vec<Region>,
    kerfs: Vec<Kerf>,
    origin: Origin,
}

impl PartialEq for Board {
    fn eq(&self, other: &Board) -> bool {
        (self.length, self.width, self.thickness) == (other.length, other.width, other.thickness)
            && self.face == other.face
            && self.kerfs == other.kerfs
    }
}

impl Eq for Board {}

impl Hash for Board {
    fn hash<H: Hasher>(&self, state: &mut H) {
        (self.length, self.width, self.thickness).hash(state);
        self.face.hash(state);
        self.kerfs.hash(state);
    }
}

impl fmt::Debug for Board {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Board")
            .field("length", &self.length)
            .field("width", &self.width)
            .field("thickness", &self.thickness)
            .field("face", &self.face)
            .field("kerfs", &self.kerfs)
            .finish()
    }
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
            origin: Origin::new_stock(length, width, thickness),
        }
    }

    /// Where the board came from: new lumber, or a piece of another board a saw cut.
    pub fn origin(&self) -> &Origin {
        &self.origin
    }

    /// Cuts `kerf` into the board, as `operation` did: the board with the slot in it, and the
    /// pieces that leaves it in, each knowing it came from this board by this cut. A slot that
    /// doesn't cut the board apart leaves it one piece, the slotted board itself.
    pub(crate) fn sawn(&self, kerf: Kerf, operation: &Arc<Operation>) -> (Board, Vec<Piece>) {
        let mut kerfed = self.with_kerf(kerf);
        let zero = [Length::ZERO; 3];
        kerfed.origin = Origin::sawn(
            operation,
            &self.origin,
            kerf,
            Part::Whole,
            zero,
            Side::Whole,
        );
        let mut pieces = kerfed.pieces();
        if let [whole] = pieces.as_mut_slice()
            && whole.board == kerfed
        {
            whole.board.origin = kerfed.origin.clone();
            return (kerfed, pieces);
        }
        for (n, piece) in pieces.iter_mut().enumerate() {
            let at = [piece.l, piece.w, piece.t];
            // Which side of the cut each piece is on, the saw sorts out.
            let part = Part::Piece(n);
            piece.board.origin = Origin::sawn(operation, &self.origin, kerf, part, at, Side::Loose);
        }
        (kerfed, pieces)
    }

    /// Notes which side of the cut that made it the board was on.
    pub(crate) fn set_side(&mut self, side: Side) {
        self.origin = self.origin.on_side(side);
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

    /// Whether the board has wood at the point `(l, w, t)`: inside its face and thickness, and
    /// not where a slot took it. Points on a surface don't count.
    pub fn has_wood_in(&self, l: Length, w: Length, t: Length) -> bool {
        Length::ZERO < t
            && t < self.thickness
            && self.face.iter().any(|region| region.contains(l, w))
            && !self.kerfs.iter().any(|kerf| kerf.contains(l, w, t))
    }

    /// The separate pieces this board is in. Wherever slots, alone or together, go all the
    /// way through the board, they divide it: a slot the length of the board splits it in two,
    /// and a rip and a crosscut that meet cut out the corner between them. Resaw slots square
    /// to the thickness that reach across the whole face, one or more together, split it into
    /// layers, and slots in the face that go through a thinner layer divide it in turn. Wood
    /// the slots took entirely is sawdust. Each piece is trimmed to its wood, and in order by
    /// where its corner was.
    ///
    /// Two slots in the face, from opposite faces, whose depths together go through don't
    /// divide a board yet.
    pub(crate) fn pieces(&self) -> Vec<Piece> {
        let across_face = self.face_pieces();
        let mut changed = !matches!(across_face.as_slice(), [whole] if whole.board == *self);
        let mut pieces = Vec::new();
        for piece in across_face {
            let Some(layers) = piece.board.layers() else {
                pieces.push(piece);
                continue;
            };
            changed = true;
            for layer in layers {
                for part in layer.board.pieces() {
                    pieces.push(Piece {
                        board: part.board,
                        l: piece.l + part.l,
                        w: piece.w + part.w,
                        t: layer.t + part.t,
                    });
                }
            }
        }
        if !changed {
            return vec![Piece::whole(self.clone())];
        }
        pieces.sort_by_key(|piece| (piece.l, piece.w, piece.t));
        pieces
    }

    /// The pieces slots through the board divide its face into, as `pieces` does but without
    /// splitting it into layers.
    fn face_pieces(&self) -> Vec<Piece> {
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
            return vec![Piece::whole(self.clone())];
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
                    t: Length::ZERO,
                }
            })
            .collect();
        pieces.sort_by_key(|piece| (piece.l, piece.w));
        pieces
    }

    /// The layers resaw slots split the board into, if they do: where slots square to the
    /// thickness, sharing some depth in it, together reach across the whole face. The layer
    /// below runs up to the highest of their lower faces and the layer above from the lowest of
    /// their upper ones; the slots stay in both, so where one of them is lower or higher than
    /// the others, it still takes its wood out of that layer.
    fn layers(&self) -> Option<Vec<Piece>> {
        let resaws: Vec<&Kerf> = self
            .kerfs
            .iter()
            .filter(|kerf| matches!(kerf.across, Across::Thickness))
            .filter(|kerf| kerf.near < self.thickness && kerf.far > Length::ZERO)
            .collect();
        // Any group of slots sharing some depth all reach just above one of their lower faces.
        resaws.iter().find_map(|first| {
            let depth = first.near + Length::from_nm(1);
            let group: Vec<&Kerf> = resaws
                .iter()
                .copied()
                .filter(|kerf| kerf.near < depth && depth < kerf.far)
                .collect();
            let outlines: Vec<Region> = group.iter().filter_map(|kerf| kerf.outline()).collect();
            let covered = self.face.iter().all(|region| {
                outlines
                    .iter()
                    .fold(vec![region.clone()], |left, outline| {
                        left.iter().flat_map(|part| part.minus(outline)).collect()
                    })
                    .is_empty()
            });
            if !covered {
                return None;
            }
            let below = group.iter().map(|kerf| kerf.near).max()?;
            let above = group.iter().map(|kerf| kerf.far).min()?;
            let layers = [
                (below > Length::ZERO).then(|| self.layer(Length::ZERO, below)),
                (above < self.thickness).then(|| self.layer(above, self.thickness)),
            ];
            Some(layers.into_iter().flatten().collect())
        })
    }

    /// The part of the board from `t` = `bottom` to `top`, as a piece of it.
    fn layer(&self, bottom: Length, top: Length) -> Piece {
        let mut board = Board::new(self.length, self.width, top - bottom);
        board.face = self.face.clone();
        board.kerfs = self
            .kerfs
            .iter()
            .map(|kerf| kerf.moved_up(bottom))
            .filter(|kerf| board.touched_by(kerf))
            .collect();
        Piece {
            board,
            l: Length::ZERO,
            w: Length::ZERO,
            t: bottom,
        }
    }

    /// How `kerf`, one of this board's slots, measures in it.
    pub fn slot(&self, kerf: &Kerf) -> SlotSize {
        match kerf.across {
            Across::Face(across) => self.slot_in_face(kerf, across),
            Across::Thickness => self.slot_in_edge(kerf),
        }
    }

    fn slot_in_face(&self, kerf: &Kerf, across: Direction) -> SlotSize {
        let in_slot: Vec<Region> = self
            .face
            .iter()
            .filter_map(|region| region.clip_all(kerf.faces()))
            .collect();
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
            width: measured(range(&in_slot, across), across),
            depth: kerf.depth(self.thickness).min(self.thickness),
            at_edge: !wood_beyond(near_face) || !wood_beyond(far_face),
            bottom: run(Length::ZERO),
            top: run(self.thickness),
        }
    }

    /// Measures a slot square to the thickness, which came in through one of the board's edges
    /// square to `along.square()`.
    fn slot_in_edge(&self, kerf: &Kerf) -> SlotSize {
        let square = kerf.along.square();
        let scale = kerf.along.scale();
        let radius = kerf.radius.as_nm() as f64 * scale;
        let center = kerf.center.as_nm() as f64;
        let (low, high) = range(&self.face, square);
        let wood = range(&self.face, kerf.along);
        // How far the blade reached along each edge, within the board's length there.
        let run = |side: f64| {
            let off = side - center;
            (off.abs() <= radius).then(|| {
                let half = (radius * radius - off * off).sqrt();
                let reach = (
                    kerf.start.as_nm() as f64 - half,
                    kerf.end.as_nm() as f64 + half,
                );
                let within = (reach.0.max(wood.0), reach.1.min(wood.1));
                Run {
                    length: measured(within, kerf.along),
                    end_to_end: reach.0 <= wood.0 && reach.1 >= wood.1,
                }
            })
        };
        let depth = if center < low {
            center + radius - low
        } else {
            high - (center - radius)
        };
        let nm = |scaled: f64| Length::from_nm((scaled / scale).round() as i64);
        SlotSize {
            width: (kerf.far.min(self.thickness) - kerf.near.max(Length::ZERO)).max(Length::ZERO),
            depth: nm(depth.min(high - low)),
            at_edge: kerf.near <= Length::ZERO || kerf.far >= self.thickness,
            bottom: run(low),
            top: run(high),
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

/// The range of `direction · (l, w)` over `regions`, times the direction's scale.
fn range(regions: &[Region], direction: Direction) -> (f64, f64) {
    regions
        .iter()
        .map(|region| region.extent(direction))
        .fold((f64::MAX, f64::MIN), |(low, high), (a, b)| {
            (low.min(a), high.max(b))
        })
}

/// The length of `(low, high)`, measured along `direction`, to the nanometer.
fn measured((low, high): (f64, f64), direction: Direction) -> Length {
    Length::from_nm(((high - low).max(0.0) / direction.scale()).round() as i64)
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
    /// How deep it goes into the board from where the blade came in, at its deepest.
    pub depth: Length,
    /// Whether it's at an edge or end of the board, with wood on one side of it only.
    pub at_edge: bool,
    /// How far it runs along the board on the two sides the blade could come out of, if it
    /// reached them: for a slot in the face, its bottom face and its top; for a slot square to
    /// the thickness, the edges where `along.square()` is lowest and highest.
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

/// A board cut from a bigger one, and where its corner was on that board: `l` along its
/// length, `w` across its width, and `t` up through its thickness.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Piece {
    pub board: Board,
    pub l: Length,
    pub w: Length,
    pub t: Length,
}

impl Piece {
    /// All of `board`, as a piece of itself.
    fn whole(board: Board) -> Piece {
        Piece {
            board,
            l: Length::ZERO,
            w: Length::ZERO,
            t: Length::ZERO,
        }
    }

    /// Where the piece's corner was on `axis` of the board it came from.
    pub fn at(&self, axis: Axis) -> Length {
        match axis {
            Axis::Length => self.l,
            Axis::Width => self.w,
            Axis::Thickness => self.t,
        }
    }
}

/// Formats the way woodworkers write it: thickness x width x length.
impl fmt::Display for Board {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} x {} x {}", self.thickness, self.width, self.length)
    }
}
