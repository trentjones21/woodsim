//! Bringing a miter saw's blade only partway down: by hand, or against a depth stop. The blade
//! leaves a scar shaped like its circle, and side-by-side scars nibble out a wider one.

use proptest::prelude::*;
use woodsim::{Board, Direction, Length, MiterSaw, Orientation, Pass};

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

fn scarred(pass: Pass) -> Board {
    match pass {
        Pass::Kerfed(board) => board,
        other => panic!("expected a scar in one board, got {other:?}"),
    }
}

#[test]
fn stopping_short_leaves_a_scar_from_above() {
    let saw = MiterSaw::twelve_inch();
    // The blade's lowest point stops 1/2" above the table, 1/4" into the board.
    let board = scarred(
        saw.chop(
            &one_by_eight(),
            Orientation::default(),
            inches(12),
            fraction(1, 2),
        )
        .unwrap(),
    );

    let scar = board.kerfs()[0];
    assert!(!scar.from_below());
    assert_eq!(scar.across, Direction::LENGTH);
    assert_eq!(scar.depth(board.thickness()), fraction(1, 4));
    // The blade's circle dips into the top only under its center, which is a few inches out
    // from the fence: the scar doesn't reach the board's front edge or the fence.
    assert_eq!(scar.start, scar.end);
    let (start, end) = scar.span_at(board.thickness()).unwrap();
    assert!(
        start > Length::ZERO && end < board.width(),
        "{start} to {end}"
    );
    assert_eq!((end - start).rounded_to(fraction(1, 32)), fraction(55, 16));
}

#[test]
fn the_blade_reaches_out_on_its_arc() {
    let saw = MiterSaw::twelve_inch();
    // Level with the pivot, the blade's center reaches the arm's full 8" from it, 4" in front
    // of the fence; all the way down, it's swung back toward the fence.
    assert_eq!(saw.center(Length::ZERO), (inches(4), inches(6)));
    let (down, height) = saw.center(-inches(3));
    assert_eq!(height, inches(3));
    assert!(fraction(27, 8) < down && down < inches(4), "{down}");
}

#[test]
fn a_blade_stopped_at_the_table_scratches_the_bottom_but_does_not_cut_through() {
    let saw = MiterSaw::twelve_inch();
    let pass = saw
        .chop(
            &one_by_eight(),
            Orientation::default(),
            inches(12),
            Length::ZERO,
        )
        .unwrap();
    assert!(matches!(pass, Pass::Kerfed(_)), "{pass:?}");
}

#[test]
fn a_blade_stopped_above_the_board_misses_it() {
    let saw = MiterSaw::twelve_inch();
    assert_eq!(
        saw.chop(
            &one_by_eight(),
            Orientation::default(),
            inches(12),
            inches(1)
        ),
        Ok(Pass::Missed)
    );
}

#[test]
fn side_by_side_scars_nibble_out_a_wider_one() {
    let saw = MiterSaw::twelve_inch();
    let lie = Orientation::default();
    let depth_stop = fraction(1, 2);
    let once = scarred(
        saw.chop(&one_by_eight(), lie, inches(12), depth_stop)
            .unwrap(),
    );
    // Move the stop out a kerf and chop again, beside the first scar.
    let twice = scarred(
        saw.chop(&once, lie, inches(12) + fraction(1, 8), depth_stop)
            .unwrap(),
    );

    let [first, second] = twice.kerfs() else {
        panic!("expected two scars, got {:?}", twice.kerfs());
    };
    assert_eq!(first.near, second.far);
    assert_eq!(twice.to_string(), "3/4\" x 7-1/4\" x 24\"");
}

proptest! {
    #[test]
    fn chopping_at_any_depth_keeps_the_board_whole_or_conserves_it(
        stop in 1..23 * 64i64,
        bottom in -3 * 64..5 * 64i64,
    ) {
        let board = one_by_eight();
        let saw = MiterSaw::twelve_inch();
        let (stop, bottom) = (fraction(stop, 64), fraction(bottom, 64));

        match saw.chop(&board, Orientation::default(), stop, bottom).unwrap() {
            Pass::Missed => prop_assert!(bottom >= board.thickness()),
            Pass::Kerfed(scarred) => {
                prop_assert_eq!(scarred.to_string(), board.to_string());
                prop_assert_eq!(scarred.kerfs().len(), 1);
                let depth = scarred.kerfs()[0].depth(board.thickness());
                prop_assert_eq!(depth, board.thickness() - bottom);
            }
            Pass::Cut(cut) => {
                prop_assert!(bottom < Length::ZERO, "a blade above the table cut the board apart");
                let offcut = cut.offcut.as_ref().map_or(Length::ZERO, |offcut| offcut.board.length());
                prop_assert_eq!(cut.piece.board.length() + offcut + cut.waste, board.length());
                prop_assert_eq!(cut.piece.board.length(), stop);
            }
        }
    }
}
