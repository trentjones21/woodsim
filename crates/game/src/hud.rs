//! Everything drawn over the shop. Along the top, a button back to the shop and a tab for each
//! station, and buttons for the inventory and the controls. At a station, its settings, each a
//! card with buttons to step it, and along the bottom a dock of buttons for what can be done
//! there, over a line on what's going on. What's in hand shows at the top. Down the right, the
//! inventory: parts, offcuts, and new lumber, each with buttons to use it where you are. Buttons
//! start the same `Action`s their keys do, show their key, say what they do when the pointer
//! rests on them, and dim when they'd do nothing.

use bevy::{
    diagnostic::{DiagnosticsStore, FrameTimeDiagnosticsPlugin},
    ecs::system::SystemParam,
    input::mouse::{MouseScrollUnit, MouseWheel},
    picking::{
        PickingSystems,
        hover::{HoverMap, Hovered},
        pointer::PointerId,
    },
    prelude::*,
    time::common_conditions::on_real_timer,
    ui::{InteractionDisabled, Pressed},
    ui_widgets::{Activate, Button},
};
use std::time::Duration;
use woodsim::Fastener;

use crate::{
    bench,
    chop::MiterSawStation,
    controls::{Action, Actions, Arrow, Step, key_for},
    feed::TableSawStation,
    flights::{HandPicture, InventoryButton, LastPress, ListArea, RowOf},
    hand::{self, Hand},
    icons::{Icon, Icons},
    inventory::{self, Inventory, LUMBER, Shelf},
    join::BenchStation,
    shop::{HoveredStation, ShopView, describe_miter},
};

const INK: Color = Color::srgb(0.17, 0.2, 0.27);
const PAPER: Color = Color::srgba(1.0, 0.98, 0.94, 0.96);
const FACE: Color = Color::srgb(1.0, 1.0, 1.0);
const FACE_HOVER: Color = Color::srgb(1.0, 0.92, 0.78);
const ACCENT: Color = Color::srgb(0.93, 0.47, 0.15);
const GO: Color = Color::srgb(0.27, 0.68, 0.4);
const EDGE: Color = Color::srgba(0.17, 0.2, 0.27, 0.2);
const SHADOW: Color = Color::srgba(0.1, 0.12, 0.18, 0.22);

/// How big buttons are: in the dock, on a settings card, and the tabs along the top.
const DOCK_BUTTON: f32 = 54.0;
const STEP_BUTTON: f32 = 34.0;
const TAB_HEIGHT: f32 = 44.0;

/// Whether the mouse is over anything drawn over the shop, a button or a panel, and whether it
/// was when its button last went down.
#[derive(Resource, Default)]
pub struct OnHud {
    over: bool,
    pressed: bool,
}

impl OnHud {
    /// Whether the mouse button went down over the HUD, so the drag that follows isn't one to
    /// look around with.
    pub fn pressed(&self) -> bool {
        self.pressed
    }

    /// Whether the mouse is over the HUD now, so scrolling there doesn't zoom the view.
    pub fn over(&self) -> bool {
        self.over
    }
}

pub struct HudPlugin;

/// The game's font, Nunito, under the SIL Open Font License (`fonts/OFL.txt`): round and
/// friendly, easy to read small, with a degree sign.
const FONT: &[u8] = include_bytes!("../fonts/Nunito.ttf");

impl Plugin for HudPlugin {
    fn build(&self, app: &mut App) {
        // In place of Bevy's own font, so every piece of text is set in it.
        app.world_mut()
            .resource_mut::<Assets<Font>>()
            .insert(AssetId::default(), Font::from_bytes(FONT.to_vec()))
            .expect("the default font's place");
        app.add_plugins(FrameTimeDiagnosticsPlugin::default())
            .init_resource::<Icons>()
            .init_resource::<Showing>()
            .init_resource::<OnHud>()
            .add_systems(Startup, spawn_hud)
            .add_observer(activate)
            .add_systems(
                PreUpdate,
                (hold_pressed, notice_hud).after(PickingSystems::Last),
            )
            .add_systems(
                Update,
                (
                    toggle_panels,
                    build_for_view,
                    build_inventory,
                    scroll_inventory,
                    update_controls,
                    update_text,
                    show_tip,
                )
                    .chain(),
            )
            // Often enough to follow, slow enough to read.
            .add_systems(
                Update,
                update_fps.run_if(on_real_timer(Duration::from_millis(250))),
            );
    }
}

/// Which panels are open, and which view the dock and cards were last built for.
#[derive(Resource)]
struct Showing {
    help: bool,
    inventory: bool,
    built_for: Option<ShopView>,
}

impl Default for Showing {
    fn default() -> Showing {
        Showing {
            help: false,
            inventory: true,
            built_for: None,
        }
    }
}

/// A button that starts an action, and how it looks.
#[derive(Component, Clone, Copy)]
struct Control {
    action: Action,
    look: Look,
    tip: TipAt,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Look {
    /// A round button with an icon.
    Round,
    /// A green one, for what finishes a job.
    Go,
    /// A small one on a settings card.
    Small,
    /// A tab along the top: an icon and a name.
    Tab,
}

/// The icon on a button.
#[derive(Component)]
struct ControlIcon;

/// What the button under the pointer does. There's one, over everything, so a button in the
/// inventory's scrolling list can show it outside the list.
#[derive(Component)]
struct Tip;

/// Where the dock's buttons and the settings cards go, rebuilt for each view.
#[derive(Component)]
struct Dock;

#[derive(Component)]
struct Cards;

/// A line of text that follows the game.
#[derive(Component, Clone, Copy, PartialEq, Eq)]
enum Readout {
    Status,
    Hand,
    Fps,
    Help,
    Inventory,
    Setting(Setting),
}

/// What a settings card shows.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Setting {
    Guide,
    Blade,
    GaugeAngle,
    Tilt,
    Stop,
    Miter,
    Depth,
    OnBench,
}

/// The panel a readout sits in, to hide it when there's nothing to show.
#[derive(Component)]
struct PanelOf(Readout);

/// A button in the dock: what it does and what it shows.
#[derive(Clone, Copy)]
struct Tool {
    action: Action,
    icon: Icon,
    look: Look,
}

const fn tool(action: Action, icon: Icon) -> Tool {
    Tool {
        action,
        icon,
        look: Look::Round,
    }
}

/// The dock's buttons at `view`, in groups.
fn dock(view: ShopView) -> Vec<Vec<Tool>> {
    use Action::*;
    let turning = vec![
        tool(TurnLeft, Icon::TurnLeft),
        tool(TurnRight, Icon::TurnRight),
        tool(Flip, Icon::Flip),
    ];
    let stand_up = tool(StandUp, Icon::StandUp);
    let rest = tool(Rest, Icon::Rest);
    match view {
        ShopView::Overview => vec![
            vec![
                tool(RotateLeft, Icon::TurnLeft),
                tool(RotateRight, Icon::TurnRight),
            ],
            vec![tool(ZoomIn, Icon::ZoomIn), tool(ZoomOut, Icon::ZoomOut)],
        ],
        ShopView::TableSaw => vec![
            vec![tool(SwitchGuide, Icon::SwitchGuide)],
            [turning.clone(), vec![stand_up, rest]].concat(),
            vec![tool(Push, Icon::Push), tool(BringBack, Icon::BringBack)],
            vec![tool(CutAgain, Icon::Again), tool(Store, Icon::Store)],
        ],
        ShopView::MiterSaw => vec![
            [turning, vec![stand_up, rest]].concat(),
            vec![tool(Lower, Icon::Lower)],
            vec![tool(CutAgain, Icon::Again), tool(Store, Icon::Store)],
        ],
        ShopView::Bench => vec![
            turning,
            vec![
                tool(Use(Fastener::Glue), Icon::Glue),
                tool(Use(Fastener::Brads), Icon::Brads),
                tool(Use(Fastener::Screws), Icon::Screws),
            ],
            [Arrow::Left, Arrow::Up, Arrow::Down, Arrow::Right]
                .map(|arrow| tool(Slide(arrow, false), Icon::Point(arrow)))
                .to_vec(),
            vec![
                Tool {
                    look: Look::Go,
                    ..tool(Fasten, Icon::Fasten)
                },
                tool(LetGo, Icon::LetGo),
            ],
            vec![
                tool(PickUp, Icon::PickUp),
                tool(Store, Icon::Store),
                tool(TakeApart, Icon::TakeApart),
                tool(Plans, Icon::Plans),
            ],
        ],
    }
}

/// The settings cards at `view`: what each shows, its icon, and which way its steps go.
fn cards(view: ShopView) -> Vec<(Setting, Icon, Option<bool>)> {
    // Steps across (left and right), up and down, or none.
    match view {
        ShopView::Overview => Vec::new(),
        ShopView::TableSaw => vec![
            (Setting::Guide, Icon::Fence, Some(true)),
            (Setting::Blade, Icon::Blade, Some(false)),
            (Setting::GaugeAngle, Icon::Miter, Some(true)),
            (Setting::Tilt, Icon::Tilt, Some(true)),
        ],
        ShopView::MiterSaw => vec![
            (Setting::Stop, Icon::Stop, Some(true)),
            (Setting::Miter, Icon::Miter, Some(true)),
            (Setting::Depth, Icon::Depth, Some(false)),
        ],
        ShopView::Bench => vec![(Setting::OnBench, Icon::Bench, None)],
    }
}

/// The action a card's step buttons start: smaller or bigger, less or more.
fn stepping(setting: Setting, step: Step) -> Option<Action> {
    Some(match setting {
        Setting::Guide => Action::Guide(step),
        Setting::Blade => Action::Blade(step),
        Setting::GaugeAngle => Action::Miter(step),
        // The blade leans left as it tilts, so the left step tilts it further.
        Setting::Tilt => Action::Tilt(Step {
            more: !step.more,
            ..step
        }),
        Setting::Stop => Action::Stop(step),
        Setting::Miter => Action::Miter(step),
        Setting::Depth => Action::Depth(step),
        Setting::OnBench => return None,
    })
}

/// What the HUD shows: where the camera is, the stations, what's in hand, and which panels are
/// open.
#[derive(SystemParam)]
struct Game<'w> {
    view: Res<'w, ShopView>,
    table_saw: Res<'w, TableSawStation>,
    miter_saw: Res<'w, MiterSawStation>,
    bench: Res<'w, BenchStation>,
    hand: Res<'w, Hand>,
    showing: Res<'w, Showing>,
    inventory: Res<'w, Inventory>,
}

impl Game<'_> {
    fn shop(&self) -> Shop<'_> {
        Shop {
            view: *self.view,
            table_saw: &self.table_saw,
            miter_saw: &self.miter_saw,
            bench: &self.bench,
            hand: &self.hand,
            showing: &self.showing,
            inventory: &self.inventory,
        }
    }

    fn is_changed(&self) -> bool {
        self.view.is_changed()
            || self.table_saw.is_changed()
            || self.miter_saw.is_changed()
            || self.bench.is_changed()
            || self.hand.is_changed()
            || self.showing.is_changed()
            || self.inventory.is_changed()
    }
}

/// What's needed to say what a button does and whether it would do anything.
struct Shop<'a> {
    view: ShopView,
    table_saw: &'a TableSawStation,
    miter_saw: &'a MiterSawStation,
    bench: &'a BenchStation,
    hand: &'a Hand,
    showing: &'a Showing,
    inventory: &'a Inventory,
}

impl Shop<'_> {
    /// What `action` does, for its tooltip.
    fn describe(&self, action: Action) -> String {
        use Action::*;
        let step = |step: Step, less: &str, more: &str, small: &str, big: &str| {
            format!(
                "{} {}",
                if step.more { more } else { less },
                if step.big { big } else { small }
            )
        };
        match action {
            Back | GoTo(ShopView::Overview) => "Back to the shop".to_string(),
            GoTo(ShopView::TableSaw) => "Table saw".to_string(),
            GoTo(ShopView::MiterSaw) => "Miter saw".to_string(),
            GoTo(ShopView::Bench) => "Workbench".to_string(),
            NextStation => "Next station".to_string(),
            Pan(_) => "Look around".to_string(),
            RotateLeft => "Turn the view left".to_string(),
            RotateRight => "Turn the view right".to_string(),
            ZoomIn => "Zoom in".to_string(),
            ZoomOut => "Zoom out".to_string(),
            TurnLeft if self.view == ShopView::Bench => "Turn the picked part left".to_string(),
            TurnRight if self.view == ShopView::Bench => "Turn the picked part right".to_string(),
            Flip if self.view == ShopView::Bench => "Flip the picked part over".to_string(),
            TurnLeft => "Turn the board left".to_string(),
            TurnRight => "Turn the board right".to_string(),
            Flip => "Flip the board over".to_string(),
            StandUp => "Stand it on an edge, then on end, then lay it down".to_string(),
            Rest => "Choose what it rests on, then what goes against the fence".to_string(),
            SwitchGuide if self.table_saw.using_gauge() => "Use the rip fence".to_string(),
            SwitchGuide => "Use the miter gauge".to_string(),
            Guide(s) if self.table_saw.using_gauge() => {
                step(s, "Gauge stop left", "Gauge stop right", "1/16\"", "1\"")
            }
            Guide(s) => step(s, "Fence left", "Fence right", "1/16\"", "1\""),
            Blade(s) => step(s, "Blade down", "Blade up", "1/16\"", "1/4\""),
            Tilt(s) => step(
                s,
                "Tilt the blade back toward square",
                "Tilt the blade further",
                "1°",
                "to the next stop",
            ),
            Push => "Hold to push the board through".to_string(),
            BringBack => "Bring the board back around".to_string(),
            Stop(s) => step(s, "Stop in", "Stop out", "1/16\"", "1\""),
            Miter(s) if self.view == ShopView::TableSaw => step(
                s,
                "Swing the gauge left",
                "Swing the gauge right",
                "1°",
                "to the next stop",
            ),
            Miter(s) => step(s, "Swing left", "Swing right", "1°", "to the next stop"),
            Depth(s) => step(s, "Depth stop down", "Depth stop up", "1/16\"", "1/4\""),
            Lower => "Hold to lower the blade".to_string(),
            Use(Fastener::Glue) => "Glue".to_string(),
            Use(Fastener::Brads) => "Brads".to_string(),
            Use(Fastener::Screws) => "Screws".to_string(),
            Slide(..) => "Slide it to where it lines up (Shift: 1/16\")".to_string(),
            Fasten => "Fasten it".to_string(),
            LetGo => "Let go".to_string(),
            TakeApart => "Take the last joint apart".to_string(),
            PickUp => "Pick it up".to_string(),
            Plans => "Plans to print".to_string(),
            Store if self.view == ShopView::Bench => "Put it away".to_string(),
            Store => "Put it all away".to_string(),
            CutAgain if self.view == ShopView::TableSaw => {
                "Put the strip away and rip the rest again".to_string()
            }
            CutAgain => "Put the piece away and cut another from the offcut".to_string(),
            PutAway => "Put it away".to_string(),
            Take(_) => self.use_here().to_string(),
            NewLumber(index) => {
                let name = LUMBER
                    .get(index as usize)
                    .map_or("board", |lumber| lumber.0);
                format!("New {name}: {}", self.use_here().to_lowercase())
            }
            MoveShelf(id) => match self.inventory.get(id).map(|item| item.shelf) {
                Some(Shelf::Offcuts) => "Move it to the parts".to_string(),
                _ => "Move it to the offcuts".to_string(),
            },
            Scrap(_) => "Throw it away".to_string(),
            Help => "Controls".to_string(),
            Inventory => "Inventory".to_string(),
        }
    }

    /// What a piece of wood from the inventory does where you are.
    fn use_here(&self) -> &'static str {
        match self.view {
            ShopView::TableSaw => "Put it on the table saw",
            ShopView::MiterSaw => "Put it on the miter saw",
            ShopView::Bench => "Set it on the bench",
            ShopView::Overview => "Pick it up",
        }
    }

    /// Whether `action` would do anything now.
    fn can(&self, action: Action) -> bool {
        use Action::*;
        let (table, miter, bench) = (self.table_saw, self.miter_saw, self.bench);
        match (self.view, action) {
            (view, Take(_) | NewLumber(_)) => inventory::can_use_at(view, table, miter),
            (_, PutAway) => self.hand.0.is_some(),
            (ShopView::TableSaw, Store) => table.can_put_away(),
            (ShopView::TableSaw, CutAgain) => table.can_cut_again(),
            (ShopView::MiterSaw, Store) => miter.can_put_away(),
            (ShopView::MiterSaw, CutAgain) => miter.can_cut_again(),
            (ShopView::Bench, Store) => bench.picked.is_some(),
            (ShopView::TableSaw, TurnLeft | TurnRight | Flip) => table.can_turn(),
            (ShopView::TableSaw, StandUp) => table.can_tip(),
            (ShopView::TableSaw, Rest) => table.can_turn(),
            (ShopView::TableSaw, Push) => table.can_push(),
            (ShopView::TableSaw, BringBack) => table.can_bring_back(),
            (ShopView::TableSaw, Miter(_)) => table.can_swing_gauge(),
            (ShopView::TableSaw, Tilt(_)) => table.can_tilt(),
            (ShopView::MiterSaw, TurnLeft | TurnRight | Flip) => miter.can_turn(),
            (ShopView::MiterSaw, StandUp) => miter.can_tip(),
            (ShopView::MiterSaw, Rest) => miter.can_turn(),
            (ShopView::MiterSaw, Lower) => miter.has_work(),
            (ShopView::Bench, TurnLeft | TurnRight | Flip) => {
                bench.lining_up.is_some() || bench.turnable().is_some()
            }
            (ShopView::Bench, Fasten | Slide(..)) => bench.lining_up.is_some(),
            (ShopView::Bench, LetGo) => bench.picked.is_some(),
            (ShopView::Bench, PickUp) => bench.picked.is_some() && self.hand.0.is_none(),
            (ShopView::Bench, TakeApart) => bench.can_take_apart(),
            (ShopView::Bench, Plans) => !bench.groups.is_empty(),
            _ => true,
        }
    }

    /// Whether `action`'s button is lit up, as a choice that's made: the station you're at,
    /// the fastener in use, an open panel.
    fn lit(&self, action: Action) -> bool {
        match action {
            Action::GoTo(view) => view == self.view,
            Action::Use(fastener) => self.bench.fastener == fastener,
            Action::SwitchGuide => self.table_saw.using_gauge(),
            Action::Rest => match self.view {
                ShopView::TableSaw => self.table_saw.choosing_rest(),
                ShopView::MiterSaw => self.miter_saw.choosing_rest(),
                _ => false,
            },
            Action::Help => self.showing.help,
            Action::Inventory => self.showing.inventory,
            _ => false,
        }
    }

    /// Whether `action`'s button shows at all: the arrows that slide a part across a joint
    /// only while one's being lined up.
    fn shown(&self, action: Action) -> bool {
        match action {
            Action::Slide(..) => self.bench.lining_up.is_some(),
            _ => true,
        }
    }

    /// A settings card's name and what it reads.
    fn setting(&self, setting: Setting) -> (String, String) {
        let (table, miter) = (self.table_saw, self.miter_saw);
        match setting {
            Setting::Guide if table.using_gauge() => {
                ("Gauge stop".to_string(), table.gauge_stop().to_string())
            }
            Setting::Guide => ("Rip fence".to_string(), table.saw.fence.to_string()),
            Setting::Blade => ("Blade height".to_string(), table.saw.height.to_string()),
            Setting::GaugeAngle => (
                "Gauge angle".to_string(),
                describe_miter(table.saw.gauge_angle),
            ),
            Setting::Tilt => (
                "Blade tilt".to_string(),
                match table.saw.tilt {
                    woodsim::Angle::ZERO => "square".to_string(),
                    tilt => tilt.to_string(),
                },
            ),
            Setting::Stop => ("Length".to_string(), miter.stop.to_string()),
            Setting::Miter => ("Miter".to_string(), describe_miter(miter.saw.miter)),
            Setting::Depth => (
                "Depth stop".to_string(),
                match miter.depth_stop {
                    Some(stop) => format!("{stop} up"),
                    None => "off".to_string(),
                },
            ),
            Setting::OnBench => ("On the bench".to_string(), bench::reading(self.bench)),
        }
    }

    /// The line on what's going on.
    fn status(&self, hovered: Option<ShopView>) -> String {
        let holding = self.hand.0.is_some();
        match self.view {
            ShopView::Overview => match (hovered, holding) {
                (Some(ShopView::TableSaw), false) => "Click to work at the table saw.",
                (Some(ShopView::MiterSaw), false) => "Click to work at the miter saw.",
                (Some(ShopView::Bench), false) => "Click to work at the workbench.",
                (Some(ShopView::TableSaw), true) => "Click to set it down on the table saw.",
                (Some(ShopView::MiterSaw), true) => "Click to set it down on the miter saw.",
                (Some(ShopView::Bench), true) => "Click to set it down at the workbench.",
                (_, true) => "Click a station or the scrap bin to set it down, or put it away.",
                _ => {
                    "Click a station to work at it. New lumber, your parts, and your offcuts \
                     are in the inventory."
                }
            }
            .to_string(),
            ShopView::TableSaw => self.table_saw.status(),
            ShopView::MiterSaw => self.miter_saw.status(),
            ShopView::Bench => self.bench.status(holding),
        }
    }
}

/// What the mouse does at `view`, for the list of controls.
fn mouse_help(view: ShopView) -> &'static [&'static str] {
    match view {
        ShopView::Overview => &[
            "Click a station: work at it",
            "Click a board: pick it up; new ones are on the lumber rack, too",
            "Holding one: click a station to set it down, or the scrap bin to throw it away",
            "Drag the floor: look around   Scroll: zoom",
        ],
        ShopView::TableSaw => &[
            "Drag the fence, or the gauge's stop block, to move it",
            "Drag the board forward to push it through, back to back it out",
            "Click a board clear of the blade: pick it up",
            "Holding one: click the saw to set it here",
        ],
        ShopView::MiterSaw => &[
            "Drag the stop block to move it",
            "Click a board: pick it up",
            "Holding one: click the saw to set it against the stop",
        ],
        ShopView::Bench => &[
            "Click a surface of a part, then the surface it goes against",
            "Drag: look around   Scroll: zoom",
        ],
    }
}

/// The keys that step a setting, and what they do.
fn setting_help(setting: Setting) -> Option<&'static str> {
    Some(match setting {
        Setting::Guide => {
            "Left/Right arrows: move the fence, or the gauge's stop, 1/16\" (Shift: 1\")"
        }
        Setting::Blade => "Up/Down arrows: raise or lower the blade 1/16\" (Shift: 1/4\")",
        Setting::Stop => "Left/Right arrows: move the stop 1/16\" (Shift: 1\")",
        Setting::Miter => "[ and ]: swing the miter 1° (Shift: to the next stop)",
        Setting::GaugeAngle => "[ and ]: swing the miter gauge 1° (Shift: to the next stop)",
        Setting::Tilt => ", and .: tilt the blade further or back 1° (Shift: to the next stop)",
        Setting::Depth => {
            "Up/Down arrows: raise or lower the depth stop 1/16\" (Shift: 1/4\"); below the \
             table, it's off"
        }
        Setting::OnBench => return None,
    })
}

/// The list of controls at `view`: every button's key, then what the mouse does.
fn help(shop: &Shop) -> String {
    let mut lines = vec![format!(
        "Controls: {}",
        shop.describe(Action::GoTo(shop.view))
    )];
    let mut actions: Vec<Action> = dock(shop.view)
        .into_iter()
        .flatten()
        .map(|tool| tool.action)
        .filter(|action| !matches!(action, Action::Slide(..)))
        .collect();
    actions.extend([
        Action::PutAway,
        Action::NextStation,
        Action::Back,
        Action::Inventory,
        Action::Help,
    ]);
    for action in actions {
        if let Some((key, _)) = key_for(shop.view, action) {
            lines.push(format!("{key}: {}", shop.describe(action)));
        }
    }
    lines.extend(
        cards(shop.view)
            .into_iter()
            .filter_map(|(setting, ..)| setting_help(setting))
            .map(str::to_string),
    );
    match shop.view {
        ShopView::Overview => lines.push("W/A/S/D or arrows: look around".to_string()),
        ShopView::Bench => {
            lines.push("Arrows: slide it across the joint (Shift: 1/16\")".to_string())
        }
        _ => {}
    }
    lines.push(String::new());
    lines.extend(mouse_help(shop.view).iter().map(|line| line.to_string()));
    lines.join("\n")
}

fn text(content: impl Into<String>, size: f32, color: Color) -> impl Bundle {
    weighted(content, size, color, FontWeight::NORMAL)
}

/// Text set bold, for what's read at a glance: a setting, a button's name or key.
fn bold(content: impl Into<String>, size: f32, color: Color) -> impl Bundle {
    weighted(content, size, color, FontWeight::BOLD)
}

fn weighted(
    content: impl Into<String>,
    size: f32,
    color: Color,
    weight: FontWeight,
) -> impl Bundle {
    (
        Text::new(content),
        TextFont {
            font_size: FontSize::Px(size),
            weight,
            ..default()
        },
        TextColor(color),
    )
}

/// A cream panel with rounded corners and a soft shadow.
fn panel(node: Node) -> impl Bundle {
    (
        Node {
            padding: UiRect::axes(px(14), px(10)),
            border: UiRect::all(px(2)),
            border_radius: BorderRadius::all(px(14)),
            ..node
        },
        BackgroundColor(PAPER),
        BorderColor::all(EDGE),
        BoxShadow::new(SHADOW, px(0), px(3), px(0), px(10)),
    )
}

/// A full-width or full-height strip of the screen that lays out what's in it but lets clicks
/// through to the shop.
fn strip(node: Node) -> impl Bundle {
    (
        Node {
            position_type: PositionType::Absolute,
            ..node
        },
        Pickable::IGNORE,
    )
}

fn spawn_hud(mut commands: Commands, icons: Res<Icons>) {
    // Along the top: the way back to the shop and to each station on the left; the frame
    // rate, the inventory, and the controls on the right.
    let top_left = commands
        .spawn(strip(Node {
            top: px(14),
            left: px(14),
            column_gap: px(8),
            ..default()
        }))
        .id();
    spawn_button(
        &mut commands,
        &icons,
        top_left,
        Action::GoTo(ShopView::Overview),
        Icon::Shop,
        Look::Round,
        TipAt::BelowLeft,
    );
    for (view, icon) in [
        (ShopView::TableSaw, Icon::TableSaw),
        (ShopView::MiterSaw, Icon::MiterSaw),
        (ShopView::Bench, Icon::Bench),
    ] {
        spawn_button(
            &mut commands,
            &icons,
            top_left,
            Action::GoTo(view),
            icon,
            Look::Tab,
            TipAt::BelowLeft,
        );
    }
    let top_right = commands
        .spawn(strip(Node {
            top: px(14),
            right: px(14),
            column_gap: px(8),
            align_items: AlignItems::Center,
            ..default()
        }))
        .id();
    commands.spawn((
        panel(Node {
            padding: UiRect::axes(px(10), px(5)),
            margin: UiRect::right(px(4)),
            ..default()
        }),
        Pickable::IGNORE,
        ChildOf(top_right),
        children![(text("", 13.0, INK.with_alpha(0.7)), Readout::Fps)],
    ));
    for (action, icon) in [
        (Action::Inventory, Icon::Inventory),
        (Action::Help, Icon::Help),
    ] {
        let button = spawn_button(
            &mut commands,
            &icons,
            top_right,
            action,
            icon,
            Look::Round,
            TipAt::BelowRight,
        );
        if action == Action::Inventory {
            commands.entity(button).insert(InventoryButton);
        }
    }
    // The inventory hangs below its button, its list scrolling if it's long, and the controls
    // go in the middle of the screen.
    commands.spawn((
        panel(Node {
            position_type: PositionType::Absolute,
            top: px(80),
            right: px(14),
            width: px(INVENTORY_WIDTH),
            max_height: vh(66),
            flex_direction: FlexDirection::Column,
            padding: UiRect::axes(px(6), px(8)),
            ..default()
        }),
        PanelOf(Readout::Inventory),
        children![(
            Node {
                flex_direction: FlexDirection::Column,
                overflow: Overflow::scroll_y(),
                padding: UiRect::horizontal(px(8)),
                ..default()
            },
            ScrollPosition::default(),
            InventoryList,
            ListArea,
        )],
    ));
    // The one tooltip, over everything.
    commands.spawn((
        Node {
            position_type: PositionType::Absolute,
            padding: UiRect::axes(px(10), px(6)),
            border_radius: BorderRadius::all(px(8)),
            display: Display::None,
            ..default()
        },
        UiTransform::default(),
        BackgroundColor(INK.with_alpha(0.94)),
        GlobalZIndex(20),
        Tip,
        Pickable::IGNORE,
        children![(
            text("", 14.0, Color::WHITE),
            TextLayout::no_wrap(),
            Pickable::IGNORE
        )],
    ));
    commands
        .spawn(strip(Node {
            width: percent(100),
            height: percent(100),
            justify_content: JustifyContent::Center,
            align_items: AlignItems::Center,
            ..default()
        }))
        .with_child((
            panel(Node {
                display: Display::None,
                max_width: vw(60),
                padding: UiRect::all(px(20)),
                ..default()
            }),
            GlobalZIndex(5),
            PanelOf(Readout::Help),
            children![(text("", 16.0, INK), Readout::Help)],
        ));
    // The station's settings down the left, under the tabs.
    commands.spawn((
        strip(Node {
            top: px(80),
            left: px(14),
            flex_direction: FlexDirection::Column,
            row_gap: px(10),
            ..default()
        }),
        Cards,
    ));
    // Along the bottom, what's in hand over the line on what's going on, over the dock.
    let mut hand = Entity::PLACEHOLDER;
    commands
        .spawn(strip(Node {
            bottom: px(14),
            width: percent(100),
            flex_direction: FlexDirection::Column,
            align_items: AlignItems::Center,
            row_gap: px(10),
            ..default()
        }))
        .with_children(|bottom| {
            hand = bottom
                .spawn((
                    panel(Node {
                        display: Display::None,
                        max_width: vw(40),
                        align_items: AlignItems::Center,
                        column_gap: px(10),
                        padding: UiRect {
                            right: px(8),
                            ..UiRect::axes(px(14), px(6))
                        },
                        ..default()
                    }),
                    PanelOf(Readout::Hand),
                    children![
                        (
                            ImageNode::default(),
                            Node {
                                width: px(40),
                                height: px(40),
                                ..default()
                            },
                            HandPicture,
                            Pickable::IGNORE,
                        ),
                        (bold("", 17.0, ACCENT), Readout::Hand)
                    ],
                ))
                .id();
            bottom.spawn((
                panel(Node {
                    max_width: vw(56),
                    ..default()
                }),
                PanelOf(Readout::Status),
                children![(text("", 16.0, INK), Readout::Status)],
            ));
            bottom.spawn((
                panel(Node {
                    padding: UiRect::all(px(8)),
                    column_gap: px(6),
                    align_items: AlignItems::Center,
                    border_radius: BorderRadius::all(px(40)),
                    ..default()
                }),
                Dock,
            ));
        });
    spawn_button(
        &mut commands,
        &icons,
        hand,
        Action::PutAway,
        Icon::Store,
        Look::Small,
        TipAt::Above,
    );
}

/// Puts a button for `action` in `parent`.
fn spawn_button(
    commands: &mut Commands,
    icons: &Icons,
    parent: Entity,
    action: Action,
    icon: Icon,
    look: Look,
    tip: TipAt,
) -> Entity {
    let (size, icon_size) = match look {
        Look::Round | Look::Go => (DOCK_BUTTON, 30.0),
        Look::Small => (STEP_BUTTON, 18.0),
        Look::Tab => (TAB_HEIGHT, 26.0),
    };
    let tab = look == Look::Tab;
    let button = commands
        .spawn((
            Button,
            Hovered::default(),
            Control { action, look, tip },
            Node {
                width: if tab { Val::Auto } else { px(size) },
                height: px(size),
                padding: UiRect::axes(px(if tab { 14.0 } else { 0.0 }), px(0)),
                column_gap: px(8),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                border: UiRect {
                    bottom: px(3),
                    ..UiRect::all(px(1.5))
                },
                border_radius: BorderRadius::all(px(size / 2.0)),
                ..default()
            },
            BackgroundColor(FACE),
            BorderColor::all(EDGE),
            BoxShadow::new(SHADOW, px(0), px(2), px(0), px(6)),
            ChildOf(parent),
        ))
        .id();
    commands.spawn((
        ImageNode {
            image: icons.get(icon),
            color: INK,
            ..default()
        },
        Node {
            width: px(icon_size),
            height: px(icon_size),
            ..default()
        },
        ControlIcon,
        Pickable::IGNORE,
        ChildOf(button),
    ));
    if tab {
        commands.spawn((
            bold("", 17.0, INK),
            ControlIcon,
            Pickable::IGNORE,
            ChildOf(button),
        ));
    }
    // The key's badge in the corner, for round buttons whose key has a short name.
    commands.spawn((
        Node {
            position_type: PositionType::Absolute,
            right: px(-6),
            bottom: px(-6),
            padding: UiRect::axes(px(5), px(1)),
            border_radius: BorderRadius::all(px(6)),
            display: Display::None,
            ..default()
        },
        BackgroundColor(INK),
        Badge,
        Pickable::IGNORE,
        ChildOf(button),
        children![(bold("", 11.0, Color::WHITE), Pickable::IGNORE)],
    ));
    button
}

/// The key's badge on a button.
#[derive(Component)]
struct Badge;

/// Where a button's tooltip shows: above it or below, centered on it or lined up with one side
/// where it's near the edge of the screen; or to its left, for the inventory down the right.
#[derive(Clone, Copy, PartialEq, Eq)]
enum TipAt {
    Above,
    Below,
    BelowLeft,
    BelowRight,
    Left,
}

/// A click on a button starts its action; Shift with an arrow that slides a part slides it a
/// 16th at a time.
fn activate(
    activate: On<Activate>,
    controls: Query<(&Control, &UiGlobalTransform, &ComputedNode)>,
    keys: Res<ButtonInput<KeyCode>>,
    mut actions: ResMut<Actions>,
    mut last_press: ResMut<LastPress>,
) {
    let Ok((control, at, size)) = controls.get(activate.entity) else {
        return;
    };
    last_press.0 = at.affine().translation * size.inverse_scale_factor;
    let shift = keys.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight]);
    let action = match control.action {
        Action::Slide(arrow, _) => Action::Slide(arrow, shift),
        action => action,
    };
    if !action.is_held() {
        actions.start(action);
    }
}

/// Holding a button down keeps an action that goes on while held going.
fn hold_pressed(controls: Query<(&Control, Has<Pressed>)>, mut actions: ResMut<Actions>) {
    for (control, pressed) in &controls {
        if pressed && control.action.is_held() {
            actions.hold(control.action);
        }
    }
}

/// Notes whether the mouse is over the HUD, so a drag that starts on a button doesn't turn the
/// view.
fn notice_hud(
    hovers: Res<HoverMap>,
    nodes: Query<(), With<Node>>,
    mouse: Res<ButtonInput<MouseButton>>,
    mut on_hud: ResMut<OnHud>,
) {
    let over = hovers
        .get(&PointerId::Mouse)
        .is_some_and(|hits| hits.keys().any(|&entity| nodes.contains(entity)));
    let pressed = if mouse.just_pressed(MouseButton::Left) {
        over
    } else {
        on_hud.pressed
    };
    if on_hud.over != over || on_hud.pressed != pressed {
        *on_hud = OnHud { over, pressed };
    }
}

/// Opens and closes the controls and the inventory.
fn toggle_panels(actions: Res<Actions>, mut showing: ResMut<Showing>) {
    if actions.just(Action::Help) {
        showing.help = !showing.help;
    }
    if actions.just(Action::Inventory) {
        showing.inventory = !showing.inventory;
    }
}

/// Fills the dock and the settings cards with what's at the view the camera's at.
fn build_for_view(
    mut commands: Commands,
    view: Res<ShopView>,
    icons: Res<Icons>,
    mut showing: ResMut<Showing>,
    dock_row: Single<Entity, With<Dock>>,
    cards_column: Single<Entity, With<Cards>>,
) {
    if showing.built_for == Some(*view) {
        return;
    }
    showing.built_for = Some(*view);
    let (dock_row, cards_column) = (*dock_row, *cards_column);
    commands.entity(dock_row).despawn_related::<Children>();
    commands.entity(cards_column).despawn_related::<Children>();
    for (index, group) in dock(*view).into_iter().enumerate() {
        if index > 0 {
            commands.spawn((
                Node {
                    width: px(2),
                    height: px(DOCK_BUTTON - 16.0),
                    margin: UiRect::horizontal(px(6)),
                    ..default()
                },
                BackgroundColor(EDGE),
                Divider,
                ChildOf(dock_row),
            ));
        }
        for tool in group {
            spawn_button(
                &mut commands,
                &icons,
                dock_row,
                tool.action,
                tool.icon,
                tool.look,
                TipAt::Above,
            );
        }
    }
    for (setting, icon, across) in cards(*view) {
        let card = commands
            .spawn((
                panel(Node {
                    align_items: AlignItems::Center,
                    column_gap: px(10),
                    padding: UiRect::axes(px(12), px(8)),
                    ..default()
                }),
                ChildOf(cards_column),
            ))
            .id();
        commands.spawn((
            ImageNode {
                image: icons.get(icon),
                color: ACCENT,
                ..default()
            },
            Node {
                width: px(30),
                height: px(30),
                ..default()
            },
            Pickable::IGNORE,
            ChildOf(card),
        ));
        let (less, more) = match across {
            Some(true) => (Arrow::Left, Arrow::Right),
            _ => (Arrow::Down, Arrow::Up),
        };
        let step_buttons = |commands: &mut Commands, more_way: bool| {
            let bigs: &[bool] = if more_way {
                &[false, true]
            } else {
                &[true, false]
            };
            for &big in bigs {
                let step = Step {
                    more: more_way,
                    big,
                };
                if let Some(action) = stepping(setting, step) {
                    let arrow = if more_way { more } else { less };
                    spawn_button(
                        commands,
                        &icons,
                        card,
                        action,
                        Icon::Step(arrow, big),
                        Look::Small,
                        TipAt::Below,
                    );
                }
            }
        };
        if across.is_some() {
            step_buttons(&mut commands, false);
        }
        commands.spawn((
            Node {
                flex_direction: FlexDirection::Column,
                // Wide enough for the longest reading, "22.5° right", so the buttons don't
                // shift as it changes.
                min_width: px(if across.is_some() { 150.0 } else { 0.0 }),
                align_items: AlignItems::Center,
                ..default()
            },
            ChildOf(card),
            children![
                (bold("", 13.0, INK.with_alpha(0.6)), SettingName(setting)),
                (bold("", 25.0, ACCENT), Readout::Setting(setting)),
            ],
        ));
        if across.is_some() {
            step_buttons(&mut commands, true);
        }
    }
}

/// A divider between groups in the dock.
#[derive(Component)]
struct Divider;

/// The name over a settings card's reading.
#[derive(Component)]
struct SettingName(Setting);

/// A button, with what `update_controls` keeps up to date on it.
type ButtonParts = (
    Entity,
    &'static Control,
    &'static Hovered,
    Has<Pressed>,
    Has<InteractionDisabled>,
    &'static mut Node,
    &'static mut BackgroundColor,
    &'static Children,
);

/// A tab's name, and its color.
type LabelParts = (&'static mut Text, &'static mut TextColor);
type LabelFilter = (With<ControlIcon>, Without<Readout>);
/// A button's key's badge.
type BadgeFilter = (With<Badge>, Without<Control>);

/// Keeps every button looking like what it does now: lit, dimmed, hidden, pressed, hovered,
/// with its key and what it does.
fn update_controls(
    mut commands: Commands,
    game: Game,
    mut buttons: Query<ButtonParts>,
    mut icons: Query<&mut ImageNode, With<ControlIcon>>,
    mut labels: Query<LabelParts, LabelFilter>,
    mut badges: Query<(&mut Node, &Children), BadgeFilter>,
    mut words: Query<&mut Text, (Without<ControlIcon>, Without<Readout>)>,
) {
    let shop = game.shop();
    let view = shop.view;
    for (entity, control, hovered, pressed, disabled, mut node, mut face, children) in &mut buttons
    {
        let action = control.action;
        let shown = shop.shown(action);
        let display = if shown { Display::Flex } else { Display::None };
        if node.display != display {
            node.display = display;
        }
        let can = shop.can(action);
        if can == disabled {
            if can {
                commands.entity(entity).remove::<InteractionDisabled>();
            } else {
                commands.entity(entity).insert(InteractionDisabled);
            }
        }
        let lit = shop.lit(action) || (pressed && can);
        let hover = hovered.get() && can;
        let base = match (lit, control.look) {
            (true, _) => ACCENT,
            (false, Look::Go) => GO,
            (false, _) => FACE,
        };
        let color = if !can {
            base.with_alpha(0.55)
        } else if hover && base == FACE {
            FACE_HOVER
        } else if hover {
            base.lighter(0.06)
        } else {
            base
        };
        if face.0 != color {
            face.0 = color;
        }
        let ink = match (lit || control.look == Look::Go, can) {
            (true, true) => Color::WHITE,
            (false, true) => INK,
            (_, false) => INK.with_alpha(0.3),
        };
        let key = key_for(view, action);
        for child in children.iter() {
            if let Ok(mut image) = icons.get_mut(child)
                && image.color != ink
            {
                image.color = ink;
            }
            if let Ok((mut label, mut color)) = labels.get_mut(child) {
                let name = shop.describe(action);
                if label.0 != name {
                    label.0 = name;
                }
                if color.0 != ink {
                    color.0 = ink;
                }
            }
            // The key's badge, for round buttons whose key has a short name.
            if let Ok((mut badge, words_of)) = badges.get_mut(child) {
                let short = key
                    .as_ref()
                    .and_then(|(_, badge)| *badge)
                    .filter(|_| matches!(control.look, Look::Round | Look::Go));
                let display = if short.is_some() {
                    Display::Flex
                } else {
                    Display::None
                };
                if badge.display != display {
                    badge.display = display;
                }
                if let (Some(short), Some(&line)) = (short, words_of.first())
                    && let Ok(mut line) = words.get_mut(line)
                    && line.0 != short
                {
                    line.0 = short.to_string();
                }
            }
        }
    }
}

/// Shows what the button under the pointer does, and its key, beside it.
fn show_tip(
    game: Game,
    buttons: Query<(&Control, &Hovered, &UiGlobalTransform, &ComputedNode)>,
    tip: Single<(&mut Node, &mut UiTransform, &Children), With<Tip>>,
    mut words: Query<&mut Text>,
) {
    let (mut node, mut transform, children) = tip.into_inner();
    let hovered = buttons
        .iter()
        .find(|(control, hovered, ..)| hovered.get() && control.look != Look::Tab);
    let Some((control, _, at, size)) = hovered else {
        if node.display != Display::None {
            node.display = Display::None;
        }
        return;
    };
    let shop = game.shop();
    let said = match key_for(shop.view, control.action) {
        Some((key, _)) => format!("{} ({key})", shop.describe(control.action)),
        None => shop.describe(control.action),
    };
    if let Some(&line) = children.first()
        && let Ok(mut line) = words.get_mut(line)
        && line.0 != said
    {
        line.0 = said;
    }
    // The button's edges, in logical pixels.
    let scale = size.inverse_scale_factor;
    let middle = at.affine().translation * scale;
    let half = size.size * scale / 2.0;
    const GAP: f32 = 10.0;
    let (x, y, shift) = match control.tip {
        TipAt::Above => (middle.x, middle.y - half.y - GAP, (-50.0, -100.0)),
        TipAt::Below => (middle.x, middle.y + half.y + GAP, (-50.0, 0.0)),
        TipAt::BelowLeft => (middle.x - half.x, middle.y + half.y + GAP, (0.0, 0.0)),
        TipAt::BelowRight => (middle.x + half.x, middle.y + half.y + GAP, (-100.0, 0.0)),
        TipAt::Left => (middle.x - half.x - GAP, middle.y, (-100.0, -50.0)),
    };
    let placed = Node {
        position_type: PositionType::Absolute,
        left: px(x),
        top: px(y),
        display: Display::Flex,
        ..node.clone()
    };
    if *node != placed {
        *node = placed;
    }
    let shifted = UiTransform::from_translation(Val2::percent(shift.0, shift.1));
    if *transform != shifted {
        *transform = shifted;
    }
}

/// The scrolling list in the inventory panel.
#[derive(Component)]
struct InventoryList;

/// How wide the inventory panel is.
const INVENTORY_WIDTH: f32 = 400.0;

/// Fills the inventory's list afresh whenever what's in it, or where you are, changes: the
/// parts, the offcuts, and new lumber, each line with buttons to use it here, move it to the
/// other shelf, or throw it away.
fn build_inventory(
    mut commands: Commands,
    view: Res<ShopView>,
    inventory: Res<Inventory>,
    icons: Res<Icons>,
    list: Single<Entity, With<InventoryList>>,
) {
    if !(inventory.is_changed() || view.is_changed()) {
        return;
    }
    let list = *list;
    commands.entity(list).despawn_related::<Children>();
    let use_icon = match *view {
        ShopView::Overview => Icon::PickUp,
        _ => Icon::Place,
    };
    for shelf in [Shelf::Parts, Shelf::Offcuts] {
        let (name, empty, move_icon) = match shelf {
            Shelf::Parts => (
                "Parts",
                "Pieces you cut to size, glue-ups, and boards you put away go here.",
                Icon::Step(Arrow::Down, false),
            ),
            Shelf::Offcuts => (
                "Offcuts",
                "What's left over from a cut goes here.",
                Icon::Step(Arrow::Up, false),
            ),
        };
        let count = inventory.count(shelf);
        heading(&mut commands, list, &format!("{name} · {count}"));
        let rows = inventory.rows(shelf);
        if rows.is_empty() {
            commands.spawn((
                text(empty, 14.0, INK.with_alpha(0.55)),
                Node {
                    margin: UiRect::bottom(px(6)),
                    ..default()
                },
                ChildOf(list),
            ));
        }
        for row in rows {
            let (size, notes) = inventory::describe(row.work);
            let line = line(&mut commands, list, &size, &notes, row.count);
            commands.entity(line).insert(RowOf(row.ids.clone()));
            for (action, icon) in [
                (Action::Take(row.id), use_icon),
                (Action::MoveShelf(row.id), move_icon),
                (Action::Scrap(row.id), Icon::Scrap),
            ] {
                spawn_button(
                    &mut commands,
                    &icons,
                    line,
                    action,
                    icon,
                    Look::Small,
                    TipAt::Left,
                );
            }
        }
    }
    heading(&mut commands, list, "New lumber");
    for (index, (name, thickness, width)) in LUMBER.iter().enumerate() {
        let size = format!("{thickness} x {width} x 24\"");
        let line = line(&mut commands, list, name, &size, 1);
        spawn_button(
            &mut commands,
            &icons,
            line,
            Action::NewLumber(index as u64),
            use_icon,
            Look::Small,
            TipAt::Left,
        );
    }
}

/// A heading in the inventory's list.
fn heading(commands: &mut Commands, list: Entity, name: &str) {
    commands.spawn((
        bold(name, 15.0, INK.with_alpha(0.65)),
        Node {
            margin: UiRect {
                top: px(10),
                bottom: px(4),
                ..default()
            },
            ..default()
        },
        ChildOf(list),
    ));
}

/// A line of the inventory: what it is, in bold, what else there is to say about it, and how
/// many there are, with room after for its buttons. Returns the line, to put them in.
fn line(commands: &mut Commands, list: Entity, name: &str, notes: &str, count: usize) -> Entity {
    let line = commands
        .spawn((
            Node {
                align_items: AlignItems::Center,
                column_gap: px(6),
                padding: UiRect::vertical(px(5)),
                border: UiRect::top(px(1)),
                ..default()
            },
            BorderColor::all(EDGE.with_alpha(0.12)),
            ChildOf(list),
        ))
        .id();
    let words = commands
        .spawn((
            Node {
                flex_direction: FlexDirection::Column,
                flex_grow: 1.0,
                flex_shrink: 1.0,
                ..default()
            },
            ChildOf(line),
            children![(bold(name, 15.0, INK), Pickable::IGNORE)],
        ))
        .id();
    if !notes.is_empty() {
        commands.spawn((
            text(notes, 13.0, INK.with_alpha(0.6)),
            Pickable::IGNORE,
            ChildOf(words),
        ));
    }
    if count > 1 {
        commands.spawn((
            Node {
                padding: UiRect::axes(px(7), px(1)),
                border_radius: BorderRadius::all(px(9)),
                ..default()
            },
            BackgroundColor(ACCENT),
            ChildOf(line),
            children![(
                bold(format!("×{count}"), 13.0, Color::WHITE),
                Pickable::IGNORE
            )],
        ));
    }
    line
}

/// Scrolls the inventory's list under the mouse wheel.
fn scroll_inventory(
    mut wheels: MessageReader<MouseWheel>,
    hovers: Res<HoverMap>,
    parents: Query<&ChildOf>,
    list: Single<(Entity, &mut ScrollPosition, &ComputedNode), With<InventoryList>>,
) {
    let lines: f32 = wheels
        .read()
        .map(|wheel| match wheel.unit {
            MouseScrollUnit::Line => wheel.y * 28.0,
            MouseScrollUnit::Pixel => wheel.y,
        })
        .sum();
    if lines == 0.0 {
        return;
    }
    let (list, mut position, node) = list.into_inner();
    let over = hovers.get(&PointerId::Mouse).is_some_and(|hits| {
        hits.keys()
            .any(|&hit| hit == list || parents.iter_ancestors(hit).any(|ancestor| ancestor == list))
    });
    if !over {
        return;
    }
    let most = ((node.content_size().y - node.size().y) * node.inverse_scale_factor).max(0.0);
    position.0.y = (position.0.y - lines).clamp(0.0, most);
}

/// Keeps the readouts up to date: the line on what's going on, what's in hand, the cut list,
/// the settings, and the controls; and hides the panels with nothing to show.
fn update_text(
    game: Game,
    hovered: Res<HoveredStation>,
    mut readouts: Query<(&Readout, &mut Text), Without<SettingName>>,
    mut names: Query<(&SettingName, &mut Text), Without<Readout>>,
    mut panels: Query<(&PanelOf, &mut Node)>,
) {
    if !(game.is_changed() || hovered.is_changed()) {
        return;
    }
    let shop = game.shop();
    let (hand, showing) = (&game.hand, &game.showing);
    for (readout, mut text) in &mut readouts {
        let said = match readout {
            Readout::Status => shop.status(hovered.station),
            Readout::Hand => hand::describe(hand),
            Readout::Inventory => continue,
            Readout::Help => help(&shop),
            Readout::Setting(setting) => shop.setting(*setting).1,
            Readout::Fps => continue,
        };
        if text.0 != said {
            text.0 = said;
        }
    }
    for (name, mut text) in &mut names {
        let said = shop.setting(name.0).0;
        if text.0 != said {
            text.0 = said;
        }
    }
    for (panel, mut node) in &mut panels {
        let shown = match panel.0 {
            Readout::Hand => hand.0.is_some(),
            Readout::Inventory => showing.inventory,
            Readout::Help => showing.help,
            _ => true,
        };
        let display = if shown { Display::Flex } else { Display::None };
        if node.display != display {
            node.display = display;
        }
    }
}

/// Shows the frame rate, the average frame time, and the slowest frame of the last couple of
/// seconds, which is where hitches show up.
fn update_fps(diagnostics: Res<DiagnosticsStore>, mut lines: Query<(&Readout, &mut Text)>) {
    let fps = diagnostics
        .get(&FrameTimeDiagnosticsPlugin::FPS)
        .and_then(|fps| fps.smoothed());
    let frame_time = diagnostics.get(&FrameTimeDiagnosticsPlugin::FRAME_TIME);
    let average = frame_time.and_then(|time| time.smoothed());
    let worst = frame_time.and_then(|time| time.values().copied().reduce(f64::max));
    let (Some(fps), Some(average), Some(worst)) = (fps, average, worst) else {
        return;
    };
    for (line, mut text) in &mut lines {
        if let Readout::Fps = line {
            text.0 = format!("{fps:.0} fps   {average:.1} ms   worst {worst:.1} ms");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shop<'a>(
        view: ShopView,
        table_saw: &'a TableSawStation,
        miter_saw: &'a MiterSawStation,
        bench: &'a BenchStation,
        hand: &'a Hand,
        showing: &'a Showing,
    ) -> Shop<'a> {
        static INVENTORY: std::sync::LazyLock<Inventory> =
            std::sync::LazyLock::new(Inventory::default);
        Shop {
            view,
            table_saw,
            miter_saw,
            bench,
            hand,
            showing,
            inventory: &INVENTORY,
        }
    }

    #[test]
    fn every_button_says_what_it_does_and_names_its_key() {
        let (table_saw, miter_saw, bench, hand, showing) = (
            TableSawStation::default(),
            MiterSawStation::default(),
            BenchStation::default(),
            Hand::default(),
            Showing::default(),
        );
        for view in [
            ShopView::Overview,
            ShopView::TableSaw,
            ShopView::MiterSaw,
            ShopView::Bench,
        ] {
            let shop = shop(view, &table_saw, &miter_saw, &bench, &hand, &showing);
            for tool in dock(view).into_iter().flatten() {
                assert!(!shop.describe(tool.action).is_empty());
                assert!(
                    key_for(view, tool.action).is_some(),
                    "{:?} has no key at {view:?}",
                    tool.action
                );
            }
            let help = help(&shop);
            assert!(help.contains("Tab: Next station"), "{help}");
        }
    }

    #[test]
    fn buttons_dim_when_they_would_do_nothing() {
        let (table_saw, miter_saw, bench, hand, showing) = (
            TableSawStation::default(),
            MiterSawStation::default(),
            BenchStation::default(),
            Hand::default(),
            Showing::default(),
        );
        let at_bench = shop(
            ShopView::Bench,
            &table_saw,
            &miter_saw,
            &bench,
            &hand,
            &showing,
        );
        assert!(!at_bench.can(Action::Fasten));
        assert!(!at_bench.can(Action::Plans));
        assert!(!at_bench.shown(Action::Slide(Arrow::Left, false)));
        assert!(at_bench.lit(Action::Use(Fastener::Glue)));
        assert!(at_bench.lit(Action::GoTo(ShopView::Bench)));
        let at_miter_saw = shop(
            ShopView::MiterSaw,
            &table_saw,
            &miter_saw,
            &bench,
            &hand,
            &showing,
        );
        assert!(!at_miter_saw.can(Action::Lower));
        assert!(!at_miter_saw.can(Action::TurnLeft));
    }

    #[test]
    fn settings_cards_read_what_the_saws_are_set_to() {
        let (table_saw, miter_saw, bench, hand, showing) = (
            TableSawStation::default(),
            MiterSawStation::default(),
            BenchStation::default(),
            Hand::default(),
            Showing::default(),
        );
        let shop = shop(
            ShopView::MiterSaw,
            &table_saw,
            &miter_saw,
            &bench,
            &hand,
            &showing,
        );
        assert_eq!(
            shop.setting(Setting::Miter),
            ("Miter".to_string(), "square".to_string())
        );
        assert_eq!(shop.setting(Setting::Depth).1, "off");
        assert_eq!(
            shop.describe(Action::Miter(Step {
                more: true,
                big: true
            })),
            "Swing right to the next stop"
        );
    }
}
