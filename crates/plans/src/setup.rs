//! Pictures of the cuts in plans: the wood on the saw as it lay for a cut, a board or every
//! board of a glue-up, the saw set as it was, and the blade where it cut, with what the cut
//! kept picked out and the setting dimensioned, so a step reads at a glance. Each is drawn in
//! the saw's own frame (along the table, across it toward the fence, and up, from the near
//! corner of the box the wood fills, as woodsim measures a cut), with the fence at the back,
//! the way `plan::draw` draws an assembly on the bench. What the cut left is the pass made
//! again on the wood it kept (`Operation::left`), and the blade is the one that cut the slot,
//! from the slot itself.

use std::{f64::consts::TAU, sync::Arc};

use woodsim::{Board, Kerf, Length, Member, Origin, Rotation, Source, Tool, Vector};

use crate::{
    cutting::CutStep,
    plan::{Dimension, Overlay, Paint, Solid, Vec3, add, cross, dot, draw, scaled, unit},
    words::measured,
};

/// Nanometers in an inch.
const INCH: f64 = 25_400_000.0;

/// How many sides the blade's circle is drawn with.
const BLADE_SIDES: usize = 96;

/// Points this near a plane, in nanometers, are on it.
const ON: f64 = 1.0;

/// How big a picture is drawn, in pixels: as big as a joint's.
const SIZE: (f64, f64) = (260.0, 170.0);

/// A convex solid as its faces, each a polygon going around it.
type Faces = Vec<Vec<Vec3>>;

/// Half of space: where `normal · p <= at`.
#[derive(Clone, Copy, Debug)]
struct Half {
    normal: Vec3,
    at: f64,
}

impl Half {
    fn new(normal: Vec3, at: f64) -> Half {
        Half { normal, at }
    }

    /// The other half.
    fn beyond(self) -> Half {
        Half::new(scaled(self.normal, -1.0), -self.at)
    }

    fn on(self, p: Vec3) -> bool {
        (dot(self.normal, p) - self.at).abs() < ON
    }
}

/// Whether a board a cut left that went on is picked out in its picture: picked out (`Some`),
/// with its label if it has one. Asked of a board the blade missed, in a part of what the cut
/// left that's picked out, only its label counts.
pub type Picked<'a> = &'a dyn Fn(&Origin) -> Option<Option<String>>;

/// A picture of `step` on its saw, with what it left picked out as `picked` says, or `None`
/// for a pass through a glue-up read from a recipe written before passes kept what they went
/// through.
pub fn picture(step: &CutStep, picked: Picked) -> Option<String> {
    let (solids, overlay) = scene(step, picked)?;
    Some(draw(&solids, &overlay, "setup", SIZE))
}

/// What a picture of `step` draws: the saw, the blade, and the wood, and what goes over them.
fn scene(step: &CutStep, picked: Picked) -> Option<(Vec<Solid>, Overlay)> {
    let operation = &step.operation;
    let tool = operation.tool;
    // What was on the saw, a board on its own as a glue-up of one, and everything the pass as
    // it ended up left of it, each board where it was in it.
    let wood = operation.wood.as_ref()?;
    let left = operation.left()?.ok()?;
    let parts = left.parts();

    // The wood on the saw: turned as it lay, from the near corner of the box it fills; on the
    // jointer, leaning with its fence.
    let rotation = match tool {
        Tool::Jointer { jointer, .. } => jointer.leaned(tool.lie().rotation()),
        _ => tool.lie().rotation(),
    };
    let nm = |length: Length| length.as_nm() as f64;
    let turned: Vec<Vec3> = wood
        .members()
        .iter()
        .flat_map(|member| {
            member
                .board
                .corners()
                .into_iter()
                .map(move |[l, w, t]| rotation.apply(member.point(l, w, t).map(nm)))
        })
        .collect();
    let least = |k: usize| turned.iter().map(|p| p[k]).fold(f64::MAX, f64::min);
    let most = |k: usize| turned.iter().map(|p| p[k]).fold(f64::MIN, f64::max);
    let low = [0, 1, 2].map(|k| least(k).round());
    let [depth, span, tall] = [0, 1, 2].map(|k| (most(k) - low[k]).round());
    // A point in the wood, on the saw.
    let on_saw = |p: Vec3| add(rotation.apply(p), scaled(low, -1.0));
    let corners: Vec<Vec3> = turned.iter().map(|&p| add(p, scaled(low, -1.0))).collect();
    // The box the wood fills, as the halves it's the inside of: its top first, so the blade
    // above the wood is one piece.
    let walls = [
        Half::new([0.0, 0.0, 1.0], tall),
        Half::new([0.0, -1.0, 0.0], 0.0),
        Half::new([-1.0, 0.0, 0.0], 0.0),
        Half::new([1.0, 0.0, 0.0], depth),
        Half::new([0.0, 1.0, 0.0], span),
        Half::new([0.0, 0.0, -1.0], 0.0),
    ];

    // The blade, from the slot it cut in the first board it cut, on that board as it was on the
    // saw: the piece of it this is had its corner `at` on it.
    let (member, kerf, at) = parts
        .iter()
        .flat_map(|part| part.members())
        .find_map(|member| match member.board.origin().source() {
            Source::Sawn {
                operation: by,
                kerf,
                at,
                ..
            } if Arc::ptr_eq(by, operation) => Some((member, kerf, at)),
            _ => None,
        })?;
    let corner = add(
        member.rotation.apply(scaled(at.map(nm), -1.0)),
        member.at.map(nm),
    );
    let disc = Disc::of(kerf).placed(member.rotation.then(rotation), on_saw(corner));
    let normal = disc.normal;
    // The jointer's fence, square to the way it leans, and where its face is: through the
    // wood's furthest reach that way.
    let fence = match tool {
        Tool::Jointer { jointer, .. } => {
            let (sin, cos) = jointer.tilt.radians().sin_cos();
            let toward = [0.0, cos, sin];
            let against = turned
                .iter()
                .map(|&p| dot(toward, add(p, scaled(low, -1.0))))
                .fold(f64::MIN, f64::max);
            Some(Half::new(toward, against))
        }
        _ => None,
    };
    let u = match tool {
        // Its cutterhead under the middle of the wood, half of it across, or over it.
        Tool::Jointer { .. } | Tool::Planer { .. } => {
            let at = |u: f64| disc.center(u)[0];
            let (start, per) = (at(0.0), at(disc.scale) - at(0.0));
            (depth / 2.0 - start) / per * disc.scale
        }
        _ => disc.drawn_at(kerf, &corners),
    };
    // The blade stops at the table and at the guides; the miter saw's at its middle, where its
    // guard would cover it, so its lower half shows. The jointer's cutterhead turns below its
    // tables, and stops at its fence; the planer's, above the wood, shows whole.
    let mut keep = match (tool, fence) {
        (Tool::Planer { .. }, _) => Vec::new(),
        (_, Some(fence)) => vec![fence],
        (_, None) => vec![Half::new([0.0, 0.0, -1.0], 0.0)],
    };
    match tool {
        Tool::TableSaw { saw, .. } if saw.miter_gauge.is_some() => {
            keep.push(Half::new([1.0, 0.0, 0.0], depth));
        }
        Tool::TableSaw { .. } => keep.push(Half::new([0.0, 1.0, 0.0], span)),
        Tool::MiterSaw { .. } => {
            keep.push(Half::new([0.0, 1.0, 0.0], span));
            keep.push(Half::new([1.0, 0.0, 0.0], depth));
            let middle = disc.center(u)[2];
            keep.push(Half::new([0.0, 0.0, 1.0], middle.max(tall + INCH)));
        }
        Tool::Jointer { .. } | Tool::Planer { .. } => {}
    }
    let (near, far) = disc.faces(u);
    let sides = [dot(normal, near[0]), dot(normal, far[0])];
    let mut slab: Faces = vec![near.clone(), far.clone()];
    for k in 0..BLADE_SIDES {
        let next = (k + 1) % BLADE_SIDES;
        slab.push(vec![near[k], near[next], far[next], far[k]]);
    }
    let clipped = keep.iter().try_fold(slab, |slab, &half| clip(&slab, half));
    let (shown, inside) = match clipped {
        Some(slab) => outside(slab, &walls),
        None => (Vec::new(), None),
    };
    let planes: Vec<Half> = keep.iter().chain(&walls).copied().collect();
    // Its outline: the edges of its faces that were the blade's, not where it was cut short.
    let rim = |faces: &Faces, sides: &[f64]| -> Vec<[Vec3; 2]> {
        faces
            .iter()
            .filter(|face| {
                sides
                    .iter()
                    .any(|&at| face.iter().all(|&p| (dot(normal, p) - at).abs() < ON))
            })
            .flat_map(|face| (0..face.len()).map(|i| [face[i], face[(i + 1) % face.len()]]))
            .filter(|&[a, b]| !planes.iter().any(|plane| plane.on(a) && plane.on(b)))
            .collect()
    };

    // The saw around it, sized to what's on it.
    let mut reach = ([0.0, 0.0, 0.0f64], [depth, span, tall]);
    for &p in shown.iter().flatten().flatten() {
        reach.0 = [0, 1, 2].map(|k| reach.0[k].min(p[k]));
        reach.1 = [0, 1, 2].map(|k| reach.1[k].max(p[k]));
    }
    let (lo, hi) = reach;
    let mut guides: Vec<Faces> = Vec::new();
    let mut dimensions = Vec::new();
    let mut turn = Rotation::NONE;
    match tool {
        Tool::TableSaw { saw, .. } => match saw.miter_gauge {
            Some(stop) => {
                // The gauge's head along the board's trailing side, its stop against the near
                // side, turned so the blade runs along the table and the head is behind the
                // wood.
                let head = 2.0 * INCH;
                guides.push(cuboid(
                    [depth, -1.5 * INCH, 0.0],
                    [depth + 0.75 * INCH, span + INCH, head],
                ));
                guides.push(cuboid(
                    [depth - INCH, -0.75 * INCH, 0.0],
                    [depth, 0.0, 1.25 * INCH],
                ));
                turn = Rotation::about([0.0, 0.0, 1.0], -saw.gauge_angle)
                    .then(Rotation::quarter_turns(2, 2));
                if stop > Length::ZERO {
                    let z = head + 0.75 * INCH;
                    dimensions.push(Dimension {
                        from: [[depth, 0.0, 1.25 * INCH], [depth, nm(stop), tall]],
                        to: [[depth, 0.0, z], [depth, nm(stop), z]],
                        text: measured(stop).to_string(),
                    });
                }
            }
            None => {
                guides.push(cuboid(
                    [lo[0] - INCH, span, 0.0],
                    [hi[0] + 1.75 * INCH, span + 0.75 * INCH, 2.5 * INCH],
                ));
                if saw.fence > Length::ZERO {
                    // Out past the end of the cut, on the table, from the fence to the blade.
                    let end = hi[0].max(depth);
                    let x = end + 0.75 * INCH;
                    let cut = span - nm(saw.fence);
                    dimensions.push(Dimension {
                        from: [[x, span, 0.0], [end, cut, 0.0]],
                        to: [[x, span, 0.0], [x, cut, 0.0]],
                        text: measured(saw.fence).to_string(),
                    });
                }
            }
        },
        Tool::MiterSaw { length, .. } => {
            let fence = 3.0 * INCH;
            guides.push(cuboid(
                [lo[0] - INCH, span, 0.0],
                [hi[0] + 2.0 * INCH, span + 0.75 * INCH, fence],
            ));
            guides.push(cuboid(
                [depth, span - 1.5 * INCH, 0.0],
                [depth + INCH, span, 1.5 * INCH],
            ));
            if length > Length::ZERO {
                // Above the fence, from where the blade crosses it to the stop.
                let (at, z) = (depth - nm(length), fence + 0.75 * INCH);
                dimensions.push(Dimension {
                    from: [[at, span, tall], [depth, span, 1.5 * INCH]],
                    to: [[at, span, z], [depth, span, z]],
                    text: measured(length).to_string(),
                });
            }
        }
        Tool::Jointer { jointer, .. } => {
            // The fence stands on the outfeed table, `cut` above the infeed table where the
            // wood rests, its face leaning over the tables.
            let fence = fence.expect("a jointer has a fence");
            let cut = nm(jointer.depth);
            let (sin, cos) = jointer.tilt.radians().sin_cos();
            let foot = (fence.at - sin * cut) / cos;
            let (rise, into) = ([0.0, -sin, cos], fence.normal);
            let at =
                |x: f64, a: f64, b: f64| add([x, foot, cut], add(scaled(rise, a), scaled(into, b)));
            guides.push(
                cuboid(
                    [lo[0] - INCH, 0.0, 0.0],
                    [hi[0] + INCH, 0.75 * INCH, 3.0 * INCH],
                )
                .into_iter()
                .map(|face| face.into_iter().map(|[x, b, a]| at(x, a, b)).collect())
                .collect(),
            );
            if jointer.fence < jointer.knives {
                // On the outfeed table past the wood's leading end, from the fence to the
                // knives' end.
                let x = lo[0].min(0.0) - 0.75 * INCH;
                let end = foot - nm(jointer.fence);
                dimensions.push(Dimension {
                    from: [[0.0, foot, cut], [0.0, end, cut]],
                    to: [[x, foot, cut], [x, end, cut]],
                    text: measured(jointer.fence).to_string(),
                });
            }
        }
        Tool::Planer { planer, .. } => {
            // Beside the wood's leading end, from the bed up to where the knives reach.
            let (up, y) = (nm(planer.thickness), span + 0.75 * INCH);
            dimensions.push(Dimension {
                from: [[0.0, span, 0.0], [0.0, span, up]],
                to: [[0.0, y, 0.0], [0.0, y, up]],
                text: measured(planer.thickness).to_string(),
            });
        }
    }
    // All of it turned for the view: only for the miter gauge.
    let view = |p: Vec3| turn.apply(p);
    let viewed = |faces: Faces| -> Faces {
        faces
            .into_iter()
            .map(|face| face.into_iter().map(view).collect())
            .collect()
    };
    let viewed_lines = |lines: Vec<[Vec3; 2]>| -> Vec<[Vec3; 2]> {
        lines.into_iter().map(|line| line.map(view)).collect()
    };
    let guides: Vec<Faces> = guides.into_iter().map(viewed).collect();
    // The table under it all, square to the blade's path.
    let wood = cuboid(lo, hi);
    let (mut table_lo, mut table_hi) = ([f64::MAX; 3], [f64::MIN; 3]);
    for p in guides
        .iter()
        .flatten()
        .flatten()
        .copied()
        .chain(wood.iter().flatten().map(|&p| view(p)))
    {
        table_lo = [0, 1, 2].map(|k| table_lo[k].min(p[k]));
        table_hi = [0, 1, 2].map(|k| table_hi[k].max(p[k]));
    }
    let (near, far) = (table_lo[1] - INCH, table_hi[1] + INCH);
    let tables = match tool {
        // Either side of the cutterhead, the outfeed table, where the wood's going, as high as
        // the knives reach, the infeed table where it rests, lower.
        Tool::Jointer { jointer, .. } => {
            let head = disc.center(u)[0];
            let gap = nm(jointer.radius()) + 0.25 * INCH;
            let cut = nm(jointer.depth);
            vec![
                cuboid(
                    [table_lo[0] - INCH, near, cut - 0.75 * INCH],
                    [head - gap, far, cut],
                ),
                cuboid(
                    [head + gap, near, -0.75 * INCH],
                    [table_hi[0] + INCH, far, 0.0],
                ),
            ]
        }
        _ => vec![cuboid(
            [table_lo[0] - INCH, near, -0.75 * INCH],
            [table_hi[0] + INCH, far, 0.0],
        )],
    };
    let mut solids: Vec<Solid> = tables
        .into_iter()
        .map(|table| Solid::convex(table, Paint::Saw, None))
        .collect();
    solids.extend(
        guides
            .into_iter()
            .map(|guide| Solid::convex(guide, Paint::Saw, None)),
    );
    for piece in &shown {
        let outline = viewed_lines(rim(piece, &sides));
        solids.push(Solid::convex(
            viewed(piece.clone()),
            Paint::Blade,
            Some(outline),
        ));
    }
    // What went on of what the pass left, by the parts it left: a part a board of which went on
    // is picked out whole, since its boards are glued together.
    let went_on = |board: &Board| {
        step.outputs
            .iter()
            .map(|(out, _)| out)
            .find(|out| same_cut(out, board.origin()))
    };
    for part in parts {
        let kept: Vec<Option<Option<String>>> = part
            .members()
            .iter()
            .map(|member| went_on(&member.board).and_then(picked))
            .collect();
        let chosen = kept.iter().any(Option::is_some);
        for (member, kept) in part.members().iter().zip(kept) {
            let label = match kept {
                Some(label) => label,
                // The blade missed it, so it's still what it was.
                None if chosen => picked(member.board.origin()).flatten(),
                None => None,
            };
            let at = view(on_saw(member.at.map(nm)));
            let member = Member {
                board: member.board.clone(),
                rotation: member.rotation.then(rotation).then(turn),
                at: at.map(|x| Length::from_nm(x.round() as i64)),
            };
            let paint = if chosen { Paint::Picked } else { Paint::Wood };
            solids.push(Solid::of(&member, paint, label));
        }
    }
    let overlay = Overlay {
        dimensions: dimensions
            .into_iter()
            .map(|dimension| Dimension {
                from: dimension.from.map(view),
                to: dimension.to.map(view),
                text: dimension.text,
            })
            .collect(),
        // The blade inside the wood, dashed.
        hidden: inside.map_or_else(Vec::new, |inside| viewed_lines(rim(&inside, &sides[..1]))),
    };
    Some((solids, overlay))
}

/// The blade that cut a slot, on the board's own axes, in nanometers: its circle on the plane
/// midway between the slot's faces, where its center was at any point of its travel.
struct Disc {
    /// Square to its faces, and its two directions across them: the way its center traveled,
    /// and up its plane from there. Unit vectors.
    normal: Vec3,
    along: Vec3,
    up: Vec3,
    /// Its center's path, with the center at the start of the slot's measure along it.
    base: Vec3,
    /// How long `kerf.along` is, which positions along it are in nanometers times.
    scale: f64,
    radius: f64,
    /// Half its thickness.
    half: f64,
}

impl Disc {
    fn of(kerf: &Kerf) -> Disc {
        let vector = |v: Vector| [v.l as f64, v.w as f64, v.t as f64];
        let (normal, along, up) = (vector(kerf.normal), vector(kerf.along), vector(kerf.up()));
        // Positions on each are in nanometers times its length.
        let middle = (kerf.near.as_nm() + kerf.far.as_nm()) as f64 / 2.0;
        let base = add(
            scaled(normal, middle / dot(normal, normal)),
            scaled(up, kerf.center as f64 / dot(up, up)),
        );
        Disc {
            normal: unit(normal),
            along: unit(along),
            up: unit(up),
            base,
            scale: dot(along, along).sqrt(),
            radius: kerf.radius.as_nm() as f64,
            half: (kerf.far - kerf.near).as_nm() as f64 / 2.0 / dot(normal, normal).sqrt(),
        }
    }

    /// The same blade, with the board it cut turned by `turn` and its corner moved to `corner`.
    fn placed(self, turn: Rotation, corner: Vec3) -> Disc {
        Disc {
            normal: turn.apply(self.normal),
            along: turn.apply(self.along),
            up: turn.apply(self.up),
            base: add(turn.apply(self.base), corner),
            ..self
        }
    }

    /// Its center at `u` along its travel, in nanometers times `scale`, as `kerf.start` and
    /// `kerf.end` are.
    fn center(&self, u: f64) -> Vec3 {
        add(self.base, scaled(self.along, u / self.scale))
    }

    fn point(&self, u: f64, angle: f64) -> Vec3 {
        add(
            self.center(u),
            add(
                scaled(self.along, self.radius * angle.cos()),
                scaled(self.up, self.radius * angle.sin()),
            ),
        )
    }

    /// Where along its travel to draw it, on the saw: where the cut stopped, or, for a pass all
    /// the way through, as the blade comes out the wood's trailing end, all of it above the
    /// table back in the wood if the wood's long enough, so nothing hides what's in front of
    /// it; `corners` are the wood's.
    fn drawn_at(&self, kerf: &Kerf, corners: &[Vec3]) -> f64 {
        let (start, end) = (kerf.start.as_nm() as f64, kerf.end.as_nm() as f64);
        let height = |p: Vec3| p[2];
        let along: Vec<f64> = corners
            .iter()
            .map(|&p| dot(self.along, add(p, scaled(self.base, -1.0))) * self.scale)
            .collect();
        let lead = along.iter().copied().fold(f64::MAX, f64::min);
        let trail = along.iter().copied().fold(f64::MIN, f64::max);
        // How far ahead of its center it reaches above the table, as it's drawn: its corners
        // above the table, and where its sides cross it.
        let center = self.center(start);
        let corner = |k: usize| self.point(start, TAU * k as f64 / BLADE_SIDES as f64);
        let mut ahead: f64 = 0.0;
        for k in 0..BLADE_SIDES {
            let (a, b) = (corner(k), corner(k + 1));
            let (ha, hb) = (height(a), height(b));
            if ha >= 0.0 {
                ahead = ahead.max(dot(self.along, add(a, scaled(center, -1.0))));
            }
            if ha * hb < 0.0 {
                let crossing = add(a, scaled(add(b, scaled(a, -1.0)), ha / (ha - hb)));
                ahead = ahead.max(dot(self.along, add(crossing, scaled(center, -1.0))));
            }
        }
        (trail - ahead * self.scale)
            .max((lead + trail) / 2.0)
            .clamp(start, end)
    }

    /// Its two faces with its center at `u`.
    fn faces(&self, u: f64) -> (Vec<Vec3>, Vec<Vec3>) {
        let face = |offset: f64| {
            (0..BLADE_SIDES)
                .map(|k| {
                    let angle = TAU * k as f64 / BLADE_SIDES as f64;
                    add(self.point(u, angle), scaled(self.normal, offset))
                })
                .collect()
        };
        (face(-self.half), face(self.half))
    }
}

/// Whether `a` and `b` are the same piece of the same cut of the same board: what a pass made
/// again leaves, and what it left.
fn same_cut(a: &Origin, b: &Origin) -> bool {
    match (a.source(), b.source()) {
        (
            Source::Sawn {
                from, kerf, part, ..
            },
            Source::Sawn {
                from: f,
                kerf: k,
                part: p,
                ..
            },
        ) => from.same(f) && kerf == k && part == p,
        _ => false,
    }
}

/// A box from its least corner to its most, as its faces.
fn cuboid(lo: Vec3, hi: Vec3) -> Faces {
    let p = |x: bool, y: bool, z: bool| {
        [
            if x { hi[0] } else { lo[0] },
            if y { hi[1] } else { lo[1] },
            if z { hi[2] } else { lo[2] },
        ]
    };
    let (o, i) = (false, true);
    vec![
        vec![p(o, o, o), p(o, i, o), p(i, i, o), p(i, o, o)],
        vec![p(o, o, i), p(i, o, i), p(i, i, i), p(o, i, i)],
        vec![p(o, o, o), p(i, o, o), p(i, o, i), p(o, o, i)],
        vec![p(o, i, o), p(o, i, i), p(i, i, i), p(i, i, o)],
        vec![p(o, o, o), p(o, o, i), p(o, i, i), p(o, i, o)],
        vec![p(i, o, o), p(i, i, o), p(i, i, i), p(i, o, i)],
    ]
}

/// What of the convex solid `faces` is in `half`, closed over where it was cut, if any of it
/// is: more than a face lying on its edge.
fn clip(faces: &Faces, half: Half) -> Option<Faces> {
    let off = |p: Vec3| dot(half.normal, p) - half.at;
    if !faces.iter().flatten().any(|&p| off(p) < -ON) {
        return None;
    }
    let mut kept = Vec::new();
    let mut cut: Vec<Vec3> = Vec::new();
    for face in faces {
        if face.iter().all(|&p| off(p).abs() < ON) {
            // Already a face on the plane.
            kept.push(face.clone());
            continue;
        }
        let mut polygon = Vec::new();
        for i in 0..face.len() {
            let (a, b) = (face[i], face[(i + 1) % face.len()]);
            let (da, db) = (off(a), off(b));
            if da < ON {
                polygon.push(a);
                if da > -ON {
                    cut.push(a);
                }
            }
            if (da < -ON && db > ON) || (da > ON && db < -ON) {
                let p = add(a, scaled(add(b, scaled(a, -1.0)), da / (da - db)));
                polygon.push(p);
                cut.push(p);
            }
        }
        if polygon.len() >= 3 {
            kept.push(polygon);
        }
    }
    let mut points: Vec<Vec3> = Vec::new();
    for p in cut {
        if !points.iter().any(|&q| {
            let d = add(p, scaled(q, -1.0));
            dot(d, d) < 100.0
        }) {
            points.push(p);
        }
    }
    if points.len() >= 3 {
        // Around their middle, on the plane.
        let middle = points
            .iter()
            .fold([0.0; 3], |sum, &p| add(sum, p))
            .map(|x| x / points.len() as f64);
        let out = |p: Vec3| add(p, scaled(middle, -1.0));
        let first = unit(out(points[0]));
        let second = cross(half.normal, first);
        let angle = |p: Vec3| dot(out(p), second).atan2(dot(out(p), first));
        points.sort_by(|&a, &b| angle(a).total_cmp(&angle(b)));
        kept.push(points);
    }
    Some(kept)
}

/// The convex solid `faces`, in convex pieces outside the box `walls` are the inside of, and
/// what's inside it.
fn outside(faces: Faces, walls: &[Half]) -> (Vec<Faces>, Option<Faces>) {
    let mut pieces = Vec::new();
    let mut rest = Some(faces);
    for &wall in walls {
        let Some(inside) = rest.take() else {
            break;
        };
        pieces.extend(clip(&inside, wall.beyond()));
        rest = clip(&inside, wall);
    }
    (pieces, rest)
}

#[cfg(test)]
mod tests {
    use woodsim::{
        Angle, Assembly, Blade, Fastener, GlueUpCut, GlueUpPass, Mating, MiterSaw, Operation,
        Orientation, Pass, Surface, TableSaw, surfaces,
    };

    use super::*;
    use crate::cutting::Cutting;

    /// 3/4" x 3-1/2" x 24".
    fn stock() -> Board {
        Board::new(
            Length::from_inches(24),
            Length::from_inch_fraction(7, 2),
            Length::from_inch_fraction(3, 4),
        )
    }

    fn table_saw(fence: Length, height: Length) -> TableSaw {
        TableSaw {
            blade: Blade {
                diameter: Length::from_inches(10),
                kerf: Blade::FULL_KERF,
            },
            fence,
            height,
            miter_gauge: None,
            gauge_angle: Angle::ZERO,
            tilt: Angle::ZERO,
        }
    }

    /// The picture of the last cut that made `board`, with all it kept picked out.
    fn scene_of(board: Board) -> (Vec<Solid>, Overlay) {
        let cutting = Cutting::of(&Assembly::of(board));
        let step = cutting.cuts.last().expect("a cut");
        scene(step, &|_| Some(None)).expect("a picture")
    }

    /// The box around the solids painted `paint`.
    fn bounds(solids: &[Solid], paint: Paint) -> (Vec3, Vec3) {
        solids
            .iter()
            .filter(|solid| solid.paint() == paint)
            .flat_map(|solid| solid.corners())
            .fold(([f64::MAX; 3], [f64::MIN; 3]), |(low, high), p| {
                (
                    [0, 1, 2].map(|k| low[k].min(p[k])),
                    [0, 1, 2].map(|k| high[k].max(p[k])),
                )
            })
    }

    /// Checks that no part of the blade drawn is inside the wood, which fills `reach` from the
    /// saw's corner.
    fn blade_clear_of_wood(solids: &[Solid], reach: Vec3) {
        for solid in solids.iter().filter(|solid| solid.paint() == Paint::Blade) {
            for p in solid.corners() {
                let inside = (0..3).all(|k| p[k] > ON && p[k] < reach[k] - ON);
                assert!(!inside, "the blade at {p:?} is inside the wood");
            }
        }
    }

    fn near(a: f64, inches: f64) -> bool {
        (a - inches * INCH).abs() < 10.0
    }

    #[test]
    fn a_ripped_strip_rides_the_fence_and_is_measured_from_it() {
        let Ok(Pass::Cut(cut)) =
            table_saw(Length::from_inch_fraction(3, 2), Length::from_inches(1))
                .rip(&stock(), Orientation::default())
        else {
            panic!("expected a rip");
        };
        let (solids, overlay) = scene_of(cut.piece.board);
        // The strip against the fence, at the far side of the board across the table, and the
        // rest in front of it, past the kerf.
        let (low, high) = bounds(&solids, Paint::Picked);
        assert!(near(high[1], 3.5) && near(low[1], 2.0), "{low:?} {high:?}");
        let (low, high) = bounds(&solids, Paint::Wood);
        assert!(
            near(low[1], 0.0) && near(high[1], 1.875),
            "{low:?} {high:?}"
        );
        assert!(solids.iter().any(|solid| solid.paint() == Paint::Blade));
        blade_clear_of_wood(&solids, [24.0 * INCH, 3.5 * INCH, 0.75 * INCH]);
        assert_eq!(overlay.dimensions.len(), 1);
        assert_eq!(overlay.dimensions[0].text, "1-1/2\"");
    }

    #[test]
    fn a_mitered_piece_lies_against_the_stop() {
        let saw = MiterSaw {
            miter: Angle::from_degrees(45),
            ..MiterSaw::twelve_inch()
        };
        let Ok(Pass::Cut(cut)) =
            saw.crosscut(&stock(), Orientation::default(), Length::from_inches(10))
        else {
            panic!("expected a crosscut");
        };
        let (solids, overlay) = scene_of(cut.piece.board);
        // Its long point against the stop, at the far end of the board along the fence.
        let (_, high) = bounds(&solids, Paint::Picked);
        assert!(near(high[0], 24.0), "{high:?}");
        let (low, _) = bounds(&solids, Paint::Wood);
        assert!(near(low[0], 0.0), "{low:?}");
        blade_clear_of_wood(&solids, [24.0 * INCH, 3.5 * INCH, 0.75 * INCH]);
        assert_eq!(overlay.dimensions[0].text, "10\"");
    }

    #[test]
    fn a_groove_shows_the_blade_dashed_inside_the_wood() {
        let Ok(Pass::Kerfed(grooved)) =
            table_saw(Length::from_inches(1), Length::from_inch_fraction(1, 4))
                .rip(&stock(), Orientation::default())
        else {
            panic!("expected a groove");
        };
        let (solids, overlay) = scene_of(grooved);
        assert!(!solids.iter().any(|solid| solid.paint() == Paint::Blade));
        assert!(!overlay.hidden.is_empty());
        // No higher than the blade reached.
        for line in &overlay.hidden {
            for p in line {
                assert!(p[2] < 0.25 * INCH + ON, "{p:?}");
            }
        }
    }

    /// `boards` glued edge to edge into a panel, face up, each on the far edge of the last.
    fn panel(boards: &[Board]) -> Assembly {
        let edge = |board: &Board, far: bool| {
            surfaces(board)
                .into_iter()
                .find(|surface| {
                    matches!(surface, Surface::Side(edge)
                        if edge.out.l == 0 && (edge.out.w > 0) == far)
                })
                .expect("an edge")
        };
        let mut panel = Assembly::of(boards[0].clone());
        for (k, board) in boards.iter().enumerate().skip(1) {
            let onto = (k - 1, edge(&panel.members()[k - 1].board, true));
            let other = Assembly::of(board.clone());
            let surface = (0, edge(board, false));
            let mating = (0..4)
                .map(|turn| Mating {
                    turn,
                    slide: [Length::ZERO; 2],
                })
                .find(|&mating| {
                    let motion = panel.mating(onto, &other, surface, mating).unwrap();
                    motion.rotation.apply([0.0, 0.0, 1.0])[2] > 0.999
                })
                .expect("a way that keeps it face up");
            assert!(panel.attach(onto, &other, surface, mating, Fastener::Glue));
        }
        panel
    }

    /// `glue_up` crosscut square at the miter saw, `inches` from the stop.
    fn crosscut(glue_up: &Assembly, inches: i64) -> GlueUpCut {
        match MiterSaw::twelve_inch().crosscut_glue_up(
            glue_up,
            Rotation::NONE,
            Length::from_inches(inches),
        ) {
            Ok(GlueUpPass::Cut(cut)) => cut,
            other => panic!("expected the glue-up cut apart, got {other:?}"),
        }
    }

    /// The picture of the last cut that made `glue_up`, with all it kept picked out and each
    /// of its boards labeled by where it is in it.
    fn scene_of_glue_up(glue_up: &Assembly) -> (Vec<Solid>, Overlay) {
        let cutting = Cutting::of(glue_up);
        let step = cutting.cuts.last().expect("a cut");
        let picked = |out: &Origin| {
            let member = glue_up
                .members()
                .iter()
                .position(|member| member.board.origin().same(out));
            Some(member.map(|member| format!("A{}", member + 1)))
        };
        scene(step, &picked).expect("a picture")
    }

    #[test]
    fn a_glue_up_crosscut_lies_on_the_saw_with_its_piece_against_the_stop() {
        let one_by_three = Board::new(
            Length::from_inches(24),
            Length::from_inch_fraction(5, 2),
            Length::from_inch_fraction(3, 4),
        );
        let cut = crosscut(&panel(&[one_by_three.clone(), one_by_three]), 12);
        let (solids, overlay) = scene_of_glue_up(&cut.piece);
        // Both boards of the piece picked out, against the stop at the far end of the panel
        // along the fence, each labeled; and both boards of the rest of it past the kerf.
        let drawn = |paint: Paint| solids.iter().filter(move |solid| solid.paint() == paint);
        assert_eq!(drawn(Paint::Picked).count(), 2);
        let mut labels: Vec<_> = drawn(Paint::Picked).map(|solid| solid.label()).collect();
        labels.sort();
        assert_eq!(labels, [Some("A1"), Some("A2")]);
        assert_eq!(drawn(Paint::Wood).count(), 2);
        let (low, high) = bounds(&solids, Paint::Picked);
        assert!(
            near(low[0], 12.0) && near(high[0], 24.0),
            "{low:?} {high:?}"
        );
        assert!(near(low[1], 0.0) && near(high[1], 5.0), "{low:?} {high:?}");
        let (low, high) = bounds(&solids, Paint::Wood);
        assert!(
            near(low[0], 0.0) && near(high[0], 11.875),
            "{low:?} {high:?}"
        );
        assert!(near(low[1], 0.0) && near(high[1], 5.0), "{low:?} {high:?}");
        assert!(solids.iter().any(|solid| solid.paint() == Paint::Blade));
        blade_clear_of_wood(&solids, [24.0 * INCH, 5.0 * INCH, 0.75 * INCH]);
        assert_eq!(overlay.dimensions.len(), 1);
        assert_eq!(overlay.dimensions[0].text, "12\"");
    }

    #[test]
    fn a_board_the_blade_missed_goes_on_with_what_its_glued_to() {
        // A 24" 1x3 with a 12" one glued along it at the end against the stop, and the long
        // one cut 16" from the stop: the piece goes on, the short one with it.
        let board = |inches: i64| {
            Board::new(
                Length::from_inches(inches),
                Length::from_inch_fraction(5, 2),
                Length::from_inch_fraction(3, 4),
            )
        };
        let glue_up = panel(&[board(24), board(12)]);
        let short = &glue_up.members()[1];
        let zero = Length::ZERO;
        let far = short.point(short.board.length(), short.board.width(), zero);
        assert_eq!(
            (short.point(zero, zero, zero)[0], far[0]),
            (Length::from_inches(12), Length::from_inches(24))
        );
        let cut = crosscut(&glue_up, 16);
        assert_eq!(cut.piece.members().len(), 2);
        let (solids, _) = scene_of_glue_up(&cut.piece);
        let picked: Vec<&Solid> = solids
            .iter()
            .filter(|solid| solid.paint() == Paint::Picked)
            .collect();
        assert_eq!(picked.len(), 2);
        assert!(picked.iter().all(|solid| solid.label().is_some()));
        let (low, high) = bounds(&solids, Paint::Picked);
        assert!(near(low[0], 8.0) && near(high[0], 24.0), "{low:?} {high:?}");
        assert!(near(low[1], 0.0) && near(high[1], 5.0), "{low:?} {high:?}");
        // The rest of the long one, past the kerf.
        let (low, high) = bounds(&solids, Paint::Wood);
        assert!(
            near(low[0], 0.0) && near(high[0], 7.875),
            "{low:?} {high:?}"
        );
        assert!(near(low[1], 0.0) && near(high[1], 2.5), "{low:?} {high:?}");
    }

    #[test]
    fn a_pass_that_kept_no_wood_has_no_picture() {
        let cut = crosscut(&panel(&[stock(), stock()]), 12);
        let mut cutting = Cutting::of(&cut.piece);
        let step = cutting.cuts.last_mut().expect("a cut");
        step.operation = Arc::new(Operation {
            wood: None,
            ..(*step.operation).clone()
        });
        assert!(scene(step, &|_| Some(None)).is_none());
    }

    #[test]
    fn a_box_cut_in_two_is_a_closed_box() {
        let cube = cuboid([0.0; 3], [2.0 * INCH; 3]);
        let half = clip(&cube, Half::new([1.0, 0.0, 0.0], INCH)).expect("half the box");
        assert_eq!(half.len(), 6);
        assert!(half.iter().flatten().all(|p| p[0] < INCH + ON));
        // Closed where it was cut.
        let cap: Vec<&Vec<Vec3>> = half
            .iter()
            .filter(|face| face.iter().all(|p| (p[0] - INCH).abs() < ON))
            .collect();
        assert_eq!(cap.len(), 1);
        assert_eq!(cap[0].len(), 4);
        assert!(clip(&cube, Half::new([1.0, 0.0, 0.0], -INCH)).is_none());
        // Outside a box inside it, in pieces, and the box.
        let (pieces, inside) = outside(
            cube,
            &[
                Half::new([1.0, 0.0, 0.0], INCH),
                Half::new([0.0, 1.0, 0.0], INCH),
            ],
        );
        assert_eq!(pieces.len(), 2);
        let inside = inside.expect("the inside");
        assert!(
            inside
                .iter()
                .flatten()
                .all(|p| p[0] < INCH + ON && p[1] < INCH + ON)
        );
    }
}
