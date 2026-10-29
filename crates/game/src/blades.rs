//! Saw blades running, and the jointer's cutterhead. Each saw's blade spins up when the saw's
//! switched on and coasts to a stop after (`Motor`), drawn with its teeth while they can be
//! followed and as a blur once they can't, and a cut throws sawdust up where the teeth come out
//! of the wood (`Sawing`).
//!
//! A blade turns about its own x, from up toward +z: the top of a table saw's blade runs
//! toward you and its front teeth down into the board, and a miter saw's front teeth come down
//! onto the board and carry it back against the fence. The teeth come up out of the wood behind
//! the cut and throw the dust up there.

use std::f32::consts::TAU;

use bevy::{light::NotShadowCaster, prelude::*};
use woodsim::{Blade, Length};

use crate::{
    meshes::{BladeShape, saw_blade, spinning_plate, spinning_teeth},
    textures,
};

/// A blade at full speed, 4,000 rpm, in radians a second.
const FULL_SPEED: f32 = 4000.0 * TAU / 60.0;
/// Faster than this, in radians a second, the teeth move further between frames than the eye can
/// follow, so the blade's drawn as a blur.
const BLUR_ABOVE: f32 = 6.0;
/// How fast the blur turns at most, in radians a second: fast enough to whir, slow enough that
/// its streaks can be followed around.
const BLUR_SPIN: f32 = 30.0;

/// Sawdust thrown up at a full feed, each second: flecks, and puffs of fine dust.
const FLECKS_PER_SECOND: f32 = 300.0;
const PUFFS_PER_SECOND: f32 = 18.0;
/// At most this many flecks fly at once.
const MOST_FLECKS: usize = 900;
const GRAVITY: f32 = 9.8;
/// How quickly the air slows a fleck, and a puff, per second.
const FLECK_DRAG: f32 = 3.0;
const PUFF_DRAG: f32 = 2.5;

pub struct BladesPlugin;

impl Plugin for BladesPlugin {
    fn build(&self, app: &mut App) {
        app.add_message::<Sawing>()
            .init_resource::<Sawdust>()
            .add_systems(
                Update,
                (spin_blades, (throw_sawdust, fly_sawdust).chain()).in_set(BladeSystems),
            );
    }
}

/// Where blades spin and sawdust is thrown: saws switch their motors on and say where they're
/// sawing before this.
#[derive(SystemSet, Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct BladeSystems;

/// What turns a saw's blade: how fast it's going, and how far it's turned. It's on while it's
/// asked to `run`, and a little after, as a switch left on between passes would be.
#[derive(Component, Clone, Debug, PartialEq)]
pub struct Motor {
    /// Seconds it takes to come up to full speed, and to coast to a stop.
    spin_up: f32,
    coast: f32,
    /// Seconds it keeps running after it was last asked to.
    run_on: f32,
    /// Seconds it has left to run.
    left: f32,
    /// How fast it's going, from 0 (stopped) to 1 (full speed).
    speed: f32,
    /// How far the blade, or its blur, has turned, in radians.
    angle: f32,
}

impl Motor {
    pub fn new(spin_up: f32, coast: f32, run_on: f32) -> Motor {
        Motor {
            spin_up,
            coast,
            run_on,
            left: 0.0,
            speed: 0.0,
            angle: 0.0,
        }
    }

    /// Switches it on, or keeps it on, until a while after this is last called.
    pub fn run(&mut self) {
        self.left = self.run_on.max(f32::MIN_POSITIVE);
    }

    /// Runs it for `seconds`: up toward full speed while it's on, and down toward a stop after.
    pub fn turn(&mut self, seconds: f32) {
        self.speed = if self.left > 0.0 {
            (self.speed + seconds / self.spin_up).min(1.0)
        } else {
            (self.speed - seconds / self.coast).max(0.0)
        };
        self.left -= seconds;
        self.angle = (self.angle + self.spin().min(BLUR_SPIN) * seconds).rem_euclid(TAU);
    }

    /// How fast the blade turns, in radians a second. It comes up from a stop slowly at first,
    /// and coasts to one slowly at the end, so the teeth can be seen getting going and slowing
    /// down.
    pub fn spin(&self) -> f32 {
        FULL_SPEED * self.speed.powi(3)
    }

    /// Whether it's going too fast for its teeth to be followed.
    pub fn blurred(&self) -> bool {
        self.spin() > BLUR_ABOVE
    }
}

/// A blade drawn sharp, with its teeth.
#[derive(Component)]
struct Sharp;

/// A spinning blade's blur: its plate, and the ring its teeth make.
#[derive(Component)]
struct Blurred;

/// Spawns a saw's blade, turning about its x, with `motor` turning it: an entity for the saw to
/// place, with the blade drawn sharp and as a blur in it.
pub fn spawn_blade(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    images: &mut Assets<Image>,
    blade: Blade,
    motor: Motor,
) -> Entity {
    let shape = BladeShape::new(blade);
    // Steel glows a little with its own colors, so a face turned from the sun stays bright.
    let steel = |texture: Handle<Image>, alpha_mode: AlphaMode| StandardMaterial {
        base_color_texture: Some(texture.clone()),
        emissive: LinearRgba::WHITE * 4.0,
        emissive_texture: Some(texture),
        perceptual_roughness: 0.35,
        alpha_mode,
        ..default()
    };
    let sharp = materials.add(steel(
        images.add(textures::saw_blade(&shape)),
        AlphaMode::Opaque,
    ));
    let spinning = images.add(textures::spinning_blade(&shape));
    let plate = materials.add(steel(spinning.clone(), AlphaMode::Opaque));
    let teeth = materials.add(steel(spinning, AlphaMode::Blend));
    commands
        .spawn((Transform::default(), Visibility::default(), motor))
        .with_children(|blade| {
            blade.spawn((
                Mesh3d(meshes.add(saw_blade(&shape))),
                MeshMaterial3d(sharp),
                Transform::default(),
                Pickable::IGNORE,
                Sharp,
            ));
            blade.spawn((
                Mesh3d(meshes.add(spinning_plate(&shape))),
                MeshMaterial3d(plate),
                Transform::default(),
                Visibility::Hidden,
                Pickable::IGNORE,
                Blurred,
            ));
            // The teeth are too thin to shadow anything, and a see-through ring's shadow
            // would be solid.
            blade.spawn((
                Mesh3d(meshes.add(spinning_teeth(&shape))),
                MeshMaterial3d(teeth),
                Transform::default(),
                Visibility::Hidden,
                Pickable::IGNORE,
                NotShadowCaster,
                Blurred,
            ));
        })
        .id()
}

/// Spawns a jointer's or a planer's cutterhead, `diameter` across its knives' circle and as
/// long as its `knives`, turning about its x with `motor` turning it, its middle at the entity's
/// origin: a drum with three knives set into it, drawn as a see-through blur once it's going too
/// fast to follow them.
pub fn spawn_cutterhead(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    (diameter, knives): (Length, Length),
    motor: Motor,
) -> Entity {
    let radius = diameter.as_meters_f32() / 2.0;
    let length = knives.as_meters_f32();
    // Steel glows a little with its own color, so it stays bright in the gap between the tables.
    let steel = |color: Color, alpha_mode: AlphaMode| StandardMaterial {
        base_color: color,
        emissive: LinearRgba::from(color) * 0.6,
        perceptual_roughness: 0.35,
        alpha_mode,
        ..default()
    };
    let drum = materials.add(steel(Color::srgb(0.62, 0.66, 0.72), AlphaMode::Opaque));
    let edge = materials.add(steel(Color::srgb(0.9, 0.92, 0.95), AlphaMode::Opaque));
    let blur = materials.add(steel(
        Color::srgba(0.85, 0.88, 0.93, 0.45),
        AlphaMode::Blend,
    ));
    let along_x = Quat::from_rotation_z(std::f32::consts::FRAC_PI_2);
    let cylinder = |radius: f32| Mesh::from(Cylinder::new(radius, length)).rotated_by(along_x);
    commands
        .spawn((Transform::default(), Visibility::default(), motor))
        .with_children(|head| {
            head.spawn((
                Mesh3d(meshes.add(cylinder(radius * 0.86))),
                MeshMaterial3d(drum),
                Transform::default(),
                Pickable::IGNORE,
            ));
            // Each knife stands out of a slot in the drum to the circle its edge cuts.
            let depth = radius * 0.3;
            for k in 0..3 {
                let around = Quat::from_rotation_x(TAU * k as f32 / 3.0);
                let knife = Mesh::from(Cuboid::new(length, depth, radius * 0.12))
                    .translated_by(Vec3::Y * (radius - depth / 2.0))
                    .rotated_by(around);
                head.spawn((
                    Mesh3d(meshes.add(knife)),
                    MeshMaterial3d(edge.clone()),
                    Transform::default(),
                    Pickable::IGNORE,
                    Sharp,
                ));
            }
            head.spawn((
                Mesh3d(meshes.add(cylinder(radius))),
                MeshMaterial3d(blur),
                Transform::default(),
                Visibility::Hidden,
                Pickable::IGNORE,
                NotShadowCaster,
                Blurred,
            ));
        })
        .id()
}

/// A drawing of a blade, which turns with it: whether it's the blur, and how it's placed and
/// shown.
type Drawing = (
    &'static mut Transform,
    &'static mut Visibility,
    Has<Blurred>,
);
type Drawings = Or<(With<Sharp>, With<Blurred>)>;

/// Turns each blade as its motor drives it, sharp or blurred.
fn spin_blades(
    time: Res<Time>,
    mut motors: Query<(&mut Motor, &Children)>,
    mut drawings: Query<Drawing, Drawings>,
) {
    for (mut motor, children) in &mut motors {
        motor.turn(time.delta_secs());
        let rotation = Quat::from_rotation_x(motor.angle);
        for &child in children {
            let Ok((mut transform, mut visibility, blur)) = drawings.get_mut(child) else {
                continue;
            };
            transform.rotation = rotation;
            visibility.set_if_neq(if blur == motor.blurred() {
                Visibility::Inherited
            } else {
                Visibility::Hidden
            });
        }
    }
}

/// A blade's rim, in some frame: its center, the directions up its plane and along it, and its
/// radius. It turns from `up` toward `along`.
pub struct Rim {
    pub center: Vec3,
    pub up: Vec3,
    pub along: Vec3,
    pub radius: f32,
}

impl Rim {
    /// The point `angle` around the rim from `up`, and the way the teeth there are going.
    fn at(&self, angle: f32) -> (Vec3, Vec3) {
        let (sin, cos) = angle.sin_cos();
        (
            self.center + (self.up * cos + self.along * sin) * self.radius,
            self.along * cos - self.up * sin,
        )
    }
}

/// Steps around a rim looking for where its teeth come out of the wood.
const RIM_STEPS: usize = 720;

/// Where a blade's teeth throw up dust from the wood that fills `wood` (boxes, as their least
/// and greatest corners), and which way: the highest place the rim crosses the surface of the
/// wood, going in or, better, coming out, just outside it. The dust flies the way the teeth
/// there go, but up, and out of the wood rather than into it. None if the rim's nowhere in the
/// wood.
pub fn chips(rim: &Rim, wood: &[(Vec3, Vec3)]) -> Option<(Vec3, Vec3)> {
    let in_wood = |point: Vec3| {
        wood.iter()
            .any(|&(low, high)| point.cmpge(low).all() && point.cmple(high).all())
    };
    // Teeth coming out lift the dust they carry: they win over teeth going in a little lower.
    const COMING_OUT: f32 = 0.002;
    let step = TAU / RIM_STEPS as f32;
    let (at, going, _) = (0..RIM_STEPS)
        .filter_map(|i| {
            let (from, going) = rim.at(i as f32 * step);
            let (to, _) = rim.at((i + 1) as f32 * step);
            match (in_wood(from), in_wood(to)) {
                (true, false) => Some((to, going, to.y + COMING_OUT)),
                (false, true) => Some((from, going, from.y)),
                _ => None,
            }
        })
        .max_by(|(_, _, a), (_, _, b)| a.total_cmp(b))?;
    let mut toward = going.with_y(going.y.abs());
    if in_wood(at + toward * 0.005) {
        toward = Vec3::new(-toward.x, toward.y, -toward.z);
    }
    Some((at, toward))
}

/// A saw cutting wood this frame: where its teeth come up out of the wood, and which way
/// they're going there, in the shop, and how hard it's going at it, 1 for a steady feed.
#[derive(Message, Clone, Debug)]
pub struct Sawing {
    pub at: Vec3,
    pub toward: Vec3,
    pub amount: f32,
}

/// What sawdust is drawn with, and the dice it's thrown with.
#[derive(Resource)]
struct Sawdust {
    fleck: Handle<Mesh>,
    puff: Handle<Mesh>,
    /// Pine dust, from light to dark.
    flecks: [Handle<StandardMaterial>; 3],
    puffs: Handle<StandardMaterial>,
    random: u32,
    /// Flecks and puffs owed from frames too short to throw a whole one.
    owed: (f32, f32),
}

impl FromWorld for Sawdust {
    fn from_world(world: &mut World) -> Sawdust {
        let mut meshes = world.resource_mut::<Assets<Mesh>>();
        let fleck = meshes.add(Cuboid::from_length(1.0));
        let puff = meshes.add(Sphere::new(1.0).mesh().ico(1).expect("a small icosphere"));
        let mut materials = world.resource_mut::<Assets<StandardMaterial>>();
        // Flecks glow a little too, so they don't go dark in the saw's shadow.
        let mut fleck_of = |color: Color| {
            materials.add(StandardMaterial {
                base_color: color,
                emissive: color.to_linear() * 8.0,
                perceptual_roughness: 0.9,
                ..default()
            })
        };
        let flecks = [
            fleck_of(Color::srgb(1.0, 0.95, 0.82)),
            fleck_of(Color::srgb(1.0, 0.89, 0.66)),
            fleck_of(Color::srgb(0.95, 0.78, 0.52)),
        ];
        // Puffs glow a little, so their shadowed side stays pale: a soft blob of dust.
        let puffs = materials.add(StandardMaterial {
            base_color: Color::srgba(1.0, 0.96, 0.88, 0.45),
            emissive: LinearRgba::rgb(1.0, 0.93, 0.8) * 15.0,
            perceptual_roughness: 1.0,
            reflectance: 0.0,
            alpha_mode: AlphaMode::Blend,
            ..default()
        });
        Sawdust {
            fleck,
            puff,
            flecks,
            puffs,
            random: 0x9e37_79b9,
            owed: (0.0, 0.0),
        }
    }
}

impl Sawdust {
    /// A pseudo-random number in [0, 1).
    fn random(&mut self) -> f32 {
        // Xorshift: plenty for where dust goes.
        let mut x = self.random;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.random = x;
        (x >> 8) as f32 / 16_777_216.0
    }

    /// A pseudo-random number between `low` and `high`.
    fn between(&mut self, low: f32, high: f32) -> f32 {
        low + (high - low) * self.random()
    }
}

/// A bit of sawdust in the air.
#[derive(Component)]
struct Fleck {
    velocity: Vec3,
    /// How it tumbles: about which axis, and how fast, in radians a second.
    tumble: Vec3,
    size: Vec3,
    age: f32,
    life: f32,
}

/// A puff of fine dust, swelling as it drifts and thins out.
#[derive(Component)]
struct Puff {
    velocity: Vec3,
    /// How big it starts and ends, in meters across.
    size: (f32, f32),
    age: f32,
    life: f32,
}

/// Throws sawdust from wherever a saw's cutting, more the harder it's going at it.
fn throw_sawdust(
    mut commands: Commands,
    mut sawing: MessageReader<Sawing>,
    mut dust: ResMut<Sawdust>,
    time: Res<Time>,
    flecks: Query<(), With<Fleck>>,
) {
    let seconds = time.delta_secs().min(0.05);
    let mut flying = flecks.iter().count();
    for cut in sawing.read() {
        let toward = cut.toward.normalize_or(Vec3::Y);
        // Across the way the teeth throw it: out of the blade's plane, and up it.
        let side = toward.cross(Vec3::Y).normalize_or(Vec3::X);
        let lift = side.cross(toward);
        dust.owed.0 += FLECKS_PER_SECOND * cut.amount * seconds;
        dust.owed.1 += PUFFS_PER_SECOND * cut.amount * seconds;
        while dust.owed.0 >= 1.0 {
            dust.owed.0 -= 1.0;
            if flying >= MOST_FLECKS {
                continue;
            }
            flying += 1;
            // Mostly the way the teeth go, but up, fanned out up the blade's plane more than
            // across it.
            let speed = dust.between(0.8, 2.4);
            let fan = lift * dust.between(-0.4, 0.8) + side * dust.between(-0.35, 0.35);
            let velocity = (toward + Vec3::Y * 0.9 + fan).normalize() * speed;
            let size = 0.008 * dust.between(0.45, 1.0);
            let fleck = Fleck {
                velocity,
                tumble: Vec3::new(dust.random() - 0.5, dust.random() - 0.5, dust.random())
                    .normalize()
                    * dust.between(6.0, 25.0),
                size: Vec3::new(size, size * dust.between(0.5, 0.9), size * 0.4),
                age: 0.0,
                life: dust.between(0.45, 1.1),
            };
            let shade = (dust.random() * 3.0) as usize;
            let rotation = Quat::from_euler(
                EulerRot::XYZ,
                dust.random() * TAU,
                dust.random() * TAU,
                dust.random() * TAU,
            );
            commands.spawn((
                Mesh3d(dust.fleck.clone()),
                MeshMaterial3d(dust.flecks[shade.min(2)].clone()),
                Transform::from_translation(cut.at + jitter(&mut dust))
                    .with_rotation(rotation)
                    .with_scale(fleck.size),
                fleck,
                NotShadowCaster,
                Pickable::IGNORE,
            ));
        }
        while dust.owed.1 >= 1.0 {
            dust.owed.1 -= 1.0;
            let velocity = toward * dust.between(0.1, 0.35)
                + Vec3::Y * dust.between(0.15, 0.4)
                + side * dust.between(-0.1, 0.1);
            let start = dust.between(0.012, 0.02);
            let puff = Puff {
                velocity,
                size: (start, start * dust.between(2.5, 4.0)),
                age: 0.0,
                life: dust.between(0.8, 1.4),
            };
            commands.spawn((
                Mesh3d(dust.puff.clone()),
                MeshMaterial3d(dust.puffs.clone()),
                Transform::from_translation(cut.at + jitter(&mut dust))
                    .with_scale(Vec3::splat(start / 2.0)),
                puff,
                NotShadowCaster,
                Pickable::IGNORE,
            ));
        }
    }
}

/// A few millimeters any way, so dust comes off the length of the cut rather than a point.
fn jitter(dust: &mut Sawdust) -> Vec3 {
    Vec3::new(dust.random(), dust.random(), dust.random()) * 0.006 - 0.003
}

/// Flies the sawdust: flecks arc up, slow in the air, tumble, and fall, shrinking away at the
/// end; puffs drift, swell, and thin out.
fn fly_sawdust(
    mut commands: Commands,
    time: Res<Time>,
    mut flecks: Query<(Entity, &mut Fleck, &mut Transform), Without<Puff>>,
    mut puffs: Query<(Entity, &mut Puff, &mut Transform), Without<Fleck>>,
) {
    let seconds = time.delta_secs().min(0.05);
    for (entity, mut fleck, mut transform) in &mut flecks {
        fleck.age += seconds;
        if fleck.age >= fleck.life {
            commands.entity(entity).despawn();
            continue;
        }
        fleck.velocity *= (-FLECK_DRAG * seconds).exp();
        fleck.velocity.y -= GRAVITY * seconds;
        transform.translation += fleck.velocity * seconds;
        let tumble = fleck.tumble;
        transform.rotate(Quat::from_scaled_axis(tumble * seconds));
        transform.scale = fleck.size * fading(fleck.age / fleck.life, 0.7);
    }
    for (entity, mut puff, mut transform) in &mut puffs {
        puff.age += seconds;
        if puff.age >= puff.life {
            commands.entity(entity).despawn();
            continue;
        }
        puff.velocity *= (-PUFF_DRAG * seconds).exp();
        transform.translation += puff.velocity * seconds;
        let grown = puff.age / puff.life;
        // Swelling quickly at first, then more slowly.
        let across = puff.size.0 + (puff.size.1 - puff.size.0) * (1.0 - (1.0 - grown).powi(2));
        transform.scale = Vec3::splat(across / 2.0 * fading(grown, 0.6));
    }
}

/// How big something is, as a fraction of its size, `done` of the way through its life: full
/// size until `from`, then shrinking smoothly away.
fn fading(done: f32, from: f32) -> f32 {
    let t = ((done - from) / (1.0 - from)).clamp(0.0, 1.0);
    1.0 - t * t * (3.0 - 2.0 * t)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_motor_spins_up_runs_on_and_coasts_to_a_stop() {
        let mut motor = Motor::new(0.5, 2.0, 1.0);
        motor.run();
        for _ in 0..40 {
            motor.turn(1.0 / 60.0);
        }
        assert_eq!(motor.spin(), FULL_SPEED);
        assert!(motor.blurred());
        // Most of a second after it was last asked to run, it's still going.
        for _ in 0..15 {
            motor.turn(1.0 / 60.0);
        }
        assert_eq!(motor.spin(), FULL_SPEED);
        // Then it coasts, and its teeth show again before it stops.
        let mut seen = false;
        for _ in 0..200 {
            motor.turn(1.0 / 60.0);
            seen |= motor.spin() > 0.0 && !motor.blurred();
        }
        assert!(seen, "the teeth show as it slows");
        assert_eq!(motor.spin(), 0.0);
    }

    #[test]
    fn a_motor_without_run_on_stops_when_let_go() {
        let mut motor = Motor::new(0.5, 1.0, 0.0);
        motor.run();
        motor.turn(0.1);
        let going = motor.spin();
        assert!(going > 0.0);
        motor.turn(0.1);
        assert!(
            motor.spin() < going,
            "it slows as soon as it's not asked to run"
        );
    }

    /// A table saw's 10" blade with its top 1" above the table, as `table_saw::draw_blade`
    /// places it, and a 3/4" board 24" long over it, its leading end `lead` meters along.
    fn table_saw_rim() -> Rim {
        let inch = 0.0254;
        Rim {
            center: Vec3::new(-inch / 16.0, -4.0 * inch, 0.0),
            up: Vec3::Y,
            along: Vec3::Z,
            radius: 5.0 * inch,
        }
    }

    fn board(lead: f32) -> (Vec3, Vec3) {
        let inch = 0.0254;
        (
            Vec3::new(-6.0 * inch, 0.0, lead),
            Vec3::new(1.25 * inch, 0.75 * inch, lead + 24.0 * inch),
        )
    }

    #[test]
    fn chips_fly_from_the_top_of_the_board_behind_the_blade() {
        let inch = 0.0254;
        let (at, toward) = chips(&table_saw_rim(), &[board(-6.0 * inch)]).expect("cutting");
        assert!((at.y - 0.75 * inch).abs() < 0.05 * inch, "{at}");
        // Where a 10" blade 1" up crosses 3/4" up, behind its center.
        let crossing = (25.0f32 - 4.75 * 4.75).sqrt() * inch;
        assert!((at.z + crossing).abs() < 0.1 * inch, "{at}");
        // The teeth there come up and toward you.
        assert!(toward.y > 0.0 && toward.z > 0.0, "{toward}");
    }

    #[test]
    fn chips_fly_from_where_the_teeth_go_in_until_the_board_reaches_the_back_of_the_blade() {
        let inch = 0.0254;
        // The leading end is past the blade's center, but not where its back comes up through
        // the top of the board: the teeth go in at the front and out the bottom.
        let (at, toward) = chips(&table_saw_rim(), &[board(-inch)]).expect("cutting");
        let crossing = (25.0f32 - 4.75 * 4.75).sqrt() * inch;
        assert!((at.z - crossing).abs() < 0.1 * inch, "{at}");
        assert!((at.y - 0.75 * inch).abs() < 0.05 * inch, "{at}");
        assert!(toward.y > 0.0 && toward.z > 0.0, "{toward}");
    }

    #[test]
    fn chips_fly_out_of_the_wood() {
        let inch = 0.0254;
        // The blade's below the top of a 2" block whose leading end is over it: the teeth come
        // up into the end, and the dust blows back out of it.
        let block = (
            Vec3::new(-6.0 * inch, 0.0, -inch),
            Vec3::new(1.25 * inch, 2.0 * inch, 23.0 * inch),
        );
        let (at, toward) = chips(&table_saw_rim(), &[block]).expect("cutting");
        assert!((at.z + inch).abs() < 0.1 * inch, "{at}");
        assert!(toward.y > 0.0 && toward.z < 0.0, "{toward}");
    }

    #[test]
    fn no_chips_fly_from_wood_clear_of_the_blade() {
        let inch = 0.0254;
        assert_eq!(chips(&table_saw_rim(), &[board(4.0 * inch)]), None);
        let beside = (
            Vec3::new(inch, 0.0, -inch),
            Vec3::new(3.0 * inch, inch, inch),
        );
        assert_eq!(chips(&table_saw_rim(), &[beside]), None);
    }

    #[test]
    fn dust_shrinks_away_at_the_end() {
        assert_eq!(fading(0.2, 0.7), 1.0);
        assert!(fading(0.85, 0.7) < 1.0 && fading(0.85, 0.7) > 0.0);
        assert_eq!(fading(1.0, 0.7), 0.0);
    }
}
