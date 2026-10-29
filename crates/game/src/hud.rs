//! Everything drawn over the shop. Along the top, a button for the inventory, a button back to
//! the shop and a tab for each station, and buttons for copying and the controls. Down the
//! left, the inventory as a tree of every piece of wood in the shop, wherever it is (`parts`),
//! with a card under it for the line picked out and what can be done with it. At a station, its
//! settings down the right, each a card with buttons to step it, and along the bottom a dock of
//! buttons for what can be done there, over a line on what's going on. What's in hand shows at
//! the bottom. Buttons start the same `Action`s their keys do, show their key, say what they do
//! when the pointer rests on them, and dim when they'd do nothing.

use bevy::{
    diagnostic::{DiagnosticsStore, FrameTimeDiagnosticsPlugin},
    ecs::{relationship::RelatedSpawner, spawn::SpawnWith, system::SystemParam},
    input::mouse::{MouseScrollUnit, MouseWheel},
    input_focus::AutoFocus,
    picking::{
        PickingSystems,
        hover::{HoverMap, Hovered},
        pointer::PointerId,
    },
    prelude::*,
    text::{EditableText, TextCursorStyle, TextEdit},
    time::common_conditions::on_real_timer,
    ui::{InteractionDisabled, Pressed},
    ui_widgets::{
        Activate, Button, ControlOrientation, Scrollbar, ScrollbarThumb, SelectAllOnFocus,
        TextInput,
    },
};
use plans::{LUMBER, describe_miter, plan};
use std::time::Duration;
use woodsim::Fastener;

use crate::{
    bench::{self, BenchView},
    chop::MiterSawStation,
    controls::{Action, Actions, Arrow, Step, key_for},
    copy::{Notice, to_copy},
    dress::JointerStation,
    edit::{self, Editor, Knob},
    feed::TableSawStation,
    flights::{HandPicture, InventoryButton, LastPress, ListArea, RowOf},
    hand::{self, Hand, Workpiece},
    icons::{Icon, Icons, picture},
    inventory::{self, Inventory, Shelf},
    join::BenchStation,
    parts::{self, DetailLine, Entry, Heading, Kind, Line, Places, Tree, Where, WoodLine},
    saved::Saved,
    shoot::Studio,
    shop::{HoveredStation, SawView, ShopView},
    thickness::PlanerStation,
    upload::{self, Sharing, TitleBox},
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
            .init_resource::<Viewfinder>()
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
                    show_naming_card,
                    build_tree,
                    scroll_inventory,
                    fit_inventory,
                    update_controls,
                    update_text,
                    show_tip,
                    measure_viewfinder,
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
    /// A line of the inventory's tree, lit while it's picked out.
    Row,
    /// A small flat chevron in a line of the tree, to open or fold it.
    Chevron,
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
    /// At the jointer: how deep it cuts, where its fence is, and how far it's tilted.
    Cut,
    JointerFence,
    FenceTilt,
    /// At the planer: how thick it leaves the wood.
    Thickness,
    OnBench,
    /// In the studio: the picked light's place around the piece, its height, and its power;
    /// the camera's height; and the lens.
    LightAround,
    LightHeight,
    LightPower,
    CameraHeight,
    Lens,
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
    let look = vec![tool(ResetView, Icon::View)];
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
            look.clone(),
        ],
        ShopView::MiterSaw => vec![
            [turning.clone(), vec![stand_up, rest]].concat(),
            vec![tool(Lower, Icon::Lower)],
            vec![tool(CutAgain, Icon::Again), tool(Store, Icon::Store)],
            look.clone(),
        ],
        ShopView::Jointer | ShopView::Planer => vec![
            [turning.clone(), vec![stand_up, rest]].concat(),
            vec![tool(Push, Icon::Push), tool(BringBack, Icon::BringBack)],
            vec![tool(Store, Icon::Store)],
            look.clone(),
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
                tool(Loosen, Icon::Unglue),
                tool(History, Icon::Pencil),
            ],
            vec![
                tool(Plans, Icon::Plans),
                tool(Share, Icon::Share),
                tool(Studio, Icon::Camera),
            ],
            look,
        ],
        ShopView::Studio => vec![
            vec![
                tool(TurnLeft, Icon::TurnLeft),
                tool(TurnRight, Icon::TurnRight),
                tool(Flip, Icon::Flip),
            ],
            vec![tool(ZoomIn, Icon::ZoomIn), tool(ZoomOut, Icon::ZoomOut)],
            vec![
                tool(Light(0), Icon::Key),
                tool(Light(1), Icon::Fill),
                tool(Light(2), Icon::Rim),
            ],
            vec![
                tool(
                    Backdrop(Step {
                        more: true,
                        big: false,
                    }),
                    Icon::Backdrop,
                ),
                tool(
                    Quality(Step {
                        more: true,
                        big: false,
                    }),
                    Icon::Quality,
                ),
                tool(DepthOfField, Icon::Focus),
            ],
            vec![
                tool(Engine, Icon::Chip),
                Tool {
                    look: Look::Go,
                    ..tool(Render, Icon::Shutter)
                },
                tool(StopRender, Icon::LetGo),
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
        ShopView::Jointer => vec![
            (Setting::Cut, Icon::Depth, Some(false)),
            (Setting::JointerFence, Icon::Fence, Some(true)),
            (Setting::FenceTilt, Icon::Tilt, Some(true)),
        ],
        ShopView::Planer => vec![(Setting::Thickness, Icon::Depth, Some(false))],
        ShopView::Bench => vec![(Setting::OnBench, Icon::Bench, None)],
        ShopView::Studio => vec![
            (Setting::LightAround, Icon::Around, Some(true)),
            (Setting::LightHeight, Icon::Raise, Some(false)),
            (Setting::LightPower, Icon::Bright, Some(true)),
            (Setting::CameraHeight, Icon::Camera, Some(false)),
            (Setting::Lens, Icon::Camera, Some(true)),
        ],
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
        Setting::Cut => Action::Depth(step),
        Setting::JointerFence => Action::Guide(step),
        // The fence's top leans out over the tables, to the left, as it tilts.
        Setting::FenceTilt => Action::Tilt(Step {
            more: !step.more,
            ..step
        }),
        Setting::Thickness => Action::Blade(step),
        Setting::OnBench => return None,
        Setting::LightAround => Action::Swing(step),
        Setting::LightHeight => Action::Raise(step),
        Setting::LightPower => Action::Brighten(step),
        Setting::CameraHeight => Action::Tip(step),
        Setting::Lens => Action::Lens(step),
    })
}

/// What the HUD shows: where the camera is, the stations, what's in hand, and which panels are
/// open.
#[derive(SystemParam)]
struct Game<'w> {
    view: Res<'w, ShopView>,
    table_saw: Res<'w, TableSawStation>,
    miter_saw: Res<'w, MiterSawStation>,
    jointer: Res<'w, JointerStation>,
    planer: Res<'w, PlanerStation>,
    bench: Res<'w, BenchStation>,
    hand: Res<'w, Hand>,
    showing: Res<'w, Showing>,
    inventory: Res<'w, Inventory>,
    saved: Res<'w, Saved>,
    tree: Res<'w, Tree>,
    editor: Res<'w, Editor>,
    notice: Res<'w, Notice>,
    studio: Res<'w, Studio>,
    sharing: Res<'w, Sharing>,
    views: (Res<'w, SawView>, Res<'w, BenchView>),
}

impl Game<'_> {
    fn shop(&self) -> Shop<'_> {
        Shop {
            view: *self.view,
            table_saw: &self.table_saw,
            miter_saw: &self.miter_saw,
            jointer: &self.jointer,
            planer: &self.planer,
            bench: &self.bench,
            hand: &self.hand,
            showing: &self.showing,
            inventory: &self.inventory,
            saved: &self.saved,
            tree: &self.tree,
            editor: &self.editor,
            notice: self.notice.0.as_ref().map(|(notice, _)| notice.as_str()),
            studio: &self.studio,
            sharing: &self.sharing,
            saw_view: &self.views.0,
            bench_view: &self.views.1,
        }
    }

    fn is_changed(&self) -> bool {
        self.notice.is_changed()
            || self.view.is_changed()
            || self.table_saw.is_changed()
            || self.miter_saw.is_changed()
            || self.jointer.is_changed()
            || self.planer.is_changed()
            || self.bench.is_changed()
            || self.hand.is_changed()
            || self.showing.is_changed()
            || self.inventory.is_changed()
            || self.saved.is_changed()
            || self.tree.is_changed()
            || self.editor.is_changed()
            || self.studio.is_changed()
            || self.sharing.is_changed()
            || self.views.0.is_changed()
            || self.views.1.is_changed()
    }
}

/// What's needed to say what a button does and whether it would do anything.
struct Shop<'a> {
    view: ShopView,
    table_saw: &'a TableSawStation,
    miter_saw: &'a MiterSawStation,
    jointer: &'a JointerStation,
    planer: &'a PlanerStation,
    bench: &'a BenchStation,
    hand: &'a Hand,
    showing: &'a Showing,
    inventory: &'a Inventory,
    saved: &'a Saved,
    tree: &'a Tree,
    editor: &'a Editor,
    /// A line in place of the status for now, like where a copy went.
    notice: Option<&'a str>,
    studio: &'a Studio,
    sharing: &'a Sharing,
    /// How the view's been turned at the saw you're at, or the bench.
    saw_view: &'a SawView,
    bench_view: &'a BenchView,
}

impl Shop<'_> {
    /// Everywhere wood is.
    fn places(&self) -> Places<'_> {
        Places {
            hand: self.hand,
            table_saw: self.table_saw,
            miter_saw: self.miter_saw,
            jointer: self.jointer,
            planer: self.planer,
            bench: self.bench,
            inventory: self.inventory,
            saved: self.saved,
        }
    }

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
            GoTo(ShopView::Jointer) => "Jointer".to_string(),
            GoTo(ShopView::Planer) => "Planer".to_string(),
            GoTo(ShopView::Bench) => "Workbench".to_string(),
            GoTo(ShopView::Studio) => "Photo studio".to_string(),
            NextStation => "Next station".to_string(),
            Pan(_) => "Look around".to_string(),
            RotateLeft => "Turn the view left".to_string(),
            RotateRight => "Turn the view right".to_string(),
            ZoomIn => "Zoom in".to_string(),
            ZoomOut => "Zoom out".to_string(),
            ResetView if self.view == ShopView::Bench => {
                "Look from in front of the bench again".to_string()
            }
            ResetView => "Look from where you stand again".to_string(),
            TurnLeft if self.view == ShopView::Studio => "Turn the piece left".to_string(),
            TurnRight if self.view == ShopView::Studio => "Turn the piece right".to_string(),
            Flip if self.view == ShopView::Studio => "Flip the piece over".to_string(),
            TurnLeft if self.view == ShopView::Bench => "Turn the picked part left".to_string(),
            TurnRight if self.view == ShopView::Bench => "Turn the picked part right".to_string(),
            Flip if self.view == ShopView::Bench => "Flip the picked part over".to_string(),
            TurnLeft => "Turn the board left".to_string(),
            TurnRight => "Turn the board right".to_string(),
            Flip => "Flip the board over".to_string(),
            StandUp => "Stand it on an edge, then on end, then lay it down".to_string(),
            Rest if self.view == ShopView::Jointer => {
                "Choose what goes against the fence; the knives take off what it rests on"
                    .to_string()
            }
            Rest if self.view == ShopView::Planer => {
                "Choose what it rests on; the knives plane the top parallel to it".to_string()
            }
            Rest => "Choose what it rests on, then what goes against the fence".to_string(),
            SwitchGuide if self.table_saw.using_gauge() => "Use the rip fence".to_string(),
            SwitchGuide => "Use the miter gauge".to_string(),
            Guide(s) if self.table_saw.using_gauge() => {
                step(s, "Gauge stop left", "Gauge stop right", "1/16\"", "1\"")
            }
            Guide(s) if self.view == ShopView::Jointer => step(
                s,
                "Fence in, using less of the knives",
                "Fence back, using more of them",
                "1/16\"",
                "1\"",
            ),
            Guide(s) => step(s, "Fence left", "Fence right", "1/16\"", "1\""),
            Blade(s) if self.view == ShopView::Planer => step(
                s,
                "Lower the head, planing thinner",
                "Raise the head, planing thicker",
                "1/64\"",
                "1/16\"",
            ),
            Blade(s) => step(s, "Blade down", "Blade up", "1/16\"", "1/4\""),
            Tilt(s) if self.view == ShopView::Jointer => step(
                s,
                "Tilt the fence back toward square",
                "Tilt the fence further",
                "1°",
                "to the next stop",
            ),
            Tilt(s) => step(
                s,
                "Tilt the blade back toward square",
                "Tilt the blade further",
                "1°",
                "to the next stop",
            ),
            Push if self.view == ShopView::Jointer => {
                "Hold to feed the board across the cutterhead".to_string()
            }
            Push if self.view == ShopView::Planer => {
                "Hold to feed it in; the rollers take it from there".to_string()
            }
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
            Depth(s) if self.view == ShopView::Jointer => {
                step(s, "Shallower cut", "Deeper cut", "1/64\"", "1/16\"")
            }
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
            Share => "Put plans up on the marketplace, to share at a link".to_string(),
            PutUp => "Put them up".to_string(),
            StopSharing => "Don't put them up".to_string(),
            Studio => "Render it in the studio".to_string(),
            Light(index) => format!(
                "Move the {}",
                crate::shoot::Studio::light_name(index).to_lowercase()
            ),
            Swing(s) => step(
                s,
                "Swing the light left",
                "Swing the light right",
                "5°",
                "45°",
            ),
            Raise(s) => step(s, "Lower the light", "Raise the light", "5°", "15°"),
            Brighten(s) => step(s, "Dim the light", "Brighten the light", "10%", "50%"),
            Tip(s) => step(s, "Camera lower", "Camera higher", "1°", "5°"),
            Lens(s) => step(s, "Shorter lens", "Longer lens", "5 mm", "to the next lens"),
            Backdrop(s) => {
                let setup = &self.studio.setup;
                format!(
                    "Backdrop: {}. Change it to {}",
                    setup.backdrop.name().to_lowercase(),
                    setup.backdrop.next(s.more).name().to_lowercase()
                )
            }
            Quality(s) => {
                let setup = &self.studio.setup;
                format!(
                    "Render quality: {}. Change it to {}",
                    setup.quality.name().to_lowercase(),
                    setup.quality.next(s.more).name().to_lowercase()
                )
            }
            Render if self.studio.rendering() => "Finish now, and save it as it is".to_string(),
            Render => "Render it".to_string(),
            Engine => match self.studio.setup.engine {
                crate::shoot::Engine::Gpu => {
                    "Rendering on the GPU. Render on the CPU instead".to_string()
                }
                crate::shoot::Engine::Cpu => {
                    "Rendering on the CPU. Render on the GPU instead".to_string()
                }
            },
            DepthOfField if self.studio.setup.depth_of_field => {
                "Depth of field: what's far behind the piece is soft. Make everything sharp"
                    .to_string()
            }
            DepthOfField => {
                "No depth of field: everything's sharp. Soften what's far behind the piece"
                    .to_string()
            }
            StopRender if self.studio.rendering() => "Stop rendering".to_string(),
            StopRender => "Back to setting up".to_string(),
            Store if self.view == ShopView::Bench => "Put it away".to_string(),
            Store => "Put it all away".to_string(),
            CutAgain if self.view == ShopView::TableSaw => {
                "Put the strip away and rip the rest again".to_string()
            }
            CutAgain => "Put the piece away and cut another from the offcut".to_string(),
            PutAway => "Put it away".to_string(),
            Copy => "Copy what's in hand, or what's here, for Claude".to_string(),
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
            Save(_) => "Save it as a file, to use after a restart".to_string(),
            Load(_) => format!("Saved part: {}", self.use_here().to_lowercase()),
            DeleteSaved(_) => "Delete its file".to_string(),
            Select(_) => "Pick it out".to_string(),
            Fold(fold) => match (fold, self.tree.is_open(fold)) {
                (_, true) => "Fold it away".to_string(),
                (parts::Fold::Wood(entry), false) => match self.places().wood(entry) {
                    Some(Workpiece::GlueUp(_)) => "List its boards and joints".to_string(),
                    _ => "List the cuts that made it".to_string(),
                },
                (_, false) => "Open it".to_string(),
            },
            Edit(_) => "Change this cut".to_string(),
            Tweak(knob, s) => {
                let (less, more, small, big) = match knob {
                    Knob::Fence => ("Fence in", "Fence out", "1/16\"", "1\""),
                    Knob::GaugeStop => ("Stop in", "Stop out", "1/16\"", "1\""),
                    Knob::GaugeAngle => ("Swing left", "Swing right", "1°", "to the next stop"),
                    Knob::Height => ("Blade down", "Blade up", "1/16\"", "1/4\""),
                    Knob::Tilt => ("Tilt back", "Tilt further", "1°", "to the next stop"),
                    Knob::Stop => ("Shorter", "Longer", "1/16\"", "1\""),
                    Knob::Miter => ("Swing left", "Swing right", "1°", "to the next stop"),
                    Knob::Depth => ("Depth stop down", "Depth stop up", "1/16\"", "1/4\""),
                    Knob::Cut => ("Shallower", "Deeper", "1/64\"", "1/16\""),
                    Knob::Thickness => ("Thinner", "Thicker", "1/64\"", "1/16\""),
                };
                step(s, less, more, small, big)
            }
            Remake => {
                "Make it this way: every piece this cut made, and everything after".to_string()
            }
            StopEditing => "Leave it as it was".to_string(),
            Unglue(_) => "Take this joint apart".to_string(),
            History => "List how it was made, to change a cut".to_string(),
            Loosen => "Take it off its glue-up".to_string(),
            Duplicate(entry) if entry.member.is_some() => {
                "Duplicate this board on its own: another made the same way, from new lumber, \
                 on the parts shelf"
                    .to_string()
            }
            Duplicate(_) => {
                "Duplicate it: another made the same way, from new lumber, on the parts shelf"
                    .to_string()
            }
            Stash(_) => "Put it away".to_string(),
            Help => "Controls".to_string(),
            Inventory => "Inventory".to_string(),
        }
    }

    /// What a piece of wood from the inventory does where you are.
    fn use_here(&self) -> &'static str {
        match self.view {
            ShopView::TableSaw => "Put it on the table saw",
            ShopView::MiterSaw => "Put it on the miter saw",
            ShopView::Jointer => "Put it on the jointer",
            ShopView::Planer => "Put it on the planer",
            ShopView::Bench => "Set it on the bench",
            ShopView::Overview | ShopView::Studio => "Pick it up",
        }
    }

    /// Whether `action` would do anything now.
    fn can(&self, action: Action) -> bool {
        use Action::*;
        let (table, miter, bench) = (self.table_saw, self.miter_saw, self.bench);
        let (jointer, planer) = (self.jointer, self.planer);
        match (self.view, action) {
            (view, Take(_) | NewLumber(_) | Load(_)) => {
                inventory::can_use_at(view, table, miter, jointer, planer)
            }
            (_, PutAway | Stash(Where::Hand)) => self.hand.0.is_some(),
            (_, Duplicate(entry)) => {
                !matches!(entry.at, Where::Lumber(_)) && self.places().wood(entry).is_some()
            }
            (_, Stash(Where::Saw(ShopView::TableSaw, part))) => table.can_take(part),
            (_, Stash(Where::Saw(ShopView::MiterSaw, part))) => miter.can_take(part),
            (_, Stash(Where::Saw(ShopView::Jointer, part))) => jointer.can_take(part),
            (_, Stash(Where::Saw(ShopView::Planer, part))) => planer.can_take(part),
            (_, Stash(Where::Bench(index))) => index < bench.groups.len(),
            (_, Stash(_)) => false,
            (_, Tweak(..) | StopEditing) => self.editor.editing.is_some(),
            (_, Remake) => self
                .editor
                .editing
                .as_ref()
                .is_some_and(|editing| editing.changed() && editing.preview.is_ok()),
            (ShopView::Bench, ResetView) => *self.bench_view != BenchView::default(),
            (_, ResetView) => *self.saw_view != SawView::default(),
            (ShopView::Bench, History) => bench.picked.is_some(),
            (ShopView::Bench, Loosen) => !bench.picked_joints().is_empty(),
            // Nothing in the studio comes from the inventory or goes into it.
            (ShopView::Studio, Inventory) => false,
            (view, Copy) => {
                to_copy(view, self.hand, table, miter, jointer, planer, bench).is_some()
            }
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
            (ShopView::Jointer, Store) => jointer.can_put_away(),
            (ShopView::Jointer, TurnLeft | TurnRight | Flip | Rest) => jointer.can_turn(),
            (ShopView::Jointer, StandUp) => jointer.can_tip(),
            (ShopView::Jointer, Push) => jointer.can_push(),
            (ShopView::Jointer, BringBack) => jointer.can_bring_back(),
            (ShopView::Jointer, Depth(_) | Guide(_) | Tilt(_)) => jointer.can_set(),
            (ShopView::Planer, Store) => planer.can_put_away(),
            (ShopView::Planer, TurnLeft | TurnRight | Flip | Rest) => planer.can_turn(),
            (ShopView::Planer, StandUp) => planer.can_tip(),
            (ShopView::Planer, Push) => planer.can_push(),
            (ShopView::Planer, BringBack) => planer.can_bring_back(),
            (ShopView::Planer, Blade(_)) => planer.can_set(),
            (ShopView::Bench, TurnLeft | TurnRight | Flip) => {
                bench.lining_up.is_some() || bench.turnable().is_some()
            }
            (ShopView::Bench, Fasten | Slide(..)) => bench.lining_up.is_some(),
            (ShopView::Bench, LetGo) => bench.picked.is_some(),
            (ShopView::Bench, PickUp) => bench.picked.is_some() && self.hand.0.is_none(),
            (ShopView::Bench, TakeApart) => bench.can_take_apart(),
            (ShopView::Bench, Plans | Studio) => !bench.groups.is_empty(),
            (ShopView::Bench, Share) => !bench.groups.is_empty() && !self.sharing.putting_up(),
            (_, PutUp | StopSharing) => self.sharing.naming().is_some(),
            (ShopView::Studio, Render) => {
                self.studio.shoot().is_some()
                    && (!self.studio.rendering() || self.studio.can_finish_now())
            }
            (ShopView::Studio, StopRender) => self.studio.rendering() || self.studio.done(),
            (
                ShopView::Studio,
                TurnLeft | TurnRight | Flip | ZoomIn | ZoomOut | Light(_) | Swing(_) | Raise(_)
                | Brighten(_) | Tip(_) | Lens(_) | Backdrop(_) | Quality(_) | Engine | DepthOfField,
            ) => self.studio.shoot().is_some() && !self.studio.rendering(),
            _ => true,
        }
    }

    /// Whether `action`'s button is lit up, as a choice that's made: the station you're at,
    /// the fastener in use, an open panel.
    fn lit(&self, action: Action) -> bool {
        match action {
            Action::GoTo(view) => view == self.view,
            Action::Use(fastener) => self.bench.fastener == fastener,
            Action::Share => self.sharing.naming().is_some() || self.sharing.putting_up(),
            Action::SwitchGuide => self.table_saw.using_gauge(),
            Action::Rest => match self.view {
                ShopView::TableSaw => self.table_saw.choosing_rest(),
                ShopView::MiterSaw => self.miter_saw.choosing_rest(),
                ShopView::Jointer => self.jointer.choosing_rest(),
                ShopView::Planer => self.planer.choosing_rest(),
                _ => false,
            },
            Action::Light(index) => self.studio.setup.picked == index,
            Action::Engine => self.studio.setup.engine == crate::shoot::Engine::Gpu,
            Action::DepthOfField => self.studio.setup.depth_of_field,
            Action::Help => self.showing.help,
            Action::Inventory => self.showing.inventory,
            Action::Select(entry) => self.tree.selected == Some(entry),
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
            Setting::Cut => (
                "Depth of cut".to_string(),
                self.jointer.machine.depth.to_string(),
            ),
            Setting::JointerFence => (
                "Fence".to_string(),
                format!("{} of knife", self.jointer.machine.fence),
            ),
            Setting::FenceTilt => (
                "Fence tilt".to_string(),
                match self.jointer.machine.tilt {
                    woodsim::Angle::ZERO => "square".to_string(),
                    tilt => tilt.to_string(),
                },
            ),
            Setting::Thickness => (
                "Thickness".to_string(),
                self.planer.machine.thickness.to_string(),
            ),
            Setting::OnBench => ("On the bench".to_string(), bench::reading(self.bench)),
            Setting::LightAround | Setting::LightHeight | Setting::LightPower => {
                let (name, light) = self.studio.picked();
                let (around, height) = Studio::where_light(light);
                match setting {
                    Setting::LightAround => (name.to_string(), around),
                    Setting::LightHeight => (format!("{name} height"), height),
                    _ => (format!("{name} power"), Studio::power_reading(light)),
                }
            }
            Setting::CameraHeight => (
                "Camera".to_string(),
                format!("{:.0}° down", self.studio.setup.pitch.to_degrees()),
            ),
            Setting::Lens => ("Lens".to_string(), format!("{} mm", self.studio.setup.lens)),
        }
    }

    /// The line on what's going on.
    fn status(&self, hovered: Option<ShopView>) -> String {
        if let Some(notice) = self.notice {
            return notice.to_string();
        }
        let holding = self.hand.0.is_some();
        match self.view {
            ShopView::Overview => match (hovered, holding) {
                (Some(ShopView::TableSaw), false) => "Click to work at the table saw.",
                (Some(ShopView::MiterSaw), false) => "Click to work at the miter saw.",
                (Some(ShopView::Jointer), false) => "Click to work at the jointer.",
                (Some(ShopView::Planer), false) => "Click to work at the planer.",
                (Some(ShopView::Bench), false) => "Click to work at the workbench.",
                (Some(ShopView::TableSaw), true) => "Click to set it down on the table saw.",
                (Some(ShopView::MiterSaw), true) => "Click to set it down on the miter saw.",
                (Some(ShopView::Jointer), true) => "Click to set it down on the jointer.",
                (Some(ShopView::Planer), true) => "Click to set it down on the planer.",
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
            ShopView::Jointer => self.jointer.status(),
            ShopView::Planer => self.planer.status(),
            ShopView::Bench => self.bench.status(holding),
            ShopView::Studio => self.studio.status(),
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
            "Drag anything else, or right-drag anywhere: look around   Scroll: zoom",
        ],
        ShopView::MiterSaw => &[
            "Drag the stop block to move it",
            "Click a board: pick it up",
            "Holding one: click the saw to set it against the stop",
            "Drag anything else, or right-drag anywhere: look around   Scroll: zoom",
        ],
        ShopView::Jointer => &[
            "Drag the fence to move it",
            "Drag the board forward to feed it across, back to back it out",
            "Click a board clear of the cutterhead: pick it up",
            "Holding one: click the jointer to set it against the fence",
            "Drag anything else, or right-drag anywhere: look around   Scroll: zoom",
        ],
        ShopView::Planer => &[
            "Drag the board forward to feed it in; the rollers take it through",
            "Click a board clear of the head: pick it up",
            "Holding one: click the planer to set it on the bed",
            "Drag anything else, or right-drag anywhere: look around   Scroll: zoom",
        ],
        ShopView::Bench => &[
            "Click a surface of a part, then the surface it goes against",
            "Drag: look around   Scroll: zoom",
        ],
        ShopView::Studio => &[
            "Drag sideways: turn the piece   Drag up or down: look from higher or lower",
            "Scroll: zoom",
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
        Setting::Cut => {
            "Up/Down arrows: lower or raise the infeed table, cutting 1/64\" deeper or shallower \
             (Shift: 1/16\")"
        }
        Setting::JointerFence => "Left/Right arrows: move the fence 1/16\" (Shift: 1\")",
        Setting::FenceTilt => {
            ", and .: tilt the fence further or back 1° (Shift: to the next stop)"
        }
        Setting::Thickness => "Up/Down arrows: raise or lower the head 1/64\" (Shift: 1/16\")",
        Setting::OnBench => return None,
        Setting::LightAround => "Left/Right arrows: swing the light around 5° (Shift: 45°)",
        Setting::LightHeight => "Up/Down arrows: raise or lower the light 5° (Shift: 15°)",
        Setting::LightPower => "[ and ]: dim or brighten the light 10% (Shift: 50%)",
        Setting::CameraHeight => "W/S: look from higher or lower 1° (Shift: 5°)",
        Setting::Lens => ", and .: a shorter or longer lens 5 mm (Shift: to the next lens)",
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
    // Along the top: the inventory, the way back to the shop, and each station on the left;
    // the frame rate, copying, and the controls on the right.
    let top_left = commands
        .spawn(strip(Node {
            top: px(14),
            left: px(14),
            column_gap: px(8),
            ..default()
        }))
        .id();
    let inventory = spawn_button(
        &mut commands,
        &icons,
        top_left,
        Action::Inventory,
        Icon::Inventory,
        Look::Round,
        TipAt::BelowLeft,
    );
    commands.entity(inventory).insert(InventoryButton);
    commands.spawn((
        Node {
            width: px(6),
            ..default()
        },
        ChildOf(top_left),
    ));
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
        (ShopView::Jointer, Icon::Jointer),
        (ShopView::Planer, Icon::Planer),
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
    for (action, icon) in [(Action::Copy, Icon::Copy), (Action::Help, Icon::Help)] {
        spawn_button(
            &mut commands,
            &icons,
            top_right,
            action,
            icon,
            Look::Round,
            TipAt::BelowRight,
        );
    }
    // The inventory's tree hangs down the left below its button, its list scrolling if it's
    // long, over the card for the line picked out; the controls go in the middle of the
    // screen.
    commands.spawn((
        panel(Node {
            position_type: PositionType::Absolute,
            top: px(INVENTORY_TOP),
            left: px(14),
            width: px(INVENTORY_WIDTH),
            max_height: vh(70),
            min_height: px(0),
            flex_direction: FlexDirection::Column,
            padding: UiRect::axes(px(6), px(8)),
            ..default()
        }),
        PanelOf(Readout::Inventory),
        children![
            (
                Node {
                    min_height: px(0),
                    flex_shrink: 1.0,
                    column_gap: px(4),
                    ..default()
                },
                Children::spawn(SpawnWith(|row: &mut RelatedSpawner<ChildOf>| {
                    let list = row
                        .spawn((
                            Node {
                                flex_direction: FlexDirection::Column,
                                flex_grow: 1.0,
                                overflow: Overflow::scroll_y(),
                                min_height: px(0),
                                padding: UiRect::horizontal(px(2)),
                                ..default()
                            },
                            ScrollPosition::default(),
                            InventoryList,
                            ListArea,
                        ))
                        .id();
                    // Only there when the list's too long to see all at once
                    // (`fit_inventory`).
                    row.spawn((
                        Node {
                            display: Display::None,
                            width: px(6),
                            margin: UiRect::vertical(px(4)),
                            ..default()
                        },
                        Scrollbar::new(list, ControlOrientation::Vertical, 24.0),
                        children![(
                            Hovered::default(),
                            BackgroundColor(INK.with_alpha(0.22)),
                            ScrollbarThumb {
                                border_radius: BorderRadius::all(px(3)),
                                border: UiRect::ZERO,
                            },
                        )],
                    ));
                })),
            ),
            (
                Node {
                    display: Display::None,
                    flex_direction: FlexDirection::Column,
                    flex_shrink: 0.0,
                    row_gap: px(8),
                    margin: UiRect::top(px(6)),
                    padding: UiRect {
                        top: px(10),
                        ..UiRect::horizontal(px(6))
                    },
                    border: UiRect::top(px(1.5)),
                    ..default()
                },
                BorderColor::all(EDGE),
                Details,
            )
        ],
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
    // The station's settings down the right, under the buttons there.
    commands.spawn((
        strip(Node {
            top: px(80),
            right: px(14),
            flex_direction: FlexDirection::Column,
            align_items: AlignItems::FlexEnd,
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
        Look::Row | Look::Chevron => (CHEVRON, 14.0),
    };
    let tab = look == Look::Tab;
    let flat = matches!(look, Look::Row | Look::Chevron);
    let button = commands
        .spawn((
            Button,
            Hovered::default(),
            Control { action, look, tip },
            Node {
                width: if tab { Val::Auto } else { px(size) },
                height: px(size),
                flex_shrink: 0.0,
                padding: UiRect::axes(px(if tab { 14.0 } else { 0.0 }), px(0)),
                column_gap: px(8),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                border: match flat {
                    true => UiRect::ZERO,
                    false => UiRect {
                        bottom: px(3),
                        ..UiRect::all(px(1.5))
                    },
                },
                border_radius: BorderRadius::all(px(size / 2.0)),
                ..default()
            },
            BackgroundColor(if flat { Color::NONE } else { FACE }),
            BorderColor::all(EDGE),
            ChildOf(parent),
        ))
        .id();
    if !flat {
        commands
            .entity(button)
            .insert(BoxShadow::new(SHADOW, px(0), px(2), px(0), px(6)));
    }
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
/// where it's near the edge of the screen; or to its right, for the inventory down the left.
#[derive(Clone, Copy, PartialEq, Eq)]
enum TipAt {
    Above,
    Below,
    BelowLeft,
    BelowRight,
    Right,
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
    if actions.just(Action::History) && !showing.inventory {
        showing.inventory = true;
    }
}
/// The card plans are named on before they go up on the marketplace: what they'll be called,
/// in a box to type over, and buttons to put them up or not. It's there while they're named.
#[derive(Component)]
struct NamingCard;

fn show_naming_card(
    mut commands: Commands,
    icons: Res<Icons>,
    sharing: Res<Sharing>,
    cards: Query<Entity, With<NamingCard>>,
) {
    if !sharing.is_changed() {
        return;
    }
    for card in &cards {
        commands.entity(card).despawn();
    }
    let Some(suggested) = sharing.naming() else {
        return;
    };
    let card = commands
        .spawn((
            strip(Node {
                top: px(80),
                width: percent(100),
                justify_content: JustifyContent::Center,
                ..default()
            }),
            NamingCard,
        ))
        .id();
    let panel = commands
        .spawn((
            panel(Node {
                flex_direction: FlexDirection::Column,
                row_gap: px(10),
                padding: UiRect::all(px(16)),
                ..default()
            }),
            GlobalZIndex(4),
            ChildOf(card),
        ))
        .id();
    commands.spawn((bold("Put the plans up", 18.0, INK), ChildOf(panel)));
    commands.spawn((
        text(
            "Anyone with the link can see them, with every cut and joint that made it.",
            14.0,
            INK.with_alpha(0.7),
        ),
        ChildOf(panel),
    ));
    let mut title = EditableText {
        max_characters: Some(upload::MOST_TITLE),
        visible_width: Some(32.0),
        allow_newlines: false,
        ..EditableText::new(suggested)
    };
    // Picked out, ready to type over.
    title.queue_edit(TextEdit::SelectAll);
    commands.spawn((
        Node {
            padding: UiRect::axes(px(10), px(6)),
            border: UiRect::all(px(2)),
            border_radius: BorderRadius::all(px(8)),
            ..default()
        },
        BackgroundColor(FACE),
        BorderColor::all(EDGE),
        TitleBox,
        TextInput,
        title,
        TextLayout::no_wrap(),
        TextFont {
            font_size: FontSize::Px(18.0),
            ..default()
        },
        TextColor(INK),
        TextCursorStyle::default(),
        (SelectAllOnFocus, AutoFocus),
        ChildOf(panel),
    ));
    let buttons = commands
        .spawn((
            Node {
                column_gap: px(8),
                justify_content: JustifyContent::FlexEnd,
                ..default()
            },
            ChildOf(panel),
        ))
        .id();
    spawn_button(
        &mut commands,
        &icons,
        buttons,
        Action::StopSharing,
        Icon::LetGo,
        Look::Round,
        TipAt::Below,
    );
    spawn_button(
        &mut commands,
        &icons,
        buttons,
        Action::PutUp,
        Icon::Share,
        Look::Go,
        TipAt::Below,
    );
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
        let flat = matches!(control.look, Look::Row | Look::Chevron);
        let base = match (lit, control.look) {
            (true, Look::Row) => ACCENT.with_alpha(0.2),
            (true, _) => ACCENT,
            (false, Look::Go) => GO,
            (false, Look::Row | Look::Chevron) => Color::NONE,
            (false, _) => FACE,
        };
        let color = if !can {
            base.with_alpha(0.55)
        } else if hover && base == Color::NONE {
            INK.with_alpha(0.07)
        } else if hover && base == FACE {
            FACE_HOVER
        } else if hover && flat {
            ACCENT.with_alpha(0.26)
        } else if hover {
            base.lighter(0.06)
        } else {
            base
        };
        if face.0 != color {
            face.0 = color;
        }
        let ink = match (lit || control.look == Look::Go, can) {
            (_, true) if flat => INK.with_alpha(0.7),
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
    let hovered = buttons.iter().find(|(control, hovered, ..)| {
        hovered.get() && !matches!(control.look, Look::Tab | Look::Row)
    });
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
        TipAt::Right => (middle.x + half.x + GAP, middle.y, (0.0, -50.0)),
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

/// The card under the tree for the line picked out.
#[derive(Component)]
struct Details;

/// How wide the inventory panel is, how far a glue-up's boards are set in under it, and how
/// big a line's chevron and picture are.
const INVENTORY_WIDTH: f32 = 360.0;
const INDENT: f32 = 16.0;
const CHEVRON: f32 = 22.0;
const THUMBNAIL: f32 = 30.0;

/// The tree and the card as last built, so they're only built again when what they show
/// changes, and the pictures of wood they show, painted once each.
#[derive(Default)]
struct BuiltTree {
    view: Option<ShopView>,
    lines: Vec<Line>,
    card: Option<Card>,
    pictures: Vec<(Workpiece, Handle<Image>)>,
}

/// What the card under the tree shows: the line picked out, its wood, and where that is in
/// words; the pass or joint, if it's one; and the saw as it's being set to change the pass,
/// with what that makes of the wood, or why it can't.
#[derive(Clone, PartialEq)]
struct Card {
    entry: Entry,
    work: Workpiece,
    at: String,
    detail: Option<DetailLine>,
    editing: Option<(woodsim::Tool, Result<Workpiece, String>)>,
}

impl BuiltTree {
    /// A picture of `work`, painted the first time it's needed.
    fn picture(&mut self, work: &Workpiece, images: &mut Assets<Image>) -> Handle<Image> {
        if let Some((_, handle)) = self.pictures.iter().find(|(seen, _)| seen == work) {
            return handle.clone();
        }
        let handle = images.add(picture(work));
        self.pictures.push((work.clone(), handle.clone()));
        handle
    }
}

/// Fills the inventory's tree afresh whenever what it lists changes: a heading for what's in
/// hand, each station, each shelf, the parts saved, and new lumber, with the wood under each,
/// and the card for the line picked out with buttons for what can be done with it.
fn build_tree(
    mut commands: Commands,
    game: Game,
    icons: Res<Icons>,
    mut images: ResMut<Assets<Image>>,
    list: Single<Entity, With<InventoryList>>,
    details: Single<(Entity, &mut Node), With<Details>>,
    mut built: Local<BuiltTree>,
) {
    if !game.is_changed() || *game.view == ShopView::Studio {
        return;
    }
    let shop = game.shop();
    let places = shop.places();
    let view = Some(shop.view);
    let lines = places.lines(shop.tree);
    if built.lines != lines || built.view != view {
        let list = *list;
        commands.entity(list).despawn_related::<Children>();
        let mut pictures = std::mem::take(&mut built.pictures);
        built.pictures.clear();
        for line in &lines {
            match line {
                Line::Heading {
                    heading,
                    count,
                    open,
                } => tree_heading(&mut commands, &icons, list, *heading, *count, *open),
                Line::Empty(said) => {
                    commands.spawn((
                        text(*said, 13.0, INK.with_alpha(0.55)),
                        Node {
                            margin: UiRect {
                                left: px(CHEVRON + 6.0),
                                bottom: px(4),
                                ..default()
                            },
                            ..default()
                        },
                        ChildOf(list),
                    ));
                }
                Line::Wood(line) => {
                    // Pictures painted for the last tree, kept for this one if it shows them.
                    if let Some(at) = pictures.iter().position(|(seen, _)| *seen == line.work) {
                        built.pictures.push(pictures.swap_remove(at));
                    }
                    let picture = built.picture(&line.work, &mut images);
                    tree_line(&mut commands, &icons, &shop, list, line, picture);
                }
                Line::Detail(line) => tree_detail(&mut commands, &icons, list, line),
            }
        }
        built.lines = lines;
    }
    let card = shop.tree.selected.and_then(|entry| {
        let detail = match entry.detail {
            Some(_) => Some(places.detail(entry)?),
            None => None,
        };
        let editing = shop
            .editor
            .editing
            .as_ref()
            .filter(|editing| editing.entry == entry)
            .map(|editing| (editing.tool, editing.preview.clone()));
        Some(Card {
            entry,
            work: places.wood(entry.wood())?,
            at: places.say_where(entry),
            detail,
            editing,
        })
    });
    if built.card != card || built.view != view {
        let (details, mut node) = details.into_inner();
        commands.entity(details).despawn_related::<Children>();
        let display = if card.is_some() {
            Display::Flex
        } else {
            Display::None
        };
        if node.display != display {
            node.display = display;
        }
        if let Some(shown) = &card {
            let label = shown.entry.member.and_then(|member| {
                match places.wood(Entry::at(shown.entry.at))? {
                    Workpiece::GlueUp(assembly) => plan::labels(&assembly).get(member).cloned(),
                    Workpiece::Board(_) => None,
                }
            });
            let shows = match &shown.editing {
                Some((_, Ok(made))) => made,
                _ => &shown.work,
            };
            let picture = built.picture(shows, &mut images);
            let mut fill = Fill {
                commands: &mut commands,
                icons: &icons,
                shop: &shop,
                card: details,
            };
            match (&shown.detail, &shown.editing) {
                (Some(detail), Some((saw, made))) => {
                    fill.editor(detail, *saw, made, label, picture);
                }
                (Some(detail), None) => fill.detail(shown, detail, label, picture),
                (None, _) => fill.wood(shown, label, picture),
            }
        }
        built.card = card;
    }
    built.view = view;
}

/// A heading of the tree: a click folds it away or opens it again.
fn tree_heading(
    commands: &mut Commands,
    icons: &Icons,
    list: Entity,
    heading: Heading,
    count: usize,
    open: bool,
) {
    let row = commands
        .spawn((
            Button,
            Hovered::default(),
            Control {
                action: Action::Fold(parts::Fold::Heading(heading)),
                look: Look::Row,
                tip: TipAt::Right,
            },
            Node {
                align_items: AlignItems::Center,
                column_gap: px(6),
                padding: UiRect::axes(px(0), px(3)),
                margin: UiRect::top(px(6)),
                border_radius: BorderRadius::all(px(8)),
                ..default()
            },
            BackgroundColor(Color::NONE),
            ChildOf(list),
        ))
        .id();
    let chevron = if open { Arrow::Down } else { Arrow::Right };
    commands.spawn((
        Node {
            width: px(CHEVRON),
            height: px(CHEVRON),
            justify_content: JustifyContent::Center,
            align_items: AlignItems::Center,
            flex_shrink: 0.0,
            ..default()
        },
        Pickable::IGNORE,
        ChildOf(row),
        children![(
            ImageNode {
                image: icons.get(Icon::Step(chevron, false)),
                color: INK,
                ..default()
            },
            Node {
                width: px(14),
                height: px(14),
                ..default()
            },
            Pickable::IGNORE,
        )],
    ));
    commands.spawn((
        bold(heading.name(), 14.0, INK.with_alpha(0.65)),
        Node {
            flex_grow: 1.0,
            ..default()
        },
        Pickable::IGNORE,
        ChildOf(row),
    ));
    if heading != Heading::Lumber {
        commands.spawn((
            Node {
                padding: UiRect::axes(px(7), px(0)),
                margin: UiRect::right(px(4)),
                border_radius: BorderRadius::all(px(9)),
                ..default()
            },
            BackgroundColor(INK.with_alpha(if count > 0 { 0.1 } else { 0.05 })),
            Pickable::IGNORE,
            ChildOf(row),
            children![(
                bold(count.to_string(), 12.0, INK.with_alpha(0.6)),
                Pickable::IGNORE
            )],
        ));
    }
}

/// A line of the tree for a piece of wood: a chevron to list a glue-up's boards, its picture,
/// what it is, in bold, and what else there is to say about it, how many there are, and a
/// button to use it here if it comes from the inventory. A click picks it out.
fn tree_line(
    commands: &mut Commands,
    icons: &Icons,
    shop: &Shop,
    list: Entity,
    line: &WoodLine,
    picture: Handle<Image>,
) {
    // The line lights up here when wood lands in it, behind the button that picks it out.
    let outer = commands
        .spawn((
            Node {
                flex_direction: FlexDirection::Column,
                border_radius: BorderRadius::all(px(8)),
                ..default()
            },
            BackgroundColor(Color::NONE),
            ChildOf(list),
        ))
        .id();
    if !line.ids.is_empty() {
        commands.entity(outer).insert(RowOf(line.ids.clone()));
    }
    let depth = usize::from(line.entry.member.is_some());
    let row = commands
        .spawn((
            Button,
            Hovered::default(),
            Control {
                action: Action::Select(line.entry),
                look: Look::Row,
                tip: TipAt::Right,
            },
            Node {
                align_items: AlignItems::Center,
                column_gap: px(6),
                padding: UiRect {
                    left: px(depth as f32 * INDENT),
                    right: px(2),
                    top: px(3),
                    bottom: px(3),
                },
                border_radius: BorderRadius::all(px(8)),
                ..default()
            },
            BackgroundColor(Color::NONE),
            ChildOf(outer),
        ))
        .id();
    match line.open {
        Some(open) => {
            let arrow = if open { Arrow::Down } else { Arrow::Right };
            spawn_button(
                commands,
                icons,
                row,
                Action::Fold(parts::Fold::Wood(line.entry)),
                Icon::Step(arrow, false),
                Look::Chevron,
                TipAt::Right,
            );
        }
        None => {
            commands.spawn((
                Node {
                    width: px(CHEVRON),
                    flex_shrink: 0.0,
                    ..default()
                },
                Pickable::IGNORE,
                ChildOf(row),
            ));
        }
    }
    commands.spawn((
        ImageNode::new(picture),
        Node {
            width: px(THUMBNAIL),
            height: px(THUMBNAIL),
            flex_shrink: 0.0,
            ..default()
        },
        Pickable::IGNORE,
        ChildOf(row),
    ));
    let words = commands
        .spawn((
            Node {
                flex_direction: FlexDirection::Column,
                flex_grow: 1.0,
                flex_shrink: 1.0,
                min_width: px(0),
                ..default()
            },
            Pickable::IGNORE,
            ChildOf(row),
        ))
        .id();
    named(commands, words, line.label.as_deref(), &line.name, 14.0);
    if !line.notes.is_empty() {
        commands.spawn((
            text(&line.notes, 12.0, INK.with_alpha(0.6)),
            Pickable::IGNORE,
            ChildOf(words),
        ));
    }
    if line.count > 1 {
        commands.spawn((
            Node {
                padding: UiRect::axes(px(7), px(1)),
                border_radius: BorderRadius::all(px(9)),
                flex_shrink: 0.0,
                ..default()
            },
            BackgroundColor(ACCENT),
            Pickable::IGNORE,
            ChildOf(row),
            children![(
                bold(format!("×{}", line.count), 13.0, Color::WHITE),
                Pickable::IGNORE
            )],
        ));
    }
    let use_here = match line.entry.at {
        _ if line.entry.member.is_some() => None,
        Where::Shelf(id) => Some(Action::Take(id)),
        Where::Saved(index) => Some(Action::Load(index)),
        Where::Lumber(index) => Some(Action::NewLumber(index)),
        _ => None,
    };
    if let Some(action) = use_here {
        spawn_button(
            commands,
            icons,
            row,
            action,
            use_icon(shop.view),
            Look::Small,
            TipAt::Right,
        );
    }
}

/// What a piece of wood is called, in bold, after its letter in a glue-up's plans, if it has
/// one.
fn named(commands: &mut Commands, parent: Entity, label: Option<&str>, name: &str, size: f32) {
    let row = commands
        .spawn((
            Node {
                align_items: AlignItems::Center,
                column_gap: px(6),
                ..default()
            },
            Pickable::IGNORE,
            ChildOf(parent),
        ))
        .id();
    if let Some(label) = label {
        commands.spawn((
            Node {
                padding: UiRect::axes(px(5), px(0)),
                border_radius: BorderRadius::all(px(5)),
                ..default()
            },
            BackgroundColor(INK.with_alpha(0.75)),
            Pickable::IGNORE,
            ChildOf(row),
            children![(bold(label, size - 3.0, Color::WHITE), Pickable::IGNORE)],
        ));
    }
    commands.spawn((bold(name, size, INK), Pickable::IGNORE, ChildOf(row)));
}

/// The icon for using a piece of wood where you are: picking it up in the shop, or setting it
/// down at a station.
fn use_icon(view: ShopView) -> Icon {
    match view {
        ShopView::Overview => Icon::PickUp,
        _ => Icon::Place,
    }
}

/// What fills the card under the tree.
struct Fill<'a, 'w, 's> {
    commands: &'a mut Commands<'w, 's>,
    icons: &'a Icons,
    shop: &'a Shop<'a>,
    card: Entity,
}

impl Fill<'_, '_, '_> {
    /// The line picked out, a piece of wood: its picture, what it is and where, and buttons
    /// for what can be done with it there.
    fn wood(&mut self, shown: &Card, label: Option<String>, picture: Handle<Image>) {
        let (name, notes) = inventory::describe(&shown.work);
        let words = self.top(picture);
        named(self.commands, words, label.as_deref(), &name, 16.0);
        self.said(words, &notes, 0.7);
        self.said(words, &capitalized(&shown.at), 0.55);
        let actions = card_actions(self.shop, shown.entry);
        self.buttons(actions);
    }

    /// A pass that cut it, or a joint that holds it: what it did, how, and a button to change
    /// it, if it can be.
    fn detail(
        &mut self,
        shown: &Card,
        detail: &DetailLine,
        label: Option<String>,
        picture: Handle<Image>,
    ) {
        let words = self.top(picture);
        named(self.commands, words, label.as_deref(), &detail.name, 16.0);
        self.said(words, &detail.notes, 0.7);
        let what = match detail.kind {
            Kind::Joint => "A joint of the glue-up",
            _ => "A cut that made it",
        };
        self.said(words, &format!("{what}, {}", shown.at), 0.55);
        let action = match detail.kind {
            Kind::Joint => (Action::Unglue(shown.entry), Icon::Unglue),
            _ => (Action::Edit(shown.entry), Icon::Pencil),
        };
        if detail.changes {
            self.buttons(vec![action]);
        } else {
            let why = match (shown.entry.at, detail.kind) {
                (Where::Saved(_), _) => "Saved parts stay as they were saved.",
                (_, Kind::Joint) => "Take it off the saw to take it apart.",
                _ => "It went through a glue-up, so it can't be cut again.",
            };
            self.said(words, why, 0.55);
        }
    }

    /// A pass being changed: each of the saw's settings with buttons to step it, what the piece
    /// comes out as set that way (or why it can't be), and buttons to make it that way or
    /// leave it as it was.
    fn editor(
        &mut self,
        detail: &DetailLine,
        saw: woodsim::Tool,
        made: &Result<Workpiece, String>,
        label: Option<String>,
        picture: Handle<Image>,
    ) {
        let title = format!("Changing the {}", detail.name.to_lowercase());
        let header = self
            .commands
            .spawn((Node::default(), ChildOf(self.card)))
            .id();
        named(self.commands, header, label.as_deref(), &title, 16.0);
        for knob in edit::knobs(saw) {
            let (name, value) = edit::reading(saw, knob);
            let row = self
                .commands
                .spawn((
                    Node {
                        align_items: AlignItems::Center,
                        column_gap: px(4),
                        ..default()
                    },
                    ChildOf(self.card),
                ))
                .id();
            self.commands.spawn((
                Node {
                    flex_direction: FlexDirection::Column,
                    flex_grow: 1.0,
                    ..default()
                },
                ChildOf(row),
                children![
                    (bold(name, 12.0, INK.with_alpha(0.6)), Pickable::IGNORE),
                    (bold(value, 18.0, ACCENT), Pickable::IGNORE),
                ],
            ));
            for (more, big) in [(false, true), (false, false), (true, false), (true, true)] {
                let arrow = if more { Arrow::Right } else { Arrow::Left };
                spawn_button(
                    self.commands,
                    self.icons,
                    row,
                    Action::Tweak(knob, Step { more, big }),
                    Icon::Step(arrow, big),
                    Look::Small,
                    TipAt::Above,
                );
            }
        }
        let words = self.top(picture);
        match made {
            Ok(made) => {
                let (name, notes) = inventory::describe(made);
                self.said(words, "Comes out", 0.55);
                named(self.commands, words, None, &name, 15.0);
                self.said(words, &notes, 0.7);
            }
            Err(why) => {
                self.said(words, "Can't be made this way:", 0.55);
                self.commands.spawn((
                    text(why.clone(), 13.0, ACCENT),
                    Pickable::IGNORE,
                    ChildOf(words),
                ));
            }
        }
        let remake = Tool {
            look: Look::Go,
            ..tool(Action::Remake, Icon::Fasten)
        };
        let row = self.button_row();
        for button in [remake, tool(Action::StopEditing, Icon::LetGo)] {
            spawn_button(
                self.commands,
                self.icons,
                row,
                button.action,
                button.icon,
                button.look,
                TipAt::Above,
            );
        }
    }

    /// A row along the card with `picture` in it, and the column of words beside it.
    fn top(&mut self, picture: Handle<Image>) -> Entity {
        let top = self
            .commands
            .spawn((
                Node {
                    align_items: AlignItems::Center,
                    column_gap: px(10),
                    ..default()
                },
                ChildOf(self.card),
            ))
            .id();
        self.commands.spawn((
            ImageNode::new(picture),
            Node {
                width: px(52),
                height: px(52),
                flex_shrink: 0.0,
                ..default()
            },
            Pickable::IGNORE,
            ChildOf(top),
        ));
        self.commands
            .spawn((
                Node {
                    flex_direction: FlexDirection::Column,
                    flex_shrink: 1.0,
                    min_width: px(0),
                    row_gap: px(1),
                    ..default()
                },
                ChildOf(top),
            ))
            .id()
    }

    /// A line of words in `words`, as dark as `ink` says.
    fn said(&mut self, words: Entity, said: &str, ink: f32) {
        if said.is_empty() {
            return;
        }
        self.commands.spawn((
            text(said, 13.0, INK.with_alpha(ink)),
            Pickable::IGNORE,
            ChildOf(words),
        ));
    }

    fn button_row(&mut self) -> Entity {
        self.commands
            .spawn((
                Node {
                    column_gap: px(6),
                    ..default()
                },
                ChildOf(self.card),
            ))
            .id()
    }

    /// A row of small buttons along the card.
    fn buttons(&mut self, actions: Vec<(Action, Icon)>) {
        let row = self.button_row();
        for (action, icon) in actions {
            spawn_button(
                self.commands,
                self.icons,
                row,
                action,
                icon,
                Look::Small,
                TipAt::Above,
            );
        }
    }
}

/// `said` with its first letter a capital.
fn capitalized(said: &str) -> String {
    let mut chars = said.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}

/// A line of the tree for a pass that cut a board, or a joint of a glue-up: its saw or glue,
/// what it did, how, and a button to change it or take it apart, if it can be. A click picks it
/// out.
fn tree_detail(commands: &mut Commands, icons: &Icons, list: Entity, line: &DetailLine) {
    let row = commands
        .spawn((
            Button,
            Hovered::default(),
            Control {
                action: Action::Select(line.entry),
                look: Look::Row,
                tip: TipAt::Right,
            },
            Node {
                align_items: AlignItems::Center,
                column_gap: px(6),
                padding: UiRect {
                    left: px(line.depth() as f32 * INDENT + CHEVRON + 6.0),
                    right: px(2),
                    top: px(2),
                    bottom: px(2),
                },
                border_radius: BorderRadius::all(px(8)),
                ..default()
            },
            BackgroundColor(Color::NONE),
            ChildOf(list),
        ))
        .id();
    let icon = match line.kind {
        Kind::TableSaw => Icon::TableSaw,
        Kind::MiterSaw => Icon::MiterSaw,
        Kind::Jointer => Icon::Jointer,
        Kind::Planer => Icon::Planer,
        Kind::Joint => Icon::Glue,
    };
    commands.spawn((
        ImageNode {
            image: icons.get(icon),
            color: INK.with_alpha(0.55),
            ..default()
        },
        Node {
            width: px(18),
            height: px(18),
            flex_shrink: 0.0,
            ..default()
        },
        Pickable::IGNORE,
        ChildOf(row),
    ));
    commands.spawn((
        Node {
            flex_direction: FlexDirection::Column,
            flex_grow: 1.0,
            flex_shrink: 1.0,
            min_width: px(0),
            ..default()
        },
        Pickable::IGNORE,
        ChildOf(row),
        children![
            (bold(&line.name, 13.0, INK), Pickable::IGNORE),
            (
                text(&line.notes, 12.0, INK.with_alpha(0.6)),
                Pickable::IGNORE
            ),
        ],
    ));
    if line.changes {
        let (action, icon) = match line.kind {
            Kind::Joint => (Action::Unglue(line.entry), Icon::Unglue),
            _ => (Action::Edit(line.entry), Icon::Pencil),
        };
        spawn_button(
            commands,
            icons,
            row,
            action,
            icon,
            Look::Small,
            TipAt::Right,
        );
    }
}

/// What can be done with the wood at `entry`, from its card: go to the station it's on, use it
/// here, duplicate it, put it away, move it to the other shelf, save it, or throw it away.
fn card_actions(shop: &Shop, entry: Entry) -> Vec<(Action, Icon)> {
    let duplicate = (Action::Duplicate(entry), Icon::Duplicate);
    if entry.member.is_some() {
        return vec![duplicate];
    }
    let go_to = |view: ShopView| {
        let icon = match view {
            ShopView::TableSaw => Icon::TableSaw,
            ShopView::MiterSaw => Icon::MiterSaw,
            ShopView::Jointer => Icon::Jointer,
            ShopView::Planer => Icon::Planer,
            _ => Icon::Bench,
        };
        (shop.view != view).then_some((Action::GoTo(view), icon))
    };
    let stash = (Action::Stash(entry.at), Icon::Store);
    match entry.at {
        Where::Hand => vec![(Action::PutAway, Icon::Store), duplicate],
        Where::Saw(view, _) => go_to(view).into_iter().chain([stash, duplicate]).collect(),
        Where::Bench(_) => go_to(ShopView::Bench)
            .into_iter()
            .chain([stash, duplicate])
            .collect(),
        Where::Shelf(id) => {
            let other_shelf = match shop.inventory.get(id).map(|item| item.shelf) {
                Some(Shelf::Offcuts) => Icon::Step(Arrow::Up, false),
                _ => Icon::Step(Arrow::Down, false),
            };
            vec![
                (Action::Take(id), use_icon(shop.view)),
                duplicate,
                (Action::MoveShelf(id), other_shelf),
                (Action::Save(id), Icon::Save),
                (Action::Scrap(id), Icon::Scrap),
            ]
        }
        Where::Saved(index) => vec![
            (Action::Load(index), use_icon(shop.view)),
            (Action::DeleteSaved(index), Icon::Scrap),
        ],
        Where::Lumber(index) => vec![(Action::NewLumber(index), use_icon(shop.view))],
    }
}

/// How far the inventory hangs from the top of the window, and stays above the line on
/// what's going on.
const INVENTORY_TOP: f32 = 80.0;
const INVENTORY_GAP: f32 = 14.0;

/// Keeps the inventory clear of the line on what's going on, however tall the window is, and
/// shows its scrollbar only while its list is too long to see all at once.
fn fit_inventory(
    status: Query<(&PanelOf, &ComputedNode, &UiGlobalTransform)>,
    mut panels: Query<(&PanelOf, &mut Node), Without<Scrollbar>>,
    list: Single<&ComputedNode, With<InventoryList>>,
    scrollbar: Single<&mut Node, With<Scrollbar>>,
) {
    let bottom = status
        .iter()
        .find(|(panel, ..)| panel.0 == Readout::Status)
        .map(|(_, node, transform)| edges(node, transform).min.y);
    if let Some(bottom) = bottom.filter(|&bottom| bottom > 0.0) {
        let height = px((bottom - INVENTORY_GAP - INVENTORY_TOP).max(160.0).round());
        for (panel, mut node) in &mut panels {
            if panel.0 == Readout::Inventory && node.max_height != height {
                node.max_height = height;
            }
        }
    }
    let overflows = list.content_size().y > list.size().y + 1.0;
    let display = if overflows {
        Display::Flex
    } else {
        Display::None
    };
    let mut scrollbar = scrollbar.into_inner();
    if scrollbar.display != display {
        scrollbar.display = display;
    }
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
            Readout::Inventory => showing.inventory && shop.view != ShopView::Studio,
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
        static JOINTER: std::sync::LazyLock<JointerStation> =
            std::sync::LazyLock::new(JointerStation::default);
        static PLANER: std::sync::LazyLock<PlanerStation> =
            std::sync::LazyLock::new(PlanerStation::default);
        static INVENTORY: std::sync::LazyLock<Inventory> =
            std::sync::LazyLock::new(Inventory::default);
        static SAVED: std::sync::LazyLock<Saved> = std::sync::LazyLock::new(Saved::default);
        static TREE: std::sync::LazyLock<Tree> = std::sync::LazyLock::new(Tree::default);
        static EDITOR: std::sync::LazyLock<Editor> = std::sync::LazyLock::new(Editor::default);
        static STUDIO: std::sync::LazyLock<Studio> = std::sync::LazyLock::new(Studio::default);
        static SHARING: std::sync::LazyLock<Sharing> = std::sync::LazyLock::new(Sharing::default);
        static SAW_VIEW: std::sync::LazyLock<SawView> = std::sync::LazyLock::new(SawView::default);
        static BENCH_VIEW: std::sync::LazyLock<BenchView> =
            std::sync::LazyLock::new(BenchView::default);
        Shop {
            view,
            table_saw,
            miter_saw,
            jointer: &JOINTER,
            planer: &PLANER,
            bench,
            hand,
            showing,
            inventory: &INVENTORY,
            saved: &SAVED,
            tree: &TREE,
            editor: &EDITOR,
            notice: None,
            studio: &STUDIO,
            sharing: &SHARING,
            saw_view: &SAW_VIEW,
            bench_view: &BENCH_VIEW,
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
            ShopView::Jointer,
            ShopView::Planer,
            ShopView::Bench,
            ShopView::Studio,
        ] {
            let shop = shop(view, &table_saw, &miter_saw, &bench, &hand, &showing);
            let steps = cards(view).into_iter().flat_map(|(setting, ..)| {
                [false, true].into_iter().flat_map(move |more| {
                    [false, true]
                        .into_iter()
                        .filter_map(move |big| stepping(setting, Step { more, big }))
                })
            });
            for action in dock(view)
                .into_iter()
                .flatten()
                .map(|tool| tool.action)
                .chain(steps)
            {
                assert!(!shop.describe(action).is_empty());
                assert!(
                    key_for(view, action).is_some(),
                    "{action:?} has no key at {view:?}"
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

/// The part of the window the HUD leaves clear in the studio, in logical pixels: left of the
/// settings, below the tabs, and above the line on what's going on. It's the camera's picture,
/// so the piece is framed there and the render shows there. It's measured while setting up,
/// and kept while a render's under way.
#[derive(Resource, Default, Clone, Copy, Debug, PartialEq)]
pub struct Viewfinder {
    pub area: Option<Rect>,
    pub window: Vec2,
}

impl Viewfinder {
    /// How many times wider than high the picture is.
    pub fn aspect(&self) -> Option<f32> {
        let area = self.area?;
        (area.height() > 0.0).then(|| area.width() / area.height())
    }
}

/// A node's edges, in logical pixels.
fn edges(node: &ComputedNode, transform: &UiGlobalTransform) -> Rect {
    let scale = node.inverse_scale_factor;
    let center = transform.affine().translation * scale;
    Rect::from_center_size(center, node.size * scale)
}

/// How far the picture stays from the HUD and the window's edges, and below the top.
const VIEWFINDER_MARGIN: f32 = 14.0;
const VIEWFINDER_TOP: f32 = 72.0;

fn measure_viewfinder(
    game: Game,
    camera: Single<&Camera, With<crate::shop::ShopCamera>>,
    cards: Single<(&ComputedNode, &UiGlobalTransform), With<Cards>>,
    panels: Query<(&PanelOf, &ComputedNode, &UiGlobalTransform)>,
    mut viewfinder: ResMut<Viewfinder>,
) {
    if *game.view != ShopView::Studio || game.showing.built_for != Some(ShopView::Studio) {
        if viewfinder.area.is_some() {
            viewfinder.area = None;
        }
        return;
    }
    if game.studio.rendering() || game.studio.done() {
        return;
    }
    let Some(window) = camera.logical_viewport_size() else {
        return;
    };
    let (node, transform) = *cards;
    let right = edges(node, transform).min.x - VIEWFINDER_MARGIN;
    let bottom = panels
        .iter()
        .filter(|(panel, ..)| panel.0 == Readout::Status)
        .map(|(_, node, transform)| edges(node, transform).min.y - VIEWFINDER_MARGIN)
        .fold(window.y - VIEWFINDER_MARGIN, f32::min);
    let area = Rect::new(VIEWFINDER_MARGIN, VIEWFINDER_TOP, right, bottom);
    let measured = Viewfinder {
        area: (area.width() > 50.0 && area.height() > 50.0).then_some(area),
        window,
    };
    if *viewfinder != measured {
        *viewfinder = measured;
    }
}
