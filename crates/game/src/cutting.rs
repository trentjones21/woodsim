//! The cutting half of plans, from where each part came from (`woodsim::Origin`): the lumber to
//! buy, where each part comes out of it, and every cut that made them, with the saw's settings,
//! in the order they were made. Cuts on wood that didn't end up in the project are left out.

use std::{fmt::Write as _, sync::Arc};

use woodsim::{
    Assembly, Axis, Board, Length, Lie, Member, Operation, Orientation, Origin, Part, Side, Source,
    Tool,
};

use crate::{
    inventory::LUMBER,
    join::name,
    shop::{describe_kerf, describe_miter, describe_size, measured},
};

/// The lumber a project came out of, and the cuts that made its parts.
pub struct Cutting {
    pub stock: Vec<Stock>,
    pub cuts: Vec<CutStep>,
    /// How the cuts are listed, in order: some one at a time, some together where the same
    /// work was done again.
    pub listings: Vec<Listing>,
}

/// What the page lists for cutting, by the cuts' places in `Cutting::cuts`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Listing {
    /// One cut.
    Cut(usize),
    /// The same pass again and again along a board, each taking one more piece off it: strips
    /// ripped one after another, or pieces cut one after another to the same length.
    Passes(Vec<usize>),
    /// The cuts of `first`, listed on their own just before, made again on what's left, once
    /// for each of `runs`.
    Again {
        first: Vec<usize>,
        runs: Vec<Vec<usize>>,
    },
    /// The same cuts made to each of several pieces: each piece's cuts, in order.
    EachOf(Vec<Vec<usize>>),
}

impl Listing {
    /// The cuts it lists. The cuts `Again` makes again are listed on their own.
    pub fn cuts(&self) -> Vec<usize> {
        match self {
            Listing::Cut(cut) => vec![*cut],
            Listing::Passes(cuts) => cuts.clone(),
            Listing::Again { runs, .. } => runs.concat(),
            Listing::EachOf(tails) => tails.concat(),
        }
    }
}

/// What tells two cuts apart when looking for work done again: the saw and its settings (not
/// how far a table saw pass went, which is just the board's length), and whether the measured
/// piece was kept.
type Signature = (Tool, bool);

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
        let finished: Vec<&Origin> = assembly
            .members()
            .iter()
            .map(|member| member.board.origin())
            .collect();
        merge_nibbles(&mut cuts, &finished);
        let mut cutting = Cutting {
            stock,
            cuts,
            listings: Vec::new(),
        };
        cutting.listings = cutting.list(&finished);
        cutting
    }

    /// Lists the cuts: runs along a board that do the same work again together, the same cuts
    /// made to several pieces together, and the rest one at a time; in the order each began.
    fn list(&self, finished: &[&Origin]) -> Vec<Listing> {
        let count = self.cuts.len();
        let mut claimed = vec![false; count];
        let mut listings = Vec::new();
        for chain in self.chains() {
            let signatures: Vec<Signature> = chain.iter().map(|&cut| self.signature(cut)).collect();
            let Some((start, period, times)) = repeats(&signatures) else {
                continue;
            };
            let blocks: Vec<Vec<usize>> = (0..times)
                .map(|k| chain[start + k * period..start + (k + 1) * period].to_vec())
                .collect();
            for &cut in blocks.concat().iter() {
                claimed[cut] = true;
            }
            if period == 1 {
                listings.push(Listing::Passes(blocks.concat()));
            } else {
                listings.extend(blocks[0].iter().map(|&cut| Listing::Cut(cut)));
                listings.push(Listing::Again {
                    first: blocks[0].clone(),
                    runs: blocks[1..].to_vec(),
                });
            }
        }
        // Each part's own cuts, back from the one that finished it as far as nothing else came
        // of them.
        let mut tails: Vec<Vec<usize>> = Vec::new();
        for (member, origin) in finished.iter().enumerate() {
            let only = |cut: usize| {
                self.cuts[cut]
                    .outputs
                    .iter()
                    .all(|(_, led_to)| led_to.as_slice() == [member])
            };
            let Some(mut cut) = self.made(origin).filter(|&cut| !claimed[cut] && only(cut)) else {
                continue;
            };
            let mut tail = vec![cut];
            while let Some(earlier) = self.cuts[cut]
                .inputs
                .first()
                .and_then(|input| self.made(input))
                .filter(|&earlier| !claimed[earlier] && only(earlier))
            {
                tail.insert(0, earlier);
                cut = earlier;
            }
            tails.push(tail);
        }
        while let Some(tail) = tails.pop() {
            let recipe: Vec<Signature> = tail.iter().map(|&cut| self.signature(cut)).collect();
            let (mut same, others): (Vec<_>, Vec<_>) = tails.into_iter().partition(|other| {
                other
                    .iter()
                    .map(|&cut| self.signature(cut))
                    .collect::<Vec<_>>()
                    == recipe
            });
            tails = others;
            if same.is_empty() {
                continue;
            }
            same.insert(0, tail);
            same.sort_by_key(|tail| self.cuts[tail[0]].number);
            for &cut in same.concat().iter() {
                claimed[cut] = true;
            }
            listings.push(Listing::EachOf(same));
        }
        listings.extend((0..count).filter(|&cut| !claimed[cut]).map(Listing::Cut));
        let first = |listing: &Listing| match listing {
            Listing::Again { runs, .. } => runs[0][0],
            other => other.cuts()[0],
        };
        listings.sort_by_key(|listing| self.cuts[first(listing)].number);
        listings
    }

    /// Runs of cuts along a board: each cut's offcut cut next.
    fn chains(&self) -> Vec<Vec<usize>> {
        let next = |cut: usize| {
            self.cuts[cut]
                .outputs
                .iter()
                .filter(|(out, _)| out.side() == Some(Side::Offcut))
                .find_map(|(out, _)| self.cutting(out))
        };
        let nexts: Vec<Option<usize>> = (0..self.cuts.len()).map(next).collect();
        (0..self.cuts.len())
            .filter(|cut| !nexts.contains(&Some(*cut)))
            .map(|start| {
                let mut chain = vec![start];
                while let Some(next) = nexts[*chain.last().unwrap()] {
                    chain.push(next);
                }
                chain
            })
            .collect()
    }

    fn signature(&self, cut: usize) -> Signature {
        let mut tool = self.cuts[cut].operation.tool;
        if let Tool::TableSaw { travel, .. } = &mut tool {
            *travel = (Length::ZERO, Length::ZERO);
        }
        let measured = self.cuts[cut]
            .outputs
            .iter()
            .any(|(out, _)| out.side() == Some(Side::Measured));
        (tool, measured)
    }

    /// The listing a cut is in.
    pub fn listing_of(&self, cut: usize) -> Option<usize> {
        self.listings
            .iter()
            .position(|listing| listing.cuts().contains(&cut))
    }

    /// When a listing's first cut began, to put it in order among the joints.
    pub fn number(&self, listing: usize) -> u64 {
        self.listings[listing]
            .cuts()
            .iter()
            .map(|&cut| self.cuts[cut].number)
            .min()
            .unwrap_or(0)
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

    /// A listing in words, as HTML: the saw and how it's set, what it cuts, and what to keep,
    /// with `step` giving the number each listing has on the page and `labels` what each
    /// member is called.
    pub fn describe(
        &self,
        listing: usize,
        members: &[Member],
        labels: &[String],
        step: &dyn Fn(usize) -> usize,
    ) -> String {
        let say = Say {
            cutting: self,
            members,
            labels,
            step,
        };
        match &self.listings[listing] {
            Listing::Cut(cut) => format!("<p>{}</p>", say.cut(*cut)),
            Listing::Passes(cuts) => format!("<p>{}</p>", say.passes(cuts)),
            Listing::Again { first, runs } => format!("<p>{}</p>", say.again(first, runs)),
            Listing::EachOf(tails) => say.each_of(tails),
        }
    }

    /// A short name for a board on its way into the project: the lumber it was, a strip a rip
    /// left, a piece cut to length, or the rest of a board an earlier cut cut: "the 3/4" strip",
    /// "the rest of the 1x4".
    fn name(&self, origin: &Origin) -> String {
        if let Source::Stock {
            length,
            width,
            thickness,
        } = *origin.source()
        {
            return match lumber_name([length, width, thickness]).split_once(", ") {
                Some((name, _)) => format!("the {name}"),
                None => "the board".to_string(),
            };
        }
        let board = origin.replay();
        let made = self.made(origin).map(|cut| &self.cuts[cut]);
        let Some((cut, input)) = made.and_then(|cut| Some((cut, cut.inputs.first()?))) else {
            return format!("the {}", describe_size(&board));
        };
        let tool = cut.operation.tool;
        match origin.side() {
            // A slot doesn't change what it's called.
            Some(Side::Whole) => self.name(input),
            Some(Side::Measured) if is_glue_up(tool) => format!("the {}", describe_size(&board)),
            Some(Side::Measured) if is_resaw(tool) => {
                format!("the {} slice", measured(board.thickness()))
            }
            Some(Side::Measured) if is_crosscut(tool) => {
                format!("the {} piece", measured(board.length()))
            }
            Some(Side::Measured) => format!("the {} strip", measured(board.width())),
            _ => {
                let name = self.name(input);
                let base = name.strip_prefix("the rest of ").unwrap_or(&name);
                format!("the rest of {base}")
            }
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

/// The saw and how it's set for a pass, and how the wood lies on it, the way you'd set it up.
fn settings(step: &CutStep) -> String {
    let tool = step.operation.tool;
    let lying = lying(step);
    match tool {
        Tool::TableSaw { saw, .. } => {
            let guide = match saw.miter_gauge {
                Some(stop) if saw.gauge_angle != woodsim::Angle::ZERO => format!(
                    "miter gauge at {}, stop at {}",
                    describe_miter(saw.gauge_angle),
                    measured(stop)
                ),
                Some(stop) => format!("miter gauge, stop at {}", measured(stop)),
                None => format!("rip fence at {}", measured(saw.fence)),
            };
            let tilt = if saw.tilt == woodsim::Angle::ZERO {
                String::new()
            } else {
                format!(" and tilted {}", saw.tilt)
            };
            format!(
                "Table saw, {guide}, blade {} high{tilt}{lying}",
                measured(saw.height)
            )
        }
        Tool::MiterSaw {
            saw,
            length,
            bottom,
            ..
        } => {
            let angle = match describe_miter(saw.miter).as_str() {
                "square" => ", square".to_string(),
                swung => format!(" at {swung}"),
            };
            let depth = if bottom > Length::ZERO {
                format!(", depth stop {} above the table", measured(bottom))
            } else {
                String::new()
            };
            format!(
                "Miter saw{angle}, stop at {}{depth}{lying}",
                measured(length)
            )
        }
    }
}

/// How the wood lay on the saw, if not flat the way it came: ", the board on edge", or, rested
/// on a surface at an angle, ", the board resting on its beveled edge, its bottom face to the
/// fence".
fn lying(step: &CutStep) -> String {
    let (lie, glue_up) = match step.operation.tool {
        Tool::TableSaw { lie, .. } | Tool::MiterSaw { lie, .. } => match lie {
            Lie::Board(lie) if lie.is_square() && lie.on_edge() => {
                return ", the board on edge".to_string();
            }
            Lie::Board(lie) if lie.is_square() && lie.on_end() => {
                return ", the board on end".to_string();
            }
            Lie::Board(lie) if lie.is_square() => return String::new(),
            Lie::Board(lie) => (lie, false),
            Lie::GlueUp(rotation) => {
                let lie = Orientation::from_rotation(rotation);
                if lie.is_square() {
                    return String::new();
                }
                (lie, true)
            }
        },
    };
    // What it rested on, from the wood it cut.
    let wood = match glue_up {
        false => step
            .inputs
            .first()
            .map(|board| Assembly::of(board.replay())),
        true => None,
    };
    match wood.as_ref().and_then(|wood| wood.rests_on(lie)) {
        Some(((_, down), (_, fence))) => format!(
            ", the board resting on its {}, its {} to the fence",
            name(down),
            name(fence)
        ),
        None if glue_up => ", the glue-up resting at an angle".to_string(),
        None => ", the board resting at an angle".to_string(),
    }
}

/// How each listing is put into words.
struct Say<'a> {
    cutting: &'a Cutting,
    members: &'a [Member],
    labels: &'a [String],
    step: &'a dyn Fn(usize) -> usize,
}

impl Say<'_> {
    /// One cut: "Miter saw at 45° right, stop at 4": Cut A1 off the rest of the 3/4" strip
    /// from step 2. Keep the rest of the 3/4" strip for step 4."
    fn cut(&self, cut: usize) -> String {
        let what = self.what(cut);
        let mut words = match self.turn(cut, &what) {
            Some(turn) => format!(
                "{}: {}, then {}.",
                settings(&self.cutting.cuts[cut]),
                capital(&turn),
                self.action(cut, "it", false)
            ),
            None => format!(
                "{}: {}.",
                settings(&self.cutting.cuts[cut]),
                capital(&self.action(cut, &what, false))
            ),
        };
        words.push_str(&self.keep(cut));
        words
    }

    /// Passes one after another along a board: "Table saw, rip fence at 2-3/8", blade 1"
    /// high: Rip four 2-3/8" strips off a new 1x12. Keep them for step 2."
    fn passes(&self, cuts: &[usize]) -> String {
        let first = cuts[0];
        let tool = self.cutting.cuts[first].operation.tool;
        let what = match setting(tool) {
            Some(size) if is_resaw(tool) => format!("{size} slices"),
            Some(size) if is_crosscut(tool) => format!("{size} pieces"),
            Some(size) => format!("{size} strips"),
            None => "pieces".to_string(),
        };
        let mut words = format!(
            "{}: {} {} {what} off {}.",
            settings(&self.cutting.cuts[first]),
            verb(tool),
            count(cuts.len()),
            self.what(first)
        );
        // What each pass took off: parts, or what's kept for later steps.
        let mut parts = Vec::new();
        let mut later = Vec::new();
        for &cut in cuts {
            for (out, led_to) in &self.cutting.cuts[cut].outputs {
                if out.side() != Some(Side::Measured) {
                    continue;
                }
                parts.extend(self.finished(out, led_to));
                if let Some(next) = self.later(out) {
                    later.push(next);
                }
            }
        }
        later.sort_unstable();
        later.dedup();
        if !later.is_empty() {
            let steps: Vec<String> = later.iter().map(|&next| next.to_string()).collect();
            let s = if steps.len() == 1 { "" } else { "s" };
            let _ = write!(words, " Keep them for step{s} {}.", join(&steps));
        }
        if !parts.is_empty() {
            let _ = write!(words, " That makes {}.", join(&parts));
        }
        // What's left after the last pass, if it goes on.
        let last = *cuts.last().unwrap();
        for (out, _) in &self.cutting.cuts[last].outputs {
            if out.side() == Some(Side::Offcut)
                && let Some(next) = self.later(out)
            {
                let _ = write!(words, " Keep {} for step {next}.", self.cutting.name(out));
            }
        }
        words
    }

    /// A run of cuts made again on what's left: "Repeat steps 2 and 3 three more times on the
    /// rest of the 3/4" strip, making A2, A3, and A4."
    fn again(&self, first: &[usize], runs: &[Vec<usize>]) -> String {
        let steps: Vec<String> = first
            .iter()
            .filter_map(|&cut| self.cutting.listing_of(cut))
            .map(|listing| (self.step)(listing).to_string())
            .collect();
        let s = if steps.len() == 1 { "" } else { "s" };
        let times = match runs.len() {
            1 => "once more".to_string(),
            2 => "twice more".to_string(),
            n => format!("{} more times", count(n)),
        };
        let on = self.cutting.cuts[runs[0][0]]
            .inputs
            .first()
            .map_or("what's left".to_string(), |input| self.cutting.name(input));
        let parts: Vec<String> = runs
            .concat()
            .iter()
            .flat_map(|&cut| {
                self.cutting.cuts[cut]
                    .outputs
                    .iter()
                    .flat_map(|(out, led_to)| self.finished(out, led_to))
                    .collect::<Vec<_>>()
            })
            .collect();
        let mut words = format!("Repeat step{s} {} {times} on {on}", join(&steps));
        if !parts.is_empty() {
            let _ = write!(words, ", making {}", join(&parts));
        }
        words.push('.');
        let last = *runs.concat().last().unwrap();
        for (out, _) in &self.cutting.cuts[last].outputs {
            if out.side() == Some(Side::Offcut)
                && let Some(next) = self.later(out)
            {
                let _ = write!(words, " Keep what's left for step {next}.");
            }
        }
        words
    }

    /// The same cuts to several pieces: "For each 2-3/8" strip from step 1, to make A1, A2,
    /// A3, and A4:" and then each cut, once.
    fn each_of(&self, tails: &[Vec<usize>]) -> String {
        let first = &tails[0];
        let input = self.cutting.cuts[first[0]].inputs.first();
        let each = input.map_or("each piece".to_string(), |input| {
            let name = self.cutting.name(input);
            format!("each {}", name.strip_prefix("the ").unwrap_or(&name))
        });
        let from: Vec<usize> = tails
            .iter()
            .filter_map(|tail| self.cutting.cuts[tail[0]].inputs.first())
            .filter_map(|input| self.cutting.made(input))
            .filter_map(|cut| self.cutting.listing_of(cut))
            .map(|listing| (self.step)(listing))
            .collect();
        let from = match from.first() {
            Some(&step) if from.iter().all(|&other| other == step) => {
                format!(" from step {step}")
            }
            _ => String::new(),
        };
        let parts: Vec<String> = tails
            .iter()
            .flat_map(|tail| {
                let last = *tail.last().unwrap();
                self.cutting.cuts[last]
                    .outputs
                    .iter()
                    .flat_map(|(out, led_to)| self.finished(out, led_to))
                    .collect::<Vec<_>>()
            })
            .collect();
        let mut html = format!(
            "<p>For {each}{from}, to make {}:</p><ol class=\"recipe\">",
            join(&parts)
        );
        for &cut in first {
            let line = match self.turn(cut, "it") {
                Some(turn) => format!("{}, then {}", capital(&turn), self.action(cut, "it", true)),
                None => capital(&self.action(cut, "it", true)),
            };
            let _ = write!(
                html,
                "<li>{}: {line}.</li>",
                settings(&self.cutting.cuts[cut])
            );
        }
        html.push_str("</ol>");
        html
    }

    /// What a cut cuts, by name: new lumber, what an earlier step left, or the glue-up.
    fn what(&self, cut: usize) -> String {
        let cut_step = &self.cutting.cuts[cut];
        if is_glue_up(cut_step.operation.tool) {
            return "the glue-up".to_string();
        }
        let Some(input) = cut_step.inputs.first() else {
            return "the board".to_string();
        };
        if let Source::Stock {
            length,
            width,
            thickness,
        } = *input.source()
        {
            return format!("a new {}", lumber_name([length, width, thickness]));
        }
        let name = self.cutting.name(input);
        match self.later_of(input) {
            Some(step) => format!("{name} from step {step}"),
            None => name,
        }
    }

    /// What a cut does to `it`: cuts parts off it by name (or, `generic`, to size), trims its
    /// end off, cuts it to a size, or slots it.
    fn action(&self, cut: usize, it: &str, generic: bool) -> String {
        let cut_step = &self.cutting.cuts[cut];
        let tool = cut_step.operation.tool;
        let verb = verb(tool).to_lowercase();
        if let Some((board, kerf)) =
            cut_step
                .outputs
                .iter()
                .find_map(|(out, _)| match out.source() {
                    Source::Sawn {
                        part: Part::Whole,
                        kerf,
                        ..
                    } => Some((out.replay(), *kerf)),
                    _ => None,
                })
        {
            return format!("cut {} in {it}", describe_kerf(&board, &kerf));
        }
        let parts: Vec<String> = cut_step
            .outputs
            .iter()
            .flat_map(|(out, led_to)| self.finished(out, led_to))
            .collect();
        let measured_kept = cut_step
            .outputs
            .iter()
            .any(|(out, _)| out.side() == Some(Side::Measured));
        let to = setting(tool).map_or(String::new(), |size| format!(" to {size}"));
        if !parts.is_empty() && !generic {
            format!("{verb} {} off {it}", join(&parts))
        } else if measured_kept && generic {
            format!("{verb} {it}{to}")
        } else if measured_kept {
            format!("{verb} {it}")
        } else if is_crosscut(tool) {
            format!("trim the end off {it}")
        } else {
            format!("{verb} {it}")
        }
    }

    /// What a cut leaves that goes on to a later step, to keep: " Keep the rest of the 3/4"
    /// strip for step 4."
    fn keep(&self, cut: usize) -> String {
        let kept: Vec<String> = self.cutting.cuts[cut]
            .outputs
            .iter()
            .filter(|(out, led_to)| self.finished(out, led_to).is_empty())
            .filter_map(|(out, _)| {
                let next = self.later(out)?;
                Some(format!("{} for step {next}", self.cutting.name(out)))
            })
            .collect();
        match kept.is_empty() {
            true => String::new(),
            false => format!(" Keep {}.", join(&kept)),
        }
    }

    /// How the wood's turned since the cut before on the same saw, if it is: "turn the 14-1/4"
    /// piece from step 5 end for end".
    fn turn(&self, cut: usize, it: &str) -> Option<String> {
        let cut_step = &self.cutting.cuts[cut];
        let input = cut_step.inputs.first()?;
        let before = &self.cutting.cuts[self.cutting.made(input)?];
        let (Some(was), Some(now)) = (
            board_lie(before.operation.tool),
            board_lie(cut_step.operation.tool),
        ) else {
            return None;
        };
        let same_saw = matches!(
            (before.operation.tool, cut_step.operation.tool),
            (Tool::MiterSaw { .. }, Tool::MiterSaw { .. })
                | (Tool::TableSaw { .. }, Tool::TableSaw { .. })
        );
        // Resting at an angle, the saw's settings say how it lies.
        if !same_saw || was == now || !was.is_square() || !now.is_square() {
            return None;
        }
        let mut moves = Vec::new();
        let ((was_up, was_down), (now_up, now_down)) = (was.up(), now.up());
        if was_up != now_up {
            moves.push(if now.on_edge() {
                format!("stand {it} on edge")
            } else if now.on_end() {
                format!("stand {it} on end")
            } else {
                format!("lay {it} flat")
            });
        } else if was_down != now_down {
            moves.push(format!("flip {it} over"));
        }
        let ((was_along, was_back), (now_along, now_back)) = (was.along(), now.along());
        if was_along != now_along {
            moves.push(format!("turn {it} a quarter turn"));
        } else if was_back != now_back {
            moves.push(format!("turn {it} end for end"));
        }
        // Later moves say "it".
        let moves: Vec<String> = moves
            .into_iter()
            .enumerate()
            .map(|(k, phrase)| {
                if k == 0 {
                    phrase
                } else {
                    phrase.replacen(it, "it", 1)
                }
            })
            .collect();
        (!moves.is_empty()).then(|| moves.join(" and "))
    }

    /// The members of `led_to` that finished as `out`, by name.
    fn finished(&self, out: &Origin, led_to: &[usize]) -> Vec<String> {
        finished_as(out, led_to, self.members)
            .into_iter()
            .map(|member| self.labels[member].clone())
            .collect()
    }

    /// The page's step that cuts `out` next, if one does.
    fn later(&self, out: &Origin) -> Option<usize> {
        let cut = self.cutting.cutting(out)?;
        Some((self.step)(self.cutting.listing_of(cut)?))
    }

    /// The page's step that made `input`, if one did.
    fn later_of(&self, input: &Origin) -> Option<usize> {
        let cut = self.cutting.made(input)?;
        Some((self.step)(self.cutting.listing_of(cut)?))
    }
}

/// How a board lay for a pass, if it was a board.
fn board_lie(tool: Tool) -> Option<Orientation> {
    match tool {
        Tool::TableSaw {
            lie: Lie::Board(lie),
            ..
        }
        | Tool::MiterSaw {
            lie: Lie::Board(lie),
            ..
        } => Some(lie),
        _ => None,
    }
}

/// What a pass cuts to: the miter saw's stop, the gauge's stop, or the fence.
fn setting(tool: Tool) -> Option<Length> {
    match tool {
        Tool::MiterSaw { length, .. } => Some(measured(length)),
        Tool::TableSaw { saw, .. } => Some(measured(saw.miter_gauge.unwrap_or(saw.fence))),
    }
}

fn is_glue_up(tool: Tool) -> bool {
    matches!(
        tool,
        Tool::TableSaw {
            lie: Lie::GlueUp(_),
            ..
        } | Tool::MiterSaw {
            lie: Lie::GlueUp(_),
            ..
        }
    )
}

/// Whether the table saw's blade went through a board parallel to its faces.
fn is_resaw(tool: Tool) -> bool {
    matches!(tool, Tool::TableSaw { lie: Lie::Board(lie), .. } if lie.across().0 == Axis::Thickness)
}

/// "Rip", from "rip".
fn capital(words: &str) -> String {
    let mut letters = words.chars();
    match letters.next() {
        Some(first) => first.to_uppercase().chain(letters).collect(),
        None => String::new(),
    }
}

/// "two", "three", ... "ten", then numbers.
fn count(n: usize) -> String {
    const NUMBERS: [&str; 11] = [
        "no", "one", "two", "three", "four", "five", "six", "seven", "eight", "nine", "ten",
    ];
    NUMBERS
        .get(n)
        .map_or(n.to_string(), |word| word.to_string())
}

/// Where a run of work repeats in `signatures`: where it starts, how many cuts it is, and how
/// many times it's done in a row; the run that covers the most, and among those the shortest.
fn repeats(signatures: &[Signature]) -> Option<(usize, usize, usize)> {
    for start in 0..signatures.len() {
        let mut best: Option<(usize, usize)> = None;
        for period in 1..=3 {
            if start + 2 * period > signatures.len() {
                break;
            }
            let block = &signatures[start..start + period];
            let mut times = 1;
            while start + (times + 1) * period <= signatures.len()
                && &signatures[start + times * period..start + (times + 1) * period] == block
            {
                times += 1;
            }
            if times >= 2 && best.is_none_or(|(p, t)| times * period > t * p) {
                best = Some((period, times));
            }
        }
        if let Some((period, times)) = best {
            return Some((start, period, times));
        }
    }
    None
}

/// The members of `led_to` that finished as `out`, with no more cuts.
fn finished_as(out: &Origin, led_to: &[usize], members: &[Member]) -> Vec<usize> {
    led_to
        .iter()
        .copied()
        .filter(|&member| members[member].board.origin().same(out))
        .collect()
}

/// Whether a pass cuts across a board's length, rather than along it.
fn is_crosscut(tool: Tool) -> bool {
    match tool {
        Tool::MiterSaw { .. } => true,
        Tool::TableSaw {
            lie: Lie::Board(lie),
            ..
        } => lie.along().0 == Axis::Width,
        Tool::TableSaw { saw, .. } => saw.miter_gauge.is_some(),
    }
}

/// Cuts that nibbled a board down a little at a time, each a sliver off the same end with the
/// saw set the same way but a little shorter, are their last cut: what matters is where it
/// ended up, not the slivers on the way.
fn merge_nibbles(cuts: &mut Vec<CutStep>, finished: &[&Origin]) {
    loop {
        let merge = (0..cuts.len()).find_map(|first| {
            let [(out, _)] = cuts[first].outputs.as_slice() else {
                return None;
            };
            if finished.iter().any(|part| part.same(out)) || out.side() != Some(Side::Measured) {
                return None;
            }
            let then = cuts
                .iter()
                .position(|cut| cut.inputs.iter().any(|input| input.same(out)))?;
            nibbles(cuts[first].operation.tool, cuts[then].operation.tool).then_some((first, then))
        });
        let Some((first, then)) = merge else {
            return;
        };
        let earlier = cuts.remove(first);
        let then = &mut cuts[then - usize::from(first < then)];
        then.inputs = earlier.inputs;
        then.number = then.number.min(earlier.number);
    }
}

/// Whether `then`, after `first`, takes a little more off the same end: the same saw set the
/// same way, but to a length, or a width, no greater.
fn nibbles(first: Tool, then: Tool) -> bool {
    match (first, then) {
        (
            Tool::MiterSaw {
                saw: a,
                lie: lie_a,
                length: length_a,
                bottom: bottom_a,
            },
            Tool::MiterSaw {
                saw: b,
                lie: lie_b,
                length: length_b,
                bottom: bottom_b,
            },
        ) => a == b && lie_a == lie_b && bottom_a == bottom_b && length_b <= length_a,
        (
            Tool::TableSaw {
                saw: a, lie: lie_a, ..
            },
            Tool::TableSaw {
                saw: b, lie: lie_b, ..
            },
        ) => {
            lie_a == lie_b
                && (a.blade, a.height, a.gauge_angle, a.tilt)
                    == (b.blade, b.height, b.gauge_angle, b.tilt)
                && match (a.miter_gauge, b.miter_gauge) {
                    (None, None) => b.fence <= a.fence,
                    (Some(stop_a), Some(stop_b)) => stop_b <= stop_a,
                    _ => false,
                }
        }
        _ => false,
    }
}

/// What a pass does to the wood: rips it, crosscuts it, resaws it, or cuts it to length. The
/// table saw's blade stands square to whichever of a board's axes lies across the table:
/// through its thickness it resaws, through its length it crosscuts.
fn verb(tool: Tool) -> &'static str {
    match tool {
        _ if is_resaw(tool) => "Resaw",
        Tool::TableSaw {
            lie: Lie::Board(lie),
            ..
        } if lie.across().0 == Axis::Length => "Crosscut",
        Tool::TableSaw {
            saw:
                woodsim::TableSaw {
                    miter_gauge: Some(_),
                    gauge_angle: woodsim::Angle::ZERO,
                    tilt: woodsim::Angle::ZERO,
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
            for (k, (l, w)) in region.footprint(board.thickness()).into_iter().enumerate() {
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
    fn a_table_saw_pass_is_named_by_the_board_axis_across_the_table() {
        let saw = woodsim::TableSaw {
            blade: woodsim::Blade {
                diameter: Length::from_inches(10),
                kerf: woodsim::Blade::FULL_KERF,
            },
            fence: Length::from_inches(2),
            height: Length::from_inches(1),
            miter_gauge: None,
            gauge_angle: woodsim::Angle::ZERO,
            tilt: woodsim::Angle::ZERO,
        };
        let pass = |lie: Orientation| {
            verb(Tool::TableSaw {
                saw,
                lie: Lie::Board(lie),
                travel: (Length::ZERO, Length::ZERO),
            })
        };
        let flat = Orientation::default();
        assert_eq!(pass(flat), "Rip");
        assert_eq!(pass(flat.turned_left()), "Crosscut");
        assert_eq!(pass(flat.tipped()), "Resaw");
        // On edge and turned, the blade goes through its length.
        assert_eq!(pass(flat.tipped().turned_left()), "Crosscut");
        // On end against the fence, its faces are parallel to the blade.
        assert_eq!(pass(flat.stood()), "Resaw");
        assert_eq!(pass(flat.stood().turned_left()), "Rip");
    }

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
