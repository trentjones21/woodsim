//! Copying wood out of the game, so a shape that misbehaves can be handed over exactly. The
//! `Copy` action writes what's in hand, or what's at the station you're at (on a saw, as it was
//! before the pass, with the saw set as it was for it and how the wood lies; at the bench, the
//! picked part's glue-up), as a `woodsim::Recipe`, with what it is and what's going on in
//! comments on top. It goes on the clipboard and is saved as the next free
//! `pieces/piece-N.txt`, and the status says so for a few seconds (`Notice`).

use std::{
    fs, io,
    io::Write,
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

use bevy::{ecs::system::SystemParam, prelude::*};
use woodsim::{Recipe, Wood};

use crate::{
    chop::MiterSawStation,
    controls::{Action, Actions},
    feed::TableSawStation,
    hand::Hand,
    join::BenchStation,
    shop::ShopView,
    work,
};

pub struct CopyPlugin;

impl Plugin for CopyPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Notice>()
            .add_systems(Update, (copy_wood, end_notice));
    }
}

/// A line for the status, in place of the station's, until a time (in seconds since the game
/// started).
#[derive(Resource, Default)]
pub struct Notice(pub Option<(String, f64)>);

/// How long a notice shows, in seconds.
const NOTICE_SECONDS: f64 = 6.0;

/// What there is to copy, if anything: what's in hand, or at the station you're at. What it
/// is, in words, and its recipe.
pub fn to_copy(
    view: ShopView,
    hand: &Hand,
    table_saw: &TableSawStation,
    miter_saw: &MiterSawStation,
    bench: &BenchStation,
) -> Option<(String, Recipe)> {
    if let Some(work) = &hand.0 {
        let recipe = Recipe {
            wood: work::wood(work),
            saw: None,
        };
        return Some((format!("in hand, {}", work::describe(work)), recipe));
    }
    match view {
        ShopView::TableSaw => Some((
            format!("on the table saw. {}", table_saw.status()),
            table_saw.recipe()?,
        )),
        ShopView::MiterSaw => Some((
            format!("on the miter saw. {}", miter_saw.status()),
            miter_saw.recipe()?,
        )),
        ShopView::Bench => {
            let group = match bench.picked {
                Some(spot) => spot.group,
                None if bench.groups.len() == 1 => 0,
                None => return None,
            };
            let assembly = bench.groups.get(group)?.assembly.clone();
            Some((
                "at the bench, the picked part's glue-up, as it's set up (not as it lies on \
                 the bench)"
                    .to_string(),
                Recipe {
                    wood: Wood::GlueUp(assembly),
                    saw: None,
                },
            ))
        }
        ShopView::Overview | ShopView::Studio => None,
    }
}

/// Where there might be wood to copy: in hand, or at the station you're at.
#[derive(SystemParam)]
struct Copyable<'w> {
    view: Res<'w, ShopView>,
    hand: Res<'w, Hand>,
    table_saw: Res<'w, TableSawStation>,
    miter_saw: Res<'w, MiterSawStation>,
    bench: Res<'w, BenchStation>,
}

fn copy_wood(
    actions: Res<Actions>,
    copyable: Copyable,
    time: Res<Time>,
    mut notice: ResMut<Notice>,
) {
    if !actions.just(Action::Copy) {
        return;
    }
    let found = to_copy(
        *copyable.view,
        &copyable.hand,
        &copyable.table_saw,
        &copyable.miter_saw,
        &copyable.bench,
    );
    let message = match found {
        None => "Nothing to copy here. Hold a piece, or copy it at a station.".to_string(),
        Some((about, recipe)) => {
            let text = format!(
                "# Woodshop Simulator, copied: {about}\n\
                 # woodsim::Recipe::read reads the rest back exactly.\n{}",
                recipe.write()
            );
            match (to_clipboard(&text), save(&text, Path::new("pieces"))) {
                (Ok(()), Ok(path)) => format!("Copied, and saved as {}.", path.display()),
                (Err(_), Ok(path)) => format!(
                    "Couldn't reach the clipboard, so it's saved as {}.",
                    path.display()
                ),
                (Ok(()), Err(_)) => "Copied.".to_string(),
                (Err(why), Err(_)) => format!("Couldn't copy it: {why}."),
            }
        }
    };
    notice.0 = Some((message, time.elapsed_secs_f64() + NOTICE_SECONDS));
}

fn end_notice(time: Res<Time>, mut notice: ResMut<Notice>) {
    if notice
        .0
        .as_ref()
        .is_some_and(|(_, until)| time.elapsed_secs_f64() > *until)
    {
        notice.0 = None;
    }
}

/// Puts `text` on the clipboard, with whatever the system has to do it.
fn to_clipboard(text: &str) -> Result<(), String> {
    let tools: &[(&str, &[&str])] = if cfg!(target_os = "macos") {
        &[("pbcopy", &[])]
    } else if cfg!(target_os = "windows") {
        &[("clip", &[])]
    } else {
        &[
            ("wl-copy", &[]),
            ("xclip", &["-selection", "clipboard"]),
            ("xsel", &["--clipboard", "--input"]),
        ]
    };
    for (program, args) in tools {
        let Ok(mut child) = Command::new(program)
            .args(*args)
            .stdin(Stdio::piped())
            .spawn()
        else {
            continue;
        };
        if let Some(mut input) = child.stdin.take() {
            let _ = input.write_all(text.as_bytes());
        }
        if child.wait().is_ok_and(|status| status.success()) {
            return Ok(());
        }
    }
    Err("no clipboard tool".to_string())
}

/// Saves `text` as the next unused `piece-N.txt` in `folder`, making the folder if it isn't
/// there, and returns where.
fn save(text: &str, folder: &Path) -> io::Result<PathBuf> {
    fs::create_dir_all(folder)?;
    let path = (1..)
        .map(|n| folder.join(format!("piece-{n}.txt")))
        .find(|path| !path.exists())
        .expect("a free name");
    fs::write(&path, text)?;
    Ok(path)
}

#[cfg(test)]
mod tests {
    use woodsim::{Board, Length};

    use super::*;
    use crate::hand::Workpiece;

    #[test]
    fn whats_on_a_saw_is_copied_with_how_the_saw_is_set_and_reads_back() {
        let mut table_saw = TableSawStation::default();
        table_saw.load(Workpiece::Board(Board::new(
            Length::from_inches(24),
            Length::from_inches(3),
            Length::from_inch_fraction(3, 4),
        )));
        table_saw.push(0.2);
        let (about, recipe) = to_copy(
            ShopView::TableSaw,
            &Hand::default(),
            &table_saw,
            &MiterSawStation::default(),
            &BenchStation::default(),
        )
        .expect("a board on the saw");
        assert!(about.starts_with("on the table saw."), "{about}");
        let read = Recipe::read(&recipe.write()).unwrap();
        assert_eq!(read, recipe);
        // The saw as it's set, and the pass as far as it's gone.
        let Some(woodsim::Tool::TableSaw { saw, travel, .. }) = read.saw else {
            panic!("expected the table saw");
        };
        assert_eq!(saw, table_saw.saw);
        assert!(travel.1 > travel.0);
    }

    #[test]
    fn whats_in_hand_is_copied_wherever_you_are() {
        let board = Board::new(
            Length::from_inches(24),
            Length::from_inches(3),
            Length::from_inch_fraction(3, 4),
        );
        let hand = Hand(Some(Workpiece::Board(board.clone())));
        let (about, recipe) = to_copy(
            ShopView::Overview,
            &hand,
            &TableSawStation::default(),
            &MiterSawStation::default(),
            &BenchStation::default(),
        )
        .expect("what's in hand");
        assert!(about.starts_with("in hand"), "{about}");
        assert_eq!(recipe.wood, Wood::Board(board));
        assert!(
            to_copy(
                ShopView::Overview,
                &Hand::default(),
                &TableSawStation::default(),
                &MiterSawStation::default(),
                &BenchStation::default(),
            )
            .is_none()
        );
    }
}
