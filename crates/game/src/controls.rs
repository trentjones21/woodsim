//! Everything the player can do, from the keyboard or the buttons on screen. Each control is an
//! `Action`, bound to a key at each view (`bound`), and the stations act on `Actions` rather than
//! on keys, so a button and its key do exactly the same thing.

use bevy::{input::InputSystems, picking::PickingSystems, prelude::*};
use woodsim::Fastener;

use crate::{
    edit::Knob,
    parts::{Entry, Fold, Where},
    shop::ShopView,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Action {
    /// Back out to the whole shop.
    Back,
    /// On around the stations: the table saw, the miter saw, the workbench.
    NextStation,
    /// Straight to a station, or the whole shop.
    GoTo(ShopView),
    /// Looking around the shop.
    Pan(Arrow),
    RotateLeft,
    RotateRight,
    ZoomIn,
    ZoomOut,
    /// A board waiting on a saw, or a part on the bench: a quarter turn each way, over, or up
    /// onto an edge or an end.
    TurnLeft,
    TurnRight,
    Flip,
    StandUp,
    /// At a saw: choosing what the board rests on, and what goes against the fence, by
    /// clicking them.
    Rest,
    /// The table saw's guide: the rip fence or the miter gauge, and moving it.
    SwitchGuide,
    Guide(Step),
    Blade(Step),
    /// Tilting the table saw's blade further from square, or back toward it.
    Tilt(Step),
    /// Feeding the board into the table saw's blade, held, and bringing it back around.
    Push,
    BringBack,
    /// The miter saw's stop, its angle, its depth stop, and lowering its blade, held.
    Stop(Step),
    Miter(Step),
    Depth(Step),
    Lower,
    /// At the bench: which fastener, sliding a part across the joint, fastening it, letting go,
    /// taking the last joint apart, picking a part up, and drawing up plans.
    Use(Fastener),
    Slide(Arrow, Fine),
    Fasten,
    LetGo,
    TakeApart,
    PickUp,
    Plans,
    /// At the bench, take a glue-up into the photo studio to render it.
    Studio,
    /// In the studio: pick the `n`th light (key, fill, rim) to move; swing it around the
    /// piece, raise or lower it, or brighten or dim it; tip the camera up or down; a longer or
    /// shorter lens; the next backdrop; more or less time over the render; render it (or, while
    /// it renders, finish it now and save it); and stop the render.
    Light(usize),
    Swing(Step),
    Raise(Step),
    Brighten(Step),
    Tip(Step),
    Lens(Step),
    Backdrop(Step),
    Quality(Step),
    Render,
    StopRender,
    /// In the studio, render on the CPU from now on, or on the GPU.
    Engine,
    /// In the studio, render with everything sharp from now on, or with what's far behind the
    /// piece soft.
    DepthOfField,
    /// At a saw: put away what's on it, or put the measured piece away and the offcut back to
    /// cut again. At the bench, put away the picked part or glue-up.
    Store,
    CutAgain,
    /// Put away what's in hand.
    PutAway,
    /// Copy what's in hand, or at the station you're at, for Claude.
    Copy,
    /// From the inventory: use a piece, or a new board of the `n`th size of lumber, at the
    /// station you're at; move a piece to the other shelf; or throw it away.
    Take(u64),
    NewLumber(u64),
    MoveShelf(u64),
    Scrap(u64),
    /// Save a piece from the inventory as a file, use the `n`th part saved, as new lumber is,
    /// or delete its file.
    Save(u64),
    Load(u64),
    DeleteSaved(u64),
    /// In the inventory's tree: pick out a line, fold a heading away or open a glue-up's
    /// boards, duplicate a piece wherever it is, or put away a piece that's on a station.
    Select(Entry),
    Fold(Fold),
    Duplicate(Entry),
    Stash(Where),
    /// Changing how wood was made: open a pass from the tree to change it, step one of its
    /// settings, make every piece it cut again that way, or leave it as it was; and take a
    /// joint apart. At the bench, list how the picked part was made, or take it off the
    /// glue-up it's in.
    Edit(Entry),
    Tweak(Knob, Step),
    Remake,
    StopEditing,
    Unglue(Entry),
    History,
    Loosen,
    /// The list of every control here, and the inventory.
    Help,
    Inventory,
}

/// A way on the screen, as the arrow keys point.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Arrow {
    Left,
    Right,
    Up,
    Down,
}

/// Which way a setting moves, the way an arrow key points (right or up is `more`), and whether
/// a big step or a small one.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Step {
    pub more: bool,
    pub big: bool,
}

/// Whether a part slides across a joint 1/16" at a time, rather than to the next place it lines
/// up.
pub type Fine = bool;

impl Action {
    /// Whether it goes on for as long as its key or button is held, rather than happening once.
    pub fn is_held(self) -> bool {
        matches!(self, Action::Pan(_) | Action::Push | Action::Lower)
    }
}

/// What `key` does at `view`, with Shift held or not.
pub fn bound(view: ShopView, key: KeyCode, shift: bool) -> Option<Action> {
    use Action::*;
    use KeyCode::*;
    use ShopView::{Bench, MiterSaw, Overview, Studio, TableSaw};
    let step = |more| Step { more, big: shift };
    let arrow = match key {
        ArrowLeft => Some(Arrow::Left),
        ArrowRight => Some(Arrow::Right),
        ArrowUp => Some(Arrow::Up),
        ArrowDown => Some(Arrow::Down),
        _ => None,
    };
    Some(match (view, key) {
        (_, Tab) => NextStation,
        (TableSaw | MiterSaw | Bench | Studio, Escape) => Back,
        (_, KeyH) => Action::Help,
        (_, KeyI) => Inventory,
        (_, KeyX) => PutAway,
        (_, KeyY) => Action::Copy,
        (TableSaw | MiterSaw | Bench, KeyC) => Store,
        (TableSaw | MiterSaw, KeyA) => CutAgain,
        (Overview, KeyW | ArrowUp) => Pan(Arrow::Up),
        (Overview, KeyS | ArrowDown) => Pan(Arrow::Down),
        (Overview, KeyA | ArrowLeft) => Pan(Arrow::Left),
        (Overview, KeyD | ArrowRight) => Pan(Arrow::Right),
        (Overview, KeyQ) => RotateLeft,
        (Overview, KeyE) => RotateRight,
        (Overview, Equal | NumpadAdd) => ZoomIn,
        (Overview, Minus | NumpadSubtract) => ZoomOut,
        (Studio, Equal | NumpadAdd) => ZoomIn,
        (Studio, Minus | NumpadSubtract) => ZoomOut,
        (Studio, Digit1) => Light(0),
        (Studio, Digit2) => Light(1),
        (Studio, Digit3) => Light(2),
        (Studio, ArrowLeft | ArrowRight) => Swing(step(key == ArrowRight)),
        (Studio, ArrowUp | ArrowDown) => Raise(step(key == ArrowUp)),
        (Studio, BracketLeft | BracketRight) => Brighten(step(key == BracketRight)),
        (Studio, KeyW | KeyS) => Tip(step(key == KeyW)),
        (Studio, Comma | Period) => Lens(step(key == Period)),
        (Studio, KeyB) => Backdrop(Step {
            more: !shift,
            big: false,
        }),
        (Studio, KeyN | KeyM) => Quality(Step {
            more: key == KeyM,
            big: false,
        }),
        (Studio, Enter | NumpadEnter) => Render,
        (Studio, Backspace | Delete) => StopRender,
        (Studio, KeyG) => Engine,
        (Studio, KeyD) => DepthOfField,
        (_, KeyQ) => TurnLeft,
        (_, KeyE) => TurnRight,
        (_, KeyF) => Flip,
        (TableSaw | MiterSaw, KeyV) => StandUp,
        (TableSaw | MiterSaw, KeyR) => Rest,
        (TableSaw, KeyG) => SwitchGuide,
        (TableSaw, ArrowLeft | ArrowRight) => Guide(step(key == ArrowRight)),
        (TableSaw, ArrowUp | ArrowDown) => Blade(step(key == ArrowUp)),
        (TableSaw, Comma | Period) => Tilt(step(key == Comma)),
        (TableSaw, Space) => Push,
        (TableSaw, KeyB) => BringBack,
        (MiterSaw, ArrowLeft | ArrowRight) => Stop(step(key == ArrowRight)),
        (MiterSaw, ArrowUp | ArrowDown) => Depth(step(key == ArrowUp)),
        (TableSaw | MiterSaw, BracketLeft | BracketRight) => Miter(step(key == BracketRight)),
        (MiterSaw, Space) => Lower,
        (Bench, Digit1) => Use(Fastener::Glue),
        (Bench, Digit2) => Use(Fastener::Brads),
        (Bench, Digit3) => Use(Fastener::Screws),
        (Bench, ArrowLeft | ArrowRight | ArrowUp | ArrowDown) => Slide(arrow?, shift),
        (Bench, Enter | NumpadEnter) => Fasten,
        (Bench, Backspace | Delete) => LetGo,
        (Bench, KeyU) => TakeApart,
        (Bench, Space) => PickUp,
        (Bench, KeyP) => Plans,
        (Bench, KeyM) => History,
        (Bench, KeyL) => Loosen,
        (Bench, KeyR) => Action::Studio,
        _ => return None,
    })
}

/// Every key some action is bound to, in the order they're named on buttons.
const KEYS: [KeyCode; 40] = [
    KeyCode::KeyQ,
    KeyCode::KeyE,
    KeyCode::KeyF,
    KeyCode::KeyV,
    KeyCode::KeyR,
    KeyCode::KeyY,
    KeyCode::KeyG,
    KeyCode::KeyB,
    KeyCode::KeyU,
    KeyCode::KeyP,
    KeyCode::KeyN,
    KeyCode::KeyM,
    KeyCode::KeyL,
    KeyCode::KeyH,
    KeyCode::KeyI,
    KeyCode::KeyX,
    KeyCode::KeyC,
    KeyCode::KeyW,
    KeyCode::KeyA,
    KeyCode::KeyS,
    KeyCode::KeyD,
    KeyCode::Digit1,
    KeyCode::Digit2,
    KeyCode::Digit3,
    KeyCode::Space,
    KeyCode::Enter,
    KeyCode::Backspace,
    KeyCode::Escape,
    KeyCode::Tab,
    KeyCode::ArrowLeft,
    KeyCode::ArrowRight,
    KeyCode::ArrowUp,
    KeyCode::ArrowDown,
    KeyCode::BracketLeft,
    KeyCode::BracketRight,
    KeyCode::Comma,
    KeyCode::Period,
    KeyCode::Equal,
    KeyCode::Minus,
    KeyCode::NumpadEnter,
];

/// The key that does `action` at `view`, in words for a tooltip ("Shift+Right arrow"), and
/// short for the badge on its button ("Q"), if it's short enough to fit.
pub fn key_for(view: ShopView, action: Action) -> Option<(String, Option<&'static str>)> {
    for shift in [false, true] {
        for key in KEYS {
            if bound(view, key, shift) != Some(action) {
                continue;
            }
            let (words, badge) = key_name(key);
            let words = if shift && needs_shift(view, key) {
                format!("Shift+{words}")
            } else {
                words.to_string()
            };
            return Some((words, if shift { None } else { badge }));
        }
    }
    None
}

/// Whether Shift changes what `key` does at `view`.
fn needs_shift(view: ShopView, key: KeyCode) -> bool {
    bound(view, key, false) != bound(view, key, true)
}

/// A key's name, and its badge if it has a short one.
fn key_name(key: KeyCode) -> (&'static str, Option<&'static str>) {
    match key {
        KeyCode::KeyQ => ("Q", Some("Q")),
        KeyCode::KeyE => ("E", Some("E")),
        KeyCode::KeyF => ("F", Some("F")),
        KeyCode::KeyV => ("V", Some("V")),
        KeyCode::KeyR => ("R", Some("R")),
        KeyCode::KeyY => ("Y", Some("Y")),
        KeyCode::KeyG => ("G", Some("G")),
        KeyCode::KeyB => ("B", Some("B")),
        KeyCode::KeyU => ("U", Some("U")),
        KeyCode::KeyP => ("P", Some("P")),
        KeyCode::KeyN => ("N", Some("N")),
        KeyCode::KeyM => ("M", Some("M")),
        KeyCode::KeyL => ("L", Some("L")),
        KeyCode::KeyH => ("H", Some("H")),
        KeyCode::KeyI => ("I", Some("I")),
        KeyCode::KeyX => ("X", Some("X")),
        KeyCode::KeyC => ("C", Some("C")),
        KeyCode::KeyW => ("W", Some("W")),
        KeyCode::KeyA => ("A", Some("A")),
        KeyCode::KeyS => ("S", Some("S")),
        KeyCode::KeyD => ("D", Some("D")),
        KeyCode::Digit1 => ("1", Some("1")),
        KeyCode::Digit2 => ("2", Some("2")),
        KeyCode::Digit3 => ("3", Some("3")),
        KeyCode::Space => ("Space", Some("Space")),
        KeyCode::Enter | KeyCode::NumpadEnter => ("Enter", Some("Enter")),
        KeyCode::Backspace => ("Backspace", Some("Bksp")),
        KeyCode::Escape => ("Esc", Some("Esc")),
        KeyCode::Tab => ("Tab", Some("Tab")),
        KeyCode::ArrowLeft => ("Left arrow", None),
        KeyCode::ArrowRight => ("Right arrow", None),
        KeyCode::ArrowUp => ("Up arrow", None),
        KeyCode::ArrowDown => ("Down arrow", None),
        KeyCode::BracketLeft => ("[", Some("[")),
        KeyCode::BracketRight => ("]", Some("]")),
        KeyCode::Comma => (",", Some(",")),
        KeyCode::Period => (".", Some(".")),
        KeyCode::Equal => ("+", Some("+")),
        KeyCode::Minus => ("-", Some("-")),
        _ => ("?", None),
    }
}

/// What the player asked for this frame: actions just started, from a key or a button, and
/// those still held down.
#[derive(Resource, Default, Debug)]
pub struct Actions {
    started: Vec<Action>,
    held: Vec<Action>,
}

impl Actions {
    /// Whether `action` started this frame.
    pub fn just(&self, action: Action) -> bool {
        self.started.contains(&action)
    }

    /// Whether `action` is going on: its key or button is down.
    pub fn held(&self, action: Action) -> bool {
        self.held.contains(&action) || self.started.contains(&action)
    }

    /// Every action that started this frame, in order.
    pub fn started(&self) -> impl Iterator<Item = Action> + '_ {
        self.started.iter().copied()
    }

    /// Starts `action`, as a click on its button does.
    pub fn start(&mut self, action: Action) {
        if !self.started.contains(&action) {
            self.started.push(action);
        }
    }

    /// Keeps `action` going this frame, as holding its button down does.
    pub fn hold(&mut self, action: Action) {
        if !self.held.contains(&action) {
            self.held.push(action);
        }
    }
}

pub struct ControlsPlugin;

impl Plugin for ControlsPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Actions>().add_systems(
            PreUpdate,
            // Before the buttons hear about clicks, which start their actions.
            read_keys
                .after(InputSystems)
                .before(PickingSystems::ProcessInput),
        );
    }
}

/// Starts this frame's actions afresh from the keys pressed, and those held.
fn read_keys(keys: Res<ButtonInput<KeyCode>>, view: Res<ShopView>, mut actions: ResMut<Actions>) {
    actions.started.clear();
    actions.held.clear();
    let shift = keys.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight]);
    for &key in keys.get_just_pressed() {
        if let Some(action) = bound(*view, key, shift) {
            actions.start(action);
        }
    }
    for &key in keys.get_pressed() {
        if let Some(action) = bound(*view, key, shift)
            && action.is_held()
        {
            actions.hold(action);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_same_key_does_what_fits_where_you_are() {
        assert_eq!(
            bound(ShopView::Overview, KeyCode::KeyQ, false),
            Some(Action::RotateLeft)
        );
        assert_eq!(
            bound(ShopView::TableSaw, KeyCode::KeyQ, false),
            Some(Action::TurnLeft)
        );
        assert_eq!(
            bound(ShopView::TableSaw, KeyCode::ArrowRight, true),
            Some(Action::Guide(Step {
                more: true,
                big: true
            }))
        );
        assert_eq!(
            bound(ShopView::Bench, KeyCode::ArrowUp, true),
            Some(Action::Slide(Arrow::Up, true))
        );
        assert_eq!(bound(ShopView::Overview, KeyCode::Escape, false), None);
        assert_eq!(bound(ShopView::Bench, KeyCode::KeyV, false), None);
        assert_eq!(
            bound(ShopView::Studio, KeyCode::ArrowLeft, true),
            Some(Action::Swing(Step {
                more: false,
                big: true
            }))
        );
        assert_eq!(
            bound(ShopView::Studio, KeyCode::Escape, false),
            Some(Action::Back)
        );
        assert_eq!(
            bound(ShopView::Bench, KeyCode::KeyR, false),
            Some(Action::Studio)
        );
    }

    #[test]
    fn buttons_name_their_keys() {
        assert_eq!(
            key_for(ShopView::Bench, Action::Fasten),
            Some(("Enter".to_string(), Some("Enter")))
        );
        assert_eq!(
            key_for(ShopView::MiterSaw, Action::Lower),
            Some(("Space".to_string(), Some("Space")))
        );
        assert_eq!(
            key_for(
                ShopView::TableSaw,
                Action::Guide(Step {
                    more: false,
                    big: true
                })
            ),
            Some(("Shift+Left arrow".to_string(), None))
        );
        assert_eq!(
            key_for(ShopView::Overview, Action::GoTo(ShopView::Bench)),
            None
        );
    }
}
