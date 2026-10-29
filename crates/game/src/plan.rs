//! Plans for what's been put together at the bench, as a page to print: a drawing of the whole
//! thing, the lumber to buy, a cut list, where each part comes out of the lumber, a drawing of
//! each part, and the steps to build it, every cut and joint in the order they were made. What
//! each part is and where it goes come from the assembly; how it was cut, from where each board
//! came from (`cutting`).

use std::{
    fmt::Write as _,
    fs, io,
    path::{Path, PathBuf},
    process::Command,
};

use woodsim::{Across, Angle, Assembly, Board, Fastener, Joint, Length, Member, Surface, surfaces};

use crate::{
    cutting::Cutting,
    shop::{describe_board, describe_size, measured},
};

/// Where plans are saved, from where the game runs.
pub const FOLDER: &str = "plans";

/// Nanometers in an inch, and in the 64th of one that lengths are rounded to.
const NM_PER_INCH: f64 = 25_400_000.0;
const NM_PER_64TH: f64 = NM_PER_INCH / 64.0;

/// Places closer than this, in nanometers, are the same place: a micron.
const TOUCHING: f64 = 1_000.0;

/// Which way the drawings look at the assembly: from in front, to the right, and above, as it
/// lay on the bench (its `x` along the bench, `y` away from the operator, `z` up).
const TOWARD_VIEWER: Vec3 = [0.62, -1.0, 0.85];

/// Where the light comes from, to shade the faces so the drawings read as solid.
const TOWARD_LIGHT: Vec3 = [-0.45, -0.7, 1.0];

type Vec3 = [f64; 3];
type Vec2 = [f64; 2];

/// Plans for an assembly, lying as it lay on the bench.
pub struct Plan {
    assembly: Assembly,
    parts: Vec<Part>,
    /// What each member is called in the drawings and steps: its part's letter, numbered when
    /// there are more than one.
    labels: Vec<String>,
    steps: Vec<Step>,
    cutting: Cutting,
}

/// A step of building it, in the order things were done: cutting, or parts going together, by
/// their place in `Cutting::listings` and `Plan::steps`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Built {
    Cut(usize),
    Joined(usize),
}

/// One line of the cut list: parts that come out the same, however they're turned over.
struct Part {
    letter: String,
    members: Vec<usize>,
    board: Board,
    sketch: Sketch,
}

/// Putting one part, or parts already together, against another.
struct Step {
    joint: Joint,
    /// Every member in what it makes, and those that went on.
    together: Vec<usize>,
    added: Vec<usize>,
    /// The other places they meet flush, fastened at the same time.
    also: Vec<Joint>,
}

impl Plan {
    pub fn of(assembly: Assembly) -> Plan {
        let mut parts: Vec<Part> = Vec::new();
        for (index, member) in assembly.members().iter().enumerate() {
            let sketch = Sketch::of(&member.board);
            match parts.iter_mut().find(|part| part.sketch.key == sketch.key) {
                Some(part) => part.members.push(index),
                None => parts.push(Part {
                    letter: letter(parts.len()),
                    members: vec![index],
                    board: member.board.clone(),
                    sketch,
                }),
            }
        }
        let mut labels = vec![String::new(); assembly.members().len()];
        for part in &parts {
            for (count, &member) in part.members.iter().enumerate() {
                labels[member] = match part.members.len() {
                    1 => part.letter.clone(),
                    _ => format!("{}{}", part.letter, count + 1),
                };
            }
        }
        let steps = steps(&assembly);
        let cutting = Cutting::of(&assembly);
        Plan {
            assembly,
            parts,
            labels,
            steps,
            cutting,
        }
    }

    /// Every cut and every joint, in the order they were made.
    fn build_steps(&self) -> Vec<Built> {
        let mut built: Vec<(u64, Built)> = (0..self.cutting.listings.len())
            .map(|listing| (self.cutting.number(listing), Built::Cut(listing)))
            .chain(
                self.steps
                    .iter()
                    .enumerate()
                    .map(|(index, step)| (step.joint.number, Built::Joined(index))),
            )
            .collect();
        built.sort_by_key(|&(number, _)| number);
        built.into_iter().map(|(_, step)| step).collect()
    }

    /// What it is, for the page's title: "a glue-up of 4 parts".
    fn title(&self) -> String {
        match self.assembly.members().len() {
            1 => "one part".to_string(),
            n => format!("a glue-up of {n} parts"),
        }
    }

    /// The plans as a web page, ready to print.
    pub fn page(&self) -> String {
        let [wide, deep, high] = self
            .assembly
            .bounds()
            .map(|(least, most)| measured(most - least));
        let everything: Vec<usize> = (0..self.assembly.members().len()).collect();
        let mut page = String::new();
        let _ = write!(
            page,
            "<!doctype html>\n<html lang=\"en\">\n<head>\n<meta charset=\"utf-8\">\n\
             <title>Plans for {title}</title>\n<style>{STYLE}</style>\n</head>\n<body>\n\
             <header><h1>Plans for {title}</h1>\
             <p class=\"sub\">{wide} wide, {deep} deep, and {high} high, as it lay on the \
             bench</p></header>\n\
             <section class=\"overview\">{drawing}</section>\n",
            title = self.title(),
            drawing = self.drawing(&everything, &[], &everything, (680.0, 400.0)),
        );
        page.push_str(&self.cutting.buy_list());
        page.push_str(&self.cut_list());
        let members = self.assembly.members();
        page.push_str(&self.cutting.diagrams(members, &self.labels));
        page.push_str("<section>\n<h2>Parts</h2>\n");
        for part in &self.parts {
            let _ = writeln!(
                page,
                "<figure class=\"part\"><figcaption><b>{letter}</b> {count} &middot; {size}\
                 </figcaption>{drawing}{notes}</figure>",
                letter = part.letter,
                count = pieces(part.members.len()),
                size = describe_size(&part.board).replace(" x ", " &times; "),
                drawing = part.sketch.drawing(&part.letter),
                notes = match notes(&part.board) {
                    notes if notes.is_empty() => String::new(),
                    notes => format!("<p class=\"notes\">{notes}</p>"),
                },
            );
        }
        page.push_str("</section>\n");
        let built = self.build_steps();
        if !built.is_empty() {
            page.push_str(
                "<section>\n<h2>Building it</h2>\n<p class=\"sub\">Every cut and joint, in the \
                 order they were made. Your blade may not take exactly 1/8\" like the game's: \
                 cut to the sizes here, set a stop block for parts that repeat, and cut miters \
                 to fit.</p>\n<ol class=\"steps\">\n",
            );
            // The number each cut has on the page, for steps that keep a piece for a later one.
            let step_of = |listing: usize| {
                built
                    .iter()
                    .position(|&step| step == Built::Cut(listing))
                    .map_or(0, |at| at + 1)
            };
            for (number, step) in built.iter().enumerate() {
                let (class, words, drawing) = match *step {
                    Built::Cut(listing) => (
                        "cut",
                        self.cutting
                            .describe(listing, members, &self.labels, &step_of),
                        String::new(),
                    ),
                    Built::Joined(index) => {
                        let step = &self.steps[index];
                        let named: Vec<usize> = std::iter::once(&step.joint)
                            .chain(&step.also)
                            .flat_map(|joint| joint.between.map(|(member, _)| member))
                            .collect();
                        (
                            "join",
                            format!("<p>{}</p>", self.describe_step(step)),
                            self.drawing(&step.together, &step.added, &named, (260.0, 170.0)),
                        )
                    }
                };
                let _ = writeln!(
                    page,
                    "<li class=\"{class}\"><span class=\"number\">{}</span>\
                     <div class=\"what\">{words}</div>{drawing}</li>",
                    number + 1
                );
            }
            page.push_str("</ol>\n</section>\n");
        }
        page.push_str(
            "<footer>Drawn up by Woodshop Simulator. Lengths are to the nearest 1/64\"; a \
             mitered part's length is to its long points.</footer>\n</body>\n</html>\n",
        );
        page
    }

    fn cut_list(&self) -> String {
        let mut list = String::from(
            "<section>\n<h2>Cut list</h2>\n<table>\n<thead><tr><th>Part</th><th>Qty</th>\
             <th>Thickness</th><th>Width</th><th>Length</th><th>Notes</th></tr></thead>\n\
             <tbody>\n",
        );
        for part in &self.parts {
            let board = &part.board;
            let _ = writeln!(
                list,
                "<tr><td><b>{}</b></td><td>{}</td><td>{}</td><td>{}</td><td>{}</td>\
                 <td>{}</td></tr>",
                part.letter,
                part.members.len(),
                measured(board.thickness()),
                measured(board.width()),
                measured(board.length()),
                notes(board)
            );
        }
        list.push_str("</tbody>\n</table>\n</section>\n");
        list
    }

    /// One step in words: what goes against what, and how it lines up.
    fn describe_step(&self, step: &Step) -> String {
        let mut words = self.describe_joint(step.joint, true);
        for joint in &step.also {
            let _ = write!(
                words,
                " {} too, where they meet.",
                self.describe_joint(*joint, false).trim_end_matches('.')
            );
        }
        words
    }

    /// "Glue B's end to A's edge, flush with A's faces and one of its ends." A joint that's
    /// only fastened along with another leaves out how it lines up.
    fn describe_joint(&self, joint: Joint, lined_up: bool) -> String {
        let [(onto, theirs), (part, mine)] = joint.between;
        let (verb, with) = match joint.fastener {
            Fastener::Glue => ("Glue", ""),
            Fastener::Brads => ("Nail", " with brads"),
            Fastener::Screws => ("Screw", " with screws"),
        };
        let lined = if lined_up {
            self.lined_up((onto, theirs), (part, mine))
        } else {
            String::new()
        };
        format!(
            "{verb} {}'s {} to {}'s {}{with}{lined}.",
            self.labels[part],
            called(mine),
            self.labels[onto],
            called(theirs)
        )
    }

    /// How `part`'s surface sits on `onto`'s, along each way across it: ", flush with A's
    /// faces and centered between its ends".
    fn lined_up(&self, onto: (usize, Surface), part: (usize, Surface)) -> String {
        let members = self.assembly.members();
        let theirs = &members[onto.0];
        let directions = across(theirs, onto.1);
        let label = &self.labels[onto.0];
        let mut phrases = Vec::new();
        let mut flush_all_around = true;
        for direction in directions {
            let (a0, a1) = span(&corners(theirs, onto.1), direction);
            let (b0, b1) = span(&corners(&members[part.0], part.1), direction);
            let sides = [-1.0, 1.0]
                .map(|sign| side_toward(theirs, direction.map(|x| x * sign)).unwrap_or("side"));
            let (low, high) = (b0 - a0, a1 - b1);
            flush_all_around &= low.abs() < TOUCHING && high.abs() < TOUCHING;
            phrases.push(alignment(label, sides, low, high));
        }
        if flush_all_around {
            return ", flush all around".to_string();
        }
        // The second phrase about the same part says "its".
        let second = phrases[1].replacen(&format!("{label}'s"), "its", 1);
        format!(", {} and {second}", phrases[0])
    }

    /// The assembly drawn from above and to one side: the members `shown`, with those
    /// `highlighted` picked out and those `labeled` labeled, to fit in `size` pixels.
    fn drawing(
        &self,
        shown: &[usize],
        highlighted: &[usize],
        labeled: &[usize],
        size: (f64, f64),
    ) -> String {
        let toward = unit(TOWARD_VIEWER);
        let right = unit(cross([0.0, 0.0, 1.0], toward));
        let up = cross(toward, right);
        let solids: Vec<Solid> = shown
            .iter()
            .map(|&member| Solid::of(&self.assembly.members()[member], member))
            .collect();
        let flat = |p: Vec3| [dot(p, right), -dot(p, up)];
        let (mut low, mut high) = ([f64::MAX; 2], [f64::MIN; 2]);
        for solid in &solids {
            for &corner in &solid.corners {
                let [x, y] = flat(corner);
                low = [low[0].min(x), low[1].min(y)];
                high = [high[0].max(x), high[1].max(y)];
            }
        }
        const MARGIN: f64 = 16.0;
        let scale = ((size.0 - 2.0 * MARGIN) / (high[0] - low[0]).max(1.0))
            .min((size.1 - 2.0 * MARGIN) / (high[1] - low[1]).max(1.0));
        let width = (high[0] - low[0]) * scale + 2.0 * MARGIN;
        let height = (high[1] - low[1]) * scale + 2.0 * MARGIN;
        let screen = |p: Vec3| {
            let [x, y] = flat(p);
            [(x - low[0]) * scale + MARGIN, (y - low[1]) * scale + MARGIN]
        };
        let points = |polygon: &[Vec3]| {
            polygon
                .iter()
                .map(|&p| {
                    let [x, y] = screen(p);
                    format!("{x:.1},{y:.1}")
                })
                .collect::<Vec<_>>()
                .join(" ")
        };
        let mut svg = format!(
            "<svg class=\"assembly\" viewBox=\"0 0 {width:.0} {height:.0}\" width=\"{width:.0}\" \
             height=\"{height:.0}\" xmlns=\"http://www.w3.org/2000/svg\">"
        );
        for index in drawing_order(&solids, toward, &flat) {
            let solid = &solids[index];
            let lit = highlighted.contains(&solid.member);
            let mut sides: Vec<&Face> = solid
                .faces
                .iter()
                .filter(|face| !face.flat && dot(face.normal, toward) > 1e-9)
                .collect();
            sides.sort_by(|a, b| a.depth(toward).total_cmp(&b.depth(toward)));
            for face in sides {
                let _ = write!(
                    svg,
                    "<polygon class=\"side\" fill=\"{}\" points=\"{}\"/>",
                    shade(face.normal, lit),
                    points(&face.points)
                );
            }
            // The face toward the viewer, top or bottom, is nearest of all the board's: the top
            // if the board's face up, the bottom if it's been turned over.
            let top = dot(solid.up, toward) >= 0.0;
            for face in solid
                .faces
                .iter()
                .filter(|face| face.flat && dot(face.normal, toward) >= 0.0)
            {
                let fill = shade(face.normal, lit);
                let _ = write!(
                    svg,
                    "<polygon class=\"flat\" fill=\"{fill}\" stroke=\"{fill}\" points=\"{}\"/>",
                    points(&face.points)
                );
            }
            let outline = if top {
                &solid.top_edges
            } else {
                &solid.bottom_edges
            };
            let path: String = outline
                .iter()
                .map(|[a, b]| {
                    let ([ax, ay], [bx, by]) = (screen(*a), screen(*b));
                    format!("M{ax:.1} {ay:.1}L{bx:.1} {by:.1}")
                })
                .collect();
            let _ = write!(svg, "<path class=\"edge\" d=\"{path}\"/>");
        }
        for solid in solids
            .iter()
            .filter(|solid| labeled.contains(&solid.member))
        {
            let middle = solid
                .corners
                .iter()
                .fold([0.0; 3], |sum, &p| add(sum, p))
                .map(|x| x / solid.corners.len() as f64);
            let [x, y] = screen(middle);
            let _ = write!(
                svg,
                "<g class=\"label\"><circle cx=\"{x:.1}\" cy=\"{y:.1}\" r=\"11\"/>\
                 <text x=\"{x:.1}\" y=\"{y:.1}\">{}</text></g>",
                self.labels[solid.member]
            );
        }
        svg.push_str("</svg>");
        svg
    }
}

/// The joints in the order they went together, a step for each that brought two parts (or
/// glue-ups) together; the joints that only fastened parts already together, like the last
/// corner of a frame, go with the step before them.
fn steps(assembly: &Assembly) -> Vec<Step> {
    let count = assembly.members().len();
    let mut group: Vec<usize> = (0..count).collect();
    let mut steps: Vec<Step> = Vec::new();
    for joint in assembly.joints() {
        let [(onto, _), (part, _)] = joint.between;
        let (theirs, mine) = (group[onto], group[part]);
        if theirs == mine {
            if let Some(step) = steps.last_mut() {
                step.also.push(*joint);
            }
            continue;
        }
        let added: Vec<usize> = (0..count).filter(|&k| group[k] == mine).collect();
        for k in &added {
            group[*k] = theirs;
        }
        steps.push(Step {
            joint: *joint,
            together: (0..count).filter(|&k| group[k] == theirs).collect(),
            added,
            also: Vec::new(),
        });
    }
    steps
}

/// "A", "B", ... "Z", "AA", "AB", ...
fn letter(index: usize) -> String {
    let mut index = index;
    let mut letters = String::new();
    loop {
        letters.insert(0, (b'A' + (index % 26) as u8) as char);
        if index < 26 {
            return letters;
        }
        index = index / 26 - 1;
    }
}

/// "1 piece", "4 pieces".
fn pieces(count: usize) -> String {
    match count {
        1 => "1 piece".to_string(),
        n => format!("{n} pieces"),
    }
}

/// What the cut list says about a board besides its size: its miters and slots.
fn notes(board: &Board) -> String {
    let described = describe_board(board);
    described
        .strip_prefix(&describe_size(board))
        .unwrap_or(&described)
        .trim_start_matches(", ")
        .to_string()
}

/// What a surface is called in a plan's steps: a face, an edge, an end, or a miter.
fn called(surface: Surface) -> &'static str {
    match surface {
        Surface::Bottom | Surface::Top => "face",
        Surface::Side(edge) if edge.out.l != 0 && edge.out.w != 0 => "miter",
        Surface::Side(edge) if edge.out.l != 0 => "end",
        Surface::Side(_) => "edge",
    }
}

/// How a part sits along one way across the surface it went against: `low` and `high` are how
/// far its own surface stops short of that one's sides (`sides`), negative where it hangs
/// over.
fn alignment(label: &str, sides: [&str; 2], low: f64, high: f64) -> String {
    let [low_side, high_side] = sides;
    let (flush_low, flush_high) = (low.abs() < TOUCHING, high.abs() < TOUCHING);
    let both = if low_side == high_side {
        format!("{label}'s {low_side}s")
    } else {
        format!("{label}'s {low_side} and {high_side}")
    };
    let one = |side: &str| {
        if low_side == high_side {
            format!("one of {label}'s {side}s")
        } else {
            format!("{label}'s {side}")
        }
    };
    let inches = |nm: f64| measured(Length::from_nm(nm.abs().round() as i64));
    if flush_low && flush_high {
        format!("flush with {both}")
    } else if flush_low {
        format!("flush with {}", one(low_side))
    } else if flush_high {
        format!("flush with {}", one(high_side))
    } else if (low - high).abs() < TOUCHING {
        if low > 0.0 {
            format!("centered between {both}")
        } else {
            format!("hanging {} past {both}", inches(low))
        }
    } else if low > 0.0 && high > 0.0 {
        let (gap, side) = if low < high {
            (low, low_side)
        } else {
            (high, high_side)
        };
        format!("{} in from {}", inches(gap), one(side))
    } else if low < 0.0 && high < 0.0 {
        format!(
            "hanging past {both}, {} and {}",
            inches(low.min(high)),
            inches(low.max(high))
        )
    } else {
        let (over, side) = if low < 0.0 {
            (low, low_side)
        } else {
            (high, high_side)
        };
        format!("hanging {} past {}", inches(over), one(side))
    }
}

/// The two ways across a member's surface, in its assembly: along the board and across it for
/// a face; for a side, through the thickness and then along it.
fn across(member: &Member, surface: Surface) -> [Vec3; 2] {
    let [l, w, t] = member.axes();
    match surface {
        Surface::Bottom | Surface::Top => [w, l],
        Surface::Side(edge) => {
            let (out_l, out_w) = (edge.out.l as f64, edge.out.w as f64);
            [t, unit(add(scaled(l, -out_w), scaled(w, out_l)))]
        }
    }
}

/// The way a member's surface faces, out of the board, in its assembly.
fn normal(member: &Member, surface: Surface) -> Vec3 {
    let [l, w, t] = member.axes();
    match surface {
        Surface::Bottom => scaled(t, -1.0),
        Surface::Top => t,
        Surface::Side(edge) => unit(add(
            scaled(l, edge.out.l as f64),
            scaled(w, edge.out.w as f64),
        )),
    }
}

/// The corners of a member's surface, in its assembly, in nanometers.
fn corners(member: &Member, surface: Surface) -> Vec<Vec3> {
    woodsim::outline(&member.board, surface)
        .into_iter()
        .flatten()
        .map(|[l, w, t]| member.point(l, w, t).map(|at| at.as_nm() as f64))
        .collect()
}

/// What the side of a member that faces `direction` is called, if one does: a face, an edge,
/// an end, or a miter, within 60° of it.
fn side_toward(member: &Member, direction: Vec3) -> Option<&'static str> {
    surfaces(&member.board)
        .into_iter()
        .map(|surface| (surface, dot(normal(member, surface), direction)))
        .filter(|&(_, facing)| facing > 0.5)
        .max_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(surface, _)| match called(surface) {
            "miter" => "end",
            called => called,
        })
}

/// The least and most of `points` along `direction`.
fn span(points: &[Vec3], direction: Vec3) -> (f64, f64) {
    points.iter().fold((f64::MAX, f64::MIN), |(low, high), &p| {
        let at = dot(p, direction);
        (low.min(at), high.max(at))
    })
}

/// A member as the drawings see it: its faces and corners, in its assembly, in nanometers.
struct Solid {
    member: usize,
    /// Which way its top faces.
    up: Vec3,
    faces: Vec<Face>,
    corners: Vec<Vec3>,
    /// The outline of its top and bottom faces: the edges of its sides, not where the regions
    /// of its face meet.
    top_edges: Vec<[Vec3; 2]>,
    bottom_edges: Vec<[Vec3; 2]>,
}

struct Face {
    points: Vec<Vec3>,
    normal: Vec3,
    /// Whether it's the board's top or bottom, rather than a side.
    flat: bool,
}

impl Face {
    /// How near the viewer its middle is.
    fn depth(&self, toward: Vec3) -> f64 {
        self.points.iter().map(|&p| dot(p, toward)).sum::<f64>() / self.points.len() as f64
    }
}

impl Solid {
    fn of(member: &Member, index: usize) -> Solid {
        let mut faces = Vec::new();
        let (mut top_edges, mut bottom_edges) = (Vec::new(), Vec::new());
        for surface in surfaces(&member.board) {
            let normal = normal(member, surface);
            for polygon in woodsim::outline(&member.board, surface) {
                let points: Vec<Vec3> = polygon
                    .iter()
                    .map(|&[l, w, t]| member.point(l, w, t).map(|at| at.as_nm() as f64))
                    .collect();
                if let Surface::Side(_) = surface {
                    bottom_edges.push([points[0], points[1]]);
                    top_edges.push([points[3], points[2]]);
                }
                faces.push(Face {
                    points,
                    normal,
                    flat: !matches!(surface, Surface::Side(_)),
                });
            }
        }
        let corners = faces
            .iter()
            .filter(|face| face.flat)
            .flat_map(|face| face.points.iter().copied())
            .collect();
        Solid {
            member: index,
            up: member.axes()[2],
            faces,
            corners,
            top_edges,
            bottom_edges,
        }
    }

    /// Whether `other` is in front of this one as seen from `toward`, if a plane through one
    /// of their faces has them on either side of it.
    fn behind(&self, other: &Solid, toward: Vec3) -> Option<bool> {
        for face in self.faces.iter().chain(&other.faces) {
            let facing = dot(face.normal, toward);
            if facing.abs() < 1e-9 {
                continue;
            }
            let plane = dot(face.normal, face.points[0]);
            let (mine, theirs) = (
                span(&self.corners, face.normal),
                span(&other.corners, face.normal),
            );
            if mine.1 <= plane + TOUCHING && theirs.0 >= plane - TOUCHING {
                return Some(facing > 0.0);
            }
            if theirs.1 <= plane + TOUCHING && mine.0 >= plane - TOUCHING {
                return Some(facing < 0.0);
            }
        }
        None
    }

    fn depth(&self, toward: Vec3) -> f64 {
        self.corners.iter().map(|&p| dot(p, toward)).sum::<f64>() / self.corners.len() as f64
    }
}

/// The order to draw `solids` in so nearer ones cover farther ones: wherever two overlap on
/// the page, the one behind first.
fn drawing_order(solids: &[Solid], toward: Vec3, flat: &dyn Fn(Vec3) -> Vec2) -> Vec<usize> {
    let boxes: Vec<(Vec2, Vec2)> = solids
        .iter()
        .map(|solid| {
            solid
                .corners
                .iter()
                .fold(([f64::MAX; 2], [f64::MIN; 2]), |(low, high), &p| {
                    let [x, y] = flat(p);
                    (
                        [low[0].min(x), low[1].min(y)],
                        [high[0].max(x), high[1].max(y)],
                    )
                })
        })
        .collect();
    let overlap = |a: usize, b: usize| {
        let ((a0, a1), (b0, b1)) = (boxes[a], boxes[b]);
        (0..2).all(|k| a0[k] < b1[k] - TOUCHING && b0[k] < a1[k] - TOUCHING)
    };
    let count = solids.len();
    let depth: Vec<f64> = solids.iter().map(|solid| solid.depth(toward)).collect();
    let mut after: Vec<Vec<usize>> = vec![Vec::new(); count];
    let mut waiting = vec![0usize; count];
    for a in 0..count {
        for b in a + 1..count {
            if !overlap(a, b) {
                continue;
            }
            let b_in_front = solids[a]
                .behind(&solids[b], toward)
                .unwrap_or(depth[b] > depth[a]);
            let (first, then) = if b_in_front { (a, b) } else { (b, a) };
            after[first].push(then);
            waiting[then] += 1;
        }
    }
    let mut order = Vec::with_capacity(count);
    let mut left: Vec<usize> = (0..count).collect();
    while !left.is_empty() {
        // The farthest of those with nothing left behind them, or, if they all wait on each
        // other, the farthest of all.
        let farthest = |candidates: &mut dyn Iterator<Item = usize>| {
            candidates.min_by(|&a, &b| depth[a].total_cmp(&depth[b]))
        };
        let next = farthest(&mut left.iter().copied().filter(|&k| waiting[k] == 0))
            .or_else(|| farthest(&mut left.iter().copied()))
            .expect("something left to draw");
        left.retain(|&k| k != next);
        for &then in &after[next] {
            waiting[then] = waiting[then].saturating_sub(1);
        }
        order.push(next);
    }
    order
}

/// The paint for a face turned `normal`: lit by the light over the viewer's shoulder, warm if
/// it's picked out.
fn shade(normal: Vec3, lit: bool) -> String {
    let light = 0.74 + 0.26 * dot(normal, unit(TOWARD_LIGHT)).max(0.0);
    let base = if lit {
        [1.0, 0.8, 0.55]
    } else {
        [1.0, 1.0, 1.0]
    };
    let [r, g, b] = base.map(|c| (c * light * 255.0).round() as u8);
    format!("#{r:02x}{g:02x}{b:02x}")
}

/// How a slot shows on the face a part is drawn from.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Seen {
    /// Sawn into the face toward the viewer.
    Above,
    /// Sawn into the face away from the viewer: drawn dashed, as if seen through it.
    Below,
    /// Through the board.
    Through,
}

/// A part drawn on its face: its outline and the slots sawn into it, in nanometers from its
/// corner, `l` across the page and `w` up it. Of the four ways it could lie in its box, it's
/// drawn the one that sorts first, so the same part turned over draws the same.
#[derive(Clone, Debug)]
struct Sketch {
    length: f64,
    width: f64,
    /// The regions of its face, to fill and to keep the slots within.
    regions: Vec<Vec<Vec2>>,
    /// Its outline, a straight run at a time.
    edges: Vec<[Vec2; 2]>,
    slots: Vec<(Vec<Vec2>, Seen)>,
    /// Where each slanted end's middle is, which way it faces, and its angle from square.
    miters: Vec<(Vec2, Vec2, Angle)>,
    /// What tells it apart from other parts: its thickness and its drawing, to a 64th.
    key: String,
}

impl Sketch {
    fn of(board: &Board) -> Sketch {
        let sides: Vec<woodsim::Edge> = surfaces(board)
            .into_iter()
            .filter_map(|surface| match surface {
                Surface::Side(edge) => Some(edge),
                _ => None,
            })
            .collect();
        let point = |(l, w): (Length, Length)| [l.as_nm() as f64, w.as_nm() as f64];
        let mut regions = Vec::new();
        // Each side's runs, to join where a side passes from one region of the face to the next.
        let mut runs: Vec<(woodsim::Edge, Vec<[Vec2; 2]>)> = Vec::new();
        for region in board.face() {
            let corners: Vec<Vec2> = region.corners().into_iter().map(point).collect();
            let edges = region.edges();
            for (i, edge) in edges.iter().enumerate() {
                if !sides.contains(edge) {
                    continue;
                }
                let run = [corners[i], corners[(i + 1) % corners.len()]];
                match runs.iter_mut().find(|(side, _)| side == edge) {
                    Some((_, list)) => list.push(run),
                    None => runs.push((*edge, vec![run])),
                }
            }
            regions.push(corners);
        }
        let mut edges = Vec::new();
        let mut miters = Vec::new();
        for (side, list) in runs {
            let joined = join_runs(list);
            if side.out.l != 0 && side.out.w != 0 {
                let out = unit([side.out.l as f64, side.out.w as f64, 0.0]);
                let slant = (side.out.w.abs() as f64).atan2(side.out.l.abs() as f64);
                let angle = Angle::from_hundredths((slant.to_degrees() * 100.0).round() as i32);
                for [a, b] in &joined {
                    let middle = [(a[0] + b[0]) / 2.0, (a[1] + b[1]) / 2.0];
                    miters.push((middle, [out[0], out[1]], angle));
                }
            }
            edges.extend(joined);
        }
        let thickness = board.thickness();
        let mut slots = Vec::new();
        for kerf in board.kerfs() {
            let Across::Face(across) = kerf.across else {
                continue;
            };
            let from_below = kerf.from_below();
            let face = if from_below { Length::ZERO } else { thickness };
            // From positions along and across the slot back to the board's `(l, w)`.
            let (al, aw, cl, cw) = (
                kerf.along.l as f64,
                kerf.along.w as f64,
                across.l as f64,
                across.w as f64,
            );
            let determinant = al * cw - aw * cl;
            let at = |along: Length, across: Length| {
                let (u, a) = (along.as_nm() as f64, across.as_nm() as f64);
                [
                    (u * cw - aw * a) / determinant,
                    (al * a - u * cl) / determinant,
                ]
            };
            let footprint = |(start, end): (Length, Length)| {
                vec![
                    at(start, kerf.near),
                    at(end, kerf.near),
                    at(end, kerf.far),
                    at(start, kerf.far),
                ]
            };
            if let Some(span) = kerf.span_at(face) {
                let seen = if from_below { Seen::Below } else { Seen::Above };
                slots.push((footprint(span), seen));
            }
            if let Some(span) = kerf.through_span(thickness) {
                slots.push((footprint(span), Seen::Through));
            }
        }
        let resawn: Vec<String> = board
            .kerfs()
            .iter()
            .filter(|kerf| kerf.across == Across::Thickness)
            .map(|kerf| {
                let slot = board.slot(kerf);
                format!("{}x{}", measured(slot.width), measured(slot.depth))
            })
            .collect();
        let (mut low, mut high) = ([f64::MAX; 2], [f64::MIN; 2]);
        for &[l, w] in regions.iter().flatten() {
            low = [low[0].min(l), low[1].min(w)];
            high = [high[0].max(l), high[1].max(w)];
        }
        let drawn = Sketch {
            length: high[0] - low[0],
            width: high[1] - low[1],
            regions,
            edges,
            slots,
            miters,
            key: String::new(),
        };
        [(false, false), (true, false), (false, true), (true, true)]
            .into_iter()
            .map(|(flip_l, flip_w)| {
                let mut sketch = drawn.turned(low, high, flip_l, flip_w);
                sketch.key = format!(
                    "{:?} {} {resawn:?}",
                    measured(thickness),
                    sketch.drawn_key()
                );
                sketch
            })
            .min_by(|a, b| a.key.cmp(&b.key))
            .expect("four ways to lie")
    }

    /// The sketch moved to start at its box's corner, `low`, and flipped end for end, edge for
    /// edge, or both: one of the ways the part could lie face up or face down. Flipping it end
    /// for end or edge for edge turns it over, so what was sawn into its bottom shows on top.
    fn turned(&self, low: Vec2, high: Vec2, flip_l: bool, flip_w: bool) -> Sketch {
        let at = |[l, w]: Vec2| {
            [
                if flip_l { high[0] - l } else { l - low[0] },
                if flip_w { high[1] - w } else { w - low[1] },
            ]
        };
        let way = |[l, w]: Vec2| [if flip_l { -l } else { l }, if flip_w { -w } else { w }];
        let over = flip_l != flip_w;
        Sketch {
            length: self.length,
            width: self.width,
            regions: self
                .regions
                .iter()
                .map(|region| region.iter().map(|&p| at(p)).collect())
                .collect(),
            edges: self.edges.iter().map(|&[a, b]| [at(a), at(b)]).collect(),
            slots: self
                .slots
                .iter()
                .map(|(points, seen)| {
                    let seen = match (seen, over) {
                        (Seen::Above, true) => Seen::Below,
                        (Seen::Below, true) => Seen::Above,
                        (seen, _) => *seen,
                    };
                    (points.iter().map(|&p| at(p)).collect(), seen)
                })
                .collect(),
            miters: self
                .miters
                .iter()
                .map(|&(middle, out, angle)| (at(middle), way(out), angle))
                .collect(),
            key: String::new(),
        }
    }

    /// The drawing to a 64th, in an order that doesn't depend on how the face is divided.
    fn drawn_key(&self) -> String {
        let round = |[l, w]: Vec2| {
            (
                (l / NM_PER_64TH).round() as i64,
                (w / NM_PER_64TH).round() as i64,
            )
        };
        let mut edges: Vec<_> = self
            .edges
            .iter()
            .map(|&[a, b]| {
                let (a, b) = (round(a), round(b));
                (a.min(b), a.max(b))
            })
            .collect();
        edges.sort();
        let mut slots: Vec<_> = self
            .slots
            .iter()
            .map(|(points, seen)| {
                let mut points: Vec<_> = points.iter().map(|&p| round(p)).collect();
                points.sort();
                (*seen, points)
            })
            .collect();
        slots.sort();
        format!("{edges:?} {slots:?}")
    }

    /// The part on its face, to scale, with its length and width and the angle of each miter.
    fn drawing(&self, id: &str) -> String {
        const MOST_WIDE: f64 = 560.0;
        const MOST_HIGH: f64 = 150.0;
        const MOST_PER_INCH: f64 = 22.0;
        const LEFT: f64 = 14.0;
        const TOP: f64 = 14.0;
        const RIGHT: f64 = 84.0;
        const BOTTOM: f64 = 42.0;
        let scale = (MOST_WIDE / self.length)
            .min(MOST_HIGH / self.width)
            .min(MOST_PER_INCH / NM_PER_INCH);
        let (long, wide) = (self.length * scale, self.width * scale);
        let at = |[l, w]: Vec2| [LEFT + l * scale, TOP + wide - w * scale];
        let path = |polygons: &mut dyn Iterator<Item = &Vec<Vec2>>| {
            polygons
                .map(|polygon| {
                    let mut d = String::new();
                    for (k, &p) in polygon.iter().enumerate() {
                        let [x, y] = at(p);
                        let _ = write!(d, "{}{x:.1} {y:.1}", if k == 0 { "M" } else { "L" });
                    }
                    d + "Z"
                })
                .collect::<String>()
        };
        let (width, height) = (LEFT + long + RIGHT, TOP + wide + BOTTOM);
        let regions = path(&mut self.regions.iter());
        let mut svg = format!(
            "<svg class=\"part-drawing\" viewBox=\"0 0 {width:.0} {height:.0}\" \
             width=\"{width:.0}\" height=\"{height:.0}\" xmlns=\"http://www.w3.org/2000/svg\">\
             <defs><clipPath id=\"face-{id}\"><path d=\"{regions}\"/></clipPath></defs>\
             <path class=\"wood\" d=\"{regions}\"/><g clip-path=\"url(#face-{id})\">"
        );
        for (points, seen) in &self.slots {
            let class = match seen {
                Seen::Above => "slot",
                Seen::Below => "slot-below",
                Seen::Through => "slot-through",
            };
            let _ = write!(
                svg,
                "<path class=\"{class}\" d=\"{}\"/>",
                path(&mut std::iter::once(points))
            );
        }
        svg.push_str("</g><path class=\"edge\" d=\"");
        for &[a, b] in &self.edges {
            let ([ax, ay], [bx, by]) = (at(a), at(b));
            let _ = write!(svg, "M{ax:.1} {ay:.1}L{bx:.1} {by:.1}");
        }
        svg.push_str("\"/>");
        // The length below it and the width to its right, each between extension lines.
        let (right, bottom) = (LEFT + long, TOP + wide);
        let along = bottom + 22.0;
        let up = right + 24.0;
        let length = measured(Length::from_nm(self.length.round() as i64));
        let width_of = measured(Length::from_nm(self.width.round() as i64));
        let _ = write!(
            svg,
            "<g class=\"dimension\">\
             <path d=\"M{LEFT:.1} {e0:.1}V{e1:.1}M{right:.1} {e0:.1}V{e1:.1}\
             M{LEFT:.1} {along:.1}H{right:.1}\
             M{t0:.1} {t1:.1}l8 -8M{t2:.1} {t1:.1}l8 -8\
             M{x0:.1} {TOP:.1}H{x1:.1}M{x0:.1} {bottom:.1}H{x1:.1}\
             M{up:.1} {TOP:.1}V{bottom:.1}\
             M{u0:.1} {u1:.1}l8 -8M{u0:.1} {u2:.1}l8 -8\"/>\
             <text x=\"{mid:.1}\" y=\"{text_y:.1}\" text-anchor=\"middle\">{length}</text>\
             <text x=\"{text_x:.1}\" y=\"{middle_y:.1}\" dominant-baseline=\"middle\">{width_of}</text>\
             </g>",
            e0 = bottom + 4.0,
            e1 = along + 4.0,
            t0 = LEFT - 4.0,
            t1 = along + 4.0,
            t2 = right - 4.0,
            x0 = right + 4.0,
            x1 = up + 4.0,
            u0 = up - 4.0,
            u1 = TOP + 4.0,
            u2 = bottom + 4.0,
            mid = LEFT + long / 2.0,
            text_y = along + 16.0,
            text_x = up + 8.0,
            middle_y = TOP + wide / 2.0,
        );
        for &(middle, out, angle) in &self.miters {
            // Inside the end, back from its middle.
            let [x, y] = at(middle);
            let (x, y) = (x - out[0] * 26.0, y + out[1] * 18.0);
            let _ = write!(
                svg,
                "<text class=\"angle\" x=\"{x:.1}\" y=\"{y:.1}\" text-anchor=\"middle\" \
                 dominant-baseline=\"middle\">{angle}</text>"
            );
        }
        svg.push_str("</svg>");
        svg
    }
}

/// Straight runs along one side of a board joined end to end where one starts as another
/// stops.
fn join_runs(mut runs: Vec<[Vec2; 2]>) -> Vec<[Vec2; 2]> {
    let mut joined: Vec<[Vec2; 2]> = Vec::new();
    while let Some(mut run) = runs.pop() {
        loop {
            let next = runs.iter().position(|&[a, b]| {
                close(a, run[1]) || close(b, run[0]) || close(a, run[0]) || close(b, run[1])
            });
            let Some(next) = next else { break };
            let [a, b] = runs.swap_remove(next);
            run = if close(a, run[1]) {
                [run[0], b]
            } else if close(b, run[0]) {
                [a, run[1]]
            } else if close(a, run[0]) {
                [b, run[1]]
            } else {
                [run[0], a]
            };
        }
        joined.push(run);
    }
    joined
}

fn close(a: Vec2, b: Vec2) -> bool {
    (a[0] - b[0]).abs() < TOUCHING && (a[1] - b[1]).abs() < TOUCHING
}

/// Saves `page` as the next unused `plan-N.html` in `folder`, making the folder if it isn't
/// there.
pub fn save(page: &str, folder: &Path) -> io::Result<PathBuf> {
    fs::create_dir_all(folder)?;
    let path = (1..)
        .map(|n| folder.join(format!("plan-{n}.html")))
        .find(|path| !path.exists())
        .expect("a free name");
    fs::write(&path, page)?;
    Ok(path)
}

/// Opens a saved page in the browser, to read or print.
pub fn open(path: &Path) -> io::Result<()> {
    #[cfg(target_os = "macos")]
    let mut command = Command::new("open");
    #[cfg(target_os = "windows")]
    let mut command = {
        let mut command = Command::new("cmd");
        command.args(["/C", "start", ""]);
        command
    };
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    let mut command = Command::new("xdg-open");
    command.arg(path).spawn().map(|_| ())
}

fn dot(a: Vec3, b: Vec3) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn cross(a: Vec3, b: Vec3) -> Vec3 {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

fn add(a: Vec3, b: Vec3) -> Vec3 {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}

fn scaled(v: Vec3, by: f64) -> Vec3 {
    v.map(|x| x * by)
}

fn unit(v: Vec3) -> Vec3 {
    scaled(v, 1.0 / dot(v, v).sqrt())
}

const STYLE: &str = "
:root { color: #1d2026; }
body { font: 14px/1.45 -apple-system, 'Segoe UI', Helvetica, Arial, sans-serif; background: #fff;
  max-width: 7.5in; margin: 32px auto; padding: 0 16px; }
h1 { font-size: 26px; margin: 0; }
h2 { font-size: 17px; margin: 28px 0 10px; padding-bottom: 4px; border-bottom: 2px solid #1d2026;
  break-after: avoid; }
h3 { font-size: 14px; margin: 0 0 6px; }
.sub { color: #555; margin: 4px 0 12px; }
table { border-collapse: collapse; width: 100%; }
th, td { text-align: left; padding: 6px 8px; border-bottom: 1px solid #ddd; vertical-align: top; }
th { font-size: 12px; text-transform: uppercase; letter-spacing: 0.04em; color: #555; }
svg { display: block; max-width: 100%; height: auto; }
svg text { font: 12px -apple-system, 'Segoe UI', Helvetica, Arial, sans-serif; fill: #1d2026; }
.overview svg { margin: 8px auto; }
.part { margin: 0 0 20px; break-inside: avoid; }
.part figcaption { margin-bottom: 6px; }
.part figcaption b { font-size: 16px; margin-right: 4px; }
.notes { color: #444; margin: 4px 0 0; }
.steps { list-style: none; padding: 0; margin: 0; }
.steps li { display: flex; align-items: center; gap: 14px; padding: 10px 0;
  border-bottom: 1px solid #e4e4e4; break-inside: avoid; }
.steps .number { flex: none; width: 28px; height: 28px; border-radius: 14px; background: #1d2026;
  color: #fff; font-weight: 700; display: flex; align-items: center; justify-content: center; }
.steps .cut .number { background: #c4581a; }
.steps .what { flex: 1; }
.steps p { margin: 0; }
.recipe { margin: 6px 0 0; padding-left: 20px; }
.recipe li { display: list-item; border: none; padding: 2px 0; }
.steps svg { flex: none; width: 260px; }
.cut-part { fill: #fbf6ee; stroke: #1d2026; stroke-width: 1.2; stroke-linejoin: round; }
.stock { stroke: #8a7a62; stroke-width: 1; }
.diagram text { font-weight: 600; font-size: 11px; fill: #c4581a; }
.side { stroke: #1d2026; stroke-width: 1.2; stroke-linejoin: round; }
.flat { stroke-width: 0.6; stroke-linejoin: round; }
.edge { fill: none; stroke: #1d2026; stroke-width: 1.4; stroke-linecap: round; }
.label circle { fill: #fff; stroke: #c4581a; stroke-width: 1.5; }
.label text { font-weight: 600; font-size: 11px; fill: #c4581a; text-anchor: middle;
  dominant-baseline: central; }
.wood { fill: #fbf6ee; }
.slot { fill: #d9cbb5; stroke: #1d2026; stroke-width: 0.8; }
.slot-below { fill: none; stroke: #1d2026; stroke-width: 0.8; stroke-dasharray: 4 3; }
.slot-through { fill: #1d2026; }
.dimension path { fill: none; stroke: #777; stroke-width: 0.8; }
.angle { font-size: 11px; fill: #c4581a; }
footer { color: #777; font-size: 12px; margin: 28px 0 0; }
@page { margin: 0.5in; }
@media print { body { margin: 0; max-width: none; } .steps li { border-color: #bbb; } }
";

#[cfg(test)]
mod tests {
    use woodsim::{Mating, MiterSaw, Orientation, Pass};

    use super::*;

    /// A frame piece `length` long point to long point, mitered `degrees` at both ends.
    fn frame_piece(length: Length, hundredths: i32) -> Board {
        let stock = Board::new(
            Length::from_inches(48),
            Length::from_inch_fraction(3, 2),
            Length::from_inch_fraction(3, 4),
        );
        let saw = |hundredths| MiterSaw {
            miter: Angle::from_hundredths(hundredths),
            ..MiterSaw::twelve_inch()
        };
        let Ok(Pass::Cut(trim)) =
            saw(-hundredths).crosscut(&stock, Orientation::default(), Length::from_inches(2))
        else {
            panic!("expected the end trimmed");
        };
        let Ok(Pass::Cut(cut)) =
            saw(hundredths).crosscut(&trim.offcut.unwrap().board, Orientation::default(), length)
        else {
            panic!("expected a frame piece");
        };
        cut.piece.board
    }

    fn side(board: &Board, pick: impl Fn(woodsim::Edge) -> bool) -> Surface {
        surfaces(board)
            .into_iter()
            .find(|surface| matches!(surface, Surface::Side(edge) if pick(*edge)))
            .expect("that side")
    }

    fn miter(board: &Board, far: bool) -> Surface {
        side(board, |edge| edge.out.w != 0 && (edge.out.l > 0) == far)
    }

    /// Puts `piece`'s `surface` against `onto`, face up, at the least stop each way across.
    fn add(assembly: &mut Assembly, onto: (usize, Surface), piece: &Board, surface: Surface) {
        let other = Assembly::of(piece.clone());
        let turn = (0..4)
            .find(|&turn| {
                let mating = Mating {
                    turn,
                    slide: [Length::ZERO; 2],
                };
                let motion = assembly.mating(onto, &other, (0, surface), mating).unwrap();
                motion.rotation.apply([0.0, 0.0, 1.0])[2] > 0.999
            })
            .expect("a way that keeps it face up");
        let stops = assembly.stops(onto, &other, (0, surface), turn).unwrap();
        let mating = Mating {
            turn,
            slide: [stops[0][0], stops[1][0]],
        };
        assert!(assembly.attach(onto, &other, (0, surface), mating, Fastener::Glue));
    }

    /// `count` pieces mitered to close into a ring: a frame of four, an octagon of eight.
    fn ring(count: usize, length: Length) -> Assembly {
        let hundredths = 18_000 / count as i32;
        let piece = frame_piece(length, hundredths);
        let mut assembly = Assembly::of(piece.clone());
        for last in 0..count - 1 {
            let onto = (last, miter(&assembly.members()[last].board, true));
            add(&mut assembly, onto, &piece, miter(&piece, false));
        }
        assembly
    }

    #[test]
    fn parts_are_lettered_in_order_and_counted_together() {
        assert_eq!(
            [0, 1, 25, 26, 27, 701, 702].map(letter),
            ["A", "B", "Z", "AA", "AB", "ZZ", "AAA"]
        );
        let plan = Plan::of(ring(8, Length::from_inches(6)));
        assert_eq!(plan.parts.len(), 1);
        assert_eq!(plan.parts[0].members.len(), 8);
        assert_eq!(plan.labels[0], "A1");
        assert_eq!(plan.labels[7], "A8");
    }

    #[test]
    fn the_same_part_turned_over_is_the_same_line_of_the_cut_list() {
        // Mitered the other way, its long points are along its other edge, away from the fence
        // the saw measured it along: it's the same part turned over.
        let piece = frame_piece(Length::from_inches(10), 4500);
        let mirrored = frame_piece(Length::from_inch_fraction(17, 2), -4500);
        assert_ne!(piece, mirrored);
        assert_eq!(Sketch::of(&piece).key, Sketch::of(&mirrored).key);
        // A square-cut board the same size isn't.
        let square = Board::new(
            Length::from_inches(10),
            Length::from_inch_fraction(3, 2),
            Length::from_inch_fraction(3, 4),
        );
        assert_ne!(Sketch::of(&square).key, Sketch::of(&piece).key);
    }

    #[test]
    fn a_frame_goes_together_in_three_steps_and_closes_on_the_last() {
        let plan = Plan::of(ring(4, Length::from_inches(10)));
        assert_eq!(plan.steps.len(), 3);
        assert_eq!(plan.steps[0].together, [0, 1]);
        assert_eq!(plan.steps[0].added, [1]);
        assert_eq!(plan.steps[2].together, [0, 1, 2, 3]);
        assert_eq!(plan.steps[2].also.len(), 1);
        assert_eq!(
            plan.describe_step(&plan.steps[0]),
            "Glue A2's miter to A1's miter, flush all around."
        );
        assert_eq!(
            plan.describe_step(&plan.steps[2]),
            "Glue A4's miter to A3's miter, flush all around. Glue A4's miter to A1's miter \
             too, where they meet."
        );
    }

    #[test]
    fn an_end_against_an_edge_says_where_it_lines_up() {
        let base = Board::new(
            Length::from_inches(24),
            Length::from_inches(4),
            Length::from_inch_fraction(3, 4),
        );
        let upright = Board::new(
            Length::from_inches(10),
            Length::from_inches(2),
            Length::from_inch_fraction(3, 4),
        );
        let mut assembly = Assembly::of(base.clone());
        let edge = side(&base, |edge| edge.out.l == 0 && edge.out.w > 0);
        let end = side(&upright, |edge| edge.out.w == 0 && edge.out.l < 0);
        add(&mut assembly, (0, edge), &upright, end);
        let plan = Plan::of(assembly);
        assert_eq!(
            plan.describe_step(&plan.steps[0]),
            "Glue B's end to A's edge, flush with A's faces and flush with one of its ends."
        );
    }

    #[test]
    fn alignments_read_as_flush_centered_in_from_or_hanging_past() {
        let inch = NM_PER_INCH;
        let sides = ["end", "end"];
        assert_eq!(alignment("A", sides, 0.0, 0.0), "flush with A's ends");
        assert_eq!(
            alignment("A", sides, 0.0, inch),
            "flush with one of A's ends"
        );
        assert_eq!(
            alignment("A", sides, inch, inch),
            "centered between A's ends"
        );
        assert_eq!(
            alignment("A", sides, inch, 2.0 * inch),
            "1\" in from one of A's ends"
        );
        assert_eq!(
            alignment("A", sides, -inch / 4.0, -inch / 4.0),
            "hanging 1/4\" past A's ends"
        );
        assert_eq!(
            alignment("A", ["edge", "miter"], -inch / 2.0, inch),
            "hanging 1/2\" past A's edge"
        );
    }

    /// A frame of four pieces mitered 45° at both ends, 4" long point to long point, made the
    /// way you'd make it: a 1x4 ripped to 3/4", then from the strip, for each piece, the end
    /// trimmed to a miter and the piece cut off it; then the pieces glued up into the frame.
    /// The trim has to take more than the strip is wide: any less leaves a corner of the last
    /// miter on the next piece.
    fn frame_from_a_one_by_four() -> Assembly {
        let stock = Board::new(
            Length::from_inches(24),
            Length::from_inch_fraction(7, 2),
            Length::from_inch_fraction(3, 4),
        );
        let table_saw = woodsim::TableSaw {
            blade: woodsim::Blade {
                diameter: Length::from_inches(10),
                kerf: woodsim::Blade::FULL_KERF,
            },
            fence: Length::from_inch_fraction(3, 4),
            height: Length::from_inches(1),
            miter_gauge: None,
            gauge_angle: woodsim::Angle::ZERO,
            tilt: woodsim::Angle::ZERO,
        };
        let Ok(Pass::Cut(rip)) = table_saw.rip(&stock, Orientation::default()) else {
            panic!("expected the 1x4 ripped");
        };
        let saw = |hundredths| MiterSaw {
            miter: Angle::from_hundredths(hundredths),
            ..MiterSaw::twelve_inch()
        };
        let mut strip = rip.piece.board;
        let mut pieces = Vec::new();
        for _ in 0..4 {
            let Ok(Pass::Cut(trim)) =
                saw(-4500).crosscut(&strip, Orientation::default(), Length::from_inches(1))
            else {
                panic!("expected the end trimmed");
            };
            let Ok(Pass::Cut(cut)) = saw(4500).crosscut(
                &trim.offcut.unwrap().board,
                Orientation::default(),
                Length::from_inches(4),
            ) else {
                panic!("expected a frame piece");
            };
            pieces.push(cut.piece.board);
            strip = cut.offcut.unwrap().board;
        }
        let mut frame = Assembly::of(pieces[0].clone());
        for (last, piece) in pieces.iter().enumerate().skip(1).map(|(k, p)| (k - 1, p)) {
            let onto = (last, miter(&frame.members()[last].board, true));
            add(&mut frame, onto, piece, miter(piece, false));
        }
        frame
    }

    #[test]
    fn a_frame_cut_from_one_board_buys_it_and_repeats_the_cuts_for_each_piece() {
        let plan = Plan::of(frame_from_a_one_by_four());
        assert_eq!(plan.cutting.stock.len(), 1);
        assert_eq!(plan.cutting.stock[0].members, [0, 1, 2, 3]);
        // The rip, the first piece's trim and cut, the same again for the rest; then three
        // joints.
        let built = plan.build_steps();
        assert_eq!(built.len(), 7);
        assert!(built[..4].iter().all(|step| matches!(step, Built::Cut(_))));
        assert!(
            built[4..]
                .iter()
                .all(|step| matches!(step, Built::Joined(_)))
        );
        let members = plan.assembly.members();
        let say = |listing: usize| {
            plan.cutting
                .describe(listing, members, &plan.labels, &|listing| listing + 1)
        };
        assert_eq!(
            say(0),
            "<p>Table saw, rip fence at 3/4\", blade 1\" high: Rip a new 1x4, 3/4\" x 3-1/2\" x \
             24\". Keep the 3/4\" strip for step 2.</p>"
        );
        assert_eq!(
            say(1),
            "<p>Miter saw at 45° left, stop at 1\": Trim the end off the 3/4\" strip from step \
             1. Keep the rest of the 3/4\" strip for step 3.</p>"
        );
        assert_eq!(
            say(2),
            "<p>Miter saw at 45° right, stop at 4\": Cut A1 off the rest of the 3/4\" strip \
             from step 2. Keep the rest of the 3/4\" strip for step 4.</p>"
        );
        assert_eq!(
            say(3),
            "<p>Repeat steps 2 and 3 three more times on the rest of the 3/4\" strip, making A2, \
             A3, and A4.</p>"
        );
    }

    /// Four pieces made alike: a 1x12 ripped into four 2-3/8" strips, one after another, then
    /// each strip cut to 14-1/4" at 45°, turned end for end, and cut to 14-1/4" at 45° again,
    /// mitering both ends; the four glued up.
    fn frame_from_a_one_by_twelve() -> Assembly {
        let table_saw = woodsim::TableSaw {
            blade: woodsim::Blade {
                diameter: Length::from_inches(10),
                kerf: woodsim::Blade::FULL_KERF,
            },
            fence: Length::from_inch_fraction(19, 8),
            height: Length::from_inches(1),
            miter_gauge: None,
            gauge_angle: woodsim::Angle::ZERO,
            tilt: woodsim::Angle::ZERO,
        };
        let mut rest = Board::new(
            Length::from_inches(24),
            Length::from_inch_fraction(45, 4),
            Length::from_inch_fraction(3, 4),
        );
        let mut strips = Vec::new();
        for _ in 0..4 {
            let Ok(Pass::Cut(rip)) = table_saw.rip(&rest, Orientation::default()) else {
                panic!("expected a strip ripped off");
            };
            strips.push(rip.piece.board);
            rest = rip.offcut.unwrap().board;
        }
        let saw = MiterSaw {
            miter: Angle::from_degrees(-45),
            ..MiterSaw::twelve_inch()
        };
        let length = Length::from_inch_fraction(57, 4);
        let pieces: Vec<Board> = strips
            .iter()
            .map(|strip| {
                let Ok(Pass::Cut(first)) = saw.crosscut(strip, Orientation::default(), length)
                else {
                    panic!("expected the strip cut to length");
                };
                let turned = Orientation::default().turned_left().turned_left();
                let Ok(Pass::Cut(second)) = saw.crosscut(&first.piece.board, turned, length) else {
                    panic!("expected the other end cut");
                };
                second.piece.board
            })
            .collect();
        // Stacked, just to put them together: the plan's cutting doesn't care how.
        let mut glue_up = Assembly::of(pieces[0].clone());
        for (last, piece) in pieces.iter().enumerate().skip(1).map(|(k, p)| (k - 1, p)) {
            add(&mut glue_up, (last, Surface::Top), piece, Surface::Bottom);
        }
        glue_up
    }

    #[test]
    fn strips_ripped_one_after_another_and_cut_alike_are_two_steps() {
        let plan = Plan::of(frame_from_a_one_by_twelve());
        assert_eq!(plan.cutting.listings.len(), 2);
        let members = plan.assembly.members();
        let say = |listing: usize| {
            plan.cutting
                .describe(listing, members, &plan.labels, &|listing| listing + 1)
        };
        assert_eq!(
            say(0),
            "<p>Table saw, rip fence at 2-3/8\", blade 1\" high: Rip four 2-3/8\" strips off a \
             new 1x12, 3/4\" x 11-1/4\" x 24\". Keep them for step 2.</p>"
        );
        assert_eq!(
            say(1),
            "<p>For each 2-3/8\" strip from step 1, to make A1, A2, A3, and A4:</p>\
             <ol class=\"recipe\">\
             <li>Miter saw at 45° left, stop at 14-1/4\": Cut it to 14-1/4\".</li>\
             <li>Miter saw at 45° left, stop at 14-1/4\": Turn it end for end, then cut it to \
             14-1/4\".</li></ol>"
        );
    }

    #[test]
    fn cuts_that_nibble_a_part_to_length_are_one_cut_to_the_length_it_ended_up() {
        let saw = MiterSaw::twelve_inch();
        let mut board = Board::new(
            Length::from_inches(24),
            Length::from_inch_fraction(7, 2),
            Length::from_inch_fraction(3, 4),
        );
        for sixteenths in [162, 161, 160] {
            let Ok(Pass::Cut(cut)) = saw.crosscut(
                &board,
                Orientation::default(),
                Length::from_inch_fraction(sixteenths, 16),
            ) else {
                panic!("expected a cut");
            };
            board = cut.piece.board;
        }
        let plan = Plan::of(Assembly::of(board));
        assert_eq!(plan.cutting.cuts.len(), 1);
        assert_eq!(
            plan.cutting
                .describe(0, plan.assembly.members(), &plan.labels, &|cut| cut + 1),
            "<p>Miter saw, square, stop at 10\": Cut A off a new 1x4, 3/4\" x 3-1/2\" x 24\".</p>"
        );
    }

    #[test]
    fn the_page_has_what_to_buy_and_every_part_and_step() {
        let page = Plan::of(frame_from_a_one_by_four()).page();
        assert!(page.starts_with("<!doctype html>"));
        assert!(page.contains("<title>Plans for a glue-up of 4 parts</title>"));
        assert!(page.contains("mitered 45° at both ends"), "{page}");
        assert!(page.contains("<td>1x4, 3/4\" x 3-1/2\" x 24\"</td><td>1</td>"));
        // The part, and the board it comes out of.
        assert_eq!(page.matches("<figure class=\"part\">").count(), 2);
        assert_eq!(page.matches("<li class=\"cut\">").count(), 4);
        assert_eq!(page.matches("<li class=\"join\">").count(), 3);
        // A drawing of it all, the board's diagram, the part, and each joint.
        assert_eq!(page.matches("<svg").count(), 6);
        // What the rip left on the far side of the blade never went into the frame.
        assert!(!page.contains("2-5/8"), "{page}");
        assert!(page.contains("A4"));
    }

    #[test]
    fn a_single_part_has_no_steps() {
        let board = Board::new(
            Length::from_inches(24),
            Length::from_inches(4),
            Length::from_inch_fraction(3, 4),
        );
        let page = Plan::of(Assembly::of(board)).page();
        assert!(page.contains("Plans for one part"));
        assert!(!page.contains("Putting it together"));
    }

    #[test]
    fn nearer_boards_are_drawn_over_farther_ones() {
        // Two boards side by side across the bench, the second further from the viewer, who
        // looks from the front.
        let board = Board::new(
            Length::from_inches(10),
            Length::from_inches(4),
            Length::from_inch_fraction(3, 4),
        );
        let mut assembly = Assembly::of(board.clone());
        let far_edge = side(&board, |edge| edge.out.l == 0 && edge.out.w > 0);
        let near_edge = side(&board, |edge| edge.out.l == 0 && edge.out.w < 0);
        add(&mut assembly, (0, far_edge), &board, near_edge);
        let toward = unit(TOWARD_VIEWER);
        let solids: Vec<Solid> = assembly
            .members()
            .iter()
            .enumerate()
            .map(|(index, member)| Solid::of(member, index))
            .collect();
        let back = if solids[1].depth(toward) < solids[0].depth(toward) {
            1
        } else {
            0
        };
        let flat = |p: Vec3| [p[0] - p[1], p[2] + p[1]];
        assert_eq!(drawing_order(&solids, toward, &flat)[0], back);
    }

    #[test]
    fn a_part_turned_over_is_drawn_with_the_face_toward_you_lit() {
        let board = Board::new(
            Length::from_inches(10),
            Length::from_inches(4),
            Length::from_inch_fraction(3, 4),
        );
        for turns in [0, 2] {
            let over = woodsim::Motion {
                rotation: woodsim::Rotation::quarter_turns(0, turns),
                shift: [Length::ZERO; 3],
            };
            let plan = Plan::of(Assembly::of(board.clone()).moved(over));
            let drawing = plan.drawing(&[0], &[], &[], (300.0, 200.0));
            // Face up or turned over, the face you see is lit from above.
            let lit = shade([0.0, 0.0, 1.0], false);
            assert!(
                drawing.contains(&format!("class=\"flat\" fill=\"{lit}\"")),
                "{turns}: {drawing}"
            );
        }
    }

    #[test]
    fn plans_are_saved_to_the_next_free_page() {
        let folder = std::env::temp_dir().join(format!("woodshop-plans-{}", std::process::id()));
        let _ = fs::remove_dir_all(&folder);
        let first = save("one", &folder).unwrap();
        let second = save("two", &folder).unwrap();
        assert_eq!(first.file_name().unwrap(), "plan-1.html");
        assert_eq!(second.file_name().unwrap(), "plan-2.html");
        assert_eq!(fs::read_to_string(&first).unwrap(), "one");
        fs::remove_dir_all(&folder).unwrap();
    }
}
