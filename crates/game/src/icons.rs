//! The icons on the buttons, painted in code at startup like every other texture: white shapes
//! on clear, which the button tints. Each is a few strokes and filled shapes in a unit square,
//! `x` right and `y` down, drawn with soft edges from their distance to each pixel. Pictures of
//! wood, for the HUD to show what's in hand or moving to and from the inventory, are painted the
//! same way, in pine with each board outlined.

use std::collections::HashMap;

use bevy::{
    asset::RenderAssetUsages,
    image::{ImageFilterMode, ImageSampler, ImageSamplerDescriptor},
    prelude::*,
    render::render_resource::{Extent3d, TextureDimension, TextureFormat},
};

use crate::{controls::Arrow, hand::Workpiece, textures::with_mipmaps};

/// How many pixels across an icon is painted, before it's scaled down to fit its button.
const SIZE: u32 = 128;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Icon {
    TurnLeft,
    TurnRight,
    Flip,
    StandUp,
    /// A beveled block coming down to rest on its slope.
    Rest,
    SwitchGuide,
    Push,
    BringBack,
    Lower,
    Fence,
    Blade,
    /// The blade leaning from square.
    Tilt,
    Stop,
    Miter,
    Depth,
    Glue,
    Brads,
    Screws,
    Fasten,
    LetGo,
    TakeApart,
    PickUp,
    Plans,
    Shop,
    TableSaw,
    MiterSaw,
    Bench,
    ZoomIn,
    ZoomOut,
    Help,
    /// A small step a setting moves, or a big one, the way an arrow points.
    Step(Arrow, bool),
    /// A solid arrowhead, for sliding a part across a joint or panning.
    Point(Arrow),
    Inventory,
    /// Two sheets, one over the other: a copy.
    Copy,
    /// A floppy disk: save it as a file.
    Save,
    /// Into the inventory, out of it to where you are, cut again, and throw away.
    Store,
    Place,
    Again,
    Scrap,
    /// A camera: the studio, and its lens.
    Camera,
    /// A lens's iris: render.
    Shutter,
    /// The studio's lights: a soft box with rays, a big soft box, and a tall strip.
    Key,
    Fill,
    Rim,
    /// A light swinging around the piece, rising over it, and getting brighter.
    Around,
    Raise,
    Bright,
    /// A seamless backdrop sweeping up behind a block.
    Backdrop,
    /// Sparkles: how fine a render.
    Quality,
}

impl Icon {
    const ALL: [Icon; 47] = [
        Icon::TurnLeft,
        Icon::TurnRight,
        Icon::Flip,
        Icon::StandUp,
        Icon::Rest,
        Icon::Copy,
        Icon::Save,
        Icon::SwitchGuide,
        Icon::Push,
        Icon::BringBack,
        Icon::Lower,
        Icon::Fence,
        Icon::Blade,
        Icon::Tilt,
        Icon::Stop,
        Icon::Miter,
        Icon::Depth,
        Icon::Glue,
        Icon::Brads,
        Icon::Screws,
        Icon::Fasten,
        Icon::LetGo,
        Icon::TakeApart,
        Icon::PickUp,
        Icon::Plans,
        Icon::Shop,
        Icon::TableSaw,
        Icon::MiterSaw,
        Icon::Bench,
        Icon::ZoomIn,
        Icon::ZoomOut,
        Icon::Help,
        Icon::Inventory,
        Icon::Store,
        Icon::Place,
        Icon::Again,
        Icon::Scrap,
        Icon::Camera,
        Icon::Shutter,
        Icon::Key,
        Icon::Fill,
        Icon::Rim,
        Icon::Around,
        Icon::Raise,
        Icon::Bright,
        Icon::Backdrop,
        Icon::Quality,
    ];

    /// Every icon there is.
    fn every() -> Vec<Icon> {
        let arrows = [Arrow::Left, Arrow::Right, Arrow::Up, Arrow::Down];
        let mut every: Vec<Icon> = Icon::ALL.to_vec();
        for arrow in arrows {
            every.extend([
                Icon::Step(arrow, false),
                Icon::Step(arrow, true),
                Icon::Point(arrow),
            ]);
        }
        every
    }

    /// What it's painted with.
    fn shapes(self) -> Vec<Shape> {
        use Shape::*;
        let p = |x: f32, y: f32| Vec2::new(x, y);
        match self {
            Icon::TurnLeft => arrow_arc(p(0.5, 0.56), 0.28, 200.0, -290.0, 0.11),
            Icon::TurnRight => mirrored(Icon::TurnLeft.shapes()),
            Icon::Flip => {
                let mut shapes = vec![Rounded(p(0.1, 0.7), p(0.9, 0.84), 0.04)];
                shapes.extend(arrow_arc(p(0.5, 0.6), 0.3, -175.0, 150.0, 0.1));
                shapes
            }
            Icon::StandUp => {
                let mut shapes = vec![
                    Rounded(p(0.58, 0.16), p(0.72, 0.84), 0.04),
                    Line(vec![p(0.12, 0.9), p(0.88, 0.9)], 0.07),
                ];
                shapes.extend(arrow_arc(p(0.6, 0.84), 0.4, 182.0, 62.0, 0.09));
                shapes
            }
            Icon::Save => vec![Cut(
                Box::new(Cut(
                    Box::new(Rounded(p(0.14, 0.12), p(0.86, 0.88), 0.1)),
                    Box::new(Rounded(p(0.32, 0.04), p(0.64, 0.34), 0.03)),
                )),
                Box::new(Rounded(p(0.28, 0.54), p(0.72, 0.8), 0.04)),
            )],
            Icon::Copy => vec![
                Line(
                    vec![
                        p(0.34, 0.24),
                        p(0.34, 0.1),
                        p(0.9, 0.1),
                        p(0.9, 0.66),
                        p(0.76, 0.66),
                    ],
                    0.08,
                ),
                Rounded(p(0.1, 0.34), p(0.66, 0.9), 0.07),
            ],
            Icon::Rest => {
                let mut shapes = vec![
                    Fill(vec![
                        p(0.18, 0.84),
                        p(0.82, 0.84),
                        p(0.64, 0.5),
                        p(0.36, 0.5),
                    ]),
                    Line(vec![p(0.08, 0.92), p(0.92, 0.92)], 0.06),
                ];
                shapes.extend(arrow_line(p(0.5, 0.06), p(0.5, 0.4), 0.09));
                shapes
            }
            Icon::SwitchGuide => {
                let mut shapes = arrow_line(p(0.16, 0.34), p(0.84, 0.34), 0.1);
                shapes.extend(arrow_line(p(0.84, 0.66), p(0.16, 0.66), 0.1));
                shapes
            }
            Icon::Push => {
                let mut shapes = arrow_line(p(0.5, 0.88), p(0.5, 0.14), 0.13);
                shapes.push(Line(vec![p(0.2, 0.62), p(0.8, 0.62)], 0.07));
                shapes
            }
            Icon::BringBack => {
                let mut shapes = vec![Line(vec![p(0.7, 0.86), p(0.7, 0.42)], 0.11)];
                shapes.push(Line(arc(p(0.5, 0.42), 0.2, 0.0, -180.0), 0.11));
                shapes.extend(arrow_line(p(0.3, 0.42), p(0.3, 0.82), 0.11));
                shapes
            }
            Icon::Lower => {
                let mut shapes = vec![Cut(
                    Box::new(Fill(teeth(p(0.5, 0.4), 0.29, 0.23, 14))),
                    Box::new(Disc(p(0.5, 0.4), 0.08)),
                )];
                shapes.push(Fill(vec![p(0.34, 0.76), p(0.66, 0.76), p(0.5, 0.94)]));
                shapes
            }
            Icon::Fence => {
                let mut shapes = vec![Rounded(p(0.66, 0.12), p(0.8, 0.88), 0.03)];
                shapes.extend(arrow_line(p(0.42, 0.5), p(0.12, 0.5), 0.09));
                shapes.extend(arrow_line(p(0.3, 0.5), p(0.58, 0.5), 0.09));
                shapes
            }
            Icon::Blade => vec![
                Cut(
                    Box::new(Fill(teeth(p(0.5, 0.66), 0.38, 0.31, 16))),
                    Box::new(Rounded(p(0.0, 0.66), p(1.0, 1.0), 0.0)),
                ),
                Rounded(p(0.06, 0.66), p(0.94, 0.78), 0.03),
            ],
            Icon::Tilt => {
                // The blade leans left about where it meets the table, past a dashed upright.
                let pivot = p(0.5, 0.7);
                let (sin, cos) = (-30f32).to_radians().sin_cos();
                let lean = |q: Vec2| {
                    let c = q - pivot;
                    pivot + Vec2::new(c.x * cos - c.y * sin, c.x * sin + c.y * cos)
                };
                let below = || Box::new(Rounded(p(-0.5, 0.7), p(1.5, 1.5), 0.0));
                let blade = Cut(Box::new(Fill(teeth(pivot, 0.4, 0.33, 16))), below());
                vec![
                    Cut(Box::new(blade.moved(&lean)), below()),
                    Line(vec![p(0.62, 0.1), p(0.62, 0.22)], 0.05),
                    Line(vec![p(0.62, 0.32), p(0.62, 0.44)], 0.05),
                    Rounded(p(0.06, 0.7), p(0.94, 0.82), 0.03),
                ]
            }
            Icon::Stop => {
                let mut shapes = vec![
                    Line(vec![p(0.12, 0.26), p(0.12, 0.74)], 0.08),
                    Line(vec![p(0.88, 0.26), p(0.88, 0.74)], 0.08),
                ];
                shapes.extend(arrow_line(p(0.5, 0.5), p(0.2, 0.5), 0.08));
                shapes.extend(arrow_line(p(0.4, 0.5), p(0.8, 0.5), 0.08));
                shapes
            }
            Icon::Miter => vec![
                Line(vec![p(0.86, 0.8), p(0.16, 0.8), p(0.64, 0.24)], 0.09),
                Line(arc(p(0.16, 0.8), 0.36, -49.0, 49.0), 0.06),
            ],
            Icon::Depth => {
                let mut shapes = arrow_line(p(0.5, 0.1), p(0.5, 0.62), 0.11);
                shapes.push(Rounded(p(0.16, 0.72), p(0.84, 0.84), 0.03));
                shapes
            }
            Icon::Glue => vec![
                Rounded(p(0.3, 0.44), p(0.7, 0.9), 0.07),
                Fill(vec![p(0.3, 0.46), p(0.7, 0.46), p(0.57, 0.3), p(0.43, 0.3)]),
                Rounded(p(0.46, 0.08), p(0.54, 0.32), 0.02),
            ],
            Icon::Brads => tilted(vec![
                Fill(vec![p(0.3, 0.12), p(0.7, 0.12), p(0.7, 0.2), p(0.3, 0.2)]),
                Fill(vec![
                    p(0.45, 0.19),
                    p(0.55, 0.19),
                    p(0.55, 0.74),
                    p(0.45, 0.74),
                ]),
                Fill(vec![p(0.45, 0.73), p(0.55, 0.73), p(0.5, 0.92)]),
            ]),
            Icon::Screws => {
                let mut shapes: Vec<Shape> = vec![
                    Cut(
                        Box::new(Rounded(p(0.26, 0.1), p(0.74, 0.26), 0.05)),
                        Box::new(Rounded(p(0.46, 0.08), p(0.54, 0.2), 0.0)),
                    ),
                    Fill(vec![p(0.4, 0.25), p(0.6, 0.25), p(0.5, 0.92)]),
                ];
                for y in [0.36, 0.5, 0.64] {
                    shapes.push(Line(vec![p(0.34, y + 0.06), p(0.66, y - 0.04)], 0.06));
                }
                tilted(shapes)
            }
            Icon::Fasten => vec![Line(
                vec![p(0.18, 0.52), p(0.41, 0.75), p(0.84, 0.27)],
                0.14,
            )],
            Icon::LetGo => vec![
                Line(vec![p(0.24, 0.24), p(0.76, 0.76)], 0.13),
                Line(vec![p(0.76, 0.24), p(0.24, 0.76)], 0.13),
            ],
            Icon::TakeApart => {
                let mut shapes = vec![Line(vec![p(0.82, 0.8), p(0.82, 0.56)], 0.11)];
                shapes.extend(arrow_arc(p(0.54, 0.56), 0.28, 0.0, -150.0, 0.11));
                shapes
            }
            Icon::PickUp => {
                let mut shapes = vec![Line(
                    vec![p(0.16, 0.56), p(0.16, 0.86), p(0.84, 0.86), p(0.84, 0.56)],
                    0.09,
                )];
                shapes.extend(arrow_line(p(0.5, 0.72), p(0.5, 0.1), 0.11));
                shapes
            }
            Icon::Plans => {
                let mut shapes = vec![Cut(
                    Box::new(Rounded(p(0.2, 0.08), p(0.8, 0.92), 0.08)),
                    Box::new(Rounded(p(0.29, 0.17), p(0.71, 0.83), 0.03)),
                )];
                for y in [0.34, 0.5, 0.66] {
                    shapes.push(Line(vec![p(0.38, y), p(0.62, y)], 0.07));
                }
                shapes
            }
            Icon::Shop => vec![
                Fill(vec![p(0.08, 0.52), p(0.5, 0.12), p(0.92, 0.52)]),
                Cut(
                    Box::new(Rounded(p(0.2, 0.46), p(0.8, 0.88), 0.03)),
                    Box::new(Rounded(p(0.42, 0.62), p(0.58, 1.0), 0.02)),
                ),
            ],
            Icon::TableSaw => vec![
                Cut(
                    Box::new(Fill(teeth(p(0.5, 0.5), 0.3, 0.24, 14))),
                    Box::new(Rounded(p(0.0, 0.5), p(1.0, 1.0), 0.0)),
                ),
                Rounded(p(0.06, 0.5), p(0.94, 0.6), 0.03),
                Line(vec![p(0.18, 0.6), p(0.14, 0.92)], 0.08),
                Line(vec![p(0.82, 0.6), p(0.86, 0.92)], 0.08),
            ],
            Icon::MiterSaw => vec![
                Rounded(p(0.08, 0.8), p(0.92, 0.9), 0.03),
                Line(vec![p(0.84, 0.76), p(0.84, 0.5), p(0.5, 0.36)], 0.08),
                Cut(
                    Box::new(Fill(teeth(p(0.42, 0.44), 0.27, 0.22, 14))),
                    Box::new(Disc(p(0.42, 0.44), 0.07)),
                ),
            ],
            Icon::Bench => vec![
                Rounded(p(0.06, 0.34), p(0.94, 0.48), 0.03),
                Rounded(p(0.14, 0.46), p(0.24, 0.88), 0.02),
                Rounded(p(0.76, 0.46), p(0.86, 0.88), 0.02),
                Line(vec![p(0.2, 0.72), p(0.8, 0.72)], 0.06),
            ],
            Icon::ZoomIn | Icon::ZoomOut => {
                let mut shapes = vec![
                    Ring(p(0.42, 0.42), 0.25, 0.09),
                    Line(vec![p(0.61, 0.61), p(0.86, 0.86)], 0.13),
                    Line(vec![p(0.31, 0.42), p(0.53, 0.42)], 0.07),
                ];
                if self == Icon::ZoomIn {
                    shapes.push(Line(vec![p(0.42, 0.31), p(0.42, 0.53)], 0.07));
                }
                shapes
            }
            Icon::Camera => vec![
                Cut(
                    Box::new(Rounded(p(0.08, 0.3), p(0.92, 0.84), 0.08)),
                    Box::new(Disc(p(0.5, 0.57), 0.2)),
                ),
                Fill(vec![
                    p(0.32, 0.31),
                    p(0.4, 0.17),
                    p(0.6, 0.17),
                    p(0.68, 0.31),
                ]),
                Disc(p(0.5, 0.57), 0.11),
            ],
            Icon::Shutter => {
                let center = p(0.5, 0.5);
                let mut shapes = vec![Ring(center, 0.36, 0.09)];
                for k in 0..6 {
                    let angle = (k as f32 * 60.0).to_radians();
                    let corner = center + 0.13 * Vec2::from_angle(angle);
                    let out = center + 0.36 * Vec2::from_angle(angle + 1.1);
                    shapes.push(Line(vec![corner, out], 0.06));
                }
                shapes
            }
            Icon::Key => {
                let mut shapes = vec![Rounded(p(0.32, 0.32), p(0.68, 0.68), 0.05)];
                shapes.extend(rays(p(0.5, 0.5), 0.3, 0.44, 8));
                shapes
            }
            Icon::Fill => {
                let mut shapes = vec![Cut(
                    Box::new(Rounded(p(0.1, 0.18), p(0.9, 0.82), 0.07)),
                    Box::new(Rounded(p(0.2, 0.28), p(0.8, 0.72), 0.03)),
                )];
                for x in [0.35, 0.5, 0.65] {
                    shapes.push(Line(vec![p(x, 0.36), p(x, 0.64)], 0.05));
                }
                shapes
            }
            Icon::Rim => vec![
                Rounded(p(0.4, 0.1), p(0.6, 0.9), 0.05),
                Line(vec![p(0.18, 0.3), p(0.28, 0.3)], 0.07),
                Line(vec![p(0.14, 0.5), p(0.28, 0.5)], 0.07),
                Line(vec![p(0.18, 0.7), p(0.28, 0.7)], 0.07),
                Line(vec![p(0.72, 0.3), p(0.82, 0.3)], 0.07),
                Line(vec![p(0.72, 0.5), p(0.86, 0.5)], 0.07),
                Line(vec![p(0.72, 0.7), p(0.82, 0.7)], 0.07),
            ],
            Icon::Around => {
                let mut shapes = vec![Rounded(p(0.38, 0.38), p(0.62, 0.62), 0.03)];
                shapes.extend(arrow_arc(p(0.5, 0.5), 0.34, 120.0, 270.0, 0.08));
                shapes
            }
            Icon::Raise => {
                let mut shapes = vec![
                    Line(vec![p(0.08, 0.84), p(0.92, 0.84)], 0.07),
                    Rounded(p(0.16, 0.7), p(0.36, 0.84), 0.02),
                ];
                shapes.extend(arrow_arc(p(0.26, 0.84), 0.58, 0.0, -75.0, 0.08));
                shapes
            }
            Icon::Bright => {
                let mut shapes = vec![Disc(p(0.5, 0.5), 0.17)];
                shapes.extend(rays(p(0.5, 0.5), 0.27, 0.42, 8));
                shapes
            }
            Icon::Backdrop => vec![
                Line(
                    [
                        vec![p(0.08, 0.82)],
                        arc(p(0.52, 0.5), 0.32, 90.0, 0.0),
                        vec![p(0.84, 0.1)],
                    ]
                    .concat(),
                    0.08,
                ),
                Rounded(p(0.2, 0.6), p(0.44, 0.78), 0.03),
            ],
            Icon::Quality => vec![
                Fill(sparkle(p(0.42, 0.56), 0.34)),
                Fill(sparkle(p(0.76, 0.22), 0.16)),
            ],
            Icon::Help => vec![
                Line(
                    [arc(p(0.5, 0.34), 0.18, 200.0, 450.0), vec![p(0.5, 0.64)]].concat(),
                    0.11,
                ),
                Disc(p(0.5, 0.82), 0.075),
            ],
            Icon::Step(arrow, big) => {
                let chevron = |x: f32| Line(vec![p(x, 0.24), p(x + 0.24, 0.5), p(x, 0.76)], 0.12);
                let shapes = if big {
                    vec![chevron(0.22), chevron(0.48)]
                } else {
                    vec![chevron(0.36)]
                };
                pointing(shapes, arrow)
            }
            Icon::Point(arrow) => pointing(
                vec![Fill(vec![p(0.3, 0.2), p(0.8, 0.5), p(0.3, 0.8)])],
                arrow,
            ),
            Icon::Inventory => vec![
                Rounded(p(0.1, 0.18), p(0.9, 0.36), 0.04),
                Cut(
                    Box::new(Rounded(p(0.16, 0.4), p(0.84, 0.88), 0.06)),
                    Box::new(Rounded(p(0.38, 0.5), p(0.62, 0.59), 0.04)),
                ),
            ],
            Icon::Store => {
                let mut shapes = vec![Line(
                    vec![p(0.16, 0.56), p(0.16, 0.86), p(0.84, 0.86), p(0.84, 0.56)],
                    0.09,
                )];
                shapes.extend(arrow_line(p(0.5, 0.1), p(0.5, 0.72), 0.11));
                shapes
            }
            Icon::Place => {
                let mut shapes = vec![Rounded(p(0.1, 0.16), p(0.22, 0.84), 0.04)];
                shapes.extend(arrow_line(p(0.88, 0.5), p(0.3, 0.5), 0.12));
                shapes
            }
            Icon::Again => {
                let mut shapes = arrow_arc(p(0.5, 0.5), 0.3, -160.0, 140.0, 0.1);
                shapes.extend(arrow_arc(p(0.5, 0.5), 0.3, 20.0, 140.0, 0.1));
                shapes
            }
            Icon::Scrap => vec![
                Line(vec![p(0.16, 0.25), p(0.84, 0.25)], 0.08),
                Line(vec![p(0.4, 0.17), p(0.6, 0.17)], 0.08),
                Cut(
                    Box::new(Cut(
                        Box::new(Fill(vec![
                            p(0.24, 0.33),
                            p(0.76, 0.33),
                            p(0.7, 0.9),
                            p(0.3, 0.9),
                        ])),
                        Box::new(Rounded(p(0.37, 0.43), p(0.44, 0.8), 0.03)),
                    )),
                    Box::new(Rounded(p(0.56, 0.43), p(0.63, 0.8), 0.03)),
                ),
            ],
        }
    }
}

/// Something painted: a stroke along points, a filled shape, a dot, a ring, a box with rounded
/// corners, or one shape with another cut out of it. Sizes are fractions of the icon.
#[derive(Clone, Debug)]
enum Shape {
    Line(Vec<Vec2>, f32),
    Fill(Vec<Vec2>),
    Disc(Vec2, f32),
    Ring(Vec2, f32, f32),
    Rounded(Vec2, Vec2, f32),
    Cut(Box<Shape>, Box<Shape>),
}

impl Shape {
    /// How far `point` is outside the shape: negative inside.
    fn distance(&self, point: Vec2) -> f32 {
        match self {
            Shape::Line(points, width) => {
                points
                    .windows(2)
                    .map(|pair| to_segment(point, pair[0], pair[1]))
                    .fold(f32::MAX, f32::min)
                    - width / 2.0
            }
            Shape::Fill(corners) => {
                let edge = (0..corners.len())
                    .map(|i| to_segment(point, corners[i], corners[(i + 1) % corners.len()]))
                    .fold(f32::MAX, f32::min);
                if inside(point, corners) { -edge } else { edge }
            }
            Shape::Disc(center, radius) => point.distance(*center) - radius,
            Shape::Ring(center, radius, width) => {
                (point.distance(*center) - radius).abs() - width / 2.0
            }
            Shape::Rounded(low, high, radius) => {
                let middle = (*low + *high) / 2.0;
                let half = (*high - *low) / 2.0 - Vec2::splat(*radius);
                let q = (point - middle).abs() - half;
                q.max(Vec2::ZERO).length() + q.x.max(q.y).min(0.0) - radius
            }
            Shape::Cut(shape, hole) => shape.distance(point).max(-hole.distance(point)),
        }
    }

    /// The shape with every point moved by `to`.
    fn moved(&self, to: &dyn Fn(Vec2) -> Vec2) -> Shape {
        match self {
            Shape::Line(points, width) => {
                Shape::Line(points.iter().map(|&p| to(p)).collect(), *width)
            }
            Shape::Fill(corners) => Shape::Fill(corners.iter().map(|&p| to(p)).collect()),
            Shape::Disc(center, radius) => Shape::Disc(to(*center), *radius),
            Shape::Ring(center, radius, width) => Shape::Ring(to(*center), *radius, *width),
            Shape::Rounded(low, high, radius) => {
                let (a, b) = (to(*low), to(*high));
                Shape::Rounded(a.min(b), a.max(b), *radius)
            }
            Shape::Cut(shape, hole) => {
                Shape::Cut(Box::new(shape.moved(to)), Box::new(hole.moved(to)))
            }
        }
    }
}

fn to_segment(point: Vec2, a: Vec2, b: Vec2) -> f32 {
    let along = b - a;
    let t = ((point - a).dot(along) / along.length_squared().max(f32::EPSILON)).clamp(0.0, 1.0);
    point.distance(a + along * t)
}

/// Whether `point` is inside the polygon `corners`, by how many times its edges wind around it.
fn inside(point: Vec2, corners: &[Vec2]) -> bool {
    let mut crossings = 0;
    for i in 0..corners.len() {
        let (a, b) = (corners[i], corners[(i + 1) % corners.len()]);
        if (a.y > point.y) != (b.y > point.y) {
            let x = a.x + (point.y - a.y) / (b.y - a.y) * (b.x - a.x);
            if point.x < x {
                crossings += 1;
            }
        }
    }
    crossings % 2 == 1
}

/// Points along a circle about `center`, from `from` degrees through `to`, clockwise on the
/// screen as the angle grows (`y` is down).
fn arc(center: Vec2, radius: f32, from: f32, to: f32) -> Vec<Vec2> {
    const STEPS: usize = 48;
    (0..=STEPS)
        .map(|step| {
            let angle = (from + (to - from) * step as f32 / STEPS as f32).to_radians();
            center + radius * Vec2::new(angle.cos(), angle.sin())
        })
        .collect()
}

/// An arrowhead at `tip`, pointing `toward`, for a stroke `width` wide.
fn head(tip: Vec2, toward: Vec2, width: f32) -> Shape {
    let toward = toward.normalize();
    let side = Vec2::new(-toward.y, toward.x);
    let (long, wide) = (width * 2.5, width * 2.1);
    Shape::Fill(vec![
        tip + toward * width * 0.4,
        tip - toward * long + side * wide,
        tip - toward * long - side * wide,
    ])
}

/// A straight arrow from `from` to `to`.
fn arrow_line(from: Vec2, to: Vec2, width: f32) -> Vec<Shape> {
    let toward = (to - from).normalize();
    vec![
        Shape::Line(vec![from, to - toward * width * 1.6], width),
        head(to, toward, width),
    ]
}

/// A curved arrow around `center`, from `from` degrees `sweep` degrees on.
fn arrow_arc(center: Vec2, radius: f32, from: f32, sweep: f32, width: f32) -> Vec<Shape> {
    // Stop the stroke short of the tip, so the head's point stays sharp.
    let short = (width * 1.6 / radius).to_degrees() * sweep.signum();
    let to = from + sweep;
    let tip = center + radius * Vec2::from_angle(to.to_radians());
    let tangent = Vec2::from_angle((to + 90.0 * sweep.signum()).to_radians());
    vec![
        Shape::Line(arc(center, radius, from, to - short), width),
        head(tip, tangent, width),
    ]
}

/// Short strokes radiating from `center`, from `inner` to `outer`, `count` of them.
fn rays(center: Vec2, inner: f32, outer: f32, count: usize) -> Vec<Shape> {
    (0..count)
        .map(|k| {
            let toward = Vec2::from_angle(std::f32::consts::TAU * k as f32 / count as f32);
            Shape::Line(vec![center + toward * inner, center + toward * outer], 0.07)
        })
        .collect()
}

/// A four-pointed star about `center`, `radius` to its points.
fn sparkle(center: Vec2, radius: f32) -> Vec<Vec2> {
    (0..8)
        .map(|k| {
            let angle = std::f32::consts::PI * k as f32 / 4.0 - std::f32::consts::FRAC_PI_2;
            let reach = if k % 2 == 0 { radius } else { radius * 0.28 };
            center + reach * Vec2::from_angle(angle)
        })
        .collect()
}

/// The teeth of a saw blade: a star of `count` points about `center`.
fn teeth(center: Vec2, outer: f32, inner: f32, count: usize) -> Vec<Vec2> {
    (0..count * 2)
        .map(|k| {
            let angle = std::f32::consts::PI * k as f32 / count as f32;
            let radius = if k % 2 == 0 { outer } else { inner };
            center + radius * Vec2::from_angle(angle)
        })
        .collect()
}

/// Shapes flipped left for right.
fn mirrored(shapes: Vec<Shape>) -> Vec<Shape> {
    shapes
        .iter()
        .map(|shape| shape.moved(&|p| Vec2::new(1.0 - p.x, p.y)))
        .collect()
}

/// Shapes leaned over to the right, the way a nail or screw is drawn going in.
fn tilted(shapes: Vec<Shape>) -> Vec<Shape> {
    let (sin, cos) = 30f32.to_radians().sin_cos();
    let lean = |p: Vec2| {
        let c = p - Vec2::splat(0.5);
        Vec2::splat(0.5) + Vec2::new(c.x * cos - c.y * sin, c.x * sin + c.y * cos)
    };
    shapes.iter().map(|shape| shape.moved(&lean)).collect()
}

/// Shapes drawn pointing right, turned to point `arrow`'s way.
fn pointing(shapes: Vec<Shape>, arrow: Arrow) -> Vec<Shape> {
    let turn = |p: Vec2| -> Vec2 {
        let c = p - Vec2::splat(0.5);
        Vec2::splat(0.5)
            + match arrow {
                Arrow::Right => c,
                Arrow::Left => -c,
                Arrow::Down => Vec2::new(-c.y, c.x),
                Arrow::Up => Vec2::new(c.y, -c.x),
            }
    };
    shapes.iter().map(|shape| shape.moved(&turn)).collect()
}

/// An icon's image: white, as opaque as the shapes cover each pixel, with mipmaps so it stays
/// smooth at whatever size its button draws it.
fn paint(shapes: &[Shape]) -> Image {
    let mut pixels = Vec::with_capacity((SIZE * SIZE * 4) as usize);
    let pixel = 1.0 / SIZE as f32;
    for y in 0..SIZE {
        for x in 0..SIZE {
            let point = Vec2::new(x as f32 + 0.5, y as f32 + 0.5) * pixel;
            let distance = shapes
                .iter()
                .map(|shape| shape.distance(point))
                .fold(f32::MAX, f32::min);
            let cover = (0.5 - distance / pixel).clamp(0.0, 1.0);
            pixels.extend_from_slice(&[255, 255, 255, (cover * 255.0).round() as u8]);
        }
    }
    image(pixels)
}

/// An icon-sized image of `pixels`, RGBA, with mipmaps.
fn image(pixels: Vec<u8>) -> Image {
    let (data, levels) = with_mipmaps(SIZE, SIZE, pixels);
    let mut image = Image::new_fill(
        Extent3d {
            width: SIZE,
            height: SIZE,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        &[0, 0, 0, 0],
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::RENDER_WORLD,
    );
    image.data = Some(data);
    image.texture_descriptor.mip_level_count = levels;
    image.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
        mag_filter: ImageFilterMode::Linear,
        min_filter: ImageFilterMode::Linear,
        mipmap_filter: ImageFilterMode::Linear,
        ..default()
    });
    image
}

/// The colors of a picture of wood: pine, outlined in a darker brown.
const PINE: [f32; 3] = [0.96, 0.82, 0.58];
const GRAIN: [f32; 3] = [0.56, 0.36, 0.17];

/// A picture of `work` from above, as it lies: each board's outline, filled in pine and edged in
/// brown, fitted into a square. A board too thin to see at that size is drawn thick enough to.
pub fn picture(work: &Workpiece) -> Image {
    // Each board's outline from above, as polygons in meters along x and y.
    let boards: Vec<Vec<Vec<Vec2>>> = match work {
        Workpiece::Board(board) => vec![
            board
                .face()
                .iter()
                .map(|region| {
                    region
                        .footprint(board.thickness())
                        .into_iter()
                        .map(|(l, w)| Vec2::new(l.as_meters_f32(), w.as_meters_f32()))
                        .collect()
                })
                .collect(),
        ],
        Workpiece::GlueUp(assembly) => assembly
            .members()
            .iter()
            .map(|member| {
                member
                    .board
                    .face()
                    .iter()
                    .map(|region| {
                        // The region through the board's thickness, seen from above.
                        let corners: Vec<Vec2> = region
                            .corners_through(member.board.thickness())
                            .into_iter()
                            .map(|[l, w, t]| member.point(l, w, t))
                            .map(|[x, y, _]| Vec2::new(x.as_meters_f32(), y.as_meters_f32()))
                            .collect();
                        hull(corners)
                    })
                    .collect()
            })
            .collect(),
    };
    let (low, high) = boards
        .iter()
        .flatten()
        .flatten()
        .fold((Vec2::MAX, Vec2::MIN), |(low, high), &point| {
            (low.min(point), high.max(point))
        });
    // Fit the longer side across the square; the shorter, at least a tenth of it.
    const MARGIN: f32 = 0.1;
    let size = (high - low).max(Vec2::splat(1e-6));
    let scale = (1.0 - 2.0 * MARGIN) / size.max_element();
    let shown = (size * scale).max(Vec2::splat(0.1));
    let stretch = shown / (size * scale);
    let at = |point: Vec2| {
        let fitted = (point - low) * scale * stretch;
        // `y` down on the picture, centered in the square.
        let offset = (Vec2::ONE - shown) / 2.0;
        Vec2::new(offset.x + fitted.x, 1.0 - offset.y - fitted.y)
    };
    let boards: Vec<Vec<Shape>> = boards
        .iter()
        .map(|regions| {
            regions
                .iter()
                .map(|corners| Shape::Fill(corners.iter().map(|&p| at(p)).collect()))
                .collect()
        })
        .collect();
    let pixel = 1.0 / SIZE as f32;
    let edge = 2.5 * pixel;
    let mut pixels = Vec::with_capacity((SIZE * SIZE * 4) as usize);
    for y in 0..SIZE {
        for x in 0..SIZE {
            let point = Vec2::new(x as f32 + 0.5, y as f32 + 0.5) * pixel;
            // How far outside each board, and outside them all.
            let each: Vec<f32> = boards
                .iter()
                .map(|regions| {
                    regions
                        .iter()
                        .map(|shape| shape.distance(point))
                        .fold(f32::MAX, f32::min)
                })
                .collect();
            let outside = each.iter().copied().fold(f32::MAX, f32::min);
            let to_edge = each.iter().map(|d| d.abs()).fold(f32::MAX, f32::min);
            let cover = (0.5 - outside / pixel).clamp(0.0, 1.0);
            let inked = (0.5 - (to_edge - edge) / pixel).clamp(0.0, 1.0);
            for (pine, grain) in PINE.into_iter().zip(GRAIN) {
                pixels.push(((pine + (grain - pine) * inked) * 255.0).round() as u8);
            }
            pixels.push((cover * 255.0).round() as u8);
        }
    }
    image(pixels)
}

/// The smallest convex polygon around `points`, counterclockwise.
fn hull(mut points: Vec<Vec2>) -> Vec<Vec2> {
    points.sort_by(|a, b| a.x.total_cmp(&b.x).then(a.y.total_cmp(&b.y)));
    points.dedup();
    if points.len() < 3 {
        return points;
    }
    let turn = |o: Vec2, a: Vec2, b: Vec2| (a - o).perp_dot(b - o);
    let mut lower: Vec<Vec2> = Vec::new();
    for &point in &points {
        while lower.len() >= 2 && turn(lower[lower.len() - 2], lower[lower.len() - 1], point) <= 0.0
        {
            lower.pop();
        }
        lower.push(point);
    }
    let mut upper: Vec<Vec2> = Vec::new();
    for &point in points.iter().rev() {
        while upper.len() >= 2 && turn(upper[upper.len() - 2], upper[upper.len() - 1], point) <= 0.0
        {
            upper.pop();
        }
        upper.push(point);
    }
    lower.pop();
    upper.pop();
    lower.extend(upper);
    lower
}

/// Every icon's image.
#[derive(Resource)]
pub struct Icons(HashMap<Icon, Handle<Image>>);

impl Icons {
    pub fn get(&self, icon: Icon) -> Handle<Image> {
        self.0[&icon].clone()
    }
}

impl FromWorld for Icons {
    fn from_world(world: &mut World) -> Icons {
        let mut images = world.resource_mut::<Assets<Image>>();
        Icons(
            Icon::every()
                .into_iter()
                .map(|icon| (icon, images.add(paint(&icon.shapes()))))
                .collect(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// How much of the icon is painted, from 0 to 1.
    fn coverage(icon: Icon) -> f32 {
        let image = paint(&icon.shapes());
        let data = image.data.unwrap();
        let alpha: u64 = data[..(SIZE * SIZE * 4) as usize]
            .chunks(4)
            .map(|pixel| pixel[3] as u64)
            .sum();
        alpha as f32 / (255.0 * (SIZE * SIZE) as f32)
    }

    #[test]
    fn every_icon_is_painted_and_none_fills_its_square() {
        for icon in Icon::every() {
            let covered = coverage(icon);
            assert!(covered > 0.04 && covered < 0.6, "{icon:?} covers {covered}");
        }
    }

    #[test]
    fn mirrored_and_turned_icons_cover_the_same() {
        assert!((coverage(Icon::TurnLeft) - coverage(Icon::TurnRight)).abs() < 0.002);
        assert!(
            (coverage(Icon::Point(Arrow::Up)) - coverage(Icon::Point(Arrow::Left))).abs() < 0.002
        );
    }

    #[test]
    fn a_board_is_pictured_in_pine_edged_in_brown_and_a_thin_one_still_shows() {
        let board = |width: i64| {
            Workpiece::Board(woodsim::Board::new(
                woodsim::Length::from_inches(24),
                woodsim::Length::from_inch_fraction(width, 4),
                woodsim::Length::from_inch_fraction(3, 4),
            ))
        };
        for width in [1, 14, 96] {
            let image = picture(&board(width));
            let data = image.data.unwrap();
            let pixel = |x: u32, y: u32| {
                let at = ((y * SIZE + x) * 4) as usize;
                [data[at], data[at + 1], data[at + 2], data[at + 3]]
            };
            // Clear in the corner; solid pine in the middle.
            assert_eq!(pixel(0, 0)[3], 0);
            let middle = pixel(SIZE / 2, SIZE / 2);
            assert_eq!(middle[3], 255, "{width}");
            assert_eq!(middle[0], (PINE[0] * 255.0).round() as u8);
        }
    }

    #[test]
    fn a_ring_is_hollow_and_a_disc_is_not() {
        let center = Vec2::splat(0.5);
        assert!(Shape::Ring(center, 0.3, 0.1).distance(center) > 0.0);
        assert!(Shape::Disc(center, 0.3).distance(center) < 0.0);
        assert!(Shape::Fill(vec![Vec2::ZERO, Vec2::X, Vec2::Y]).distance(Vec2::splat(0.2)) < 0.0);
    }
}
