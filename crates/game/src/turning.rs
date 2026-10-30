//! Wood shown turning, so the eye can follow which side goes where. When a station lays the wood
//! on it another way (turned, flipped, stood up, or rested on a surface picked), it doesn't jump
//! there: it lifts clear of the table, turns about its middle, and settles where it now lies, the
//! way you'd pick a board up and turn it over in your hands.

use std::f32::consts::PI;

use bevy::prelude::*;

use crate::shop::inches;

pub struct TurningPlugin;

impl Plugin for TurningPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, turn_wood);
    }
}

/// How long a turn takes, in seconds, and as long again for each half turn it makes.
const SECONDS: f32 = 0.25;
/// How high the wood's lifted halfway through, above what it takes to clear the table.
const LIFT: f32 = inches(0.5);
/// How many times along the way a turn is checked for clearing the table.
const STEPS: usize = 32;

/// A board on a station, turning from where it was to where it now lies, if it is.
#[derive(Component, Default)]
pub struct Turning(Option<Turned>);

struct Turned {
    turn: Turn,
    from: Transform,
    to: Transform,
}

impl Turning {
    /// Where the board's turning to, while it is.
    pub fn toward(&self) -> Option<Transform> {
        self.0.as_ref().map(|turned| turned.to)
    }

    /// Starts the board turning, as all the wood it's part of does, from `from` to `to`.
    pub fn start(&mut self, turn: Turn, from: Transform, to: Transform) {
        self.0 = Some(Turned { turn, from, to });
    }

    pub fn stop(&mut self) {
        self.0 = None;
    }

    /// Takes the board `seconds` further through its turn, giving where it's drawn now, if it's
    /// turning. At the end, it's where it goes, and done.
    fn step(&mut self, seconds: f32) -> Option<Transform> {
        let turned = self.0.as_mut()?;
        turned.turn.elapsed += seconds;
        if turned.turn.elapsed < turned.turn.seconds {
            return Some(turned.turn.pose(&turned.from, turned.turn.progress()));
        }
        let to = turned.to;
        self.stop();
        Some(to)
    }
}

/// One turn wood on a station makes, the same for every board of it, so a glue-up turns as one.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Turn {
    /// The turn it makes, about its middle.
    turn: Quat,
    /// Where its middle is, as it lay and as it now lies.
    middle: (Vec3, Vec3),
    /// How high it's lifted halfway through.
    lift: f32,
    /// How long it takes, and how long it's been going, in seconds.
    seconds: f32,
    elapsed: f32,
}

impl Turn {
    /// The turn that takes wood from where it was to where it now lies: for each of its boards,
    /// where it's drawn from, where it goes, and its corners in its own coordinates. `None` if
    /// it's where it was already, or its boards don't all move together.
    pub fn between(boards: &[(Transform, Transform, Vec<Vec3>)]) -> Option<Turn> {
        let (from, to, _) = boards.first()?;
        let turn = to.rotation * from.rotation.inverse();
        let middle = |pose: fn(&(Transform, Transform, Vec<Vec3>)) -> Transform| {
            let corners: Vec<Vec3> = boards
                .iter()
                .flat_map(|board| board.2.iter().map(|&at| pose(board).transform_point(at)))
                .collect();
            corners.iter().sum::<Vec3>() / corners.len().max(1) as f32
        };
        let middle = (middle(|board| board.0), middle(|board| board.1));
        let turned = angle(turn);
        if turned < 1e-3 && middle.0.distance(middle.1) < 1e-4 {
            return None;
        }
        let mut turn = Turn {
            turn,
            middle,
            lift: 0.0,
            seconds: SECONDS * (1.0 + turned / PI),
            elapsed: 0.0,
        };
        let together = boards.iter().all(|(from, to, _)| {
            let moved = turn.pose(from, 1.0);
            moved.translation.distance(to.translation) < 1e-4
                && angle(moved.rotation * to.rotation.inverse()) < 1e-3
        });
        if !together {
            return None;
        }
        // Lifted on a curve just high enough that no corner goes lower than it lay, or lies.
        let lowest = |u: f32| {
            boards
                .iter()
                .flat_map(|(from, _, corners)| {
                    let pose = turn.pose(from, u);
                    corners.iter().map(move |&at| pose.transform_point(at).y)
                })
                .fold(f32::MAX, f32::min)
        };
        let floor = lowest(0.0).min(lowest(1.0));
        turn.lift = LIFT
            + (1..STEPS)
                .map(|k| k as f32 / STEPS as f32)
                .map(|u| (floor - lowest(u)).max(0.0) / (PI * u).sin())
                .fold(0.0, f32::max);
        Some(turn)
    }

    /// Where a board drawn from `from` is `u` of the way through the turn, from 0 to 1.
    fn pose(&self, from: &Transform, u: f32) -> Transform {
        let turn = Quat::IDENTITY.slerp(self.turn, u);
        let middle = self.middle.0.lerp(self.middle.1, u) + Vec3::Y * self.lift * (PI * u).sin();
        Transform::from_translation(middle + turn * (from.translation - self.middle.0))
            .with_rotation(turn * from.rotation)
    }

    /// How far through it is, easing in and out, from 0 to 1.
    fn progress(&self) -> f32 {
        let t = (self.elapsed / self.seconds).clamp(0.0, 1.0);
        t * t * (3.0 - 2.0 * t)
    }
}

/// How far `turn` turns, in radians, from 0 to π: exactly, even for the least turn, as
/// `Quat::angle_between`'s quick arc cosine isn't.
fn angle(turn: Quat) -> f32 {
    2.0 * turn.xyz().length().atan2(turn.w.abs())
}

/// Moves each board that's turning along its turn, and sets it down where it goes at the end.
pub fn turn_wood(time: Res<Time>, mut wood: Query<(&mut Transform, &mut Turning)>) {
    for (mut transform, mut turning) in &mut wood {
        if turning.0.is_some()
            && let Some(pose) = turning.step(time.delta_secs())
        {
            *transform = pose;
        }
    }
}

#[cfg(test)]
mod tests {
    use std::f32::consts::FRAC_PI_2;

    use super::*;

    /// A 1x6, 24" long, as `lumber_mesh` draws it: `w`, `t`, `l` along x, y, z.
    fn one_by_six() -> Vec<Vec3> {
        let size = Vec3::new(inches(5.5), inches(0.75), inches(24.0));
        (0..8)
            .map(|k| Vec3::new((k & 1) as f32, ((k >> 1) & 1) as f32, (k >> 2) as f32) * size)
            .collect()
    }

    /// Where the board's lowest corner is, drawn from `pose`.
    fn lowest(pose: &Transform, corners: &[Vec3]) -> f32 {
        corners
            .iter()
            .map(|&at| pose.transform_point(at).y)
            .fold(f32::MAX, f32::min)
    }

    fn assert_near(a: &Transform, b: &Transform) {
        assert!(
            a.translation.distance(b.translation) < 1e-5
                && angle(a.rotation * b.rotation.inverse()) < 1e-4,
            "{a:?} is not {b:?}"
        );
    }

    /// Runs `turn` for a board drawn from `from`, a frame at a time, and gives every pose it's
    /// drawn in on the way.
    fn run(turn: Turn, from: Transform, to: Transform) -> Vec<Transform> {
        let mut turning = Turning::default();
        turning.start(turn, from, to);
        std::iter::once(from)
            .chain(std::iter::from_fn(|| turning.step(1.0 / 60.0)))
            .collect()
    }

    #[test]
    fn flipped_stood_up_or_turned_a_board_lifts_clear_of_the_table_and_lands_where_it_lies() {
        let corners = one_by_six();
        let from = Transform::IDENTITY;
        for (to, seconds) in [
            // Flipped over about its length, in the same place.
            (
                Transform::from_translation(Vec3::new(inches(5.5), inches(0.75), 0.0))
                    .with_rotation(Quat::from_rotation_z(PI)),
                0.5,
            ),
            // On its edge.
            (
                Transform::from_translation(Vec3::new(inches(0.75), 0.0, 0.0))
                    .with_rotation(Quat::from_rotation_z(FRAC_PI_2)),
                0.375,
            ),
            // A quarter turn left, across the table.
            (
                Transform::from_translation(Vec3::new(0.0, 0.0, inches(5.5)))
                    .with_rotation(Quat::from_rotation_y(FRAC_PI_2)),
                0.375,
            ),
        ] {
            let turn = Turn::between(&[(from, to, corners.clone())]).unwrap();
            let poses = run(turn, from, to);
            assert_eq!(*poses.last().unwrap(), to);
            // A frame each sixtieth of a second: a half turn takes longer than a quarter.
            let took = (poses.len() - 1) as f32 / 60.0;
            assert!((took - seconds).abs() < 0.02, "{to:?} took {took} s");
            for pose in &poses[1..poses.len() - 1] {
                assert!(lowest(pose, &corners) > 0.0, "{pose:?} drags on the table");
            }
        }
    }

    #[test]
    fn a_glue_up_turns_as_one() {
        let corners = one_by_six();
        // A second 1x6 glued on top of the first, and the two flipped over together.
        let flip = |pose: Transform| {
            Transform::from_translation(Vec3::new(inches(5.5), inches(1.5), 0.0))
                * Transform::from_rotation(Quat::from_rotation_z(PI))
                * pose
        };
        let boards = [
            Transform::IDENTITY,
            Transform::from_translation(Vec3::Y * inches(0.75)),
        ];
        let turn = Turn::between(&boards.map(|from| (from, flip(from), corners.clone()))).unwrap();
        let [bottom, top] = boards.map(|from| run(turn, from, flip(from)));
        assert_eq!(bottom.len(), top.len());
        for (bottom, top) in bottom.iter().zip(&top) {
            // Wherever they are along the way, the top board is just where it's glued on.
            assert_near(&(*bottom * boards[1]), top);
        }

        // Boards that don't move together, as if one came unglued, just go where they go.
        let apart = [
            (boards[0], flip(boards[0]), corners.clone()),
            (boards[1], boards[1], corners.clone()),
        ];
        assert_eq!(Turn::between(&apart), None);
    }

    #[test]
    fn wood_left_where_it_was_doesnt_turn() {
        let pose = Transform::from_translation(Vec3::X);
        assert_eq!(Turn::between(&[(pose, pose, one_by_six())]), None);
        assert_eq!(Turn::between(&[]), None);
    }
}
