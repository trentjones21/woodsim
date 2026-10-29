//! The shop's inventory: the wood that isn't on a station or in hand. Finished parts and
//! glue-ups go on one shelf and offcuts on another, and either can move to the other. New
//! lumber comes in the sizes the rack stocks, as much as you like. From anywhere, a part goes
//! straight to the station you're at: onto a saw to cut, or onto the bench to put together.
//!
//! Wood a station sets aside comes here: whatever a board set down on a saw displaces, what a
//! cut leaves when you take one side, strips a cut sets loose, and what you put away.

use bevy::prelude::*;
use woodsim::Length;

use crate::{
    chop::MiterSawStation,
    controls::{Action, Actions},
    feed::TableSawStation,
    hand::{Hand, Workpiece, describe_glue_up, from_rack},
    join::BenchStation,
    shop::{ShopView, describe_board, describe_size},
};

/// Where in the inventory a piece of wood goes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Shelf {
    /// Parts to use: what a cut was measured to make, glue-ups, boards put away whole.
    Parts,
    /// What's left over: the other side of a cut, and strips a cut set loose.
    Offcuts,
}

impl Shelf {
    pub fn other(self) -> Shelf {
        match self {
            Shelf::Parts => Shelf::Offcuts,
            Shelf::Offcuts => Shelf::Parts,
        }
    }
}

/// The lumber the rack stocks: its name, thickness, and width. New boards are 24" long.
pub const LUMBER: [(&str, Length, Length); 7] = [
    (
        "1x3",
        Length::from_inch_fraction(3, 4),
        Length::from_inch_fraction(5, 2),
    ),
    (
        "1x4",
        Length::from_inch_fraction(3, 4),
        Length::from_inch_fraction(7, 2),
    ),
    (
        "1x6",
        Length::from_inch_fraction(3, 4),
        Length::from_inch_fraction(11, 2),
    ),
    (
        "1x8",
        Length::from_inch_fraction(3, 4),
        Length::from_inch_fraction(29, 4),
    ),
    (
        "1x10",
        Length::from_inch_fraction(3, 4),
        Length::from_inch_fraction(37, 4),
    ),
    (
        "1x12",
        Length::from_inch_fraction(3, 4),
        Length::from_inch_fraction(45, 4),
    ),
    (
        "2x4",
        Length::from_inch_fraction(3, 2),
        Length::from_inch_fraction(7, 2),
    ),
];

/// A new board of the `index`th size of lumber.
pub fn lumber(index: usize) -> Option<Workpiece> {
    let &(_, thickness, width) = LUMBER.get(index)?;
    Some(Workpiece::Board(from_rack(width, thickness)))
}

/// A piece of wood in the inventory.
#[derive(Clone, Debug, PartialEq)]
pub struct Item {
    pub id: u64,
    pub shelf: Shelf,
    pub work: Workpiece,
}

/// A line of the inventory: identical pieces on a shelf, counted together.
#[derive(Clone, Debug, PartialEq)]
pub struct Row<'a> {
    /// The first of them stored, which is the one taken.
    pub id: u64,
    pub count: usize,
    pub work: &'a Workpiece,
}

#[derive(Resource, Default, Debug)]
pub struct Inventory {
    items: Vec<Item>,
    next: u64,
}

impl Inventory {
    /// Puts `work` on `shelf`, and says what it's called there.
    pub fn store(&mut self, shelf: Shelf, work: Workpiece) -> u64 {
        self.next += 1;
        self.items.push(Item {
            id: self.next,
            shelf,
            work,
        });
        self.next
    }

    pub fn store_all(&mut self, wood: impl IntoIterator<Item = (Shelf, Workpiece)>) {
        for (shelf, work) in wood {
            self.store(shelf, work);
        }
    }

    /// Takes the piece `id` out, to use.
    pub fn take(&mut self, id: u64) -> Option<Workpiece> {
        let index = self.items.iter().position(|item| item.id == id)?;
        Some(self.items.remove(index).work)
    }

    /// Moves the piece `id` to the other shelf, after the pieces already there.
    pub fn move_to_other_shelf(&mut self, id: u64) -> bool {
        let Some(index) = self.items.iter().position(|item| item.id == id) else {
            return false;
        };
        let mut item = self.items.remove(index);
        item.shelf = item.shelf.other();
        self.items.push(item);
        true
    }

    pub fn get(&self, id: u64) -> Option<&Item> {
        self.items.iter().find(|item| item.id == id)
    }

    /// How many pieces are on `shelf`.
    pub fn count(&self, shelf: Shelf) -> usize {
        self.items.iter().filter(|item| item.shelf == shelf).count()
    }

    /// The lines of `shelf`: identical pieces together, in the order the first of each was
    /// stored.
    pub fn rows(&self, shelf: Shelf) -> Vec<Row<'_>> {
        let mut rows: Vec<Row> = Vec::new();
        for item in self.items.iter().filter(|item| item.shelf == shelf) {
            match rows.iter_mut().find(|row| *row.work == item.work) {
                Some(row) => row.count += 1,
                None => rows.push(Row {
                    id: item.id,
                    count: 1,
                    work: &item.work,
                }),
            }
        }
        rows
    }
}

/// A piece of wood in words for its line in the inventory: its size, and what else there is
/// to say about it (its miters and slots, or that it's a glue-up).
pub fn describe(work: &Workpiece) -> (String, String) {
    match work {
        Workpiece::Board(board) => {
            let size = describe_size(board);
            let all = describe_board(board);
            let notes = all
                .strip_prefix(&size)
                .unwrap_or(&all)
                .trim_start_matches(", ");
            (size, notes.to_string())
        }
        Workpiece::GlueUp(assembly) => {
            let described = describe_glue_up(assembly);
            match described.split_once(", ") {
                Some((parts, size)) => (size.to_string(), parts.to_string()),
                None => (described, String::new()),
            }
        }
    }
}

pub struct InventoryPlugin;

impl Plugin for InventoryPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Inventory>()
            .add_systems(Update, (collect_set_aside, use_inventory).chain());
    }
}

/// Puts away what the saws set aside: what a board set down displaced, the other side of a cut
/// when one side's taken, and strips a cut set loose.
fn collect_set_aside(
    mut table_saw: ResMut<TableSawStation>,
    mut miter_saw: ResMut<MiterSawStation>,
    mut inventory: ResMut<Inventory>,
) {
    if table_saw.has_set_aside() {
        inventory.store_all(table_saw.take_set_aside());
    }
    if miter_saw.has_set_aside() {
        inventory.store_all(miter_saw.take_set_aside());
    }
}

/// What can be done with a piece of wood from the inventory, at `view`: put on a saw, set on
/// the bench, or, from the overview, picked up.
pub fn can_use_at(
    view: ShopView,
    table_saw: &TableSawStation,
    miter_saw: &MiterSawStation,
) -> bool {
    match view {
        ShopView::TableSaw => table_saw.can_load(),
        ShopView::MiterSaw => miter_saw.can_load(),
        ShopView::Bench | ShopView::Overview => true,
    }
}

/// Uses the inventory as asked: a piece or new lumber to wherever you are, a piece to the other
/// shelf or the scrap bin, or what's in hand, on the saw, or picked at the bench, put away.
fn use_inventory(
    actions: Res<Actions>,
    view: Res<ShopView>,
    mut inventory: ResMut<Inventory>,
    mut hand: ResMut<Hand>,
    mut table_saw: ResMut<TableSawStation>,
    mut miter_saw: ResMut<MiterSawStation>,
    mut bench: ResMut<BenchStation>,
) {
    for action in actions.started() {
        match action {
            Action::Take(id) | Action::NewLumber(id)
                if can_use_at(*view, &table_saw, &miter_saw) =>
            {
                let work = match action {
                    Action::Take(id) => inventory.take(id),
                    _ => lumber(id as usize),
                };
                let Some(work) = work else { continue };
                match *view {
                    ShopView::TableSaw => table_saw.load(work),
                    ShopView::MiterSaw => miter_saw.load(work),
                    ShopView::Bench => bench.set_down(work, None),
                    ShopView::Overview => {
                        if let Some(held) = hand.0.replace(work) {
                            inventory.store(Shelf::Parts, held);
                        }
                    }
                }
            }
            Action::MoveShelf(id) => {
                inventory.move_to_other_shelf(id);
            }
            Action::Scrap(id) => {
                inventory.take(id);
            }
            Action::PutAway => {
                if let Some(work) = hand.0.take() {
                    inventory.store(Shelf::Parts, work);
                }
            }
            Action::Store => match *view {
                ShopView::TableSaw => table_saw.put_away(),
                ShopView::MiterSaw => miter_saw.put_away(),
                ShopView::Bench => {
                    if let Some(work) = bench.take_picked() {
                        inventory.store(Shelf::Parts, work);
                    }
                }
                ShopView::Overview => {}
            },
            Action::CutAgain => match *view {
                ShopView::TableSaw => table_saw.cut_again(),
                ShopView::MiterSaw => miter_saw.cut_again(),
                _ => {}
            },
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use bevy::ecs::system::RunSystemOnce;
    use woodsim::Board;

    use super::*;

    /// A shop at `view` with nothing anywhere but what the test puts there.
    fn shop(view: ShopView) -> World {
        let mut world = World::new();
        world.insert_resource(view);
        world.init_resource::<Inventory>();
        world.init_resource::<Actions>();
        world.init_resource::<Hand>();
        world.init_resource::<TableSawStation>();
        world.init_resource::<MiterSawStation>();
        world.init_resource::<BenchStation>();
        world
    }

    /// Does `action`, and puts away what it set aside.
    fn act(world: &mut World, action: Action) {
        world.resource_mut::<Actions>().start(action);
        world.run_system_once(use_inventory).unwrap();
        world.run_system_once(collect_set_aside).unwrap();
        *world.resource_mut::<Actions>() = Actions::default();
    }

    fn board(inches: i64) -> Workpiece {
        Workpiece::Board(Board::new(
            Length::from_inches(inches),
            Length::from_inch_fraction(3, 2),
            Length::from_inch_fraction(3, 4),
        ))
    }

    #[test]
    fn identical_pieces_count_together_and_the_first_is_taken() {
        let mut inventory = Inventory::default();
        let first = inventory.store(Shelf::Parts, board(10));
        inventory.store(Shelf::Parts, board(12));
        inventory.store(Shelf::Parts, board(10));
        inventory.store(Shelf::Offcuts, board(10));
        let rows = inventory.rows(Shelf::Parts);
        assert_eq!(
            rows.iter()
                .map(|row| (row.id, row.count))
                .collect::<Vec<_>>(),
            [(first, 2), (2, 1)]
        );
        assert_eq!(inventory.count(Shelf::Offcuts), 1);
        assert_eq!(inventory.take(first), Some(board(10)));
        assert_eq!(inventory.rows(Shelf::Parts)[0].count, 1);
        assert_eq!(inventory.take(first), None);
    }

    #[test]
    fn an_offcut_moves_to_the_parts_and_back() {
        let mut inventory = Inventory::default();
        let offcut = inventory.store(Shelf::Offcuts, board(7));
        assert!(inventory.move_to_other_shelf(offcut));
        assert_eq!(inventory.get(offcut).unwrap().shelf, Shelf::Parts);
        assert_eq!(inventory.count(Shelf::Offcuts), 0);
        assert!(inventory.move_to_other_shelf(offcut));
        assert_eq!(inventory.get(offcut).unwrap().shelf, Shelf::Offcuts);
    }

    #[test]
    fn a_part_goes_onto_the_saw_youre_at_and_what_was_there_is_put_away() {
        let mut world = shop(ShopView::MiterSaw);
        let id = world
            .resource_mut::<Inventory>()
            .store(Shelf::Parts, board(10));
        world.resource_mut::<MiterSawStation>().load(board(24));
        act(&mut world, Action::Take(id));
        let inventory = world.resource::<Inventory>();
        assert_eq!(inventory.get(id), None);
        assert_eq!(inventory.rows(Shelf::Parts)[0].work, &board(24));
        assert!(world.resource::<MiterSawStation>().can_turn());
    }

    #[test]
    fn new_lumber_goes_on_the_bench_and_what_is_in_hand_is_put_away() {
        let mut world = shop(ShopView::Bench);
        act(&mut world, Action::NewLumber(1));
        assert_eq!(world.resource::<BenchStation>().groups.len(), 1);
        world.resource_mut::<Hand>().0 = Some(board(7));
        act(&mut world, Action::PutAway);
        assert!(world.resource::<Hand>().0.is_none());
        assert_eq!(world.resource::<Inventory>().count(Shelf::Parts), 1);
    }

    #[test]
    fn at_the_saw_put_away_shelves_both_sides_of_the_cut() {
        let mut world = shop(ShopView::TableSaw);
        world.resource_mut::<TableSawStation>().push(10.0);
        act(&mut world, Action::Store);
        let inventory = world.resource::<Inventory>();
        assert_eq!(
            (
                inventory.count(Shelf::Parts),
                inventory.count(Shelf::Offcuts)
            ),
            (1, 1)
        );
    }

    #[test]
    fn lumber_comes_in_the_rack_sizes_24_inches_long() {
        let Some(Workpiece::Board(two_by_four)) = lumber(6) else {
            panic!("a 2x4");
        };
        assert_eq!(two_by_four.to_string(), "1-1/2\" x 3-1/2\" x 24\"");
        assert_eq!(lumber(LUMBER.len()), None);
    }

    #[test]
    fn a_line_reads_as_its_size_and_whats_special_about_it() {
        assert_eq!(
            describe(&board(10)),
            ("3/4\" x 1-1/2\" x 10\"".to_string(), String::new())
        );
        let panel = Workpiece::GlueUp(crate::work::panel(
            &[Length::from_inches(4); 2],
            Length::from_inches(24),
        ));
        assert_eq!(
            describe(&panel),
            (
                "3/4\" x 8\" x 24\"".to_string(),
                "glue-up of 2 parts".to_string()
            )
        );
    }
}
