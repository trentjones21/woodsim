//! Cutting what's on a saw, a board or a glue-up, however it lies. The saw works out where its
//! blade sweeps in its own frame (`Sweep`): the plane it turns in, whatever the saw's angle and
//! tilt, and the path its center takes. Each board gets the slot that sweep cuts through it, in
//! its own coordinates and at its own angle, and comes apart the way any board does. A board on
//! its own is cut as a glue-up of one. The pieces of a glue-up stay fastened wherever pieces of
//! boards that were fastened together still meet flush, and it comes apart into whatever no
//! longer holds together.

use crate::{
    Assembly, Board, CutError, Direction, Length, Rotation, Vector,
    assembly::{Joint, Member},
    history::{Operation, Side, Tool},
    kerf::Kerf,
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

/// Where a saw's blade swept, in the saw's frame: `x` along the table (or the fence), `y`
/// across it toward the fence, and `z` up, in nanometers from the near corner of the box what's
/// on the saw fills. Directions are whole numbers.
pub(crate) struct Sweep {
    /// The saw and its settings, and how what's on it lay, to record with each cut.
    pub tool: Tool,
    /// Square to the blade's faces.
    pub normal: [i64; 3],
    /// A point on the face the setting measures to: the slot runs from there away from the
    /// measured side, `kerf` wide.
    pub face: [Length; 3],
    pub kerf: Length,
    /// Whether the measured side is the one `normal` points to: the fence's side of a rip, or
    /// the stop's side of a chop. Against a miter gauge's stop, it's the other side.
    pub measured_ahead: bool,
    /// The way the blade's center traveled, in its plane, square to `normal`; how far, as
    /// positions along it times its scale; and a point on its path.
    pub along: [i64; 3],
    pub travel: (Length, Length),
    pub through: [Length; 3],
    pub radius: Length,
}

/// What's on a saw: `lie` turns its axes onto the saw's (along the table, across it toward the
/// fence, and up, as `x`, `y`, `z`), and positions are measured from the near corner of the box
/// it fills, the way a board's are.
pub(crate) struct Laid<'a> {
    glue_up: &'a Assembly,
    lie: Rotation,
    /// The near corner of the box it fills, in the saw's axes, in nanometers.
    low: [f64; 3],
    /// How far it reaches along the table, across it, and up.
    pub extent: [Length; 3],
}

/// Where one board lies on the saw: where its axes point and where its corner is, in
/// nanometers from the near corner of what's on the saw.
struct Frame {
    columns: [[f64; 3]; 3],
    origin: [f64; 3],
    /// Whether its axes lie exactly along the saw's, so directions and positions on it come out
    /// exact.
    square: bool,
}

/// Slanted directions are whole numbers at this scale, the same as a saw's angles.
const DIRECTION_UNIT: f64 = (1 << 24) as f64;

impl<'a> Laid<'a> {
    pub(crate) fn new(glue_up: &'a Assembly, lie: Rotation) -> Result<Laid<'a>, CutError> {
        if !lie.is_square() {
            return Err(CutError::NotModeled(
                "what's on the saw has to lie square on it",
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

    /// The near corner of the box it fills and how far it reaches, as the corners of that box,
    /// in the saw's axes.
    pub(crate) fn box_corners(&self) -> Vec<[Length; 3]> {
        let [x, y, z] = self.extent;
        let zero = Length::ZERO;
        [
            [zero, zero, zero],
            [x, zero, zero],
            [zero, y, zero],
            [x, y, zero],
            [zero, zero, z],
            [x, zero, z],
            [zero, y, z],
            [x, y, z],
        ]
        .to_vec()
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

    /// The slot `sweep` cuts in the board `frame` places, and its faces' normal as the board
    /// has it before the slot writes it one way, with where its measured face is on it.
    fn slot(&self, frame: &Frame, sweep: &Sweep) -> Result<(Kerf, Vector, Length), CutError> {
        const STANDING: CutError = CutError::NotModeled(
            "a board in the glue-up stands at an angle the saw can't cut it at yet",
        );
        let normal = frame.direction(sweep.normal).ok_or(STANDING)?;
        let along = frame.square_to(normal, sweep.along).ok_or(STANDING)?;
        let at = |direction: Vector, point: [Length; 3]| {
            let [l, w, t] = frame.point(point);
            direction.dot(l, w, t)
        };
        let face = Length::from_nm(at(normal, sweep.face) as i64);
        let width = scaled(sweep.kerf, normal.scale());
        let faces = if sweep.measured_ahead {
            (face - width, face)
        } else {
            (face, face + width)
        };
        // Along the path, from its point on the board, as far as it went each way in the saw.
        let through = Length::from_nm(at(along, sweep.through) as i64);
        let saw_along = Vector::new(sweep.along[0], sweep.along[1], sweep.along[2]);
        let [x, y, z] = sweep.through;
        let from = Length::from_nm(saw_along.dot(x, y, z) as i64);
        let ratio = along.scale() / saw_along.scale();
        let travel = |end: Length| through + scaled(end - from, ratio);
        let center = normal.cross(along).dot_point(frame.point(sweep.through));
        let kerf = Kerf::blade(
            normal,
            faces,
            along,
            (travel(sweep.travel.0), travel(sweep.travel.1)),
            sweep.radius,
            center,
        );
        Ok((kerf, normal, face))
    }

    /// How far short of the wood the blade's measured face is, square to it, if it's past all
    /// of it: the setting's too long. `None` if some wood is beyond it, where the blade cuts.
    pub(crate) fn short_of(&self, sweep: &Sweep) -> Result<Option<f64>, CutError> {
        let mut nearest = f64::INFINITY;
        for member in self.glue_up.members() {
            let (_, normal, face) = self.slot(&self.frame(member), sweep)?;
            // The wood the blade cuts is past the measured face, away from the measured side.
            let (toward, at) = if sweep.measured_ahead {
                (normal.reversed(), -face.as_nm() as i128)
            } else {
                (normal, face.as_nm() as i128)
            };
            if member.board.reaches_beyond(toward.edge(at)) {
                return Ok(None);
            }
            let furthest = member
                .board
                .corners()
                .into_iter()
                .map(|[l, w, t]| toward.dot(l, w, t) as f64)
                .fold(f64::MIN, f64::max);
            nearest = nearest.min((at as f64 - furthest) / toward.scale());
        }
        Ok(Some(nearest))
    }

    /// Runs the blade along `sweep` through every board, and sorts out what's left.
    pub(crate) fn cut(&self, sweep: &Sweep) -> Result<GlueUpPass, CutError> {
        // One pass, through every board it touches.
        let operation = Operation::new(sweep.tool);
        let mut members = Vec::new();
        // Which of the glue-up's boards each new member came from.
        let mut from = Vec::new();
        let mut touched = false;
        // Whether any board came apart or lost some of its outline, rather than just a slot.
        let mut shaped = false;
        for (index, member) in self.glue_up.members().iter().enumerate() {
            let (kerf, ..) = self.slot(&self.frame(member), sweep)?;
            if !member.board.touched_by(&kerf) {
                members.push(member.clone());
                from.push(index);
                continue;
            }
            touched = true;
            let (kerfed, pieces) = member.board.sawn(kerf, &operation);
            shaped |= !matches!(pieces.as_slice(), [whole] if whole.board == kerfed);
            for piece in pieces {
                members.push(Member {
                    board: piece.board,
                    rotation: member.rotation,
                    at: member.point(piece.l, piece.w, piece.t),
                });
                from.push(index);
            }
        }
        if !touched {
            return Ok(GlueUpPass::Missed);
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
        if parts.len() == 1 && !shaped {
            return Ok(GlueUpPass::Kerfed(parts.remove(0)));
        }
        let (mut near, mut far): (Vec<Assembly>, Vec<Assembly>) = parts
            .into_iter()
            .partition(|part| self.ahead(part, sweep) == sweep.measured_ahead);
        if near.is_empty() {
            std::mem::swap(&mut near, &mut far);
        }
        let piece = self
            .take_biggest(&mut near)
            .expect("a cut leaves some wood");
        let offcut = self.take_biggest(&mut far);
        near.append(&mut far);
        // Each board the blade cut is on the side of the cut its part of the glue-up is.
        let piece = on_side(piece, Side::Measured);
        let offcut = offcut.map(|offcut| on_side(offcut, Side::Offcut));
        Ok(GlueUpPass::Cut(GlueUpCut {
            piece,
            offcut,
            loose: near,
        }))
    }

    /// Where `part`'s corners are in the saw's axes, in nanometers.
    fn on_saw(&self, part: &Assembly) -> impl Iterator<Item = [f64; 3]> {
        part.members()
            .iter()
            .flat_map(corners)
            .map(|point| {
                let point = self.lie.apply(point);
                [0, 1, 2].map(|k| point[k] - self.low[k])
            })
            .collect::<Vec<_>>()
            .into_iter()
    }

    /// Whether `part` is past the middle of the slot `sweep` cuts, the way its normal points.
    fn ahead(&self, part: &Assembly, sweep: &Sweep) -> bool {
        let normal = sweep.normal.map(|v| v as f64);
        let size = (normal[0] * normal[0] + normal[1] * normal[1] + normal[2] * normal[2]).sqrt();
        let on = |point: [f64; 3]| {
            (point[0] * normal[0] + point[1] * normal[1] + point[2] * normal[2]) / size
        };
        let (low, high) = self
            .on_saw(part)
            .map(on)
            .fold((f64::MAX, f64::MIN), |(low, high), x| {
                (low.min(x), high.max(x))
            });
        let face = on(sweep.face.map(|length| length.as_nm() as f64));
        let half = sweep.kerf.as_nm() as f64 / 2.0;
        let middle = if sweep.measured_ahead {
            face - half
        } else {
            face + half
        };
        (low + high) / 2.0 > middle
    }

    /// Removes and returns the biggest part, the one reaching nearest the fence among equals.
    fn take_biggest(&self, parts: &mut Vec<Assembly>) -> Option<Assembly> {
        let toward_fence = |part: &Assembly| {
            self.on_saw(part)
                .map(|point| point[1].round() as i64)
                .max()
                .unwrap_or(i64::MIN)
        };
        let index = (0..parts.len()).max_by_key(|&k| (size(&parts[k]), toward_fence(&parts[k])))?;
        Some(parts.remove(index))
    }
}

impl Frame {
    /// A vector in the saw's frame, along the board's `l`, `w`, and `t`.
    fn onto_board(&self, v: [f64; 3]) -> [f64; 3] {
        let dot = |column: [f64; 3]| column[0] * v[0] + column[1] * v[1] + column[2] * v[2];
        self.columns.map(dot)
    }

    /// The board's axis that stands straight up the saw, if one does, and which way.
    fn upright(&self) -> Option<(usize, f64)> {
        (0..3)
            .find(|&k| (self.columns[k][2].abs() - 1.0).abs() < 1e-9)
            .map(|k| (k, self.columns[k][2].signum()))
    }

    /// The direction given by whole numbers in the saw's frame, on the board: exact if the
    /// board lies square, otherwise as near as whole numbers at the scale of a saw's angles come.
    /// `None` for a board turned out of square except about the saw's up, which isn't modeled.
    fn direction(&self, v: [i64; 3]) -> Option<Vector> {
        let mapped = self.onto_board(v.map(|c| c as f64));
        if self.square {
            let [l, w, t] = mapped.map(|c| c.round() as i64);
            return Some(Vector::new(l, w, t));
        }
        // Turned about the saw's up: what runs up the saw stays exact, the rest comes as near
        // as whole numbers at the saw's scale do.
        let (up, _) = self.upright()?;
        let across: Vec<usize> = (0..3).filter(|&k| k != up).collect();
        let size = mapped[across[0]].hypot(mapped[across[1]]);
        let mut parts = [0i64; 3];
        if size < 1e-9 {
            parts[up] = mapped[up].signum() as i64;
        } else {
            let scale = DIRECTION_UNIT / size;
            for k in 0..3 {
                parts[k] = (mapped[k] * scale).round() as i64;
            }
        }
        Some(Vector::new(parts[0], parts[1], parts[2]))
    }

    /// The direction `v` in the saw's frame, on the board, made exactly square to `normal` there,
    /// as a blade's path is to its faces. For a board turned off square, `v` has to run level
    /// in the saw, as a blade's path does.
    fn square_to(&self, normal: Vector, v: [i64; 3]) -> Option<Vector> {
        let near = self.direction(v)?;
        if self.square {
            return Some(near);
        }
        // Level in the saw, a blade's path is square to the board's upright axis, and to the
        // faces: across the level plane from the faces' normal there.
        let (up, _) = self.upright()?;
        let parts = [normal.l, normal.w, normal.t];
        let across: Vec<usize> = (0..3).filter(|&k| k != up).collect();
        let (a, b) = (parts[across[0]], parts[across[1]]);
        if a == 0 && b == 0 {
            return Some(near);
        }
        let mut square = [0i64; 3];
        square[across[0]] = -b;
        square[across[1]] = a;
        let square = Vector::new(square[0], square[1], square[2]);
        Some(if square.dot_vector(near) >= 0 {
            square
        } else {
            square.reversed()
        })
    }

    /// Where a point in the saw's frame is on the board, to the nanometer.
    fn point(&self, at: [Length; 3]) -> [Length; 3] {
        let v = [0, 1, 2].map(|k| at[k].as_nm() as f64 - self.origin[k]);
        self.onto_board(v)
            .map(|c| Length::from_nm(c.round() as i64))
    }
}

/// `length` times `scale`, to the nanometer.
fn scaled(length: Length, scale: f64) -> Length {
    Length::from_nm((length.as_nm() as f64 * scale).round() as i64)
}

impl Vector {
    /// How far the point `[l, w, t]` is along it, times its scale.
    fn dot_point(self, [l, w, t]: [Length; 3]) -> i128 {
        self.dot(l, w, t)
    }
}

/// Where each corner of a member's wood is, in its assembly, in nanometers.
fn corners(member: &Member) -> Vec<[f64; 3]> {
    member
        .board
        .corners()
        .into_iter()
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

/// `part` with each board the cut made noted as on `side` of it. Boards the blade didn't touch
/// keep where they came from.
fn on_side(part: Assembly, side: Side) -> Assembly {
    let members = part
        .members()
        .iter()
        .map(|member| {
            let mut member = member.clone();
            if member.board.origin().side() == Some(Side::Loose) {
                member.board.set_side(side);
            }
            member
        })
        .collect();
    Assembly::from_parts(members, part.joints().to_vec())
}

/// A board as a glue-up of one, to cut it as glue-ups are cut.
pub(crate) fn alone(board: &Board) -> Assembly {
    Assembly::of(board.clone())
}

/// A direction across the table given by whole numbers along and across it, level.
pub(crate) fn level(direction: Direction) -> [i64; 3] {
    [direction.l, direction.w, 0]
}
