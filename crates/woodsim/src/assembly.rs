//! Putting boards together. Each board in an assembly is turned and set with its corner at an
//! exact point. Two assemblies go together surface to surface, as the player chooses: which
//! surface of each, which of the four ways around the joint one faces the other, and where
//! across the joint it sits. Where surfaces meet or nearly do, the assembly measures how well:
//! tight, open, or running into each other.

use crate::{Board, Length, face::Edge};

/// A turn in space, as where it turns each of three axes: whole numbers over 2^40, so quarter
/// turns are exact and any other turn is within a trillionth of a radian.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Rotation {
    columns: [[i64; 3]; 3],
}

const ROTATION_UNIT: f64 = (1u64 << 40) as f64;

impl Rotation {
    pub const NONE: Rotation = Rotation {
        columns: [[1 << 40, 0, 0], [0, 1 << 40, 0], [0, 0, 1 << 40]],
    };

    /// The rotation that turns `x`, `y`, and `z` to `columns`, three directions of unit length,
    /// square to each other, and not mirrored.
    fn from_columns(columns: [[f64; 3]; 3]) -> Rotation {
        Rotation {
            columns: columns.map(|column| column.map(|v| (v * ROTATION_UNIT).round() as i64)),
        }
    }

    /// `turns` quarter turns about `x`, `y`, or `z` (`axis` 0, 1, or 2), counterclockwise
    /// looking back down it.
    pub fn quarter_turns(axis: usize, turns: i32) -> Rotation {
        let (cos, sin) = match turns.rem_euclid(4) {
            0 => (1, 0),
            1 => (0, 1),
            2 => (-1, 0),
            _ => (0, -1),
        };
        let (a, b) = ((axis + 1) % 3, (axis + 2) % 3);
        let mut columns = [[0i64; 3]; 3];
        columns[axis][axis] = 1;
        columns[a][a] = cos;
        columns[a][b] = sin;
        columns[b][a] = -sin;
        columns[b][b] = cos;
        Rotation {
            columns: columns.map(|column| column.map(|v| v << 40)),
        }
    }

    /// Where it turns `x`, `y`, and `z`.
    pub fn columns(self) -> [[f64; 3]; 3] {
        self.columns
            .map(|column| column.map(|v| v as f64 / ROTATION_UNIT))
    }

    /// `v`, turned.
    pub fn apply(self, v: [f64; 3]) -> [f64; 3] {
        let columns = self.columns();
        [0, 1, 2].map(|row| (0..3).map(|k| columns[k][row] * v[k]).sum())
    }

    /// This turn, then `next`.
    pub fn then(self, next: Rotation) -> Rotation {
        Rotation::from_columns(self.columns().map(|column| next.apply(column)))
    }

    /// The turn that undoes this one.
    pub fn inverse(self) -> Rotation {
        let c = self.columns;
        Rotation {
            columns: [0, 1, 2].map(|i| [0, 1, 2].map(|j| c[j][i])),
        }
    }

    /// How far it turns, about whatever axis it turns about, in degrees.
    pub fn degrees(self) -> f64 {
        let c = self.columns();
        ((c[0][0] + c[1][1] + c[2][2] - 1.0) / 2.0)
            .clamp(-1.0, 1.0)
            .acos()
            .to_degrees()
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

/// A board in an assembly: turned (`rotation` takes its `l`, `w`, and `t` to the assembly's
/// axes), then set with its corner (`l = w = t = 0`) at `at`, to the nanometer.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Member {
    pub board: Board,
    pub rotation: Rotation,
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

/// How one assembly goes against another, once a surface of each is chosen: `turn` is which of
/// the four ways around the joint, a quarter turn apart, it faces the other, and `slide` is how
/// far across the joint it sits, along each of the directions `Assembly::extent` gives, from
/// where the two surfaces' least sides line up.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Mating {
    pub turn: u8,
    pub slide: [Length; 2],
}

/// A move of a whole assembly: turned about its origin, then shifted.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Motion {
    pub rotation: Rotation,
    pub shift: [Length; 3],
}

/// A surface's two directions across it, which a `Mating` slides along, in its assembly's axes,
/// and how far the surface reaches along each: least and most.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Extent {
    pub directions: [[f64; 3]; 2],
    pub reach: [(Length, Length); 2],
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

impl Assembly {
    /// An assembly of one board, as it is.
    pub fn of(board: Board) -> Assembly {
        Assembly {
            members: vec![Member {
                board,
                rotation: Rotation::NONE,
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

    /// The assembly, moved as a whole.
    pub fn moved(&self, motion: Motion) -> Assembly {
        let shift = motion.shift.map(|length| length.as_nm() as f64);
        Assembly {
            members: self
                .members
                .iter()
                .map(|member| {
                    let at = motion
                        .rotation
                        .apply(member.at.map(|length| length.as_nm() as f64));
                    Member {
                        board: member.board.clone(),
                        rotation: member.rotation.then(motion.rotation),
                        at: [0, 1, 2].map(|k| Length::from_nm((at[k] + shift[k]).round() as i64)),
                    }
                })
                .collect(),
            joints: self.joints.clone(),
        }
    }

    /// The directions across `onto`, a member's surface, that a `Mating` slides along, and how
    /// far the surface reaches along them.
    pub fn extent(&self, onto: (usize, Surface)) -> Option<Extent> {
        let patch = self.members.get(onto.0)?.patch(onto.1)?;
        let reach = [patch.u, patch.v].map(|along| {
            let (least, most) = span(&patch.corners, along);
            (nm(least), nm(most))
        });
        Some(Extent {
            directions: [patch.u, patch.v],
            reach,
        })
    }

    /// How `other` moves to put its member's `surface` against `onto`, one of this assembly's,
    /// as `mating` says. `None` if either surface isn't there.
    pub fn mating(
        &self,
        onto: (usize, Surface),
        other: &Assembly,
        surface: (usize, Surface),
        mating: Mating,
    ) -> Option<Motion> {
        let theirs = self.members.get(onto.0)?.patch(onto.1)?;
        let mine = other.members.get(surface.0)?.patch(surface.1)?;
        let rotation = facing(&mine, &theirs, mating.turn);
        let turned: Vec<[f64; 3]> = mine.corners.iter().map(|&p| rotation.apply(p)).collect();
        // Along their normal, flush with their surface; across it, our least sides on theirs,
        // then slid.
        let mean = |points: &[[f64; 3]], along: [f64; 3]| {
            points.iter().map(|&p| dot(p, along)).sum::<f64>() / points.len() as f64
        };
        let shifts = [
            (
                theirs.normal,
                mean(&theirs.corners, theirs.normal) - mean(&turned, theirs.normal),
            ),
            (
                theirs.u,
                span(&theirs.corners, theirs.u).0 + mating.slide[0].as_nm() as f64
                    - span(&turned, theirs.u).0,
            ),
            (
                theirs.v,
                span(&theirs.corners, theirs.v).0 + mating.slide[1].as_nm() as f64
                    - span(&turned, theirs.v).0,
            ),
        ];
        let mut shift = [0.0f64; 3];
        for (direction, amount) in shifts {
            for k in 0..3 {
                shift[k] += amount * direction[k];
            }
        }
        Some(Motion {
            rotation,
            shift: shift.map(nm),
        })
    }

    /// Where `other`'s `surface` can sit across `onto` facing it the `turn`th way, along each
    /// of the joint's two directions, as a `Mating`'s slide: flush with `onto`'s least side,
    /// centered on it, and flush with its most side.
    pub fn stops(
        &self,
        onto: (usize, Surface),
        other: &Assembly,
        surface: (usize, Surface),
        turn: u8,
    ) -> Option<[[Length; 3]; 2]> {
        let theirs = self.members.get(onto.0)?.patch(onto.1)?;
        let mine = other.members.get(surface.0)?.patch(surface.1)?;
        let rotation = facing(&mine, &theirs, turn);
        let turned: Vec<[f64; 3]> = mine.corners.iter().map(|&p| rotation.apply(p)).collect();
        Some([theirs.u, theirs.v].map(|along| {
            let (least, most) = span(&theirs.corners, along);
            let (low, high) = span(&turned, along);
            let room = (most - least) - (high - low);
            [0.0, room / 2.0, room].map(nm)
        }))
    }

    /// Adds `other`, moved where `mating` puts it, with its member's `surface` against `onto`,
    /// held by `fastener`. Its members follow this assembly's, in the same order. Returns false,
    /// leaving the assembly as it was, if either surface isn't there.
    pub fn attach(
        &mut self,
        onto: (usize, Surface),
        other: &Assembly,
        surface: (usize, Surface),
        mating: Mating,
        fastener: Fastener,
    ) -> bool {
        let Some(motion) = self.mating(onto, other, surface, mating) else {
            return false;
        };
        let moved = other.moved(motion);
        let first = self.members.len();
        self.members.extend(moved.members);
        self.joints.extend(moved.joints.iter().map(|joint| {
            Joint {
                between: joint
                    .between
                    .map(|(member, surface)| (member + first, surface)),
                fastener: joint.fastener,
            }
        }));
        self.joints.push(Joint {
            between: [onto, (surface.0 + first, surface.1)],
            fastener,
        });
        true
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
    /// Where each of the board's axes points in the assembly: `l`, `w`, and `t`.
    pub fn axes(&self) -> [[f64; 3]; 3] {
        self.rotation.columns()
    }

    /// One of the board's surfaces, in the assembly.
    fn patch(&self, surface: Surface) -> Option<Patch> {
        let local = patch(&self.board, surface)?;
        let at = self.at.map(|length| length.as_nm() as f64);
        let turn = |v| self.rotation.apply(v);
        Some(Patch {
            normal: turn(local.normal),
            u: turn(local.u),
            v: turn(local.v),
            corners: local
                .corners
                .iter()
                .map(|&p| {
                    let p = turn(p);
                    [p[0] + at[0], p[1] + at[1], p[2] + at[2]]
                })
                .collect(),
        })
    }

    /// Where the point `(l, w, t)` of the board is in the assembly, to the nanometer.
    pub fn point(&self, l: Length, w: Length, t: Length) -> [Length; 3] {
        let p = self
            .rotation
            .apply([l, w, t].map(|length| length.as_nm() as f64));
        [0, 1, 2].map(|k| self.at[k] + nm(p[k]))
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

/// The outline of one of `board`'s surfaces, in its own axes (`l`, `w`, `t`): flat polygons,
/// each going around counterclockwise seen from outside the board. Empty if it has no such
/// surface.
pub fn outline(board: &Board, surface: Surface) -> Vec<Vec<[Length; 3]>> {
    let thickness = board.thickness();
    match surface {
        Surface::Bottom | Surface::Top => board
            .face()
            .iter()
            .map(|region| {
                let t = if surface == Surface::Top {
                    thickness
                } else {
                    Length::ZERO
                };
                let mut corners: Vec<[Length; 3]> = region
                    .corners()
                    .into_iter()
                    .map(|(l, w)| [l, w, t])
                    .collect();
                if surface == Surface::Bottom {
                    corners.reverse();
                }
                corners
            })
            .collect(),
        Surface::Side(side) => board
            .face()
            .iter()
            .flat_map(|region| {
                let edges = region.edges();
                let points = region.corners();
                let n = edges.len();
                (0..n).filter(move |&i| edges[i] == side).map(move |i| {
                    let ((l0, w0), (l1, w1)) = (points[i], points[(i + 1) % n]);
                    vec![
                        [l0, w0, Length::ZERO],
                        [l1, w1, Length::ZERO],
                        [l1, w1, thickness],
                        [l0, w0, thickness],
                    ]
                })
            })
            .collect(),
    }
}

/// A flat surface: the way it faces (out of the board), two directions across it (`u` and
/// `v`, turning counterclockwise from one to the other seen from outside), and its corners, in
/// nanometers.
struct Patch {
    normal: [f64; 3],
    u: [f64; 3],
    v: [f64; 3],
    corners: Vec<[f64; 3]>,
}

/// One of `board`'s surfaces, in its own axes: `l`, `w`, `t`. Across a face, `u` runs along
/// the length; across a side, along the side, with `v` up the thickness.
fn patch(board: &Board, surface: Surface) -> Option<Patch> {
    let corners: Vec<[f64; 3]> = outline(board, surface)
        .into_iter()
        .flatten()
        .map(|p| p.map(|length| length.as_nm() as f64))
        .collect();
    if corners.is_empty() {
        return None;
    }
    let (normal, u, v) = match surface {
        Surface::Bottom => ([0.0, 0.0, -1.0], [1.0, 0.0, 0.0], [0.0, -1.0, 0.0]),
        Surface::Top => ([0.0, 0.0, 1.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]),
        Surface::Side(side) => {
            let n = unit([side.out.l as f64, side.out.w as f64, 0.0]);
            (n, [-n[1], n[0], 0.0], [0.0, 0.0, 1.0])
        }
    };
    Some(Patch {
        normal,
        u,
        v,
        corners,
    })
}

/// The turn that brings `mine` around to face back against `theirs`, the `turn`th of the four
/// ways around: each a quarter turn further about their normal.
fn facing(mine: &Patch, theirs: &Patch, turn: u8) -> Rotation {
    let (u, v) = (theirs.u, theirs.v);
    let back = |d: [f64; 3]| d.map(|c| -c);
    let [a, b] = match turn % 4 {
        0 => [u, back(v)],
        1 => [v, u],
        2 => [back(u), v],
        _ => [back(v), back(u)],
    };
    let to = [back(theirs.normal), a, b];
    let from = [mine.normal, mine.u, mine.v];
    Rotation::from_columns(
        [0, 1, 2].map(|j| [0, 1, 2].map(|row| (0..3).map(|i| to[i][row] * from[i][j]).sum())),
    )
}

/// The least and most of `points` along `along`.
fn span(points: &[[f64; 3]], along: [f64; 3]) -> (f64, f64) {
    points
        .iter()
        .map(|&p| dot(p, along))
        .fold((f64::MAX, f64::MIN), |(low, high), x| {
            (low.min(x), high.max(x))
        })
}

/// A distance in nanometers, to the nearest one.
fn nm(x: f64) -> Length {
    Length::from_nm(x.round() as i64)
}

fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

fn unit(v: [f64; 3]) -> [f64; 3] {
    let size = dot(v, v).sqrt();
    v.map(|c| c / size)
}

/// How nearly `mine` faces straight back against `theirs`: 1 when exactly.
fn facing_back(mine: [f64; 3], theirs: [f64; 3]) -> f64 {
    -dot(unit(mine), unit(theirs))
}

/// How far apart `b` is from `a`, along `a`'s normal, at the corners of where they overlap
/// across it, if they face each other and are near: least and most, negative where `b` runs
/// into `a`.
fn gap(a: &Patch, b: &Patch) -> Option<(Length, Length)> {
    if facing_back(b.normal, a.normal) < MOST_TILT_DEGREES.to_radians().cos() {
        return None;
    }
    let n = unit(a.normal);
    // Two directions across the surface.
    let helper = if n[0].abs() < 0.9 {
        [1.0, 0.0, 0.0]
    } else {
        [0.0, 1.0, 0.0]
    };
    let u = unit(cross(n, helper));
    let v = unit(cross(n, u));
    let (au, bu) = (span(&a.corners, u), span(&b.corners, u));
    let (av, bv) = (span(&a.corners, v), span(&b.corners, v));
    let across = (au.0.max(bu.0), au.1.min(bu.1));
    let up = (av.0.max(bv.0), av.1.min(bv.1));
    // Overlapping by more than a micron each way, not just touching along an edge.
    if across.1 - across.0 < 1_000.0 || up.1 - up.0 < 1_000.0 {
        return None;
    }
    let plane_a = dot(a.corners[0], n);
    let nb = unit(b.normal);
    let plane_b = dot(b.corners[0], nb);
    // At each corner of the overlap on `a`, how far out along `a`'s normal `b`'s plane is.
    let mut distances = Vec::new();
    for x in [across.0, across.1] {
        for y in [up.0, up.1] {
            let on_a = [0, 1, 2].map(|k| u[k] * x + v[k] * y + n[k] * plane_a);
            distances.push((plane_b - dot(on_a, nb)) / dot(n, nb));
        }
    }
    let least = distances.iter().copied().fold(f64::MAX, f64::min);
    let most = distances.iter().copied().fold(f64::MIN, f64::max);
    let near = NEAR.as_nm() as f64;
    (least <= near && most >= -near).then(|| (nm(least), nm(most)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn board() -> Board {
        Board::new(
            Length::from_inches(24),
            Length::from_inches(4),
            Length::from_inch_fraction(3, 4),
        )
    }

    #[test]
    fn quarter_turns_are_exact_and_four_go_all_the_way_around() {
        for axis in 0..3 {
            let quarter = Rotation::quarter_turns(axis, 1);
            assert_eq!(quarter.degrees(), 90.0);
            let around = (0..4).fold(Rotation::NONE, |turned, _| turned.then(quarter));
            assert_eq!(around, Rotation::NONE);
            assert_eq!(quarter.then(quarter.inverse()), Rotation::NONE);
        }
        // A quarter turn about z takes x to y.
        assert_eq!(
            Rotation::quarter_turns(2, 1).apply([1.0, 0.0, 0.0]),
            [0.0, 1.0, 0.0]
        );
    }

    #[test]
    fn a_plain_board_has_six_surfaces_each_with_one_outline() {
        let board = board();
        let surfaces = surfaces(&board);
        assert_eq!(surfaces.len(), 6);
        for surface in surfaces {
            assert_eq!(outline(&board, surface).len(), 1);
            // Its directions across lie flat on it, square to the way it faces.
            let patch = patch(&board, surface).unwrap();
            assert_eq!(cross(patch.u, patch.v), patch.normal);
        }
    }

    #[test]
    fn the_four_ways_around_a_joint_are_a_quarter_turn_apart() {
        let board = board();
        let assembly = Assembly::of(board.clone());
        let ways: Vec<Rotation> = (0..4)
            .map(|turn| {
                assembly
                    .mating(
                        (0, Surface::Top),
                        &assembly,
                        (0, Surface::Bottom),
                        Mating {
                            turn,
                            slide: [Length::ZERO; 2],
                        },
                    )
                    .unwrap()
                    .rotation
            })
            .collect();
        // Bottom on top, the first way keeps it as it was; each next way is a quarter turn
        // further about the joint, which here is straight up.
        assert_eq!(ways[0], Rotation::NONE);
        for turn in 1..4 {
            assert_eq!(
                ways[turn - 1].then(Rotation::quarter_turns(2, 1)),
                ways[turn]
            );
        }
    }
}
