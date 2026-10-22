//! Woodshop Simulator. Bevy draws the shop; `woodsim` owns every board's real dimensions.

use std::path::PathBuf;

use bevy::{
    diagnostic::FrameCount,
    prelude::*,
    render::view::screenshot::{Screenshot, save_to_disk},
};
use woodsim::{Blade, Board, Length, MiterSaw, TableSaw};

/// Space between boards laid out on the floor, so each piece is easy to see.
const LAYOUT_GAP: Length = Length::from_inches(2);

fn main() {
    let mut app = App::new();
    app.add_plugins(DefaultPlugins.set(WindowPlugin {
        primary_window: Some(Window {
            title: "Woodshop Simulator".into(),
            ..default()
        }),
        ..default()
    }))
    .add_systems(Startup, setup);

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

/// Rips a 1x8 to 6" wide, crosscuts that to 24" long, and lays every piece on the floor.
fn setup(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let one_by_eight = Board::new(
        Length::from_inches(48),
        Length::from_inch_fraction(29, 4),
        Length::from_inch_fraction(3, 4),
    );
    let rip = TableSaw {
        blade: Blade::FULL_KERF,
        fence: Length::from_inches(6),
    }
    .rip(&one_by_eight)
    .expect("a 6\" rip fits a 1x8");
    let crosscut = MiterSaw {
        blade: Blade::FULL_KERF,
    }
    .crosscut(&rip.piece, Length::from_inches(24))
    .expect("a 24\" crosscut fits a 48\" board");

    let pieces: Vec<Board> = [Some(crosscut.piece), crosscut.offcut, rip.offcut]
        .into_iter()
        .flatten()
        .collect();
    for piece in &pieces {
        info!("cut piece: {piece}");
    }

    // Lay the pieces flat and side by side, left ends aligned, centered on the origin.
    let longest = pieces.iter().map(Board::length).max().unwrap_or_default();
    let total_width = pieces
        .iter()
        .map(|piece| piece.width() + LAYOUT_GAP)
        .sum::<Length>()
        - LAYOUT_GAP;
    let wood = materials.add(Color::srgb(0.87, 0.72, 0.53));
    let left = -longest.as_meters_f32() / 2.0;
    let mut z = -total_width.as_meters_f32() / 2.0;
    for piece in &pieces {
        let length = piece.length().as_meters_f32();
        let width = piece.width().as_meters_f32();
        let thickness = piece.thickness().as_meters_f32();
        commands.spawn((
            // Length along X, width along Z, thickness up Y.
            Mesh3d(meshes.add(Cuboid::new(length, thickness, width))),
            MeshMaterial3d(wood.clone()),
            Transform::from_xyz(left + length / 2.0, thickness / 2.0, z + width / 2.0),
        ));
        z += width + LAYOUT_GAP.as_meters_f32();
    }

    commands.spawn((
        Mesh3d(meshes.add(Plane3d::default().mesh().size(4.0, 4.0))),
        MeshMaterial3d(materials.add(Color::srgb(0.55, 0.55, 0.52))),
    ));
    commands.spawn((
        DirectionalLight {
            shadow_maps_enabled: true,
            ..default()
        },
        Transform::from_xyz(2.0, 4.0, 1.0).looking_at(Vec3::ZERO, Vec3::Y),
    ));
    commands.spawn((
        Camera3d::default(),
        Transform::from_xyz(0.0, 0.9, 1.1).looking_at(Vec3::ZERO, Vec3::Y),
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
