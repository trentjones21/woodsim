//! The table saw station: the saw, its fence, the board, and the controls.

use std::f32::consts::FRAC_PI_2;

use bevy::{picking::pointer::PointerButton, prelude::*};
use woodsim::Length;

use crate::{
    chop::MiterSawStation,
    feed::TableSawStation,
    meshes::{Grain, board_mesh, rounded_box},
    shop::{
        Block, Part, ShopAssets, ShopView, inches, place_parts, pointer_on_plane, rounded_block,
        set_material_on, snap, snap_to_sixteenth, station_part,
    },
};

/// Where the station's coordinates start in the shop: the blade's fence-side face, at the
/// table top.
pub const ORIGIN: Vec3 = Vec3::new(0.0, inches(34.0), 0.0);

/// The space the saw and its outfeed table take up, for outlining it in the overview.
pub const BOUNDS: Block = Block {
    min: Vec3::new(TABLE_LEFT, -ORIGIN.y, OUTFEED_END),
    max: Vec3::new(TABLE_RIGHT, FENCE_HEIGHT, TABLE_FRONT + inches(2.0)),
};

pub const HELP: &str = "Left/Right: move the fence 1/16\"   Shift+Left/Right: 1\"   \
                        Or drag the fence\n\
                        Up/Down: raise or lower the blade 1/16\"   Shift+Up/Down: 1/4\"\n\
                        Drag the board, or hold Space, to push it   Drag it back to back it out\n\
                        Q/E: turn the board   F: flip it   B: bring it back around   \
                        R: new board\n\
                        Right-click a board or press C: carry it to the miter saw\n\
                        Tab: go to the miter saw   Esc: back to the shop";

/// The cast-iron top, in table space.
const TABLE_LEFT: f32 = inches(-14.0);
const TABLE_RIGHT: f32 = inches(28.0);
const TABLE_FRONT: f32 = inches(13.5);
const TABLE_BACK: f32 = inches(-13.5);
/// The far end of the outfeed table behind the saw.
const OUTFEED_END: f32 = inches(-32.0);

const FENCE_WIDTH: f32 = inches(2.0);
const FENCE_HEIGHT: f32 = inches(2.5);
/// The fence runs the depth of the table and over the front rail.
const FENCE_LENGTH: f32 = TABLE_FRONT + inches(2.0) - TABLE_BACK;

const FENCE_STEP: Length = Length::from_inch_fraction(1, 16);
const FENCE_BIG_STEP: Length = Length::from_inches(1);
const HEIGHT_STEP: Length = Length::from_inch_fraction(1, 16);
const HEIGHT_BIG_STEP: Length = Length::from_inch_fraction(1, 4);
/// The riving knife's top sits this far below the blade's, so it can follow a cut that doesn't
/// go through.
const KNIFE_BELOW_BLADE: f32 = inches(1.0 / 8.0);
const KNIFE_HEIGHT: f32 = inches(4.0);
/// How fast the board moves while Space is held: 10" a second.
const FEED_SPEED: f32 = inches(10.0);

pub struct TableSawPlugin;

impl Plugin for TableSawPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<TableSawStation>()
            .init_resource::<BoardGrab>()
            .add_systems(Startup, spawn_saw)
            .add_systems(
                Update,
                (
                    (
                        move_fence_with_keys,
                        raise_blade_with_keys,
                        turn_with_keys,
                        push_board,
                        bring_back_with_key,
                        new_board,
                        carry_with_key,
                    )
                        .run_if(resource_equals(ShopView::TableSaw)),
                    set_loose_aside,
                    (draw_fence, draw_blade, draw_board),
                )
                    .chain(),
            );
    }
}

#[derive(Component)]
struct Fence {
    /// How far right of the fence face the pointer grabbed it, in meters.
    grab_offset: f32,
}

#[derive(Component)]
struct AtTableSaw;

/// The blade, which rises and falls with its height setting.
#[derive(Component)]
struct BladeDisc;

/// The riving knife, which rises and falls with the blade.
#[derive(Component)]
struct RivingKnife;

/// Where the pointer grabbed the board to drag it, along the table, and how far along the blade
/// the board was then. The board follows how far the pointer moves.
#[derive(Resource, Default)]
struct BoardGrab {
    pointer: f32,
    advance: Length,
}

fn spawn_saw(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    shop: Res<ShopAssets>,
    station: Res<TableSawStation>,
) {
    let mut paint = |color: Color| {
        materials.add(StandardMaterial {
            base_color: color,
            perceptual_roughness: 0.55,
            ..default()
        })
    };
    let table_top = paint(Color::srgb(0.76, 0.79, 0.83));
    let cabinet = paint(Color::srgb(0.98, 0.72, 0.2));
    let dark_metal = paint(Color::srgb(0.22, 0.24, 0.3));
    let safety_red = paint(Color::srgb(0.92, 0.25, 0.22));
    let tape_yellow = paint(Color::srgb(1.0, 0.88, 0.35));
    let wood = shop.wood.clone();

    let spawn_box = |commands: &mut Commands,
                     meshes: &mut Assets<Mesh>,
                     material: &Handle<StandardMaterial>,
                     min: Vec3,
                     max: Vec3| {
        let entity = commands.spawn((
            rounded_block(meshes, Block { min, max }, ORIGIN),
            MeshMaterial3d(material.clone()),
        ));
        station_part(entity, ShopView::TableSaw);
    };
    spawn_box(
        &mut commands,
        &mut meshes,
        &table_top,
        Vec3::new(TABLE_LEFT, inches(-1.5), TABLE_BACK),
        Vec3::new(TABLE_RIGHT, 0.0, TABLE_FRONT),
    );
    // Miter slots: shallow channels either side of the blade, drawn as dark strips.
    for x in [-6.5, 5.5] {
        spawn_box(
            &mut commands,
            &mut meshes,
            &dark_metal,
            Vec3::new(inches(x), inches(-0.2), TABLE_BACK),
            Vec3::new(inches(x + 0.75), inches(0.02), TABLE_FRONT),
        );
    }
    // The throat plate around the blade.
    spawn_box(
        &mut commands,
        &mut meshes,
        &safety_red,
        Vec3::new(inches(-1.75), inches(-0.2), inches(-7.0)),
        Vec3::new(inches(1.75), inches(0.03), inches(7.0)),
    );
    // Front rail, which the fence rides on, with a measuring tape along its top.
    spawn_box(
        &mut commands,
        &mut meshes,
        &dark_metal,
        Vec3::new(TABLE_LEFT, inches(-2.5), TABLE_FRONT),
        Vec3::new(TABLE_RIGHT, 0.0, TABLE_FRONT + inches(2.0)),
    );
    spawn_box(
        &mut commands,
        &mut meshes,
        &tape_yellow,
        Vec3::new(TABLE_LEFT + inches(1.0), 0.0, TABLE_FRONT + inches(0.4)),
        Vec3::new(
            TABLE_RIGHT - inches(1.0),
            inches(0.03),
            TABLE_FRONT + inches(1.4),
        ),
    );
    // The power paddle hangs under the rail, left of the blade, where a hip can bump it off.
    spawn_box(
        &mut commands,
        &mut meshes,
        &safety_red,
        Vec3::new(inches(-12.0), inches(-5.0), TABLE_FRONT + inches(1.5)),
        Vec3::new(inches(-8.5), inches(-2.5), TABLE_FRONT + inches(3.0)),
    );
    // The cabinet on a dark plinth, with a dust port out the back.
    spawn_box(
        &mut commands,
        &mut meshes,
        &cabinet,
        Vec3::new(inches(-10.0), inches(3.0) - ORIGIN.y, inches(-10.0)),
        Vec3::new(inches(10.0), inches(-1.5), inches(10.0)),
    );
    spawn_box(
        &mut commands,
        &mut meshes,
        &dark_metal,
        Vec3::new(inches(-11.0), -ORIGIN.y, inches(-11.0)),
        Vec3::new(inches(11.0), inches(3.0) - ORIGIN.y, inches(11.0)),
    );
    commands.spawn((
        Mesh3d(meshes.add(Cylinder::new(inches(2.0), inches(3.0)))),
        MeshMaterial3d(dark_metal.clone()),
        Transform::from_translation(ORIGIN + Vec3::new(0.0, inches(-24.0), inches(-11.0)))
            .with_rotation(Quat::from_rotation_x(FRAC_PI_2)),
        Pickable::IGNORE,
    ));
    // Outfeed table: a maple top on a painted support panel.
    let outfeed_top = Block {
        min: Vec3::new(inches(-10.0), inches(-0.75), OUTFEED_END),
        max: Vec3::new(inches(20.0), 0.0, TABLE_BACK),
    };
    let entity = commands.spawn((
        Mesh3d(meshes.add(board_mesh(
            outfeed_top.max - outfeed_top.min,
            Vec3::new(0.3, 0.0, 0.1),
            Grain::X,
        ))),
        MeshMaterial3d(shop.maple.clone()),
        Transform::from_translation(ORIGIN + outfeed_top.min),
    ));
    station_part(entity, ShopView::TableSaw);
    spawn_box(
        &mut commands,
        &mut meshes,
        &cabinet,
        Vec3::new(inches(-9.0), -ORIGIN.y, OUTFEED_END),
        Vec3::new(inches(19.0), inches(-0.75), OUTFEED_END + inches(0.75)),
    );

    // The blade and riving knife are placed at the blade's height by `draw_blade`.
    let kerf = station.saw.blade.kerf.as_meters_f32();
    let radius = station.saw.blade.radius().as_meters_f32();
    commands.spawn((
        Mesh3d(meshes.add(Cylinder::new(radius, kerf))),
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color: Color::srgb(0.88, 0.89, 0.92),
            perceptual_roughness: 0.3,
            ..default()
        })),
        // A cylinder's axis starts along Y; turn it to X so the blade spins toward the operator.
        Transform::from_rotation(Quat::from_rotation_z(FRAC_PI_2)),
        Pickable::IGNORE,
        BladeDisc,
    ));
    // The riving knife stands in the kerf just behind the blade, so pieces pass either side.
    let knife = Block {
        min: Vec3::new(-kerf * 0.8, -KNIFE_HEIGHT, inches(-6.5)),
        max: Vec3::new(-kerf * 0.2, 0.0, inches(-3.0)),
    };
    commands.spawn((
        Mesh3d(meshes.add(rounded_box(knife.max - knife.min))),
        MeshMaterial3d(dark_metal.clone()),
        Transform::default(),
        Pickable::IGNORE,
        RivingKnife,
    ));

    let fence_color = materials.add(StandardMaterial {
        base_color: Color::srgb(0.9, 0.2, 0.22),
        perceptual_roughness: 0.4,
        ..default()
    });
    let fence_hover = materials.add(StandardMaterial {
        base_color: Color::srgb(1.0, 0.45, 0.4),
        emissive: LinearRgba::new(1.0, 0.35, 0.2, 1.0) * 40.0,
        perceptual_roughness: 0.4,
        ..default()
    });
    commands
        .spawn((
            Mesh3d(meshes.add(rounded_box(Vec3::new(
                FENCE_WIDTH,
                FENCE_HEIGHT,
                FENCE_LENGTH,
            )))),
            MeshMaterial3d(fence_color.clone()),
            Transform::default(),
            Fence { grab_offset: 0.0 },
        ))
        .observe(set_material_on::<PointerOver>(fence_hover))
        .observe(set_material_on::<PointerOut>(fence_color))
        .observe(grab_fence)
        .observe(drag_fence);

    // Each part gets its own mesh, rebuilt as the board is cut.
    for part in [Part::Whole, Part::Piece, Part::Offcut] {
        commands
            .spawn((
                Mesh3d(meshes.add(rounded_box(Vec3::splat(0.01)))),
                MeshMaterial3d(wood.clone()),
                Transform::default(),
                Visibility::Hidden,
                part,
                AtTableSaw,
            ))
            .observe(highlight_carryable)
            .observe(set_material_on::<PointerOut>(wood.clone()))
            .observe(carry_clicked)
            .observe(grab_board)
            .observe(drag_board);
    }
}

fn move_fence_with_keys(keys: Res<ButtonInput<KeyCode>>, mut station: ResMut<TableSawStation>) {
    let step = if keys.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight]) {
        FENCE_BIG_STEP
    } else {
        FENCE_STEP
    };
    let fence = station.saw.fence;
    if keys.just_pressed(KeyCode::ArrowRight) {
        station.set_fence(fence + step);
    }
    if keys.just_pressed(KeyCode::ArrowLeft) {
        station.set_fence(fence - step);
    }
}

/// Remembers where on the fence the pointer grabbed it, so the fence doesn't jump to the cursor.
fn grab_fence(
    drag: On<PointerDragStart>,
    camera: Single<(&Camera, &GlobalTransform)>,
    mut fences: Query<&mut Fence>,
    station: Res<TableSawStation>,
) {
    let (camera, camera_transform) = *camera;
    if drag.button == PointerButton::Primary
        && let Some(point) = pointer_on_plane(
            camera,
            camera_transform,
            drag.pointer.position,
            ORIGIN.y + FENCE_HEIGHT,
        )
        && let Ok(mut fence) = fences.get_mut(drag.entity)
    {
        fence.grab_offset = point.x - ORIGIN.x - station.saw.fence.as_meters_f32();
    }
}

fn drag_fence(
    drag: On<PointerDrag>,
    camera: Single<(&Camera, &GlobalTransform)>,
    fences: Query<&Fence>,
    mut station: ResMut<TableSawStation>,
) {
    let (camera, camera_transform) = *camera;
    if drag.button == PointerButton::Primary
        && let Some(point) = pointer_on_plane(
            camera,
            camera_transform,
            drag.pointer.position,
            ORIGIN.y + FENCE_HEIGHT,
        )
        && let Ok(fence) = fences.get(drag.entity)
    {
        station.set_fence(snap_to_sixteenth(point.x - ORIGIN.x - fence.grab_offset));
    }
}

fn raise_blade_with_keys(keys: Res<ButtonInput<KeyCode>>, mut station: ResMut<TableSawStation>) {
    let step = if keys.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight]) {
        HEIGHT_BIG_STEP
    } else {
        HEIGHT_STEP
    };
    let height = station.saw.height;
    if keys.just_pressed(KeyCode::ArrowUp) {
        station.set_height(height + step);
    }
    if keys.just_pressed(KeyCode::ArrowDown) {
        station.set_height(height - step);
    }
}

fn push_board(
    keys: Res<ButtonInput<KeyCode>>,
    time: Res<Time>,
    mut station: ResMut<TableSawStation>,
) {
    if keys.pressed(KeyCode::Space) {
        station.push(FEED_SPEED * time.delta_secs());
    }
}

fn bring_back_with_key(keys: Res<ButtonInput<KeyCode>>, mut station: ResMut<TableSawStation>) {
    if keys.just_pressed(KeyCode::KeyB) {
        station.bring_back();
    }
}

/// Remembers where the pointer grabbed the board, so it doesn't jump to the cursor.
fn grab_board(
    drag: On<PointerDragStart>,
    camera: Single<(&Camera, &GlobalTransform)>,
    station: Res<TableSawStation>,
    mut grab: ResMut<BoardGrab>,
) {
    let (camera, camera_transform) = *camera;
    if drag.button == PointerButton::Primary
        && let Some(point) =
            pointer_on_plane(camera, camera_transform, drag.pointer.position, ORIGIN.y)
    {
        *grab = BoardGrab {
            pointer: point.z,
            advance: station.advance(),
        };
    }
}

/// Slides the board along the fence after the pointer: forward to cut, back to back it out.
fn drag_board(
    drag: On<PointerDrag>,
    camera: Single<(&Camera, &GlobalTransform)>,
    grab: Res<BoardGrab>,
    mut station: ResMut<TableSawStation>,
) {
    let (camera, camera_transform) = *camera;
    if drag.button == PointerButton::Primary
        && let Some(point) =
            pointer_on_plane(camera, camera_transform, drag.pointer.position, ORIGIN.y)
    {
        // The board feeds toward -z, so moving the pointer that way pushes it.
        station.move_to(grab.advance - snap(point.z - grab.pointer, 64));
    }
}

/// Q and E turn the board a quarter turn left and right, and F flips it over, while it waits
/// in front of the blade.
fn turn_with_keys(keys: Res<ButtonInput<KeyCode>>, mut station: ResMut<TableSawStation>) {
    if keys.just_pressed(KeyCode::KeyQ) {
        station.turn_left();
    }
    if keys.just_pressed(KeyCode::KeyE) {
        station.turn_right();
    }
    if keys.just_pressed(KeyCode::KeyF) {
        station.flip();
    }
}

/// Puts any strips a cut set loose on the parts pile by the miter saw.
fn set_loose_aside(mut table_saw: ResMut<TableSawStation>, mut miter_saw: ResMut<MiterSawStation>) {
    if table_saw.has_loose() {
        let loose = table_saw.take_loose();
        miter_saw.parts.extend(loose);
    }
}

fn new_board(keys: Res<ButtonInput<KeyCode>>, mut station: ResMut<TableSawStation>) {
    if keys.just_pressed(KeyCode::KeyR) {
        station.new_board();
    }
}

/// Carries a board off the saw to the miter saw. Up close, the camera follows it; in the
/// overview, it stays put. Returns false if that board isn't there to take.
fn carry(
    part: Part,
    table_saw: &mut TableSawStation,
    miter_saw: &mut MiterSawStation,
    view: &mut ShopView,
) -> bool {
    let Some(board) = table_saw.take(part) else {
        return false;
    };
    miter_saw.load(board);
    if *view != ShopView::Overview {
        *view = ShopView::MiterSaw;
    }
    true
}

/// C carries the piece cut against the fence, the offcut once that piece is gone, or the board
/// if it's still in one piece.
fn carry_with_key(
    keys: Res<ButtonInput<KeyCode>>,
    mut table_saw: ResMut<TableSawStation>,
    mut miter_saw: ResMut<MiterSawStation>,
    mut view: ResMut<ShopView>,
) {
    if keys.just_pressed(KeyCode::KeyC) {
        for part in [Part::Piece, Part::Offcut, Part::Whole] {
            if carry(part, &mut table_saw, &mut miter_saw, &mut view) {
                break;
            }
        }
    }
}

fn carry_clicked(
    click: On<PointerClick>,
    parts: Query<&Part>,
    shop: Res<ShopAssets>,
    mut materials: Query<&mut MeshMaterial3d<StandardMaterial>>,
    mut table_saw: ResMut<TableSawStation>,
    mut miter_saw: ResMut<MiterSawStation>,
    mut view: ResMut<ShopView>,
) {
    if click.button == PointerButton::Secondary
        && let Ok(part) = parts.get(click.entity)
        && carry(*part, &mut table_saw, &mut miter_saw, &mut view)
        && let Ok(mut material) = materials.get_mut(click.entity)
    {
        material.0 = shop.wood.clone();
    }
}

/// Lights up a board under the pointer when it can be carried off.
fn highlight_carryable(
    over: On<PointerOver>,
    station: Res<TableSawStation>,
    shop: Res<ShopAssets>,
    parts: Query<&Part>,
    mut materials: Query<&mut MeshMaterial3d<StandardMaterial>>,
) {
    if let Ok(part) = parts.get(over.entity)
        && station.can_take(*part)
        && let Ok(mut material) = materials.get_mut(over.entity)
    {
        material.0 = shop.wood_highlight.clone();
    }
}

fn draw_fence(station: Res<TableSawStation>, mut fence: Single<&mut Transform, With<Fence>>) {
    if station.is_changed() {
        let x = station.saw.fence.as_meters_f32();
        fence.translation = ORIGIN + Vec3::new(x, 0.0, TABLE_BACK);
    }
}

/// Raises the blade and riving knife to the blade's height. The blade's center is its radius
/// below its top.
fn draw_blade(
    station: Res<TableSawStation>,
    mut blade: Single<&mut Transform, (With<BladeDisc>, Without<RivingKnife>)>,
    mut knife: Single<&mut Transform, (With<RivingKnife>, Without<BladeDisc>)>,
) {
    if station.is_changed() {
        let saw = station.saw;
        let height = saw.height.as_meters_f32();
        let kerf = saw.blade.kerf.as_meters_f32();
        let center = height - saw.blade.radius().as_meters_f32();
        blade.translation = ORIGIN + Vec3::new(-kerf / 2.0, center, 0.0);
        let knife_top = height - KNIFE_BELOW_BLADE;
        knife.translation = ORIGIN + Vec3::new(-kerf * 0.8, knife_top - KNIFE_HEIGHT, inches(-6.5));
    }
}

fn draw_board(
    station: Res<TableSawStation>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut parts: Query<(&Part, &Mesh3d, &mut Transform, &mut Visibility), With<AtTableSaw>>,
) {
    if station.is_changed() {
        let boards = station.boards();
        place_parts(&boards, ORIGIN, Quat::IDENTITY, &mut meshes, &mut parts);
    }
}
