//! The workbench's state: the parts on it, loose or put together, and the joint being lined
//! up. Pick a surface of one part, then the surface of another that it goes against: the first
//! part moves there, to be turned around the joint and slid across it until it's lined up, and
//! is fastened there for good when it's glued. A joint can come back apart, the last first.
//!
//! `woodsim` puts the parts together and measures how the joints fit; this module keeps track
//! of what's on the bench and where, and puts it into words.

use bevy::{math::DVec3, prelude::*};
use plans::name;
use woodsim::{
    Assembly, Board, Contact, Fastener, Length, Mating, Member, Motion, Refusal, Remake, Rotation,
    Surface, slot_bottoms, slot_surfaces, surfaces,
};

use crate::{hand::Workpiece, shop::inches, work::glue_up_bounds};

/// Half the bench top's length and depth, in meters.
pub const HALF_TOP: Vec2 = Vec2::new(inches(30.0), inches(15.0));

/// Where a part set down without a place in mind goes: the middle of the bench, a little back.
const SET_DOWN_AT: Vec2 = Vec2::new(0.0, inches(-2.0));

/// How far apart parts lying loose on the bench stay.
const ROOM: f32 = inches(1.0);

/// The finest a part slides across a joint being lined up.
pub const NUDGE: Length = Length::from_inch_fraction(1, 16);

/// Parts on the bench that move together: one loose part, or parts put together.
#[derive(Clone, Debug, PartialEq)]
pub struct Group {
    pub assembly: Assembly,
    /// How it lies on the bench, turned square from how its assembly is set up.
    pub turn: Rotation,
    /// Where its assembly's origin sits across the bench top, in meters from the middle: `x`
    /// along the bench to the right, `y` toward the operator.
    pub at: Vec2,
    /// The groups joined to it, earliest first, to take them apart again.
    joined: Vec<Joined>,
}

/// A group joined to another: the other's assembly before it, and the group as it was, so it
/// can come back off and go back where it was.
#[derive(Clone, Debug, PartialEq)]
struct Joined {
    before: Assembly,
    group: Group,
    /// When it was joined, counting joints on the bench.
    order: u64,
    /// How many places it was fastened: where it was put, and wherever else it met flush.
    places: usize,
}

/// What put a group together, made again as `remake` makes its wood: what each joint went
/// onto, and the group that went on, as each was.
fn remade_joins(remake: &mut Remake, joined: &[Joined]) -> Result<Vec<Joined>, Refusal> {
    joined
        .iter()
        .map(|joined| {
            let mut group = joined.group.clone();
            if let Some(assembly) = remake.assembly(&group.assembly)? {
                group.assembly = assembly;
            }
            group.joined = remade_joins(remake, &group.joined)?;
            let before = remake.assembly(&joined.before)?;
            Ok(Joined {
                before: before.unwrap_or_else(|| joined.before.clone()),
                group,
                ..joined.clone()
            })
        })
        .collect()
}

/// A surface of a part on the bench.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Spot {
    pub group: usize,
    pub member: usize,
    pub surface: Surface,
}

/// The picked surface's part, put against `onto` to be lined up before it's glued.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LiningUp {
    pub onto: Spot,
    pub mating: Mating,
}

#[derive(Resource)]
pub struct BenchStation {
    pub groups: Vec<Group>,
    pub fastener: Fastener,
    /// The surface picked to go against another, if any.
    pub picked: Option<Spot>,
    pub lining_up: Option<LiningUp>,
    /// Whether the last click on a part found no flat surface to pick, to say so instead of
    /// doing nothing.
    missed: bool,
    /// What came of drawing up the last plans, said until something on the bench changes.
    note: Option<String>,
    /// How many joints have gone together on the bench, to number the next.
    joints: u64,
}

impl Default for BenchStation {
    fn default() -> BenchStation {
        BenchStation {
            groups: Vec::new(),
            fastener: Fastener::Glue,
            picked: None,
            lining_up: None,
            missed: false,
            note: None,
            joints: 0,
        }
    }
}

/// A part to draw on the bench: how it sits, and which part on the bench it is.
#[derive(Clone, Debug, PartialEq)]
pub struct Shown {
    pub board: Board,
    /// The rotation that turns its mesh (`w`, `t`, `l` along x, y, z) into place.
    pub rotation: Quat,
    /// Where its corner is, in meters from the middle of the bench top.
    pub corner: Vec3,
    pub group: usize,
    pub member: usize,
    /// Whether it's the part picked to move, or a part of its group.
    pub moving: bool,
}

impl Group {
    pub fn of(board: Board) -> Group {
        Group {
            assembly: Assembly::of(board),
            turn: Rotation::NONE,
            at: Vec2::ZERO,
            joined: Vec::new(),
        }
    }

    /// Whether it's a single part, not put together with any other.
    pub fn is_loose(&self) -> bool {
        self.assembly.members().len() == 1
    }

    /// The least and most corners of the box it fills, in meters from its origin as it lies.
    fn bounds(&self) -> (Vec3, Vec3) {
        glue_up_bounds(&self.assembly, &|v| onto_bench(self.turn, v))
    }

    /// Where its origin is on the bench: at `at` across the top, raised so its lowest point
    /// rests on it.
    fn origin(&self) -> Vec3 {
        let (low, _) = self.bounds();
        Vec3::new(self.at.x, -low.y, self.at.y)
    }

    /// Where it covers the bench top: `x` along the bench and `y` toward the operator.
    pub fn footprint(&self) -> Rect {
        let (low, high) = self.bounds();
        Rect::new(low.x, low.z, high.x, high.z).translate(self.at)
    }

    /// Moves it so the middle of its footprint is at `middle`.
    fn center_on(&mut self, middle: Vec2) {
        self.at += middle - self.footprint().center();
    }

    /// How a member sits on the bench: the rotation that turns its mesh (`w`, `t`, `l` along
    /// x, y, z) into place, and where its corner is, in meters from the middle of the top.
    pub fn pose(&self, member: usize) -> (Quat, Vec3) {
        let member = &self.assembly.members()[member];
        let [l, w, t] = member.axes().map(|axis| onto_bench(self.turn, axis));
        let rotation = Quat::from_mat3(&Mat3::from_cols(w, t, l));
        (
            rotation,
            self.origin() + onto_bench(self.turn, meters(member.at)),
        )
    }

    /// The order of the last joint that went together in it, if any.
    fn last_joined(&self) -> Option<u64> {
        self.joined.last().map(|joined| joined.order)
    }
}

/// A point of an assembly in meters along its axes.
fn meters(point: [Length; 3]) -> [f64; 3] {
    point.map(|length| length.as_nm() as f64 / 1e9)
}

/// Turns an assembly's axes into the bench's, as a group lying `turn` has them: unturned, the
/// assembly's `x` runs along the bench to the right, its `y` away from the operator, and its
/// `z` up.
fn onto_bench(turn: Rotation, v: [f64; 3]) -> Vec3 {
    let v = turn.apply(v);
    Vec3::new(v[0] as f32, v[2] as f32, -v[1] as f32)
}

/// Where a point on a member's mesh (`w`, `t`, `l` along x, y, z, in meters) is in its
/// assembly, in nanometers.
pub fn assembly_point(member: &Member, mesh_point: Vec3) -> [f64; 3] {
    let length = |meters: f32| Length::from_nm((f64::from(meters) * 1e9).round() as i64);
    member
        .point(
            length(mesh_point.z),
            length(mesh_point.x),
            length(mesh_point.y),
        )
        .map(|length| length.as_nm() as f64)
}

impl BenchStation {
    /// Sets `work` down on the bench, as near `near` (across the top, in meters from its
    /// middle) as there's room: a board loose, a glue-up lying as it was carried. A glue-up
    /// that's been carried away has set: its joints don't come back apart.
    pub fn set_down(&mut self, work: Workpiece, near: Option<Vec2>) {
        let mut group = match work {
            Workpiece::Board(board) => Group::of(board),
            Workpiece::GlueUp(assembly) => Group {
                assembly,
                turn: Rotation::NONE,
                at: Vec2::ZERO,
                joined: Vec::new(),
            },
        };
        group.center_on(near.unwrap_or(SET_DOWN_AT));
        let spot = self.free_spot(&group, None);
        group.center_on(spot);
        self.groups.push(group);
        self.note = None;
    }

    /// Where the middle of `group`'s footprint can go, as near where it is as there's room on
    /// the bench, clear of the other groups (all but `index`, if `group` is on the bench).
    fn free_spot(&self, group: &Group, index: Option<usize>) -> Vec2 {
        let footprint = group.footprint();
        let others: Vec<Rect> = self
            .groups
            .iter()
            .enumerate()
            .filter(|&(k, _)| Some(k) != index)
            .map(|(_, other)| other.footprint().inflate(ROOM))
            .collect();
        let half = footprint.half_size();
        let room = (HALF_TOP - half).max(Vec2::ZERO);
        let wanted = footprint.center().clamp(-room, room);
        let step = inches(1.0);
        let (across, deep) = ((room.x / step) as i32, (room.y / step) as i32);
        let mut spots: Vec<Vec2> = (-across..=across)
            .flat_map(|i| (-deep..=deep).map(move |j| Vec2::new(i as f32, j as f32) * step))
            .chain([wanted])
            .collect();
        spots.sort_by(|a, b| a.distance(wanted).total_cmp(&b.distance(wanted)));
        spots
            .into_iter()
            .find(|&spot| {
                let placed = Rect::from_center_half_size(spot, half);
                others
                    .iter()
                    .all(|other| other.intersect(placed).is_empty())
            })
            .unwrap_or(wanted)
    }

    /// Moves any group that `index` now overlaps to the nearest place there's room for it.
    fn make_room(&mut self, index: usize) {
        let footprint = self.groups[index].footprint().inflate(ROOM);
        for other in 0..self.groups.len() {
            if other != index
                && !self.groups[other]
                    .footprint()
                    .intersect(footprint)
                    .is_empty()
            {
                let spot = self.free_spot(&self.groups[other], Some(other));
                self.groups[other].center_on(spot);
            }
        }
    }

    /// Picks the surface `spot`, clicked at `at` (in its assembly's axes, in nanometers): the
    /// first surface picked is the one that moves; a surface of another part is where it goes.
    /// Picking another surface of the moving part moves that one instead, and picking the
    /// same surface again lets it go.
    pub fn pick(&mut self, spot: Spot, at: [f64; 3]) {
        self.missed = false;
        self.note = None;
        match self.picked {
            Some(picked) if picked == spot && self.lining_up.is_none() => self.picked = None,
            Some(picked) if picked.group == spot.group => {
                self.picked = Some(spot);
                if let Some(lining_up) = self.lining_up {
                    self.line_up(lining_up.onto, None);
                }
            }
            Some(_) => self.line_up(spot, Some(at)),
            None => self.picked = Some(spot),
        }
    }

    /// Puts the picked surface against `onto`: turned a way that leaves its part lying square
    /// on the bench, if any does (two scarfs, sloped alike, go together end to end with one
    /// turned over, not tipped up off the bench), and of those the one that leaves it most
    /// nearly as it lies now, keeping whichever side is up; and slid to whichever end of
    /// `onto` the click at `at` was nearest, or the middle; or where it was, if `at` is `None`.
    fn line_up(&mut self, onto: Spot, at: Option<[f64; 3]>) {
        let Some(picked) = self.picked else {
            return;
        };
        let (target, moving) = (&self.groups[onto.group], &self.groups[picked.group]);
        let (theirs, mine) = ((onto.member, onto.surface), (picked.member, picked.surface));
        let up_now = moving.turn.inverse().apply([0.0, 0.0, 1.0]);
        let rank = |turn: u8| {
            let motion = target.assembly.mating(
                theirs,
                &moving.assembly,
                mine,
                Mating {
                    turn,
                    slide: [Length::ZERO; 2],
                },
            )?;
            let lies = motion.rotation.then(target.turn);
            // Lying square on the bench, each of its axes along one of the bench's.
            let square = lies
                .columns()
                .iter()
                .flatten()
                .all(|v| v.abs() < 1e-9 || (v.abs() - 1.0).abs() < 1e-9);
            let up = lies.apply(up_now)[2];
            let turned = moving.turn.inverse().then(lies).degrees();
            Some((
                !square,
                (-up * 1e3).round() as i64,
                (turned * 1e2).round() as i64,
            ))
        };
        let Some(turn) = (0..4)
            .filter_map(|turn| Some((rank(turn)?, turn)))
            .min()
            .map(|(_, turn)| turn)
        else {
            self.lining_up = None;
            return;
        };
        let stops = target.assembly.stops(theirs, &moving.assembly, mine, turn);
        let extent = target.assembly.extent(theirs);
        let (Some(stops), Some(extent)) = (stops, extent) else {
            self.lining_up = None;
            return;
        };
        let slide = match (at, self.lining_up) {
            (Some(at), _) => [0, 1].map(|axis| {
                let along = extent.directions[axis];
                let at = along[0] * at[0] + along[1] * at[1] + along[2] * at[2];
                let (least, most) = extent.reach[axis];
                let (least, most) = (least.as_nm() as f64, most.as_nm() as f64);
                let third = ((at - least) / (most - least).max(1.0) * 3.0).clamp(0.0, 2.0);
                stops[axis][third as usize]
            }),
            (None, Some(lining_up)) => lining_up.mating.slide,
            (None, None) => [stops[0][0], stops[1][0]],
        };
        self.lining_up = Some(LiningUp {
            onto,
            mating: Mating { turn, slide },
        });
        self.keep_slide_within_stops();
    }

    /// The stops along each direction across the joint being lined up, as `Assembly::stops`
    /// gives them: flush at one side, centered, flush at the other.
    pub fn stops(&self) -> Option<[[Length; 3]; 2]> {
        let (picked, lining_up) = (self.picked?, self.lining_up?);
        self.groups[lining_up.onto.group].assembly.stops(
            (lining_up.onto.member, lining_up.onto.surface),
            &self.groups[picked.group].assembly,
            (picked.member, picked.surface),
            lining_up.mating.turn,
        )
    }

    /// The directions across the joint being lined up that `slide` moves along, in the
    /// bench's axes.
    pub fn slide_directions(&self) -> Option<[Vec3; 2]> {
        let lining_up = self.lining_up?;
        let target = &self.groups[lining_up.onto.group];
        let extent = target
            .assembly
            .extent((lining_up.onto.member, lining_up.onto.surface))?;
        Some(
            extent
                .directions
                .map(|along| onto_bench(target.turn, along)),
        )
    }

    fn keep_slide_within_stops(&mut self) {
        let Some(stops) = self.stops() else {
            return;
        };
        if let Some(lining_up) = &mut self.lining_up {
            for (slide, stops) in lining_up.mating.slide.iter_mut().zip(stops) {
                let (least, most) = (stops[0].min(stops[2]), stops[0].max(stops[2]));
                *slide = (*slide).clamp(least, most);
            }
        }
    }

    /// Turns the part being lined up `quarters` quarter turns further around the joint,
    /// keeping it flush or centered where it was.
    pub fn turn_around_joint(&mut self, quarters: i32) {
        let Some(before) = self.stops() else {
            return;
        };
        let Some(lining_up) = &mut self.lining_up else {
            return;
        };
        lining_up.mating.turn = (i32::from(lining_up.mating.turn) + quarters).rem_euclid(4) as u8;
        let slide = lining_up.mating.slide;
        let Some(after) = self.stops() else {
            return;
        };
        if let Some(lining_up) = &mut self.lining_up {
            for axis in 0..2 {
                if let Some(stop) = before[axis].iter().position(|&s| s == slide[axis]) {
                    lining_up.mating.slide[axis] = after[axis][stop];
                }
            }
        }
        self.keep_slide_within_stops();
    }

    /// Slides the part being lined up along one of the joint's directions (`axis` 0 or 1),
    /// toward its most side or its least: to the next stop that way, or `NUDGE` if `fine`.
    pub fn slide(&mut self, axis: usize, toward_most: bool, fine: bool) {
        let Some(stops) = self.stops() else {
            return;
        };
        let Some(lining_up) = &mut self.lining_up else {
            return;
        };
        let now = lining_up.mating.slide[axis];
        let next = if fine {
            if toward_most {
                now + NUDGE
            } else {
                now - NUDGE
            }
        } else {
            let mut stops = stops[axis];
            stops.sort();
            if toward_most {
                stops.into_iter().find(|&stop| stop > now).unwrap_or(now)
            } else {
                stops
                    .into_iter()
                    .rev()
                    .find(|&stop| stop < now)
                    .unwrap_or(now)
            }
        };
        lining_up.mating.slide[axis] = next;
        self.keep_slide_within_stops();
    }

    /// Turns group `index` where it lies, about the bench's axes as an assembly's are set up
    /// (`x` along the bench, `z` up), keeping it where it is on the top: to bring a surface
    /// that faces away around into view.
    pub fn turn_group(&mut self, index: usize, rotation: Rotation) {
        let Some(group) = self.groups.get_mut(index) else {
            return;
        };
        let middle = group.footprint().center();
        group.turn = group.turn.then(rotation);
        group.center_on(middle);
        self.make_room(index);
    }

    /// Notes that a click on a part found no flat surface there to pick: the side of a saw kerf,
    /// say. The status says so until the next pick.
    pub fn missed(&mut self) {
        self.missed = true;
    }

    /// Lets go of the joint being lined up, or else of the picked surface.
    pub fn cancel(&mut self) {
        if self.lining_up.take().is_none() {
            self.picked = None;
        }
    }

    /// Fastens the part being lined up where it is, with the chosen fastener. Returns false if
    /// nothing's being lined up.
    pub fn glue(&mut self) -> bool {
        let (Some(picked), Some(lining_up)) = (self.picked, self.lining_up) else {
            return false;
        };
        let moving = self.groups[picked.group].clone();
        let target = &mut self.groups[lining_up.onto.group];
        let before = target.assembly.clone();
        if !target.assembly.attach(
            (lining_up.onto.member, lining_up.onto.surface),
            &moving.assembly,
            (picked.member, picked.surface),
            lining_up.mating,
            self.fastener,
        ) {
            return false;
        }
        self.joints += 1;
        let places =
            target.assembly.joints().len() - before.joints().len() - moving.assembly.joints().len();
        target.joined.push(Joined {
            before,
            group: moving,
            order: self.joints,
            places,
        });
        self.groups.remove(picked.group);
        self.picked = None;
        self.lining_up = None;
        self.note = None;
        let target = lining_up.onto.group - usize::from(picked.group < lining_up.onto.group);
        self.make_room(target);
        true
    }

    /// The group a turn turns when the pointer isn't on a part: the picked part's, or the only
    /// one on the bench.
    pub fn turnable(&self) -> Option<usize> {
        self.picked
            .map(|picked| picked.group)
            .or((self.groups.len() == 1).then_some(0))
    }

    /// Whether there's a joint for `take_apart` to take back apart.
    pub fn can_take_apart(&self) -> bool {
        self.latest().is_some()
    }

    /// The group the last joint went together in, if any.
    fn latest(&self) -> Option<usize> {
        (0..self.groups.len())
            .filter(|&k| self.groups[k].last_joined().is_some())
            .max_by_key(|&k| self.groups[k].last_joined())
    }

    /// Takes the last joint that went together back apart: the part (or parts) that went on
    /// go back where they were. Returns false if there's no joint.
    pub fn take_apart(&mut self) -> bool {
        let Some(index) = self.latest() else {
            return false;
        };
        let group = &mut self.groups[index];
        let Some(joined) = group.joined.pop() else {
            return false;
        };
        group.assembly = joined.before;
        self.picked = None;
        self.lining_up = None;
        self.note = None;
        let mut apart = joined.group;
        let spot = self.free_spot(&apart, None);
        apart.center_on(spot);
        self.groups.push(apart);
        true
    }

    /// Takes the picked part off the bench to carry: a loose board, or the whole glue-up it's
    /// fastened into, lying as it lies on the bench.
    pub fn take_picked(&mut self) -> Option<Workpiece> {
        let picked = self.picked?;
        self.lining_up = None;
        self.take(picked.group)
    }

    /// The part or glue-up `index`, as it would be carried off: a loose board, or the glue-up
    /// lying as it lies on the bench.
    pub fn wood(&self, index: usize) -> Option<Workpiece> {
        let group = self.groups.get(index)?;
        Some(if group.is_loose() {
            Workpiece::Board(group.assembly.members()[0].board.clone())
        } else {
            Workpiece::GlueUp(group.assembly.moved(Motion {
                rotation: group.turn,
                shift: [Length::ZERO; 3],
            }))
        })
    }

    /// Takes the part or glue-up `index` off the bench to carry, as `wood` has it. What's
    /// picked, and the joint being lined up, stay as they were if they weren't on it.
    pub fn take(&mut self, index: usize) -> Option<Workpiece> {
        let work = self.wood(index)?;
        self.groups.remove(index);
        let after = |group: usize| group - usize::from(group > index);
        if self.picked.is_some_and(|picked| picked.group == index)
            || self
                .lining_up
                .is_some_and(|lining_up| lining_up.onto.group == index)
        {
            self.lining_up = None;
        }
        self.picked = self
            .picked
            .filter(|picked| picked.group != index)
            .map(|picked| Spot {
                group: after(picked.group),
                ..picked
            });
        if let Some(lining_up) = &mut self.lining_up {
            lining_up.onto.group = after(lining_up.onto.group);
        }
        self.note = None;
        Some(work)
    }

    /// Makes every part and glue-up on the bench again as `remake` makes its wood, keeping where
    /// it lies, and what put it together, to take apart again, made again too, or forgotten if
    /// it can't be. Changes nothing unless `apply`; returns how many it would change, or why it
    /// can't.
    pub fn remake(&mut self, remake: &mut Remake, apply: bool) -> Result<usize, Refusal> {
        let mut changed = 0;
        for index in 0..self.groups.len() {
            let Some(assembly) = remake.assembly(&self.groups[index].assembly)? else {
                continue;
            };
            changed += 1;
            if !apply {
                continue;
            }
            let joined = std::mem::take(&mut self.groups[index].joined);
            let group = &mut self.groups[index];
            group.assembly = assembly;
            group.joined = remade_joins(remake, &joined).unwrap_or_default();
            // Its surfaces may have moved, or gone.
            if self.picked.is_some_and(|picked| picked.group == index)
                || self
                    .lining_up
                    .is_some_and(|lining_up| lining_up.onto.group == index)
            {
                self.picked = None;
                self.lining_up = None;
            }
            self.make_room(index);
            self.note = None;
        }
        Ok(changed)
    }

    /// Takes the joints `joints` (by their places in its glue-up) of group `index` apart. The
    /// biggest piece that leaves stays where it is, and the others are set down beside it, each
    /// as near where it was as there's room. Its joints have set, so U no longer takes them
    /// apart one by one. Returns false if there's no such joint.
    pub fn take_apart_joints(&mut self, index: usize, joints: &[usize]) -> bool {
        let Some(group) = self.groups.get(index) else {
            return false;
        };
        let count = group.assembly.joints().len();
        if joints.is_empty() || joints.iter().any(|&joint| joint >= count) {
            return false;
        }
        let mut pieces = group.assembly.taken_apart(joints);
        let biggest = (0..pieces.len())
            .max_by_key(|&k| (pieces[k].members().len(), std::cmp::Reverse(k)))
            .expect("at least one piece");
        let stays = pieces.remove(biggest);
        let (turn, at) = (group.turn, group.at);
        let group = &mut self.groups[index];
        group.assembly = stays;
        group.joined.clear();
        for piece in pieces {
            let mut apart = Group {
                assembly: piece,
                turn,
                at,
                joined: Vec::new(),
            };
            let spot = self.free_spot(&apart, None);
            apart.center_on(spot);
            self.groups.push(apart);
        }
        self.picked = None;
        self.lining_up = None;
        self.note = None;
        true
    }

    /// The joints the picked part has, by their places in its glue-up's.
    pub fn picked_joints(&self) -> Vec<usize> {
        let Some(picked) = self.picked else {
            return Vec::new();
        };
        self.groups[picked.group]
            .assembly
            .joints()
            .iter()
            .enumerate()
            .filter(|(_, joint)| {
                joint
                    .between
                    .iter()
                    .any(|&(member, _)| member == picked.member)
            })
            .map(|(index, _)| index)
            .collect()
    }

    /// What to draw up plans for, lying as it lies on the bench: the glue-up (or loose part)
    /// in group `under_pointer`, or else the picked part's, or the one that last had a joint go
    /// together, or the first on the bench. `None` if the bench is bare.
    pub fn to_plan(&self, under_pointer: Option<usize>) -> Option<Assembly> {
        let index = under_pointer
            .or(self.picked.map(|picked| picked.group))
            .or(self.latest())
            .or((!self.groups.is_empty()).then_some(0))?;
        let group = self.groups.get(index)?;
        Some(group.assembly.moved(Motion {
            rotation: group.turn,
            shift: [Length::ZERO; 3],
        }))
    }

    /// Says what came of drawing up plans, until something on the bench changes.
    pub fn note(&mut self, note: String) {
        self.note = Some(note);
    }

    /// The group being lined up against another, put together the way it would be glued.
    fn preview(&self) -> Option<Group> {
        let (picked, lining_up) = (self.picked?, self.lining_up?);
        let mut preview = self.groups[lining_up.onto.group].clone();
        preview
            .assembly
            .attach(
                (lining_up.onto.member, lining_up.onto.surface),
                &self.groups[picked.group].assembly,
                (picked.member, picked.surface),
                lining_up.mating,
                self.fastener,
            )
            .then_some(preview)
    }

    /// Every part on the bench, where it sits: the part being lined up where it would go.
    pub fn shown(&self) -> Vec<Shown> {
        let picked = self.picked.map(|picked| picked.group);
        let preview = self.preview();
        let mut shown = Vec::new();
        for (index, group) in self.groups.iter().enumerate() {
            let onto = self.lining_up.map(|lining_up| lining_up.onto.group);
            if preview.is_some() && Some(index) == picked {
                continue;
            }
            let (drawn, own) = match (&preview, onto == Some(index)) {
                (Some(preview), true) => (preview, group.assembly.members().len()),
                _ => (group, usize::MAX),
            };
            for (member, part) in drawn.assembly.members().iter().enumerate() {
                let (rotation, corner) = drawn.pose(member);
                let (group, member, moving) = match (member >= own, picked) {
                    (true, Some(picked)) => (picked, member - own, true),
                    _ => (index, member, Some(index) == picked),
                };
                shown.push(Shown {
                    board: part.board.clone(),
                    rotation,
                    corner,
                    group,
                    member,
                    moving,
                });
            }
        }
        shown
    }

    /// One line on what's happening at the bench, given what's in hand, after what came of
    /// the last plans drawn up.
    pub fn status(&self, holding: bool) -> String {
        let doing = self.doing(holding);
        match &self.note {
            Some(note) => format!("{note} {doing}"),
            None => doing,
        }
    }

    /// What the surface at `spot` is called: "end", "slot bottom".
    fn called(&self, spot: Spot) -> &'static str {
        let board = &self.groups[spot.group].assembly.members()[spot.member].board;
        name_on(board, spot.surface)
    }

    fn doing(&self, holding: bool) -> String {
        if holding {
            return "Click the bench to set it down.".to_string();
        }
        if self.groups.is_empty() {
            return "Nothing on the bench. Set parts on it from the inventory, or set down the one \
                    in hand."
                .to_string();
        }
        if self.missed {
            return "There's no flat surface there to put against another: try a face, an \
                    edge, or an end."
                .to_string();
        }
        let Some(picked) = self.picked else {
            let last = self.latest().map_or(String::new(), |index| {
                let group = &self.groups[index];
                let joined = group.joined.last().expect("the latest group has a joint");
                let fastener = group.assembly.joints().last().map(|joint| joint.fastener);
                format!(
                    " The last part went on with {} {}.{}",
                    fastened(fastener.unwrap_or(self.fastener)),
                    at_joints(joined.places),
                    describe_fit(&group.assembly, 0)
                )
            });
            return format!(
                "Click the surface of a part to move it: an end, an edge, or a face. If the \
                 side you want faces away, click the part and turn it around.{last}"
            );
        };
        let mine = self.called(picked);
        let (Some(lining_up), Some(preview)) = (self.lining_up, self.preview()) else {
            let take = if self.groups[picked.group].is_loose() {
                "pick it up"
            } else {
                "pick up the glue-up"
            };
            return format!(
                "Moving its {mine}. Click the surface it goes against (if it faces away, drag \
                 to look around), or {take}."
            );
        };
        let theirs = self.called(lining_up.onto);
        let lined_up = self.stops().map_or(String::new(), |stops| {
            describe_slide(lining_up.mating.slide, stops)
        });
        let first = self.groups[lining_up.onto.group].assembly.members().len();
        // Two halves of one miter cut, both face up, go back together into the board they
        // were cut from; flipping one over makes the corner.
        let members = preview.assembly.members();
        let [a, b] = [lining_up.onto.member, first + picked.member].map(|k| members[k].axes()[0]);
        let in_line = (a[0] * b[0] + a[1] * b[1] + a[2] * b[2]).abs() > 0.999;
        let flip = if in_line && mine == "mitered end" && theirs == "mitered end" {
            " They run on in a straight line; flip it over to make a corner."
        } else {
            ""
        };
        let places = preview.assembly.joints().len()
            - self.groups[lining_up.onto.group].assembly.joints().len()
            - self.groups[picked.group].assembly.joints().len();
        let places = if places > 1 {
            format!(" {}", at_joints(places))
        } else {
            String::new()
        };
        format!(
            "Its {mine} against the {theirs}{lined_up}.{flip} Fasten it with {}{places}.{}",
            fastened(self.fastener),
            describe_fit(&preview.assembly, first)
        )
    }
}

/// "at 1 joint", "at 2 joints".
fn at_joints(count: usize) -> String {
    match count {
        1 => "at 1 joint".to_string(),
        n => format!("at {n} joints"),
    }
}

/// Where a part sits across the joint, along each of its directions that it can slide: flush
/// with one side, centered, flush with the other, or in between.
fn describe_slide(slide: [Length; 2], stops: [[Length; 3]; 2]) -> String {
    let places: Vec<String> = (0..2)
        .filter(|&axis| stops[axis][0] != stops[axis][2])
        .map(|axis| {
            let [least, middle, most] = stops[axis];
            let now = slide[axis];
            if now == least {
                "flush with one side".to_string()
            } else if now == middle {
                "centered".to_string()
            } else if now == most {
                "flush with the other side".to_string()
            } else {
                let apart = |a: Length, b: Length| Length::from_nm((a - b).as_nm().abs());
                let off = apart(now, least).min(apart(now, most));
                format!("{} from flush", measured(off))
            }
        })
        .collect();
    if places.is_empty() {
        String::new()
    } else {
        format!(", {}", places.join(" and "))
    }
}

/// How the joints between `assembly`'s members fit, counting only those that involve a member
/// from `first` on: the places that aren't tight, worst first, which go unfastened, or that
/// they all are. Starts with a space, or is empty if there are no joints.
fn describe_fit(assembly: &Assembly, first: usize) -> String {
    let contacts: Vec<Contact> = assembly
        .fit()
        .into_iter()
        .filter(|contact| contact.between[1].0 >= first)
        .collect();
    if contacts.is_empty() {
        return String::new();
    }
    let mut loose: Vec<Contact> = contacts
        .into_iter()
        .filter(|contact| !contact.is_tight())
        .collect();
    if loose.is_empty() {
        return " Every joint is tight.".to_string();
    }
    let worst = |contact: &Contact| contact.least.as_nm().abs().max(contact.most.as_nm().abs());
    loose.sort_by_key(|contact| std::cmp::Reverse(worst(contact)));
    let lines: Vec<String> = loose
        .iter()
        .map(|contact| {
            let [a, b] = contact
                .between
                .map(|(member, surface)| name_on(&assembly.members()[member].board, surface));
            let (least, most) = (measured(contact.least), measured(contact.most));
            let how = if contact.most.as_nm() < 0 {
                format!("runs into it by {}", measured(-contact.least))
            } else if contact.least.as_nm() < 0 {
                format!("runs into it by {} at one end", measured(-contact.least))
            } else if least == most {
                format!("is open {most}")
            } else {
                format!("is open {least} to {most}")
            };
            format!("A {b} {how} where it meets a {a}, so it's not fastened there.")
        })
        .collect();
    format!(" {}", lines.join(" "))
}

/// A length as a tape measure reads it, to the nearest 64th.
fn measured(length: Length) -> Length {
    length.rounded_to(Length::from_inch_fraction(1, 64))
}

/// The surface of `board` at `point`, with the surface facing `normal` there, both in the
/// board's mesh axes (`w`, `t`, `l` along x, y, z, in meters): of the surfaces facing about the
/// same way, the one whose plane the point is on. Every surface is a plane, the faces like any
/// side, so a side that leans, even one sloping nearly as flat as a face, as a scarf does, is
/// found the same way, and so is a slot's bottom or side.
pub fn surface_at(board: &Board, point: Vec3, normal: Vec3) -> Option<Surface> {
    let normal = normal.normalize_or_zero().as_dvec3();
    let point = point.as_dvec3();
    let thickness = board.thickness().as_meters_f32() as f64;
    let mut found: Vec<(Surface, f64, f64)> = surfaces(board)
        .into_iter()
        .chain(slot_surfaces(board))
        .filter_map(|surface| {
            // Facing out of the wood, and where its plane is that way, in meters.
            let (out, at) = match surface {
                Surface::Top => (DVec3::Y, thickness),
                Surface::Bottom => (DVec3::NEG_Y, 0.0),
                Surface::Side(edge) | Surface::Slot(edge) => {
                    let out = DVec3::new(edge.out.w as f64, edge.lean as f64, edge.out.l as f64);
                    let scale = out.length();
                    (out / scale, edge.at as f64 / scale / 1e9)
                }
            };
            let facing = out.dot(normal);
            let off = (out.dot(point) - at).abs();
            (facing > 0.9 && off < 0.002).then_some((surface, facing, off))
        })
        .collect();
    // On a plane, to well within a mesh's float precision, the one facing most nearly the way
    // the point does; where two meet, the one it's nearest otherwise.
    let on = |off: f64| off < 1e-5;
    found.sort_by(|a, b| {
        on(b.2).cmp(&on(a.2)).then(if on(a.2) && on(b.2) {
            b.1.total_cmp(&a.1)
        } else {
            a.2.total_cmp(&b.2)
        })
    });
    found.first().map(|&(surface, ..)| surface)
}

/// How a joint is held: "glue", "brads", "screws".
pub fn fastened(fastener: Fastener) -> &'static str {
    match fastener {
        Fastener::Glue => "glue",
        Fastener::Brads => "brads",
        Fastener::Screws => "screws",
    }
}

/// What `board`'s surface is called, as `name` has it, with a slot's bottom told from its
/// sides: "slot bottom", "slot side".
pub fn name_on(board: &Board, surface: Surface) -> &'static str {
    match surface {
        Surface::Slot(plane) if slot_bottoms(board).contains(&plane) => "slot bottom",
        Surface::Slot(_) => "slot side",
        surface => name(surface),
    }
}

#[cfg(test)]
mod tests {
    use bevy::mesh::{Indices, VertexAttributeValues};
    use woodsim::{Angle, Length, MiterSaw, Orientation, Pass, surfaces};

    use super::*;

    /// A frame piece `length` long point to long point, mitered 45° at both ends.
    fn frame_piece_of(length: Length) -> Board {
        let stock = Board::new(
            Length::from_inches(48),
            Length::from_inch_fraction(3, 2),
            Length::from_inch_fraction(3, 4),
        );
        let saw = |degrees| MiterSaw {
            miter: Angle::from_degrees(degrees),
            ..MiterSaw::twelve_inch()
        };
        let Ok(Pass::Cut(trim)) =
            saw(-45).crosscut(&stock, Orientation::default(), Length::from_inches(2))
        else {
            panic!("expected the end trimmed");
        };
        let Ok(Pass::Cut(cut)) =
            saw(45).crosscut(&trim.offcut.unwrap().board, Orientation::default(), length)
        else {
            panic!("expected a frame piece");
        };
        cut.piece.board
    }

    fn frame_piece() -> Board {
        frame_piece_of(Length::from_inches(10))
    }

    fn plain() -> Board {
        Board::new(
            Length::from_inches(24),
            Length::from_inches(4),
            Length::from_inch_fraction(3, 4),
        )
    }

    fn miter(board: &Board, far_end: bool) -> Surface {
        surfaces(board)
            .into_iter()
            .find(|surface| {
                matches!(surface, Surface::Side(edge) if edge.out.w != 0 && (edge.out.l > 0) == far_end)
            })
            .unwrap()
    }

    fn end(board: &Board, far: bool) -> Surface {
        surfaces(board)
            .into_iter()
            .find(|surface| {
                matches!(surface, Surface::Side(edge) if edge.out.w == 0 && (edge.out.l > 0) == far)
            })
            .unwrap()
    }

    fn spot(group: usize, member: usize, surface: Surface) -> Spot {
        Spot {
            group,
            member,
            surface,
        }
    }

    /// Clicks `spot` in the middle of the surface.
    fn click(bench: &mut BenchStation, spot: Spot) {
        let extent = bench.groups[spot.group]
            .assembly
            .extent((spot.member, spot.surface))
            .unwrap();
        let mut middle = [0.0; 3];
        for axis in 0..2 {
            let (least, most) = extent.reach[axis];
            let along = (least.as_nm() + most.as_nm()) as f64 / 2.0;
            for (middle, direction) in middle.iter_mut().zip(extent.directions[axis]) {
                *middle += along * direction;
            }
        }
        // Somewhere on the surface's plane will do: only how far along it matters.
        bench.pick(spot, middle);
    }

    /// Puts the parts on the bench, one per group.
    fn bench_with(boards: &[Board]) -> BenchStation {
        let mut bench = BenchStation::default();
        for board in boards {
            bench.set_down(Workpiece::Board(board.clone()), None);
        }
        bench
    }

    #[test]
    fn parts_set_down_lie_flat_on_the_bench_clear_of_each_other() {
        let bench = bench_with(&[plain(), plain(), frame_piece()]);
        assert_eq!(bench.groups.len(), 3);
        for (i, a) in bench.groups.iter().enumerate() {
            let (rotation, corner) = a.pose(0);
            // Face up, the length along the bench, resting on the top.
            assert!((rotation * Vec3::Z).abs_diff_eq(Vec3::X, 1e-6));
            assert!((rotation * Vec3::Y).abs_diff_eq(Vec3::Y, 1e-6));
            assert!(corner.y.abs() < 1e-6);
            for b in &bench.groups[i + 1..] {
                assert!(a.footprint().intersect(b.footprint()).is_empty());
            }
        }
    }

    #[test]
    fn two_mitered_ends_go_together_face_up_into_a_corner() {
        let piece = frame_piece();
        let mut bench = bench_with(&[piece.clone(), piece.clone()]);
        click(&mut bench, spot(1, 0, miter(&piece, false)));
        assert!(bench.lining_up.is_none());
        click(&mut bench, spot(0, 0, miter(&piece, true)));
        let lining_up = bench.lining_up.expect("lining up");
        assert_eq!(lining_up.onto, spot(0, 0, miter(&piece, true)));
        assert_eq!(
            bench.status(false),
            "Its mitered end against the mitered end. Fasten it with glue. Every joint is \
             tight."
        );
        // While it's lined up, it's shown where it would go, and nowhere else.
        let shown = bench.shown();
        assert_eq!(shown.len(), 2);
        assert!(shown[1].moving && shown[1].group == 1);
        // Face up, at a right angle to the first.
        assert!((shown[1].rotation * Vec3::Y).abs_diff_eq(Vec3::Y, 1e-6));
        let lengths = shown.iter().map(|part| part.rotation * Vec3::Z);
        let [a, b] = <[Vec3; 2]>::try_from(lengths.collect::<Vec<_>>()).unwrap();
        assert!(a.dot(b).abs() < 1e-6);

        assert!(bench.glue());
        assert_eq!(bench.groups.len(), 1);
        assert_eq!(bench.groups[0].assembly.members().len(), 2);
        assert!(bench.status(false).ends_with("Every joint is tight."));
    }

    #[test]
    fn four_frame_pieces_close_tight_and_a_long_one_says_where_it_is_open() {
        let piece = frame_piece();
        let long = frame_piece_of(Length::from_inches(10) + Length::from_inch_fraction(1, 16));
        let mut bench = bench_with(&[piece.clone(), piece.clone(), long, piece.clone()]);
        // Each next loose part's near miter against the frame's last far miter. The frame is
        // always group 0; the next loose part, once the others are glued on, group 1.
        for last in 0..3 {
            let next = bench.groups[1].assembly.members()[0].board.clone();
            let frame = bench.groups[0].assembly.members()[last].board.clone();
            click(&mut bench, spot(1, 0, miter(&next, false)));
            click(&mut bench, spot(0, last, miter(&frame, true)));
            if last == 2 {
                assert!(
                    bench
                        .status(false)
                        .ends_with("where it meets a mitered end, so it's not fastened there."),
                    "{}",
                    bench.status(false)
                );
            }
            assert!(bench.glue());
        }
        assert_eq!(bench.groups.len(), 1);
        assert_eq!(bench.groups[0].assembly.members().len(), 4);
    }

    #[test]
    fn the_last_frame_piece_is_glued_at_both_its_corners() {
        let piece = frame_piece();
        let mut bench = bench_with(&[piece.clone(), piece.clone(), piece.clone(), piece.clone()]);
        for last in 0..3 {
            click(&mut bench, spot(1, 0, miter(&piece, false)));
            click(&mut bench, spot(0, last, miter(&piece, true)));
            if last == 2 {
                assert!(
                    bench
                        .status(false)
                        .contains("Fasten it with glue at 2 joints."),
                    "{}",
                    bench.status(false)
                );
            }
            assert!(bench.glue());
        }
        assert_eq!(bench.groups[0].assembly.joints().len(), 4);
        assert!(
            bench
                .status(false)
                .ends_with("The last part went on with glue at 2 joints. Every joint is tight."),
            "{}",
            bench.status(false)
        );
        // Taken back apart, both its joints come off with it.
        assert!(bench.take_apart());
        assert_eq!(bench.groups[0].assembly.joints().len(), 2);
    }

    #[test]
    fn plans_are_for_the_glue_up_pointed_at_or_else_the_last_put_together() {
        let piece = frame_piece();
        let mut bench = bench_with(&[piece.clone(), piece.clone(), plain()]);
        assert_eq!(bench.to_plan(None).unwrap().members().len(), 1);
        click(&mut bench, spot(1, 0, miter(&piece, false)));
        click(&mut bench, spot(0, 0, miter(&piece, true)));
        assert!(bench.glue());
        // The corner, the last put together, is group 0; the plain board group 1.
        assert_eq!(bench.to_plan(None).unwrap().members().len(), 2);
        assert_eq!(bench.to_plan(Some(1)).unwrap().members().len(), 1);
        // Drawn up as it lies on the bench: turned over, it's drawn turned over.
        let lying = bench.to_plan(None).unwrap();
        bench.turn_group(0, Rotation::quarter_turns(0, 2));
        let turned = bench.to_plan(None).unwrap();
        assert!(lying.members()[0].axes()[2][2] > 0.999);
        assert!(turned.members()[0].axes()[2][2] < -0.999);
        // What came of it says so until the next pick.
        bench.note("Drew up plans.".to_string());
        assert!(
            bench
                .status(false)
                .starts_with("Drew up plans. Click the surface")
        );
        click(&mut bench, spot(1, 0, end(&plain(), true)));
        assert!(!bench.status(false).contains("Drew up plans"));
        assert!(BenchStation::default().to_plan(None).is_none());
    }

    #[test]
    fn an_end_slides_from_stop_to_stop_across_a_face_and_can_be_nudged() {
        let (base, upright) = (plain(), plain());
        let mut bench = bench_with(&[base.clone(), upright.clone()]);
        click(&mut bench, spot(1, 0, end(&upright, true)));
        // Clicked near the base's `l = 0` end, the upright goes flush with it.
        let near_end = [Length::from_inches(1).as_nm() as f64, 0.0, 0.0];
        bench.pick(spot(0, 0, Surface::Top), near_end);
        let stops = bench.stops().unwrap();
        let slide = |bench: &BenchStation| bench.lining_up.unwrap().mating.slide;
        assert_eq!(slide(&bench)[0], stops[0][0]);
        assert!(bench.status(false).contains("flush with one side"));

        bench.slide(0, true, false);
        assert_eq!(slide(&bench)[0], stops[0][1]);
        assert!(bench.status(false).contains("centered"));
        bench.slide(0, true, true);
        assert_eq!(slide(&bench)[0], stops[0][1] + NUDGE);
        bench.slide(0, true, false);
        assert_eq!(slide(&bench)[0], stops[0][2]);
        // It goes no further than flush with the far side.
        bench.slide(0, true, true);
        assert_eq!(slide(&bench)[0], stops[0][2]);

        // Turned a quarter turn around the joint, it stays flush with the same side.
        bench.turn_around_joint(1);
        let turned = bench.stops().unwrap();
        assert_eq!(slide(&bench)[0], turned[0][2]);
        assert!(bench.glue());
        assert!(bench.groups[0].assembly.fit().iter().all(Contact::is_tight));
    }

    #[test]
    fn a_joint_comes_back_apart_and_its_part_goes_back_where_it_was() {
        let piece = frame_piece();
        let mut bench = bench_with(&[piece.clone(), piece.clone()]);
        let before = bench.groups.clone();
        click(&mut bench, spot(1, 0, miter(&piece, false)));
        click(&mut bench, spot(0, 0, miter(&piece, true)));
        assert!(bench.glue());
        assert!(bench.take_apart());
        assert_eq!(bench.groups, before);
        // Nothing left to take apart.
        assert!(!bench.take_apart());
    }

    #[test]
    fn a_loose_part_comes_off_as_a_board_and_a_glue_up_whole() {
        let piece = frame_piece();
        let mut bench = bench_with(&[piece.clone(), piece.clone(), plain()]);
        click(&mut bench, spot(1, 0, miter(&piece, false)));
        click(&mut bench, spot(0, 0, miter(&piece, true)));
        assert!(bench.glue());
        // The loose board comes off as itself.
        click(&mut bench, spot(1, 0, Surface::Top));
        assert_eq!(bench.take_picked(), Some(Workpiece::Board(plain())));
        // The frame's corner comes off whole, turned as it lay, with its joint.
        bench.turn_group(0, Rotation::quarter_turns(2, 1));
        click(&mut bench, spot(0, 1, Surface::Top));
        assert!(bench.status(false).ends_with("or pick up the glue-up."));
        let Some(Workpiece::GlueUp(corner)) = bench.take_picked() else {
            panic!("expected the glue-up");
        };
        assert!(bench.groups.is_empty());
        assert_eq!(corner.members().len(), 2);
        assert_eq!(corner.joints().len(), 1);
        // Set back down, it lies as it did.
        let before = corner.clone();
        bench.set_down(Workpiece::GlueUp(corner), None);
        assert_eq!(bench.groups[0].assembly, before);
        assert_eq!(bench.groups[0].turn, Rotation::NONE);
        let axes = |k: usize| bench.groups[0].pose(k).0 * Vec3::Z;
        assert!(axes(0).abs_diff_eq(Vec3::NEG_Z, 1e-6), "{}", axes(0));
    }

    #[test]
    fn a_part_turns_and_flips_where_it_lies() {
        let mut bench = bench_with(&[plain()]);
        let middle = bench.groups[0].footprint().center();
        bench.turn_group(0, Rotation::quarter_turns(2, 1));
        let (rotation, _) = bench.groups[0].pose(0);
        // Its length now runs away from the operator, and it's still where it was.
        assert!((rotation * Vec3::Z).abs_diff_eq(Vec3::NEG_Z, 1e-6));
        assert!(
            bench.groups[0]
                .footprint()
                .center()
                .abs_diff_eq(middle, 1e-6)
        );
        bench.turn_group(0, Rotation::quarter_turns(0, 2));
        let (rotation, corner) = bench.groups[0].pose(0);
        assert!((rotation * Vec3::Y).abs_diff_eq(Vec3::NEG_Y, 1e-6));
        // Upside down, its corner is on top: its thickness above the bench.
        assert!((corner.y - inches(0.75)).abs() < 1e-6);
    }

    #[test]
    fn two_halves_of_a_frame_go_together() {
        let piece = frame_piece();
        let mut bench = bench_with(&[piece.clone(), piece.clone(), piece.clone(), piece.clone()]);
        click(&mut bench, spot(1, 0, miter(&piece, false)));
        click(&mut bench, spot(0, 0, miter(&piece, true)));
        assert!(bench.glue());
        // The other two, now groups 1 and 2.
        click(&mut bench, spot(2, 0, miter(&piece, false)));
        click(&mut bench, spot(1, 0, miter(&piece, true)));
        assert!(bench.glue());
        // One half's free end against the other's.
        click(&mut bench, spot(1, 0, miter(&piece, false)));
        click(&mut bench, spot(0, 1, miter(&piece, true)));
        assert!(bench.status(false).ends_with("Every joint is tight."));
        assert!(bench.glue());
        assert_eq!(bench.groups.len(), 1);
        let fit = bench.groups[0].assembly.fit();
        assert_eq!(fit.len(), 4);
        assert!(fit.iter().all(Contact::is_tight));
    }

    #[test]
    fn every_surface_of_both_halves_of_a_miter_cut_can_be_found_on_its_mesh() {
        let board = Board::new(
            Length::from_inches(24),
            Length::from_inch_fraction(7, 2),
            Length::from_inch_fraction(3, 4),
        );
        for degrees in [45, -45] {
            let saw = MiterSaw {
                miter: Angle::from_degrees(degrees),
                ..MiterSaw::twelve_inch()
            };
            let Ok(Pass::Cut(cut)) =
                saw.crosscut(&board, Orientation::default(), Length::from_inches(12))
            else {
                panic!("expected the board cut in two");
            };
            for half in [cut.piece.board, cut.offcut.unwrap().board] {
                let mesh = crate::meshes::lumber_mesh(&half, Vec3::ZERO);
                let (Some(VertexAttributeValues::Float32x3(positions)), Some(normals)) = (
                    mesh.attribute(Mesh::ATTRIBUTE_POSITION),
                    mesh.attribute(Mesh::ATTRIBUTE_NORMAL),
                ) else {
                    panic!("expected positions and normals");
                };
                let VertexAttributeValues::Float32x3(normals) = normals else {
                    panic!("expected normals");
                };
                let Some(Indices::U32(indices)) = mesh.indices() else {
                    panic!("expected indices");
                };
                let mut found = Vec::new();
                for triangle in indices.chunks(3) {
                    let middle = triangle
                        .iter()
                        .map(|&i| Vec3::from(positions[i as usize]))
                        .sum::<Vec3>()
                        / 3.0;
                    let normal = Vec3::from(normals[triangle[0] as usize]);
                    let surface = surface_at(&half, middle, normal);
                    assert!(surface.is_some(), "nothing at {middle} facing {normal}");
                    found.extend(surface);
                }
                for surface in surfaces(&half) {
                    assert!(found.contains(&surface), "{surface:?} at {degrees}");
                }
            }
        }
    }

    #[test]
    fn two_halves_of_one_miter_cut_make_a_corner_once_one_is_flipped() {
        let saw = MiterSaw {
            miter: Angle::from_degrees(45),
            ..MiterSaw::twelve_inch()
        };
        let Ok(Pass::Cut(cut)) = saw.crosscut(&plain(), Orientation::default(), inches_of(12))
        else {
            panic!("expected the board cut in two");
        };
        let (piece, offcut) = (cut.piece.board, cut.offcut.unwrap().board);
        let slanted = |board: &Board| {
            surfaces(board)
                .into_iter()
                .find(|surface| {
                    matches!(surface, Surface::Side(edge) if edge.out.l != 0 && edge.out.w != 0)
                })
                .unwrap()
        };
        let mut bench = bench_with(&[piece.clone(), offcut.clone()]);
        click(&mut bench, spot(1, 0, slanted(&offcut)));
        click(&mut bench, spot(0, 0, slanted(&piece)));
        assert!(
            bench
                .status(false)
                .contains("flip it over to make a corner.")
        );
        bench.turn_around_joint(2);
        let status = bench.status(false);
        assert!(!status.contains("flip it over"), "{status}");
        assert!(status.ends_with("Every joint is tight."), "{status}");
        let shown = bench.shown();
        assert!(
            (shown[0].rotation * Vec3::Z)
                .dot(shown[1].rotation * Vec3::Z)
                .abs()
                < 1e-6
        );
    }

    fn inches_of(n: i64) -> Length {
        Length::from_inches(n)
    }

    #[test]
    fn a_click_that_finds_no_surface_says_so_until_the_next_pick() {
        let mut bench = bench_with(&[plain()]);
        bench.missed();
        assert!(
            bench
                .status(false)
                .starts_with("There's no flat surface there")
        );
        click(&mut bench, spot(0, 0, Surface::Top));
        assert!(bench.status(false).starts_with("Moving its top face."));
    }

    #[test]
    fn a_click_anywhere_on_a_cut_board_picks_the_surface_it_lands_on() {
        use bevy::mesh::VertexAttributeValues;
        let saw = |fence: Length, tilt: i32, gauge: Option<i32>| woodsim::TableSaw {
            blade: woodsim::Blade {
                diameter: Length::from_inches(10),
                kerf: woodsim::Blade::FULL_KERF,
            },
            fence,
            height: Length::from_inches(3),
            miter_gauge: gauge.map(|_| Length::from_inches(10)),
            gauge_angle: Angle::from_degrees(gauge.unwrap_or(0)),
            tilt: Angle::from_degrees(tilt),
        };
        let (two, sixty_fourth) = (Length::from_inches(2), Length::from_inch_fraction(1, 64));
        let flat = Orientation::default();
        // Bevels along an edge and across an end, face up and face down, a compound miter, a
        // face a tilted resaw slopes, and a scarf: a board stood on end, cut with the blade
        // tilted 15°, its end sloping off as flat as 15° from its face.
        for (saw, lie) in [
            (saw(two, 15, None), flat),
            (saw(two, 45, None), flat.flipped()),
            (saw(two, 15, Some(0)), flat.turned_left()),
            (saw(two, 15, Some(0)), flat.turned_left().flipped()),
            (saw(two, 15, Some(20)), flat.turned_left()),
            (
                saw(Length::from_inch_fraction(1, 4), 15, None),
                flat.tipped(),
            ),
            (saw(sixty_fourth, 15, None), flat.stood()),
        ] {
            let board = Board::new(
                Length::from_inches(24),
                Length::from_inches(3),
                Length::from_inch_fraction(3, 4),
            );
            let Ok(Pass::Cut(cut)) = saw.rip(&board, lie) else {
                panic!("expected {lie:?} cut");
            };
            for piece in [cut.piece.board, cut.offcut.expect("both sides").board] {
                let mesh = crate::meshes::lumber_mesh(&piece, Vec3::ZERO);
                let (
                    Some(VertexAttributeValues::Float32x3(points)),
                    Some(VertexAttributeValues::Float32x3(normals)),
                    Some(Indices::U32(indices)),
                ) = (
                    mesh.attribute(Mesh::ATTRIBUTE_POSITION),
                    mesh.attribute(Mesh::ATTRIBUTE_NORMAL),
                    mesh.indices(),
                )
                else {
                    panic!("lumber meshes have positions, normals, and indices");
                };
                let thickness = piece.thickness().as_meters_f32();
                let mut picked = Vec::new();
                for triangle in indices.chunks(3) {
                    let corners = [0, 1, 2].map(|k| Vec3::from(points[triangle[k] as usize]));
                    let normal = Vec3::from(normals[triangle[0] as usize]);
                    // The surface whose plane the triangle lies in, facing the way it does.
                    let Some(surface) = surfaces(&piece).into_iter().find(|&surface| {
                        let (out, at) = match surface {
                            Surface::Top => (Vec3::Y, thickness),
                            Surface::Bottom => (Vec3::NEG_Y, 0.0),
                            Surface::Side(edge) | Surface::Slot(edge) => {
                                let out = Vec3::new(
                                    edge.out.w as f32,
                                    edge.lean as f32,
                                    edge.out.l as f32,
                                );
                                let scale = out.length();
                                (out / scale, (edge.at as f64 / scale as f64 / 1e9) as f32)
                            }
                        };
                        out.dot(normal) > 0.999
                            && corners.iter().all(|&p| (out.dot(p) - at).abs() < 1e-5)
                    }) else {
                        continue;
                    };
                    let middle = corners.iter().sum::<Vec3>() / 3.0;
                    assert_eq!(
                        surface_at(&piece, middle, normal),
                        Some(surface),
                        "{lie:?} at {middle}"
                    );
                    picked.push(surface);
                }
                // Each of its leaning sides was there to click, and was picked.
                for surface in surfaces(&piece) {
                    if matches!(surface, Surface::Side(edge) if edge.leans()) {
                        assert!(picked.contains(&surface), "{lie:?}: {}", name(surface));
                    }
                }
            }
        }
    }

    /// The pasted rabbet: 7/8" wide and 3/8" deep across a 1x8's end, in seven strokes.
    const RABBET: &str = include_str!("../../woodsim/tests/pasted/rabbet-across-the-end.txt");

    #[test]
    fn a_click_in_a_slot_picks_its_bottom_or_side() {
        let saw = |fence: Length, height: Length, tilt: i32| woodsim::TableSaw {
            blade: woodsim::Blade {
                diameter: Length::from_inches(10),
                kerf: woodsim::Blade::FULL_KERF,
            },
            fence,
            height,
            miter_gauge: None,
            gauge_angle: Angle::ZERO,
            tilt: Angle::from_degrees(tilt),
        };
        let board = Board::new(
            Length::from_inches(24),
            Length::from_inches(3),
            Length::from_inch_fraction(3, 4),
        );
        let quarter = Length::from_inch_fraction(1, 4);
        let grooved = |saw: woodsim::TableSaw, lie: Orientation| {
            let Ok(Pass::Kerfed(grooved)) = saw.rip(&board, lie) else {
                panic!("expected a groove");
            };
            grooved
        };
        let woodsim::Wood::GlueUp(rabbeted) = woodsim::Recipe::read(RABBET).unwrap().wood else {
            panic!("a glue-up of one");
        };
        // A groove in the bottom face, one in the top tilted 15°, one in an edge, a rabbet at
        // the fence, and the rabbet nibbled across an end.
        for (piece, sides) in [
            (
                grooved(
                    saw(Length::from_inches(1), quarter, 0),
                    Orientation::default(),
                ),
                2,
            ),
            (
                grooved(
                    saw(Length::from_inches(1), quarter, 15),
                    Orientation::default().flipped(),
                ),
                2,
            ),
            (
                grooved(
                    saw(Length::from_inch_fraction(5, 16), quarter, 0),
                    Orientation::default().tipped(),
                ),
                2,
            ),
            (
                grooved(saw(Length::ZERO, quarter, 0), Orientation::default()),
                1,
            ),
            (rabbeted.members()[0].board.clone(), 1),
        ] {
            let mesh = crate::meshes::lumber_mesh(&piece, Vec3::ZERO);
            let (
                Some(VertexAttributeValues::Float32x3(points)),
                Some(VertexAttributeValues::Float32x3(normals)),
                Some(Indices::U32(indices)),
            ) = (
                mesh.attribute(Mesh::ATTRIBUTE_POSITION),
                mesh.attribute(Mesh::ATTRIBUTE_NORMAL),
                mesh.indices(),
            )
            else {
                panic!("lumber meshes have positions, normals, and indices");
            };
            let slots = slot_surfaces(&piece);
            let mut picked = Vec::new();
            for triangle in indices.chunks(3) {
                let corners = [0, 1, 2].map(|k| Vec3::from(points[triangle[k] as usize]));
                let normal = Vec3::from(normals[triangle[0] as usize]);
                // The slot's surface whose plane the triangle lies in, facing the way it does.
                let Some(&surface) = slots.iter().find(|&&surface| {
                    let Surface::Slot(edge) = surface else {
                        return false;
                    };
                    let out = Vec3::new(edge.out.w as f32, edge.lean as f32, edge.out.l as f32);
                    let scale = out.length();
                    let at = (edge.at as f64 / scale as f64 / 1e9) as f32;
                    (out / scale).dot(normal) > 0.999
                        && corners
                            .iter()
                            .all(|&p| ((out / scale).dot(p) - at).abs() < 1e-5)
                }) else {
                    continue;
                };
                let middle = corners.iter().sum::<Vec3>() / 3.0;
                assert_eq!(
                    surface_at(&piece, middle, normal),
                    Some(surface),
                    "at {middle}"
                );
                picked.push(surface);
            }
            // Its bottom and each side are there to click, and were picked.
            let names: Vec<&str> = slots
                .iter()
                .map(|&surface| name_on(&piece, surface))
                .collect();
            assert_eq!(
                names.iter().filter(|&&name| name == "slot bottom").count(),
                1
            );
            assert_eq!(
                names.iter().filter(|&&name| name == "slot side").count(),
                sides
            );
            for &surface in &slots {
                assert!(picked.contains(&surface), "{}", name_on(&piece, surface));
            }
        }
    }

    #[test]
    fn two_scarfs_glue_end_to_end_lying_flat_on_the_bench() {
        let saw = woodsim::TableSaw {
            blade: woodsim::Blade {
                diameter: Length::from_inches(10),
                kerf: woodsim::Blade::FULL_KERF,
            },
            fence: Length::from_inch_fraction(1, 64),
            height: Length::from_inches(3),
            miter_gauge: None,
            gauge_angle: Angle::ZERO,
            tilt: Angle::from_degrees(15),
        };
        let Ok(Pass::Cut(cut)) = saw.rip(&plain(), Orientation::default().stood()) else {
            panic!("expected the end scarfed");
        };
        let scarfed = cut.piece.board;
        let slope = Surface::Side(scarfed.tapers()[0].0);
        assert_eq!(name(slope), "tapered face");
        let mut station = BenchStation::default();
        station.set_down(Workpiece::Board(scarfed.clone()), None);
        station.set_down(Workpiece::Board(scarfed.clone()), None);
        let spot = |group| Spot {
            group,
            member: 0,
            surface: slope,
        };
        station.pick(spot(0), [0.0; 3]);
        station.pick(spot(1), [0.0; 3]);
        assert!(station.glue());
        // Set down alike, slope up, one goes on turned over, end to end, flat on the bench,
        // not tipped up off it with its slope against the other's.
        let [group] = &station.groups[..] else {
            panic!("expected one glue-up");
        };
        let axes = group
            .assembly
            .members()
            .iter()
            .map(|member| member.rotation.then(group.turn).columns())
            .collect::<Vec<_>>();
        for [_, _, thickness] in &axes {
            assert!((thickness[2].abs() - 1.0).abs() < 1e-9, "{thickness:?}");
        }
        let along: f64 = (0..3).map(|k| axes[0][0][k] * axes[1][0][k]).sum();
        assert!(along < -0.999);
        assert!(
            group
                .assembly
                .fit()
                .iter()
                .all(|contact| contact.is_tight())
        );
    }

    #[test]
    fn a_click_on_a_bevel_picks_the_beveled_edge() {
        let saw = woodsim::TableSaw {
            blade: woodsim::Blade {
                diameter: Length::from_inches(10),
                kerf: woodsim::Blade::FULL_KERF,
            },
            fence: Length::from_inches(2),
            height: Length::from_inches(1),
            miter_gauge: None,
            gauge_angle: Angle::ZERO,
            tilt: Angle::from_degrees(45),
        };
        let Ok(Pass::Cut(cut)) = saw.rip(&plain(), Orientation::default()) else {
            panic!("expected a beveled rip");
        };
        let piece = cut.piece.board;
        let [(bevel, angle)] = piece.bevels()[..] else {
            panic!("expected one bevel");
        };
        assert_eq!(angle, Angle::from_degrees(45));
        let surface = Surface::Side(bevel);
        assert_eq!(name(surface), "beveled edge");
        // The middle of the bevel, in the mesh's axes, facing the way it leans.
        let outline = &woodsim::outline(&piece, surface)[0];
        let middle = outline
            .iter()
            .map(|&[l, w, t]| Vec3::new(w.as_meters_f32(), t.as_meters_f32(), l.as_meters_f32()))
            .sum::<Vec3>()
            / outline.len() as f32;
        let normal = Vec3::new(bevel.out.w as f32, bevel.lean as f32, bevel.out.l as f32);
        assert_eq!(
            surface_at(&piece, middle, normal.normalize()),
            Some(surface)
        );
        // It faces 45° up or down, so it isn't the top or bottom face.
        assert!(normal.normalize().y.abs() > 0.7);
    }

    #[test]
    fn a_click_finds_the_surface_under_it() {
        let piece = frame_piece();
        let thickness = piece.thickness().as_meters_f32();
        // The top, from above.
        assert_eq!(
            surface_at(&piece, Vec3::new(0.01, thickness, 0.1), Vec3::Y),
            Some(Surface::Top)
        );
        // The mitered end at `l = 0`: its long point is at the `w = width` edge, so it faces
        // back along the length and out toward `w = 0`.
        let width = piece.width().as_meters_f32();
        let normal = Vec3::new(-1.0, 0.0, -1.0).normalize();
        let on_miter = Vec3::new(width / 2.0, thickness / 2.0, width / 2.0);
        assert_eq!(
            surface_at(&piece, on_miter, normal),
            Some(miter(&piece, false))
        );
        // The long edge.
        assert_eq!(
            surface_at(&piece, Vec3::new(width, 0.005, 0.1), Vec3::X).map(name),
            Some("edge")
        );
    }
}
