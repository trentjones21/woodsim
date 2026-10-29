//! The studio's renderer on the GPU: the path tracer in `trace.rs`, as the compute shader in
//! `trace.wgsl`, run on the GPU Bevy draws with, from a background thread. It takes the same
//! `Scene` the CPU renderer does and gives the same `Picture`s, four or five times faster on an
//! M5 Pro; the CPU one stays as the reference it's tested against.
//!
//! The shader's constants are written in front of it from the Rust ones (`source`), and its
//! Sobol directions handed to it from `trace`'s, so the two can't drift apart. Each run of the
//! shader adds samples to a band of rows, as many rows and samples as take about `PASS_TIME`,
//! so the game keeps drawing frames between runs; every `DEVELOP_EVERY`, the picture is
//! developed on the GPU and read back.

use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU32, Ordering},
        mpsc,
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

use bevy::{
    math::Vec3,
    render::{
        render_resource::{
            BindGroup, BindGroupDescriptor, BindGroupEntry, Buffer, BufferDescriptor,
            BufferInitDescriptor, BufferUsages, CommandEncoderDescriptor, ComputePassDescriptor,
            ComputePipeline, MapMode, PipelineCompilationOptions, PollType,
            RawComputePipelineDescriptor, ShaderModuleDescriptor, ShaderSource,
        },
        renderer::{RenderDevice, RenderQueue},
    },
};

use crate::{
    grain::PINE,
    trace::{self, Material, Picture, Scene},
};

const SHADER: &str = include_str!("trace.wgsl");

/// About how long one run of the shader should take.
const PASS_TIME: Duration = Duration::from_millis(12);
/// The most samples one run takes.
const MOST_PER_PASS: u32 = 64;
/// How often the picture's developed while a render runs.
const DEVELOP_EVERY: Duration = Duration::from_millis(400);
/// The shader's square of pixels for each workgroup.
const WORKGROUP: u32 = 8;

/// The GPU, and the queue its work goes in.
#[derive(Clone)]
pub struct Gpu {
    pub device: RenderDevice,
    pub queue: RenderQueue,
}

/// What a render's thread shares with the studio.
#[derive(Default)]
struct Shared {
    stop: AtomicBool,
    done: AtomicBool,
    samples: AtomicU32,
    picture: Mutex<Picture>,
}

/// A render under way on the GPU. Dropping it stops it.
pub struct GpuRender {
    shared: Arc<Shared>,
    worker: Option<JoinHandle<()>>,
    started: Instant,
    size: (u32, u32),
    samples: u32,
}

impl GpuRender {
    /// Starts rendering `scene` on `gpu`, `width` by `height` pixels with `samples` samples each.
    pub fn start(gpu: Gpu, scene: &Scene, width: u32, height: u32, samples: u32) -> GpuRender {
        let laid_out = Layout::of(scene);
        let shared = Arc::new(Shared::default());
        let worker = {
            let shared = shared.clone();
            thread::spawn(move || run(&gpu, &laid_out, (width, height), samples, &shared))
        };
        GpuRender {
            shared,
            worker: Some(worker),
            started: Instant::now(),
            size: (width, height),
            samples,
        }
    }

    pub fn size(&self) -> (u32, u32) {
        self.size
    }

    /// How far along it is, from 0 to 1.
    pub fn progress(&self) -> f32 {
        self.samples() as f32 / self.samples.max(1) as f32
    }

    /// How many samples each pixel has had.
    pub fn samples(&self) -> u32 {
        self.shared.samples.load(Ordering::Relaxed)
    }

    /// Whether it's finished, or its thread has stopped.
    pub fn is_done(&self) -> bool {
        self.shared.done.load(Ordering::Acquire)
            || self.worker.as_ref().is_none_or(JoinHandle::is_finished)
    }

    pub fn elapsed(&self) -> Duration {
        self.started.elapsed()
    }

    /// The picture as it's come out so far, if it's been developed again since `version`.
    pub fn picture_since(&self, version: u64) -> Option<Picture> {
        let picture = self.shared.picture.lock().ok()?;
        (picture.version > version).then(|| picture.clone())
    }

    /// Stops where it's got to, and gives the picture as it is.
    pub fn stop_and_finish(self) -> Picture {
        self.shared.stop.store(true, Ordering::Relaxed);
        self.finish()
    }

    /// Waits for it to finish, and gives the finished picture.
    pub fn finish(mut self) -> Picture {
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
        self.shared
            .picture
            .lock()
            .map(|picture| picture.clone())
            .unwrap_or_default()
    }
}

impl Drop for GpuRender {
    fn drop(&mut self) {
        self.shared.stop.store(true, Ordering::Relaxed);
    }
}

/// The shader, with the constants it shares with the CPU renderer in front of it.
fn source() -> String {
    let vec3 = |v: Vec3| format!("vec3<f32>({:?}, {:?}, {:?})", v.x, v.y, v.z);
    format!(
        "const MAX_BOUNCES: u32 = {}u;\n\
         const OFFSET: f32 = {:?};\n\
         const BRIGHTEST_BOUNCE: f32 = {:?};\n\
         const END_GRAIN: f32 = {:?};\n\
         const SATIN: f32 = {:?};\n\
         const SATIN_ON_END_GRAIN: f32 = {:?};\n\
         const FINISH_TINT = {};\n\
         const FINISH_REFLECTANCE: f32 = {:?};\n\
         const EARLYWOOD = {};\n\
         const LATEWOOD = {};\n\
         const RING: f32 = {:?};\n\
         const WAVINESS: f32 = {:?};\n\
         {SHADER}",
        trace::MAX_BOUNCES,
        trace::OFFSET,
        trace::BRIGHTEST_BOUNCE,
        trace::END_GRAIN,
        trace::SATIN,
        trace::SATIN_ON_END_GRAIN,
        vec3(trace::FINISH_TINT),
        trace::FINISH_REFLECTANCE,
        vec3(PINE.earlywood),
        vec3(PINE.latewood),
        PINE.ring,
        PINE.waviness,
    )
}

/// Bytes for the GPU, in the layout `trace.wgsl` reads: every field four 32-bit words.
#[derive(Default)]
struct Bytes(Vec<u8>);

impl Bytes {
    fn words(&mut self, words: [u32; 4]) {
        for word in words {
            self.0.extend_from_slice(&word.to_le_bytes());
        }
    }

    fn floats(&mut self, x: f32, y: f32, z: f32, w: f32) {
        self.words([x, y, z, w].map(f32::to_bits));
    }

    /// `v`, and `w` as bits.
    fn vector(&mut self, v: Vec3, w: u32) {
        self.words([v.x.to_bits(), v.y.to_bits(), v.z.to_bits(), w]);
    }

    /// The bytes, or a field's worth of zeros if there are none, as a buffer can't be empty.
    fn finished(mut self) -> Vec<u8> {
        if self.0.is_empty() {
            self.words([0; 4]);
        }
        self.0
    }
}

/// A scene laid out for the shader, all but what changes from run to run.
struct Layout {
    nodes: Vec<u8>,
    triangles: Vec<u8>,
    shading: Vec<u8>,
    materials: Vec<u8>,
    lamps: Vec<u8>,
    triangle_count: u32,
    lamp_count: u32,
    camera: trace::Camera,
    environment: trace::Environment,
    exposure: f32,
}

impl Layout {
    fn of(scene: &Scene) -> Layout {
        let mut nodes = Bytes::default();
        for node in &scene.bvh.nodes {
            nodes.vector(node.min, node.first);
            nodes.vector(node.max, node.count);
        }
        let mut triangles = Bytes::default();
        for triangle in &scene.bvh.packed {
            triangles.vector(triangle.corner, 0);
            triangles.vector(triangle.edge1, 0);
            triangles.vector(triangle.edge2, 0);
        }
        let mut shading = Bytes::default();
        for shade in &scene.bvh.shading {
            shading.vector(shade.geometric, shade.material);
            for normal in shade.normals {
                shading.vector(normal, 0);
            }
        }
        let mut materials = Bytes::default();
        for material in &scene.materials {
            let (kind, color, roughness) = match material {
                Material::Matte(color) => (0, *color, 0.0),
                Material::Finished { color, roughness } => (1, *color, *roughness),
                Material::Wood(_) => (2, Vec3::ZERO, 0.0),
            };
            materials.words([kind, roughness.to_bits(), 0, 0]);
            materials.vector(color, 0);
            match material {
                Material::Wood(wood) => {
                    let matrix = wood.to_stock.matrix3;
                    let shift = wood.to_stock.translation;
                    for row in 0..3 {
                        materials.floats(
                            matrix.x_axis[row],
                            matrix.y_axis[row],
                            matrix.z_axis[row],
                            shift[row],
                        );
                    }
                    materials.vector(wood.along, 0);
                    let log = &wood.log;
                    materials.floats(log.pith.x, log.pith.y, log.drift.x, log.drift.y);
                    materials.vector(log.tone, log.seed);
                }
                _ => {
                    for _ in 0..6 {
                        materials.words([0; 4]);
                    }
                }
            }
        }
        let mut lamps = Bytes::default();
        for lamp in &scene.lamps {
            lamps.vector(lamp.center, 0);
            lamps.vector(lamp.u, 0);
            lamps.vector(lamp.v, 0);
            lamps.vector(lamp.radiance, 0);
        }
        Layout {
            nodes: nodes.finished(),
            triangles: triangles.finished(),
            shading: shading.finished(),
            materials: materials.finished(),
            lamps: lamps.finished(),
            triangle_count: scene.bvh.packed.len() as u32,
            lamp_count: scene.lamps.len() as u32,
            camera: scene.camera,
            environment: scene.environment,
            exposure: scene.exposure,
        }
    }

    /// What the shader's told for a run that takes samples `from` up to `to`, of the rows
    /// from `row` on of a picture `size` pixels.
    fn params(&self, size: (u32, u32), from: u32, to: u32, row: u32) -> Vec<u8> {
        let (camera, environment) = (&self.camera, &self.environment);
        let mut params = Bytes::default();
        params.vector(camera.position, 0);
        params.vector(camera.forward, 0);
        params.vector(camera.right, 0);
        params.vector(camera.up, 0);
        params.floats(
            camera.tan_half_height,
            camera.aperture,
            camera.focus,
            size.0 as f32 / size.1 as f32,
        );
        params.vector(environment.zenith, environment.panels.to_bits());
        params.vector(environment.horizon, 0);
        params.vector(environment.ground, 0);
        params.words([size.0, size.1, from, to]);
        params.words([self.lamp_count, self.triangle_count, row, 0]);
        params.floats(self.exposure, to as f32, 0.0, 0.0);
        for directions in trace::SOBOL.as_flattened().as_chunks::<4>().0 {
            params.words(*directions);
        }
        params.finished()
    }
}

/// Renders on the GPU until every sample's taken or the render's stopped.
fn run(gpu: &Gpu, scene: &Layout, size: (u32, u32), samples: u32, shared: &Shared) {
    let device = gpu.device.wgpu_device();
    let module = device.create_shader_module(ShaderModuleDescriptor {
        label: Some("studio path tracer"),
        source: ShaderSource::Wgsl(source().into()),
    });
    let pipeline = |entry_point: &str| {
        gpu.device
            .create_compute_pipeline(&RawComputePipelineDescriptor {
                label: Some(entry_point),
                layout: None,
                module: &module,
                entry_point: Some(entry_point),
                compilation_options: PipelineCompilationOptions::default(),
                cache: None,
            })
    };
    let (trace, develop) = (pipeline("trace"), pipeline("develop"));
    let filled = |label: &str, contents: &[u8], usage: BufferUsages| {
        gpu.device.create_buffer_with_data(&BufferInitDescriptor {
            label: Some(label),
            contents,
            usage,
        })
    };
    let empty = |label: &str, size: u64, usage: BufferUsages| {
        gpu.device.create_buffer(&BufferDescriptor {
            label: Some(label),
            size,
            usage,
            mapped_at_creation: false,
        })
    };
    let pixels = size.0 as u64 * size.1 as u64;
    let buffers = [
        filled("nodes", &scene.nodes, BufferUsages::STORAGE),
        filled("triangles", &scene.triangles, BufferUsages::STORAGE),
        filled("shading", &scene.shading, BufferUsages::STORAGE),
        filled("materials", &scene.materials, BufferUsages::STORAGE),
        filled("lamps", &scene.lamps, BufferUsages::STORAGE),
        filled(
            "params",
            &scene.params(size, 0, 0, 0),
            BufferUsages::STORAGE | BufferUsages::COPY_DST,
        ),
        empty("light", pixels * 16, BufferUsages::STORAGE),
        empty(
            "pixels",
            pixels * 4,
            BufferUsages::STORAGE | BufferUsages::COPY_SRC,
        ),
    ];
    let staging = empty(
        "pixels to read",
        pixels * 4,
        BufferUsages::MAP_READ | BufferUsages::COPY_DST,
    );
    let group = |pipeline: &ComputePipeline, bindings: &[u32]| {
        let entries: Vec<BindGroupEntry> = bindings
            .iter()
            .map(|&binding| BindGroupEntry {
                binding,
                resource: buffers[binding as usize].as_entire_binding(),
            })
            .collect();
        BindGroup::from(device.create_bind_group(&BindGroupDescriptor {
            label: None,
            layout: &pipeline.get_bind_group_layout(0),
            entries: &entries,
        }))
    };
    let trace_group = group(&trace, &[0, 1, 2, 3, 4, 5, 6]);
    let develop_group = group(&develop, &[5, 6, 7]);
    let dispatch = |pipeline: &ComputePipeline, bind_group: &BindGroup, rows: u32| {
        let mut encoder = device.create_command_encoder(&CommandEncoderDescriptor::default());
        {
            let mut pass = encoder.begin_compute_pass(&ComputePassDescriptor::default());
            pass.set_pipeline(pipeline);
            pass.set_bind_group(0, bind_group, &[]);
            pass.dispatch_workgroups(size.0.div_ceil(WORKGROUP), rows.div_ceil(WORKGROUP), 1);
        }
        encoder
    };
    let develop_and_read = || {
        let mut encoder = dispatch(&develop, &develop_group, size.1);
        encoder.copy_buffer_to_buffer(&buffers[7], 0, &staging, 0, pixels * 4);
        gpu.queue.submit([encoder.finish()]);
        let slice = staging.slice(..);
        let (sender, receiver) = mpsc::channel();
        slice.map_async(MapMode::Read, move |result| {
            let _ = sender.send(result);
        });
        let _ = device.poll(PollType::wait_indefinitely());
        if let Ok(Ok(())) = receiver.recv()
            && let Ok(view) = slice.get_mapped_range()
        {
            let developed = view.to_vec();
            drop(view);
            staging.unmap();
            if let Ok(mut picture) = shared.picture.lock() {
                picture.pixels = developed;
                picture.version += 1;
            }
        }
    };
    // How many samples a run takes, and of how many rows: if a run's slow, fewer samples, or
    // once it's down to one, fewer rows; if it's quick, more rows, or once it's the whole
    // picture, more samples.
    let (mut from, mut per_pass, mut rows) = (0, 1, WORKGROUP * 8);
    let mut developed = Instant::now();
    'samples: while from < samples {
        let to = (from + per_pass).min(samples);
        let mut row = 0;
        while row < size.1 {
            if shared.stop.load(Ordering::Relaxed) {
                break 'samples;
            }
            let band = rows.min(size.1 - row);
            let started = Instant::now();
            write(&gpu.queue, &buffers[5], &scene.params(size, from, to, row));
            gpu.queue
                .submit([dispatch(&trace, &trace_group, band).finish()]);
            let _ = device.poll(PollType::wait_indefinitely());
            let took = started.elapsed();
            row += band;
            if took > PASS_TIME * 2 {
                if per_pass > 1 {
                    per_pass /= 2;
                } else {
                    rows = (rows / 2).max(WORKGROUP);
                }
            } else if took < PASS_TIME / 2 {
                if rows < size.1 {
                    rows = (rows * 2).min(size.1);
                } else {
                    per_pass = (per_pass * 2).min(MOST_PER_PASS);
                }
            }
        }
        from = to;
        shared.samples.store(from, Ordering::Relaxed);
        if from == samples || developed.elapsed() > DEVELOP_EVERY {
            develop_and_read();
            developed = Instant::now();
        }
    }
    if from > 0 && shared.stop.load(Ordering::Relaxed) {
        // Stopped early: the picture as it's got to.
        develop_and_read();
    }
    shared.done.store(true, Ordering::Release);
}

fn write(queue: &RenderQueue, buffer: &Buffer, bytes: &[u8]) {
    queue.write_buffer(buffer, 0, bytes);
}

/// A GPU of their own, for tests, which run without the game.
#[cfg(test)]
pub fn for_tests() -> Gpu {
    let resources = bevy::tasks::block_on(bevy::render::renderer::initialize_renderer(
        bevy::render::settings::Backends::all(),
        None,
        &bevy::render::settings::WgpuSettings::default(),
    ));
    Gpu {
        device: resources.0,
        queue: resources.1,
    }
}

#[cfg(test)]
mod tests {
    use woodsim::{Assembly, Board, Length};

    use super::*;
    use crate::{
        shoot::{Setup, Shoot},
        trace::{Camera, Environment, Lamp, Render, Triangle},
    };

    /// The average color of each square of `block` pixels, of a picture `width` wide.
    fn blocks(picture: &Picture, width: usize, block: usize) -> Vec<[f32; 3]> {
        let height = picture.pixels.len() / 4 / width;
        let mut averages = Vec::new();
        for by in (0..height).step_by(block) {
            for bx in (0..width).step_by(block) {
                let mut sum = [0.0; 3];
                for y in by..(by + block).min(height) {
                    for x in bx..(bx + block).min(width) {
                        for (channel, total) in sum.iter_mut().enumerate() {
                            *total += picture.pixels[(y * width + x) * 4 + channel] as f32;
                        }
                    }
                }
                averages.push(sum.map(|total| total / (block * block) as f32));
            }
        }
        averages
    }

    /// Renders the scene `scene` makes on the CPU and on the GPU, and checks they come out the
    /// same, square by square, within the noise of `samples` samples.
    fn same_both_ways(scene: impl Fn() -> Scene, size: (u32, u32), samples: u32) {
        let cpu = Render::start(scene(), size.0, size.1, samples).finish();
        let on_gpu = GpuRender::start(for_tests(), &scene(), size.0, size.1, samples).finish();
        assert_eq!(on_gpu.pixels.len(), cpu.pixels.len());
        let (a, b) = (
            blocks(&cpu, size.0 as usize, 8),
            blocks(&on_gpu, size.0 as usize, 8),
        );
        // A real picture, with light and shade in it, not a blank one.
        let brightness = |block: &[f32; 3]| block.iter().sum::<f32>() / 3.0;
        let (darkest, brightest) = a
            .iter()
            .map(brightness)
            .fold((f32::MAX, 0.0f32), |(low, high), b| {
                (low.min(b), high.max(b))
            });
        assert!(brightest - darkest > 40.0, "{darkest} to {brightest}");
        let mut worst: f32 = 0.0;
        for (a, b) in a.iter().zip(&b) {
            for channel in 0..3 {
                worst = worst.max((a[channel] - b[channel]).abs());
            }
        }
        eprintln!("squares from {darkest:.0} to {brightest:.0} differ by up to {worst:.2} of 255");
        assert!(worst < 5.0, "squares differ by up to {worst} of 255");
    }

    #[test]
    fn the_gpu_draws_a_lit_box_on_a_floor_as_the_cpu_does() {
        let scene = || {
            let quad = |corners: [Vec3; 4], normal: Vec3, material: u32| {
                [[0, 1, 2], [0, 2, 3]].map(|[a, b, c]| Triangle {
                    corners: [corners[a], corners[b], corners[c]],
                    normals: [normal; 3],
                    material,
                })
            };
            let mut triangles = Vec::new();
            let v = Vec3::new;
            triangles.extend(quad(
                [
                    v(-5.0, 0.0, -5.0),
                    v(5.0, 0.0, -5.0),
                    v(5.0, 0.0, 5.0),
                    v(-5.0, 0.0, 5.0),
                ],
                Vec3::Y,
                0,
            ));
            // A box's top and front, glossy.
            triangles.extend(quad(
                [
                    v(-0.3, 0.4, -0.3),
                    v(0.3, 0.4, -0.3),
                    v(0.3, 0.4, 0.3),
                    v(-0.3, 0.4, 0.3),
                ],
                Vec3::Y,
                1,
            ));
            triangles.extend(quad(
                [
                    v(-0.3, 0.0, 0.3),
                    v(0.3, 0.0, 0.3),
                    v(0.3, 0.4, 0.3),
                    v(-0.3, 0.4, 0.3),
                ],
                Vec3::Z,
                1,
            ));
            Scene::new(
                triangles,
                vec![
                    Material::Matte(Vec3::splat(0.7)),
                    Material::Finished {
                        color: Vec3::new(0.6, 0.2, 0.1),
                        roughness: 0.3,
                    },
                ],
                vec![Lamp {
                    center: v(-1.0, 2.0, 1.0),
                    u: v(0.4, 0.0, 0.4),
                    v: v(0.2, -0.3, -0.2),
                    radiance: Vec3::splat(8.0),
                }],
                Environment {
                    zenith: Vec3::splat(0.3),
                    horizon: Vec3::splat(0.15),
                    ground: Vec3::splat(0.05),
                    panels: 0.7,
                },
                Camera {
                    position: v(0.0, 1.2, 2.2),
                    forward: v(0.0, -0.45, -0.89).normalize(),
                    right: Vec3::X,
                    up: Vec3::X.cross(v(0.0, -0.45, -0.89).normalize()),
                    tan_half_height: 0.4,
                    aperture: 0.0,
                    focus: 2.4,
                },
                1.0,
            )
        };
        same_both_ways(scene, (96, 64), 256);
    }

    #[test]
    fn the_gpu_draws_wood_in_the_studio_as_the_cpu_does() {
        let board = Board::new(
            Length::from_inches(12),
            Length::from_inch_fraction(7, 2),
            Length::from_inch_fraction(3, 4),
        );
        let setup = Setup::default();
        let scene = || {
            let mut shoot = Shoot::new(&Assembly::of(board.clone()));
            shoot.frame(&setup, 1.5);
            shoot.scene(&setup)
        };
        same_both_ways(scene, (96, 64), 128);
    }

    #[test]
    fn a_gpu_render_can_be_stopped_partway() {
        let board = Board::new(
            Length::from_inches(12),
            Length::from_inch_fraction(7, 2),
            Length::from_inch_fraction(3, 4),
        );
        let setup = Setup::default();
        let mut shoot = Shoot::new(&Assembly::of(board));
        shoot.frame(&setup, 1.5);
        let render = GpuRender::start(for_tests(), &shoot.scene(&setup), 320, 200, 100_000);
        while render.samples() == 0 {
            thread::sleep(Duration::from_millis(5));
        }
        let picture = render.stop_and_finish();
        assert_eq!(picture.pixels.len(), 320 * 200 * 4);
    }
}
