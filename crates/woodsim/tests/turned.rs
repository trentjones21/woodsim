//! Boards lying other ways on the saws: turned a quarter turn to crosscut on the table saw,
//! turned end for end, and flipped over. And slots in two directions that meet.

mod common;

use common::CutApart;

use proptest::prelude::*;
use woodsim::{
    Blade, Board, Cut, CutError, Direction, Length, MiterSaw, Orientation, Pass, Piece, TableSaw,
};

fn inches(n: i64) -> Length {
    Length::from_inches(n)
}

fn fraction(numerator: i64, denominator: i64) -> Length {
    Length::from_inch_fraction(numerator, denominator)
}

/// A 1x8, 24" long: 3/4" x 7-1/4" x 24".
fn one_by_eight() -> Board {
    Board::new(inches(24), fraction(29, 4), fraction(3, 4))
}

/// A table saw with a 10" full-kerf blade, its fence at `fence` and blade `height` up.
fn table_saw(fence: Length, height: Length) -> TableSaw {
    TableSaw {
        blade: Blade {
            diameter: inches(10),
            kerf: Blade::FULL_KERF,
        },
        fence,
        height,
        miter_gauge: None,
    }
}

/// A 12" miter saw that doesn't slide.
fn miter_saw() -> MiterSaw {
    MiterSaw::twelve_inch()
}

fn unturned() -> Orientation {
    Orientation::default()
}

fn cut(pass: Result<Pass, CutError>) -> Cut {
    match pass {
        Ok(Pass::Cut(cut)) => cut,
        other => panic!("expected the board to be cut apart, got {other:?}"),
    }
}

fn kerfed(pass: Result<Pass, CutError>) -> Board {
    match pass {
        Ok(Pass::Kerfed(board)) => board,
        other => panic!("expected a slot in one board, got {other:?}"),
    }
}

#[test]
fn turned_a_quarter_turn_the_table_saw_crosscuts() {
    let lie = unturned().turned_left();
    let cut = cut(table_saw(inches(12), inches(1)).rip(&one_by_eight(), lie));

    // The fence side is the board's far end now, so the 12" piece comes off that end.
    assert_eq!(cut.piece.board.to_string(), "3/4\" x 7-1/4\" x 12\"");
    assert_eq!((cut.piece.l, cut.piece.w), (inches(12), Length::ZERO));
    let offcut = cut.offcut.unwrap();
    assert_eq!(offcut.board.to_string(), "3/4\" x 7-1/4\" x 11-7/8\"");
    assert_eq!(offcut.l, Length::ZERO);
    assert_eq!(cut.waste, Blade::FULL_KERF);
}

#[test]
fn turned_the_other_way_the_piece_comes_off_the_other_end() {
    let lie = unturned().turned_right();
    let cut = cut(table_saw(inches(12), inches(1)).rip(&one_by_eight(), lie));

    assert_eq!(cut.piece.board.length(), inches(12));
    assert_eq!(cut.piece.l, Length::ZERO);
    assert_eq!(cut.offcut.unwrap().l, inches(12) + Blade::FULL_KERF);
}

/// The table saw with its miter gauge in, its stop `stop` from the blade.
fn miter_gauge(stop: Length) -> TableSaw {
    TableSaw {
        miter_gauge: Some(stop),
        ..table_saw(inches(6), inches(1))
    }
}

#[test]
fn the_miter_gauge_measures_the_piece_from_its_stop() {
    let lie = unturned().turned_left();
    let cut = cut(miter_gauge(inches(12)).rip(&one_by_eight(), lie));

    // The stop is against the board's near side across the table: its `l = 0` end.
    assert_eq!(cut.piece.board.to_string(), "3/4\" x 7-1/4\" x 12\"");
    assert_eq!(cut.piece.l, Length::ZERO);
    let offcut = cut.offcut.unwrap();
    assert_eq!(offcut.board.length(), fraction(95, 8));
    assert_eq!(offcut.l, inches(12) + Blade::FULL_KERF);
}

#[test]
fn a_gauge_stop_past_the_board_misses_it() {
    assert_eq!(
        miter_gauge(inches(24)).rip(&one_by_eight(), unturned().turned_left()),
        Err(CutError::MissesBoard {
            setting: inches(24),
            available: inches(24),
        })
    );
}

#[test]
fn turned_with_the_blade_low_it_cuts_a_dado_across_the_board() {
    let lie = unturned().turned_left();
    let board = kerfed(table_saw(inches(12), fraction(1, 4)).rip(&one_by_eight(), lie));

    let dado = board.kerfs()[0];
    assert_eq!(dado.across, Direction::LENGTH);
    assert_eq!(
        (dado.near, dado.far),
        (inches(12) - Blade::FULL_KERF, inches(12))
    );
    assert_eq!(dado.depth(board.thickness()), fraction(1, 4));
    // It runs the board's whole width.
    let (start, end) = dado.span_at(Length::ZERO).unwrap();
    assert!(start < Length::ZERO && end > board.width());
}

#[test]
fn flipped_over_the_blade_cuts_into_the_top_face() {
    let lie = unturned().flipped();
    let board = kerfed(table_saw(inches(6), fraction(1, 4)).rip(&one_by_eight(), lie));

    let groove = board.kerfs()[0];
    assert!(!groove.from_below());
    assert_eq!(groove.depth(board.thickness()), fraction(1, 4));
    // Flipped, the board's `w = 0` edge rides the fence, so the groove is 6" from it.
    assert_eq!(
        (groove.near, groove.far),
        (inches(6), inches(6) + Blade::FULL_KERF)
    );
}

#[test]
fn a_rip_and_a_crosscut_that_meet_cut_out_the_corner_between_them() {
    // A stopped rip 2" from the far edge, through the board for its first 9-9/16" or so.
    let ripped = kerfed(table_saw(inches(2), inches(1)).feed(
        &one_by_eight(),
        unturned(),
        -inches(6),
        inches(8),
    ));
    // Turned, a stopped crosscut 9" along it, in from that far edge until it meets the rip.
    let cut = cut(table_saw(fraction(119, 8), inches(1)).feed(
        &ripped,
        unturned().turned_left(),
        -inches(6),
        fraction(3, 2),
    ));

    // The 2" x 9" corner falls out.
    let corner = cut.offcut.unwrap();
    assert_eq!(corner.board.to_string(), "3/4\" x 2\" x 9\"");
    assert_eq!((corner.l, corner.w), (Length::ZERO, fraction(21, 4)));
    // The rest of the board is still 24" long and 7-1/4" wide, less that corner.
    assert_eq!(cut.piece.board.to_string(), "3/4\" x 7-1/4\" x 24\"");
    let rest = &cut.piece.board;
    assert!(!rest.has_wood_at(inches(4), inches(6)));
    assert!(rest.has_wood_at(inches(10), inches(6)));
    assert!(rest.has_wood_at(inches(4), inches(5)));
    assert!(cut.loose.is_empty());
}

#[test]
fn a_miter_saw_cuts_into_a_board_wider_than_it_reaches_and_leaves_it_whole() {
    let lie = unturned().turned_left();
    let slotted = kerfed(miter_saw().crosscut(&one_by_eight(), lie, inches(4)));

    // Turned, the board is 24" across the saw; the blade goes through it for the 7-1/2" out
    // from the fence that it reaches.
    let slot = slotted.slot(&slotted.kerfs()[0]);
    let bottom = slot.bottom.unwrap();
    assert!(!bottom.end_to_end);
    assert_eq!(bottom.length.rounded_to(fraction(1, 32)), fraction(15, 2));
    assert_eq!(slotted.to_string(), one_by_eight().to_string());
}

#[test]
fn turned_end_for_end_the_other_end_goes_against_the_stop() {
    // A stopped cut through the first 11-9/16" or so of the board, at its `l = 0` end.
    let slotted = kerfed(table_saw(inches(6), inches(1)).feed(
        &one_by_eight(),
        unturned(),
        -inches(6),
        inches(10),
    ));
    let end_for_end = unturned().turned_left().turned_left();
    let cut = miter_saw()
        .crosscut(&slotted, end_for_end, inches(10))
        .cut_apart();

    // The 10" against the stop is from the slotted end, so it falls apart along the slot.
    assert_eq!(cut.piece.l, Length::ZERO);
    assert_eq!(cut.piece.board.to_string(), "3/4\" x 6\" x 10\"");
    assert_eq!(cut.loose.len(), 1);
    assert_eq!(cut.loose[0].board.to_string(), "3/4\" x 1-1/8\" x 10\"");
    // The other 13-7/8" holds together.
    assert_eq!(cut.offcut.unwrap().board.width(), fraction(29, 4));
}

/// The orientation `turns` quarter turns to the left, flipped first if `flipped`.
fn lying(turns: u8, flipped: bool) -> Orientation {
    let mut lie = if flipped {
        unturned().flipped()
    } else {
        unturned()
    };
    for _ in 0..turns {
        lie = lie.turned_left();
    }
    lie
}

/// Whether `piece`, with its corner at `corner` on the original board, has wood all the way
/// through at the point `(l, w)` of the original board.
fn wood_at(piece: &Board, corner: (Length, Length), (l, w): (Length, Length)) -> bool {
    piece.has_wood_at(l - corner.0, w - corner.1)
}

proptest! {
    #[test]
    fn cutting_never_puts_the_same_wood_in_two_pieces(
        passes in prop::collection::vec(
            (0u8..4, any::<bool>(), 1..24 * 4i64, 0..8 * 4i64, -6 * 4..30 * 4i64),
            1..5,
        ),
    ) {
        let original = one_by_eight();
        // The board still being cut, and its corner on the original; and the pieces set aside.
        let mut board = original.clone();
        let mut corner = (Length::ZERO, Length::ZERO);
        let mut set_aside: Vec<(Board, (Length, Length))> = Vec::new();
        for (turns, flipped, fence, height, end) in passes {
            let saw = table_saw(fraction(fence, 4), fraction(height, 4));
            let lie = lying(turns, flipped);
            let start = -inches(6);
            let end = fraction(end, 4).max(start);
            match saw.feed(&board, lie, start, end) {
                Ok(Pass::Kerfed(slotted)) => board = slotted,
                Ok(Pass::Cut(cut)) => {
                    let at = |piece: &Piece| (corner.0 + piece.l, corner.1 + piece.w);
                    for other in cut.offcut.iter().chain(&cut.loose) {
                        set_aside.push((other.board.clone(), at(other)));
                    }
                    corner = at(&cut.piece);
                    board = cut.piece.board;
                }
                Ok(Pass::Missed) | Err(_) => {}
            }
        }
        set_aside.push((board, corner));

        // Sample the original board's face, off the grid of 1/16" marks the saws are set to.
        let step = fraction(1, 4);
        let nudge = Length::from_nm(12_345);
        let mut l = nudge;
        while l < original.length() {
            let mut w = nudge;
            while w < original.width() {
                let holders = set_aside.iter().filter(|(piece, at)| wood_at(piece, *at, (l, w))).count();
                prop_assert!(holders <= 1, "{} pieces have wood at ({}, {})", holders, l, w);
                w = w + step;
            }
            l = l + step;
        }
    }
}
