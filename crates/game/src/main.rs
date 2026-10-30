//! Woodshop Simulator. Bevy draws the shop; `woodsim` owns every board's real dimensions.

mod bench;
mod blades;
mod chop;
mod controls;
mod copy;
mod dress;
mod edit;
mod feed;
mod flights;
mod gpu;
mod grain;
mod hand;
mod hud;
mod icons;
mod infeed;
mod inventory;
mod join;
mod jointer;
mod meshes;
mod miter_saw;
mod parts;
mod planer;
mod rest;
mod saved;
mod scenery;
mod shoot;
mod shop;
mod studio;
mod table_saw;
mod textures;
mod thickness;
mod trace;
mod turning;
mod upload;
mod work;

use std::{path::PathBuf, time::Duration};

use bevy::{
    app::ScheduleRunnerPlugin,
    asset::RenderAssetUsages,
    camera::{ImageRenderTarget, RenderTarget},
    diagnostic::FrameCount,
    prelude::*,
    render::{
        gpu_readback::{ReadbackComplete, ReadbackOnce},
        render_resource::{Extent3d, TextureDimension, TextureFormat, TextureUsages},
    },
    window::ExitCondition,
    winit::WinitPlugin,
};

use bench::BenchPlugin;
use blades::BladesPlugin;
use controls::ControlsPlugin;
use copy::CopyPlugin;
use edit::EditPlugin;
use flights::FlightsPlugin;
use hand::HandPlugin;
use hud::HudPlugin;
use inventory::InventoryPlugin;
use jointer::JointerPlugin;
use miter_saw::MiterSawPlugin;
use parts::PartsPlugin;
use planer::PlanerPlugin;
use rest::RestPlugin;
use saved::SavedPlugin;
use scenery::ScenePlugin;
use shop::ShopPlugin;
use studio::StudioPlugin;
use table_saw::TableSawPlugin;
use turning::TurningPlugin;
use upload::UploadPlugin;

fn main() {
    let screenshot = std::env::var_os("WOODSHOP_SCREENSHOT").map(PathBuf::from);
    let bench = std::env::var_os("WOODSHOP_BENCH").is_some();
    let mut app = App::new();
    let plugins = DefaultPlugins.set(WindowPlugin {
        primary_window: Some(Window {
            title: "Woodshop Simulator".into(),
            ..default()
        }),
        ..default()
    });

    // Two tools draw offscreen instead of in a window, so they work while the window is covered
    // or the screen is locked (macOS gives a window nothing to draw into then), and the display
    // doesn't hold them to its refresh rate.
    //
    // `WOODSHOP_SCREENSHOT=screenshots/shop.png cargo run -p game` renders a few frames, saves
    // one to that path, and quits, so changes can be checked without anyone watching.
    //
    // `WOODSHOP_BENCH=1 cargo run -p game --release` renders the overview at 4K as fast as it
    // can for a few seconds, prints how long frames took, and quits. A 4K frame has 4x the
    // pixels of a 1080p one, so a game that holds 60 fps here should hold it on a much slower
    // GPU at 1080p.
    if screenshot.is_some() || bench {
        let (size, frame_time) = if bench {
            (UVec2::new(3840, 2160), Duration::ZERO)
        } else {
            (UVec2::new(2560, 1440), Duration::from_secs_f64(1.0 / 60.0))
        };
        app.add_plugins((
            plugins
                .set(WindowPlugin {
                    primary_window: None,
                    exit_condition: ExitCondition::DontExit,
                    ..default()
                })
                .disable::<WinitPlugin>(),
            ScheduleRunnerPlugin::run_loop(frame_time),
        ))
        .insert_resource(Offscreen {
            size,
            image: Handle::default(),
        })
        .add_systems(PostStartup, render_offscreen);
    } else {
        app.add_plugins(plugins);
    }
    app.add_plugins((
        MeshPickingPlugin,
        ShopPlugin,
        ScenePlugin,
        // A tuple of plugins holds at most 15.
        (
            TableSawPlugin,
            MiterSawPlugin,
            JointerPlugin,
            PlanerPlugin,
            BladesPlugin,
        ),
        BenchPlugin,
        HandPlugin,
        ControlsPlugin,
        InventoryPlugin,
        (PartsPlugin, EditPlugin),
        HudPlugin,
        (FlightsPlugin, TurningPlugin),
        RestPlugin,
        (CopyPlugin, UploadPlugin),
        SavedPlugin,
        StudioPlugin,
    ));

    if let Some(path) = screenshot {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).expect("create screenshot directory");
        }
        app.insert_resource(ScreenshotPath(path))
            .add_systems(Update, screenshot_and_exit);
    }
    if bench {
        app.init_resource::<Bench>()
            .add_systems(Update, measure_frames);
    }

    app.run();
}

/// The image the camera draws into when there's no window, and its size in pixels.
#[derive(Resource)]
struct Offscreen {
    size: UVec2,
    image: Handle<Image>,
}

/// Points the camera at an image instead of a window. It's drawn at the scale of a Retina
/// display, so text is the size it would be in the game's usual window.
fn render_offscreen(
    mut commands: Commands,
    mut images: ResMut<Assets<Image>>,
    mut offscreen: ResMut<Offscreen>,
    camera: Single<Entity, With<Camera3d>>,
) {
    let mut target = Image::new_target_texture(
        offscreen.size.x,
        offscreen.size.y,
        TextureFormat::Rgba8UnormSrgb,
        None,
    );
    // So screenshots can copy it back from the GPU.
    target.texture_descriptor.usage |= TextureUsages::COPY_SRC;
    offscreen.image = images.add(target);
    commands.entity(*camera).insert((
        RenderTarget::Image(ImageRenderTarget {
            handle: offscreen.image.clone(),
            scale_factor: offscreen.size.x as f32 / 1280.0,
        }),
        // UI goes to the window's camera by default, and there's no window.
        IsDefaultUiCamera,
    ));
}

#[derive(Resource)]
struct ScreenshotPath(PathBuf);

fn screenshot_and_exit(mut commands: Commands, frames: Res<FrameCount>, offscreen: Res<Offscreen>) {
    // Give shaders time to compile. This copies the image straight back rather than using
    // Bevy's `Screenshot`, whose captures of an image came out black.
    match frames.0 {
        90 => {
            commands
                .spawn(ReadbackOnce::texture(offscreen.image.clone()))
                .observe(save_capture);
        }
        300 => panic!("the screenshot never came back from the GPU"),
        _ => {}
    }
}

fn save_capture(
    capture: On<ReadbackComplete>,
    path: Res<ScreenshotPath>,
    offscreen: Res<Offscreen>,
    mut commands: Commands,
) {
    // A row of 2560 pixels is a whole multiple of the GPU's 256-byte row alignment, so the
    // rows come back without padding.
    let image = Image::new(
        Extent3d {
            width: offscreen.size.x,
            height: offscreen.size.y,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        capture.data.clone(),
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::MAIN_WORLD,
    );
    image
        .try_into_dynamic()
        .expect("an RGBA image")
        .to_rgb8()
        .save(&path.0)
        .expect("save the screenshot");
    info!("Screenshot saved to {}", path.0.display());
    commands.write_message(AppExit::Success);
}

/// Frame times measured by `WOODSHOP_BENCH`, in seconds.
#[derive(Resource, Default)]
struct Bench(Vec<f32>);

/// Frames to skip while shaders compile and the camera settles.
const BENCH_WARMUP: u32 = 180;
const BENCH_FRAMES: usize = 600;

fn measure_frames(
    mut commands: Commands,
    frames: Res<FrameCount>,
    time: Res<Time<Real>>,
    mut samples: ResMut<Bench>,
) {
    if frames.0 < BENCH_WARMUP {
        return;
    }
    samples.0.push(time.delta_secs());
    if samples.0.len() < BENCH_FRAMES {
        return;
    }
    let mut sorted = samples.0.clone();
    sorted.sort_by(f32::total_cmp);
    let ms = |seconds: f32| seconds * 1000.0;
    let average = sorted.iter().sum::<f32>() / sorted.len() as f32;
    let slowest_percent = sorted[sorted.len() * 99 / 100];
    println!(
        "{} frames: average {:.2} ms ({:.0} fps), 99th percentile {:.2} ms, worst {:.2} ms",
        sorted.len(),
        ms(average),
        1.0 / average,
        ms(slowest_percent),
        ms(sorted[sorted.len() - 1]),
    );
    commands.write_message(AppExit::Success);
}
