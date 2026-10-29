mod common;

use common::CutApart;
use woodsim::{Blade, Board, Cut, CutError, Length, MiterSaw, Orientation, Pass, Side, TableSaw};

/// `whole numerator/denominator` inches, e.g. `inches(7, 1, 4)` for 7-1/4".
fn inches(whole: i64, numerator: i64, denominator: i64) -> Length {
    Length::from_inches(whole) + Length::from_inch_fraction(numerator, denominator)
}

/// A 1x8 as sold: 3/4" x 7-1/4" x 8'.
fn one_by_eight() -> Board {
    Board::new(Length::from_inches(96), inches(7, 1, 4), inches(0, 3, 4))
}

/// A 10" blade.
fn blade(kerf: Length) -> Blade {
    Blade {
        diameter: Length::from_inches(10),
        kerf,
    }
}

/// A table saw with its blade 1" up, high enough to cut through 3/4" stock.
fn table_saw(blade: Blade, fence: Length) -> TableSaw {
    TableSaw {
        blade,
        fence,
        height: Length::from_inches(1),
        miter_gauge: None,
        gauge_angle: woodsim::Angle::ZERO,
        tilt: woodsim::Angle::ZERO,
    }
}

fn miter_saw() -> MiterSaw {
    MiterSaw::twelve_inch()
}

/// Rips `board` all the way through, expecting it to come apart.
fn rip(saw: TableSaw, board: &Board) -> Cut {
    match saw.rip(board, Orientation::default()) {
        Ok(Pass::Cut(cut)) => cut,
        other => panic!("expected the board to be cut apart, got {other:?}"),
    }
}

#[test]
fn rip_leaves_the_fence_width_and_loses_a_kerf() {
    let cut = rip(
        table_saw(blade(Blade::FULL_KERF), Length::from_inches(6)),
        &one_by_eight(),
    );

    assert_eq!(
        cut.piece.board,
        Board::new(
            Length::from_inches(96),
            Length::from_inches(6),
            inches(0, 3, 4)
        )
    );
    // The piece was the 6" beside the fence; the offcut the 1-1/8" at the other edge.
    assert_eq!(cut.piece.w, inches(1, 1, 4));
    let offcut = cut.offcut.unwrap();
    assert_eq!(
        (offcut.board.width(), offcut.w),
        (inches(1, 1, 8), Length::ZERO)
    );
    assert_eq!(cut.waste, inches(0, 1, 8));
    assert!(cut.loose.is_empty());
}

#[test]
fn thin_kerf_blade_leaves_a_wider_offcut() {
    let cut = rip(
        table_saw(blade(Blade::THIN_KERF), Length::from_inches(6)),
        &one_by_eight(),
    );

    assert_eq!(cut.piece.board.width(), Length::from_inches(6));
    assert_eq!(cut.offcut.unwrap().board.width(), inches(1, 5, 32));
    assert_eq!(cut.waste, inches(0, 3, 32));
}

#[test]
fn ripping_within_a_kerf_of_the_edge_turns_the_rest_to_sawdust() {
    let cut = rip(
        table_saw(blade(Blade::FULL_KERF), inches(7, 3, 16)),
        &one_by_eight(),
    );

    assert_eq!(cut.piece.board.width(), inches(7, 3, 16));
    assert_eq!(cut.offcut, None);
    assert_eq!(cut.waste, inches(0, 1, 16));
}

#[test]
fn fence_at_or_past_the_board_width_misses_the_board() {
    for fence in [inches(7, 1, 4), Length::from_inches(8)] {
        assert_eq!(
            table_saw(blade(Blade::FULL_KERF), fence).rip(&one_by_eight(), Orientation::default()),
            Err(CutError::MissesBoard {
                setting: fence,
                available: inches(7, 1, 4)
            })
        );
    }
}

#[test]
fn fence_at_zero_trims_a_kerf_off_the_edge_against_it() {
    let cut = rip(
        table_saw(blade(Blade::FULL_KERF), Length::ZERO),
        &one_by_eight(),
    );
    assert_eq!(
        cut.piece.board,
        Board::new(
            Length::from_inches(96),
            inches(7, 1, 4) - Blade::FULL_KERF,
            inches(0, 3, 4)
        )
    );
    assert_eq!(cut.offcut, None);
    assert_eq!(cut.waste, Blade::FULL_KERF);
    // The kerf came off the edge against the fence, so the rest is where it was.
    assert_eq!(
        (cut.piece.l, cut.piece.w, cut.piece.t),
        (Length::ZERO, Length::ZERO, Length::ZERO)
    );
    // It was on the blade's far side: nothing was against the fence to measure.
    assert_eq!(cut.piece.board.origin().side(), Some(Side::Offcut));
    assert_eq!(cut.piece.board.origin().replay(), cut.piece.board);
}

#[test]
fn a_low_blade_at_the_fence_cuts_a_rabbet_in_the_edge_against_it() {
    let board = one_by_eight();
    let saw = TableSaw {
        height: inches(0, 3, 8),
        ..table_saw(blade(Blade::FULL_KERF), Length::ZERO)
    };
    let Ok(Pass::Kerfed(rabbeted)) = saw.rip(&board, Orientation::default()) else {
        panic!("expected a rabbet");
    };
    assert_eq!(
        (rabbeted.length(), rabbeted.width(), rabbeted.thickness()),
        (board.length(), board.width(), board.thickness())
    );
    let [kerf] = rabbeted.kerfs() else {
        panic!("expected one slot");
    };
    assert_eq!(
        (kerf.near, kerf.far),
        (board.width() - Blade::FULL_KERF, board.width())
    );
    let slot = rabbeted.slot(kerf);
    assert_eq!(
        (slot.width, slot.depth),
        (Blade::FULL_KERF, inches(0, 3, 8))
    );
}

#[test]
fn a_fence_past_the_blade_is_rejected() {
    let fence = -inches(0, 1, 16);
    assert_eq!(
        table_saw(blade(Blade::FULL_KERF), fence).rip(&one_by_eight(), Orientation::default()),
        Err(CutError::NegativeSetting(fence))
    );
}

#[test]
fn a_strip_no_wider_than_the_kerf_against_the_fence_at_zero_is_all_sawdust() {
    let strip = Board::new(Length::from_inches(24), Blade::FULL_KERF, inches(0, 3, 4));
    assert_eq!(
        table_saw(blade(Blade::FULL_KERF), Length::ZERO).rip(&strip, Orientation::default()),
        Err(CutError::AllSawdust)
    );
}

#[test]
fn crosscut_leaves_the_measured_length_and_loses_a_kerf() {
    let cut = miter_saw()
        .crosscut(
            &one_by_eight(),
            Orientation::default(),
            Length::from_inches(24),
        )
        .cut_apart();

    assert_eq!(cut.piece.board.length(), Length::from_inches(24));
    assert_eq!(cut.piece.board.width(), inches(7, 1, 4));
    assert_eq!(cut.offcut.unwrap().board.length(), inches(71, 7, 8));
    assert_eq!(cut.waste, inches(0, 1, 8));
}

#[test]
fn rip_to_width_then_crosscut_to_length() {
    let ripped = rip(
        table_saw(blade(Blade::FULL_KERF), Length::from_inches(6)),
        &one_by_eight(),
    );
    let cut = miter_saw()
        .crosscut(
            &ripped.piece.board,
            Orientation::default(),
            Length::from_inches(24),
        )
        .cut_apart();

    assert_eq!(cut.piece.board.to_string(), "3/4\" x 6\" x 24\"");
}

#[test]
fn kerf_means_an_eight_foot_board_does_not_make_four_two_foot_pieces() {
    let saw = miter_saw();
    let mut rest = one_by_eight();
    for _ in 0..3 {
        let cut = saw
            .crosscut(&rest, Orientation::default(), Length::from_inches(24))
            .cut_apart();
        assert_eq!(cut.piece.board.length(), Length::from_inches(24));
        rest = cut.offcut.unwrap().board;
    }

    assert_eq!(rest.length(), inches(23, 5, 8));
}
