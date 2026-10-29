//! A piece of wood written out as text and read back exactly (`Recipe`): a board or a glue-up,
//! and how a saw was set for it, to the nanometer. Every board is written as where it came
//! from, the new lumber it started as and each saw pass that cut it, with the saw's every
//! setting and the exact slot it cut, so reading it back cuts it again (as `Origin::replay`
//! does) and keeps its history, down to which boards shared a cut. A glue-up adds where each of
//! its boards is and how it's turned, and its joints.
//!
//! One thing to a line, a word and then whole numbers, and `#` starts a comment. Lengths are in
//! nanometers, angles in hundredths of a degree, and turns are their nine whole numbers over
//! 2^40, by column:
//!
//! ```text
//! stock s0 609600000 63500000 19050000
//! pass p0 7 table-saw 254000000 3175000 25400000 25400000 none 0 1500 board 1099511627776 0 0 0 1099511627776 0 0 0 1099511627776 -127000000 736600000
//! sawn s1 s0 p0 0 -16777216 4495441 ... piece 0 0 0 0 measured
//! board s1
//! ```
//!
//! `stock` names new lumber: its length, width, and thickness. `pass` names a saw pass: its
//! number, then the saw (`table-saw`: blade diameter and kerf, fence, height, the miter gauge's
//! stop or `none`, its angle, and the tilt; `miter-saw`: blade diameter and kerf, miter, the
//! furthest it turns, the pivot behind the fence and up, the arm, and the stroke's top and
//! bottom), how the wood lay (`board` or `glue-up`, and the turn), and how far the pass went
//! (the table saw's travel from and to; the miter saw's length from the stop, and how low the
//! blade came). `sawn` names a board a pass cut from another: which board, which pass, the slot
//! (its faces' normal, its near and far faces, the way it traveled, the blade's radius, the
//! path's height, and its start and end), which part it is (`whole`, or `piece` and which),
//! where its corner was on the board it was cut from, and which side of the cut it was on.
//! Then either `board`, the piece itself, or a `member` line for each board of a glue-up (which
//! board, where its corner is, and its turn) and a `joint` line for each joint (each member and
//! its surface, `top`, `bottom`, or `side` and the side's edge, then the fastener and when it
//! was fastened). A `saw` line, the saw and how what's on it lies, as a pass has them, says
//! how it's set up now.

use std::{collections::HashMap, fmt::Write, sync::Arc};

use crate::{
    Angle, Assembly, Blade, Board, Direction, Edge, Fastener, Joint, Kerf, Length, Member,
    MiterSaw, Orientation, Rotation, Surface, TableSaw, Vector,
    history::{Lie, Operation, Origin, Part, Side, Source, Tool},
};

/// Wood: one board, or boards put together.
#[derive(Clone, Debug, PartialEq)]
pub enum Wood {
    Board(Board),
    GlueUp(Assembly),
}

/// Wood, and how a saw's set up for it, if it's on one: the saw and how the wood lies, as a pass
/// has them, and how far the pass has gone.
#[derive(Clone, Debug, PartialEq)]
pub struct Recipe {
    pub wood: Wood,
    pub saw: Option<Tool>,
}

impl Recipe {
    /// The recipe as text, every board's history written once and shared as it's shared.
    pub fn write(&self) -> String {
        let mut writer = Writer::default();
        match &self.wood {
            Wood::Board(board) => {
                let id = writer.origin(board.origin());
                writer.line(format!("board {id}"));
            }
            Wood::GlueUp(glue_up) => {
                for member in glue_up.members() {
                    let id = writer.origin(member.board.origin());
                    let [x, y, z] = member.at.map(nm);
                    writer.line(format!("member {id} {x} {y} {z} {}", turn(member.rotation)));
                }
                for joint in glue_up.joints() {
                    let [(a, sa), (b, sb)] = joint.between;
                    let fastener = match joint.fastener {
                        Fastener::Glue => "glue",
                        Fastener::Brads => "brads",
                        Fastener::Screws => "screws",
                    };
                    writer.line(format!(
                        "joint {a} {} {b} {} {fastener} {}",
                        surface(sa),
                        surface(sb),
                        joint.number
                    ));
                }
            }
        }
        if let Some(tool) = &self.saw {
            writer.line(format!("saw {}", tool_words(tool)));
        }
        writer.lines
    }

    /// The recipe `text` says, or what's wrong with it, by line.
    pub fn read(text: &str) -> Result<Recipe, String> {
        let mut reader = Reader::default();
        for (number, line) in text.lines().enumerate() {
            let line = line.split('#').next().unwrap_or_default().trim();
            if line.is_empty() {
                continue;
            }
            reader
                .line(line)
                .map_err(|why| format!("line {}: {why}: {line}", number + 1))?;
        }
        let wood = match (reader.board, reader.members.is_empty()) {
            (Some(board), true) => Wood::Board(board),
            (None, false) => Wood::GlueUp(Assembly::from_parts(reader.members, reader.joints)),
            _ => return Err("expected one `board` line, or `member` lines".to_string()),
        };
        Ok(Recipe {
            wood,
            saw: reader.saw,
        })
    }
}

fn nm(length: Length) -> i64 {
    length.as_nm()
}

/// A turn's nine whole numbers over 2^40, column by column.
fn turn(rotation: Rotation) -> String {
    rotation
        .raw()
        .iter()
        .flatten()
        .map(|v| v.to_string())
        .collect::<Vec<_>>()
        .join(" ")
}

fn surface(surface: Surface) -> String {
    match surface {
        Surface::Top => "top".to_string(),
        Surface::Bottom => "bottom".to_string(),
        Surface::Side(edge) => format!(
            "side {} {} {} {}",
            edge.out.l, edge.out.w, edge.lean, edge.at
        ),
    }
}

/// A pass's saw, how the wood lay, and how far it went, as words.
fn tool_words(tool: &Tool) -> String {
    let lie = |lie: &Lie| match lie {
        Lie::Board(lie) => format!("board {}", turn(lie.rotation())),
        Lie::GlueUp(rotation) => format!("glue-up {}", turn(*rotation)),
    };
    match tool {
        Tool::TableSaw {
            saw,
            lie: on,
            travel,
        } => format!(
            "table-saw {} {} {} {} {} {} {} {} {} {}",
            nm(saw.blade.diameter),
            nm(saw.blade.kerf),
            nm(saw.fence),
            nm(saw.height),
            saw.miter_gauge
                .map_or("none".to_string(), |stop| nm(stop).to_string()),
            saw.gauge_angle.hundredths(),
            saw.tilt.hundredths(),
            lie(on),
            nm(travel.0),
            nm(travel.1)
        ),
        Tool::MiterSaw {
            saw,
            lie: on,
            length,
            bottom,
        } => format!(
            "miter-saw {} {} {} {} {} {} {} {} {} {} {} {}",
            nm(saw.blade.diameter),
            nm(saw.blade.kerf),
            saw.miter.hundredths(),
            saw.max_miter.hundredths(),
            nm(saw.pivot.0),
            nm(saw.pivot.1),
            nm(saw.arm),
            nm(saw.stroke.0),
            nm(saw.stroke.1),
            lie(on),
            nm(*length),
            nm(*bottom)
        ),
    }
}

/// Writes each board's history once, the boards and passes it shares named the first time.
#[derive(Default)]
struct Writer {
    lines: String,
    origins: Vec<(Origin, String)>,
    passes: Vec<(Arc<Operation>, String)>,
}

impl Writer {
    fn line(&mut self, line: String) {
        let _ = writeln!(self.lines, "{line}");
    }

    /// The name of `origin`, written out with what it came from if it's new here.
    fn origin(&mut self, origin: &Origin) -> String {
        if let Some((_, id)) = self.origins.iter().find(|(seen, _)| seen.same(origin)) {
            return id.clone();
        }
        // Named after what it came from, which is written first.
        let line = match origin.source() {
            Source::Stock {
                length,
                width,
                thickness,
            } => format!(
                "stock {{}} {} {} {}",
                nm(*length),
                nm(*width),
                nm(*thickness)
            ),
            Source::Sawn {
                operation,
                from,
                kerf,
                part,
                at,
                side,
            } => {
                let from = self.origin(from);
                let pass = self.pass(operation);
                let part = match part {
                    Part::Whole => "whole".to_string(),
                    Part::Piece(n) => format!("piece {n}"),
                };
                let side = match side {
                    Side::Whole => "whole",
                    Side::Measured => "measured",
                    Side::Offcut => "offcut",
                    Side::Loose => "loose",
                };
                let [l, w, t] = at.map(nm);
                format!(
                    "sawn {{}} {from} {pass} {} {part} {l} {w} {t} {side}",
                    kerf_words(kerf)
                )
            }
        };
        let id = format!("s{}", self.origins.len());
        self.line(line.replacen("{}", &id, 1));
        self.origins.push((origin.clone(), id.clone()));
        id
    }

    /// The name of `operation`, written out if it's new here.
    fn pass(&mut self, operation: &Arc<Operation>) -> String {
        if let Some((_, id)) = self
            .passes
            .iter()
            .find(|(seen, _)| Arc::ptr_eq(seen, operation))
        {
            return id.clone();
        }
        let id = format!("p{}", self.passes.len());
        self.line(format!(
            "pass {id} {} {}",
            operation.number,
            tool_words(&operation.tool)
        ));
        self.passes.push((Arc::clone(operation), id.clone()));
        id
    }
}

fn kerf_words(kerf: &Kerf) -> String {
    let Kerf {
        normal,
        near,
        far,
        along,
        radius,
        center,
        start,
        end,
    } = kerf;
    format!(
        "{} {} {} {} {} {} {} {} {} {} {} {}",
        normal.l,
        normal.w,
        normal.t,
        nm(*near),
        nm(*far),
        along.l,
        along.w,
        along.t,
        nm(*radius),
        center,
        nm(*start),
        nm(*end)
    )
}

/// Reads lines one after another, each naming what later ones use.
#[derive(Default)]
struct Reader {
    origins: HashMap<String, Origin>,
    passes: HashMap<String, Arc<Operation>>,
    board: Option<Board>,
    members: Vec<Member>,
    joints: Vec<Joint>,
    saw: Option<Tool>,
}

/// The words of a line, read one at a time.
struct Words<'a>(std::str::SplitWhitespace<'a>);

impl<'a> Words<'a> {
    fn word(&mut self) -> Result<&'a str, String> {
        self.0
            .next()
            .ok_or_else(|| "the line stops short".to_string())
    }

    fn number<T: std::str::FromStr>(&mut self) -> Result<T, String> {
        let word = self.word()?;
        word.parse().map_err(|_| format!("`{word}` isn't a number"))
    }

    fn length(&mut self) -> Result<Length, String> {
        Ok(Length::from_nm(self.number()?))
    }

    fn angle(&mut self) -> Result<Angle, String> {
        Ok(Angle::from_hundredths(self.number()?))
    }

    fn turn(&mut self) -> Result<Rotation, String> {
        let mut columns = [[0i64; 3]; 3];
        for column in &mut columns {
            for value in column.iter_mut() {
                *value = self.number()?;
            }
        }
        Ok(Rotation::from_raw(columns))
    }

    fn vector(&mut self) -> Result<Vector, String> {
        Ok(Vector {
            l: self.number()?,
            w: self.number()?,
            t: self.number()?,
        })
    }

    fn surface(&mut self) -> Result<Surface, String> {
        Ok(match self.word()? {
            "top" => Surface::Top,
            "bottom" => Surface::Bottom,
            "side" => Surface::Side(Edge {
                out: Direction {
                    l: self.number()?,
                    w: self.number()?,
                },
                lean: self.number()?,
                at: self.number()?,
            }),
            other => return Err(format!("`{other}` isn't a surface")),
        })
    }

    fn lie(&mut self) -> Result<Lie, String> {
        Ok(match self.word()? {
            "board" => Lie::Board(Orientation::from_rotation(self.turn()?)),
            "glue-up" => Lie::GlueUp(self.turn()?),
            other => return Err(format!("`{other}` isn't how wood lies")),
        })
    }

    fn tool(&mut self) -> Result<Tool, String> {
        Ok(match self.word()? {
            "table-saw" => {
                let blade = Blade {
                    diameter: self.length()?,
                    kerf: self.length()?,
                };
                let (fence, height) = (self.length()?, self.length()?);
                let miter_gauge = match self.word()? {
                    "none" => None,
                    stop => Some(Length::from_nm(
                        stop.parse().map_err(|_| format!("`{stop}` isn't a stop"))?,
                    )),
                };
                let saw = TableSaw {
                    blade,
                    fence,
                    height,
                    miter_gauge,
                    gauge_angle: self.angle()?,
                    tilt: self.angle()?,
                };
                Tool::TableSaw {
                    saw,
                    lie: self.lie()?,
                    travel: (self.length()?, self.length()?),
                }
            }
            "miter-saw" => {
                let saw = MiterSaw {
                    blade: Blade {
                        diameter: self.length()?,
                        kerf: self.length()?,
                    },
                    miter: self.angle()?,
                    max_miter: self.angle()?,
                    pivot: (self.length()?, self.length()?),
                    arm: self.length()?,
                    stroke: (self.length()?, self.length()?),
                };
                Tool::MiterSaw {
                    saw,
                    lie: self.lie()?,
                    length: self.length()?,
                    bottom: self.length()?,
                }
            }
            other => return Err(format!("`{other}` isn't a saw")),
        })
    }
}

impl Reader {
    fn origin(&self, id: &str) -> Result<Origin, String> {
        self.origins
            .get(id)
            .cloned()
            .ok_or_else(|| format!("no board `{id}` before this"))
    }

    fn line(&mut self, line: &str) -> Result<(), String> {
        let mut words = Words(line.split_whitespace());
        match words.word()? {
            "stock" => {
                let id = words.word()?.to_string();
                let origin = Origin::new_stock(words.length()?, words.length()?, words.length()?);
                self.origins.insert(id, origin);
            }
            "pass" => {
                let id = words.word()?.to_string();
                let operation = Operation {
                    number: words.number()?,
                    tool: words.tool()?,
                };
                self.passes.insert(id, Arc::new(operation));
            }
            "sawn" => {
                let id = words.word()?.to_string();
                let from = self.origin(words.word()?)?;
                let pass = words.word()?;
                let operation = self
                    .passes
                    .get(pass)
                    .cloned()
                    .ok_or_else(|| format!("no pass `{pass}` before this"))?;
                let kerf = Kerf {
                    normal: words.vector()?,
                    near: words.length()?,
                    far: words.length()?,
                    along: words.vector()?,
                    radius: words.length()?,
                    center: words.number()?,
                    start: words.length()?,
                    end: words.length()?,
                };
                let part = match words.word()? {
                    "whole" => Part::Whole,
                    "piece" => Part::Piece(words.number()?),
                    other => return Err(format!("`{other}` isn't a part")),
                };
                let at = [words.length()?, words.length()?, words.length()?];
                let side = match words.word()? {
                    "whole" => Side::Whole,
                    "measured" => Side::Measured,
                    "offcut" => Side::Offcut,
                    "loose" => Side::Loose,
                    other => return Err(format!("`{other}` isn't a side")),
                };
                let origin = Origin::sawn(&operation, &from, kerf, part, at, side);
                self.origins.insert(id, origin);
            }
            "board" => {
                let origin = self.origin(words.word()?)?;
                self.board = Some(origin.replay().with_origin(origin));
            }
            "member" => {
                let origin = self.origin(words.word()?)?;
                let at = [words.length()?, words.length()?, words.length()?];
                self.members.push(Member {
                    board: origin.replay().with_origin(origin),
                    rotation: words.turn()?,
                    at,
                });
            }
            "joint" => {
                let a = (words.number()?, words.surface()?);
                let b = (words.number()?, words.surface()?);
                let fastener = match words.word()? {
                    "glue" => Fastener::Glue,
                    "brads" => Fastener::Brads,
                    "screws" => Fastener::Screws,
                    other => return Err(format!("`{other}` isn't a fastener")),
                };
                self.joints.push(Joint {
                    between: [a, b],
                    fastener,
                    number: words.number()?,
                });
            }
            "saw" => self.saw = Some(words.tool()?),
            other => return Err(format!("`{other}` isn't something a recipe has")),
        }
        Ok(())
    }
}
