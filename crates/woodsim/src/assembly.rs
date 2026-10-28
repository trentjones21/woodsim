//! Putting boards together: each board in an assembly is turned square one of 24 ways and set
//! with its corner at an exact point, and boards go together surface to surface: an end
//! against a face, a miter against a miter. Where surfaces meet or nearly do, the assembly
//! measures how well: tight, open, or running into each other.

use crate::{Board, Length, face::Edge};

/// A point or a direction in an assembly, along its `x`, `y`, and `z`.
pub type Vector = [i128; 3];

/// One of the 24 ways to turn a board square without mirroring it: each of the board's axes
/// (`l`, `w`, `t`) onto one of the assembly's, one way or the other.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Turn {
    /// Where each of the board's axes points in the assembly: `columns[0]` for `l`, `[1]` for
    /// `w`, `[2]` for `t`.
    columns: [[i8; 3]; 3],
}

impl Turn {
    /// Not turned: `l`, `w`, and `t` along `x`, `y`, and `z`.
    pub const NONE: Turn = Turn {
        columns: [[1, 0, 0], [0, 1, 0], [0, 0, 1]],
    };

    /// All 24 turns.
    pub fn all() -> impl Iterator<Item = Turn> {
        let axes = [[1, 0, 0], [0, 1, 0], [0, 0, 1]];
        let signed = move |axis: usize, sign: i8| axes[axis].map(|v: i8| v * sign);
        (0..3)
            .flat_map(move |l| (0..3).filter(move |&w| w != l).map(move |w| (l, w)))
            .flat_map(move |(l, w)| {
                [1, -1].into_iter().flat_map(move |sl| {
                    [1, -1].into_iter().map(move |sw| {
                        let (l, w) = (signed(l, sl), signed(w, sw));
                        // The thickness is square to both, the way that doesn't mirror.
                        let t = [
                            l[1] * w[2] - l[2] * w[1],
                            l[2] * w[0] - l[0] * w[2],
                            l[0] * w[1] - l[1] * w[0],
                        ];
                        Turn { columns: [l, w, t] }
                    })
                })
            })
    }

    /// Where each of the board's axes points in the assembly: `l`, `w`, and `t`.
    pub fn columns(self) -> [[i8; 3]; 3] {
        self.columns
    }

    /// `v`, in the board's own axes, in the assembly's.
    pub fn apply(self, v: Vector) -> Vector {
        let mut out = [0; 3];
        for (axis, column) in self.columns.iter().enumerate() {
            for (row, &entry) in column.iter().enumerate() {
                out[row] += entry as i128 * v[axis];
            }
        }
        out
    }
}

/// How a joint is held together.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Fastener {
    Glue,
    Brads,
    Screws,
}

/// A flat surface of a board you could put against another: its bottom or top face, or one of
/// its sides through the thickness, named by the straight edge of its outline it stands on (an
/// end, an edge, or a miter).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Surface {
    Bottom,
    Top,
    Side(Edge),
}

/// A board in an assembly: turned, then set with its corner (`l = w = t = 0`) at `at`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Member {
    pub board: Board,
    pub turn: Turn,
    pub at: [Length; 3],
}

/// Two surfaces held together.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Joint {
    /// Which members, by their place in the assembly, and which of their surfaces.
    pub between: [(usize, Surface); 2],
    pub fastener: Fastener,
}

/// Where two members' surfaces meet or nearly do, and how far apart they are across where they
/// overlap: from `least` to `most`. Negative where one runs into the other.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Contact {
    pub between: [(usize, Surface); 2],
    pub least: Length,
    pub most: Length,
}

impl Contact {
    /// Whether the surfaces are flush all across where they meet, to within a micron.
    pub fn is_tight(&self) -> bool {
        self.least.as_nm().abs() < 1_000 && self.most.as_nm().abs() < 1_000
    }
}

/// Surfaces this far apart, or this far into each other, still count as meeting.
const NEAR: Length = Length::from_inch_fraction(1, 8);
/// Surfaces turned more than this from facing each other don't meet.
const MOST_TILT_DEGREES: f64 = 10.0;

/// Boards put together.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Assembly {
    members: Vec<Member>,
    joints: Vec<Joint>,
}

/// One of the four corners of a surface to line another up against, around it from where both
/// of its directions are least.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Corner(pub u8);

impl Assembly {
    /// An assembly of one board, as it is.
    pub fn of(board: Board) -> Assembly {
        Assembly {
            members: vec![Member {
                board,
                turn: Turn::NONE,
                at: [Length::ZERO; 3],
            }],
            joints: Vec::new(),
        }
    }

    pub fn members(&self) -> &[Member] {
        &self.members
    }

    pub fn joints(&self) -> &[Joint] {
        &self.joints
    }

    /// Where `board` goes to put its `surface` against `onto`, a member's surface: turned so the
    /// two face each other as nearly as they can (the `way`th of the turns that do, keeping the
    /// board face up like the member first), then slid flush against it with their `corner`s
    /// lined up. If the surfaces aren't quite parallel, it touches at one point and opens from
    /// there. `None` if the surfaces can't face each other at all.
    pub fn place(
        &self,
        board: &Board,
        surface: Surface,
        onto: (usize, Surface),
        way: usize,
        corner: Corner,
    ) -> Option<Member> {
        let target = self.members.get(onto.0)?;
        let at = target.patch(onto.1)?;
        let mine = patch(board, surface)?;
        let ways = ways_to_face(mine.normal, at.normal, target.turn);
        let turn = *ways.get(way % ways.len().max(1))?;

        let corners: Vec<Vector> = mine.corners.iter().map(|&p| turn.apply(p)).collect();
        let (u, v) = in_plane(at.normal, target.turn, onto.1);
        let pick = |points: &[Vector], along: Vector, high: bool| {
            let values = points.iter().map(|&p| dot(along, p));
            if high { values.max() } else { values.min() }.expect("a surface has corners")
        };
        let (high_u, high_v) = match corner.0 % 4 {
            0 => (false, false),
            1 => (true, false),
            2 => (true, true),
            _ => (false, true),
        };
        // Along the target's normal, up to where the nearest point touches; across it, the
        // chosen corners together.
        let normal = at.normal;
        let plane = dot(normal, at.corners[0]);
        let shift = [
            (normal, plane - pick(&corners, normal, false)),
            (u, pick(&at.corners, u, high_u) - pick(&corners, u, high_u)),
            (v, pick(&at.corners, v, high_v) - pick(&corners, v, high_v)),
        ];
        let mut offset = [0.0f64; 3];
        for (direction, amount) in shift {
            let scale = dot(direction, direction) as f64;
            for k in 0..3 {
                offset[k] += amount as f64 * direction[k] as f64 / scale;
            }
        }
        Some(Member {
            board: board.clone(),
            turn,
            at: offset.map(|nm| Length::from_nm(nm.round() as i64)),
        })
    }

    /// Adds `member`, put against another's surface as `place` worked out, held by
    /// `fastener`.
    pub fn join(
        &mut self,
        member: Member,
        surface: Surface,
        onto: (usize, Surface),
        fastener: Fastener,
    ) {
        self.members.push(member);
        self.joints.push(Joint {
            between: [onto, (self.members.len() - 1, surface)],
            fastener,
        });
    }

    /// Takes the last member back out, with the joints that hold it.
    pub fn remove_last(&mut self) -> Option<Member> {
        if self.members.len() <= 1 {
            return None;
        }
        let last = self.members.len() - 1;
        self.joints
            .retain(|joint| joint.between.iter().all(|(member, _)| *member != last));
        self.members.pop()
    }

    /// Every place two members' surfaces meet or nearly do, facing each other, with how far
    /// apart they are.
    pub fn fit(&self) -> Vec<Contact> {
        let mut contacts = Vec::new();
        for (i, first) in self.members.iter().enumerate() {
            for (j, second) in self.members.iter().enumerate().skip(i + 1) {
                for a in surfaces(&first.board) {
                    for b in surfaces(&second.board) {
                        if let (Some(pa), Some(pb)) = (first.patch(a), second.patch(b))
                            && let Some((least, most)) = gap(&pa, &pb)
                        {
                            contacts.push(Contact {
                                between: [(i, a), (j, b)],
                                least,
                                most,
                            });
                        }
                    }
                }
            }
        }
        contacts
    }
}

impl Member {
    /// One of the board's surfaces, in the assembly.
    fn patch(&self, surface: Surface) -> Option<Patch> {
        let local = patch(&self.board, surface)?;
        let at = self.at.map(|length| length.as_nm() as i128);
        Some(Patch {
            normal: self.turn.apply(local.normal),
            corners: local
                .corners
                .iter()
                .map(|&p| {
                    let p = self.turn.apply(p);
                    [p[0] + at[0], p[1] + at[1], p[2] + at[2]]
                })
                .collect(),
        })
    }

    /// Where the point `(l, w, t)` of the board is in the assembly.
    pub fn point(&self, l: Length, w: Length, t: Length) -> [Length; 3] {
        let p = self
            .turn
            .apply([l, w, t].map(|length| length.as_nm() as i128));
        [0, 1, 2].map(|k| self.at[k] + Length::from_nm(p[k] as i64))
    }
}

/// The flat surfaces of `board`: its bottom, its top, and each straight side around its outline.
pub fn surfaces(board: &Board) -> Vec<Surface> {
    let edges: Vec<Edge> = board
        .face()
        .iter()
        .flat_map(|region| region.edges())
        .copied()
        .collect();
    let mut sides: Vec<Edge> = edges
        .iter()
        .filter(|edge| !edges.contains(&edge.flipped()))
        .copied()
        .collect();
    sides.sort();
    sides.dedup();
    [Surface::Bottom, Surface::Top]
        .into_iter()
        .chain(sides.into_iter().map(Surface::Side))
        .collect()
}

/// A flat surface, as the way it faces (out of the board, in whole numbers with no common
/// factor) and its corners.
struct Patch {
    normal: Vector,
    corners: Vec<Vector>,
}

/// One of `board`'s surfaces, in its own axes: `l`, `w`, `t`.
fn patch(board: &Board, surface: Surface) -> Option<Patch> {
    let nm = |length: Length| length.as_nm() as i128;
    let face_corners = |t: Length| -> Vec<Vector> {
        board
            .face()
            .iter()
            .flat_map(|region| region.corners())
            .map(|(l, w)| [nm(l), nm(w), nm(t)])
            .collect()
    };
    match surface {
        Surface::Bottom => Some(Patch {
            normal: [0, 0, -1],
            corners: face_corners(Length::ZERO),
        }),
        Surface::Top => Some(Patch {
            normal: [0, 0, 1],
            corners: face_corners(board.thickness()),
        }),
        Surface::Side(side) => {
            let mut corners = Vec::new();
            for region in board.face() {
                let edges = region.edges();
                let points = region.corners();
                let n = edges.len();
                for i in (0..n).filter(|&i| edges[i] == side) {
                    for (l, w) in [points[i], points[(i + 1) % n]] {
                        for t in [Length::ZERO, board.thickness()] {
                            corners.push([nm(l), nm(w), nm(t)]);
                        }
                    }
                }
            }
            (!corners.is_empty()).then_some(Patch {
                normal: [side.out.l as i128, side.out.w as i128, 0],
                corners,
            })
        }
    }
}

fn dot(a: Vector, b: Vector) -> i128 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn cross(a: Vector, b: Vector) -> Vector {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

fn length(a: Vector) -> f64 {
    (dot(a, a) as f64).sqrt()
}

/// How nearly `mine`, turned, faces straight back against `theirs`: 1 when exactly.
fn facing(mine: Vector, theirs: Vector) -> f64 {
    -(dot(mine, theirs) as f64) / (length(mine) * length(theirs))
}

/// The turns that put a surface facing `mine` back against one facing `theirs` on a member
/// turned `target`, nearest to facing first; among equals, those that keep the board's top up
/// the same way as the member's, then in a fixed order.
fn ways_to_face(mine: Vector, theirs: Vector, target: Turn) -> Vec<Turn> {
    let least = MOST_TILT_DEGREES.to_radians().cos();
    let up = target.apply([0, 0, 1]);
    let mut ways: Vec<(Turn, f64)> = Turn::all()
        .map(|turn| (turn, facing(turn.apply(mine), theirs)))
        .filter(|&(_, how)| how >= least)
        .collect();
    // To a billionth, so turns that face exactly alike tie.
    let nearness = |how: f64| (how * 1e9).round() as i64;
    let upside_down = |turn: Turn| turn.apply([0, 0, 1]) != up;
    ways.sort_by_key(|&(turn, how)| (-nearness(how), upside_down(turn)));
    ways.into_iter().map(|(turn, _)| turn).collect()
}

/// Two directions square to `normal` and each other, along a surface of a member turned
/// `turn`: on a side, along the side and up the board's thickness; on a face, along the
/// board's length and width.
fn in_plane(normal: Vector, turn: Turn, surface: Surface) -> (Vector, Vector) {
    match surface {
        Surface::Side(_) => {
            let up = turn.apply([0, 0, 1]);
            (cross(normal, up), up)
        }
        Surface::Bottom | Surface::Top => (turn.apply([1, 0, 0]), turn.apply([0, 1, 0])),
    }
}

/// How far apart `b` is from `a`, along `a`'s normal, at the corners of where they overlap
/// across it, if they face each other and are near: least and most, negative where `b` runs
/// into `a`.
fn gap(a: &Patch, b: &Patch) -> Option<(Length, Length)> {
    if facing(b.normal, a.normal) < MOST_TILT_DEGREES.to_radians().cos() {
        return None;
    }
    let unit = |v: Vector| {
        let size = length(v);
        v.map(|c| c as f64 / size)
    };
    let n = unit(a.normal);
    // Two directions across the surface.
    let helper = if n[0].abs() < 0.9 {
        [1, 0, 0]
    } else {
        [0, 1, 0]
    };
    let u = unit(cross(a.normal, helper));
    let u_whole = cross(a.normal, helper);
    let v = unit(cross(a.normal, u_whole));
    let as_f64 = |p: Vector| p.map(|c| c as f64);
    let dotf = |x: [f64; 3], y: [f64; 3]| x[0] * y[0] + x[1] * y[1] + x[2] * y[2];
    let span = |patch: &Patch, along: [f64; 3]| {
        patch
            .corners
            .iter()
            .map(|&p| dotf(as_f64(p), along))
            .fold((f64::MAX, f64::MIN), |(low, high), x| {
                (low.min(x), high.max(x))
            })
    };
    let (au, bu) = (span(a, u), span(b, u));
    let (av, bv) = (span(a, v), span(b, v));
    let across = (au.0.max(bu.0), au.1.min(bu.1));
    let up = (av.0.max(bv.0), av.1.min(bv.1));
    // Overlapping by more than a micron each way, not just touching along an edge.
    if across.1 - across.0 < 1_000.0 || up.1 - up.0 < 1_000.0 {
        return None;
    }
    let plane_a = dotf(as_f64(a.corners[0]), n);
    let nb = unit(b.normal);
    let plane_b = dotf(as_f64(b.corners[0]), nb);
    // At each corner of the overlap on `a`, how far out along `a`'s normal `b`'s plane is.
    let mut distances = Vec::new();
    for x in [across.0, across.1] {
        for y in [up.0, up.1] {
            let on_a = [0, 1, 2].map(|k| u[k] * x + v[k] * y + n[k] * plane_a);
            let out = (plane_b - dotf(on_a, nb)) / dotf(n, nb);
            distances.push(out);
        }
    }
    let least = distances.iter().copied().fold(f64::MAX, f64::min);
    let most = distances.iter().copied().fold(f64::MIN, f64::max);
    let near = NEAR.as_nm() as f64;
    (least <= near && most >= -near).then(|| {
        let nm = |x: f64| Length::from_nm(x.round() as i64);
        (nm(least), nm(most))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn there_are_twenty_four_turns_and_none_mirror() {
        let turns: Vec<Turn> = Turn::all().collect();
        assert_eq!(turns.len(), 24);
        assert!(turns.contains(&Turn::NONE));
        for turn in turns {
            let [l, w, t] = turn.columns().map(|c| c.map(|v| v as i128));
            assert_eq!(cross(l, w), t);
        }
    }

    #[test]
    fn a_plain_board_has_six_surfaces() {
        let board = Board::new(
            Length::from_inches(24),
            Length::from_inches(4),
            Length::from_inch_fraction(3, 4),
        );
        assert_eq!(surfaces(&board).len(), 6);
    }
}
