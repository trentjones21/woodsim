//! Woodshop Simulator. Bevy draws the shop; `woodsim` owns every board's real dimensions.

mod chop;
mod feed;
mod miter_saw;
mod shop;
mod table_saw;

use std::path::PathBuf;

use bevy::{
    diagnostic::FrameCount,
    prelude::*,
    render::view::screenshot::{Screenshot, save_to_disk},
};

use miter_saw::MiterSawPlugin;
use shop::ShopPlugin;
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
        ShopPlugin,
        TableSawPlugin,
        MiterSawPlugin,
    ));

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
