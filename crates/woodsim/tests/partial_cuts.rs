//! Cuts that don't go all the way: stopping partway through, and a blade set lower than the
//! board is thick. They leave slots in the board, which come along when it's cut apart later.

use woodsim::{Blade, Board, Kerf, Length, MiterSaw, Pass, TableSaw};

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
    }
}

/// The one slot a pass cut, expecting it left the board in one piece.
fn kerfed(pass: Pass) -> (Board, Kerf) {
    match pass {
        Pass::Kerfed(board) => {
            let kerf = *board.kerfs().last().unwrap();
            (board, kerf)
        }
        other => panic!("expected a slot in one board, got {other:?}"),
    }
}

#[test]
fn stopping_partway_leaves_one_board_with_a_slot() {
    // The board starts 6" short of the blade's center and stops with the center 10" along it.
    let saw = table_saw(inches(6), inches(1));
    let (board, kerf) = kerfed(saw.feed(&one_by_eight(), -inches(6), inches(10)).unwrap());

    assert_eq!(board.to_string(), "3/4\" x 7-1/4\" x 24\"");
    // The slot is 6" from the fence edge, 1/8" wide.
    assert_eq!((kerf.near, kerf.far), (fraction(9, 8), fraction(5, 4)));
    // With the blade's center 4" below the board, it reaches 3" past its center along the
    // bottom, but only about 1-9/16" along the top.
    assert_eq!(kerf.span_at(Length::ZERO).unwrap().1, inches(13));
    let top = kerf.span_at(fraction(3, 4)).unwrap().1;
    assert_eq!(
        (top - inches(10)).rounded_to(fraction(1, 32)),
        fraction(25, 16)
    );
}

#[test]
fn the_board_comes_apart_when_the_slot_reaches_its_far_top_corner() {
    let saw = table_saw(inches(6), inches(1));
    let board = one_by_eight();
    // How far past its center the blade reaches along the top of the board.
    let (_, kerf) = kerfed(saw.feed(&board, -inches(6), Length::ZERO).unwrap());
    let reach = kerf.span_at(fraction(3, 4)).unwrap().1;
    let last = board.length() - reach;

    assert!(matches!(
        saw.feed(&board, -inches(6), last - Length::from_nm(1)),
        Ok(Pass::Kerfed(_))
    ));
    let Ok(Pass::Cut(cut)) = saw.feed(&board, -inches(6), last) else {
        panic!("the board should be cut apart");
    };
    assert_eq!(cut.piece.width(), inches(6));
    assert!(cut.piece.kerfs().is_empty());
}

#[test]
fn a_blade_lower_than_the_board_cuts_a_groove() {
    let saw = table_saw(inches(6), fraction(1, 4));
    let (board, kerf) = kerfed(saw.rip(&one_by_eight()).unwrap());

    assert_eq!(kerf.top(), fraction(1, 4));
    let (start, end) = kerf.span_at(Length::ZERO).unwrap();
    assert!(start < Length::ZERO && end > board.length());
}

#[test]
fn a_blade_exactly_as_high_as_the_board_cuts_through() {
    let board = one_by_eight();
    assert!(matches!(
        table_saw(inches(6), fraction(3, 4)).rip(&board),
        Ok(Pass::Cut(_))
    ));
    assert!(matches!(
        table_saw(inches(6), fraction(3, 4) - Length::from_nm(1)).rip(&board),
        Ok(Pass::Kerfed(_))
    ));
}

#[test]
fn a_blade_below_the_table_misses_the_board() {
    for height in [Length::ZERO, -inches(1)] {
        assert_eq!(
            table_saw(inches(6), height).rip(&one_by_eight()),
            Ok(Pass::Missed)
        );
    }
}

#[test]
fn a_board_short_of_the_blade_is_untouched() {
    let saw = table_saw(inches(6), inches(1));
    // The blade breaks the table 3" in front of its center.
    assert_eq!(
        saw.feed(&one_by_eight(), -inches(6), -inches(3)),
        Ok(Pass::Missed)
    );
    assert!(matches!(
        saw.feed(&one_by_eight(), -inches(6), -inches(3) + Length::from_nm(1)),
        Ok(Pass::Kerfed(_))
    ));
}

#[test]
fn a_blade_over_the_edge_cuts_a_notch_along_it() {
    // The fence 1/16" short of the board's width puts half the blade past the edge.
    let saw = table_saw(fraction(29, 4) - fraction(1, 16), fraction(3, 8));
    let (_, kerf) = kerfed(saw.rip(&one_by_eight()).unwrap());

    assert_eq!((kerf.near, kerf.far), (-fraction(1, 16), fraction(1, 16)));
}

#[test]
fn slots_come_along_when_the_board_is_cut_apart() {
    // A 1/4"-deep groove 2" from the fence edge, then a rip at 6" and a crosscut at 12".
    let (grooved, _) = kerfed(
        table_saw(inches(2), fraction(1, 4))
            .rip(&one_by_eight())
            .unwrap(),
    );
    let Ok(Pass::Cut(ripped)) = table_saw(inches(6), inches(1)).rip(&grooved) else {
        panic!("the rip should cut the board apart");
    };
    // The groove is in the 6" piece, now measured from its own sawn edge.
    let groove = ripped.piece.kerfs()[0];
    assert_eq!((groove.near, groove.far), (fraction(31, 8), inches(4)));
    assert!(ripped.offcut.unwrap().kerfs().is_empty());

    let miter_saw = MiterSaw {
        blade: Blade {
            diameter: inches(12),
            kerf: Blade::FULL_KERF,
        },
    };
    let crosscut = miter_saw.crosscut(&ripped.piece, inches(12)).unwrap();
    // Both ends of the crosscut keep the groove, which still runs their full length.
    for part in [&crosscut.piece, crosscut.offcut.as_ref().unwrap()] {
        let groove = part.kerfs()[0];
        let (start, end) = groove.span_at(Length::ZERO).unwrap();
        assert!(start < Length::ZERO && end > part.length(), "{part}");
    }
}
