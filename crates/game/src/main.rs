//! Woodshop Simulator. Bevy draws the shop; `woodsim` owns every board's real dimensions.

mod feed;
mod table_saw;

use std::path::PathBuf;

use bevy::{
    diagnostic::FrameCount,
    prelude::*,
    render::view::screenshot::{Screenshot, save_to_disk},
};

use table_saw::TableSawPlugin;

fn main() {
    let mut app = App::new();
    app.add_plugins((
        DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: "Woodshop Simulator".into(),
                ..default()
            }),
            ..default()
        }),
        MeshPickingPlugin,
        TableSawPlugin,
    ))
    .add_systems(Startup, spawn_shop);

    // `WOODSHOP_SCREENSHOT=screenshots/shop.png cargo run -p game` renders a few frames,
    // saves one to that path, and quits, so changes can be checked without anyone watching.
    if let Some(path) = std::env::var_os("WOODSHOP_SCREENSHOT").map(PathBuf::from) {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).expect("create screenshot directory");
        }
        app.insert_resource(ScreenshotPath(path))
            .add_systems(Update, screenshot_and_exit);
    }

    app.run();
}

/// The garage around the saw: a floor, a light, and the operator's view from the infeed side.
fn spawn_shop(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    commands.spawn((
        Mesh3d(meshes.add(Plane3d::default().mesh().size(6.0, 6.0))),
        MeshMaterial3d(materials.add(Color::srgb(0.55, 0.55, 0.52))),
        Pickable::IGNORE,
    ));
    commands.spawn((
        DirectionalLight {
            shadow_maps_enabled: true,
            ..default()
        },
        Transform::from_xyz(1.0, 4.0, 2.0).looking_at(Vec3::ZERO, Vec3::Y),
    ));
    // Standing at the front of the saw, a little left of the blade, looking down at the table.
    commands.spawn((
        Camera3d::default(),
        Transform::from_xyz(-0.3, 1.65, 1.35).looking_at(Vec3::new(0.1, 0.86, -0.05), Vec3::Y),
    ));
}

#[derive(Resource)]
struct ScreenshotPath(PathBuf);

fn screenshot_and_exit(mut commands: Commands, frames: Res<FrameCount>, path: Res<ScreenshotPath>) {
    // Give the renderer time to warm up; the capture lands a frame or two after the request.
    match frames.0 {
        30 => {
            commands
                .spawn(Screenshot::primary_window())
                .observe(save_to_disk(path.0.clone()));
        }
        60 => {
            commands.write_message(AppExit::Success);
        }
        _ => {}
    }
}
