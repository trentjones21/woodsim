//! The shop's inventory: the wood that isn't on a station or in hand. Finished parts and
//! glue-ups go on one shelf and offcuts on another, and either can move to the other. New
//! lumber comes in the sizes the rack stocks, as much as you like. From anywhere, a part goes
//! straight to the station you're at: onto a saw to cut, or onto the bench to put together.
//! Any piece, wherever it is, can be duplicated onto the parts shelf, made again the same way
//! from new lumber of its own, and put away from wherever it is. The tree (`parts`) lists it
//! all, the shelves along with the stations.
//!
//! Wood a station sets aside comes here: whatever a board set down on a saw displaces, what a
//! cut leaves when you take one side, strips a cut sets loose, and what you put away.

use bevy::{ecs::system::SystemParam, prelude::*};
use woodsim::Length;

use crate::{
    chop::MiterSawStation,
    controls::{Action, Actions},
    feed::{TableSawStation, shelf},
    hand::{Hand, Workpiece, describe_glue_up, from_rack},
    join::BenchStation,
    parts::{Entry, Places, Tree, Where},
    saved::Saved,
    shop::{Part, ShopView, describe_board, describe_size},
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
    /// All of them, first to last.
    pub ids: Vec<u64>,
    pub count: usize,
    pub work: &'a Workpiece,
}

/// Wood going into the inventory or out of it, for the HUD to show it going.
#[derive(Message, Clone, Debug)]
pub struct Moved {
    pub work: Workpiece,
    pub from: Spot,
    pub to: Spot,
}

/// Where wood moves from or to, as the HUD shows it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Spot {
    /// Where the wood is at a station.
    Station(ShopView),
    /// Its line in the inventory.
    Item(u64),
    /// The button just clicked.
    Pressed,
    /// The pointer, where what's in hand is.
    Hand,
    /// Nowhere: thrown away.
    Gone,
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

    /// The first piece on the same shelf as `id` that's just like it: the one its line is
    /// named for.
    pub fn first_like(&self, id: u64) -> u64 {
        let Some(item) = self.get(id) else {
            return id;
        };
        self.items
            .iter()
            .find(|other| other.shelf == item.shelf && other.work == item.work)
            .map_or(id, |other| other.id)
    }

    /// How many pieces are on `shelf`.
    #[cfg(test)]
    pub fn count(&self, shelf: Shelf) -> usize {
        self.items.iter().filter(|item| item.shelf == shelf).count()
    }

    /// The lines of `shelf`: identical pieces together, in the order the first of each was
    /// stored.
    pub fn rows(&self, shelf: Shelf) -> Vec<Row<'_>> {
        let mut rows: Vec<Row> = Vec::new();
        for item in self.items.iter().filter(|item| item.shelf == shelf) {
            match rows.iter_mut().find(|row| *row.work == item.work) {
                Some(row) => {
                    row.count += 1;
                    row.ids.push(item.id);
                }
                None => rows.push(Row {
                    id: item.id,
                    ids: vec![item.id],
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

/// When wood goes into the inventory and out of it, so the HUD can show it after.
#[derive(SystemSet, Clone, Debug, PartialEq, Eq, Hash)]
pub struct InventorySystems;

impl Inventory {
    /// Puts `work` on `shelf`, and says it came from `from`. Returns the line it's on.
    fn put(
        &mut self,
        shelf: Shelf,
        work: Workpiece,
        from: Spot,
        moved: &mut MessageWriter<Moved>,
    ) -> Where {
        let id = self.store(shelf, work.clone());
        moved.write(Moved {
            work,
            from,
            to: Spot::Item(id),
        });
        Where::Shelf(self.first_like(id))
    }
}

impl Plugin for InventoryPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Inventory>()
            .add_message::<Moved>()
            .add_systems(
                Update,
                (collect_set_aside, use_inventory)
                    .chain()
                    .in_set(InventorySystems),
            );
    }
}

/// Puts away what the saws set aside: what a board set down displaced, the other side of a cut
/// when one side's taken, and strips a cut set loose.
fn collect_set_aside(
    mut table_saw: ResMut<TableSawStation>,
    mut miter_saw: ResMut<MiterSawStation>,
    mut inventory: ResMut<Inventory>,
    mut moved: MessageWriter<Moved>,
) {
    if table_saw.has_set_aside() {
        for (shelf, work) in table_saw.take_set_aside() {
            let from = Spot::Station(ShopView::TableSaw);
            inventory.put(shelf, work, from, &mut moved);
        }
    }
    if miter_saw.has_set_aside() {
        for (shelf, work) in miter_saw.take_set_aside() {
            let from = Spot::Station(ShopView::MiterSaw);
            inventory.put(shelf, work, from, &mut moved);
        }
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
        ShopView::Studio => false,
    }
}

/// Where the stations are, and what's in hand: everything the inventory gives wood to or takes
/// it from; and the tree, which follows it.
#[derive(SystemParam)]
struct Stations<'w> {
    view: Res<'w, ShopView>,
    hand: ResMut<'w, Hand>,
    table_saw: ResMut<'w, TableSawStation>,
    miter_saw: ResMut<'w, MiterSawStation>,
    bench: ResMut<'w, BenchStation>,
    tree: ResMut<'w, Tree>,
}

/// Uses the inventory as asked: a piece or new lumber to wherever you are, a piece to the other
/// shelf or the scrap bin, or what's in hand, on the saw, or picked at the bench, put away; a
/// piece anywhere duplicated, or put away from where it is.
fn use_inventory(
    actions: Res<Actions>,
    saved: Res<Saved>,
    mut inventory: ResMut<Inventory>,
    mut moved: MessageWriter<Moved>,
    mut at: Stations,
) {
    let view = *at.view;
    for action in actions.started() {
        match action {
            Action::Take(id) | Action::NewLumber(id) | Action::Load(id)
                if can_use_at(view, &at.table_saw, &at.miter_saw) =>
            {
                let work = match action {
                    Action::Take(id) => inventory.take(id),
                    // A copy with a history of its own, so a part used twice is two pieces of
                    // wood, each to buy and cut.
                    Action::Load(index) => saved.part(index as usize).map(|part| part.duplicate()),
                    _ => lumber(id as usize),
                };
                let Some(work) = work else { continue };
                let to = match view {
                    ShopView::Overview => Spot::Hand,
                    station => Spot::Station(station),
                };
                moved.write(Moved {
                    work: work.clone(),
                    from: Spot::Pressed,
                    to,
                });
                match view {
                    ShopView::TableSaw => at.table_saw.load(work),
                    ShopView::MiterSaw => at.miter_saw.load(work),
                    ShopView::Bench => at.bench.set_down(work, None),
                    ShopView::Overview | ShopView::Studio => {
                        if let Some(held) = at.hand.0.replace(work) {
                            inventory.put(Shelf::Parts, held, Spot::Hand, &mut moved);
                        }
                    }
                }
                if let Action::Take(id) = action {
                    let to = match view {
                        ShopView::TableSaw | ShopView::MiterSaw => Where::Saw(view, Part::Whole),
                        ShopView::Bench => Where::Bench(at.bench.groups.len() - 1),
                        ShopView::Overview | ShopView::Studio => Where::Hand,
                    };
                    at.tree.follow(Where::Shelf(id), to);
                }
            }
            Action::Duplicate(entry) if !matches!(entry.at, Where::Lumber(_)) => {
                let places = Places {
                    hand: &at.hand,
                    table_saw: &at.table_saw,
                    miter_saw: &at.miter_saw,
                    bench: &at.bench,
                    inventory: &inventory,
                    saved: &saved,
                };
                let Some(work) = places.wood(entry) else {
                    continue;
                };
                let line = inventory.put(Shelf::Parts, work.duplicate(), Spot::Pressed, &mut moved);
                at.tree.selected = Some(Entry::at(line));
            }
            Action::Stash(from) => {
                let station = |view| Spot::Station(view);
                let taken = match from {
                    Where::Hand => at
                        .hand
                        .0
                        .take()
                        .map(|work| (Shelf::Parts, work, Spot::Hand)),
                    Where::Saw(ShopView::TableSaw, part) => at
                        .table_saw
                        .take(part)
                        .map(|work| (shelf(part), work, station(ShopView::TableSaw))),
                    Where::Saw(ShopView::MiterSaw, part) => at
                        .miter_saw
                        .take(part)
                        .map(|work| (shelf(part), work, station(ShopView::MiterSaw))),
                    Where::Bench(index) => at
                        .bench
                        .take(index)
                        .map(|work| (Shelf::Parts, work, station(ShopView::Bench))),
                    _ => None,
                };
                if let Some((shelf, work, spot)) = taken {
                    let line = inventory.put(shelf, work, spot, &mut moved);
                    at.tree.follow(from, line);
                }
            }
            Action::MoveShelf(id) => {
                if let Some(work) = inventory.get(id).map(|item| item.work.clone())
                    && inventory.move_to_other_shelf(id)
                {
                    moved.write(Moved {
                        work,
                        from: Spot::Pressed,
                        to: Spot::Item(id),
                    });
                }
            }
            Action::Scrap(id) => {
                if let Some(work) = inventory.take(id) {
                    moved.write(Moved {
                        work,
                        from: Spot::Pressed,
                        to: Spot::Gone,
                    });
                }
            }
            Action::PutAway => {
                if let Some(work) = at.hand.0.take() {
                    let line = inventory.put(Shelf::Parts, work, Spot::Hand, &mut moved);
                    at.tree.follow(Where::Hand, line);
                }
            }
            Action::Store => match view {
                ShopView::TableSaw => at.table_saw.put_away(),
                ShopView::MiterSaw => at.miter_saw.put_away(),
                ShopView::Bench => {
                    let picked = at.bench.picked.map(|picked| Where::Bench(picked.group));
                    if let (Some(work), Some(picked)) = (at.bench.take_picked(), picked) {
                        let from = Spot::Station(ShopView::Bench);
                        let line = inventory.put(Shelf::Parts, work, from, &mut moved);
                        at.tree.follow(picked, line);
                    }
                }
                ShopView::Overview | ShopView::Studio => {}
            },
            Action::CutAgain => match view {
                ShopView::TableSaw => at.table_saw.cut_again(),
                ShopView::MiterSaw => at.miter_saw.cut_again(),
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
        world.init_resource::<Saved>();
        world.init_resource::<Tree>();
        world
    }

    /// Does `action`, and puts away what it set aside.
    fn act(world: &mut World, action: Action) {
        if !world.contains_resource::<Messages<Moved>>() {
            world.init_resource::<Messages<Moved>>();
        }
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
    fn a_saved_part_goes_where_you_are_and_stays_saved() {
        let mut world = shop(ShopView::Bench);
        world.insert_resource(Saved {
            parts: vec![crate::saved::SavedPart {
                path: "saved/part-1.txt".into(),
                work: board(10),
            }],
            unread: Vec::new(),
        });
        act(&mut world, Action::Load(0));
        act(&mut world, Action::Load(0));
        assert_eq!(world.resource::<BenchStation>().groups.len(), 2);
        assert_eq!(world.resource::<Saved>().parts.len(), 1);
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
    fn a_duplicate_of_wood_anywhere_goes_on_the_parts_shelf_made_from_its_own_lumber() {
        let mut world = shop(ShopView::TableSaw);
        let on_saw = Entry::at(Where::Saw(ShopView::TableSaw, Part::Whole));
        act(&mut world, Action::Duplicate(on_saw));
        act(&mut world, Action::Duplicate(on_saw));
        let inventory = world.resource::<Inventory>();
        let rows = inventory.rows(Shelf::Parts);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].count, 2);
        let saw = world.resource::<TableSawStation>().wood();
        assert_eq!(rows[0].work, &saw[0].1);
        let stock = |work: &Workpiece| work.board().unwrap().origin().stock().clone();
        let copies: Vec<_> = rows[0]
            .ids
            .iter()
            .map(|&id| stock(&inventory.get(id).unwrap().work))
            .collect();
        assert!(!copies[0].same(&stock(&saw[0].1)));
        assert!(!copies[0].same(&copies[1]));
        // The copy is picked out, on the line it joined.
        assert_eq!(
            world.resource::<Tree>().selected,
            Some(Entry::at(Where::Shelf(rows[0].id)))
        );
        // New lumber is new already.
        act(&mut world, Action::Duplicate(Entry::at(Where::Lumber(0))));
        assert_eq!(world.resource::<Inventory>().count(Shelf::Parts), 2);
    }

    #[test]
    fn a_board_of_a_glue_up_duplicates_on_its_own() {
        let mut world = shop(ShopView::Bench);
        let panel = crate::work::panel(
            &[Length::from_inches(4), Length::from_inches(2)],
            Length::from_inches(24),
        );
        world
            .resource_mut::<BenchStation>()
            .set_down(Workpiece::GlueUp(panel.clone()), None);
        let member = Entry {
            at: Where::Bench(0),
            member: Some(1),
        };
        act(&mut world, Action::Duplicate(member));
        let inventory = world.resource::<Inventory>();
        assert_eq!(
            inventory.rows(Shelf::Parts)[0].work,
            &Workpiece::Board(panel.members()[1].board.clone())
        );
        assert_eq!(world.resource::<BenchStation>().groups.len(), 1);
    }

    #[test]
    fn wood_on_a_station_is_put_away_from_the_tree_and_stays_picked_out() {
        let mut world = shop(ShopView::Overview);
        let mut bench = world.resource_mut::<BenchStation>();
        bench.set_down(board(10), None);
        bench.set_down(board(12), None);
        world
            .resource_mut::<Tree>()
            .select(Entry::at(Where::Bench(1)));
        act(&mut world, Action::Stash(Where::Bench(1)));
        assert_eq!(world.resource::<BenchStation>().groups.len(), 1);
        let inventory = world.resource::<Inventory>();
        let row = &inventory.rows(Shelf::Parts)[0];
        assert_eq!(row.work, &board(12));
        assert_eq!(
            world.resource::<Tree>().selected,
            Some(Entry::at(Where::Shelf(row.id)))
        );
        // The offcut of a cut on the miter saw goes to the offcuts; the piece is set aside
        // to the parts.
        let mut saw = world.resource_mut::<MiterSawStation>();
        saw.load(board(24));
        saw.lower(1.0);
        saw.raise(1.0);
        act(
            &mut world,
            Action::Stash(Where::Saw(ShopView::MiterSaw, Part::Offcut)),
        );
        let inventory = world.resource::<Inventory>();
        assert_eq!(
            (
                inventory.count(Shelf::Parts),
                inventory.count(Shelf::Offcuts)
            ),
            (2, 1)
        );
    }

    #[test]
    fn a_saved_part_used_twice_is_two_pieces_of_wood() {
        let mut world = shop(ShopView::Bench);
        world.insert_resource(Saved {
            parts: vec![crate::saved::SavedPart {
                path: "saved/part-1.txt".into(),
                work: board(10),
            }],
            unread: Vec::new(),
        });
        act(&mut world, Action::Load(0));
        act(&mut world, Action::Load(0));
        let bench = world.resource::<BenchStation>();
        let stock = |group: usize| {
            bench.groups[group].assembly.members()[0]
                .board
                .origin()
                .stock()
                .clone()
        };
        assert!(!stock(0).same(&stock(1)));
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
