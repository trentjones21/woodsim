//! Choosing what wood on a saw rests on. With the saw's `Rest` action on, the surface under the
//! pointer lights up yellow; a click rests the wood on it, flat on the table, and it lights up
//! blue; and a second click picks the surface to go against the fence. Each station keeps what's
//! being chosen (`work::Rest`); this shows it, and turns clicks into picks.

use bevy::{light::NotShadowCaster, picking::backend::HitData, prelude::*};
use woodsim::{Board, Surface, exposed};

use crate::{
    bench::sheet,
    chop::MiterSawStation,
    dress::JointerStation,
    feed::TableSawStation,
    join::surface_at,
    jointer::AtJointer,
    miter_saw::AtMiterSaw,
    shop::{Part, Shape, ShopView, Slot},
    table_saw::AtTableSaw,
};

pub struct RestPlugin;

impl Plugin for RestPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<SawPointing>()
            .add_systems(Startup, spawn_marks)
            .add_systems(Update, mark_surfaces);
    }
}

/// The board on a saw under the pointer, if any, and the surface of it, until it moves.
#[derive(Resource, Default)]
pub struct SawPointing {
    part: Option<Entity>,
    surface: Option<Surface>,
}

/// A sheet that lights up a surface while choosing: the one under the pointer, or the one the
/// wood rests on.
#[derive(Component, Clone, Copy, PartialEq, Eq)]
enum Mark {
    Pointed,
    Down,
}

/// The board and surface a mark's sheet was last built for.
#[derive(Component, Default)]
struct MarkShape(Option<(Board, Surface)>);

/// The surface of the board `shape` was drawn from where `hit` landed on it, the board drawn
/// with `transform`.
pub fn surface_hit(shape: &Shape, transform: &GlobalTransform, hit: &HitData) -> Option<Surface> {
    let board = shape.board()?;
    let (point, normal) = (hit.position?, hit.normal?);
    let inverse = transform.affine().inverse();
    surface_at(
        board,
        inverse.transform_point3(point),
        inverse.transform_vector3(normal),
    )
}

/// Notes the board on a saw under the pointer and the surface of it, as the pointer moves over
/// it.
pub fn point_at(
    moved: On<PointerMove>,
    parts: Query<(&Shape, &GlobalTransform)>,
    mut pointing: ResMut<SawPointing>,
) {
    let entity = moved.entity;
    let Ok((shape, transform)) = parts.get(entity) else {
        return;
    };
    let surface = surface_hit(shape, transform, &moved.hit);
    if pointing.part != Some(entity) || pointing.surface != surface {
        *pointing = SawPointing {
            part: Some(entity),
            surface,
        };
    }
}

pub fn point_away(out: On<PointerOut>, mut pointing: ResMut<SawPointing>) {
    if pointing.part == Some(out.entity) {
        *pointing = SawPointing::default();
    }
}

/// The two sheets: pale yellow under the pointer, blue where the wood rests.
fn spawn_marks(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    for (mark, color) in [
        (Mark::Pointed, Color::srgb(1.0, 0.9, 0.35)),
        (Mark::Down, Color::srgb(0.15, 0.5, 1.0)),
    ] {
        // A stand-in until there's a surface to lay it over; it's hidden until then.
        commands.spawn((
            Mesh3d(meshes.add(Rectangle::new(0.01, 0.01))),
            MeshMaterial3d(materials.add(StandardMaterial {
                base_color: color,
                unlit: true,
                ..default()
            })),
            Transform::default(),
            Visibility::Hidden,
            mark,
            MarkShape::default(),
            NotShadowCaster,
            Pickable::IGNORE,
        ));
    }
}

/// Lays the sheets over the surface under the pointer and the one the wood rests on, at the
/// saw you're at, while choosing what it rests on.
#[allow(clippy::type_complexity)]
fn mark_surfaces(
    view: Res<ShopView>,
    (table_saw, miter_saw, jointer): (
        Res<TableSawStation>,
        Res<MiterSawStation>,
        Res<JointerStation>,
    ),
    pointing: Res<SawPointing>,
    parts: Query<(
        Entity,
        &Part,
        &Slot,
        &Shape,
        &GlobalTransform,
        (Has<AtTableSaw>, Has<AtMiterSaw>, Has<AtJointer>),
    )>,
    mut marks: Query<(
        &Mark,
        &Mesh3d,
        &mut Transform,
        &mut Visibility,
        &mut MarkShape,
    )>,
    mut meshes: ResMut<Assets<Mesh>>,
) {
    let (choosing, down) = match *view {
        ShopView::TableSaw => (table_saw.choosing_rest(), table_saw.resting_on()),
        ShopView::MiterSaw => (miter_saw.choosing_rest(), miter_saw.resting_on()),
        ShopView::Jointer => (jointer.choosing_rest(), jointer.resting_on()),
        _ => (false, None),
    };
    // The whole board or glue-up on the saw you're at, a board of it by its slot.
    let here = |(at_table, at_miter, at_jointer): (bool, bool, bool)| match *view {
        ShopView::TableSaw => at_table,
        ShopView::MiterSaw => at_miter,
        ShopView::Jointer => at_jointer,
        _ => false,
    };
    let pointed = pointing
        .part
        .zip(pointing.surface)
        .and_then(|(entity, surface)| {
            let (_, part, slot, ..) = parts.get(entity).ok()?;
            (*part == Part::Whole).then_some((slot.0, surface))
        })
        .filter(|&spot| Some(spot) != down);
    for (mark, mesh, mut transform, mut visibility, mut shape) in &mut marks {
        let spot = match mark {
            Mark::Pointed => pointed,
            Mark::Down => down,
        }
        .filter(|_| choosing);
        let found = spot.and_then(|(member, surface)| {
            parts
                .iter()
                .find(|(_, part, slot, _, _, at)| {
                    **part == Part::Whole && slot.0 == member && here(*at)
                })
                .and_then(|(_, _, _, board, placed, ..)| {
                    Some((surface, board.board()?.clone(), placed))
                })
        });
        let Some((surface, board, placed)) = found else {
            if *visibility != Visibility::Hidden {
                *visibility = Visibility::Hidden;
            }
            continue;
        };
        let at = Transform::from_matrix(placed.to_matrix());
        if *transform != at {
            *transform = at;
        }
        let built = shape
            .0
            .as_ref()
            .is_some_and(|(built, on)| *built == board && *on == surface);
        if !built && let Some(mut sheet_mesh) = meshes.get_mut(&mesh.0) {
            *sheet_mesh = sheet(&exposed(&board, surface));
            shape.0 = Some((board, surface));
        }
        if *visibility != Visibility::Inherited {
            *visibility = Visibility::Inherited;
        }
    }
}
