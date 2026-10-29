//! Wood put into words, for the game's statuses and for plans alike: a board as you'd measure
//! it, with its mitered ends, bevels, tapers, and slots; a cut and what it left; and what each
//! of a board's surfaces is called.

use woodsim::{Angle, Board, Cut, Kerf, Length, Run, Side, Surface};

/// Describes a finished cut, e.g. "Ripped a 3/4" x 6" x 24" piece, plus a ... offcut.", and
/// any strips a slot already in the board set loose.
pub fn describe_cut(verb: &str, cut: &Cut) -> String {
    let piece = describe_size(&cut.piece.board);
    let waste = measured(cut.waste);
    let mut description = match &cut.offcut {
        Some(offcut) => format!(
            "{verb} a {piece} piece, plus a {} offcut. The blade turned {waste} to sawdust.",
            describe_size(&offcut.board)
        ),
        // Nothing was on the measured side, the blade right up against the fence: it took its
        // kerf off the side there.
        None if cut.piece.board.origin().side() == Some(Side::Offcut) => {
            format!("Trimmed {waste} off to sawdust, leaving a {piece} piece.")
        }
        None => {
            format!("{verb} a {piece} piece, and the blade turned the last {waste} to sawdust.")
        }
    };
    let shaped: Vec<String> = [
        describe_miters(&cut.piece.board),
        describe_bevels(&cut.piece.board),
        describe_tapers(&cut.piece.board),
    ]
    .into_iter()
    .flatten()
    .collect();
    if !shaped.is_empty() {
        description.push_str(&format!(" The piece is {}.", shaped.join(" and ")));
    }
    let loose: Vec<String> = cut
        .loose
        .iter()
        .map(|piece| format!("a {}", describe_size(&piece.board)))
        .collect();
    if !loose.is_empty() {
        description.push_str(&format!(
            " The slot already in the board split off {}.",
            loose.join(" and ")
        ));
    }
    description
}

/// Which way a miter saw's table is turned, as you face it with the stop to the right: a
/// positive miter swings the blade's front toward the stop.
pub fn describe_miter(miter: Angle) -> String {
    match miter.hundredths().signum() {
        0 => "square".to_string(),
        1 => format!("{miter} right"),
        _ => format!("{} left", miter.abs()),
    }
}

/// A length as a tape measure reads it, to the nearest 64th.
pub fn measured(length: Length) -> Length {
    length.rounded_to(Length::from_inch_fraction(1, 64))
}

/// A board's size the way a cut list writes it, thickness x width x length, e.g. "3/4" x 2" x
/// 12"". A mitered board's length is to its long points.
pub fn describe_size(board: &Board) -> String {
    let size = format!(
        "{} x {} x {}",
        measured(board.thickness()),
        measured(board.width()),
        measured(board.length())
    );
    if board.miters().is_empty() {
        size
    } else {
        format!("{size} (long point)")
    }
}

/// How a board's ends are mitered, if they are: "mitered 45° at both ends".
pub fn describe_miters(board: &Board) -> Option<String> {
    match board.miters().as_slice() {
        [] => None,
        [one] => Some(format!("mitered {one} at one end")),
        [a, b] if a == b => Some(format!("mitered {a} at both ends")),
        angles => Some(format!(
            "mitered {}",
            angles
                .iter()
                .map(|angle| angle.to_string())
                .collect::<Vec<_>>()
                .join(" and ")
        )),
    }
}

/// How a board's sides are beveled, if any are: "beveled 45° along one edge", "beveled 30°
/// at both ends", or both, "beveled 45° along one edge and 30° at one end".
pub fn describe_bevels(board: &Board) -> Option<String> {
    let bevels = board.bevels();
    let along = |ends: bool| -> Option<String> {
        let angles: Vec<woodsim::Angle> = bevels
            .iter()
            .filter(|(edge, _)| (edge.out.l != 0) == ends)
            .map(|&(_, angle)| angle)
            .collect();
        let (one, both, where_) = if ends {
            ("at one end", "at both ends", "at its ends")
        } else {
            ("along one edge", "along both edges", "along its edges")
        };
        match angles.as_slice() {
            [] => None,
            [angle] => Some(format!("{angle} {one}")),
            [a, b] if a == b => Some(format!("{a} {both}")),
            angles => Some(format!(
                "{} {where_}",
                angles
                    .iter()
                    .map(|angle| angle.to_string())
                    .collect::<Vec<_>>()
                    .join(" and ")
            )),
        }
    };
    let described: Vec<String> = [along(false), along(true)].into_iter().flatten().collect();
    (!described.is_empty()).then(|| format!("beveled {}", described.join(" and ")))
}

/// How a board's faces taper, if any do: "tapered 5° across its width", or along its length.
pub fn describe_tapers(board: &Board) -> Option<String> {
    let tapers = board.tapers();
    let described: Vec<String> = [("across its width", true), ("along its length", false)]
        .into_iter()
        .filter_map(|(way, across)| {
            let angles: Vec<String> = tapers
                .iter()
                .filter(|(edge, _)| (edge.out.l == 0) == across)
                .map(|(_, angle)| angle.to_string())
                .collect();
            (!angles.is_empty()).then(|| format!("{} {way}", angles.join(" and ")))
        })
        .collect();
    (!described.is_empty()).then(|| format!("tapered {}", described.join(" and ")))
}

/// Describes a board, its mitered ends, and the slots sawn into it, e.g. "3/4" x 7-1/4" x 24",
/// with a stopped cut 11-9/16" long on top and 13" on the bottom". Slots that match once
/// measured are counted rather than listed: "with three 3/4"-wide, 1/4"-deep dados all the way
/// across".
pub fn describe_board(board: &Board) -> String {
    let mut groups: Vec<(String, Kerf, usize)> = Vec::new();
    for slot in slots(board) {
        let description = describe_slots(board, &slot, 1);
        match groups.iter_mut().find(|(seen, ..)| *seen == description) {
            Some((_, _, count)) => *count += 1,
            None => groups.push((description, slot, 1)),
        }
    }
    let slots: Vec<String> = groups
        .iter()
        .map(|(description, slot, count)| match count {
            1 => description.clone(),
            _ => describe_slots(board, slot, *count),
        })
        .collect();
    let mut description = describe_size(board);
    if let Some(mitered) = describe_miters(board) {
        description.push_str(&format!(", {mitered}"));
    }
    for shaped in [describe_bevels(board), describe_tapers(board)]
        .into_iter()
        .flatten()
    {
        description.push_str(&format!(", {shaped}"));
    }
    if !slots.is_empty() {
        description.push_str(&format!(", with {}", slots.join(" and ")));
    }
    description
}

/// The slots in `board` as you'd see them. Kerfs cut side by side the same way (the blade at
/// the same height, its center traveling the same distance) whose faces touch or overlap are
/// one wider slot: a dado nibbled out a stroke at a time, say.
fn slots(board: &Board) -> Vec<Kerf> {
    let mut slots: Vec<Kerf> = Vec::new();
    for kerf in board.kerfs() {
        let mut slot = *kerf;
        slots.retain(|other| {
            let beside = Kerf {
                near: slot.near,
                far: slot.far,
                ..*other
            } == slot
                && other.near <= slot.far
                && slot.near <= other.far;
            if beside {
                slot.near = slot.near.min(other.near);
                slot.far = slot.far.max(other.far);
            }
            !beside
        });
        slots.push(slot);
    }
    slots
}

/// Describes a slot sawn into `board`: a stopped cut if it goes through, otherwise a groove or
/// dado (or a rabbet, along an edge or across an end). Lengths are rounded to 1/32", as you'd
/// measure them.
pub fn describe_kerf(board: &Board, kerf: &Kerf) -> String {
    describe_slots(board, kerf, 1)
}

/// Describes `count` slots shaped like `kerf`: "a 1/4"-deep groove", "two 1/4"-deep grooves".
fn describe_slots(board: &Board, kerf: &Kerf, count: usize) -> String {
    const NUMBERS: [&str; 9] = [
        "two", "three", "four", "five", "six", "seven", "eight", "nine", "ten",
    ];
    let (a, s) = match count {
        1 => ("a".to_string(), ""),
        2..=10 => (NUMBERS[count - 2].to_string(), "s"),
        _ => (count.to_string(), "s"),
    };
    let thirty_second = Length::from_inch_fraction(1, 32);
    let size = board.slot(kerf);
    let run = |run: Option<Run>| {
        run.map_or(Length::ZERO, |run| run.length)
            .rounded_to(thirty_second)
    };
    let width = size.width.rounded_to(thirty_second);
    let depth = size.depth.rounded_to(thirty_second);
    // Which way it runs: along the board, or across the face or edge it's in.
    let along = kerf.along;
    let lengthwise = along.l.abs() > along.w.abs().max(along.t.abs());
    let full = if lengthwise {
        "the full length"
    } else {
        "all the way across"
    };
    // Where a slot opens, in words, and what sort of surface that is.
    let plain = |surface: Surface| match surface {
        Surface::Side(edge) if edge.lean == 0 && edge.out.l == 0 => Some("edge"),
        Surface::Side(edge) if edge.lean == 0 && edge.out.w == 0 => Some("end"),
        _ => None,
    };
    let on = |surface: Surface| match surface {
        Surface::Top => "top".to_string(),
        Surface::Bottom => "the bottom".to_string(),
        surface => match plain(surface) {
            Some(side) => format!("one {side}"),
            None => format!("its {}", name(surface)),
        },
    };
    // By a blade parallel to the faces, a resaw slot.
    let resaw = kerf.normal.t.abs() > kerf.normal.l.abs().max(kerf.normal.w.abs());
    let (cut, slot) = if resaw {
        ("resaw cut", "resaw kerf")
    } else {
        ("cut", "kerf")
    };
    if let (Some(one), Some(other)) = (size.on_entry, size.on_exit) {
        // Out through another surface somewhere, it would have cut the board apart all along,
        // so it stops.
        let (one, other) = (run(Some(one)), run(Some(other)));
        return match (size.entry, size.exit) {
            (Surface::Top, Surface::Bottom) => {
                format!("{a} stopped cut{s} {one} long on top and {other} on the bottom")
            }
            (Surface::Bottom, Surface::Top) => {
                format!("{a} stopped cut{s} {other} long on top and {one} on the bottom")
            }
            (entry, exit) if plain(entry).is_some() && plain(entry) == plain(exit) => format!(
                "{a} stopped {cut}{s} {one} long on {} and {other} on the other",
                on(entry)
            ),
            (entry, exit) => format!(
                "{a} stopped {cut}{s} {one} long on {} and {other} on {}",
                on(entry),
                on(exit)
            ),
        };
    }
    let Surface::Side(_) = size.entry else {
        // Sawn into a face.
        let kind = match (lengthwise, size.at_edge) {
            (true, false) => format!("groove{s}"),
            (true, true) => format!("rabbet{s}"),
            (false, false) => format!("dado{s}"),
            (false, true) => format!("rabbet{s} across the end"),
        };
        let slant = kerf.slant();
        let described = if slant == Angle::ZERO {
            format!("{a} {width}-wide, {depth}-deep {kind}")
        } else {
            format!("{a} {width}-wide, {depth}-deep {kind} at {},", slant)
        };
        // How far it runs on the face the blade came through.
        return match size.on_entry {
            Some(opening) if opening.end_to_end => format!("{described} {full}"),
            opening => format!("{described} {} long", run(opening)),
        };
    };
    // Sawn into an edge, an end, or whatever else the board rested on.
    let into = match plain(size.entry) {
        Some(side) => format!("an {side}"),
        None => format!("its {}", name(size.entry)),
    };
    match size.on_entry.or(size.on_exit) {
        Some(opening) if opening.end_to_end => {
            format!("{a} {width}-wide, {depth}-deep {slot}{s} in {into}, {full}")
        }
        opening => format!(
            "{a} {width}-wide, {depth}-deep {slot}{s} in {into}, {} long",
            run(opening)
        ),
    }
}

/// What a board's surface is called: a face, an edge, an end, or a mitered end, any of the
/// sides beveled, a face that tapers, or a slot (`name_on` says which of its surfaces).
pub fn name(surface: Surface) -> &'static str {
    match surface {
        Surface::Slot(_) => "slot",
        Surface::Bottom => "bottom face",
        Surface::Top => "top face",
        Surface::Side(edge) if edge.tapers() => "tapered face",
        Surface::Side(edge) if edge.leans() && edge.out.l != 0 && edge.out.w != 0 => {
            "beveled miter"
        }
        Surface::Side(edge) if edge.leans() && edge.out.l != 0 => "beveled end",
        Surface::Side(edge) if edge.leans() => "beveled edge",
        Surface::Side(edge) if edge.out.l != 0 && edge.out.w != 0 => "mitered end",
        Surface::Side(edge) if edge.out.l != 0 => "end",
        Surface::Side(_) => "edge",
    }
}

#[cfg(test)]
mod tests {
    use woodsim::{MiterSaw, Orientation, Pass};

    use super::*;

    /// A 1x8, 24" long, chopped 1/4" deep on the miter saw against the stop at each of `stops`
    /// (in sixteenths of an inch).
    fn scarred(stops: &[i64]) -> Board {
        let saw = MiterSaw::twelve_inch();
        let bottom = Length::from_inch_fraction(1, 2);
        let mut board = Board::new(
            Length::from_inches(24),
            Length::from_inch_fraction(29, 4),
            Length::from_inch_fraction(3, 4),
        );
        for &stop in stops {
            let stop = Length::from_inch_fraction(stop, 16);
            board = match saw.chop(&board, Orientation::default(), stop, bottom) {
                Ok(Pass::Kerfed(board)) => board,
                other => panic!("expected a scar, got {other:?}"),
            };
        }
        board
    }

    #[test]
    fn side_by_side_kerfs_read_as_one_wider_slot() {
        // Two strokes a kerf apart, then one overlapping the second.
        let board = scarred(&[192, 190, 189]);
        assert_eq!(
            describe_board(&board),
            "3/4\" x 7-1/4\" x 24\", with a 5/16\"-wide, 1/4\"-deep dado 3-7/16\" long"
        );
    }

    #[test]
    fn matching_slots_are_counted() {
        let board = scarred(&[64, 128, 192, 256, 258]);
        assert_eq!(
            describe_board(&board),
            "3/4\" x 7-1/4\" x 24\", with three 1/8\"-wide, 1/4\"-deep dados 3-7/16\" long \
             and a 1/4\"-wide, 1/4\"-deep dado 3-7/16\" long"
        );
    }

    #[test]
    fn a_resaw_slot_is_described_from_the_edge_it_came_in_through() {
        let saw = woodsim::TableSaw {
            blade: woodsim::Blade {
                diameter: Length::from_inches(10),
                kerf: woodsim::Blade::FULL_KERF,
            },
            fence: Length::from_inch_fraction(5, 16),
            height: Length::from_inches(3),
            miter_gauge: None,
            gauge_angle: woodsim::Angle::ZERO,
            tilt: woodsim::Angle::ZERO,
        };
        let board = Board::new(
            Length::from_inches(24),
            Length::from_inch_fraction(11, 2),
            Length::from_inch_fraction(3, 4),
        );
        let Ok(Pass::Kerfed(slotted)) = saw.rip(&board, Orientation::default().tipped()) else {
            panic!("expected a slot in the edge");
        };
        assert_eq!(
            describe_board(&slotted),
            "3/4\" x 5-1/2\" x 24\", with a 1/8\"-wide, 3\"-deep resaw kerf in an edge, the full \
             length"
        );
    }
}
