use woodsim::{Blade, Board, Cut, CutError, Length, MiterSaw, Pass, TableSaw};

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
    }
}

fn miter_saw() -> MiterSaw {
    MiterSaw {
        blade: Blade {
            diameter: Length::from_inches(12),
            kerf: Blade::FULL_KERF,
        },
    }
}

/// Rips `board` all the way through, expecting it to come apart.
fn rip(saw: TableSaw, board: &Board) -> Cut {
    match saw.rip(board) {
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
        cut.piece,
        Board::new(
            Length::from_inches(96),
            Length::from_inches(6),
            inches(0, 3, 4)
        )
    );
    assert_eq!(cut.offcut.unwrap().width(), inches(1, 1, 8));
    assert_eq!(cut.waste, inches(0, 1, 8));
}

#[test]
fn thin_kerf_blade_leaves_a_wider_offcut() {
    let cut = rip(
        table_saw(blade(Blade::THIN_KERF), Length::from_inches(6)),
        &one_by_eight(),
    );

    assert_eq!(cut.piece.width(), Length::from_inches(6));
    assert_eq!(cut.offcut.unwrap().width(), inches(1, 5, 32));
    assert_eq!(cut.waste, inches(0, 3, 32));
}

#[test]
fn ripping_within_a_kerf_of_the_edge_turns_the_rest_to_sawdust() {
    let cut = rip(
        table_saw(blade(Blade::FULL_KERF), inches(7, 3, 16)),
        &one_by_eight(),
    );

    assert_eq!(cut.piece.width(), inches(7, 3, 16));
    assert_eq!(cut.offcut, None);
    assert_eq!(cut.waste, inches(0, 1, 16));
}

#[test]
fn fence_at_or_past_the_board_width_misses_the_board() {
    for fence in [inches(7, 1, 4), Length::from_inches(8)] {
        assert_eq!(
            table_saw(blade(Blade::FULL_KERF), fence).rip(&one_by_eight()),
            Err(CutError::MissesBoard {
                setting: fence,
                available: inches(7, 1, 4)
            })
        );
    }
}

#[test]
fn fence_at_zero_is_rejected() {
    assert_eq!(
        table_saw(blade(Blade::FULL_KERF), Length::ZERO).rip(&one_by_eight()),
        Err(CutError::NonPositiveSetting(Length::ZERO))
    );
}

#[test]
fn crosscut_leaves_the_measured_length_and_loses_a_kerf() {
    let cut = miter_saw()
        .crosscut(&one_by_eight(), Length::from_inches(24))
        .unwrap();

    assert_eq!(cut.piece.length(), Length::from_inches(24));
    assert_eq!(cut.piece.width(), inches(7, 1, 4));
    assert_eq!(cut.offcut.unwrap().length(), inches(71, 7, 8));
    assert_eq!(cut.waste, inches(0, 1, 8));
}

#[test]
fn rip_to_width_then_crosscut_to_length() {
    let ripped = rip(
        table_saw(blade(Blade::FULL_KERF), Length::from_inches(6)),
        &one_by_eight(),
    );
    let cut = miter_saw()
        .crosscut(&ripped.piece, Length::from_inches(24))
        .unwrap();

    assert_eq!(cut.piece.to_string(), "3/4\" x 6\" x 24\"");
}

#[test]
fn kerf_means_an_eight_foot_board_does_not_make_four_two_foot_pieces() {
    let saw = miter_saw();
    let mut rest = one_by_eight();
    for _ in 0..3 {
        let cut = saw.crosscut(&rest, Length::from_inches(24)).unwrap();
        assert_eq!(cut.piece.length(), Length::from_inches(24));
        rest = cut.offcut.unwrap();
    }

    assert_eq!(rest.length(), inches(23, 5, 8));
}
