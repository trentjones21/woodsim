//! Putting boards together. Each board in an assembly is turned and set with its corner at an
//! exact point. Two assemblies go together surface to surface, as the player chooses: which
//! surface of each, which of the four ways around the joint one faces the other, and where
//! across the joint it sits. Where surfaces meet or nearly do, the assembly measures how well:
//! tight, open, or running into each other.

use crate::{
    Angle, Board, Length, Orientation,
    exposed::{exposed, slot_shapes},
    face::Edge,
    history::{Copies, next_number},
};

/// Why something can't rest on a saw the way it was asked to (`Assembly::resting`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Unsteady {
    /// One of the surfaces isn't there.
    NoSurface,
    /// Wood reaches past the surface it was to rest on, so it wouldn't sit on it.
    Rocks,
    /// The surface to go against the fence faces straight up or down.
    FacesUp,
    /// Wood reaches past the surface to go against the fence, toward it, so it would meet the
    /// fence first.
    Blocked,
}

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

    /// `angle` about `axis`, counterclockwise looking back down it: any turn, as near as
    /// whole numbers over 2^40 come.
    pub fn about(axis: [f64; 3], angle: Angle) -> Rotation {
        let size = (axis[0] * axis[0] + axis[1] * axis[1] + axis[2] * axis[2]).sqrt();
        let [x, y, z] = axis.map(|c| c / size);
        let (sin, cos) = angle.radians().sin_cos();
        let t = 1.0 - cos;
        Rotation::from_columns([
            [t * x * x + cos, t * x * y + sin * z, t * x * z - sin * y],
            [t * x * y - sin * z, t * y * y + cos, t * y * z + sin * x],
            [t * x * z + sin * y, t * y * z - sin * x, t * z * z + cos],
        ])
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

    /// Its whole numbers over 2^40: where it turns `x`, `y`, and `z`.
    pub(crate) fn raw(self) -> [[i64; 3]; 3] {
        self.columns
    }

    /// The turn whose whole numbers over 2^40 are `columns`.
    pub(crate) fn from_raw(columns: [[i64; 3]; 3]) -> Rotation {
        Rotation { columns }
    }

    /// Whether it turns each axis exactly onto another, one way or the other: a square turn.
    pub(crate) fn is_square(self) -> bool {
        self.columns
            .iter()
            .flatten()
            .all(|&v| v == 0 || v.abs() == 1 << 40)
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

/// A flat surface of a board you could put against another: its bottom or top face, one of its
/// sides through the thickness, named by the straight edge of its outline it stands on (an end,
/// an edge, or a miter), or a surface a slot left inside its outline.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Surface {
    Bottom,
    Top,
    Side(Edge),
    /// One of a slot's sides, or its bottom where the blade's teeth ran level, named by the
    /// plane it's on, with the wood behind it as an `Edge` has it: facing up or down, like a
    /// groove's bottom, it has no `out` and all `lean`. Slots that share a plane, like the
    /// strokes of a dado nibbled out, share the surface (`slot_surfaces`).
    Slot(Edge),
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
#[derive(Clone, Copy, Debug)]
pub struct Joint {
    /// Which members, by their place in the assembly, and which of their surfaces.
    pub between: [(usize, Surface); 2],
    pub fastener: Fastener,
    /// When it was fastened, in the order saw passes and joints happen: every joint fastened at
    /// once (where a part went on, and wherever else it met flush) has the same number.
    pub number: u64,
}

/// The same joint is the same surfaces held the same way, whenever it was made.
impl PartialEq for Joint {
    fn eq(&self, other: &Joint) -> bool {
        (self.between, self.fastener) == (other.between, other.fastener)
    }
}

impl Eq for Joint {}

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

    /// Another just like it, made the same way from new lumber of its own (as
    /// `Board::duplicate`): boards that came from the same stock still do, but not the stock
    /// this one's boards came from, and every pass and joint is numbered after everything so
    /// far, in the order they were, joints fastened at once still sharing a number.
    pub fn duplicate(&self) -> Assembly {
        let passes = self
            .members
            .iter()
            .flat_map(|member| member.board.origin().operations())
            .map(|operation| operation.number);
        let joints = self.joints.iter().map(|joint| joint.number);
        let mut copies = Copies::numbering(passes.chain(joints));
        Assembly {
            members: self
                .members
                .iter()
                .map(|member| Member {
                    board: member
                        .board
                        .clone()
                        .with_origin(copies.origin(member.board.origin())),
                    ..*member
                })
                .collect(),
            joints: self
                .joints
                .iter()
                .map(|joint| Joint {
                    number: copies.number(joint.number),
                    ..*joint
                })
                .collect(),
        }
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

    /// The box its members fill, along the assembly's `x`, `y`, and `z`: least and most each
    /// way, from the corners of their faces.
    pub fn bounds(&self) -> [(Length, Length); 3] {
        let mut bounds = [(Length::from_nm(i64::MAX), Length::from_nm(i64::MIN)); 3];
        for member in &self.members {
            for surface in [Surface::Bottom, Surface::Top] {
                for [l, w, t] in outline(&member.board, surface).into_iter().flatten() {
                    let point = member.point(l, w, t);
                    for (bound, at) in bounds.iter_mut().zip(point) {
                        *bound = (bound.0.min(at), bound.1.max(at));
                    }
                }
            }
        }
        bounds
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
    /// held by `fastener`. Every other place one of its members now meets one of ours flush is
    /// fastened too, the way a glue-up gets glue on every face that meets: the last piece of a
    /// frame is glued at both its corners. A place that's open, even a little, isn't. Its
    /// members follow this assembly's, in the same order. Returns false, leaving the assembly
    /// as it was, if either surface isn't there.
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
                ..*joint
            }
        }));
        let chosen = [onto, (surface.0 + first, surface.1)];
        let number = next_number();
        self.joints.push(Joint {
            between: chosen,
            fastener,
            number,
        });
        let flush: Vec<Joint> = self
            .contacts(|i, j| i < first && j >= first)
            .into_iter()
            .filter(|contact| contact.is_tight() && contact.between != chosen)
            .map(|contact| Joint {
                between: contact.between,
                fastener,
                number,
            })
            .collect();
        self.joints.extend(flush);
        true
    }

    /// Every place two members' surfaces meet or nearly do, facing each other, with how far
    /// apart they are.
    /// The way a member's surface faces, out of its wood, in the assembly's axes, as a unit
    /// vector: `None` if the member or the surface isn't there.
    pub fn facing(&self, (member, surface): (usize, Surface)) -> Option<[f64; 3]> {
        Some(self.members.get(member)?.patch(surface)?.normal)
    }

    /// What it rests on, lying on a saw as `lie` says, and what faces the fence: the surface
    /// facing straight down that's lowest, and the one facing most nearly toward the fence.
    pub fn rests_on(&self, lie: Orientation) -> Option<((usize, Surface), (usize, Surface))> {
        let rotation = lie.rotation();
        let all: Vec<((usize, Surface), [f64; 3], f64)> = self
            .members
            .iter()
            .enumerate()
            .flat_map(|(index, member)| {
                surfaces(&member.board)
                    .into_iter()
                    .filter_map(move |surface| {
                        let patch = member.patch(surface)?;
                        let low = patch
                            .corners
                            .iter()
                            .map(|&p| rotation.apply(p)[2])
                            .fold(f64::MAX, f64::min);
                        Some(((index, surface), rotation.apply(patch.normal), low))
                    })
            })
            .collect();
        let down = all
            .iter()
            .filter(|(_, normal, _)| normal[2] < -1.0 + 1e-9)
            .min_by(|a, b| a.2.total_cmp(&b.2))?
            .0;
        let fence = all.iter().max_by(|a, b| a.1[1].total_cmp(&b.1[1]))?.0;
        Some((down, fence))
    }

    /// How it lies on a saw resting on `down`, flat on the table, and turned so `against`
    /// faces the fence (or the miter gauge's face, or the miter saw's): as squarely as it
    /// can, so a surface square to `down` lies flat against the fence, and one that leans,
    /// like a bevel, touches it along its edge. A board on its own rests the same way as a
    /// glue-up of one (`Assembly::of`).
    pub fn resting(
        &self,
        down: (usize, Surface),
        against: (usize, Surface),
    ) -> Result<Orientation, Unsteady> {
        let patch = |(member, surface): (usize, Surface)| self.members.get(member)?.patch(surface);
        let (on, to) = (
            patch(down).ok_or(Unsteady::NoSurface)?,
            patch(against).ok_or(Unsteady::NoSurface)?,
        );
        // Down onto the table, and the rest of the way toward the fence, level.
        let d = unit(on.normal);
        let level = dot(to.normal, d);
        let f = [0, 1, 2].map(|k| to.normal[k] - level * d[k]);
        if dot(f, f) < 1e-12 {
            return Err(Unsteady::FacesUp);
        }
        let f = unit(f);
        let x = cross(d, f);
        let rotation = Rotation::from_columns([0, 1, 2].map(|k| [x[k], f[k], -d[k]]));
        // Nothing reaches past the surface it rests on, nor past the one to the fence: within
        // a micron, so a surface cut at a slant whole-number directions only come near counts.
        let corners: Vec<[f64; 3]> = self
            .members
            .iter()
            .flat_map(|member| {
                member
                    .board
                    .corners()
                    .into_iter()
                    .map(|[l, w, t]| member.point(l, w, t).map(|length| length.as_nm() as f64))
            })
            .collect();
        let furthest = |corners: &[[f64; 3]], toward: [f64; 3]| {
            corners
                .iter()
                .map(|&p| dot(p, toward))
                .fold(f64::MIN, f64::max)
        };
        const MICRON: f64 = 1_000.0;
        if furthest(&corners, d) > furthest(&on.corners, d) + MICRON {
            return Err(Unsteady::Rocks);
        }
        if furthest(&corners, f) > furthest(&to.corners, f) + MICRON {
            return Err(Unsteady::Blocked);
        }
        Ok(Orientation::from_rotation(rotation))
    }

    pub fn fit(&self) -> Vec<Contact> {
        self.contacts(|_, _| true)
    }

    /// An assembly of `members` held by `joints`, between them by their places in `members`.
    pub(crate) fn from_parts(members: Vec<Member>, joints: Vec<Joint>) -> Assembly {
        Assembly { members, joints }
    }

    /// The places members `i` and `j` meet or nearly do, for each pair `i < j` that `pair`
    /// picks out.
    pub(crate) fn contacts(&self, pair: impl Fn(usize, usize) -> bool) -> Vec<Contact> {
        let mut contacts = Vec::new();
        // Each member's surfaces, its slots' too, worked out once, for the members `pair` picks.
        let mut patches: Vec<Option<Vec<(Surface, Patch)>>> =
            self.members.iter().map(|_| None).collect();
        for i in 0..self.members.len() {
            for j in i + 1..self.members.len() {
                if !pair(i, j) {
                    continue;
                }
                for k in [i, j] {
                    if patches[k].is_none() {
                        patches[k] = Some(self.members[k].patches());
                    }
                }
                let (Some(first), Some(second)) = (&patches[i], &patches[j]) else {
                    unreachable!("both were just worked out");
                };
                for (a, pa) in first {
                    for (b, pb) in second {
                        if let Some((least, most)) = gap(pa, pb) {
                            contacts.push(Contact {
                                between: [(i, *a), (j, *b)],
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
        Some(self.placed(patch(&self.board, surface)?))
    }

    /// Every surface of the board, its slots' too, in the assembly.
    fn patches(&self) -> Vec<(Surface, Patch)> {
        let outside = surfaces(&self.board)
            .into_iter()
            .filter_map(|surface| Some((surface, patch(&self.board, surface)?)));
        let slots = slot_shapes(&self.board)
            .into_iter()
            .filter_map(|(surface, shape)| Some((surface, patch_of(surface, shape)?)));
        outside
            .chain(slots)
            .map(|(surface, local)| (surface, self.placed(local)))
            .collect()
    }

    /// `local`, a surface in the board's own axes, where the board is in the assembly.
    fn placed(&self, local: Patch) -> Patch {
        let at = self.at.map(|length| length.as_nm() as f64);
        let turn = |v| self.rotation.apply(v);
        Patch {
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
        }
    }

    /// Where the point `(l, w, t)` of the board is in the assembly, to the nanometer.
    pub fn point(&self, l: Length, w: Length, t: Length) -> [Length; 3] {
        let p = self
            .rotation
            .apply([l, w, t].map(|length| length.as_nm() as f64));
        [0, 1, 2].map(|k| self.at[k] + nm(p[k]))
    }
}

/// The flat surfaces of `board`'s outline: its bottom, its top, and each straight side around
/// it. The surfaces its slots leave inside it are `slot_surfaces`.
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
/// surface. A face or a side is outlined as if no slot had touched it, and a slot's surface
/// (`Surface::Slot`) where it's wood; `exposed` has where any surface is wood.
pub fn outline(board: &Board, surface: Surface) -> Vec<Vec<[Length; 3]>> {
    if let Surface::Slot(_) = surface {
        return exposed(board, surface);
    }
    solid_outline(board, surface)
}

/// The outline of one of the surfaces of `board`'s outline, as if no slot had touched it, as
/// `outline` has it. Empty for a slot's surface.
pub(crate) fn solid_outline(board: &Board, surface: Surface) -> Vec<Vec<[Length; 3]>> {
    let thickness = board.thickness();
    if board.face().iter().any(|region| region.leans()) {
        // Leaning sides: each region's face on the surface's plane, wherever it has one.
        return board
            .face()
            .iter()
            .filter_map(|region| region.solid(thickness).outline(surface))
            .collect();
    }
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
        Surface::Slot(_) => Vec::new(),
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

/// One of `board`'s surfaces, where it's wood (`exposed`), in its own axes: `l`, `w`, `t`.
fn patch(board: &Board, surface: Surface) -> Option<Patch> {
    patch_of(surface, exposed(board, surface))
}

/// A surface whose wood is `shape`, as `exposed` has it.
fn patch_of(surface: Surface, shape: Vec<Vec<[Length; 3]>>) -> Option<Patch> {
    let corners: Vec<[f64; 3]> = shape
        .into_iter()
        .flatten()
        .map(|p| p.map(|length| length.as_nm() as f64))
        .collect();
    if corners.is_empty() {
        return None;
    }
    let (normal, u, v) = axes(surface);
    Some(Patch {
        normal,
        u,
        v,
        corners,
    })
}

/// The way `surface` faces out of the wood, a unit vector, and two directions across it, `u`
/// and `v`, turning counterclockwise from one to the other seen from outside, in the board's
/// own axes (`l`, `w`, `t`). Across a face, or anything facing straight up or down, `u` runs
/// along the length; across a side, along the side square to the thickness, with `v` up it,
/// leaning as the side leans.
pub(crate) fn axes(surface: Surface) -> ([f64; 3], [f64; 3], [f64; 3]) {
    let out = match surface {
        Surface::Bottom => [0.0, 0.0, -1.0],
        Surface::Top => [0.0, 0.0, 1.0],
        Surface::Side(edge) | Surface::Slot(edge) => {
            [edge.out.l as f64, edge.out.w as f64, edge.lean as f64]
        }
    };
    let n = unit(out);
    let u = if out[0] == 0.0 && out[1] == 0.0 {
        [1.0, 0.0, 0.0]
    } else {
        unit([-out[1], out[0], 0.0])
    };
    (n, u, cross(n, u))
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
