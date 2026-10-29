//! Cutting a glue-up on a saw. The saw works out where its blade goes in its own frame, the way
//! it does for a board. Each board of the glue-up gets the slot the blade cuts through it, in
//! its own coordinates and at its own angle, and comes apart the way any board does. The pieces
//! stay fastened wherever pieces of boards that were fastened together still meet flush, and
//! the glue-up comes apart into whatever no longer holds together.
//!
//! Only glue-ups whose boards all lie flat can be cut so far: a blade through a board standing
//! up would cut across its thickness, which isn't modeled.

use crate::{
    Assembly, CutError, Direction, Length, Rotation,
    assembly::{Joint, Member, Surface, outline},
    history::{Operation, Tool},
    kerf::{Across, Kerf},
    saw::scaled,
};

/// What one pass of a saw did to a glue-up.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GlueUpPass {
    /// The blade didn't touch it.
    Missed,
    /// The blade cut into it, and maybe through some of its boards, but it still holds
    /// together.
    Kerfed(Assembly),
    /// The blade cut it apart.
    Cut(GlueUpCut),
}

/// A glue-up the blade cut apart, sorted the way a board's pieces are: the biggest part on the
/// measured side of the blade (against the fence, or the stop), the biggest on the far side,
/// and anything else that came loose. A part can be a single board.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GlueUpCut {
    pub piece: Assembly,
    pub offcut: Option<Assembly>,
    pub loose: Vec<Assembly>,
}

/// Where a saw's blade goes, in the saw's frame, as a slot square to a flat board's face needs
/// it. Positions are along the table and across it toward the fence, from the near corner of
/// the box what's on the saw fills.
pub(crate) struct Path {
    /// The saw and its settings, and how the glue-up lay on it, to record with each cut.
    pub tool: Tool,
    /// The way out of the slot through one of its faces, square to them: whole numbers along
    /// the table and across it.
    pub across: (i64, i64),
    /// A point on that face, along and across.
    pub face: [Length; 2],
    /// How wide the slot is, square to its faces.
    pub kerf: Length,
    /// A point the blade's center passes over, along and across, and how far it travels from
    /// there each way along the slot. The slot runs a quarter turn clockwise from `across`,
    /// seen from above.
    pub through: [Length; 2],
    pub travel: (Length, Length),
    /// How high the blade's center is above the table.
    pub height: Length,
    pub radius: Length,
    /// Whether the measured side is the one `across` points to: the fence's side of a rip, or
    /// the stop's side of a chop. Against a miter gauge's stop, it's the other side.
    pub measured_ahead: bool,
}

/// A glue-up lying flat on a saw: `lie` turns its axes onto the saw's (along the table, across
/// it toward the fence, and up, as `x`, `y`, `z`), and positions are measured from the near
/// corner of the box it fills, the way a board's are.
pub(crate) struct Laid<'a> {
    glue_up: &'a Assembly,
    lie: Rotation,
    /// The near corner of the box it fills, in the saw's axes, in nanometers.
    low: [f64; 3],
    /// How far it reaches along the table, across it, and up.
    pub extent: [Length; 3],
}

/// Where one board of a glue-up lies on the saw: where its axes point and where its corner is,
/// in nanometers from the glue-up's near corner.
struct Frame {
    columns: [[f64; 3]; 3],
    origin: [f64; 3],
    /// Whether its axes lie exactly along the saw's, so directions and positions on it come out
    /// exact.
    square: bool,
}

/// Slanted directions are whole numbers at this scale, the same as a miter saw's angles.
const DIRECTION_UNIT: f64 = (1 << 24) as f64;

impl<'a> Laid<'a> {
    pub(crate) fn new(glue_up: &'a Assembly, lie: Rotation) -> Result<Laid<'a>, CutError> {
        if !lie.is_square() {
            return Err(CutError::NotModeled(
                "a glue-up has to lie square on the saw",
            ));
        }
        let flat = glue_up.members().iter().all(|member| {
            let up = member.rotation.then(lie).columns()[2][2];
            (up.abs() - 1.0).abs() < 1e-9
        });
        if !flat {
            return Err(CutError::NotModeled(
                "every board in a glue-up has to lie flat to cut it",
            ));
        }
        let mut low = [f64::MAX; 3];
        let mut high = [f64::MIN; 3];
        for member in glue_up.members() {
            for point in corners(member) {
                let point = lie.apply(point);
                for k in 0..3 {
                    low[k] = low[k].min(point[k]);
                    high[k] = high[k].max(point[k]);
                }
            }
        }
        let low = low.map(f64::round);
        Ok(Laid {
            glue_up,
            lie,
            low,
            extent: [0, 1, 2].map(|k| Length::from_nm((high[k] - low[k]).round() as i64)),
        })
    }

    fn frame(&self, member: &Member) -> Frame {
        let turn = member.rotation.then(self.lie);
        let at = self
            .lie
            .apply(member.at.map(|length| length.as_nm() as f64));
        Frame {
            columns: turn.columns(),
            origin: [0, 1, 2].map(|k| at[k] - self.low[k]),
            square: turn.is_square(),
        }
    }

    /// The slot `path` cuts in the board `frame` places.
    fn kerf(&self, frame: &Frame, path: &Path) -> Kerf {
        let across = frame.direction(path.across);
        let (along_l, along_w) =
            frame.onto_face([path.across.1 as f64, -(path.across.0 as f64), 0.0]);
        let square = across.square();
        let along = if square.l as f64 * along_l + square.w as f64 * along_w >= 0.0 {
            square
        } else {
            square.reversed()
        };
        let at = |direction: Direction, (l, w): (Length, Length)| {
            Length::from_nm(direction.dot(l, w) as i64)
        };
        let face = at(across, frame.point(path.face));
        let middle = at(along, frame.point(path.through));
        // Lying face up, its `t = 0` face is on the table; flipped, it's on top.
        let bottom = Length::from_nm(frame.origin[2].round() as i64);
        let center = if frame.columns[2][2] > 0.0 {
            path.height - bottom
        } else {
            bottom - path.height
        };
        Kerf::new(
            Across::Face(across),
            (face - scaled(path.kerf, across), face),
            along,
            (
                middle + scaled(path.travel.0, along),
                middle + scaled(path.travel.1, along),
            ),
            path.radius,
            center,
        )
    }

    /// Runs the blade along `path` through every board, and sorts out what's left.
    pub(crate) fn cut(&self, path: &Path) -> GlueUpPass {
        // One pass, through every board it touches.
        let operation = Operation::new(path.tool);
        let mut members = Vec::new();
        // Which of the glue-up's boards each new member came from.
        let mut from = Vec::new();
        let mut touched = false;
        for (index, member) in self.glue_up.members().iter().enumerate() {
            let kerf = self.kerf(&self.frame(member), path);
            if !member.board.touched_by(&kerf) {
                members.push(member.clone());
                from.push(index);
                continue;
            }
            touched = true;
            for piece in member.board.sawn(kerf, &operation).1 {
                members.push(Member {
                    board: piece.board,
                    rotation: member.rotation,
                    at: member.point(piece.l, piece.w, piece.t),
                });
                from.push(index);
            }
        }
        if !touched {
            return GlueUpPass::Missed;
        }

        // Pieces of boards that were fastened together still are wherever they meet flush,
        // with the joint they were fastened by.
        let fastened = |a: usize, b: usize| {
            self.glue_up.joints().iter().copied().find(|joint| {
                let [x, y] = joint.between.map(|(member, _)| member);
                (x, y) == (a, b) || (x, y) == (b, a)
            })
        };
        let pieces = Assembly::from_parts(members, Vec::new());
        let joints: Vec<Joint> = pieces
            .contacts(|i, j| fastened(from[i], from[j]).is_some())
            .into_iter()
            .filter(|contact| contact.is_tight())
            .filter_map(|contact| {
                let [(i, _), (j, _)] = contact.between;
                Some(Joint {
                    between: contact.between,
                    ..fastened(from[i], from[j])?
                })
            })
            .collect();

        let mut parts = held_together(pieces.members(), &joints);
        if parts.len() == 1 {
            return GlueUpPass::Kerfed(parts.remove(0));
        }
        let (mut near, mut far): (Vec<Assembly>, Vec<Assembly>) = parts
            .into_iter()
            .partition(|part| self.ahead(part, path) == path.measured_ahead);
        if near.is_empty() {
            std::mem::swap(&mut near, &mut far);
        }
        let piece = take_biggest(&mut near).expect("a cut leaves some wood");
        let offcut = take_biggest(&mut far);
        near.append(&mut far);
        GlueUpPass::Cut(GlueUpCut {
            piece,
            offcut,
            loose: near,
        })
    }

    /// Whether `part` is past the middle of the slot `path` cuts, the way `across` points.
    fn ahead(&self, part: &Assembly, path: &Path) -> bool {
        let (along, across) = (path.across.0 as f64, path.across.1 as f64);
        let size = along.hypot(across);
        let on = |point: [f64; 3]| (point[0] * along + point[1] * across) / size;
        let (low, high) = part
            .members()
            .iter()
            .flat_map(corners)
            .map(|point| {
                let point = self.lie.apply(point);
                on([0, 1, 2].map(|k| point[k] - self.low[k]))
            })
            .fold((f64::MAX, f64::MIN), |(low, high), x| {
                (low.min(x), high.max(x))
            });
        let [face_along, face_across] = path.face.map(|length| length.as_nm() as f64);
        let face = on([face_along, face_across, 0.0]);
        let middle = face - path.kerf.as_nm() as f64 / 2.0;
        (low + high) / 2.0 > middle
    }
}

impl Frame {
    /// A vector in the saw's frame, as it runs across the board's face: along `l` and `w`.
    fn onto_face(&self, v: [f64; 3]) -> (f64, f64) {
        let dot = |column: [f64; 3]| column[0] * v[0] + column[1] * v[1] + column[2] * v[2];
        (dot(self.columns[0]), dot(self.columns[1]))
    }

    /// The direction across the table given by whole numbers along and across, on the board's
    /// face: exact if the board lies square, otherwise as near as whole numbers at the scale of
    /// a miter saw's angles come.
    fn direction(&self, (along, across): (i64, i64)) -> Direction {
        let (l, w) = self.onto_face([along as f64, across as f64, 0.0]);
        if self.square {
            Direction::new(l as i64, w as i64)
        } else {
            let size = l.hypot(w);
            Direction::new(
                (l / size * DIRECTION_UNIT).round() as i64,
                (w / size * DIRECTION_UNIT).round() as i64,
            )
        }
    }

    /// Where a point along and across the table is on the board's face, to the nanometer.
    fn point(&self, [along, across]: [Length; 2]) -> (Length, Length) {
        let v = [
            along.as_nm() as f64 - self.origin[0],
            across.as_nm() as f64 - self.origin[1],
            -self.origin[2],
        ];
        let (l, w) = self.onto_face(v);
        (
            Length::from_nm(l.round() as i64),
            Length::from_nm(w.round() as i64),
        )
    }
}

/// The corners of a member's faces, in its assembly, in nanometers.
fn corners(member: &Member) -> Vec<[f64; 3]> {
    [Surface::Bottom, Surface::Top]
        .into_iter()
        .flat_map(|surface| outline(&member.board, surface))
        .flatten()
        .map(|[l, w, t]| member.point(l, w, t).map(|length| length.as_nm() as f64))
        .collect()
}

/// `members` sorted into the assemblies that `joints` hold together, each in the order its
/// members came.
fn held_together(members: &[Member], joints: &[Joint]) -> Vec<Assembly> {
    let mut group: Vec<usize> = (0..members.len()).collect();
    fn root(group: &mut [usize], mut k: usize) -> usize {
        while group[k] != k {
            group[k] = group[group[k]];
            k = group[k];
        }
        k
    }
    for joint in joints {
        let [(a, _), (b, _)] = joint.between;
        let (a, b) = (root(&mut group, a), root(&mut group, b));
        group[a.max(b)] = a.min(b);
    }
    let roots: Vec<usize> = (0..members.len()).map(|k| root(&mut group, k)).collect();
    let mut firsts: Vec<usize> = roots.clone();
    firsts.sort_unstable();
    firsts.dedup();
    firsts
        .into_iter()
        .map(|first| {
            let indices: Vec<usize> = (0..members.len()).filter(|&k| roots[k] == first).collect();
            let renumber = |k: usize| indices.iter().position(|&i| i == k);
            let joints = joints
                .iter()
                .filter_map(|joint| {
                    let [(a, sa), (b, sb)] = joint.between;
                    Some(Joint {
                        between: [(renumber(a)?, sa), (renumber(b)?, sb)],
                        ..*joint
                    })
                })
                .collect();
            Assembly::from_parts(
                indices.iter().map(|&k| members[k].clone()).collect(),
                joints,
            )
        })
        .collect()
}

/// How much wood a part has, as the boxes around its boards.
fn size(part: &Assembly) -> i128 {
    part.members()
        .iter()
        .map(|member| {
            let board = &member.board;
            [board.length(), board.width(), board.thickness()]
                .into_iter()
                .map(|side| side.as_nm() as i128)
                .product::<i128>()
        })
        .sum()
}

/// Removes and returns the biggest part.
fn take_biggest(parts: &mut Vec<Assembly>) -> Option<Assembly> {
    let index = (0..parts.len()).max_by_key(|&k| size(&parts[k]))?;
    Some(parts.remove(index))
}
