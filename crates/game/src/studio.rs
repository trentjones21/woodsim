//! The photo studio's Bevy side: the set (the backdrop, the piece, and a spot light where each
//! soft box is, so the view while setting up gives an idea of the lighting), the camera, the
//! controls, and the render as it comes in, shown over the view. The setup and the render
//! itself are in `shoot.rs` and `trace.rs`. Nothing here is spawned or runs until a piece is
//! brought into the studio, and it's all taken down again when you leave.

use std::{f32::consts::PI, path::Path};

use bevy::{
    asset::RenderAssetUsages,
    camera::{PerspectiveProjection, Projection, SubCameraView},
    input::{
        gestures::PinchGesture,
        mouse::{AccumulatedMouseMotion, AccumulatedMouseScroll, MouseScrollUnit},
    },
    light::{GlobalAmbientLight, NotShadowCaster},
    prelude::*,
    render::render_resource::{Extent3d, TextureDimension, TextureFormat},
};

use crate::{
    controls::{Action, Actions},
    hand::Gesture,
    hud::{OnHud, Viewfinder},
    meshes::lumber_mesh,
    shoot::{self, Studio, backdrop_mesh},
    shop::{ShopAssets, ShopCamera, ShopView},
    trace::Lamp,
};

/// Where the studio's set is, out of sight below the shop.
pub const ORIGIN: Vec3 = Vec3::new(0.0, -60.0, 0.0);

/// How far a turn of the piece goes, in radians.
const TURN_STEP: f32 = 15.0 * PI / 180.0;
/// How far dragging turns the piece, or tips the camera, per pixel, in radians.
const TURN_PER_PIXEL: f32 = 0.006;
/// Each line of scroll, or step of zoom, brings the camera in by this fraction. Trackpads
/// scroll in pixels instead.
const ZOOM_PER_LINE: f32 = 0.1;
const PIXELS_PER_LINE: f32 = 20.0;
/// How much the spot lights standing in for the soft boxes light the set while it's being set
/// up, per unit of the renderer's light, chosen so the view comes out about as bright as the
/// render.
const PREVIEW_LUMENS: f32 = 4.0 * PI * 140.0;
/// How dim the shop's ambient light is in the studio, where the soft boxes do the lighting.
const STUDIO_AMBIENT: f32 = 25.0;

pub struct StudioPlugin;

impl Plugin for StudioPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Studio>()
            .init_resource::<Set>()
            .add_systems(Startup, spawn_render_view)
            .add_systems(
                Update,
                (
                    (work_in_studio, look_around, keep_rendering)
                        .run_if(resource_equals(ShopView::Studio)),
                    build_set,
                    show_render,
                )
                    .chain(),
            );
    }
}

/// Opens the studio for `piece`, framed for now for `camera`'s whole view; once the HUD has
/// made room for the studio's settings, it's framed again for the picture.
pub fn open(studio: &mut Studio, piece: &woodsim::Assembly, camera: &Camera) {
    let aspect = camera
        .logical_viewport_size()
        .filter(|size| size.y > 0.0)
        .map_or(16.0 / 9.0, |size| size.x / size.y);
    studio.open(piece, aspect);
}

/// How the camera shows the picture in `area` of a `window` (both in logical pixels) with a
/// lens whose half-height is `tangent` (the tangent of half its view's height): a view of
/// which the window is part, centered on the picture, and how high that whole view sees.
fn off_center(area: Rect, window: Vec2, tangent: f32) -> (SubCameraView, f32) {
    // In quarter pixels, as the sub view takes whole numbers.
    const UNITS: f32 = 4.0;
    let center = area.center();
    let half = Vec2::new(
        center.x.max(window.x - center.x),
        center.y.max(window.y - center.y),
    );
    let sub_view = SubCameraView {
        full_size: (half * 2.0 * UNITS).round().as_uvec2(),
        offset: (half - center) * UNITS,
        size: (window * UNITS).round().as_uvec2(),
    };
    let fov = 2.0 * (tangent * 2.0 * half.y / area.height()).atan();
    (sub_view, fov)
}

/// Where the camera is in the studio, looking at the piece, if one's there.
pub fn camera_pose(studio: &Studio) -> Option<Transform> {
    let shoot = studio.shoot()?;
    let position = ORIGIN + shoot.camera_position(&studio.setup);
    Some(Transform::from_translation(position).looking_at(ORIGIN + shoot.target(), Vec3::Y))
}

/// Does what's asked in the studio: turns the piece, moves the camera or a light, changes the
/// backdrop or the lens or how long to take, and starts, finishes, or stops a render.
fn work_in_studio(actions: Res<Actions>, viewfinder: Res<Viewfinder>, mut studio: ResMut<Studio>) {
    for action in actions.started() {
        match action {
            Action::TurnLeft | Action::TurnRight => {
                let by = if action == Action::TurnLeft {
                    TURN_STEP
                } else {
                    -TURN_STEP
                };
                if let Some((shoot, _)) = studio.adjust() {
                    shoot.turn_by(by);
                }
            }
            Action::Flip => {
                if let Some((shoot, _)) = studio.adjust() {
                    shoot.flip();
                }
            }
            Action::ZoomIn | Action::ZoomOut => {
                let factor = if action == Action::ZoomIn {
                    1.0 - ZOOM_PER_LINE
                } else {
                    1.0 / (1.0 - ZOOM_PER_LINE)
                };
                if let Some((shoot, _)) = studio.adjust() {
                    shoot.zoom(factor);
                }
            }
            Action::Light(index) => studio.pick(index),
            Action::Swing(step) => studio.swing(step),
            Action::Raise(step) => studio.raise(step),
            Action::Brighten(step) => studio.brighten(step),
            Action::Tip(step) => {
                let degrees = if step.big { 5.0 } else { 1.0 };
                let sign = if step.more { 1.0 } else { -1.0 };
                studio.tip(sign * degrees * PI / 180.0);
            }
            Action::Lens(step) => studio.change_lens(step),
            Action::Backdrop(step) => studio.change_backdrop(step),
            Action::Quality(step) => studio.change_quality(step),
            Action::Render if studio.rendering() => {
                studio.finish_now(Path::new(shoot::FOLDER));
            }
            Action::Render => {
                if let Some(aspect) = viewfinder.aspect() {
                    studio.render(aspect);
                }
            }
            Action::StopRender => studio.stop(),
            _ => {}
        }
    }
}

/// Dragging sideways turns the piece, and up or down tips the camera; scrolling or pinching
/// zooms.
fn look_around(
    mouse: Res<ButtonInput<MouseButton>>,
    motion: Res<AccumulatedMouseMotion>,
    scroll: Res<AccumulatedMouseScroll>,
    mut pinches: MessageReader<PinchGesture>,
    gesture: Res<Gesture>,
    on_hud: Res<OnHud>,
    mut studio: ResMut<Studio>,
) {
    let pinch: f32 = pinches.read().map(|pinch| pinch.0).sum();
    if studio.rendering() {
        return;
    }
    if mouse.pressed(MouseButton::Left)
        && !on_hud.pressed()
        && gesture.dragged()
        && motion.delta != Vec2::ZERO
    {
        if let Some((shoot, _)) = studio.adjust() {
            shoot.turn_by(motion.delta.x * TURN_PER_PIXEL);
        }
        studio.tip(motion.delta.y * TURN_PER_PIXEL);
    }
    // Scrolling over the HUD scrolls the inventory, not the view.
    let lines = match scroll.unit {
        _ if on_hud.over() => 0.0,
        MouseScrollUnit::Line => scroll.delta.y,
        MouseScrollUnit::Pixel => scroll.delta.y / PIXELS_PER_LINE,
    };
    if (lines != 0.0 || pinch != 0.0)
        && let Some((shoot, _)) = studio.adjust()
    {
        shoot.zoom((1.0 - ZOOM_PER_LINE).powf(lines) * (1.0 - pinch));
    }
}

/// Saves the render once it's done, and keeps its progress showing until then.
fn keep_rendering(mut studio: ResMut<Studio>) {
    if !studio.bypass_change_detection().rendering() {
        return;
    }
    // Its progress is in the status, which follows changes to the studio.
    studio.set_changed();
    studio.poll(Path::new(shoot::FOLDER));
}

/// What's built of the set: the piece it was built for (counting each piece brought in), its
/// boards, the backdrop and its material, and the lights; and the shop's sun and ambient light,
/// put out while the studio's in use.
#[derive(Resource, Default)]
struct Set {
    built_for: Option<u64>,
    /// The piece framed for the picture, once the HUD's made room for it.
    framed_for: Option<u64>,
    /// Whether the lights have been put where the setup says since the set was built.
    placed: bool,
    backdrop_material: Option<Handle<StandardMaterial>>,
    ambient: Option<f32>,
}

/// Something in the studio's set, taken down when the studio's left.
#[derive(Component)]
struct OnSet;

/// One of the piece's boards, by its place in the piece.
#[derive(Component)]
struct SetBoard(usize);

/// The set's boards, not its lights or backdrop.
type OnlyBoards = (Without<SetLamp>, Without<SetBackdrop>);

#[derive(Component)]
struct SetBackdrop;

/// A spot light standing in for a soft box, by its place among the lights.
#[derive(Component)]
struct SetLamp(usize);

/// Builds the set when a piece comes into the studio, keeps it as the setup changes, and
/// takes it down, putting the shop's lights back, when the studio's left.
#[allow(clippy::too_many_arguments)]
fn build_set(
    mut commands: Commands,
    view: Res<ShopView>,
    mut studio: ResMut<Studio>,
    mut set: ResMut<Set>,
    shop: Res<ShopAssets>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut ambient: ResMut<GlobalAmbientLight>,
    mut sun: Query<&mut Visibility, (With<DirectionalLight>, Without<OnSet>)>,
    viewfinder: Res<Viewfinder>,
    mut camera: Single<(&mut Camera, &mut Projection), With<ShopCamera>>,
    mut boards: Query<(&SetBoard, &mut Transform), OnlyBoards>,
    mut lamps: Query<(&SetLamp, &mut Transform, &mut SpotLight), Without<SetBoard>>,
    on_set: Query<Entity, With<OnSet>>,
) {
    let in_studio = *view == ShopView::Studio;
    if !in_studio {
        if view.is_changed() && studio.bypass_change_detection().shoot().is_some() {
            studio.close();
        }
        if set.built_for.is_some() {
            for entity in &on_set {
                commands.entity(entity).despawn();
            }
            set.built_for = None;
            if let Some(brightness) = set.ambient.take() {
                ambient.brightness = brightness;
            }
            for mut visibility in &mut sun {
                *visibility = Visibility::Inherited;
            }
            let (camera, projection) = &mut *camera;
            camera.sub_camera_view = None;
            if let Projection::Perspective(perspective) = projection.as_mut() {
                perspective.fov = PerspectiveProjection::default().fov;
            }
        }
        return;
    }
    let opened = studio.opened();
    if set.framed_for != Some(opened)
        && let Some(aspect) = viewfinder.aspect()
    {
        studio.reframe(aspect);
        set.framed_for = Some(opened);
    }
    let Some(shoot) = studio.shoot() else {
        return;
    };
    if set.built_for != Some(opened) {
        for entity in &on_set {
            commands.entity(entity).despawn();
        }
        set.built_for = Some(opened);
        if set.ambient.is_none() {
            set.ambient = Some(ambient.brightness);
            ambient.brightness = STUDIO_AMBIENT;
        }
        for mut visibility in &mut sun {
            *visibility = Visibility::Hidden;
        }
        let backdrop_material = materials.add(StandardMaterial {
            base_color: studio.setup.backdrop.color(),
            perceptual_roughness: 0.9,
            ..default()
        });
        set.backdrop_material = Some(backdrop_material.clone());
        commands.spawn((
            Mesh3d(meshes.add(backdrop_mesh(&shoot.backdrop()))),
            MeshMaterial3d(backdrop_material),
            Transform::from_translation(ORIGIN),
            NotShadowCaster,
            SetBackdrop,
            OnSet,
        ));
        for (index, posed) in shoot.posed().into_iter().enumerate() {
            commands.spawn((
                Mesh3d(meshes.add(lumber_mesh(
                    posed.board,
                    Vec3::new(0.13, 0.0, 0.41) * index as f32,
                ))),
                MeshMaterial3d(shop.wood.clone()),
                Transform::from_translation(ORIGIN + posed.corner).with_rotation(posed.rotation),
                SetBoard(index),
                OnSet,
            ));
        }
        for index in 0..3 {
            commands.spawn((
                SpotLight {
                    range: 40.0,
                    // The key light casts the shadows.
                    shadow_maps_enabled: index == 0,
                    outer_angle: 1.2,
                    inner_angle: 0.5,
                    ..default()
                },
                Transform::default(),
                SetLamp(index),
                OnSet,
            ));
        }
        // The lights are placed once they're there, next frame.
        set.placed = false;
        return;
    }
    if !studio.is_changed() && !viewfinder.is_changed() && set.placed {
        return;
    }
    set.placed = !lamps.is_empty();
    let posed = shoot.posed();
    for (board, mut transform) in &mut boards {
        if let Some(posed) = posed.get(board.0) {
            *transform =
                Transform::from_translation(ORIGIN + posed.corner).with_rotation(posed.rotation);
        }
    }
    let target = ORIGIN + shoot.target();
    let placed: Vec<Lamp> = shoot.lamps(&studio.setup);
    for (lamp, mut transform, mut light) in &mut lamps {
        let Some(placed) = placed.get(lamp.0) else {
            continue;
        };
        *transform =
            Transform::from_translation(ORIGIN + placed.center).looking_at(target, Vec3::Y);
        light.intensity =
            PREVIEW_LUMENS * placed.radiance.x * 4.0 * placed.u.cross(placed.v).length();
    }
    if let Some(mut material) = set
        .backdrop_material
        .as_ref()
        .and_then(|handle| materials.get_mut(handle))
    {
        let backdrop = studio.setup.backdrop;
        material.base_color = backdrop.color();
        material.perceptual_roughness = if backdrop.glossy() { 0.15 } else { 0.9 };
        material.reflectance = if backdrop.glossy() { 0.5 } else { 0.3 };
    }
    if let Some(area) = viewfinder.area {
        let (sub_view, fov) = off_center(area, viewfinder.window, studio.setup.tangent());
        let (camera, projection) = &mut *camera;
        if camera.sub_camera_view != Some(sub_view) {
            camera.sub_camera_view = Some(sub_view);
        }
        if let Projection::Perspective(perspective) = projection.as_mut() {
            perspective.fov = fov;
        }
    }
}

/// Where the render shows as it comes in, over the view and under the HUD.
#[derive(Component)]
struct RenderView {
    version: u64,
    size: (u32, u32),
}

fn spawn_render_view(mut commands: Commands) {
    commands.spawn((
        Node {
            position_type: PositionType::Absolute,
            display: Display::None,
            ..default()
        },
        ImageNode::default(),
        GlobalZIndex(-1),
        Pickable::IGNORE,
        RenderView {
            version: 0,
            size: (0, 0),
        },
    ));
}

/// Shows the render over the view while it comes in and once it's done, and hides it while
/// setting up.
fn show_render(
    studio: Res<Studio>,
    view: Res<ShopView>,
    viewfinder: Res<Viewfinder>,
    mut images: ResMut<Assets<Image>>,
    mut render_view: Single<(&mut RenderView, &mut ImageNode, &mut Node)>,
) {
    let (shown, image, node) = &mut *render_view;
    let showing = *view == ShopView::Studio && (studio.rendering() || studio.done());
    let display = if showing {
        Display::Flex
    } else {
        Display::None
    };
    if node.display != display {
        node.display = display;
    }
    if !showing {
        shown.version = 0;
        return;
    }
    if let Some(area) = viewfinder.area {
        let (left, top) = (px(area.min.x), px(area.min.y));
        let (width, height) = (px(area.width()), px(area.height()));
        if (node.left, node.top, node.width, node.height) != (left, top, width, height) {
            (node.left, node.top, node.width, node.height) = (left, top, width, height);
        }
    }
    let Some((picture, size)) = studio.picture_since(shown.version) else {
        return;
    };
    shown.version = picture.version;
    let extent = Extent3d {
        width: size.0,
        height: size.1,
        depth_or_array_layers: 1,
    };
    if shown.size != size || images.get(&image.image).is_none() {
        image.image = images.add(Image::new(
            extent,
            TextureDimension::D2,
            picture.pixels,
            TextureFormat::Rgba8UnormSrgb,
            RenderAssetUsages::MAIN_WORLD | RenderAssetUsages::RENDER_WORLD,
        ));
        shown.size = size;
    } else if let Some(mut existing) = images.get_mut(&image.image) {
        existing.data = Some(picture.pixels);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_picture_is_the_middle_of_a_view_the_window_is_part_of() {
        let window = Vec2::new(1280.0, 720.0);
        let area = Rect::new(340.0, 72.0, 1266.0, 560.0);
        let tangent = 12.0 / 70.0;
        let (sub_view, fov) = off_center(area, window, tangent);
        let full = sub_view.full_size.as_vec2() / 4.0;
        let offset = sub_view.offset / 4.0;
        // The picture's middle is the whole view's middle.
        let middle = offset + area.center();
        assert!(
            (middle - full / 2.0).abs().max_element() < 0.5,
            "{middle} {full}"
        );
        // The window fits in the whole view.
        assert!(offset.x >= 0.0 && offset.y >= 0.0);
        assert!(offset.x + window.x <= full.x + 0.5 && offset.y + window.y <= full.y + 0.5);
        // The picture is as high as the lens sees.
        let seen = (fov / 2.0).tan() * area.height() / full.y;
        assert!((seen - tangent).abs() < 1e-4, "{seen}");
    }
}
