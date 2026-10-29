//! Wood written out as a recipe and read back is the same wood, with the same history, to the
//! nanometer: writing it again gives the same text.

mod common;

use common::CutApart;

use proptest::prelude::*;
use woodsim::{
    Angle, Assembly, Blade, Board, Fastener, GlueUpPass, Length, Lie, Mating, MiterSaw,
    Orientation, Origin, Recipe, Rotation, Source, Surface, TableSaw, Tool, Wood,
};

fn inches(n: i64) -> Length {
    Length::from_inches(n)
}

fn fraction(numerator: i64, denominator: i64) -> Length {
    Length::from_inch_fraction(numerator, denominator)
}

fn table_saw(fence: Length, tilt: i32) -> TableSaw {
    TableSaw {
        blade: Blade {
            diameter: inches(10),
            kerf: Blade::FULL_KERF,
        },
        fence,
        height: inches(3),
        miter_gauge: None,
        gauge_angle: Angle::ZERO,
        tilt: Angle::from_hundredths(tilt),
    }
}

/// Whether two histories are the same, step for step.
fn same_history(a: &Origin, b: &Origin) -> bool {
    match (a.source(), b.source()) {
        (
            Source::Stock {
                length,
                width,
                thickness,
            },
            Source::Stock {
                length: l,
                width: w,
                thickness: t,
            },
        ) => (length, width, thickness) == (l, w, t),
        (
            Source::Sawn {
                operation,
                from,
                kerf,
                part,
                at,
                side,
            },
            Source::Sawn {
                operation: o,
                from: f,
                kerf: k,
                part: p,
                at: a,
                side: s,
            },
        ) => operation == o && (kerf, part, at, side) == (k, p, a, s) && same_history(from, f),
        _ => false,
    }
}

/// Reads `recipe` back from its text, checks it's the same, and returns what it read.
fn round_trip(recipe: &Recipe) -> Recipe {
    let text = recipe.write();
    // Each board and pass is named once.
    let mut names: Vec<&str> = text
        .lines()
        .filter(|line| {
            ["stock", "sawn", "pass"]
                .iter()
                .any(|kind| line.starts_with(kind))
        })
        .map(|line| line.split_whitespace().nth(1).unwrap())
        .collect();
    let count = names.len();
    names.sort();
    names.dedup();
    assert_eq!(names.len(), count, "{text}");
    let read = Recipe::read(&text).unwrap_or_else(|why| panic!("{why}\n{text}"));
    assert_eq!(read, *recipe);
    assert_eq!(read.write(), text);
    read
}

#[test]
fn a_board_with_a_long_history_reads_back_the_same() {
    // A 30° bevel ripped along a 1x6, the piece rested on its bevel and ripped again, then
    // mitered at the miter saw, and the end kerfed standing on end.
    let stock = Board::new(inches(24), fraction(11, 2), fraction(3, 4));
    let beveled = table_saw(inches(3), 3000)
        .rip(&stock, Orientation::default())
        .cut_apart()
        .piece
        .board;
    let bevel = Surface::Side(beveled.bevels()[0].0);
    let lie = Assembly::of(beveled.clone())
        .resting((0, bevel), (0, Surface::Bottom))
        .unwrap();
    let ripped = table_saw(inches(1), 0)
        .rip(&beveled, lie)
        .cut_apart()
        .offcut
        .expect("the other side")
        .board;
    let mitered = MiterSaw {
        miter: Angle::from_hundredths(2250),
        ..MiterSaw::twelve_inch()
    }
    .crosscut(&ripped, Orientation::default(), inches(10))
    .cut_apart()
    .piece
    .board;
    let cheek = match table_saw(fraction(1, 4), 0).rip(&mitered, Orientation::default().stood()) {
        Ok(woodsim::Pass::Kerfed(board)) => board,
        other => panic!("expected a cheek kerf, got {other:?}"),
    };
    let read = round_trip(&Recipe {
        wood: Wood::Board(cheek.clone()),
        saw: None,
    });
    let Wood::Board(board) = read.wood else {
        panic!("a board reads back as a board");
    };
    assert!(same_history(board.origin(), cheek.origin()));
    assert_eq!(board.origin().replay(), cheek);
}

#[test]
fn a_glue_up_reads_back_with_its_boards_sharing_their_cuts() {
    // A 1x2 glued onto a 1x6's bevel, then crosscut: both boards' pieces share the pass.
    let stock = Board::new(inches(24), fraction(11, 2), fraction(3, 4));
    let beveled = table_saw(inches(4), 3000)
        .rip(&stock, Orientation::default())
        .cut_apart()
        .piece
        .board;
    let bevel = Surface::Side(beveled.bevels()[0].0);
    let mut glue_up = Assembly::of(beveled);
    assert!(glue_up.attach(
        (0, bevel),
        &Assembly::of(Board::new(inches(24), fraction(3, 2), fraction(3, 4))),
        (0, Surface::Bottom),
        Mating::default(),
        Fastener::Glue,
    ));
    let Ok(GlueUpPass::Cut(cut)) =
        MiterSaw::twelve_inch().crosscut_glue_up(&glue_up, Rotation::NONE, inches(10))
    else {
        panic!("expected the glue-up crosscut");
    };
    let text = Recipe {
        wood: Wood::GlueUp(cut.piece.clone()),
        saw: None,
    }
    .write();
    // The crosscut is written once, for both boards.
    assert_eq!(
        text.lines().filter(|line| line.starts_with("pass")).count(),
        2,
        "{text}"
    );
    let read = round_trip(&Recipe {
        wood: Wood::GlueUp(cut.piece.clone()),
        saw: None,
    });
    let Wood::GlueUp(read) = read.wood else {
        panic!("a glue-up reads back as a glue-up");
    };
    assert_eq!(read.joints().len(), 1);
    assert_eq!(read.joints()[0].number, cut.piece.joints()[0].number);
    for (member, was) in read.members().iter().zip(cut.piece.members()) {
        assert!(same_history(member.board.origin(), was.board.origin()));
    }
    let [a, b] = [0, 1].map(|k| read.members()[k].board.origin());
    let pass = |origin: &Origin| match origin.source() {
        Source::Sawn { operation, .. } => operation.clone(),
        Source::Stock { .. } => panic!("both were cut"),
    };
    assert!(std::sync::Arc::ptr_eq(&pass(a), &pass(b)));
}

#[test]
fn how_the_saw_is_set_reads_back_too() {
    let saw = TableSaw {
        miter_gauge: Some(inches(12)),
        gauge_angle: Angle::from_hundredths(-1500),
        ..table_saw(inches(2), 4500)
    };
    let tool = Tool::TableSaw {
        saw,
        lie: Lie::Board(Orientation::default().tipped().turned_left()),
        travel: (-inches(5), fraction(3, 2)),
    };
    let read = round_trip(&Recipe {
        wood: Wood::Board(Board::new(inches(24), inches(3), fraction(3, 4))),
        saw: Some(tool),
    });
    assert_eq!(read.saw, Some(tool));
}

#[test]
fn a_recipe_that_doesnt_read_says_where() {
    let why = Recipe::read("stock s0 1 2 3\nsawn s1 s0 p0 1 2 3\nboard s1").unwrap_err();
    assert!(why.starts_with("line 2: no pass `p0` before this"), "{why}");
    let why = Recipe::read("# nothing but a comment").unwrap_err();
    assert!(why.contains("expected one `board` line"), "{why}");
}

#[test]
fn a_recipe_says_which_version_it_is_and_reads_without_one() {
    let board = Recipe {
        wood: Wood::Board(Board::new(inches(24), inches(3), fraction(3, 4))),
        saw: None,
    };
    let text = board.write();
    assert_eq!(text.lines().next(), Some("version 1"), "{text}");
    // Saved before there was a version line, it reads as version 1.
    let unversioned: String = text
        .lines()
        .skip(1)
        .map(|line| format!("{line}\n"))
        .collect();
    assert_eq!(Recipe::read(&unversioned), Ok(board));
    // Comments can come before it.
    assert!(Recipe::read(&format!("# a part\n{text}")).is_ok());

    let newer = text.replacen("version 1", "version 2", 1);
    let why = Recipe::read(&newer).unwrap_err();
    assert!(
        why.starts_with("line 1: it's version 2, from a newer woodsim"),
        "{why}"
    );
    let why = Recipe::read(&format!("{unversioned}version 1\n")).unwrap_err();
    assert!(
        why.contains("`version` comes before everything else"),
        "{why}"
    );
    assert!(Recipe::read(&text.replacen("version 1", "version 0", 1)).is_err());
}

proptest! {
    #[test]
    fn any_cuts_read_back_exactly(
        cuts in proptest::collection::vec(
            (1..=40i64, -4500..=4500i32, 0..=4500i32, 0..4u8, any::<bool>()),
            1..5,
        ),
    ) {
        let mut board = Board::new(inches(24), fraction(29, 4), fraction(3, 4));
        for (setting, miter, tilt, way, offcut) in cuts {
            let setting = fraction(setting, 8);
            let pass = match way {
                0 => table_saw(setting, tilt).rip(&board, Orientation::default()),
                1 => table_saw(setting, tilt).rip(&board, Orientation::default().turned_left()),
                2 => MiterSaw {
                    miter: Angle::from_hundredths(miter),
                    ..MiterSaw::twelve_inch()
                }
                .crosscut(&board, Orientation::default(), setting),
                _ => table_saw(setting, 0).rip(&board, Orientation::default().tipped()),
            };
            board = match pass {
                Ok(woodsim::Pass::Cut(cut)) => match cut.offcut {
                    Some(other) if offcut => other.board,
                    _ => cut.piece.board,
                },
                Ok(woodsim::Pass::Kerfed(kerfed)) => kerfed,
                _ => continue,
            };
        }
        let recipe = Recipe { wood: Wood::Board(board.clone()), saw: None };
        let text = recipe.write();
        let read = Recipe::read(&text).map_err(TestCaseError::fail)?;
        prop_assert_eq!(&read, &recipe);
        prop_assert_eq!(read.write(), text);
    }
}

#[test]
fn passes_after_reading_a_recipe_come_after_the_passes_in_it() {
    let text = "\
        stock s0 609600000 88900000 19050000\n\
        pass p0 900000 table-saw 254000000 3175000 50800000 25400000 none 0 0 board \
            1099511627776 0 0 0 1099511627776 0 0 0 1099511627776 -127000000 736600000\n\
        sawn s1 s0 p0 0 1 0 63500000 66675000 1 0 0 127000000 -50800000 -127000000 \
            736600000 piece 0 0 0 0 measured\n\
        board s1\n";
    let Wood::Board(board) = Recipe::read(text).unwrap().wood else {
        panic!("a board");
    };
    let cut = table_saw(inches(1), 0)
        .rip(&board, Orientation::default())
        .cut_apart();
    let numbers: Vec<u64> = cut
        .piece
        .board
        .origin()
        .operations()
        .iter()
        .map(|operation| operation.number)
        .collect();
    assert_eq!(numbers[0], 900000);
    assert!(numbers[1] > 900000, "{numbers:?}");
}
