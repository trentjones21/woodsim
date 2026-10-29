//! The inventory's tree down the left of the screen, like the browser in a CAD program: every
//! piece of wood in the shop, wherever it is. What's in hand, what's on each saw (the board, or
//! both sides of the cut it made), the parts and glue-ups on the workbench, the two shelves, the
//! parts saved as files, and new lumber, each under its heading. Wood doesn't drop out of the
//! tree when it's used: it moves from one heading to another. A glue-up opens to list its
//! boards, lettered as its plans letter them, and a heading folds away what's under it.
//!
//! Clicking a line picks it out, and the card under the tree says what it is and where it is,
//! with buttons for what can be done with it there (`Where`): use it at the station you're at,
//! duplicate it, put it away, move it to the other shelf, save it, or throw it away.

use std::collections::HashSet;

use bevy::prelude::*;

use crate::{
    chop::MiterSawStation,
    controls::{Action, Actions},
    feed::TableSawStation,
    hand::{Hand, Workpiece},
    inventory::{self, Inventory, LUMBER, Shelf, lumber},
    join::BenchStation,
    plan,
    saved::Saved,
    shop::{Part, ShopView, describe_board, describe_size},
};

/// Where a piece of wood is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Where {
    Hand,
    /// On a saw: the board on it, or a side of the cut it made.
    Saw(ShopView, Part),
    /// The `n`th part or glue-up on the workbench.
    Bench(usize),
    /// On a shelf, by its place in the inventory.
    Shelf(u64),
    /// The `n`th part saved as a file.
    Saved(u64),
    /// New lumber of the `n`th size the rack stocks.
    Lumber(u64),
}

/// A line of the tree: the wood somewhere, or one board of a glue-up there.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Entry {
    pub at: Where,
    /// Which board of a glue-up, by its place in it.
    pub member: Option<usize>,
}

impl Entry {
    /// The wood at `at`, as a whole.
    pub fn at(at: Where) -> Entry {
        Entry { at, member: None }
    }
}

/// The tree's headings, in order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Heading {
    Hand,
    TableSaw,
    MiterSaw,
    Bench,
    Parts,
    Offcuts,
    Saved,
    Lumber,
}

impl Heading {
    const ALL: [Heading; 8] = [
        Heading::Hand,
        Heading::TableSaw,
        Heading::MiterSaw,
        Heading::Bench,
        Heading::Parts,
        Heading::Offcuts,
        Heading::Saved,
        Heading::Lumber,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Heading::Hand => "In hand",
            Heading::TableSaw => "Table saw",
            Heading::MiterSaw => "Miter saw",
            Heading::Bench => "Workbench",
            Heading::Parts => "Parts shelf",
            Heading::Offcuts => "Offcuts shelf",
            Heading::Saved => "Saved",
            Heading::Lumber => "New lumber",
        }
    }

    /// What's said under it with nothing there.
    fn empty(self) -> &'static str {
        match self {
            Heading::Hand | Heading::Lumber => "",
            Heading::TableSaw | Heading::MiterSaw | Heading::Bench => "Nothing on it.",
            Heading::Parts => "Pieces you cut to size, glue-ups, and boards you put away go here.",
            Heading::Offcuts => "What's left over from a cut goes here.",
            Heading::Saved => "Parts you save go here, and stay after a restart.",
        }
    }
}

/// What folds away: everything under a heading, or a glue-up's boards.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Fold {
    Heading(Heading),
    GlueUp(Where),
}

/// A line of the tree, as it's drawn.
#[derive(Clone, Debug, PartialEq)]
pub enum Line {
    /// A heading, how many pieces are under it, and whether they're shown.
    Heading {
        heading: Heading,
        count: usize,
        open: bool,
    },
    /// What's said under a heading with nothing under it.
    Empty(&'static str),
    Wood(Box<WoodLine>),
}

/// A piece of wood's line: what it is, in bold, and what else there is to say about it.
#[derive(Clone, Debug, PartialEq)]
pub struct WoodLine {
    pub entry: Entry,
    /// A board of a glue-up's letter, as its plans call it.
    pub label: Option<String>,
    pub name: String,
    pub notes: String,
    /// How many identical pieces it stands for, and the inventory's pieces, on a shelf.
    pub count: usize,
    pub ids: Vec<u64>,
    /// A glue-up's: whether its boards are listed under it.
    pub open: Option<bool>,
    pub work: Workpiece,
}

/// What's folded away, and which line is picked out.
#[derive(Resource, Default, Debug)]
pub struct Tree {
    pub selected: Option<Entry>,
    folded: HashSet<Heading>,
    /// Glue-ups whose boards are listed: they start closed.
    opened: HashSet<Where>,
}

impl Tree {
    pub fn is_open(&self, fold: Fold) -> bool {
        match fold {
            Fold::Heading(heading) => !self.folded.contains(&heading),
            Fold::GlueUp(at) => self.opened.contains(&at),
        }
    }

    pub fn toggle(&mut self, fold: Fold) {
        fn flip<T: Eq + std::hash::Hash>(set: &mut HashSet<T>, key: T) {
            if !set.remove(&key) {
                set.insert(key);
            }
        }
        match fold {
            Fold::Heading(heading) => flip(&mut self.folded, heading),
            Fold::GlueUp(at) => flip(&mut self.opened, at),
        }
    }

    /// Picks out `entry`, or lets it go if it's picked out already.
    pub fn select(&mut self, entry: Entry) {
        self.selected = (self.selected != Some(entry)).then_some(entry);
    }

    /// Wood at `from` has moved to `to`: if it was picked out, or open, it still is. Taken off
    /// the bench, the parts after it there move up one.
    pub fn follow(&mut self, from: Where, to: Where) {
        let moved = |at: Where| match (from, at) {
            _ if at == from => to,
            (Where::Bench(gone), Where::Bench(index)) if index > gone => Where::Bench(index - 1),
            _ => at,
        };
        if let Some(entry) = &mut self.selected {
            entry.at = moved(entry.at);
        }
        self.opened = self.opened.drain().map(moved).collect();
    }
}

/// Everywhere wood can be.
pub struct Places<'a> {
    pub hand: &'a Hand,
    pub table_saw: &'a TableSawStation,
    pub miter_saw: &'a MiterSawStation,
    pub bench: &'a BenchStation,
    pub inventory: &'a Inventory,
    pub saved: &'a Saved,
}

impl Places<'_> {
    /// The wood at `entry`, as it would be handed over: a board of a glue-up on its own.
    pub fn wood(&self, entry: Entry) -> Option<Workpiece> {
        match (entry.member, self.whole(entry.at)?) {
            (None, work) => Some(work),
            (Some(member), Workpiece::GlueUp(assembly)) => Some(Workpiece::Board(
                assembly.members().get(member)?.board.clone(),
            )),
            (Some(_), Workpiece::Board(_)) => None,
        }
    }

    fn whole(&self, at: Where) -> Option<Workpiece> {
        let on_saw = |wood: Vec<(Part, Workpiece)>, part| {
            wood.into_iter()
                .find(|&(on, _)| on == part)
                .map(|(_, work)| work)
        };
        match at {
            Where::Hand => self.hand.0.clone(),
            Where::Saw(ShopView::TableSaw, part) => on_saw(self.table_saw.wood(), part),
            Where::Saw(ShopView::MiterSaw, part) => on_saw(self.miter_saw.wood(), part),
            Where::Saw(..) => None,
            Where::Bench(index) => self.bench.wood(index),
            Where::Shelf(id) => self.inventory.get(id).map(|item| item.work.clone()),
            Where::Saved(index) => self.saved.part(index as usize),
            Where::Lumber(index) => lumber(index as usize),
        }
    }

    /// Where `entry` is, in words: "on the table saw, the measured piece".
    pub fn say_where(&self, entry: Entry) -> String {
        let at = match entry.at {
            Where::Hand => "in hand".to_string(),
            Where::Saw(view, part) => {
                let saw = match view {
                    ShopView::MiterSaw => "on the miter saw",
                    _ => "on the table saw",
                };
                match role(part) {
                    Some(role) => format!("{saw}, the {role}"),
                    None => saw.to_string(),
                }
            }
            Where::Bench(_) => "on the workbench".to_string(),
            Where::Shelf(id) => match self.inventory.get(id).map(|item| item.shelf) {
                Some(Shelf::Offcuts) => "on the offcuts shelf".to_string(),
                _ => "on the parts shelf".to_string(),
            },
            Where::Saved(index) => match self.saved.parts.get(index as usize) {
                Some(part) => format!("saved as {}", part.path.display()),
                None => "saved".to_string(),
            },
            Where::Lumber(_) => "new lumber, as much as you like".to_string(),
        };
        match entry.member {
            Some(_) => format!("in a glue-up {at}"),
            None => at,
        }
    }

    /// The tree's lines: each heading, and unless it's folded away, the wood under it, a
    /// glue-up's boards under it if it's open.
    pub fn lines(&self, tree: &Tree) -> Vec<Line> {
        let mut lines = Vec::new();
        for heading in Heading::ALL {
            let wood = self.under(heading, tree);
            if heading == Heading::Hand && wood.is_empty() {
                continue;
            }
            let open = tree.is_open(Fold::Heading(heading));
            lines.push(Line::Heading {
                heading,
                count: wood.iter().map(|line| line.count).sum(),
                open,
            });
            if !open {
                continue;
            }
            if wood.is_empty() {
                lines.push(Line::Empty(heading.empty()));
            }
            for line in wood {
                let boards = match &line.work {
                    Workpiece::GlueUp(assembly) if line.open == Some(true) => {
                        boards(line.entry.at, assembly)
                    }
                    _ => Vec::new(),
                };
                lines.push(Line::Wood(Box::new(line)));
                lines.extend(boards.into_iter().map(|line| Line::Wood(Box::new(line))));
            }
        }
        lines
    }

    /// The wood under `heading`.
    fn under(&self, heading: Heading, tree: &Tree) -> Vec<WoodLine> {
        let at = |at, work| wood_line(Entry::at(at), work, tree);
        let on_saw = |view, wood: Vec<(Part, Workpiece)>| {
            wood.into_iter()
                .map(|(part, work)| {
                    let mut line = at(Where::Saw(view, part), work);
                    if let Some(role) = role(part) {
                        line.notes = join(role, &line.notes);
                    }
                    line
                })
                .collect()
        };
        match heading {
            Heading::Hand => self
                .hand
                .0
                .iter()
                .map(|work| at(Where::Hand, work.clone()))
                .collect(),
            Heading::TableSaw => on_saw(ShopView::TableSaw, self.table_saw.wood()),
            Heading::MiterSaw => on_saw(ShopView::MiterSaw, self.miter_saw.wood()),
            Heading::Bench => (0..self.bench.groups.len())
                .filter_map(|index| Some(at(Where::Bench(index), self.bench.wood(index)?)))
                .collect(),
            Heading::Parts | Heading::Offcuts => {
                let shelf = match heading {
                    Heading::Parts => Shelf::Parts,
                    _ => Shelf::Offcuts,
                };
                self.inventory
                    .rows(shelf)
                    .into_iter()
                    .map(|row| WoodLine {
                        count: row.count,
                        ids: row.ids.clone(),
                        ..at(Where::Shelf(row.id), row.work.clone())
                    })
                    .collect()
            }
            Heading::Saved => self
                .saved
                .parts
                .iter()
                .enumerate()
                .map(|(index, part)| {
                    let mut line = at(Where::Saved(index as u64), part.work.clone());
                    let file = part
                        .path
                        .file_stem()
                        .map_or(String::new(), |name| name.to_string_lossy().into_owned());
                    line.notes = match line.notes.is_empty() {
                        true => file,
                        false => format!("{file}: {}", line.notes),
                    };
                    line
                })
                .collect(),
            Heading::Lumber => LUMBER
                .iter()
                .enumerate()
                .filter_map(|(index, (name, thickness, width))| {
                    let work = lumber(index)?;
                    Some(WoodLine {
                        name: name.to_string(),
                        notes: format!("{thickness} x {width} x 24\""),
                        ..at(Where::Lumber(index as u64), work)
                    })
                })
                .collect(),
        }
    }
}

/// What a side of a saw's cut is called.
fn role(part: Part) -> Option<&'static str> {
    match part {
        Part::Whole => None,
        Part::Piece => Some("measured piece"),
        Part::Offcut => Some("offcut"),
    }
}

/// "offcut, mitered 45° at one end".
fn join(first: &str, rest: &str) -> String {
    match rest.is_empty() {
        true => first.to_string(),
        false => format!("{first}, {rest}"),
    }
}

/// The line for `work` at `entry`.
fn wood_line(entry: Entry, work: Workpiece, tree: &Tree) -> WoodLine {
    let (name, notes) = inventory::describe(&work);
    WoodLine {
        entry,
        label: None,
        name,
        notes,
        count: 1,
        ids: Vec::new(),
        open: matches!(work, Workpiece::GlueUp(_)).then(|| tree.is_open(Fold::GlueUp(entry.at))),
        work,
    }
}

/// The lines for a glue-up's boards, lettered as its plans letter them.
fn boards(at: Where, assembly: &woodsim::Assembly) -> Vec<WoodLine> {
    let labels = plan::labels(assembly);
    assembly
        .members()
        .iter()
        .zip(labels)
        .enumerate()
        .map(|(member, (part, label))| {
            let board = &part.board;
            let size = describe_size(board);
            let all = describe_board(board);
            let notes = all.strip_prefix(&size).unwrap_or(&all);
            WoodLine {
                entry: Entry {
                    at,
                    member: Some(member),
                },
                label: Some(label),
                name: size.clone(),
                notes: notes.trim_start_matches(", ").to_string(),
                count: 1,
                ids: Vec::new(),
                open: None,
                work: Workpiece::Board(board.clone()),
            }
        })
        .collect()
}

pub struct PartsPlugin;

impl Plugin for PartsPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Tree>()
            .add_systems(Update, use_tree.before(inventory::InventorySystems));
    }
}

/// Picks out lines and folds them, as asked.
fn use_tree(actions: Res<Actions>, mut tree: ResMut<Tree>) {
    for action in actions.started() {
        match action {
            Action::Select(entry) => tree.select(entry),
            Action::Fold(fold) => tree.toggle(fold),
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use woodsim::Length;

    use super::*;

    fn board(inches: i64) -> Workpiece {
        Workpiece::Board(woodsim::Board::new(
            Length::from_inches(inches),
            Length::from_inch_fraction(7, 2),
            Length::from_inch_fraction(3, 4),
        ))
    }

    struct Shop {
        hand: Hand,
        table_saw: TableSawStation,
        miter_saw: MiterSawStation,
        bench: BenchStation,
        inventory: Inventory,
        saved: Saved,
    }

    impl Shop {
        fn new() -> Shop {
            Shop {
                hand: Hand::default(),
                table_saw: TableSawStation::default(),
                miter_saw: MiterSawStation::default(),
                bench: BenchStation::default(),
                inventory: Inventory::default(),
                saved: Saved::default(),
            }
        }

        fn places(&self) -> Places<'_> {
            Places {
                hand: &self.hand,
                table_saw: &self.table_saw,
                miter_saw: &self.miter_saw,
                bench: &self.bench,
                inventory: &self.inventory,
                saved: &self.saved,
            }
        }
    }

    /// The headings, with how many pieces each has.
    fn headings(lines: &[Line]) -> Vec<(Heading, usize)> {
        lines
            .iter()
            .filter_map(|line| match line {
                Line::Heading { heading, count, .. } => Some((*heading, *count)),
                _ => None,
            })
            .collect()
    }

    fn wood(lines: &[Line]) -> Vec<&WoodLine> {
        lines
            .iter()
            .filter_map(|line| match line {
                Line::Wood(line) => Some(line.as_ref()),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn wood_stays_in_the_tree_wherever_it_goes() {
        let mut shop = Shop::new();
        let id = shop.inventory.store(Shelf::Parts, board(10));
        let tree = Tree::default();
        let lines = shop.places().lines(&tree);
        // The table saw starts with a 1x8 on it; nothing's in hand, so there's no heading for
        // it.
        assert_eq!(
            headings(&lines),
            [
                (Heading::TableSaw, 1),
                (Heading::MiterSaw, 0),
                (Heading::Bench, 0),
                (Heading::Parts, 1),
                (Heading::Offcuts, 0),
                (Heading::Saved, 0),
                (Heading::Lumber, LUMBER.len()),
            ]
        );
        assert!(lines.contains(&Line::Empty("Nothing on it.")));
        let part = shop.inventory.take(id).unwrap();
        shop.miter_saw.load(part.clone());
        let lines = shop.places().lines(&tree);
        let on_saw = wood(&lines)
            .into_iter()
            .find(|line| line.entry.at == Where::Saw(ShopView::MiterSaw, Part::Whole))
            .expect("the part, on the miter saw");
        assert_eq!(on_saw.work, part);
        assert_eq!(on_saw.name, "3/4\" x 3-1/2\" x 10\"");
        assert_eq!(
            shop.places()
                .say_where(Entry::at(Where::Saw(ShopView::MiterSaw, Part::Whole))),
            "on the miter saw"
        );
    }

    #[test]
    fn both_sides_of_a_cut_are_listed_on_the_saw() {
        let mut shop = Shop::new();
        shop.miter_saw.load(board(24));
        shop.miter_saw.set_stop(Length::from_inches(10));
        shop.miter_saw.lower(1.0);
        shop.miter_saw.raise(1.0);
        let lines = shop.places().lines(&Tree::default());
        let on_saw: Vec<_> = wood(&lines)
            .into_iter()
            .filter(|line| matches!(line.entry.at, Where::Saw(ShopView::MiterSaw, _)))
            .map(|line| (line.entry.at, line.notes.clone()))
            .collect();
        assert_eq!(
            on_saw,
            [
                (
                    Where::Saw(ShopView::MiterSaw, Part::Piece),
                    "measured piece".to_string()
                ),
                (
                    Where::Saw(ShopView::MiterSaw, Part::Offcut),
                    "offcut".to_string()
                ),
            ]
        );
    }

    #[test]
    fn a_glue_up_opens_to_its_boards_lettered_as_its_plans_letter_them() {
        let mut shop = Shop::new();
        let panel = crate::work::panel(
            &[Length::from_inches(4), Length::from_inches(4)],
            Length::from_inches(24),
        );
        shop.bench.set_down(Workpiece::GlueUp(panel.clone()), None);
        let mut tree = Tree::default();
        let at = Where::Bench(0);
        let lines = shop.places().lines(&tree);
        let glue_up = wood(&lines)
            .into_iter()
            .find(|line| line.entry.at == at)
            .cloned()
            .unwrap();
        assert_eq!(glue_up.open, Some(false));
        assert_eq!(glue_up.notes, "glue-up of 2 parts");
        tree.toggle(Fold::GlueUp(at));
        let lines = shop.places().lines(&tree);
        let boards: Vec<_> = wood(&lines)
            .into_iter()
            .filter(|line| line.entry.at == at && line.entry.member.is_some())
            .map(|line| (line.label.clone().unwrap(), line.name.clone()))
            .collect();
        assert_eq!(
            boards,
            [
                ("A1".to_string(), "3/4\" x 4\" x 24\"".to_string()),
                ("A2".to_string(), "3/4\" x 4\" x 24\"".to_string()),
            ]
        );
        let member = Entry {
            at,
            member: Some(1),
        };
        assert_eq!(
            shop.places().wood(member),
            Some(Workpiece::Board(panel.members()[1].board.clone()))
        );
        assert_eq!(
            shop.places().say_where(member),
            "in a glue-up on the workbench"
        );
    }

    #[test]
    fn a_folded_heading_still_counts_whats_under_it() {
        let mut shop = Shop::new();
        shop.inventory.store(Shelf::Offcuts, board(5));
        shop.inventory.store(Shelf::Offcuts, board(5));
        let mut tree = Tree::default();
        tree.toggle(Fold::Heading(Heading::Offcuts));
        let lines = shop.places().lines(&tree);
        let at = lines
            .iter()
            .position(|line| {
                matches!(
                    line,
                    Line::Heading {
                        heading: Heading::Offcuts,
                        ..
                    }
                )
            })
            .unwrap();
        assert_eq!(
            lines[at],
            Line::Heading {
                heading: Heading::Offcuts,
                count: 2,
                open: false
            }
        );
        assert!(matches!(
            lines[at + 1],
            Line::Heading {
                heading: Heading::Saved,
                ..
            }
        ));
    }

    #[test]
    fn what_was_picked_out_stays_picked_out_as_it_moves() {
        let mut tree = Tree::default();
        let on_shelf = Entry::at(Where::Shelf(3));
        tree.select(on_shelf);
        tree.toggle(Fold::GlueUp(Where::Shelf(3)));
        tree.follow(Where::Shelf(3), Where::Bench(0));
        assert_eq!(tree.selected, Some(Entry::at(Where::Bench(0))));
        assert!(tree.is_open(Fold::GlueUp(Where::Bench(0))));
        tree.select(Entry::at(Where::Bench(0)));
        assert_eq!(tree.selected, None);
        // Taken off the bench, what was after it moves up.
        tree.select(Entry::at(Where::Bench(2)));
        tree.follow(Where::Bench(1), Where::Shelf(9));
        assert_eq!(tree.selected, Some(Entry::at(Where::Bench(1))));
    }
}
