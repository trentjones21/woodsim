use std::{
    fmt,
    hash::{Hash, Hasher},
    sync::Arc,
};

use crate::{
    Angle, Axis, Kerf, Length, Surface,
    assembly::surfaces,
    face::{Direction, Edge, Region, Vector, merge_all},
    history::{Operation, Origin, Part, Side},
    kerf::{self, Opening},
    solid::{self, hull, line_crosses},
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
    /// The same board, made as `origin` says it was.
    pub(crate) fn with_origin(mut self, origin: Origin) -> Board {
        self.origin = origin;
        self
    }

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

    /// Whether any of the board's wood is beyond `side`, away from the wood it bounds, exactly.
    pub(crate) fn reaches_beyond(&self, side: Edge) -> bool {
        self.face
            .iter()
            .any(|region| region.solid(self.thickness).beyond(side))
    }

    /// The corners of the board's wood, `[l, w, t]` to the nearest nanometer: its face's on the
    /// bottom and the top, and wherever leaning sides meet between.
    pub fn corners(&self) -> Vec<[Length; 3]> {
        self.face
            .iter()
            .flat_map(|region| region.solid(self.thickness).corners())
            .map(|corner| corner.nearest())
            .collect()
    }

    /// How much wood is inside the board's outline, through its thickness, in cubic
    /// nanometers: not less the slots in it.
    pub fn volume(&self) -> f64 {
        self.face
            .iter()
            .map(|region| region.solid(self.thickness).volume())
            .sum()
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
        self.face
            .iter()
            .any(|region| region.solid(self.thickness).contains_through(l, w))
            && !self.kerfs.iter().any(|kerf| {
                self.face.iter().any(|region| {
                    kerf.hole(region, self.thickness)
                        .is_some_and(|hole| line_crosses(&hole, l, w, self.thickness))
                })
            })
    }

    /// Whether the board has wood at the point `(l, w, t)`: inside its face and thickness, and
    /// not where a slot took it. Points on a surface don't count.
    pub fn has_wood_in(&self, l: Length, w: Length, t: Length) -> bool {
        self.face
            .iter()
            .any(|region| region.solid(self.thickness).contains(l, w, t))
            && !self.kerfs.iter().any(|kerf| kerf.contains(l, w, t))
    }

    /// The separate pieces this board is in. Wherever slots, alone or together, go all the
    /// way through the board, they divide it: a slot the length of the board splits it in two,
    /// and a rip and a crosscut that meet cut out the corner between them. Slots whose blades
    /// turned in the same plane, sharing some of their width, divide it where together they
    /// swept all the wood between them: resaws from both edges that meet split it into layers,
    /// as do grooves from both faces whose depths together go through. A slot that goes
    /// through what's left divides that in turn. Wood the slots took entirely is sawdust. Each
    /// piece is trimmed to its wood, and in order by where its corner was.
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
                        l: piece.l + layer.l + part.l,
                        w: piece.w + layer.w + part.w,
                        t: piece.t + layer.t + part.t,
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

    /// The pieces slots that each go through on their own divide the board into.
    fn face_pieces(&self) -> Vec<Piece> {
        let thickness = self.thickness;
        let mut wood = self.face.clone();
        for kerf in &self.kerfs {
            wood = wood
                .iter()
                .flat_map(|region| match kerf.hole(region, thickness) {
                    Some(hole) => region.solid(thickness).minus(&hole),
                    None => vec![region.clone()],
                })
                .collect();
        }
        self.held_together(wood)
    }

    /// The pieces `wood`, what's left of this board's regions, is in: regions that share a
    /// stretch of face hold together, and each piece is trimmed to its wood. The board itself,
    /// if it's still one piece as big as ever.
    fn held_together(&self, wood: Vec<Region>) -> Vec<Piece> {
        let thickness = self.thickness;

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
                if wood[i].solid(thickness).joins(&wood[j]) {
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
            let far = (Length::from_nm(i64::MAX), Length::from_nm(i64::MIN));
            regions
                .iter()
                .map(|region| region.solid(thickness).bounds())
                .fold([far; 3], |around, bounds| {
                    [0, 1, 2].map(|k| (around[k].0.min(bounds[k].0), around[k].1.max(bounds[k].1)))
                })
        };
        let whole = [
            (Length::ZERO, self.length),
            (Length::ZERO, self.width),
            (Length::ZERO, thickness),
        ];
        if let [regions] = pieces.as_slice()
            && bounds(regions) == whole
        {
            // Still in one piece and as big as ever: the slots are just slots.
            return vec![Piece::whole(self.clone())];
        }

        let mut pieces: Vec<Piece> = pieces
            .into_iter()
            .map(|regions| {
                let [(l0, l1), (w0, w1), (t0, t1)] = bounds(&regions);
                let mut board = Board::new(l1 - l0, w1 - w0, t1 - t0);
                // Moved to the piece's corner, a side along a face it now has is no side.
                board.face = merge_all(
                    regions
                        .iter()
                        .filter_map(|region| {
                            let moved = region.moved_by(l0, w0, t0);
                            solid::region(moved.edges().to_vec(), t1 - t0)
                        })
                        .collect(),
                    t1 - t0,
                );
                board.kerfs = self
                    .kerfs
                    .iter()
                    .map(|kerf| kerf.moved_by(l0, w0, t0))
                    .filter(|kerf| board.touched_by(kerf))
                    .collect();
                Piece {
                    board,
                    l: l0,
                    w: w0,
                    t: t0,
                }
            })
            .collect();
        pieces.sort_by_key(|piece| (piece.l, piece.w, piece.t));
        pieces
    }

    /// The pieces slots whose blades turned in the same plane split the board into, if they
    /// do: where slots facing the same way, sharing some of their width, together swept all the
    /// wood between them. It splits between the highest of their near faces and the lowest of
    /// their far ones; the slots stay in the pieces either side, so where one of them is wider
    /// than the others, it still takes its wood out of that piece.
    fn layers(&self) -> Option<Vec<Piece>> {
        let thickness = self.thickness;
        let corners = self.corners();
        let reaches = |kerf: &&Kerf| {
            let along: Vec<i128> = corners
                .iter()
                .map(|&[l, w, t]| kerf.normal.dot(l, w, t))
                .collect();
            let (low, high) = (along.iter().min(), along.iter().max());
            low.zip(high).is_some_and(|(&low, &high)| {
                (kerf.near.as_nm() as i128) < high && (kerf.far.as_nm() as i128) > low
            })
        };
        let slabs: Vec<&Kerf> = self.kerfs.iter().filter(reaches).collect();
        // Any group of slots sharing some width all reach just past one of their near faces.
        slabs.iter().find_map(|first| {
            let depth = first.near + Length::from_nm(1);
            let group: Vec<&Kerf> = slabs
                .iter()
                .copied()
                .filter(|kerf| kerf.normal == first.normal && kerf.near < depth && depth < kerf.far)
                .collect();
            let below = group.iter().map(|kerf| kerf.near).max()?;
            let above = group.iter().map(|kerf| kerf.far).min()?;
            if !self.swept_through(&group, (below, above)) {
                return None;
            }
            let between = slab(first.normal, (below, above));
            let wood = self
                .face
                .iter()
                .flat_map(|region| region.solid(thickness).minus(&between))
                .collect();
            // Taking the wood between them out may leave it all still in one piece.
            let pieces = self.held_together(wood);
            (!matches!(pieces.as_slice(), [whole] if whole.board == *self)).then_some(pieces)
        })
    }

    /// Whether the slots in `group`, whose blades all turned in planes facing the same way,
    /// together swept all of the board's wood between `below` and `above` on their normal: in
    /// their plane, whether where they swept covers where the wood is.
    fn swept_through(&self, group: &[&Kerf], (below, above): (Length, Length)) -> bool {
        let thickness = self.thickness;
        let normal = group[0].normal;
        let covered = |wood: Region, swept: &[Region]| {
            swept
                .iter()
                .fold(vec![wood], |left, swept| {
                    left.iter()
                        .flat_map(|part| part.solid(thickness).minus(swept.edges()))
                        .collect()
                })
                .is_empty()
        };
        if normal == Vector::THICKNESS && !self.face.iter().any(|region| region.leans()) {
            // Resawing, the plane is the board's face.
            let outlines: Vec<Region> = group.iter().filter_map(|kerf| kerf.outline()).collect();
            return self
                .face
                .iter()
                .all(|region| covered(region.clone(), &outlines));
        }
        let [x, y] = plane_axes(normal);
        let flat = |points: Vec<[f64; 3]>| {
            let points: Vec<(Length, Length)> = points
                .into_iter()
                .map(|p| {
                    let at = |axis: [f64; 3]| {
                        let along = axis[0] * p[0] + axis[1] * p[1] + axis[2] * p[2];
                        Length::from_nm(along.round() as i64)
                    };
                    (at(x), at(y))
                })
                .collect();
            Region::around(&hull(points))
        };
        let swept: Vec<Region> = group.iter().filter_map(|kerf| flat(kerf.swept())).collect();
        self.face.iter().all(|region| {
            let Some(inside) = region
                .solid(thickness)
                .clip_all(slab(normal, (below, above)))
            else {
                return true;
            };
            let corners = inside
                .solid(thickness)
                .corners()
                .into_iter()
                .map(|corner| corner.position())
                .collect();
            flat(corners).is_none_or(|wood| covered(wood, &swept))
        })
    }

    /// How `kerf`, one of this board's slots, measures in it, from the surface the blade came
    /// in through: of the surfaces it opens onto, the one facing most nearly back toward the
    /// blade's center, however the board lay. Worked out to within floating point of a
    /// nanometer, as you'd measure it.
    pub fn slot(&self, kerf: &Kerf) -> SlotSize {
        let corners: Vec<[f64; 3]> = self
            .corners()
            .into_iter()
            .map(|p| p.map(|length| length.as_nm() as f64))
            .collect();
        let dot = |a: [f64; 3], b: [f64; 3]| a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
        let unit = |v: [f64; 3]| v.map(|c| c / dot(v, v).sqrt());
        let extent = |toward: [f64; 3]| {
            corners
                .iter()
                .map(|&p| dot(p, toward))
                .fold((f64::MAX, f64::MIN), |(low, high), x| {
                    (low.min(x), high.max(x))
                })
        };
        let nm = |length: Length| length.as_nm() as f64;
        let normal = kerf.normal;
        let (least, most) = extent([normal.l, normal.w, normal.t].map(|c| c as f64));
        let (near, far) = (nm(kerf.near), nm(kerf.far));
        let to_length =
            |scaled: f64, scale: f64| Length::from_nm((scaled.max(0.0) / scale).round() as i64);
        // Up the blade's plane, from its center's path toward the wood.
        let up = kerf.up();
        let up = [up.l, up.w, up.t].map(|c| c as f64);
        let center = kerf.center as f64 / dot(up, up).sqrt();
        let up = unit(up);
        let (low, high) = extent(up);
        let toward = if (low + high) / 2.0 >= center {
            up
        } else {
            up.map(|c| -c)
        };
        // Each surface, the way it faces out of the wood, and where its plane is that way.
        let faces: Vec<(Surface, [f64; 3], f64)> = surfaces(self)
            .into_iter()
            .map(|surface| {
                let (plane, at) = self.plane_of(surface);
                let size = dot(plane, plane).sqrt();
                (surface, plane.map(|c| c / size), at / size)
            })
            .collect();
        let facing = |a: &&(Surface, [f64; 3], f64), b: &&(Surface, [f64; 3], f64)| {
            dot(a.1, toward).total_cmp(&dot(b.1, toward))
        };
        let opens: Vec<&(Surface, [f64; 3], f64)> = faces
            .iter()
            .filter(|(surface, ..)| self.opening_on(kerf, *surface).is_some())
            .collect();
        let entry = opens
            .iter()
            .copied()
            .min_by(facing)
            .or_else(|| faces.iter().min_by(facing))
            .expect("a board has surfaces");
        // Where it comes out: of the others it opens onto, the one facing most nearly away
        // from the blade; or, if it opens onto none facing away, the one it would come out of.
        let exit = opens
            .iter()
            .copied()
            .filter(|face| face.0 != entry.0 && dot(face.1, toward) > 1e-6)
            .max_by(facing)
            .or_else(|| faces.iter().max_by(facing))
            .expect("a board has surfaces");
        // Square to the surface it came in through, as far as the slot goes into the wood:
        // where it's deepest on the plane midway between its faces, either as far as the blade
        // reached, or where the slot meets the board's outside.
        let into = entry.1.map(|c| -c);
        let along = |point: [f64; 3]| dot(point, into);
        let middle = (near + far) / 2.0;
        let furthest = kerf.furthest(into);
        let reached = self
            .outside()
            .filter_map(|(plane, at)| {
                let (point, direction) = kerf.line(plane, at, middle)?;
                let (first, last) = self.wood_along(point, direction)?;
                let (from, to) = kerf.reached(point, direction)?;
                let (from, to) = (from.max(first), to.min(last));
                (from <= to).then(|| {
                    let at = |s: f64| along([0, 1, 2].map(|k| point[k] + s * direction[k]));
                    at(from).max(at(to))
                })
            })
            .chain(self.holds(furthest).then(|| along(furthest)))
            .fold(f64::MIN, f64::max);
        // The surface it came in through is where `along` is `-at`.
        let depth = reached + entry.2;
        let through = extent(into).1 + entry.2;
        // On each, how far the opening runs in the wood, and whether across all of it.
        let run = |surface: Surface| {
            let (plane, at) = self.plane_of(surface);
            let opening = self.opening_on(kerf, surface)?;
            let (first, last) = self.wood_beside(kerf, plane, at)?;
            Some(Run {
                length: to_length(opening.to - opening.from, 1.0),
                end_to_end: opening.from <= first + 0.5 && opening.to >= last - 0.5,
            })
        };
        SlotSize {
            entry: entry.0,
            exit: exit.0,
            width: to_length(far.min(most) - near.max(least), normal.scale()),
            depth: to_length(depth.min(through), 1.0),
            at_edge: least >= near || most <= far,
            on_entry: run(entry.0),
            on_exit: run(exit.0),
        }
    }

    /// The plane `surface` of the board is on, as the way it faces out of the wood (not of unit
    /// length) and where it is that way: `plane · p = at`, `p` in nanometers.
    fn plane_of(&self, surface: Surface) -> ([f64; 3], f64) {
        match surface {
            Surface::Top => ([0.0, 0.0, 1.0], self.thickness.as_nm() as f64),
            Surface::Bottom => ([0.0, 0.0, -1.0], 0.0),
            Surface::Side(edge) => (
                [edge.out.l as f64, edge.out.w as f64, edge.lean as f64],
                edge.at as f64,
            ),
        }
    }

    /// Where `kerf`, one of this board's slots, opens onto the plane where the board's `axis`
    /// is `at` nanometers: where `Kerf::meets` has it, cut short where the wood there ends.
    /// `None` if it doesn't open onto the plane in the wood.
    pub fn opening(&self, kerf: &Kerf, axis: Axis, at: f64) -> Option<Opening> {
        self.opening_in(kerf, kerf::unit(axis), at)
    }

    /// Where `kerf` opens onto `surface` of the board, in the wood, as `opening` has it.
    pub fn opening_on(&self, kerf: &Kerf, surface: Surface) -> Option<Opening> {
        let (plane, at) = self.plane_of(surface);
        self.opening_in(kerf, plane, at)
    }

    /// Where `kerf` opens onto the plane `plane · p = at`, cut short where the wood there ends.
    fn opening_in(&self, kerf: &Kerf, plane: [f64; 3], at: f64) -> Option<Opening> {
        let opening = kerf.meets_plane(plane, at)?;
        let (first, last) = self.wood_beside(kerf, plane, at)?;
        let (from, to) = (opening.from.max(first), opening.to.min(last));
        (to - from > 0.5).then_some(Opening {
            from,
            to,
            ..opening
        })
    }

    /// Along the lines where a slot's faces meet the plane `plane · p = at`, from where the
    /// wood starts to where it ends, as `Kerf::meets_plane` measures along them: on lines at
    /// its faces where they're in the board, or at the board's outside where they're past it.
    fn wood_beside(&self, kerf: &Kerf, plane: [f64; 3], at: f64) -> Option<(f64, f64)> {
        let normal = kerf.normal;
        let (least, most) = self
            .corners()
            .iter()
            .map(|&[l, w, t]| normal.dot(l, w, t) as f64)
            .fold((f64::MAX, f64::MIN), |(low, high), x| {
                (low.min(x), high.max(x))
            });
        let faces = [
            (kerf.near.as_nm() as f64).max(least),
            (kerf.far.as_nm() as f64).min(most),
        ];
        faces
            .into_iter()
            .filter_map(|face| {
                let (point, direction) = kerf.line(plane, at, face)?;
                self.wood_along(point, direction)
            })
            .reduce(|a, b| (a.0.min(b.0), a.1.max(b.1)))
    }

    /// The planes around each of the board's regions, `plane · p <= at` inside, `p` in
    /// nanometers: its sides, and its bottom and top faces.
    fn outside(&self) -> impl Iterator<Item = ([f64; 3], f64)> + '_ {
        self.face.iter().flat_map(|region| self.around(region))
    }

    /// The planes around `region` of the board, as `outside` gives them.
    fn around(&self, region: &Region) -> Vec<([f64; 3], f64)> {
        let thickness = self.thickness.as_nm() as f64;
        region
            .edges()
            .iter()
            .map(|edge| {
                (
                    [edge.out.l as f64, edge.out.w as f64, edge.lean as f64],
                    edge.at as f64,
                )
            })
            .chain([([0.0, 0.0, -1.0], 0.0), ([0.0, 0.0, 1.0], thickness)])
            .collect()
    }

    /// Whether `point` (nanometers) is in the board's wood, or within half a nanometer of it.
    fn holds(&self, point: [f64; 3]) -> bool {
        self.wood_along(point, [1.0, 0.0, 0.0])
            .is_some_and(|(first, last)| first <= 0.0 && 0.0 <= last)
    }

    /// Along the line through `point` (nanometers) the way `direction` (a unit vector) runs,
    /// from where it goes into the wood to where it last comes out, from `point`, in
    /// nanometers, or `None` if it misses the board. Points on the board's surface count as in
    /// it, to within half a nanometer.
    fn wood_along(&self, point: [f64; 3], direction: [f64; 3]) -> Option<(f64, f64)> {
        self.face
            .iter()
            .filter_map(|region| {
                self.around(region).into_iter().try_fold(
                    (f64::MIN, f64::MAX),
                    |(first, last), (out, at): ([f64; 3], f64)| {
                        let size = (out[0] * out[0] + out[1] * out[1] + out[2] * out[2]).sqrt();
                        let toward = (0..3).map(|k| out[k] * direction[k]).sum::<f64>() / size;
                        let past = ((0..3).map(|k| out[k] * point[k]).sum::<f64>() - at) / size;
                        let (first, last) = if toward.abs() < 1e-12 {
                            if past > 0.5 {
                                return None;
                            }
                            (first, last)
                        } else if toward > 0.0 {
                            (first, last.min((0.5 - past) / toward))
                        } else {
                            (first.max((0.5 - past) / toward), last)
                        };
                        (first <= last).then_some((first, last))
                    },
                )
            })
            .reduce(|a, b| (a.0.min(b.0), a.1.max(b.1)))
    }

    /// The board's beveled sides, leaning 45° or less from square to its faces, each with how
    /// far it leans: what a tilted blade cut it at. In order by `Edge`'s ordering.
    pub fn bevels(&self) -> Vec<(Edge, Angle)> {
        self.leaning()
            .into_iter()
            .filter(|edge| !edge.tapers())
            .map(|edge| (edge, edge.lean_angle()))
            .collect()
    }

    /// The board's faces that slope: sides leaning more than 45° from square to its faces
    /// (`Edge::tapers`), each with how far it slopes from them, what resawing it on edge with
    /// the blade tilted leaves. In order by `Edge`'s ordering.
    pub fn tapers(&self) -> Vec<(Edge, Angle)> {
        self.leaning()
            .into_iter()
            .filter(|edge| edge.tapers())
            .map(|edge| (edge, Angle::from_degrees(90) - edge.lean_angle()))
            .collect()
    }

    /// The board's leaning sides, in order by `Edge`'s ordering.
    fn leaning(&self) -> Vec<Edge> {
        let edges: Vec<Edge> = self.face.iter().flat_map(Region::edges).copied().collect();
        let mut leaning: Vec<Edge> = edges
            .iter()
            .filter(|edge| edge.leans() && !edges.contains(&edge.flipped()))
            .copied()
            .collect();
        leaning.sort();
        leaning.dedup();
        leaning
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

/// The wood between `near` and `far` along `normal`, as the planes around it.
fn slab(normal: Vector, (near, far): (Length, Length)) -> [Edge; 2] {
    [
        normal.reversed().edge(-(near.as_nm() as i128)),
        normal.edge(far.as_nm() as i128),
    ]
}

/// Two directions square to `normal` and to each other, to lay out what's in a plane facing
/// that way: two of the board's own axes, where it faces along the third.
fn plane_axes(normal: Vector) -> [[f64; 3]; 2] {
    let axes = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];
    let n = [normal.l, normal.w, normal.t];
    if let [only] = (0..3).filter(|&k| n[k] != 0).collect::<Vec<_>>()[..] {
        let others: Vec<[f64; 3]> = (0..3).filter(|&k| k != only).map(|k| axes[k]).collect();
        return [others[0], others[1]];
    }
    let size = normal.scale();
    let unit = n.map(|v| v as f64 / size);
    // From whichever axis it's least along, square to it.
    let least = (0..3)
        .min_by(|&a, &b| unit[a].abs().total_cmp(&unit[b].abs()))
        .expect("three axes");
    let x = [0, 1, 2].map(|k| axes[least][k] - unit[least] * unit[k]);
    let length = (x[0] * x[0] + x[1] * x[1] + x[2] * x[2]).sqrt();
    let x = x.map(|v| v / length);
    let y = [
        unit[1] * x[2] - unit[2] * x[1],
        unit[2] * x[0] - unit[0] * x[2],
        unit[0] * x[1] - unit[1] * x[0],
    ];
    [x, y]
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
    /// The surface the blade came in through: the bottom face or the top for a slot sawn into
    /// a face, an edge, an end, or whatever the board rested on, a bevel, say.
    pub entry: Surface,
    /// The surface it comes out of: of the others it opens onto, the one facing most nearly
    /// away from the blade, or if it opens onto none facing away, the one across the board
    /// facing most nearly away, that it would come out of.
    pub exit: Surface,
    /// How wide it is in the board, square to its faces.
    pub width: Length,
    /// How deep it goes into the board from `entry`, at its deepest, square to it. At the
    /// board's whole size that way, it goes through.
    pub depth: Length,
    /// Whether it's at an edge or end of the board, with wood on one side of it only.
    pub at_edge: bool,
    /// How far it runs on `entry` and on `exit`, if it opens onto them.
    pub on_entry: Option<Run>,
    pub on_exit: Option<Run>,
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
    pub(crate) fn whole(board: Board) -> Piece {
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

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;
    use crate::{Blade, kerf::Across};

    /// Wood the user copied out of the game, where the cut misbehaved: a beveled 1x8 offcut
    /// with a 1x3 glued to its 15° bevel, cut in half with the blade tilted 15° and all the way
    /// up. The 1x3's top corner sat 2.8 microns beyond the blade's reach.
    const MICRONS_BRIDGE: &str = include_str!("../tests/pasted/microns-bridge.txt");

    #[test]
    fn microns_of_wood_left_bridging_a_slot_are_sawdust_and_it_comes_apart() {
        let crate::Wood::GlueUp(glue_up) = crate::Recipe::read(MICRONS_BRIDGE).unwrap().wood else {
            panic!("a glue-up");
        };
        // The 1x3, with the slot the pass cut in it.
        let one_by_three = &glue_up.members()[2].board;
        assert_eq!(one_by_three.kerfs().len(), 1);
        let pieces = one_by_three.pieces();
        assert_eq!(pieces.len(), 2, "{pieces:?}");
        // Each a little under 12" long, the kerf and the sliver of sawdust between them.
        let volume: f64 = pieces.iter().map(|piece| piece.board.volume()).sum();
        let kerf = one_by_three.kerfs()[0];
        let lost = (one_by_three.volume() - volume) / (kerf.width().as_nm() as f64);
        let face = 2.5 * 0.75 * 25_400_000f64.powi(2);
        assert!(lost > 0.99 * face && lost < 1.3 * face, "{lost} vs {face}");
    }

    fn inches(n: i64) -> Length {
        Length::from_inches(n)
    }

    fn fraction(numerator: i64, denominator: i64) -> Length {
        Length::from_inch_fraction(numerator, denominator)
    }

    const CUBIC_INCH: f64 = 25_400_000.0 * 25_400_000.0 * 25_400_000.0;

    /// A 1x6, 24" long: 3/4" x 5-1/2" x 24".
    fn one_by_six() -> Board {
        Board::new(inches(24), fraction(11, 2), fraction(3, 4))
    }

    fn scaled(length: Length, scale: f64) -> Length {
        Length::from_nm((length.as_nm() as f64 * scale).round() as i64)
    }

    /// A slot all the way along a board `thickness` thick, running `along`, by a 10" blade
    /// tilted `tilt` with a 1/8" kerf, coming up from below and reaching 1/2" past the top.
    /// Its face toward less of `along.square()` crosses the bottom face `at` across.
    fn tilted_slot(along: Direction, tilt: Angle, at: Length, thickness: Length) -> Kerf {
        let (across, lean) = along.square().tilted(tilt);
        let normal = (across.scale().powi(2) + (lean as f64).powi(2)).sqrt();
        let near = scaled(at, across.scale());
        slot(
            along,
            (across, lean),
            (near, near + scaled(Blade::FULL_KERF, normal)),
            thickness,
        )
    }

    /// A slot all the way along, facing `across` and leaning `lean`, with its faces at `faces`.
    fn slot(
        along: Direction,
        (across, lean): (Direction, i64),
        faces: (Length, Length),
        thickness: Length,
    ) -> Kerf {
        let radius = inches(5);
        let cos = across.scale() / (across.scale().powi(2) + (lean as f64).powi(2)).sqrt();
        let center = thickness + fraction(1, 2) - scaled(radius, cos);
        let reach = scaled(inches(100), along.scale());
        Kerf::tilted(across, lean, faces, along, (-reach, reach), radius, center)
    }

    /// Checks on a grid through `board` that wherever it has wood after `cuts`, exactly one of
    /// `pieces` has that wood, and nowhere else does any.
    fn pieces_hold_its_wood(board: &Board, pieces: &[Piece], steps: i64) {
        // Off from any whole fraction differently on each axis, so no point lands on a 45° face.
        let at = |size: Length, i: i64, off: i64| {
            Length::from_nm(size.as_nm() * (2 * i + 1) / (2 * steps) + off)
        };
        for i in 0..steps {
            for j in 0..steps {
                for k in 0..steps {
                    let (l, w, t) = (
                        at(board.length(), i, 7),
                        at(board.width(), j, 13),
                        at(board.thickness(), k, 3),
                    );
                    let holding = pieces
                        .iter()
                        .filter(|piece| {
                            piece
                                .board
                                .has_wood_in(l - piece.l, w - piece.w, t - piece.t)
                        })
                        .count();
                    assert_eq!(
                        holding,
                        usize::from(board.has_wood_in(l, w, t)),
                        "at {l}, {w}, {t}"
                    );
                }
            }
        }
    }

    fn cubic_inches(volume: f64) -> f64 {
        volume / CUBIC_INCH
    }

    #[test]
    fn a_tilted_rip_leaves_pieces_beveled_along_their_edges() {
        let board = one_by_six();
        let thickness = board.thickness();
        // Tilted 45°, the slot comes in going up: 2" across on the bottom face.
        let kerf = tilted_slot(
            Direction::LENGTH,
            Angle::from_degrees(45),
            inches(2),
            thickness,
        );
        assert_eq!(kerf.tilt(), Angle::from_degrees(45));
        assert_eq!(kerf.width(), Blade::FULL_KERF);
        let cut = board.with_kerf(kerf);
        let pieces = cut.pieces();
        assert_eq!(pieces.len(), 2);
        let (narrow, wide) = (&pieces[0].board, &pieces[1].board);

        // The narrow piece is 2" wide on the bottom face and 3/4" less on the top.
        assert_eq!(
            (pieces[0].l, pieces[0].w, pieces[0].t),
            (Length::ZERO, Length::ZERO, Length::ZERO)
        );
        assert_eq!(
            (narrow.length(), narrow.width(), narrow.thickness()),
            (inches(24), inches(2), thickness)
        );
        assert!(narrow.face().iter().all(|region| region.leans()));
        assert!((cubic_inches(narrow.volume()) - 24.0 * 1.625 * 0.75).abs() < 1e-6);
        // Its bevel takes 3/4" off the top: wood under it low down, none high up.
        assert!(narrow.has_wood_in(inches(12), fraction(15, 8), fraction(1, 16)));
        assert!(!narrow.has_wood_in(inches(12), fraction(15, 8), fraction(11, 16)));
        // Neither keeps the slot: it took no wood from either.
        assert!(narrow.kerfs().is_empty() && wide.kerfs().is_empty());

        // The slot is 1/8" square to its faces, so wider across the board.
        let across = 2.0 + 0.125 * 2f64.sqrt();
        assert_eq!(
            pieces[1].w,
            Length::from_nm(((across - 0.75) * 25_400_000.0).floor() as i64)
        );
        assert_eq!(wide.width() + pieces[1].w, fraction(11, 2));
        let slot = board
            .face()
            .iter()
            .filter_map(|region| region.solid(thickness).clip_all(kerf.faces()))
            .map(|region| region.solid(thickness).volume())
            .sum::<f64>();
        let total = narrow.volume() + wide.volume() + slot;
        assert!((total / board.volume() - 1.0).abs() < 1e-12);
        pieces_hold_its_wood(&cut, &pieces, 9);
    }

    #[test]
    fn a_tilted_cut_stopped_short_leaves_the_board_whole_with_its_slot() {
        let board = one_by_six();
        let thickness = board.thickness();
        let full = tilted_slot(
            Direction::LENGTH,
            Angle::from_degrees(30),
            inches(2),
            thickness,
        );
        // The blade's center stopped 6" along.
        let stopped = Kerf {
            end: inches(6),
            ..full
        };
        let cut = board.with_kerf(stopped);
        assert_eq!(cut.pieces(), [Piece::whole(cut.clone())]);
        let in_slot = |l: Length| cut.has_wood_in(l, inches(2) + fraction(1, 32), fraction(1, 32));
        assert!(!in_slot(inches(3)));
        assert!(in_slot(inches(20)));
        assert!(cut.touched_by(&stopped));
    }

    #[test]
    fn crosscutting_a_beveled_piece_square_keeps_its_bevel() {
        let board = one_by_six();
        let thickness = board.thickness();
        let rip = tilted_slot(
            Direction::LENGTH,
            Angle::from_degrees(45),
            inches(2),
            thickness,
        );
        let narrow = board.with_kerf(rip).pieces().remove(0).board;
        let reach = inches(100);
        let crosscut = Kerf::new(
            Across::Face(Direction::LENGTH),
            (inches(10), inches(10) + Blade::FULL_KERF),
            Direction::WIDTH,
            (-reach, reach),
            inches(5),
            inches(1) - inches(5),
        );
        let cut = narrow.with_kerf(crosscut);
        let pieces = cut.pieces();
        assert_eq!(pieces.len(), 2);
        let lengths: Vec<Length> = pieces.iter().map(|piece| piece.board.length()).collect();
        assert_eq!(lengths, [inches(10), inches(14) - Blade::FULL_KERF]);
        for piece in &pieces {
            assert_eq!(piece.board.width(), inches(2));
            assert!(piece.board.face().iter().all(|region| region.leans()));
            let length = piece.board.length().as_nm() as f64 / 25_400_000.0;
            assert!((cubic_inches(piece.board.volume()) - length * 1.625 * 0.75).abs() < 1e-6);
        }
        pieces_hold_its_wood(&cut, &pieces, 9);
    }

    #[test]
    fn resawing_a_beveled_piece_leaves_layers_beveled_the_same() {
        let board = one_by_six();
        let rip = tilted_slot(
            Direction::LENGTH,
            Angle::from_degrees(45),
            inches(2),
            board.thickness(),
        );
        let narrow = board.with_kerf(rip).pieces().remove(0).board;
        // On edge, a blade coming in through the edge 3" takes 3/8" to 1/2" up the thickness.
        let reach = inches(100);
        let resaw = Kerf::new(
            Across::Thickness,
            (fraction(3, 8), fraction(1, 2)),
            Direction::LENGTH,
            (-reach, reach),
            inches(5),
            inches(3) - inches(5),
        );
        let cut = narrow.with_kerf(resaw);
        let pieces = cut.pieces();
        assert_eq!(pieces.len(), 2);
        // 2" wide on the bottom face, less 3/8" by the top of the lower layer; 1-1/2" wide on
        // the bottom of the upper one and 1-1/4" on its top.
        let (low, high) = (&pieces[0], &pieces[1]);
        assert_eq!(
            (low.t, low.board.width(), low.board.thickness()),
            (Length::ZERO, inches(2), fraction(3, 8))
        );
        assert_eq!(
            (high.t, high.board.width(), high.board.thickness()),
            (fraction(1, 2), fraction(3, 2), fraction(1, 4))
        );
        let volumes = [low, high].map(|piece| cubic_inches(piece.board.volume()));
        assert!(
            (volumes[0] - 24.0 * 1.8125 * 0.375).abs() < 1e-6,
            "{volumes:?}"
        );
        assert!(
            (volumes[1] - 24.0 * 1.375 * 0.25).abs() < 1e-6,
            "{volumes:?}"
        );
        pieces_hold_its_wood(&cut, &pieces, 9);
    }

    #[test]
    fn two_tilted_rips_crossing_leave_a_ridge_below_and_a_sliver_above() {
        let board = one_by_six();
        let thickness = board.thickness();
        // One leaning each way across the board, crossing 1/2" up.
        let (across, lean) = (Direction::WIDTH, 1);
        let rising = slot(
            Direction::LENGTH,
            (across, -lean),
            (fraction(15, 8), inches(2)),
            thickness,
        );
        let falling = slot(
            Direction::LENGTH,
            (across, lean),
            (inches(3), fraction(25, 8)),
            thickness,
        );
        let cut = board.with_kerf(rising).with_kerf(falling);
        let pieces = cut.pieces();
        assert_eq!(pieces.len(), 4, "{pieces:#?}");
        let mut sizes: Vec<(Length, Length, Length)> = pieces
            .iter()
            .map(|piece| (piece.board.thickness(), piece.board.width(), piece.t))
            .collect();
        sizes.sort();
        // A sliver along the top between them, and a ridge on the bottom 1" wide to its point
        // 1/2" up, both thinner than the board.
        assert_eq!(sizes[0], (fraction(1, 8), fraction(1, 4), fraction(5, 8)));
        assert_eq!(sizes[1], (fraction(1, 2), inches(1), Length::ZERO));
        pieces_hold_its_wood(&cut, &pieces, 11);
    }

    proptest! {
        #[test]
        fn a_tilted_cut_anywhere_loses_just_the_wood_in_its_slot(
            tilt in (-4500..=4500i32).prop_filter("tilted", |&tilt| tilt != 0),
            slant in -4500..=4500i32,
            at in 1..20 * 8i64,
        ) {
            let board = one_by_six();
            let thickness = board.thickness();
            let (cos, sin) = Angle::from_hundredths(slant).unit();
            let along = Direction::new(cos, sin);
            // Across the board's middle, square to the slot.
            let middle = along.square().dot(inches(12), fraction(11, 4)) as f64 / along.scale();
            let at = Length::from_nm(middle as i64) - inches(10) + fraction(at, 8);
            let kerf = tilted_slot(along, Angle::from_hundredths(tilt), at, thickness);
            let cut = board.with_kerf(kerf);
            let pieces = cut.pieces();
            if let [whole] = pieces.as_slice()
                && whole.board == cut
            {
                // Past the board, or grazing a corner, turning a sliver to sawdust and leaving
                // a slot.
                pieces_hold_its_wood(&cut, &pieces, 5);
                return Ok(());
            }
            let slot: f64 = board
                .face()
                .iter()
                .filter_map(|region| region.solid(thickness).clip_all(kerf.faces()))
                .map(|region| region.solid(thickness).volume())
                .sum();
            let total: f64 = pieces.iter().map(|piece| piece.board.volume()).sum::<f64>() + slot;
            prop_assert!((total / board.volume() - 1.0).abs() < 1e-9, "{} of {}", total, board.volume());
            for piece in &pieces {
                for [l, w, t] in piece.board.corners() {
                    let (l, w, t) = (piece.l + l, piece.w + w, piece.t + t);
                    prop_assert!(Length::ZERO <= l && l <= board.length());
                    prop_assert!(Length::ZERO <= w && w <= board.width());
                    prop_assert!(Length::ZERO <= t && t <= thickness);
                }
            }
            pieces_hold_its_wood(&cut, &pieces, 5);
        }
    }
}
