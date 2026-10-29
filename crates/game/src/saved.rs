//! Parts saved as files, to use again after the game starts over. A part on the inventory's
//! shelves is saved (`Action::Save`) as the next free `saved/part-N.txt`: a `woodsim::Recipe`,
//! exact to the nanometer, with its whole history, so its plans come out as they would have. The
//! parts in `saved/` are read when the game starts and again after each save (`Saved`), and
//! listed in the inventory, where each is used (`Action::Load`) the way new lumber is: a copy of
//! it goes where you are, and the file stays to use again.

use std::{
    fs, io,
    path::{Path, PathBuf},
};

use bevy::prelude::*;
use woodsim::{Recipe, Wood};

use crate::{
    controls::{Action, Actions},
    copy::Notice,
    hand::Workpiece,
    inventory::{self, Inventory},
    work,
};

/// Where parts are saved, from where the game is run.
pub const FOLDER: &str = "saved";

/// How long a notice shows, in seconds.
const NOTICE_SECONDS: f64 = 6.0;

pub struct SavedPlugin;

impl Plugin for SavedPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(Saved::read(Path::new(FOLDER)))
            .add_systems(Update, save_parts);
    }
}

/// A part saved as a file.
#[derive(Clone, Debug, PartialEq)]
pub struct SavedPart {
    pub path: PathBuf,
    pub work: Workpiece,
}

/// The parts saved in a folder, in the order they were saved.
#[derive(Resource, Default, Debug)]
pub struct Saved {
    pub parts: Vec<SavedPart>,
    /// Files that are there but didn't read as parts, and why.
    pub unread: Vec<(PathBuf, String)>,
}

impl Saved {
    /// Every part saved in `folder`: none, if there's no folder yet.
    pub fn read(folder: &Path) -> Saved {
        let mut saved = Saved::default();
        let Ok(entries) = fs::read_dir(folder) else {
            return saved;
        };
        let mut paths: Vec<PathBuf> = entries
            .filter_map(|entry| Some(entry.ok()?.path()))
            .filter(|path| path.extension().is_some_and(|extension| extension == "txt"))
            .collect();
        // part-2 before part-10.
        paths.sort_by_key(|path| (path.as_os_str().len(), path.clone()));
        for path in paths {
            match read_part(&path) {
                Ok(work) => saved.parts.push(SavedPart { path, work }),
                Err(why) => saved.unread.push((path, why)),
            }
        }
        saved
    }

    /// A copy of the `index`th saved part, to use.
    pub fn part(&self, index: usize) -> Option<Workpiece> {
        Some(self.parts.get(index)?.work.clone())
    }
}

/// The part saved at `path`.
fn read_part(path: &Path) -> Result<Workpiece, String> {
    let text = fs::read_to_string(path).map_err(|why| why.to_string())?;
    Ok(match Recipe::read(&text)?.wood {
        Wood::Board(board) => Workpiece::Board(board),
        Wood::GlueUp(glue_up) => Workpiece::GlueUp(glue_up),
    })
}

/// Saves `work` as the next unused `part-N.txt` in `folder`, making the folder if it isn't
/// there, and returns where.
pub fn save(work: &Workpiece, folder: &Path) -> io::Result<PathBuf> {
    fs::create_dir_all(folder)?;
    let path = (1..)
        .map(|n| folder.join(format!("part-{n}.txt")))
        .find(|path| !path.exists())
        .expect("a free name");
    let (size, notes) = inventory::describe(work);
    let about = if notes.is_empty() {
        size
    } else {
        format!("{size}, {notes}")
    };
    let recipe = Recipe {
        wood: work::wood(work),
        saw: None,
    };
    let text = format!(
        "# Woodshop Simulator part: {about}\n\
         # woodsim::Recipe::read reads the rest back exactly.\n{}",
        recipe.write()
    );
    fs::write(&path, text)?;
    Ok(path)
}

/// Saves the parts asked for, and reads the folder again so they're listed.
fn save_parts(
    actions: Res<Actions>,
    inventory: Res<Inventory>,
    mut saved: ResMut<Saved>,
    time: Res<Time>,
    mut notice: ResMut<Notice>,
) {
    for action in actions.started() {
        let Action::Save(id) = action else {
            continue;
        };
        let Some(item) = inventory.get(id) else {
            continue;
        };
        let message = match save(&item.work, Path::new(FOLDER)) {
            Ok(path) => {
                *saved = Saved::read(Path::new(FOLDER));
                format!(
                    "Saved as {}. It's in the inventory under Saved, after a restart too.",
                    path.display()
                )
            }
            Err(why) => format!("Couldn't save it: {why}."),
        };
        notice.0 = Some((message, time.elapsed_secs_f64() + NOTICE_SECONDS));
    }
}

#[cfg(test)]
mod tests {
    use woodsim::{Blade, Board, Length, Orientation, Pass, TableSaw};

    use super::*;

    /// A folder of its own for each test, emptied first.
    fn folder(name: &str) -> PathBuf {
        let folder = std::env::temp_dir().join(format!("woodshop-saved-{name}"));
        let _ = fs::remove_dir_all(&folder);
        folder
    }

    #[test]
    fn a_saved_part_reads_back_the_same_with_its_history() {
        let folder = folder("history");
        let saw = TableSaw {
            blade: Blade {
                diameter: Length::from_inches(10),
                kerf: Blade::FULL_KERF,
            },
            fence: Length::from_inches(2),
            height: Length::from_inches(1),
            miter_gauge: None,
            gauge_angle: woodsim::Angle::ZERO,
            tilt: woodsim::Angle::from_degrees(15),
        };
        let stock = Board::new(
            Length::from_inches(24),
            Length::from_inch_fraction(7, 2),
            Length::from_inch_fraction(3, 4),
        );
        let Ok(Pass::Cut(cut)) = saw.rip(&stock, Orientation::default()) else {
            panic!("expected a beveled rip");
        };
        let part = Workpiece::Board(cut.piece.board);
        let first = save(&part, &folder).unwrap();
        let second = save(&part, &folder).unwrap();
        assert_eq!(first.file_name().unwrap(), "part-1.txt");
        assert_eq!(second.file_name().unwrap(), "part-2.txt");
        let saved = Saved::read(&folder);
        assert!(saved.unread.is_empty(), "{:?}", saved.unread);
        assert_eq!(saved.parts.len(), 2);
        let Some(Workpiece::Board(read)) = saved.part(0) else {
            panic!("a board");
        };
        let Workpiece::Board(board) = &part else {
            unreachable!()
        };
        assert_eq!(read, *board);
        assert_eq!(read.origin().operations().len(), 1);
        assert_eq!(
            read.origin().operations()[0].tool,
            board.origin().operations()[0].tool
        );
    }

    #[test]
    fn no_folder_is_nothing_saved_and_a_bad_file_is_set_aside() {
        let folder = folder("bad");
        assert!(Saved::read(&folder).parts.is_empty());
        fs::create_dir_all(&folder).unwrap();
        fs::write(folder.join("part-1.txt"), "not a part").unwrap();
        fs::write(folder.join("notes.md"), "not a part either, and not read").unwrap();
        let saved = Saved::read(&folder);
        assert!(saved.parts.is_empty());
        assert_eq!(saved.unread.len(), 1);
    }
}
