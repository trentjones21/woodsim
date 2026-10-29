//! Changing how wood was made, the way a CAD program edits a feature in a part's history. Any
//! pass that cut a board on its own can be opened from the inventory's tree (`Action::Edit`),
//! its saw set another way a step at a time, as its own cards step it at the saw (`Knob`), with
//! what that makes of the piece shown before anything changes. Then every piece of wood in the
//! shop that pass cut is made again that way, each pass after it made as it was
//! (`woodsim::Remake`), and the glue-ups they're in put back together around them: all of it,
//! or, if any of it can't be, none, and the status says why.
//!
//! Any joint of a glue-up can be taken apart from the tree (`Action::Unglue`), wherever it is but
//! on a saw. At the bench, the picked part's history can be opened in the tree, or the part
//! taken off the glue-up it's in (`Action::Loosen`).

use std::sync::Arc;

use bevy::{ecs::system::SystemParam, prelude::*};
use woodsim::{Angle, Length, MAX_GAUGE, MAX_TILT, Operation, Refusal, Remake, Tool};

use crate::{
    chop::{DEPTH_STOP_MAX, MITER_STOPS, MiterSawStation, STOP_MAX, STOP_MIN},
    controls::{Action, Actions, Step},
    copy::Notice,
    feed::{FENCE_MAX, FENCE_MIN, GAUGE_STOP_MAX, GAUGE_STOP_MIN, HEIGHT_MAX, TableSawStation},
    hand::{Hand, Workpiece},
    inventory::{Inventory, InventorySystems, Shelf},
    join::BenchStation,
    parts::{Detail, Entry, Places, Tree, Where, through_glue_up},
    saved::Saved,
    shop::{ShopView, describe_miter, measured},
    work,
};

/// How long the status says what came of a change, in seconds.
const NOTICE_SECONDS: f64 = 6.0;

/// A setting of a saw pass that can be changed.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Knob {
    Fence,
    GaugeStop,
    GaugeAngle,
    Height,
    Tilt,
    Stop,
    Miter,
    Depth,
}

/// The settings of `tool` that can be changed, in the order the saw's cards have them.
pub fn knobs(tool: Tool) -> Vec<Knob> {
    match tool {
        Tool::TableSaw { saw, .. } if saw.miter_gauge.is_some() => {
            vec![Knob::GaugeStop, Knob::Height, Knob::GaugeAngle, Knob::Tilt]
        }
        Tool::TableSaw { .. } => vec![Knob::Fence, Knob::Height, Knob::Tilt],
        Tool::MiterSaw { .. } => vec![Knob::Stop, Knob::Miter, Knob::Depth],
    }
}

/// What a knob is called, and what it reads on `tool`: ("Rip fence", "3/4\"").
pub fn reading(tool: Tool, knob: Knob) -> (&'static str, String) {
    match (knob, tool) {
        (Knob::Fence, Tool::TableSaw { saw, .. }) => ("Rip fence", measured(saw.fence).to_string()),
        (Knob::GaugeStop, Tool::TableSaw { saw, .. }) => (
            "Gauge stop",
            saw.miter_gauge
                .map_or("none".to_string(), |stop| measured(stop).to_string()),
        ),
        (Knob::GaugeAngle, Tool::TableSaw { saw, .. }) => {
            ("Gauge angle", describe_miter(saw.gauge_angle))
        }
        (Knob::Height, Tool::TableSaw { saw, .. }) => {
            ("Blade height", measured(saw.height).to_string())
        }
        (Knob::Tilt, Tool::TableSaw { saw, .. }) => (
            "Blade tilt",
            match saw.tilt {
                Angle::ZERO => "square".to_string(),
                tilt => tilt.to_string(),
            },
        ),
        (Knob::Stop, Tool::MiterSaw { length, .. }) => ("Length", measured(length).to_string()),
        (Knob::Miter, Tool::MiterSaw { saw, .. }) => ("Miter", describe_miter(saw.miter)),
        (Knob::Depth, Tool::MiterSaw { bottom, .. }) => (
            "Depth stop",
            match depth(bottom) {
                Some(stop) => format!("{} up", measured(stop)),
                None => "off".to_string(),
            },
        ),
        _ => ("", String::new()),
    }
}

/// The depth stop that leaves a miter saw's blade at `bottom`: off, if it goes through the
/// table.
fn depth(bottom: Length) -> Option<Length> {
    (bottom >= Length::ZERO).then_some(bottom)
}

/// `tool` with `knob` stepped as `step` says, the way the saw's own card steps it: lengths
/// 1/16" (big, 1", or 1/4" up and down), angles 1° (big, to the next place the saw clicks
/// into), and only as far as the saw goes.
pub fn stepped(tool: Tool, knob: Knob, step: Step) -> Tool {
    let sixteenth = Length::from_inch_fraction(1, 16);
    let length = |now: Length, big: Length, least: Length, most: Length| {
        let by = if step.big { big } else { sixteenth };
        let to = if step.more { now + by } else { now - by };
        to.clamp(least, most)
    };
    let angle = |now: Angle, least: Angle, most: Angle| {
        let to = if step.big {
            let stops = MITER_STOPS.iter().flat_map(|&stop| [stop, -stop]);
            match step.more {
                true => stops.filter(|&stop| stop > now).min().unwrap_or(most),
                false => stops.filter(|&stop| stop < now).max().unwrap_or(least),
            }
        } else {
            let by = Angle::from_degrees(1);
            if step.more { now + by } else { now + -by }
        };
        to.clamp(least, most)
    };
    let (inch, quarter) = (Length::from_inches(1), Length::from_inch_fraction(1, 4));
    match tool {
        Tool::TableSaw {
            mut saw,
            lie,
            travel,
        } => {
            match knob {
                Knob::Fence => saw.fence = length(saw.fence, inch, FENCE_MIN, FENCE_MAX),
                Knob::GaugeStop => {
                    saw.miter_gauge = saw
                        .miter_gauge
                        .map(|stop| length(stop, inch, GAUGE_STOP_MIN, GAUGE_STOP_MAX));
                }
                Knob::GaugeAngle => {
                    saw.gauge_angle = angle(saw.gauge_angle, -MAX_GAUGE, MAX_GAUGE);
                }
                Knob::Height => saw.height = length(saw.height, quarter, Length::ZERO, HEIGHT_MAX),
                Knob::Tilt => saw.tilt = angle(saw.tilt, Angle::ZERO, MAX_TILT),
                Knob::Stop | Knob::Miter | Knob::Depth => {}
            }
            Tool::TableSaw { saw, lie, travel }
        }
        Tool::MiterSaw {
            mut saw,
            lie,
            length: stop,
            bottom,
        } => {
            let (stop, bottom) = match knob {
                Knob::Stop => (length(stop, inch, STOP_MIN, STOP_MAX), bottom),
                Knob::Miter => {
                    saw.miter = angle(saw.miter, -saw.max_miter, saw.max_miter);
                    (stop, bottom)
                }
                Knob::Depth => {
                    let by = if step.big { quarter } else { sixteenth };
                    let to = match (depth(bottom), step.more) {
                        (None, true) => Some(Length::ZERO),
                        (None, false) => None,
                        (Some(stop), true) => Some((stop + by).min(DEPTH_STOP_MAX)),
                        (Some(stop), false) => Some(stop - by).filter(|&to| to >= Length::ZERO),
                    };
                    (stop, to.unwrap_or(saw.stroke.1))
                }
                _ => (stop, bottom),
            };
            Tool::MiterSaw {
                saw,
                lie,
                length: stop,
                bottom,
            }
        }
    }
}

/// The pass being changed, if any.
#[derive(Resource, Default)]
pub struct Editor {
    pub editing: Option<Editing>,
}

/// A pass being changed: its line in the tree, the pass, the saw as it's being set, and what
/// that makes of the piece it was opened from, or why it can't.
pub struct Editing {
    pub entry: Entry,
    pub pass: Arc<Operation>,
    pub tool: Tool,
    pub preview: Result<Workpiece, String>,
}

impl Editing {
    /// Whether the saw's set any differently from how the pass was made.
    pub fn changed(&self) -> bool {
        self.tool != self.pass.tool
    }

    /// Works out what the piece comes out as with the saw set as it is now.
    fn preview(&mut self, wood: &Workpiece) {
        let mut remake = Remake::new(&self.pass, self.tool);
        self.preview = match work::remade(&mut remake, wood) {
            Ok(made) => Ok(made.unwrap_or_else(|| wood.clone())),
            Err(why) => Err(said(&why)),
        };
    }
}

/// Why wood can't be made a new way, in words.
fn said(why: &Refusal) -> String {
    match why {
        Refusal::GlueUpPass(_) => {
            "a later cut went through a glue-up, which can't be cut again".to_string()
        }
        why => why.to_string(),
    }
}

/// The pass numbered `number` that cut the board at `entry`, if one did.
pub fn pass_of(places: &Places, entry: Entry) -> Option<Arc<Operation>> {
    let Some(Detail::Pass(number)) = entry.detail else {
        return None;
    };
    let Some(Workpiece::Board(board)) = places.wood(entry.wood()) else {
        return None;
    };
    board
        .origin()
        .operations()
        .into_iter()
        .find(|pass| pass.number == number)
}

pub struct EditPlugin;

impl Plugin for EditPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Editor>()
            .add_systems(Update, edit_wood.before(InventorySystems));
    }
}

/// Everywhere wood is, to change.
#[derive(SystemParam)]
struct Wood<'w> {
    view: Res<'w, ShopView>,
    hand: ResMut<'w, Hand>,
    table_saw: ResMut<'w, TableSawStation>,
    miter_saw: ResMut<'w, MiterSawStation>,
    bench: ResMut<'w, BenchStation>,
    inventory: ResMut<'w, Inventory>,
    saved: Res<'w, Saved>,
}

impl Wood<'_> {
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

    /// Makes every piece of wood in the shop again as `remake` makes it: what's in hand, on the
    /// saws, on the bench, and on the shelves; all of it, or, if any of it can't be, none.
    /// Returns how many pieces changed.
    fn remake(&mut self, remake: &mut Remake) -> Result<usize, Refusal> {
        for apply in [false, true] {
            let mut changed = 0;
            if let Some(work) = &self.hand.0
                && let Some(made) = work::remade(remake, work)?
            {
                changed += 1;
                if apply {
                    self.hand.0 = Some(made);
                }
            }
            changed += usize::from(self.table_saw.remake(remake, apply)?);
            changed += usize::from(self.miter_saw.remake(remake, apply)?);
            changed += self.bench.remake(remake, apply)?;
            let mut made = Vec::new();
            for item in self.inventory.items() {
                if let Some(work) = work::remade(remake, &item.work)? {
                    made.push((item.id, work));
                }
            }
            changed += made.len();
            if apply {
                for (id, work) in made {
                    self.inventory.replace(id, work);
                }
                return Ok(changed);
            }
        }
        unreachable!("the second time through applies it")
    }

    /// Takes the joints `joints` of the glue-up at `at` apart: on the bench, the pieces set
    /// down beside each other; on a shelf, or in hand, the biggest piece stays and the others
    /// go on the parts shelf. Returns false if it can't be done there.
    fn unglue(&mut self, at: Where, joints: &[usize]) -> bool {
        let glue_up = match at {
            Where::Bench(index) => return self.bench.take_apart_joints(index, joints),
            Where::Shelf(id) => self.inventory.get(id).map(|item| item.work.clone()),
            Where::Hand => self.hand.0.clone(),
            _ => None,
        };
        let Some(Workpiece::GlueUp(glue_up)) = glue_up else {
            return false;
        };
        if joints.iter().any(|&joint| joint >= glue_up.joints().len()) {
            return false;
        }
        let mut pieces = glue_up.taken_apart(joints);
        let biggest = (0..pieces.len())
            .max_by_key(|&k| (pieces[k].members().len(), std::cmp::Reverse(k)))
            .expect("at least one piece");
        let stays = work::workpiece(pieces.remove(biggest));
        match at {
            Where::Shelf(id) => self.inventory.replace(id, stays),
            _ => self.hand.0 = Some(stays),
        }
        for piece in pieces {
            self.inventory.store(Shelf::Parts, work::workpiece(piece));
        }
        true
    }
}

/// Opens passes to change, changes them, and makes the wood again; takes joints apart; and at
/// the bench, opens the picked part's history, or takes it off its glue-up.
fn edit_wood(
    actions: Res<Actions>,
    mut editor: ResMut<Editor>,
    mut tree: ResMut<Tree>,
    mut wood: Wood,
    time: Res<Time>,
    mut notice: ResMut<Notice>,
) {
    let mut say = |message: String| {
        notice.0 = Some((message, time.elapsed_secs_f64() + NOTICE_SECONDS));
    };
    for action in actions.started() {
        match action {
            Action::Select(entry) => {
                if editor
                    .editing
                    .as_ref()
                    .is_some_and(|editing| editing.entry != entry)
                {
                    editor.editing = None;
                }
            }
            Action::Edit(entry) => {
                let places = wood.places();
                let (Some(pass), Some(piece)) =
                    (pass_of(&places, entry), places.wood(entry.wood()))
                else {
                    continue;
                };
                if through_glue_up(pass.tool) {
                    say("That cut went through a glue-up, so it can't be made again.".to_string());
                    continue;
                }
                tree.selected = Some(entry);
                editor.editing = Some(Editing {
                    entry,
                    tool: pass.tool,
                    pass,
                    preview: Ok(piece),
                });
            }
            Action::Tweak(knob, step) => {
                let Some(editing) = &mut editor.editing else {
                    continue;
                };
                editing.tool = stepped(editing.tool, knob, step);
                if let Some(piece) = wood.places().wood(editing.entry.wood()) {
                    editing.preview(&piece);
                }
            }
            Action::Remake => {
                let Some(editing) = &editor.editing else {
                    continue;
                };
                if !editing.changed() {
                    editor.editing = None;
                    continue;
                }
                let mut remake = Remake::new(&editing.pass, editing.tool);
                match wood.remake(&mut remake) {
                    Ok(count) => {
                        let pieces = match count {
                            1 => "1 piece".to_string(),
                            count => format!("{count} pieces"),
                        };
                        say(format!(
                            "Made {pieces} again with the saw set the new way, and every \
                             cut after it as it was."
                        ));
                        editor.editing = None;
                    }
                    Err(why) => say(format!("Can't make it that way: {}.", said(&why))),
                }
            }
            Action::StopEditing => editor.editing = None,
            Action::Unglue(entry) => {
                let Some(Detail::Joint(joint)) = entry.detail else {
                    continue;
                };
                if wood.unglue(entry.at, &[joint]) {
                    tree.selected = Some(Entry::at(entry.at));
                }
            }
            Action::History if *wood.view == ShopView::Bench => {
                let Some(picked) = wood.bench.picked else {
                    continue;
                };
                let at = Where::Bench(picked.group);
                let entry = match wood.bench.groups[picked.group].is_loose() {
                    true => Entry::at(at),
                    false => Entry::member(at, picked.member),
                };
                tree.selected = Some(entry);
                tree.open(entry);
            }
            Action::Loosen if *wood.view == ShopView::Bench => {
                let joints = wood.bench.picked_joints();
                if let Some(picked) = wood.bench.picked {
                    wood.bench.take_apart_joints(picked.group, &joints);
                }
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use bevy::ecs::system::RunSystemOnce;
    use woodsim::{Blade, Board, MiterSaw, Orientation, Pass, TableSaw};

    use super::*;
    use crate::parts::Detail;

    /// A shop at `view` with nothing anywhere but what the test puts there.
    fn shop(view: ShopView) -> World {
        let mut world = World::new();
        world.insert_resource(view);
        world.init_resource::<Actions>();
        world.init_resource::<Hand>();
        world.init_resource::<TableSawStation>();
        world.init_resource::<MiterSawStation>();
        world.init_resource::<BenchStation>();
        world.init_resource::<Inventory>();
        world.init_resource::<Saved>();
        world.init_resource::<Tree>();
        world.init_resource::<Editor>();
        world.init_resource::<Notice>();
        world.init_resource::<Time>();
        world
    }

    fn act(world: &mut World, action: Action) {
        world.resource_mut::<Actions>().start(action);
        world.run_system_once(edit_wood).unwrap();
        *world.resource_mut::<Actions>() = Actions::default();
    }

    fn said(world: &World) -> String {
        world
            .resource::<Notice>()
            .0
            .clone()
            .map(|(said, _)| said)
            .unwrap_or_default()
    }

    /// A 1x4, 24" long, ripped to a 3/4" strip: the strip, and what's left of the 1x4.
    fn ripped() -> (Board, Board) {
        let stock = Board::new(
            Length::from_inches(24),
            Length::from_inch_fraction(7, 2),
            Length::from_inch_fraction(3, 4),
        );
        let Tool::TableSaw { saw, .. } = rip() else {
            unreachable!()
        };
        let Ok(Pass::Cut(cut)) = saw.rip(&stock, Orientation::default()) else {
            panic!("a strip");
        };
        (cut.piece.board, cut.offcut.unwrap().board)
    }

    fn width(work: &Workpiece) -> Length {
        match work {
            Workpiece::Board(board) => board.width(),
            Workpiece::GlueUp(_) => panic!("a board"),
        }
    }

    #[test]
    fn a_rip_changed_from_the_tree_changes_every_piece_it_cut() {
        let mut world = shop(ShopView::Overview);
        let (strip, rest) = ripped();
        let number = strip.origin().operations()[0].number;
        let mut inventory = world.resource_mut::<Inventory>();
        let on_shelf = inventory.store(Shelf::Parts, Workpiece::Board(strip));
        let offcut = inventory.store(Shelf::Offcuts, Workpiece::Board(rest));
        let pass = Entry {
            detail: Some(Detail::Pass(number)),
            ..Entry::at(Where::Shelf(on_shelf))
        };
        act(&mut world, Action::Edit(pass));
        assert_eq!(world.resource::<Tree>().selected, Some(pass));
        let wider = Step {
            more: true,
            big: false,
        };
        act(&mut world, Action::Tweak(Knob::Fence, wider));
        act(&mut world, Action::Tweak(Knob::Fence, wider));
        let editor = world.resource::<Editor>();
        let preview = editor.editing.as_ref().unwrap().preview.as_ref().unwrap();
        assert_eq!(width(preview), Length::from_inch_fraction(7, 8));
        act(&mut world, Action::Remake);
        assert!(world.resource::<Editor>().editing.is_none());
        let inventory = world.resource::<Inventory>();
        assert_eq!(
            width(&inventory.get(on_shelf).unwrap().work),
            Length::from_inch_fraction(7, 8)
        );
        // The other side of the same rip, on the other shelf, is narrower by as much.
        let kerf = Blade::FULL_KERF;
        assert_eq!(
            width(&inventory.get(offcut).unwrap().work),
            Length::from_inch_fraction(7, 2) - Length::from_inch_fraction(7, 8) - kerf
        );
        assert!(
            said(&world).starts_with("Made 2 pieces again"),
            "{}",
            said(&world)
        );
    }

    #[test]
    fn a_change_that_would_stop_the_cut_changes_nothing_and_says_why() {
        let mut world = shop(ShopView::Overview);
        let (strip, _) = ripped();
        let number = strip.origin().operations()[0].number;
        world.resource_mut::<Hand>().0 = Some(Workpiece::Board(strip.clone()));
        let pass = Entry {
            detail: Some(Detail::Pass(number)),
            ..Entry::at(Where::Hand)
        };
        act(&mut world, Action::Edit(pass));
        let lower = Step {
            more: false,
            big: true,
        };
        act(&mut world, Action::Tweak(Knob::Height, lower));
        act(&mut world, Action::Tweak(Knob::Height, lower));
        act(&mut world, Action::Remake);
        assert_eq!(world.resource::<Hand>().0, Some(Workpiece::Board(strip)));
        assert!(world.resource::<Editor>().editing.is_some());
        assert_eq!(
            said(&world),
            "Can't make it that way: it doesn't cut it apart."
        );
    }

    /// The cut on a saw, opened from its measured piece and set `knob` a big step further.
    fn change_the_cut_on(world: &mut World, view: ShopView, knob: Knob) {
        let places = |world: &World| -> Option<Workpiece> {
            let wood = match view {
                ShopView::TableSaw => world.resource::<TableSawStation>().wood(),
                _ => world.resource::<MiterSawStation>().wood(),
            };
            wood.into_iter()
                .find(|(part, _)| *part == crate::shop::Part::Piece)
                .map(|(_, work)| work)
        };
        let piece = places(world).expect("a measured piece");
        let number = work::last_pass(&piece).unwrap().number;
        let pass = Entry {
            detail: Some(Detail::Pass(number)),
            ..Entry::at(Where::Saw(view, crate::shop::Part::Piece))
        };
        act(world, Action::Edit(pass));
        act(
            world,
            Action::Tweak(
                knob,
                Step {
                    more: true,
                    big: true,
                },
            ),
        );
        act(world, Action::Remake);
        assert!(
            said(world).starts_with("Made 1 piece again"),
            "{}",
            said(world)
        );
    }

    #[test]
    fn the_cut_on_a_saw_is_made_again_where_it_lies() {
        let mut world = shop(ShopView::MiterSaw);
        let mut saw = world.resource_mut::<MiterSawStation>();
        saw.load(Workpiece::Board(Board::new(
            Length::from_inches(24),
            Length::from_inch_fraction(7, 2),
            Length::from_inch_fraction(3, 4),
        )));
        saw.set_stop(Length::from_inches(10));
        saw.lower(1.0);
        saw.raise(1.0);
        change_the_cut_on(&mut world, ShopView::MiterSaw, Knob::Stop);
        let wood = world.resource::<MiterSawStation>().wood();
        let length = |work: &Workpiece| match work {
            Workpiece::Board(board) => board.length(),
            Workpiece::GlueUp(_) => panic!("a board"),
        };
        assert_eq!(length(&wood[0].1), Length::from_inches(11));
        assert_eq!(
            length(&wood[1].1),
            Length::from_inches(13) - Blade::FULL_KERF
        );

        let mut world = shop(ShopView::TableSaw);
        world.resource_mut::<TableSawStation>().push(10.0);
        change_the_cut_on(&mut world, ShopView::TableSaw, Knob::Fence);
        let wood = world.resource::<TableSawStation>().wood();
        assert_eq!(width(&wood[0].1), Length::from_inches(7));
    }

    #[test]
    fn joints_come_apart_on_the_bench_from_the_tree_or_the_picked_part() {
        let mut world = shop(ShopView::Bench);
        let panel = crate::work::panel(&[Length::from_inches(4); 3], Length::from_inches(24));
        world
            .resource_mut::<BenchStation>()
            .set_down(Workpiece::GlueUp(panel), None);
        let joint = Entry {
            detail: Some(Detail::Joint(1)),
            ..Entry::at(Where::Bench(0))
        };
        act(&mut world, Action::Unglue(joint));
        let bench = world.resource::<BenchStation>();
        assert_eq!(bench.groups.len(), 2);
        assert_eq!(bench.groups[0].assembly.members().len(), 2);
        assert_eq!(
            world.resource::<Tree>().selected,
            Some(Entry::at(Where::Bench(0)))
        );
        // Pick the first board and take it off what's left.
        let surface = woodsim::Surface::Top;
        world.resource_mut::<BenchStation>().picked = Some(crate::join::Spot {
            group: 0,
            member: 0,
            surface,
        });
        act(&mut world, Action::Loosen);
        let bench = world.resource::<BenchStation>();
        assert_eq!(bench.groups.len(), 3);
        assert!(bench.groups.iter().all(|group| group.is_loose()));
    }

    fn rip() -> Tool {
        Tool::TableSaw {
            saw: TableSaw {
                blade: Blade {
                    diameter: Length::from_inches(10),
                    kerf: Blade::FULL_KERF,
                },
                fence: Length::from_inch_fraction(3, 4),
                height: Length::from_inches(1),
                miter_gauge: None,
                gauge_angle: Angle::ZERO,
                tilt: Angle::ZERO,
            },
            lie: woodsim::Lie::Board(Orientation::default()),
            travel: (Length::from_inches(-5), Length::from_inches(29)),
        }
    }

    fn chop() -> Tool {
        let saw = MiterSaw::twelve_inch();
        Tool::MiterSaw {
            saw,
            lie: woodsim::Lie::Board(Orientation::default()),
            length: Length::from_inches(10),
            bottom: saw.stroke.1,
        }
    }

    fn step(more: bool, big: bool) -> Step {
        Step { more, big }
    }

    #[test]
    fn a_pass_steps_the_way_its_saw_does() {
        let wider = stepped(rip(), Knob::Fence, step(true, false));
        assert_eq!(reading(wider, Knob::Fence).1, "13/16\"");
        let much_wider = stepped(rip(), Knob::Fence, step(true, true));
        assert_eq!(reading(much_wider, Knob::Fence).1, "1-3/4\"");
        let tilted = stepped(rip(), Knob::Tilt, step(true, true));
        assert_eq!(reading(tilted, Knob::Tilt).1, "15°");
        let square = stepped(rip(), Knob::Tilt, step(false, false));
        assert_eq!(reading(square, Knob::Tilt).1, "square");
        assert_eq!(knobs(rip()), [Knob::Fence, Knob::Height, Knob::Tilt]);
        let swung = stepped(chop(), Knob::Miter, step(false, true));
        assert_eq!(reading(swung, Knob::Miter).1, "15° left");
        assert_eq!(reading(chop(), Knob::Depth).1, "off");
        let stopped = stepped(chop(), Knob::Depth, step(true, false));
        assert_eq!(reading(stopped, Knob::Depth).1, "0\" up");
        let higher = stepped(stopped, Knob::Depth, step(true, true));
        assert_eq!(reading(higher, Knob::Depth).1, "1/4\" up");
        let off = stepped(stopped, Knob::Depth, step(false, false));
        assert_eq!(reading(off, Knob::Depth).1, "off");
        assert_eq!(
            reading(stepped(chop(), Knob::Stop, step(false, true)), Knob::Stop).1,
            "9\""
        );
    }

    #[test]
    fn a_change_shows_what_it_makes_before_it_makes_it() {
        let stock = Board::new(
            Length::from_inches(24),
            Length::from_inch_fraction(7, 2),
            Length::from_inch_fraction(3, 4),
        );
        let Tool::TableSaw { saw, .. } = rip() else {
            unreachable!()
        };
        let Ok(Pass::Cut(cut)) = saw.rip(&stock, Orientation::default()) else {
            panic!("a strip");
        };
        let strip = Workpiece::Board(cut.piece.board);
        let Workpiece::Board(board) = &strip else {
            unreachable!()
        };
        let pass = board.origin().operations()[0].clone();
        let mut editing = Editing {
            entry: Entry::at(Where::Hand),
            tool: stepped(pass.tool, Knob::Fence, step(true, true)),
            pass,
            preview: Ok(strip.clone()),
        };
        assert!(editing.changed());
        editing.preview(&strip);
        let Ok(Workpiece::Board(wider)) = &editing.preview else {
            panic!("a wider strip");
        };
        assert_eq!(wider.width(), Length::from_inch_fraction(7, 4));
        editing.tool = stepped(editing.tool, Knob::Height, step(false, true));
        editing.tool = stepped(editing.tool, Knob::Height, step(false, true));
        editing.preview(&strip);
        assert_eq!(
            editing.preview,
            Err("it doesn't cut it apart".to_string()),
            "a blade 1/2\" high only grooves a 3/4\" board"
        );
    }
}
