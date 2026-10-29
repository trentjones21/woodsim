//! The jointer: fed all the way across, wood comes off it thinner by the depth of cut wherever
//! it lay on the tables, a face or an edge, and no wider or shorter; reaching past the knives'
//! end, it's rabbeted, the rabbet no deeper however many passes; stopped partway, it's left
//! with a step curved as the cutterhead; and with the fence tilted, each pass takes a little
//! more off the edge it leans on, until it's beveled. Glue-ups come off it the same way, and
//! everything it makes replays, writes out, and makes again with the jointer set another way.

mod common;

use std::sync::Arc;

use common::CutApart;
use woodsim::{
    Angle, Assembly, Board, Cut, Fastener, GlueUpPass, Jointer, Length, Mating, Operation,
    Orientation, Pass, Recipe, Remake, Rotation, Side, Surface, Tool, Wood, surfaces,
};

fn inches(n: i64) -> Length {
    Length::from_inches(n)
}

fn fraction(numerator: i64, denominator: i64) -> Length {
    Length::from_inch_fraction(numerator, denominator)
}

const CUBIC_INCH: f64 = 25_400_000.0 * 25_400_000.0 * 25_400_000.0;

/// A 1x6, 24" long: 3/4" x 5-1/2" x 24".
fn one_by_six() -> Board {
    Board::new(inches(24), fraction(11, 2), fraction(3, 4))
}

/// A 6" jointer set to take `depth`.
fn jointer(depth: Length) -> Jointer {
    Jointer {
        depth,
        ..Jointer::six_inch()
    }
}

/// The board a pass left, all the way across: the piece, with nothing else.
fn jointed(cut: Cut) -> Board {
    assert_eq!(cut.offcut, None);
    assert!(cut.loose.is_empty());
    assert_eq!(cut.piece.board.origin().side(), Some(Side::Offcut));
    cut.piece.board
}

#[test]
fn jointing_a_face_takes_the_depth_off_it_and_nothing_else() {
    let board = one_by_six();
    let cut = jointer(fraction(1, 16))
        .joint(&board, Orientation::default())
        .cut_apart();
    assert_eq!(cut.waste, fraction(1, 16));
    // The bottom face lay on the tables: the piece starts 1/16" up the board.
    assert_eq!(
        (cut.piece.l, cut.piece.w, cut.piece.t),
        (Length::ZERO, Length::ZERO, fraction(1, 16))
    );
    let piece = jointed(cut);
    assert_eq!(
        (piece.length(), piece.width(), piece.thickness()),
        (inches(24), fraction(11, 2), fraction(11, 16))
    );
    assert!(piece.kerfs().is_empty());
    assert_eq!(piece.origin().replay(), piece);
}

#[test]
fn passes_add_up() {
    let mut board = one_by_six();
    for _ in 0..4 {
        board = jointed(
            jointer(fraction(1, 16))
                .joint(&board, Orientation::default())
                .cut_apart(),
        );
    }
    assert_eq!(board.thickness(), fraction(1, 2));
    assert_eq!(board.origin().operations().len(), 4);
    assert_eq!(board.origin().replay(), board);
}

#[test]
fn on_edge_it_joints_the_edge_down() {
    // Tipped onto its `w = 0` edge, its bottom face to the fence.
    let lie = Orientation::default().tipped();
    let cut = jointer(fraction(1, 32))
        .joint(&one_by_six(), lie)
        .cut_apart();
    assert_eq!(cut.piece.w, fraction(1, 32));
    let piece = jointed(cut);
    assert_eq!(
        (piece.width(), piece.thickness()),
        (fraction(175, 32), fraction(3, 4))
    );
    assert!(piece.kerfs().is_empty());
}

#[test]
fn a_board_wider_than_the_fence_is_from_the_knives_end_is_rabbeted() {
    // A 1x8 against the fence, all the way back: 6" of it over the knives, 1-1/4" past them.
    let board = Board::new(inches(24), fraction(29, 4), fraction(3, 4));
    let Ok(Pass::Kerfed(rabbeted)) = jointer(fraction(1, 16)).joint(&board, Orientation::default())
    else {
        panic!("a board wider than the knives stays in one piece");
    };
    let at = |w: Length| rabbeted.has_wood_in(inches(12), w, fraction(1, 32));
    // Its `w = width` edge rides the fence, so the ledge is along the other.
    assert!(at(fraction(1, 2)));
    assert!(!at(inches(2)) && !at(inches(7)));
    let slot = rabbeted.slot(&rabbeted.kerfs()[0]);
    assert_eq!(slot.entry, Surface::Bottom);
    assert_eq!((slot.width, slot.depth), (inches(6), fraction(1, 16)));
    assert!(slot.at_edge);

    // The ledge rides the tables, so another pass takes nothing more.
    let Ok(Pass::Kerfed(again)) = jointer(fraction(1, 16)).joint(&rabbeted, Orientation::default())
    else {
        panic!("still in one piece");
    };
    for (w, t) in [(2, 3), (2, 5), (7, 3), (7, 5)] {
        let (w, t) = (inches(w), fraction(t, 64));
        assert_eq!(
            again.has_wood_in(inches(12), w, t),
            rabbeted.has_wood_in(inches(12), w, t)
        );
    }

    // With the fence 2" from the knives' end, it's a 2" rabbet.
    let narrow = Jointer {
        fence: inches(2),
        ..jointer(fraction(3, 8))
    };
    let Ok(Pass::Kerfed(rabbeted)) = narrow.joint(&board, Orientation::default()) else {
        panic!("a rabbet leaves it in one piece");
    };
    let slot = rabbeted.slot(&rabbeted.kerfs()[0]);
    assert_eq!((slot.width, slot.depth), (inches(2), fraction(3, 8)));
}

#[test]
fn stopped_partway_it_leaves_a_step_curved_as_the_cutterhead() {
    let board = one_by_six();
    let jointer = jointer(fraction(1, 16));
    let radius = jointer.radius();
    let Ok(Pass::Kerfed(stepped)) =
        jointer.feed(&board, Orientation::default(), -radius, inches(10))
    else {
        panic!("stopped partway, it stays in one piece");
    };
    let low = fraction(1, 64);
    let at = |l: Length| stepped.has_wood_in(l, inches(3), low);
    // Behind where the cutterhead's center stopped, the bottom 1/16" is gone; a little past,
    // the circle still reaches up 1/64"; further on, the wood's untouched.
    assert!(!at(inches(5)) && !at(inches(10)));
    assert!(!at(inches(10) + fraction(1, 4)));
    assert!(at(inches(10) + fraction(1, 2)));
    assert!(at(inches(20)));
}

#[test]
fn set_to_nothing_or_with_no_knife_showing_it_misses() {
    let board = one_by_six();
    let lie = Orientation::default();
    assert_eq!(jointer(Length::ZERO).joint(&board, lie), Ok(Pass::Missed));
    let fenced_off = Jointer {
        fence: Length::ZERO,
        ..jointer(fraction(1, 16))
    };
    assert_eq!(fenced_off.joint(&board, lie), Ok(Pass::Missed));
}

#[test]
fn with_the_fence_tilted_each_pass_bevels_the_edge_a_little_more() {
    // A 1x4 on edge, its bottom face against the fence tilted 45°, leaning on the corner of
    // its edge away from the fence.
    let mut board = Board::new(inches(24), fraction(7, 2), fraction(3, 4));
    let tilted = Jointer {
        tilt: Angle::from_degrees(45),
        ..jointer(fraction(1, 16))
    };
    let lie = Orientation::default().tipped();
    let depth = fraction(1, 16).as_nm() as f64 / 25_400_000.0;

    // The first pass takes a sliver off that corner, as deep as the setting straight down: a
    // triangle as wide as it's deep either side of the corner.
    let first = jointed(tilted.joint(&board, lie).cut_apart());
    assert_eq!(
        (first.width(), first.thickness()),
        (fraction(7, 2), fraction(3, 4))
    );
    let bevels = first.bevels();
    assert_eq!(bevels.len(), 1, "{bevels:?}");
    assert_eq!(bevels[0].1, Angle::from_degrees(45));
    let lost = (board.volume() - first.volume()) / CUBIC_INCH;
    assert!(
        (lost - 24.0 * depth * depth).abs() < 1e-6,
        "{lost} cubic inches"
    );
    assert_eq!(first.origin().replay(), first);

    // Each pass rests on what the last left and takes as much again. Once it's taken the
    // whole edge, the board's narrower, beveled 45° from face to face.
    let mut passes = 1;
    board = first;
    while board.width() == fraction(7, 2) {
        board = jointed(tilted.joint(&board, lie).cut_apart());
        passes += 1;
        assert!(passes < 20, "it never bevels the edge all the way across");
    }
    // 3/4" across the edge, 45°, takes 3/4" / sqrt 2 straight down: nine passes of 1/16".
    assert_eq!(passes, 9);
    let bevels = board.bevels();
    assert_eq!(bevels.len(), 1, "{bevels:?}");
    assert_eq!(bevels[0].1, Angle::from_degrees(45));
    assert_eq!(board.thickness(), fraction(3, 4));
    assert_eq!(board.origin().replay(), board);
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
fn a_glue_up_comes_off_thinner_and_still_glued() {
    let panel = panel();
    let Ok(GlueUpPass::Cut(cut)) = jointer(fraction(1, 8)).joint_glue_up(&panel, Rotation::NONE)
    else {
        panic!("the panel comes off trimmed");
    };
    assert!(cut.offcut.is_none() && cut.loose.is_empty());
    assert_eq!(cut.piece.members().len(), 2);
    assert_eq!(cut.piece.joints().len(), 1);
    for member in cut.piece.members() {
        assert_eq!(member.board.thickness(), fraction(5, 8));
        assert_eq!(member.board.origin().side(), Some(Side::Offcut));
    }
}

#[test]
fn a_glue_up_of_one_board_joints_like_the_board() {
    let board = one_by_six();
    let square = jointer(fraction(1, 16));
    let tilted = Jointer {
        tilt: Angle::from_degrees(30),
        ..jointer(fraction(1, 8))
    };
    let flat = Orientation::default();
    for (lie, jointer) in [
        (flat, square),
        (flat.flipped(), square),
        (flat.tipped(), square),
        (flat.stood(), square),
        (flat.tipped(), tilted),
        (flat.tipped().flipped(), tilted),
    ] {
        let alone = jointer.joint(&board, lie).cut_apart();
        let Ok(GlueUpPass::Cut(cut)) =
            jointer.joint_glue_up(&Assembly::of(board.clone()), lie.rotation())
        else {
            panic!("trimmed as a glue-up too");
        };
        assert_eq!(cut.piece.members()[0].board, alone.piece.board);
    }
}

#[test]
fn a_board_whose_lowest_corner_leans_past_the_knives_end_misses_them() {
    // Flat against a fence tilted 30°, a 1x6 leans on its edge away from the fence, 6.35" out
    // along the tables from where the fence meets them, past the knives' end 6" out.
    let tilted = Jointer {
        tilt: Angle::from_degrees(30),
        ..jointer(fraction(1, 8))
    };
    assert_eq!(
        tilted.joint(&one_by_six(), Orientation::default()),
        Ok(Pass::Missed)
    );
}

#[test]
fn what_it_makes_writes_out_and_reads_back() {
    let jointer = Jointer {
        tilt: Angle::from_degrees(15),
        ..jointer(fraction(3, 32))
    };
    let lie = Orientation::default().tipped();
    let board = jointed(jointer.joint(&one_by_six(), lie).cut_apart());
    let recipe = Recipe {
        wood: Wood::Board(board.clone()),
        saw: Some(Tool::Jointer {
            jointer,
            lie: woodsim::Lie::Board(lie),
            travel: (-jointer.radius(), inches(30)),
        }),
    };
    let text = recipe.write();
    assert!(text.contains(" jointer "), "{text}");
    let read = Recipe::read(&text).unwrap();
    assert_eq!(read.wood, Wood::Board(board));
    assert_eq!(read.saw, recipe.saw);
    assert_eq!(read.write(), text);
}

#[test]
fn made_again_deeper_it_comes_out_thinner_and_still_all_the_way_across() {
    let board = one_by_six();
    let lie = Orientation::default();
    let pass = jointed(jointer(fraction(1, 32)).joint(&board, lie).cut_apart());
    let operation: Arc<Operation> = pass.origin().operations().pop().unwrap();
    let Tool::Jointer { lie, travel, .. } = operation.tool else {
        panic!("a jointer pass");
    };
    let deeper = Tool::Jointer {
        jointer: jointer(fraction(1, 8)),
        lie,
        travel,
    };
    let made = Remake::new(&operation, deeper)
        .board(&pass)
        .unwrap()
        .unwrap();
    assert_eq!(made.thickness(), fraction(5, 8));
    assert_eq!(made.origin().operations()[0].number, operation.number);
    assert_eq!(made.origin().replay(), made);
}
