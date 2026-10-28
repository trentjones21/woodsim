//! The workbench's state: the parts on it, loose or put together, and the joint being lined
//! up. Pick a surface of one part, then the surface of another that it goes against: the first
//! part moves there, to be turned around the joint and slid across it until it's lined up, and
//! is fastened there for good when it's glued. A joint can come back apart, the last first.
//!
//! `woodsim` puts the parts together and measures how the joints fit; this module keeps track
//! of what's on the bench and where, and puts it into words.

use bevy::prelude::*;
use woodsim::{
    Assembly, Board, Contact, Fastener, Length, Mating, Member, Rotation, Surface, outline,
    surfaces,
};

use crate::shop::inches;

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
        self.assembly
            .members()
            .iter()
            .flat_map(|member| {
                [Surface::Bottom, Surface::Top]
                    .into_iter()
                    .flat_map(|surface| outline(&member.board, surface))
                    .flatten()
                    .map(|[l, w, t]| onto_bench(self.turn, meters(member.point(l, w, t))))
            })
            .fold((Vec3::MAX, Vec3::MIN), |(low, high), corner| {
                (low.min(corner), high.max(corner))
            })
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
    /// Sets `board` down loose on the bench, as near `near` (across the top, in meters from
    /// its middle) as there's room.
    pub fn set_down(&mut self, board: Board, near: Option<Vec2>) {
        let mut group = Group::of(board);
        group.center_on(near.unwrap_or(SET_DOWN_AT));
        let spot = self.free_spot(&group, None);
        group.center_on(spot);
        self.groups.push(group);
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

    /// Puts the picked surface against `onto`: turned the way that leaves its part most
    /// nearly as it lies now, keeping whichever side is up, and slid to whichever end of
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
            let up = lies.apply(up_now)[2];
            let turned = moving.turn.inverse().then(lies).degrees();
            Some(((-up * 1e3).round() as i64, (turned * 1e2).round() as i64))
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
        target.joined.push(Joined {
            before,
            group: moving,
            order: self.joints,
        });
        self.groups.remove(picked.group);
        self.picked = None;
        self.lining_up = None;
        let target = lining_up.onto.group - usize::from(picked.group < lining_up.onto.group);
        self.make_room(target);
        true
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
        let mut apart = joined.group;
        let spot = self.free_spot(&apart, None);
        apart.center_on(spot);
        self.groups.push(apart);
        true
    }

    /// Takes the picked part off the bench, if it's loose: a part fastened to others stays.
    pub fn take_picked(&mut self) -> Option<Board> {
        let picked = self.picked?;
        if !self.groups[picked.group].is_loose() {
            return None;
        }
        self.picked = None;
        self.lining_up = None;
        let group = self.groups.remove(picked.group);
        Some(group.assembly.members()[0].board.clone())
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

    /// One line on what's happening at the bench, given what's in hand.
    pub fn status(&self, holding: Option<&Board>) -> String {
        if holding.is_some() {
            return "Click the bench to set it down.".to_string();
        }
        if self.groups.is_empty() {
            return "Nothing on the bench. Pick up a part and click the bench to set it down."
                .to_string();
        }
        let Some(picked) = self.picked else {
            let fit = self
                .latest()
                .map(|index| describe_fit(&self.groups[index].assembly, 0))
                .unwrap_or_default();
            return format!(
                "Click the surface of a part to move it: an end, an edge, or a face. Q/E turn \
                 the part under the pointer to bring a side facing away around.{fit}"
            );
        };
        let mine = name(picked.surface);
        let (Some(lining_up), Some(preview)) = (self.lining_up, self.preview()) else {
            let take = if self.groups[picked.group].is_loose() {
                " Space picks it up."
            } else {
                ""
            };
            return format!(
                "Moving its {mine}. Click the surface it goes against; if it faces away, Q/E \
                 turn the part under the pointer.{take}"
            );
        };
        let theirs = name(lining_up.onto.surface);
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
            " They run on in a straight line; F flips it over to make a corner."
        } else {
            ""
        };
        format!(
            "Its {mine} against the {theirs}{lined_up}.{flip} Enter fastens it with {}.{}",
            fastened(self.fastener),
            describe_fit(&preview.assembly, first)
        )
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
/// from `first` on: the ones that aren't tight, worst first, or that they all are. Starts with
/// a space, or is empty if there are no joints.
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
            let [(_, a), (_, b)] = contact.between;
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
            format!("A {} {how} where it meets a {}.", name(b), name(a))
        })
        .collect();
    format!(" {}", lines.join(" "))
}

/// A length as a tape measure reads it, to the nearest 64th.
fn measured(length: Length) -> Length {
    length.rounded_to(Length::from_inch_fraction(1, 64))
}

/// The surface of `board` at `point`, with the surface facing `normal` there, both in the
/// board's mesh axes (`w`, `t`, `l` along x, y, z, in meters).
pub fn surface_at(board: &Board, point: Vec3, normal: Vec3) -> Option<Surface> {
    let normal = normal.normalize_or_zero();
    if normal.y.abs() > 0.9 {
        return Some(if normal.y > 0.0 {
            Surface::Top
        } else {
            Surface::Bottom
        });
    }
    // The side facing the same way whose plane the point is nearest.
    surfaces(board)
        .into_iter()
        .filter_map(|surface| {
            let Surface::Side(edge) = surface else {
                return None;
            };
            let out = Vec2::new(edge.out.w as f32, edge.out.l as f32);
            let scale = out.length();
            let facing = out.dot(Vec2::new(normal.x, normal.z)) / scale;
            let plane = edge.at as f64 / scale as f64 / 1e9;
            let off = (out.dot(Vec2::new(point.x, point.z)) / scale) as f64 - plane;
            (facing > 0.95 && off.abs() < 0.002).then_some((surface, off.abs()))
        })
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(surface, _)| surface)
}

/// How a joint is held: "glue", "brads", "screws".
pub fn fastened(fastener: Fastener) -> &'static str {
    match fastener {
        Fastener::Glue => "glue",
        Fastener::Brads => "brads",
        Fastener::Screws => "screws",
    }
}

/// What a board's surface is called: a face, an edge, an end, or a mitered end.
pub fn name(surface: Surface) -> &'static str {
    match surface {
        Surface::Bottom => "bottom face",
        Surface::Top => "top face",
        Surface::Side(edge) if edge.out.l != 0 && edge.out.w != 0 => "mitered end",
        Surface::Side(edge) if edge.out.l != 0 => "end",
        Surface::Side(_) => "edge",
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
            bench.set_down(board.clone(), None);
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
            bench.status(None),
            "Its mitered end against the mitered end. Enter fastens it with glue. Every joint is \
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
        assert!(bench.status(None).ends_with("Every joint is tight."));
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
                        .status(None)
                        .ends_with("A mitered end is open 3/64\" where it meets a mitered end."),
                    "{}",
                    bench.status(None)
                );
            }
            assert!(bench.glue());
        }
        assert_eq!(bench.groups.len(), 1);
        assert_eq!(bench.groups[0].assembly.members().len(), 4);
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
        assert!(bench.status(None).contains("flush with one side"));

        bench.slide(0, true, false);
        assert_eq!(slide(&bench)[0], stops[0][1]);
        assert!(bench.status(None).contains("centered"));
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
    fn only_a_loose_part_comes_off_the_bench() {
        let piece = frame_piece();
        let mut bench = bench_with(&[piece.clone(), piece.clone(), plain()]);
        click(&mut bench, spot(1, 0, miter(&piece, false)));
        click(&mut bench, spot(0, 0, miter(&piece, true)));
        assert!(bench.glue());
        // The frame's corner stays.
        click(&mut bench, spot(0, 0, Surface::Top));
        assert_eq!(bench.take_picked(), None);
        // The loose board comes off.
        click(&mut bench, spot(0, 0, Surface::Top));
        assert_eq!(bench.picked, None);
        click(&mut bench, spot(1, 0, Surface::Top));
        assert_eq!(bench.take_picked(), Some(plain()));
        assert_eq!(bench.groups.len(), 1);
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
        assert!(bench.status(None).ends_with("Every joint is tight."));
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
                .status(None)
                .contains("F flips it over to make a corner.")
        );
        bench.turn_around_joint(2);
        let status = bench.status(None);
        assert!(!status.contains("F flips"), "{status}");
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
