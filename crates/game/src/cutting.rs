//! The cutting half of plans, from where each part came from (`woodsim::Origin`): the lumber to
//! buy, where each part comes out of it, and every cut that made them, with the saw's settings,
//! in the order they were made. Cuts on wood that didn't end up in the project are left out.

use std::{fmt::Write as _, sync::Arc};

use woodsim::{Assembly, Axis, Board, Length, Lie, Member, Operation, Origin, Part, Source, Tool};

use crate::{
    inventory::LUMBER,
    shop::{describe_kerf, describe_miter, describe_size, measured},
};

/// The lumber a project came out of, and the cuts that made its parts.
pub struct Cutting {
    pub stock: Vec<Stock>,
    pub cuts: Vec<CutStep>,
}

/// A board of new lumber that parts came out of.
pub struct Stock {
    pub origin: Origin,
    /// Its length, width, and thickness.
    pub size: [Length; 3],
    /// The members of the assembly that came out of it.
    pub members: Vec<usize>,
}

/// One pass of a saw that made parts: the wood it cut, and what it left that went on into the
/// project.
pub struct CutStep {
    /// When it happened: the pass as it was first worked out, so it sorts where it was made.
    pub number: u64,
    /// The saw and its settings, as the pass ended up: a pass the saw worked out again as the
    /// wood moved further, or the blade went deeper, is one pass.
    pub operation: Arc<Operation>,
    /// The wood it cut: a board, or each board of a glue-up it went through.
    pub inputs: Vec<Origin>,
    /// What it left that went on, and the members each led to.
    pub outputs: Vec<(Origin, Vec<usize>)>,
}

impl Cutting {
    pub fn of(assembly: &Assembly) -> Cutting {
        let mut stock: Vec<Stock> = Vec::new();
        let mut cuts: Vec<CutStep> = Vec::new();
        for (index, member) in assembly.members().iter().enumerate() {
            let root = member.board.origin().stock();
            match stock.iter_mut().find(|stock| stock.origin.same(root)) {
                Some(stock) => stock.members.push(index),
                None => {
                    let Source::Stock {
                        length,
                        width,
                        thickness,
                    } = *root.source()
                    else {
                        unreachable!("the stock a board came from is new lumber");
                    };
                    stock.push(Stock {
                        origin: root.clone(),
                        size: [length, width, thickness],
                        members: vec![index],
                    });
                }
            }
            // Back through every cut it came from.
            let mut origin = member.board.origin();
            while let Source::Sawn {
                operation, from, ..
            } = origin.source()
            {
                // The saw works a pass out afresh as the board moves, and the miter saw as the
                // blade goes deeper, each time from the same board: that's still one pass.
                let step = match cuts
                    .iter_mut()
                    .position(|cut| cut.inputs.iter().any(|input| input.same(from)))
                    .or_else(|| {
                        cuts.iter()
                            .position(|cut| Arc::ptr_eq(&cut.operation, operation))
                    }) {
                    Some(step) => step,
                    None => {
                        cuts.push(CutStep {
                            number: operation.number,
                            operation: Arc::clone(operation),
                            inputs: Vec::new(),
                            outputs: Vec::new(),
                        });
                        cuts.len() - 1
                    }
                };
                let cut = &mut cuts[step];
                if operation.number > cut.operation.number {
                    cut.operation = Arc::clone(operation);
                }
                cut.number = cut.number.min(operation.number);
                if !cut.inputs.iter().any(|input| input.same(from)) {
                    cut.inputs.push(from.clone());
                }
                match cut.outputs.iter_mut().find(|(out, _)| out.same(origin)) {
                    Some((_, members)) => members.push(index),
                    None => cut.outputs.push((origin.clone(), vec![index])),
                }
                origin = from;
            }
        }
        cuts.sort_by_key(|cut| cut.number);
        Cutting { stock, cuts }
    }

    /// The cut that left `origin`, if a cut did.
    fn made(&self, origin: &Origin) -> Option<usize> {
        self.cuts
            .iter()
            .position(|cut| cut.outputs.iter().any(|(out, _)| out.same(origin)))
    }

    /// The cut that cuts `origin` next, if one does.
    fn cutting(&self, origin: &Origin) -> Option<usize> {
        self.cuts
            .iter()
            .position(|cut| cut.inputs.iter().any(|input| input.same(origin)))
    }

    /// A cut in words: the saw and how it's set, what it cuts, and what to keep, with `step`
    /// giving the number each cut has on the page and `labels` what each member is called.
    pub fn describe(
        &self,
        cut: usize,
        members: &[Member],
        labels: &[String],
        step: &dyn Fn(usize) -> usize,
    ) -> String {
        let cut_step = &self.cuts[cut];
        let tool = cut_step.operation.tool;
        // A slot that didn't cut the board apart leaves it whole.
        let slotted = cut_step
            .outputs
            .iter()
            .find_map(|(out, _)| match out.source() {
                Source::Sawn {
                    part: Part::Whole,
                    kerf,
                    ..
                } => Some((out.replay(), *kerf)),
                _ => None,
            });
        let what = match tool {
            Tool::TableSaw {
                lie: Lie::GlueUp(_),
                ..
            }
            | Tool::MiterSaw {
                lie: Lie::GlueUp(_),
                ..
            } => "the glue-up".to_string(),
            _ => match cut_step.inputs.first() {
                Some(input) => self.name_input(input, step),
                None => "the board".to_string(),
            },
        };
        let mut words = format!("{}: ", settings(tool));
        // What a member that came out of `out` finished as is called.
        let finished = |out: &Origin, led_to: &[usize]| -> Vec<String> {
            led_to
                .iter()
                .filter(|&&member| members[member].board.origin().same(out))
                .map(|&member| labels[member].clone())
                .collect()
        };
        if let Some((board, kerf)) = &slotted {
            let _ = write!(words, "in {what}, cut {}.", describe_kerf(board, kerf));
            if let Some((out, led_to)) = cut_step.outputs.first() {
                match self.cutting(out) {
                    Some(next) => {
                        let _ = write!(words, " Keep it for step {}.", step(next));
                    }
                    None => {
                        let _ = write!(words, " That's {}.", join(&finished(out, led_to)));
                    }
                }
            }
            return words;
        }
        let _ = write!(words, "{} {what}.", verb(tool));
        // What to keep: parts by name, and anything that goes on to another cut.
        let mut kept: Vec<String> = Vec::new();
        for (out, led_to) in &cut_step.outputs {
            let parts = finished(out, led_to);
            if !parts.is_empty() {
                kept.extend(parts);
            } else if let Some(next) = self.cutting(out) {
                kept.push(format!(
                    "the {} for step {}",
                    describe_size(&out.replay()),
                    step(next)
                ));
            }
        }
        if !kept.is_empty() {
            let _ = write!(words, " Keep {}.", join(&kept));
        }
        words
    }

    /// What a cut cuts, when it's one board: new lumber, or what an earlier step left.
    fn name_input(&self, input: &Origin, step: &dyn Fn(usize) -> usize) -> String {
        if let Source::Stock {
            length,
            width,
            thickness,
        } = *input.source()
        {
            return format!("a new {}", lumber_name([length, width, thickness]));
        }
        let size = describe_size(&input.replay());
        match self.made(input) {
            Some(earlier) => format!("the {size} from step {}", step(earlier)),
            None => format!("the {size}"),
        }
    }

    /// What to buy: each size of new lumber, and how many.
    pub fn buy_list(&self) -> String {
        let mut sizes: Vec<([Length; 3], usize)> = Vec::new();
        for stock in &self.stock {
            match sizes.iter_mut().find(|(size, _)| *size == stock.size) {
                Some((_, count)) => *count += 1,
                None => sizes.push((stock.size, 1)),
            }
        }
        let mut list = String::from(
            "<section>\n<h2>What to buy</h2>\n<table>\n<thead><tr><th>Lumber</th>\
             <th>Qty</th></tr></thead>\n<tbody>\n",
        );
        for (size, count) in sizes {
            let _ = writeln!(
                list,
                "<tr><td>{}</td><td>{count}</td></tr>",
                lumber_name(size)
            );
        }
        list.push_str("</tbody>\n</table>\n</section>\n");
        list
    }

    /// Each board of lumber drawn from above, with the parts that come out of it where they
    /// were in it, and what's left over hatched.
    pub fn diagrams(&self, members: &[Member], labels: &[String]) -> String {
        let mut section = String::from("<section>\n<h2>Cutting diagrams</h2>\n");
        for (index, stock) in self.stock.iter().enumerate() {
            let _ = writeln!(
                section,
                "<figure class=\"part\"><figcaption><b>Board {}</b> {}</figcaption>{}</figure>",
                index + 1,
                lumber_name(stock.size),
                diagram(stock, index, members, labels)
            );
        }
        section.push_str("</section>\n");
        section
    }
}

/// The saw and how it's set for a pass, the way you'd set it up.
fn settings(tool: Tool) -> String {
    match tool {
        Tool::TableSaw { saw, lie, .. } => {
            let guide = match saw.miter_gauge {
                Some(stop) => format!("miter gauge, stop at {}", measured(stop)),
                None => format!("rip fence at {}", measured(saw.fence)),
            };
            let edge = match lie {
                Lie::Board(lie) if lie.on_edge() => ", the board on edge",
                _ => "",
            };
            format!(
                "Table saw, {guide}, blade {} high{edge}",
                measured(saw.height)
            )
        }
        Tool::MiterSaw {
            saw,
            lie,
            length,
            bottom,
        } => {
            let angle = match describe_miter(saw.miter).as_str() {
                "square" => "square".to_string(),
                swung => format!("at {swung}"),
            };
            let depth = if bottom > Length::ZERO {
                format!(", depth stop {} above the table", measured(bottom))
            } else {
                String::new()
            };
            let edge = match lie {
                Lie::Board(lie) if lie.on_edge() => ", the board on edge",
                _ => "",
            };
            format!(
                "Miter saw {angle}, stop at {}{depth}{edge}",
                measured(length)
            )
        }
    }
}

/// What a pass does to the wood: rips it, crosscuts it, resaws it, or cuts it to length.
fn verb(tool: Tool) -> &'static str {
    match tool {
        Tool::TableSaw {
            lie: Lie::Board(lie),
            ..
        } if lie.on_edge() => "Resaw",
        Tool::TableSaw {
            lie: Lie::Board(lie),
            ..
        } if lie.along().0 == Axis::Width => "Crosscut",
        Tool::TableSaw {
            saw:
                woodsim::TableSaw {
                    miter_gauge: Some(_),
                    ..
                },
            ..
        } => "Crosscut",
        Tool::TableSaw { .. } => "Rip",
        Tool::MiterSaw { .. } => "Cut",
    }
}

/// Lumber by the name it's sold under, if it's one the rack stocks, and its size: "1x4, 3/4" x
/// 3-1/2" x 24"".
pub fn lumber_name([length, width, thickness]: [Length; 3]) -> String {
    let size = format!(
        "{} x {} x {}",
        measured(thickness),
        measured(width),
        measured(length)
    );
    match LUMBER
        .iter()
        .find(|(_, t, w)| (*t, *w) == (thickness, width))
    {
        Some((name, ..)) => format!("{name}, {size}"),
        None => format!("{size} board"),
    }
}

/// "A1", "A1 and A2", "A1, A2, and B".
fn join(names: &[impl AsRef<str>]) -> String {
    match names {
        [] => String::new(),
        [one] => one.as_ref().to_string(),
        [a, b] => format!("{} and {}", a.as_ref(), b.as_ref()),
        [rest @ .., last] => format!(
            "{}, and {}",
            rest.iter()
                .map(|name| name.as_ref())
                .collect::<Vec<_>>()
                .join(", "),
            last.as_ref()
        ),
    }
}

/// A board of lumber from above, `l` across the page and `w` up it, with its parts drawn where
/// they came out of it and labeled.
fn diagram(stock: &Stock, index: usize, members: &[Member], labels: &[String]) -> String {
    const MOST_WIDE: f64 = 620.0;
    const MOST_PER_INCH: f64 = 24.0;
    const MARGIN: f64 = 8.0;
    let nm = |length: Length| length.as_nm() as f64;
    let [length, width, thickness] = stock.size.map(nm);
    let scale = (MOST_WIDE / length).min(MOST_PER_INCH / 25_400_000.0);
    let (long, wide) = (length * scale, width * scale);
    let at = |l: f64, w: f64| [MARGIN + l * scale, MARGIN + wide - w * scale];
    let (svg_width, svg_height) = (long + 2.0 * MARGIN, wide + 2.0 * MARGIN);
    let mut svg = format!(
        "<svg class=\"diagram\" viewBox=\"0 0 {svg_width:.0} {svg_height:.0}\" \
         width=\"{svg_width:.0}\" height=\"{svg_height:.0}\" xmlns=\"http://www.w3.org/2000/svg\">\
         <defs><pattern id=\"waste-{index}\" width=\"6\" height=\"6\" \
         patternUnits=\"userSpaceOnUse\" patternTransform=\"rotate(45)\">\
         <rect width=\"6\" height=\"6\" fill=\"#f3ece0\"/>\
         <line x1=\"0\" y1=\"0\" x2=\"0\" y2=\"6\" stroke=\"#d8cbb4\" stroke-width=\"2\"/>\
         </pattern></defs>\
         <rect class=\"stock\" x=\"{MARGIN}\" y=\"{MARGIN}\" width=\"{long:.1}\" \
         height=\"{wide:.1}\" fill=\"url(#waste-{index})\"/>"
    );
    for &member in &stock.members {
        let board: &Board = &members[member].board;
        let [l0, w0, t0] = board.origin().at_on_stock().map(nm);
        let mut middle = [0.0, 0.0];
        let mut corners = 0.0;
        let mut path = String::new();
        for region in board.face() {
            for (k, (l, w)) in region.corners().into_iter().enumerate() {
                let [x, y] = at(l0 + nm(l), w0 + nm(w));
                let _ = write!(path, "{}{x:.1} {y:.1}", if k == 0 { "M" } else { "L" });
                middle = [middle[0] + x, middle[1] + y];
                corners += 1.0;
            }
            path.push('Z');
        }
        let [x, y] = middle.map(|sum| sum / corners);
        // A part resawn from the board's thickness says which layer it is.
        let layer = if nm(board.thickness()) < thickness - 1_000.0 {
            format!(" ({} up)", measured(Length::from_nm(t0.round() as i64)))
        } else {
            String::new()
        };
        let _ = write!(
            svg,
            "<path class=\"cut-part\" d=\"{path}\"/>\
             <text x=\"{x:.1}\" y=\"{y:.1}\" text-anchor=\"middle\" \
             dominant-baseline=\"central\">{}{layer}</text>",
            labels[member]
        );
    }
    svg.push_str("</svg>");
    svg
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lumber_goes_by_its_name_when_it_has_one() {
        let inches = Length::from_inches;
        let fraction = Length::from_inch_fraction;
        assert_eq!(
            lumber_name([inches(24), fraction(7, 2), fraction(3, 4)]),
            "1x4, 3/4\" x 3-1/2\" x 24\""
        );
        assert_eq!(
            lumber_name([inches(48), fraction(3, 2), fraction(3, 4)]),
            "3/4\" x 1-1/2\" x 48\" board"
        );
        assert_eq!(join(&["A1"]), "A1");
        assert_eq!(join(&["A1", "B"]), "A1 and B");
        assert_eq!(join(&["A1", "A2", "B"]), "A1, A2, and B");
    }
}
