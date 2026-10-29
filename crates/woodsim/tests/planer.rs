//! The thickness planer: fed all the way through, wood comes out as thick as the setting,
//! measured from whatever it rested on, and no narrower or shorter; it won't take wood wider than
//! its knives, or more than a pass's worth; stopped partway, it leaves a step in the top. Any
//! size of planer works the same way, its numbers changed. Glue-ups go through too, and
//! everything it makes replays, writes out, and makes again with it set another way.

mod common;

use std::sync::Arc;

use common::CutApart;
use woodsim::{
    Assembly, Board, Cut, CutError, Fastener, GlueUpPass, Length, Mating, Operation, Orientation,
    Pass, Planer, Recipe, Remake, Rotation, Side, Surface, Tool, Wood, surfaces,
};

fn inches(n: i64) -> Length {
    Length::from_inches(n)
}

fn fraction(numerator: i64, denominator: i64) -> Length {
    Length::from_inch_fraction(numerator, denominator)
}

/// A 1x6, 24" long: 3/4" x 5-1/2" x 24".
fn one_by_six() -> Board {
    Board::new(inches(24), fraction(11, 2), fraction(3, 4))
}

/// A 13" planer set to `thickness`.
fn planer(thickness: Length) -> Planer {
    Planer {
        thickness,
        ..Planer::thirteen_inch()
    }
}

/// The board a pass left, all the way through: the measured piece, with nothing else.
fn planed(cut: Cut) -> Board {
    assert_eq!(cut.offcut, None);
    assert!(cut.loose.is_empty());
    assert_eq!(cut.piece.board.origin().side(), Some(Side::Measured));
    cut.piece.board
}

#[test]
fn planed_it_comes_out_as_thick_as_the_setting_and_nothing_else_changes() {
    let board = one_by_six();
    let cut = planer(fraction(11, 16))
        .plane(&board, Orientation::default())
        .cut_apart();
    assert_eq!(cut.waste, fraction(1, 16));
    // It rested on its bottom face, which stays where it was; the knives took the top.
    assert_eq!(
        (cut.piece.l, cut.piece.w, cut.piece.t),
        (Length::ZERO, Length::ZERO, Length::ZERO)
    );
    let piece = planed(cut);
    assert_eq!(
        (piece.length(), piece.width(), piece.thickness()),
        (inches(24), fraction(11, 2), fraction(11, 16))
    );
    assert!(piece.kerfs().is_empty());
    assert_eq!(piece.origin().replay(), piece);
}

#[test]
fn passes_take_it_down_a_step_at_a_time() {
    let mut board = one_by_six();
    for sixteenths in (8..12).rev() {
        board = planed(
            planer(fraction(sixteenths, 16))
                .plane(&board, Orientation::default())
                .cut_apart(),
        );
    }
    assert_eq!(board.thickness(), fraction(1, 2));
    assert_eq!(board.origin().operations().len(), 4);
    assert_eq!(board.origin().replay(), board);
}

#[test]
fn it_wont_take_more_than_a_passs_worth_or_wood_too_wide_to_go_through() {
    let lie = Orientation::default();
    assert_eq!(
        planer(fraction(1, 2)).plane(&one_by_six(), lie),
        Err(CutError::TooDeep {
            off: fraction(1, 4),
            most: fraction(1, 8),
        })
    );
    let wide = Board::new(inches(24), inches(14), fraction(3, 4));
    assert_eq!(
        planer(fraction(11, 16)).plane(&wide, lie),
        Err(CutError::TooWide {
            across: inches(14),
            widest: inches(13),
        })
    );
    // A board exactly as wide as the knives goes through.
    let thirteen = Board::new(inches(24), inches(13), fraction(3, 4));
    let piece = planed(planer(fraction(11, 16)).plane(&thirteen, lie).cut_apart());
    assert_eq!(piece.thickness(), fraction(11, 16));
}

#[test]
fn set_thicker_than_the_wood_the_knives_miss_it() {
    for thickness in [fraction(3, 4), inches(1)] {
        assert_eq!(
            planer(thickness).plane(&one_by_six(), Orientation::default()),
            Ok(Pass::Missed)
        );
    }
}

#[test]
fn a_bigger_planer_is_the_same_machine_with_other_numbers() {
    // A 24" planer takes a 20" panel the 13" one won't, and a deeper pass.
    let big = Planer {
        knives: inches(24),
        cutterhead: fraction(3, 1),
        thickest: inches(9),
        deepest: fraction(3, 16),
        thickness: fraction(9, 16),
        ..Planer::thirteen_inch()
    };
    let panel = Board::new(inches(36), inches(20), fraction(3, 4));
    let lie = Orientation::default();
    assert!(matches!(
        Planer {
            thickness: fraction(9, 16),
            ..Planer::thirteen_inch()
        }
        .plane(&panel, lie),
        Err(CutError::TooWide { .. })
    ));
    let piece = planed(big.plane(&panel, lie).cut_apart());
    assert_eq!(
        (piece.width(), piece.thickness()),
        (inches(20), fraction(9, 16))
    );
}

#[test]
fn on_edge_it_planes_the_edge_up() {
    // Stood on its `w = 0` edge, its width up: the knives take the `w = width` edge.
    let lie = Orientation::default().tipped();
    let piece = planed(
        planer(fraction(43, 8))
            .plane(&one_by_six(), lie)
            .cut_apart(),
    );
    assert_eq!(
        (piece.width(), piece.thickness()),
        (fraction(43, 8), fraction(3, 4))
    );
}

#[test]
fn stopped_partway_it_leaves_a_step_in_the_top() {
    let board = one_by_six();
    let planer = planer(fraction(11, 16));
    let radius = planer.radius();
    let Ok(Pass::Kerfed(stepped)) =
        planer.feed(&board, Orientation::default(), -radius, inches(10))
    else {
        panic!("stopped partway, it stays in one piece");
    };
    let high = fraction(3, 4) - fraction(1, 64);
    let at = |l: Length| stepped.has_wood_in(l, inches(3), high);
    assert!(!at(inches(5)) && !at(inches(10)));
    assert!(at(inches(20)));
    assert_eq!(stepped.thickness(), fraction(3, 4));
}

/// A panel of two boards, each 3" x 24" x 3/4", glued edge to edge, face up.
fn panel() -> Assembly {
    let board = Board::new(inches(24), inches(3), fraction(3, 4));
    let edge = |far: bool| {
        surfaces(&board)
            .into_iter()
            .find(|surface| {
                matches!(surface, Surface::Side(edge) if edge.out.l == 0 && (edge.out.w > 0) == far)
            })
            .expect("an edge")
    };
    let mut panel = Assembly::of(board.clone());
    let mating = (0..4)
        .map(|turn| Mating {
            turn,
            slide: [Length::ZERO; 2],
        })
        .find(|&mating| {
            let other = Assembly::of(board.clone());
            let motion = panel.mating((0, edge(true)), &other, (0, edge(false)), mating);
            motion.is_some_and(|motion| motion.rotation.apply([0.0, 0.0, 1.0])[2] > 0.999)
        })
        .expect("a way that keeps it face up");
    assert!(panel.attach(
        (0, edge(true)),
        &Assembly::of(board.clone()),
        (0, edge(false)),
        mating,
        Fastener::Glue
    ));
    panel
}

#[test]
fn a_glue_up_comes_out_the_setting_thick_and_still_glued() {
    let Ok(GlueUpPass::Cut(cut)) = planer(fraction(5, 8)).plane_glue_up(&panel(), Rotation::NONE)
    else {
        panic!("the panel comes out planed");
    };
    assert!(cut.offcut.is_none() && cut.loose.is_empty());
    assert_eq!(cut.piece.joints().len(), 1);
    for member in cut.piece.members() {
        assert_eq!(member.board.thickness(), fraction(5, 8));
    }
}

#[test]
fn a_glue_up_of_one_board_planes_like_the_board() {
    let board = one_by_six();
    let flat = Orientation::default();
    for (lie, thickness) in [
        (flat, fraction(11, 16)),
        (flat.flipped(), fraction(11, 16)),
        (flat.turned_left().turned_left(), fraction(5, 8)),
        (flat.tipped(), fraction(43, 8)),
    ] {
        let planer = planer(thickness);
        let alone = planer.plane(&board, lie).cut_apart();
        let Ok(GlueUpPass::Cut(cut)) =
            planer.plane_glue_up(&Assembly::of(board.clone()), lie.rotation())
        else {
            panic!("planed as a glue-up too");
        };
        assert_eq!(cut.piece.members()[0].board, alone.piece.board);
    }
}

#[test]
fn what_it_makes_writes_out_and_reads_back() {
    let planer = planer(fraction(23, 32));
    let lie = Orientation::default().flipped();
    let board = planed(planer.plane(&one_by_six(), lie).cut_apart());
    let recipe = Recipe {
        wood: Wood::Board(board.clone()),
        saw: Some(Tool::Planer {
            planer,
            lie: woodsim::Lie::Board(lie),
            travel: (-planer.radius(), inches(30)),
        }),
    };
    let text = recipe.write();
    assert!(text.contains(" planer "), "{text}");
    let read = Recipe::read(&text).unwrap();
    assert_eq!(read.wood, Wood::Board(board));
    assert_eq!(read.saw, recipe.saw);
    assert_eq!(read.write(), text);
}

#[test]
fn made_again_thinner_it_comes_out_thinner_and_still_all_the_way_through() {
    let board = one_by_six();
    let lie = Orientation::default();
    let pass = planed(planer(fraction(23, 32)).plane(&board, lie).cut_apart());
    let operation: Arc<Operation> = pass.origin().operations().pop().unwrap();
    let Tool::Planer { lie, travel, .. } = operation.tool else {
        panic!("a planer pass");
    };
    let thinner = Tool::Planer {
        planer: planer(fraction(11, 16)),
        lie,
        travel,
    };
    let made = Remake::new(&operation, thinner)
        .board(&pass)
        .unwrap()
        .unwrap();
    assert_eq!(made.thickness(), fraction(11, 16));
    assert_eq!(made.origin().operations()[0].number, operation.number);
    assert_eq!(made.origin().replay(), made);
}
